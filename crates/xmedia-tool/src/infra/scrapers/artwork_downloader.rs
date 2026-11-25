use async_trait::async_trait;
use xmedia_core::domain::error::{DomainError, DomainResult};
use xmedia_core::infra::thumbnail::sniff_raster_image_mime;

use crate::domain::config::ProviderCredentials;
use crate::domain::ports::ScraperTransportPort;

use super::http_client::{
    acquire_provider_rate_slot, build_ssrf_safe_http_client, validate_public_http_url_async,
    validate_target_url_ssrf,
};

const MAX_IMAGE_DOWNLOAD_BYTES: usize = 16 * 1024 * 1024;

#[derive(Debug, Default, Clone, Copy)]
pub struct ReqwestScraperTransport;

#[async_trait]
impl ScraperTransportPort for ReqwestScraperTransport {
    async fn acquire_rate_slot(&self, provider_id: &str) {
        acquire_provider_rate_slot(provider_id).await;
    }

    async fn validate_public_http_url(&self, raw_url: &str) -> DomainResult<String> {
        let parsed = validate_public_http_url_async(raw_url).await?;
        Ok(parsed.into())
    }

    async fn download_artwork(
        &self,
        creds: &ProviderCredentials,
        url: &str,
        allow_private_ips: bool,
    ) -> DomainResult<(Vec<u8>, String)> {
        let client = build_ssrf_safe_http_client(creds, allow_private_ips)?;
        download_image_bytes(&client, url, allow_private_ips).await
    }
}

pub async fn download_image_bytes(
    client: &reqwest::Client,
    url: &str,
    allow_private_ips: bool,
) -> DomainResult<(Vec<u8>, String)> {
    let validated = validate_target_url_ssrf(url, allow_private_ips).await?;
    let url_str = validated.as_str();
    let mut req = client.get(validated.clone());
    if url_str.contains(".doubanio.com/") {
        req = req.header(reqwest::header::REFERER, "https://book.douban.com/");
    } else if url_str.contains(".xinhuashudian.com/") || url_str.contains("://xinhuashudian.com/") {
        req = req.header(reqwest::header::REFERER, "https://www.xhsd.com/");
    } else if url_str.contains(".nhentai.net/") || url_str.contains("://nhentai.net/") {
        req = req.header(reqwest::header::REFERER, "https://nhentai.net/");
    } else if url_str.contains("hanime1.me/")
        || url_str.contains(".hembed.com/")
        || url_str.contains(".hanime1.com/")
    {
        req = req.header(reqwest::header::REFERER, "https://hanime1.me/");
    }
    let mut resp = req
        .send()
        .await
        .map_err(|e| DomainError::Storage(format!("failed downloading image '{url}': {e}")))?;
    if !resp.status().is_success() {
        return Err(DomainError::Storage(format!(
            "image download returned HTTP {}",
            resp.status()
        )));
    }
    if let Some(content_len) = resp.content_length()
        && content_len > MAX_IMAGE_DOWNLOAD_BYTES as u64
    {
        return Err(DomainError::Validation(
            "image download exceeds 16MB size limit".into(),
        ));
    }
    let header_mime = resp
        .headers()
        .get(reqwest::header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .map(|s| s.split(';').next().unwrap_or(s).trim().to_ascii_lowercase());
    if let Some(ref ct) = header_mime
        && !ct.is_empty()
        && ct != "application/octet-stream"
        && ct != "binary/octet-stream"
        && (!ct.starts_with("image/") || ct.contains("svg"))
    {
        return Err(DomainError::Validation(format!(
            "unsupported image content type '{ct}'"
        )));
    }
    let mut raw = Vec::new();
    while let Some(chunk) = resp
        .chunk()
        .await
        .map_err(|e| DomainError::Storage(format!("failed reading image body: {e}")))?
    {
        if raw.len().saturating_add(chunk.len()) > MAX_IMAGE_DOWNLOAD_BYTES {
            return Err(DomainError::Validation(
                "image download exceeds 16MB size limit".into(),
            ));
        }
        raw.extend_from_slice(&chunk);
    }
    let sniffed = sniff_raster_image_mime(&raw).ok_or_else(|| {
        DomainError::Validation(
            "downloaded artwork is not a valid raster image (SVG/HTML/unknown rejected)".into(),
        )
    })?;
    Ok((raw, sniffed.to_string()))
}

#[cfg(test)]
mod tests {
    use crate::infra::scrapers::http_client::validate_public_http_url;

    #[test]
    fn validate_public_http_url_blocks_private_cgnat_and_ipv6_ranges() {
        for blocked in [
            "http://127.0.0.1/a.jpg",
            "http://localhost/a.jpg",
            "http://foo.local/a.jpg",
            "http://10.0.0.1/a.jpg",
            "http://172.16.0.1/a.jpg",
            "http://172.31.255.1/a.jpg",
            "http://192.168.0.1/a.jpg",
            "http://169.254.169.254/latest/meta-data/",
            "http://100.64.0.1/a.jpg",
            "http://0.0.0.0/a.jpg",
            "http://[::1]/a.jpg",
            "http://[fc00::1]/a.jpg",
            "http://[fe80::1]/a.jpg",
            "http://[::ffff:127.0.0.1]/a.jpg",
            "http://[::ffff:172.16.1.1]/a.jpg",
            "file:///etc/passwd",
            "https://user:pass@1.1.1.1/a.jpg",
        ] {
            assert!(
                validate_public_http_url(blocked).is_err(),
                "expected {blocked} to be rejected"
            );
        }
        assert!(validate_public_http_url("https://1.1.1.1/cover.jpg").is_ok());
    }
}

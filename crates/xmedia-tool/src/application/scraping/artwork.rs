use xmedia_core::domain::error::{DomainError, DomainResult};
use xmedia_core::domain::library::MediaKind;
use xmedia_core::infra::thumbnail::sniff_raster_image_mime;

use super::ScrapingOrchestratorService;
use crate::domain::media::MediaFileDetail;
use crate::domain::workspace::RootId;
use crate::infra::scrapers::http_client::{
    build_ssrf_safe_http_client, validate_artwork_http_url, validate_public_http_url,
};

const MAX_IMAGE_DOWNLOAD_BYTES: usize = 16 * 1024 * 1024;

impl ScrapingOrchestratorService {
    pub async fn proxy_scraper_image(&self, raw_url: &str) -> DomainResult<(Vec<u8>, String)> {
        let parsed = reqwest::Url::parse(raw_url.trim())
            .map_err(|_| DomainError::Validation("invalid image URL".into()))?;
        if parsed.scheme() != "https" || !parsed.username().is_empty() || parsed.port().is_some() {
            return Err(DomainError::Validation(
                "disallowed image URL scheme or port".into(),
            ));
        }
        let host = parsed.host_str().unwrap_or_default().to_ascii_lowercase();
        let allowed_domain = host.ends_with(".doubanio.com")
            || host == "xinhuashudian.com"
            || host.ends_with(".xinhuashudian.com")
            || host == "nhentai.net"
            || host.ends_with(".nhentai.net")
            || host == "hanime1.me"
            || host.ends_with(".hanime1.me")
            || host == "hembed.com"
            || host.ends_with(".hembed.com")
            || host.ends_with(".hanime1.com");
        let allowed = allowed_domain
            && host
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'.' || b == b'-');
        if !allowed {
            return Err(DomainError::Validation(format!(
                "host '{host}' is not on the scraper image proxy allowlist"
            )));
        }
        let creds = self.settings.get_credentials().await?;
        let client = build_ssrf_safe_http_client(&creds, false)?;
        download_image_bytes(&client, parsed.as_str(), false).await
    }

    /// Downloads a remote artwork image (e.g. from a scraper candidate) using configured proxy/UA
    /// and writes it into the target media file via the workbench.
    pub async fn apply_remote_artwork(
        &self,
        root_id: RootId,
        rel_path: &str,
        raw_url: &str,
        slot: &str,
        preferred_kind: Option<MediaKind>,
    ) -> DomainResult<MediaFileDetail> {
        let parsed = validate_public_http_url(raw_url)?;
        let creds = self.settings.get_credentials().await?;
        let client = build_ssrf_safe_http_client(&creds, false)?;
        let (bytes, mime) = download_image_bytes(&client, parsed.as_str(), false).await?;
        self.workbench
            .write_artwork(root_id, rel_path, &bytes, &mime, slot, preferred_kind)
            .await
    }
}

pub(super) async fn download_image_bytes(
    client: &reqwest::Client,
    url: &str,
    allow_private_ips: bool,
) -> DomainResult<(Vec<u8>, String)> {
    let validated = validate_artwork_http_url(url, allow_private_ips)?;
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

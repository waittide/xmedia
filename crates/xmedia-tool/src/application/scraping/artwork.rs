use xmedia_core::domain::error::{DomainError, DomainResult};
use xmedia_core::domain::library::MediaKind;

use super::ScrapingOrchestratorService;
use crate::domain::media::MediaFileDetail;
use crate::domain::workspace::RootId;

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
        self.transport
            .download_artwork(&creds, parsed.as_str(), false)
            .await
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
        let validated = self.transport.validate_public_http_url(raw_url).await?;
        let creds = self.settings.get_credentials().await?;
        let (bytes, mime) = self
            .transport
            .download_artwork(&creds, &validated, false)
            .await?;
        self.workbench
            .write_artwork(root_id, rel_path, &bytes, &mime, slot, preferred_kind)
            .await
    }
}

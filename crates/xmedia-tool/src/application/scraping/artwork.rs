use xmedia_core::domain::error::{DomainError, DomainResult};
use xmedia_core::domain::library::MediaKind;

use super::{ScrapeApplyService, ScraperCatalogService};
use crate::application::ports::ArtworkIo;
use crate::application::workbench::WorkspaceArtworkOutput;
use crate::domain::media::MediaFileDetail;
use crate::domain::workspace::RootId;

impl ScraperCatalogService {
    pub async fn proxy_image(&self, raw_url: &str) -> DomainResult<(Vec<u8>, String)> {
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
        let creds = self.settings.get_raw().await?.credentials;
        self.transport
            .download_artwork(&creds, parsed.as_str(), false)
            .await
    }
}

impl ScrapeApplyService {
    pub(super) async fn write_file_artwork(
        &self,
        root_id: RootId,
        rel_path: &str,
        preferred_kind: Option<MediaKind>,
        slot: &str,
        bytes: &[u8],
        mime_type: &str,
    ) -> DomainResult<MediaFileDetail> {
        match self
            .workspace
            .artwork(
                root_id,
                rel_path,
                preferred_kind,
                ArtworkIo::Write {
                    slot,
                    bytes,
                    mime_type,
                },
            )
            .await?
        {
            WorkspaceArtworkOutput::Written(detail) => Ok(*detail),
            WorkspaceArtworkOutput::Read(_) => {
                Err(DomainError::Storage("unexpected artwork output".into()))
            }
        }
    }

    pub async fn apply_remote_artwork(
        &self,
        root_id: RootId,
        rel_path: &str,
        raw_url: &str,
        slot: &str,
        preferred_kind: Option<MediaKind>,
    ) -> DomainResult<MediaFileDetail> {
        let creds = self.settings.get_raw().await?.credentials;
        let (bytes, mime) = self
            .transport
            .download_artwork(&creds, raw_url, false)
            .await?;
        self.write_file_artwork(root_id, rel_path, preferred_kind, slot, &bytes, &mime)
            .await
    }
}

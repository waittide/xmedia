use std::path::PathBuf;
use std::sync::Arc;

use serde::{Deserialize, Serialize};
use xmedia_core::domain::error::{DomainError, DomainResult};
use xmedia_core::domain::library::MediaKind;

use crate::domain::config::{ProviderCredentials, RenameTemplates, ToolSettings};
use crate::domain::ports::ConfigRepositoryPort;
use crate::domain::workspace::{ManagedRoot, RootId};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PublicSettingsDto {
    pub roots: Vec<ManagedRoot>,
    pub has_tmdb_api_key: bool,
    pub tmdb_api_key_masked: Option<String>,
    pub has_bangumi_token: bool,
    pub bangumi_token_masked: Option<String>,
    pub http_proxy: Option<String>,
    pub rename_templates: RenameTemplates,
    pub default_scrape_locale: String,
}

#[derive(Debug, Clone, Deserialize, Default)]
pub struct UpdateSettingsInput {
    pub tmdb_api_key: Option<String>,
    pub bangumi_access_token: Option<String>,
    pub http_proxy: Option<String>,
    pub rename_templates: Option<RenameTemplates>,
    pub default_scrape_locale: Option<String>,
}

#[derive(Clone)]
pub struct ToolSettingsService {
    repo: Arc<dyn ConfigRepositoryPort>,
}

impl ToolSettingsService {
    pub fn new(repo: Arc<dyn ConfigRepositoryPort>) -> Self {
        Self { repo }
    }

    pub async fn get_raw(&self) -> DomainResult<ToolSettings> {
        self.repo.load().await
    }

    pub async fn get_credentials(&self) -> DomainResult<ProviderCredentials> {
        Ok(self.repo.load().await?.credentials)
    }

    pub async fn get_public_settings(&self) -> DomainResult<PublicSettingsDto> {
        let s = self.repo.load().await?;
        Ok(Self::to_public(s))
    }

    pub async fn find_root(&self, root_id: RootId) -> DomainResult<ManagedRoot> {
        let s = self.repo.load().await?;
        s.roots
            .into_iter()
            .find(|r| r.id == root_id)
            .ok_or_else(|| DomainError::NotFound(format!("root '{root_id}' not found")))
    }

    pub async fn add_root(
        &self,
        name: String,
        path: String,
        default_kind: Option<MediaKind>,
    ) -> DomainResult<ManagedRoot> {
        let root = ManagedRoot::new(name, PathBuf::from(path.trim()), default_kind)?;
        let mut settings = self.repo.load().await?;
        if let Some(existing) = settings
            .roots
            .iter()
            .find(|r| r.path == root.path && r.default_kind == root.default_kind)
        {
            return Ok(existing.clone());
        }
        settings.roots.push(root.clone());
        self.repo.save(&settings).await?;
        Ok(root)
    }

    pub async fn remove_root(&self, root_id: RootId) -> DomainResult<()> {
        let mut settings = self.repo.load().await?;
        let before = settings.roots.len();
        settings.roots.retain(|r| r.id != root_id);
        if settings.roots.len() == before {
            return Err(DomainError::NotFound(format!("root '{root_id}' not found")));
        }
        self.repo.save(&settings).await
    }

    pub async fn update_settings(
        &self,
        input: UpdateSettingsInput,
    ) -> DomainResult<PublicSettingsDto> {
        let mut settings = self.repo.load().await?;
        if let Some(key) = input.tmdb_api_key {
            let trimmed = key.trim();
            settings.credentials.tmdb_api_key = (!trimmed.is_empty()).then(|| trimmed.to_string());
        }
        if let Some(tok) = input.bangumi_access_token {
            let trimmed = tok.trim();
            settings.credentials.bangumi_access_token =
                (!trimmed.is_empty()).then(|| trimmed.to_string());
        }
        if let Some(proxy) = input.http_proxy {
            let trimmed = proxy.trim();
            settings.credentials.http_proxy = (!trimmed.is_empty()).then(|| trimmed.to_string());
        }
        if let Some(tpl) = input.rename_templates {
            settings.rename_templates = tpl;
        }
        if let Some(loc) = input.default_scrape_locale
            && !loc.trim().is_empty()
        {
            settings.default_scrape_locale = loc.trim().to_string();
        }
        self.repo.save(&settings).await?;
        Ok(Self::to_public(settings))
    }

    fn to_public(s: ToolSettings) -> PublicSettingsDto {
        PublicSettingsDto {
            roots: s.roots,
            has_tmdb_api_key: s.credentials.has_tmdb_key(),
            tmdb_api_key_masked: mask_secret(s.credentials.tmdb_api_key.as_deref()),
            has_bangumi_token: s
                .credentials
                .bangumi_access_token
                .as_deref()
                .is_some_and(|t| !t.trim().is_empty()),
            bangumi_token_masked: mask_secret(s.credentials.bangumi_access_token.as_deref()),
            http_proxy: s.credentials.http_proxy,
            rename_templates: s.rename_templates,
            default_scrape_locale: s.default_scrape_locale,
        }
    }
}

fn mask_secret(raw: Option<&str>) -> Option<String> {
    let val = raw?.trim();
    if val.is_empty() {
        return None;
    }
    if val.len() <= 8 {
        return Some("••••••••".to_string());
    }
    Some(format!("{}••••{}", &val[..4], &val[val.len() - 4..]))
}

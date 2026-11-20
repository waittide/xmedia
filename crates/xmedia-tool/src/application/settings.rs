use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::Arc;

use serde::{Deserialize, Serialize};
use xmedia_core::domain::error::{DomainError, DomainResult};
use xmedia_core::domain::library::MediaKind;

use crate::domain::config::{
    InboxConfig, ProviderConfigOverride, ProviderCredentials, RenameTemplates, ScraperPriorities,
    ToolSettings,
};
use crate::domain::ports::ConfigRepositoryPort;
use crate::domain::workspace::{ManagedRoot, RootId};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PublicSettingsDto {
    pub roots: Vec<ManagedRoot>,
    pub has_tmdb_api_key: bool,
    pub tmdb_api_key_masked: Option<String>,
    pub has_bangumi_token: bool,
    pub bangumi_token_masked: Option<String>,
    pub has_qishui_cookie: bool,
    pub qishui_cookie_masked: Option<String>,
    pub http_proxy: Option<String>,
    pub enable_adult_scrapers: bool,
    pub rename_templates: RenameTemplates,
    pub scraper_priorities: ScraperPriorities,
    pub provider_configs: BTreeMap<String, ProviderConfigOverride>,
    pub default_scrape_locale: String,
    pub scrape_cache_ttl_ms: u64,
    pub inbox: InboxConfig,
    pub xmedia_webhook_enabled: bool,
    pub xmedia_webhook_base_url: String,
    pub has_xmedia_webhook_api_key: bool,
    pub xmedia_webhook_api_key_masked: Option<String>,
    pub xmedia_webhook_notify_on_rename: bool,
    pub xmedia_webhook_notify_on_scrape: bool,
    pub xmedia_webhook_notify_on_inbox: bool,
}

#[derive(Debug, Clone, Deserialize, Default)]
pub struct UpdateSettingsInput {
    pub tmdb_api_key: Option<String>,
    pub bangumi_access_token: Option<String>,
    pub qishui_cookie: Option<String>,
    pub http_proxy: Option<String>,
    pub enable_adult_scrapers: Option<bool>,
    pub rename_templates: Option<RenameTemplates>,
    pub scraper_priorities: Option<ScraperPriorities>,
    pub provider_configs: Option<BTreeMap<String, ProviderConfigOverride>>,
    pub default_scrape_locale: Option<String>,
    pub scrape_cache_ttl_ms: Option<u64>,
    pub inbox: Option<InboxConfig>,
    pub xmedia_webhook_enabled: Option<bool>,
    pub xmedia_webhook_base_url: Option<String>,
    pub xmedia_webhook_api_key: Option<String>,
    pub xmedia_webhook_notify_on_rename: Option<bool>,
    pub xmedia_webhook_notify_on_scrape: Option<bool>,
    pub xmedia_webhook_notify_on_inbox: Option<bool>,
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
        self.add_root_with_options(name, path, default_kind, false)
            .await
    }

    pub async fn add_root_with_options(
        &self,
        name: String,
        path: String,
        default_kind: Option<MediaKind>,
        read_only: bool,
    ) -> DomainResult<ManagedRoot> {
        let root = ManagedRoot::new(name, PathBuf::from(path.trim()), default_kind)?
            .with_read_only(read_only);
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

    pub async fn set_root_read_only(
        &self,
        root_id: RootId,
        read_only: bool,
    ) -> DomainResult<ManagedRoot> {
        let mut settings = self.repo.load().await?;
        let target = settings
            .roots
            .iter_mut()
            .find(|r| r.id == root_id)
            .ok_or_else(|| DomainError::NotFound(format!("root '{root_id}' not found")))?;
        target.read_only = read_only;
        let updated = target.clone();
        self.repo.save(&settings).await?;
        Ok(updated)
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
        if let Some(cookie) = input.qishui_cookie {
            let trimmed = cookie.trim();
            if trimmed.is_empty() {
                settings.credentials.extra.remove("qishui_cookie");
            } else {
                settings
                    .credentials
                    .extra
                    .insert("qishui_cookie".to_string(), trimmed.to_string());
            }
        }
        if let Some(proxy) = input.http_proxy {
            let trimmed = proxy.trim();
            settings.credentials.http_proxy = (!trimmed.is_empty()).then(|| trimmed.to_string());
        }
        if let Some(enable_adult) = input.enable_adult_scrapers {
            settings.credentials.enable_adult_scrapers = enable_adult;
        }
        if let Some(tpl) = input.rename_templates {
            settings.rename_templates = tpl;
        }
        if let Some(prio) = input.scraper_priorities {
            settings.scraper_priorities = prio;
        }
        if let Some(cfgs) = input.provider_configs {
            for (provider_id, raw_cfg) in cfgs {
                let id = provider_id.trim().to_string();
                if id.is_empty() {
                    continue;
                }
                let normalized = raw_cfg.normalized();
                if normalized.is_empty() {
                    settings.provider_configs.remove(&id);
                } else {
                    settings.provider_configs.insert(id, normalized);
                }
            }
        }
        if let Some(loc) = input.default_scrape_locale
            && !loc.trim().is_empty()
        {
            settings.default_scrape_locale = loc.trim().to_string();
        }
        if let Some(ttl_ms) = input.scrape_cache_ttl_ms {
            settings.scrape_cache_ttl_ms = ttl_ms;
        }
        if let Some(inbox) = input.inbox {
            settings.inbox = inbox;
        }
        if let Some(enabled) = input.xmedia_webhook_enabled {
            settings.xmedia_webhook.enabled = enabled;
        }
        if let Some(base_url) = input.xmedia_webhook_base_url {
            settings.xmedia_webhook.base_url = base_url.trim().to_string();
        }
        if let Some(api_key) = input.xmedia_webhook_api_key {
            let trimmed = api_key.trim();
            settings.xmedia_webhook.api_key = (!trimmed.is_empty()).then(|| trimmed.to_string());
        }
        if let Some(v) = input.xmedia_webhook_notify_on_rename {
            settings.xmedia_webhook.notify_on_rename = v;
        }
        if let Some(v) = input.xmedia_webhook_notify_on_scrape {
            settings.xmedia_webhook.notify_on_scrape = v;
        }
        if let Some(v) = input.xmedia_webhook_notify_on_inbox {
            settings.xmedia_webhook.notify_on_inbox = v;
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
            has_qishui_cookie: s.credentials.get_extra("qishui_cookie").is_some(),
            qishui_cookie_masked: mask_secret(s.credentials.get_extra("qishui_cookie")),
            http_proxy: s.credentials.http_proxy,
            enable_adult_scrapers: s.credentials.enable_adult_scrapers,
            rename_templates: s.rename_templates,
            scraper_priorities: s.scraper_priorities,
            provider_configs: s.provider_configs,
            default_scrape_locale: s.default_scrape_locale,
            scrape_cache_ttl_ms: s.scrape_cache_ttl_ms,
            inbox: s.inbox,
            xmedia_webhook_enabled: s.xmedia_webhook.enabled,
            xmedia_webhook_base_url: s.xmedia_webhook.base_url,
            has_xmedia_webhook_api_key: s
                .xmedia_webhook
                .api_key
                .as_deref()
                .is_some_and(|k| !k.trim().is_empty()),
            xmedia_webhook_api_key_masked: mask_secret(s.xmedia_webhook.api_key.as_deref()),
            xmedia_webhook_notify_on_rename: s.xmedia_webhook.notify_on_rename,
            xmedia_webhook_notify_on_scrape: s.xmedia_webhook.notify_on_scrape,
            xmedia_webhook_notify_on_inbox: s.xmedia_webhook.notify_on_inbox,
        }
    }
}

fn mask_secret(raw: Option<&str>) -> Option<String> {
    let val = raw?.trim();
    if val.is_empty() {
        return None;
    }
    if val.chars().count() <= 8 {
        return Some("••••••••".to_string());
    }
    let prefix: String = val.chars().take(4).collect();
    let suffix: String = val
        .chars()
        .rev()
        .take(4)
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect();
    Some(format!("{prefix}••••{suffix}"))
}

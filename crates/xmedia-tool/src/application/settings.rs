use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::Arc;

use serde::{Deserialize, Serialize};
use xmedia_core::domain::error::{DomainError, DomainResult};
use xmedia_core::domain::library::MediaKind;

use crate::application::ports::{ConfigRepositoryPort, WorkspaceFsPort};
use crate::domain::config::{
    InboxConfig, ProviderConfigOverride, ProviderCredentials, RenameTemplates, ScraperPriorities,
    ToolSettings, XmediaWebhookConfig,
};
use crate::domain::workspace::{ManagedRoot, RootId, paths_overlap};

#[derive(Debug, Clone, Default)]
pub struct SettingsSandboxPolicy {
    pub data_dir: Option<PathBuf>,
    pub cli_roots: Vec<PathBuf>,
    pub roots_locked: bool,
}

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
    fs: Arc<dyn WorkspaceFsPort>,
    write_lock: Arc<tokio::sync::Mutex<()>>,
    data_dir: Option<PathBuf>,
    cli_root_prefixes: Arc<Vec<PathBuf>>,
    roots_locked: bool,
}

impl ToolSettingsService {
    pub fn new(
        repo: Arc<dyn ConfigRepositoryPort>,
        fs: Arc<dyn WorkspaceFsPort>,
        policy: SettingsSandboxPolicy,
    ) -> Self {
        let data_dir = policy.data_dir.map(|d| d.canonicalize().unwrap_or(d));
        let canon_cli: Vec<PathBuf> = policy
            .cli_roots
            .iter()
            .filter_map(|p| p.canonicalize().ok())
            .collect();
        Self {
            repo,
            fs,
            write_lock: Arc::new(tokio::sync::Mutex::new(())),
            data_dir,
            cli_root_prefixes: Arc::new(canon_cli),
            roots_locked: policy.roots_locked,
        }
    }

    pub(in crate::application) fn validate_inbox_directory(
        &self,
        raw_inbox: &str,
        roots: &[ManagedRoot],
        target_root_id: Option<RootId>,
    ) -> DomainResult<PathBuf> {
        let inbox_dir = PathBuf::from(raw_inbox.trim())
            .canonicalize()
            .map_err(|e| DomainError::Validation(format!("inbox directory inaccessible: {e}")))?;
        if !inbox_dir.is_dir() {
            return Err(DomainError::Validation(
                "inbox_path is not a directory".into(),
            ));
        }
        self.fs.validate_safe_host_directory(&inbox_dir, "inbox")?;
        if let Some(ref data_dir) = self.data_dir
            && paths_overlap(&inbox_dir, data_dir)
        {
            return Err(DomainError::Validation(
                "inbox directory cannot overlap xmedia-tool data directory".into(),
            ));
        }
        for r in roots {
            if (target_root_id == Some(r.id) || paths_overlap(&inbox_dir, &r.path))
                && paths_overlap(&inbox_dir, &r.path)
            {
                return Err(DomainError::Validation(
                    "inbox directory cannot be inside or contain a managed root directory".into(),
                ));
            }
        }
        Ok(inbox_dir)
    }

    pub async fn get_raw(&self) -> DomainResult<ToolSettings> {
        self.repo.load().await
    }

    pub async fn get_public_settings(&self) -> DomainResult<PublicSettingsDto> {
        let s = self.repo.load().await?;
        Ok(to_public_settings(s))
    }

    pub async fn add_root(
        &self,
        name: String,
        path: String,
        default_kind: Option<MediaKind>,
        read_only: bool,
    ) -> DomainResult<ManagedRoot> {
        if self.roots_locked {
            return Err(DomainError::Forbidden(
                "managed roots are locked by CLI configuration".into(),
            ));
        }
        let mut root =
            self.fs
                .open_managed_root(name, std::path::Path::new(path.trim()), default_kind)?;
        root.read_only = read_only;
        if let Some(ref data_dir) = self.data_dir
            && paths_overlap(&root.path, data_dir)
        {
            return Err(DomainError::Validation(
                "managed root cannot overlap xmedia-tool data directory".into(),
            ));
        }
        if !self.cli_root_prefixes.is_empty()
            && !self
                .cli_root_prefixes
                .iter()
                .any(|allowed| root.path.starts_with(allowed))
        {
            return Err(DomainError::Forbidden(
                "cannot add root outside explicitly configured CLI --root directories".into(),
            ));
        }
        let _guard = self.write_lock.lock().await;
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
        let _guard = self.write_lock.lock().await;
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
        if self.roots_locked {
            return Err(DomainError::Forbidden(
                "managed roots are locked by CLI configuration".into(),
            ));
        }
        let _guard = self.write_lock.lock().await;
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
        mut input: UpdateSettingsInput,
    ) -> DomainResult<PublicSettingsDto> {
        let _guard = self.write_lock.lock().await;
        let mut settings = self.repo.load().await?;
        apply_credentials_update(&mut settings.credentials, &mut input);
        if let Some(tpl) = input.rename_templates.take() {
            settings.rename_templates = tpl;
        }
        if let Some(prio) = input.scraper_priorities.take() {
            settings.scraper_priorities = prio;
        }
        if let Some(cfgs) = input.provider_configs.take() {
            settings.set_provider_overrides(cfgs);
        }
        if let Some(loc) = input.default_scrape_locale.take()
            && !loc.trim().is_empty()
        {
            settings.default_scrape_locale = loc.trim().to_string();
        }
        if let Some(ttl_ms) = input.scrape_cache_ttl_ms {
            settings.scrape_cache_ttl_ms = ttl_ms;
        }
        if let Some(mut inbox) = input.inbox.take() {
            let clean_path = trim_non_empty_opt(inbox.inbox_path);
            if let Some(ref raw_inbox) = clean_path {
                self.validate_inbox_directory(raw_inbox, &settings.roots, inbox.target_root_id)?;
            }
            inbox.inbox_path = clean_path;
            settings.inbox = inbox;
        }
        apply_webhook_update(&mut settings.xmedia_webhook, &mut input);
        self.repo.save(&settings).await?;
        Ok(to_public_settings(settings))
    }
}

fn trim_non_empty_opt(raw: Option<String>) -> Option<String> {
    raw.and_then(|s| {
        let trimmed = s.trim();
        (!trimmed.is_empty()).then(|| trimmed.to_string())
    })
}

fn apply_credentials_update(creds: &mut ProviderCredentials, input: &mut UpdateSettingsInput) {
    if let Some(key) = input.tmdb_api_key.take() {
        creds.tmdb_api_key = trim_non_empty_opt(Some(key));
    }
    if let Some(tok) = input.bangumi_access_token.take() {
        creds.bangumi_access_token = trim_non_empty_opt(Some(tok));
    }
    if let Some(cookie) = input.qishui_cookie.take() {
        match trim_non_empty_opt(Some(cookie)) {
            Some(c) => {
                creds.extra.insert("qishui_cookie".to_string(), c);
            }
            None => {
                creds.extra.remove("qishui_cookie");
            }
        }
    }
    if let Some(proxy) = input.http_proxy.take() {
        creds.http_proxy = trim_non_empty_opt(Some(proxy));
    }
    if let Some(enable_adult) = input.enable_adult_scrapers {
        creds.enable_adult_scrapers = enable_adult;
    }
}

fn apply_webhook_update(webhook: &mut XmediaWebhookConfig, input: &mut UpdateSettingsInput) {
    if let Some(enabled) = input.xmedia_webhook_enabled {
        webhook.enabled = enabled;
    }
    if let Some(base_url) = input.xmedia_webhook_base_url.take() {
        webhook.base_url = base_url.trim().to_string();
    }
    if let Some(api_key) = input.xmedia_webhook_api_key.take() {
        webhook.api_key = trim_non_empty_opt(Some(api_key));
    }
    if let Some(v) = input.xmedia_webhook_notify_on_rename {
        webhook.notify_on_rename = v;
    }
    if let Some(v) = input.xmedia_webhook_notify_on_scrape {
        webhook.notify_on_scrape = v;
    }
    if let Some(v) = input.xmedia_webhook_notify_on_inbox {
        webhook.notify_on_inbox = v;
    }
}

fn to_public_settings(s: ToolSettings) -> PublicSettingsDto {
    let masked_provider_configs = s
        .provider_configs
        .into_iter()
        .map(|(k, mut cfg)| {
            cfg.http_proxy = mask_proxy_url(cfg.http_proxy.as_deref());
            (k, cfg)
        })
        .collect();
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
        http_proxy: mask_proxy_url(s.credentials.http_proxy.as_deref()),
        enable_adult_scrapers: s.credentials.enable_adult_scrapers,
        rename_templates: s.rename_templates,
        scraper_priorities: s.scraper_priorities,
        provider_configs: masked_provider_configs,
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

fn mask_proxy_url(raw: Option<&str>) -> Option<String> {
    let val = raw?.trim();
    if val.is_empty() {
        return None;
    }
    if let Ok(mut parsed) = reqwest::Url::parse(val)
        && (!parsed.username().is_empty() || parsed.password().is_some())
    {
        let _ = parsed.set_username("***");
        let _ = parsed.set_password(Some("***"));
        return Some(parsed.to_string());
    }
    Some(val.to_string())
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

#[cfg(test)]
mod tests {
    use std::path::Path;

    use async_trait::async_trait;

    use super::*;
    use crate::domain::workspace::SandboxPath;

    #[derive(Default)]
    struct InMemoryConfigRepo(tokio::sync::Mutex<ToolSettings>);

    #[async_trait]
    impl ConfigRepositoryPort for InMemoryConfigRepo {
        async fn load(&self) -> DomainResult<ToolSettings> {
            Ok(self.0.lock().await.clone())
        }

        async fn save(&self, settings: &ToolSettings) -> DomainResult<()> {
            *self.0.lock().await = settings.clone();
            Ok(())
        }
    }

    struct StubWorkspaceFs;

    impl WorkspaceFsPort for StubWorkspaceFs {
        fn validate_safe_host_directory(&self, canonical: &Path, label: &str) -> DomainResult<()> {
            if canonical.parent().is_none() || canonical.starts_with("/etc") {
                return Err(DomainError::Validation(format!("blocked {label}")));
            }
            Ok(())
        }

        fn open_managed_root(
            &self,
            name: String,
            raw_path: &Path,
            default_kind: Option<MediaKind>,
        ) -> DomainResult<ManagedRoot> {
            let canonical = std::fs::canonicalize(raw_path)
                .map_err(|e| DomainError::Validation(e.to_string()))?;
            self.validate_safe_host_directory(&canonical, "root")?;
            Ok(ManagedRoot::from_canonical(name, canonical, default_kind))
        }

        fn resolve_existing_in_root(
            &self,
            root: &ManagedRoot,
            rel_path: &str,
        ) -> DomainResult<SandboxPath> {
            let p = root.path.join(rel_path).canonicalize().unwrap();
            root.verify_canonical_child(p)
        }

        fn resolve_target_in_root(
            &self,
            root: &ManagedRoot,
            rel_path: &str,
        ) -> DomainResult<PathBuf> {
            Ok(root.path.join(rel_path))
        }

        fn find_companion_sidecars(&self, _media_file: &Path) -> Vec<(PathBuf, String)> {
            Vec::new()
        }

        fn cleanup_empty_parent_dirs(&self, _start_dir: &Path, _stop_at_root: &Path) {}
    }

    #[tokio::test]
    async fn concurrent_root_additions_do_not_lose_updates() {
        let tmp = tempfile::tempdir().unwrap();
        let repo = Arc::new(InMemoryConfigRepo::default());
        let svc = ToolSettingsService::new(
            repo,
            Arc::new(StubWorkspaceFs),
            SettingsSandboxPolicy::default(),
        );

        let mut dirs = Vec::new();
        for i in 0..6 {
            let d = tmp.path().join(format!("root_{i}"));
            std::fs::create_dir_all(&d).unwrap();
            dirs.push(d);
        }

        let mut handles = Vec::new();
        for (i, dir) in dirs.into_iter().enumerate() {
            let s = svc.clone();
            handles.push(tokio::spawn(async move {
                s.add_root(
                    format!("Root {i}"),
                    dir.to_string_lossy().into_owned(),
                    Some(MediaKind::Book),
                    false,
                )
                .await
                .unwrap();
            }));
        }
        for h in handles {
            h.await.unwrap();
        }

        let settings = svc.get_raw().await.unwrap();
        assert_eq!(settings.roots.len(), 6);
    }

    #[tokio::test]
    async fn cli_roots_and_inbox_validation_block_system_paths_and_outside_roots() {
        let tmp = tempfile::tempdir().unwrap();
        let data_dir = tmp.path().join("data");
        let cli_root = tmp.path().join("media");
        let sub_root = cli_root.join("movies");
        let outside_dir = tmp.path().join("outside");
        std::fs::create_dir_all(&data_dir).unwrap();
        std::fs::create_dir_all(&sub_root).unwrap();
        std::fs::create_dir_all(&outside_dir).unwrap();

        let repo = Arc::new(InMemoryConfigRepo::default());
        let svc = ToolSettingsService::new(
            repo,
            Arc::new(StubWorkspaceFs),
            SettingsSandboxPolicy {
                data_dir: Some(data_dir.clone()),
                cli_roots: vec![cli_root.clone()],
                roots_locked: false,
            },
        );

        assert!(
            svc.add_root(
                "Movies".into(),
                sub_root.to_string_lossy().into_owned(),
                Some(MediaKind::Video),
                false,
            )
            .await
            .is_ok()
        );
        assert!(matches!(
            svc.add_root(
                "Outside".into(),
                outside_dir.to_string_lossy().into_owned(),
                None,
                false,
            )
            .await,
            Err(DomainError::Forbidden(_))
        ));
        assert!(
            svc.update_settings(UpdateSettingsInput {
                inbox: Some(InboxConfig {
                    enabled: true,
                    inbox_path: Some("/".into()),
                    ..Default::default()
                }),
                ..Default::default()
            })
            .await
            .is_err()
        );
        if std::path::Path::new("/etc").is_dir() {
            assert!(
                svc.update_settings(UpdateSettingsInput {
                    inbox: Some(InboxConfig {
                        enabled: true,
                        inbox_path: Some("/etc".into()),
                        ..Default::default()
                    }),
                    ..Default::default()
                })
                .await
                .is_err()
            );
        }
    }
}

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use anyhow::Context;
use axum::Router;
use axum::http::HeaderValue;
use serde::{Deserialize, Serialize};
use uuid::Uuid;
use xmedia::{XMediaApp, XMediaConfig};
use xmedia_core::application::auth::AuthService;
use xmedia_core::application::dto::{AuthSessionDto, SetupAdminInput};
use xmedia_core::application::library::LibraryService;
use xmedia_core::domain::library::MediaKind;
use xmedia_core::http::AUTH_COOKIE_NAME;
use xmedia_core::infra::events::BroadcastEventHub;

pub const LOCAL_ADMIN_USERNAME: &str = "local";
pub const LOCAL_ADMIN_DISPLAY_NAME: &str = "Local";
pub const LOCAL_JWT_TTL_SECS: u64 = 30 * 86_400;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LocalIdentity {
    pub username: String,
    pub jwt_secret: String,
    pub local_password: String,
}

impl LocalIdentity {
    pub fn load_or_create(path: &Path) -> anyhow::Result<Self> {
        if path.exists() {
            let raw = fs::read(path).with_context(|| {
                format!("failed to read local identity file {}", path.display())
            })?;
            if !raw.iter().all(|b| b.is_ascii_whitespace()) {
                let mut parsed: Self = serde_json::from_slice(&raw).with_context(|| {
                    format!("failed to parse local identity file {}", path.display())
                })?;
                if parsed.username.trim().is_empty() {
                    parsed.username = LOCAL_ADMIN_USERNAME.to_string();
                }
                if !parsed.jwt_secret.trim().is_empty() && !parsed.local_password.trim().is_empty()
                {
                    return Ok(parsed);
                }
            }
        }

        let generated = Self {
            username: LOCAL_ADMIN_USERNAME.to_string(),
            jwt_secret: format!("{}{}", Uuid::new_v4().simple(), Uuid::new_v4().simple()),
            local_password: format!("xm_{}", Uuid::new_v4().simple()),
        };
        persist_identity_atomic(path, &generated)?;
        Ok(generated)
    }
}

#[derive(Clone)]
pub struct LocalEngine {
    pub auth: Arc<AuthService>,
    pub libraries: Arc<LibraryService>,
    pub events: Arc<BroadcastEventHub>,
    pub enabled_modules: Vec<MediaKind>,
    pub data_dir: PathBuf,
    identity: LocalIdentity,
}

impl LocalEngine {
    pub async fn start(
        data_dir: PathBuf,
        external_web_dir: Option<PathBuf>,
    ) -> anyhow::Result<(Self, Router)> {
        fs::create_dir_all(&data_dir).with_context(|| {
            format!(
                "failed to create standalone data directory {}",
                data_dir.display()
            )
        })?;

        let identity_path = data_dir.join("local_identity.json");
        let identity = LocalIdentity::load_or_create(&identity_path)?;

        let db_path = data_dir.join("xmedia.db");
        let ffmpeg_bin = std::env::var("XMEDIA_FFMPEG_BIN")
            .ok()
            .map(|v| v.trim().to_string())
            .filter(|v| !v.is_empty())
            .unwrap_or_else(|| "ffmpeg".to_string());

        let app = XMediaApp::connect_and_init(XMediaConfig {
            data_dir: data_dir.clone(),
            database_url: format!("sqlite:{}", db_path.display()),
            secret: identity.jwt_secret.clone(),
            jwt_ttl_secs: LOCAL_JWT_TTL_SECS,
            ffmpeg_bin,
            external_web_dir,
        })
        .await?;

        let setup_status = app
            .auth
            .setup_status()
            .await
            .map_err(|e| anyhow::anyhow!("failed to inspect local auth setup status: {e}"))?;
        if setup_status.needs_setup {
            app.auth
                .setup_initial_admin(SetupAdminInput {
                    username: identity.username.clone(),
                    display_name: Some(LOCAL_ADMIN_DISPLAY_NAME.to_string()),
                    password: identity.local_password.clone(),
                })
                .await
                .map_err(|e| anyhow::anyhow!("failed to create local admin account: {e}"))?;
            tracing::info!(
                username = %identity.username,
                "initialized standalone local admin account"
            );
        }

        let engine = Self {
            auth: app.auth,
            libraries: app.libraries,
            events: app.events,
            enabled_modules: app.enabled_modules,
            data_dir,
            identity,
        };

        Ok((engine, app.router))
    }

    pub async fn issue_auto_session(&self) -> anyhow::Result<AuthSessionDto> {
        self.auth
            .login(&self.identity.username, &self.identity.local_password)
            .await
            .map_err(|e| anyhow::anyhow!("failed to issue local auto session: {e}"))
    }

    pub fn session_cookie_header(&self, token: &str) -> HeaderValue {
        let ttl = self.auth.token_ttl_seconds();
        let raw =
            format!("{AUTH_COOKIE_NAME}={token}; Path=/; HttpOnly; SameSite=Lax; Max-Age={ttl}");
        HeaderValue::from_str(&raw).unwrap_or_else(|_| HeaderValue::from_static(""))
    }
}

fn persist_identity_atomic(path: &Path, identity: &LocalIdentity) -> anyhow::Result<()> {
    if let Some(parent) = path.parent()
        && !parent.as_os_str().is_empty()
    {
        fs::create_dir_all(parent)
            .with_context(|| format!("failed to create identity directory {}", parent.display()))?;
    }

    let payload = serde_json::to_vec_pretty(identity)?;
    let file_name = path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("local_identity.json");
    let tmp_path = path.with_file_name(format!(".{file_name}.{}.tmp", Uuid::new_v4()));

    {
        let mut file = fs::File::create(&tmp_path).with_context(|| {
            format!(
                "failed to create temporary identity file {}",
                tmp_path.display()
            )
        })?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = file.set_permissions(fs::Permissions::from_mode(0o600));
        }
        if let Err(err) = file.write_all(&payload).and_then(|()| file.sync_all()) {
            let _ = fs::remove_file(&tmp_path);
            return Err(err.into());
        }
    }

    if let Err(err) = fs::rename(&tmp_path, path) {
        let _ = fs::remove_file(&tmp_path);
        return Err(err)
            .with_context(|| format!("failed to atomically replace {}", path.display()));
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn local_identity_persists_and_reloads_stably() {
        let tmp = TempDir::new().unwrap();
        let path = tmp.path().join("local_identity.json");

        let first = LocalIdentity::load_or_create(&path).unwrap();
        assert_eq!(first.username, LOCAL_ADMIN_USERNAME);
        assert!(first.jwt_secret.len() >= 32);
        assert!(first.local_password.len() >= 16);

        let second = LocalIdentity::load_or_create(&path).unwrap();
        assert_eq!(second.jwt_secret, first.jwt_secret);
        assert_eq!(second.local_password, first.local_password);
    }

    #[tokio::test]
    async fn local_engine_initializes_admin_and_issues_auto_session() {
        let tmp = TempDir::new().unwrap();
        let (engine, _router) = LocalEngine::start(tmp.path().to_path_buf(), None)
            .await
            .unwrap();

        assert!(!engine.enabled_modules.is_empty());
        let session = engine.issue_auto_session().await.unwrap();
        assert_eq!(session.user.username, LOCAL_ADMIN_USERNAME);
        assert!(!session.token.is_empty());

        let cookie = engine.session_cookie_header(&session.token);
        let cookie_str = cookie.to_str().unwrap();
        assert!(cookie_str.contains("xmedia_token="));
        assert!(cookie_str.contains("HttpOnly"));
        assert!(cookie_str.contains("SameSite=Lax"));
    }
}

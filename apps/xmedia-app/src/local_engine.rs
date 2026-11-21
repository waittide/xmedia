use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

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
use xmedia_core::http::{AUTH_COOKIE_NAME, configure_temp_dir};
use xmedia_core::infra::events::BroadcastEventHub;

pub const LOCAL_ADMIN_USERNAME: &str = "local";
pub const LOCAL_ADMIN_DISPLAY_NAME: &str = "Local";
pub const LOCAL_JWT_TTL_SECS: u64 = 30 * 86_400;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LocalIdentity {
    pub username: String,
    pub jwt_secret: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub local_password: Option<String>,
}

impl LocalIdentity {
    pub fn load_or_create(path: &Path) -> anyhow::Result<Self> {
        if path.exists() {
            match fs::read(path) {
                Ok(raw) if raw.iter().all(|b| b.is_ascii_whitespace()) => {}
                Ok(raw) => match serde_json::from_slice::<Self>(&raw) {
                    Ok(mut parsed) => {
                        if parsed.username.trim().is_empty() {
                            parsed.username = LOCAL_ADMIN_USERNAME.to_string();
                        }
                        if parsed.jwt_secret.trim().len() >= 32 {
                            return Ok(parsed);
                        }
                    }
                    Err(err) => {
                        backup_corrupt_identity(path, &err.to_string());
                    }
                },
                Err(err) => {
                    backup_corrupt_identity(path, &err.to_string());
                }
            }
        }

        let generated = Self {
            username: LOCAL_ADMIN_USERNAME.to_string(),
            jwt_secret: format!("{}{}", Uuid::new_v4().simple(), Uuid::new_v4().simple()),
            local_password: None,
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
    pub temp_dir: PathBuf,
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
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            if let Err(err) = fs::set_permissions(&data_dir, fs::Permissions::from_mode(0o700)) {
                tracing::warn!(
                    path = %data_dir.display(),
                    error = %err,
                    "failed to tighten permissions on standalone data directory"
                );
            }
        }

        let temp_dir = configure_temp_dir(data_dir.join("tmp")).with_context(|| {
            format!(
                "failed to prepare standalone temp directory {}",
                data_dir.join("tmp").display()
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

        let app = XMediaApp::connect_and_init_with_cors(
            XMediaConfig {
                data_dir: data_dir.clone(),
                database_url: format!("sqlite:{}", db_path.display()),
                secret: identity.jwt_secret.clone(),
                jwt_ttl_secs: LOCAL_JWT_TTL_SECS,
                ffmpeg_bin,
                external_web_dir,
            },
            false,
        )
        .await?;

        let setup_status = app
            .auth
            .setup_status()
            .await
            .map_err(|e| anyhow::anyhow!("failed to inspect local auth setup status: {e}"))?;
        if setup_status.needs_setup {
            let bootstrap_password = identity
                .local_password
                .clone()
                .filter(|p| p.len() >= 6)
                .unwrap_or_else(|| format!("xm_{}", Uuid::new_v4().simple()));
            app.auth
                .setup_initial_admin(SetupAdminInput {
                    username: identity.username.clone(),
                    display_name: Some(LOCAL_ADMIN_DISPLAY_NAME.to_string()),
                    password: bootstrap_password,
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
            temp_dir,
            identity,
        };

        Ok((engine, app.router))
    }

    pub async fn issue_auto_session(&self) -> anyhow::Result<AuthSessionDto> {
        self.auth
            .issue_local_admin_session(Some(&self.identity.username))
            .await
            .map_err(|e| anyhow::anyhow!("failed to issue local auto session: {e}"))
    }

    pub fn session_cookie_header(&self, token: &str) -> HeaderValue {
        let ttl = self.auth.token_ttl_seconds();
        let raw =
            format!("{AUTH_COOKIE_NAME}={token}; Path=/; HttpOnly; SameSite=Strict; Max-Age={ttl}");
        HeaderValue::from_str(&raw).unwrap_or_else(|_| HeaderValue::from_static(""))
    }
}

fn backup_corrupt_identity(path: &Path, reason: &str) {
    let ts = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let file_name = path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("local_identity.json");
    let backup = path.with_file_name(format!("{file_name}.corrupt.{ts}.bak"));
    let _ = fs::rename(path, &backup);
    tracing::warn!(
        path = %path.display(),
        backup = %backup.display(),
        error = %reason,
        "local identity file was corrupt; backed up and regenerated"
    );
}

fn create_private_file(path: &Path) -> std::io::Result<fs::File> {
    let mut opts = fs::OpenOptions::new();
    opts.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        opts.mode(0o600);
    }
    opts.open(path)
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
        let mut file = create_private_file(&tmp_path).with_context(|| {
            format!(
                "failed to create temporary identity file {}",
                tmp_path.display()
            )
        })?;
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
    use xmedia_core::application::dto::UpdateUserInput;

    #[test]
    fn local_identity_persists_and_reloads_stably_and_recovers_from_corruption() {
        let tmp = TempDir::new().unwrap();
        let path = tmp.path().join("local_identity.json");

        let first = LocalIdentity::load_or_create(&path).unwrap();
        assert_eq!(first.username, LOCAL_ADMIN_USERNAME);
        assert!(first.jwt_secret.len() >= 32);
        assert!(first.local_password.is_none());

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = fs::metadata(&path).unwrap().permissions().mode() & 0o777;
            assert_eq!(mode, 0o600);
        }

        let second = LocalIdentity::load_or_create(&path).unwrap();
        assert_eq!(second.jwt_secret, first.jwt_secret);

        // Corrupt identity file is backed up and regenerated cleanly.
        fs::write(&path, b"{corrupt").unwrap();
        let recovered = LocalIdentity::load_or_create(&path).unwrap();
        assert_eq!(recovered.username, LOCAL_ADMIN_USERNAME);
        assert!(recovered.jwt_secret.len() >= 32);
        assert_ne!(recovered.jwt_secret, first.jwt_secret);
    }

    #[tokio::test]
    async fn local_engine_auto_session_survives_password_change_and_restart() {
        let tmp = TempDir::new().unwrap();
        let (engine, _router) = LocalEngine::start(tmp.path().to_path_buf(), None)
            .await
            .unwrap();

        assert!(!engine.enabled_modules.is_empty());
        assert!(engine.temp_dir.is_dir());

        let session = engine.issue_auto_session().await.unwrap();
        assert_eq!(session.user.username, LOCAL_ADMIN_USERNAME);
        assert!(!session.token.is_empty());

        let cookie = engine.session_cookie_header(&session.token);
        let cookie_str = cookie.to_str().unwrap();
        assert!(cookie_str.contains("xmedia_token="));
        assert!(cookie_str.contains("HttpOnly"));
        assert!(cookie_str.contains("SameSite=Strict"));

        // Change admin password via AuthService and restart LocalEngine (even with wiped identity)
        let actor = engine.auth.authenticate_jwt(&session.token).await.unwrap();
        engine
            .auth
            .update_user(
                &actor,
                actor.id,
                UpdateUserInput {
                    display_name: None,
                    role: None,
                    is_active: None,
                    password: Some("NewStrongPassword123!".into()),
                },
            )
            .await
            .unwrap();

        // Delete local_identity.json to simulate identity drift/recreation
        let _ = fs::remove_file(tmp.path().join("local_identity.json"));

        let (restarted, _router2) = LocalEngine::start(tmp.path().to_path_buf(), None)
            .await
            .unwrap();
        let after_restart = restarted.issue_auto_session().await.unwrap();
        assert_eq!(after_restart.user.username, LOCAL_ADMIN_USERNAME);
        assert!(!after_restart.token.is_empty());
    }
}

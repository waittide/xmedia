use std::fs;
use std::future::Future;
use std::path::{Path, PathBuf};
use std::pin::Pin;
use std::sync::Arc;

use anyhow::Context;
use axum::Router;
use axum::body::{Body, to_bytes};
use axum::http::header::{AUTHORIZATION, CONTENT_TYPE};
use axum::http::{HeaderValue, Method, Request, StatusCode};
use bytes::Bytes;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use tower_service::Service;
use uuid::Uuid;
use xmedia::{XMediaApp, XMediaConfig};
use xmedia_core::application::auth::AuthService;
use xmedia_core::application::dto::{AuthSessionDto, SetupAdminInput};
use xmedia_core::application::library::LibraryService;
use xmedia_core::domain::library::MediaKind;
use xmedia_core::http::{AUTH_COOKIE_NAME, configure_temp_dir};
use xmedia_core::infra::events::BroadcastEventHub;

use crate::remote_common::RemoteClientError;

pub const LOCAL_ADMIN_USERNAME: &str = "local";
pub const LOCAL_ADMIN_DISPLAY_NAME: &str = "Local";
pub const LOCAL_JWT_TTL_SECS: u64 = 30 * 86_400;

pub type BoxImportFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LocalImportedMedia {
    pub id: String,
    pub title: String,
}

/// Direct in-process port for importing downloaded remote OPDS/Subsonic media into the
/// standalone local library without issuing loopback TCP HTTP requests.
pub trait LocalMediaImporter: Send + Sync {
    fn import_book<'a>(
        &'a self,
        filename: String,
        data: Bytes,
    ) -> BoxImportFuture<'a, Result<LocalImportedMedia, RemoteClientError>>;

    fn import_comic<'a>(
        &'a self,
        filename: String,
        data: Bytes,
    ) -> BoxImportFuture<'a, Result<LocalImportedMedia, RemoteClientError>>;

    fn import_track<'a>(
        &'a self,
        filename: String,
        data: Bytes,
    ) -> BoxImportFuture<'a, Result<LocalImportedMedia, RemoteClientError>>;
}

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
                        discard_corrupt_identity(path, &err.to_string());
                    }
                },
                Err(err) => {
                    discard_corrupt_identity(path, &err.to_string());
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

    pub fn derive_secret_encryption_key(&self) -> [u8; 32] {
        let mut sha = Sha256::new();
        sha.update(b"xmedia-app:remote-sources:aes256gcm:v1:");
        sha.update(self.jwt_secret.as_bytes());
        let digest = sha.finalize();
        let mut key = [0u8; 32];
        key.copy_from_slice(&digest);
        key
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
    router: Router,
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

        let router = app.router.clone();
        let engine = Self {
            auth: app.auth,
            libraries: app.libraries,
            events: app.events,
            enabled_modules: app.enabled_modules,
            data_dir,
            temp_dir,
            identity,
            router,
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

    pub fn secret_encryption_key(&self) -> [u8; 32] {
        self.identity.derive_secret_encryption_key()
    }

    async fn dispatch_in_process_import(
        &self,
        route_path: &str,
        filename: &str,
        data: Bytes,
        label: &str,
    ) -> Result<Value, RemoteClientError> {
        let session = self
            .issue_auto_session()
            .await
            .map_err(|e| RemoteClientError::Internal(format!("Local auth session failed: {e}")))?;

        let boundary = format!("xmedia-inproc-{}", Uuid::new_v4().simple());
        let body = build_multipart_file_body(&boundary, filename, &data);
        let content_type = format!("multipart/form-data; boundary={boundary}");

        let req = Request::builder()
            .method(Method::POST)
            .uri(route_path)
            .header(AUTHORIZATION, format!("Bearer {}", session.token))
            .header(CONTENT_TYPE, content_type)
            .body(Body::from(body))
            .map_err(|e| {
                RemoteClientError::Internal(format!("Failed building local {label} request: {e}"))
            })?;

        let mut svc = self.router.clone();
        let resp = Service::call(&mut svc, req)
            .await
            .map_err(|err| match err {})?;
        let status = resp.status();
        let body_bytes = to_bytes(resp.into_body(), 4 * 1024 * 1024)
            .await
            .map_err(|e| {
                RemoteClientError::Internal(format!("Failed reading local {label} response: {e}"))
            })?;
        let parsed_json: Option<Value> = serde_json::from_slice(&body_bytes).ok();

        if !status.is_success() {
            let msg = parsed_json
                .as_ref()
                .and_then(|v| v.pointer("/error/message").and_then(Value::as_str))
                .map(str::to_string)
                .unwrap_or_else(|| {
                    let snippet = String::from_utf8_lossy(&body_bytes);
                    let trimmed = snippet.trim();
                    if trimmed.is_empty() {
                        format!("Local library rejected imported {label}")
                    } else {
                        trimmed.chars().take(200).collect()
                    }
                });
            if status == StatusCode::PAYLOAD_TOO_LARGE {
                return Err(RemoteClientError::TooLarge(format!(
                    "Local {label} import rejected (HTTP 413): {msg}"
                )));
            }
            return Err(RemoteClientError::Upstream(format!(
                "Local {label} import failed (HTTP {}): {msg}",
                status.as_u16()
            )));
        }

        parsed_json.ok_or_else(|| {
            RemoteClientError::Internal(format!(
                "Local {label} import succeeded but returned invalid JSON"
            ))
        })
    }
}

impl LocalMediaImporter for LocalEngine {
    fn import_book<'a>(
        &'a self,
        filename: String,
        data: Bytes,
    ) -> BoxImportFuture<'a, Result<LocalImportedMedia, RemoteClientError>> {
        Box::pin(async move {
            let resp_json = self
                .dispatch_in_process_import("/api/v1/book/books/upload", &filename, data, "book")
                .await?;
            let first = resp_json
                .get("uploaded")
                .and_then(Value::as_array)
                .and_then(|arr| arr.first())
                .ok_or_else(|| {
                    RemoteClientError::Internal(
                        "Local book import returned empty uploaded array".into(),
                    )
                })?;
            let id = first
                .get("id")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string();
            let title = first
                .pointer("/metadata/title")
                .and_then(Value::as_str)
                .unwrap_or(&filename)
                .to_string();
            Ok(LocalImportedMedia { id, title })
        })
    }

    fn import_comic<'a>(
        &'a self,
        filename: String,
        data: Bytes,
    ) -> BoxImportFuture<'a, Result<LocalImportedMedia, RemoteClientError>> {
        Box::pin(async move {
            let resp_json = self
                .dispatch_in_process_import("/api/v1/comic/upload", &filename, data, "comic")
                .await?;
            let first = resp_json
                .get("uploaded")
                .and_then(Value::as_array)
                .and_then(|arr| arr.first())
                .ok_or_else(|| {
                    RemoteClientError::Internal(
                        "Local comic import returned empty uploaded array".into(),
                    )
                })?;
            let id = first
                .get("series_id")
                .or_else(|| first.get("chapter_id"))
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string();
            let title = first
                .get("series_title")
                .or_else(|| first.get("chapter_title"))
                .and_then(Value::as_str)
                .unwrap_or(&filename)
                .to_string();
            Ok(LocalImportedMedia { id, title })
        })
    }

    fn import_track<'a>(
        &'a self,
        filename: String,
        data: Bytes,
    ) -> BoxImportFuture<'a, Result<LocalImportedMedia, RemoteClientError>> {
        Box::pin(async move {
            let resp_json = self
                .dispatch_in_process_import("/api/v1/music/tracks/upload", &filename, data, "music")
                .await?;
            let first = resp_json
                .get("uploaded_tracks")
                .and_then(Value::as_array)
                .and_then(|arr| arr.first())
                .ok_or_else(|| {
                    RemoteClientError::Internal(
                        "Local music import returned no uploaded tracks".into(),
                    )
                })?;
            let id = first
                .get("id")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string();
            let title = first
                .get("title")
                .and_then(Value::as_str)
                .unwrap_or(&filename)
                .to_string();
            Ok(LocalImportedMedia { id, title })
        })
    }
}

fn build_multipart_file_body(boundary: &str, filename: &str, data: &[u8]) -> Bytes {
    let escaped_name = filename.replace(['\\', '"', '\r', '\n'], "_");
    let header = format!(
        "--{boundary}\r\nContent-Disposition: form-data; name=\"file\"; filename=\"{escaped_name}\"\r\nContent-Type: application/octet-stream\r\n\r\n"
    );
    let footer = format!("\r\n--{boundary}--\r\n");
    let mut buf = Vec::with_capacity(header.len() + data.len() + footer.len());
    buf.extend_from_slice(header.as_bytes());
    buf.extend_from_slice(data);
    buf.extend_from_slice(footer.as_bytes());
    Bytes::from(buf)
}

fn discard_corrupt_identity(path: &Path, reason: &str) {
    let _ = fs::remove_file(path);
    tracing::warn!(
        path = %path.display(),
        error = %reason,
        "local identity file was corrupt; removed without writing plaintext backup and regenerated"
    );
}

fn persist_identity_atomic(path: &Path, identity: &LocalIdentity) -> anyhow::Result<()> {
    let payload = serde_json::to_vec_pretty(identity)?;
    crate::atomic_fs::write_private_file_atomic(path, &payload)
        .with_context(|| format!("failed to atomically persist {}", path.display()))
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

    #[tokio::test]
    async fn local_media_importer_imports_book_in_process_without_tcp_server() {
        let tmp = TempDir::new().unwrap();
        let (engine, _router) = LocalEngine::start(tmp.path().to_path_buf(), None)
            .await
            .unwrap();
        let importer: &dyn LocalMediaImporter = &engine;
        let imported = importer
            .import_book(
                "Sample Novel.txt".to_string(),
                Bytes::from_static(b"Chapter 1\n\nHello in-process import!"),
            )
            .await
            .unwrap();
        assert!(!imported.id.is_empty());
        assert_eq!(imported.title, "Sample Novel");
    }
}

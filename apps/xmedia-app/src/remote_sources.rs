use std::collections::HashMap;
use std::fmt;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, RwLock};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use url::Url;
use uuid::Uuid;

const MARK_OK_THROTTLE_SECS: u64 = 60;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum RemoteProtocol {
    Opds,
    Subsonic,
}

impl RemoteProtocol {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Opds => "opds",
            Self::Subsonic => "subsonic",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RemoteSource {
    pub id: String,
    pub name: String,
    pub protocol: RemoteProtocol,
    pub url: String,
    #[serde(default)]
    pub username: String,
    #[serde(default)]
    pub password: String,
    #[serde(default)]
    pub allow_insecure_tls: bool,
    #[serde(default)]
    pub last_ok_at: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RemoteSourceSummaryDto {
    pub id: String,
    pub name: String,
    pub protocol: RemoteProtocol,
    pub url: String,
    pub username: String,
    pub has_password: bool,
    pub allow_insecure_tls: bool,
    pub last_ok_at: Option<u64>,
}

impl From<&RemoteSource> for RemoteSourceSummaryDto {
    fn from(src: &RemoteSource) -> Self {
        Self {
            id: src.id.clone(),
            name: src.name.clone(),
            protocol: src.protocol,
            url: src.url.clone(),
            username: src.username.clone(),
            has_password: !src.password.is_empty(),
            allow_insecure_tls: src.allow_insecure_tls,
            last_ok_at: src.last_ok_at,
        }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct PersistedRemoteSources {
    #[serde(default)]
    pub sources: Vec<RemoteSource>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct CreateRemoteSourceInput {
    #[serde(default)]
    pub name: String,
    pub protocol: RemoteProtocol,
    pub url: String,
    #[serde(default)]
    pub username: Option<String>,
    #[serde(default)]
    pub password: Option<String>,
    #[serde(default)]
    pub allow_insecure_tls: bool,
}

#[derive(Debug, Clone, Deserialize)]
pub struct UpdateRemoteSourceInput {
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub protocol: Option<RemoteProtocol>,
    #[serde(default)]
    pub url: Option<String>,
    #[serde(default)]
    pub username: Option<String>,
    #[serde(default)]
    pub password: Option<String>,
    #[serde(default)]
    pub clear_password: bool,
    #[serde(default)]
    pub allow_insecure_tls: Option<bool>,
}

#[derive(Debug, Clone)]
pub enum SourceStoreError {
    InvalidUrl(String),
    NotFound(String),
    Io(String),
}

impl fmt::Display for SourceStoreError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidUrl(msg) | Self::NotFound(msg) | Self::Io(msg) => write!(f, "{msg}"),
        }
    }
}

impl std::error::Error for SourceStoreError {}

pub type SharedRemoteSourceStore = Arc<RwLock<RemoteSourceStore>>;

pub struct PendingSourcePersist {
    storage_path: PathBuf,
    snapshot: PersistedRemoteSources,
    revision: u64,
    persist_lock: Arc<Mutex<()>>,
    persisted_revision: Arc<AtomicU64>,
}

impl PendingSourcePersist {
    pub fn persist(self) -> Result<(), SourceStoreError> {
        let _guard = self
            .persist_lock
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if self.persisted_revision.load(Ordering::Acquire) >= self.revision {
            return Ok(());
        }
        persist_atomic(&self.storage_path, &self.snapshot)?;
        self.persisted_revision
            .fetch_max(self.revision, Ordering::AcqRel);
        Ok(())
    }
}

#[derive(Debug)]
pub struct RemoteSourceStore {
    storage_path: PathBuf,
    data: PersistedRemoteSources,
    last_ok_persisted: HashMap<String, u64>,
    revision: u64,
    persist_lock: Arc<Mutex<()>>,
    persisted_revision: Arc<AtomicU64>,
}

impl RemoteSourceStore {
    pub fn load(storage_path: PathBuf) -> anyhow::Result<Self> {
        let mut data = if storage_path.exists() {
            match fs::read(&storage_path) {
                Ok(raw) if raw.iter().all(|b| b.is_ascii_whitespace()) => {
                    PersistedRemoteSources::default()
                }
                Ok(raw) => match serde_json::from_slice::<PersistedRemoteSources>(&raw) {
                    Ok(parsed) => parsed,
                    Err(err) => {
                        backup_corrupt_file(&storage_path, &err.to_string());
                        PersistedRemoteSources::default()
                    }
                },
                Err(err) => {
                    backup_corrupt_file(&storage_path, &err.to_string());
                    PersistedRemoteSources::default()
                }
            }
        } else {
            PersistedRemoteSources::default()
        };

        let mut last_ok_persisted = HashMap::new();
        for src in &mut data.sources {
            if let Ok(normalized) = normalize_remote_url(&src.url) {
                src.url = normalized;
            }
            if let Some(ts) = src.last_ok_at {
                last_ok_persisted.insert(src.id.clone(), ts);
            }
        }

        Ok(Self {
            storage_path,
            data,
            last_ok_persisted,
            revision: 0,
            persist_lock: Arc::new(Mutex::new(())),
            persisted_revision: Arc::new(AtomicU64::new(0)),
        })
    }

    pub fn open_shared(storage_path: PathBuf) -> anyhow::Result<SharedRemoteSourceStore> {
        Ok(Arc::new(RwLock::new(Self::load(storage_path)?)))
    }

    pub fn storage_path(&self) -> &Path {
        &self.storage_path
    }

    pub fn count(&self) -> usize {
        self.data.sources.len()
    }

    pub fn list_summaries(&self) -> Vec<RemoteSourceSummaryDto> {
        self.data
            .sources
            .iter()
            .map(RemoteSourceSummaryDto::from)
            .collect()
    }

    pub fn get(&self, id: &str) -> Option<RemoteSource> {
        self.data.sources.iter().find(|s| s.id == id).cloned()
    }

    fn persist_sync(&mut self, next: &PersistedRemoteSources) -> Result<(), SourceStoreError> {
        let next_rev = self.revision.saturating_add(1);
        let _guard = self
            .persist_lock
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        persist_atomic(&self.storage_path, next)?;
        self.revision = next_rev;
        self.persisted_revision
            .fetch_max(next_rev, Ordering::AcqRel);
        Ok(())
    }

    pub fn create(
        &mut self,
        input: CreateRemoteSourceInput,
    ) -> Result<(RemoteSourceSummaryDto, Vec<RemoteSourceSummaryDto>), SourceStoreError> {
        let normalized_url = normalize_remote_url(&input.url)?;
        let name = {
            let trimmed = input.name.trim();
            if trimmed.is_empty() {
                default_source_name(input.protocol, &normalized_url)
            } else {
                trimmed.to_string()
            }
        };

        let source = RemoteSource {
            id: Uuid::new_v4().to_string(),
            name,
            protocol: input.protocol,
            url: normalized_url,
            username: input.username.unwrap_or_default().trim().to_string(),
            password: input.password.unwrap_or_default(),
            allow_insecure_tls: input.allow_insecure_tls,
            last_ok_at: None,
        };

        let mut next = self.data.clone();
        next.sources.push(source.clone());
        self.persist_sync(&next)?;
        self.data = next;

        Ok((RemoteSourceSummaryDto::from(&source), self.list_summaries()))
    }

    pub fn update(
        &mut self,
        id: &str,
        input: UpdateRemoteSourceInput,
    ) -> Result<(RemoteSourceSummaryDto, Vec<RemoteSourceSummaryDto>), SourceStoreError> {
        let mut next = self.data.clone();
        let Some(existing) = next.sources.iter_mut().find(|s| s.id == id) else {
            return Err(SourceStoreError::NotFound(format!(
                "Remote source '{id}' not found"
            )));
        };

        if let Some(protocol) = input.protocol {
            existing.protocol = protocol;
        }
        if let Some(raw_url) = input.url {
            existing.url = normalize_remote_url(&raw_url)?;
        }
        if let Some(raw_name) = input.name {
            let trimmed = raw_name.trim();
            existing.name = if trimmed.is_empty() {
                default_source_name(existing.protocol, &existing.url)
            } else {
                trimmed.to_string()
            };
        }
        if let Some(username) = input.username {
            existing.username = username.trim().to_string();
        }
        if input.clear_password {
            existing.password.clear();
        } else if let Some(password) = input.password
            && !password.is_empty()
        {
            // Keep existing password when password is None or empty unless clear_password is set.
            existing.password = password;
        }
        if let Some(allow_insecure_tls) = input.allow_insecure_tls {
            existing.allow_insecure_tls = allow_insecure_tls;
        }

        let summary = RemoteSourceSummaryDto::from(&*existing);
        self.persist_sync(&next)?;
        self.data = next;

        Ok((summary, self.list_summaries()))
    }

    pub fn delete(&mut self, id: &str) -> Result<Vec<RemoteSourceSummaryDto>, SourceStoreError> {
        let mut next = self.data.clone();
        let before = next.sources.len();
        next.sources.retain(|s| s.id != id);
        if next.sources.len() == before {
            return Err(SourceStoreError::NotFound(format!(
                "Remote source '{id}' not found"
            )));
        }
        self.persist_sync(&next)?;
        self.data = next;
        self.last_ok_persisted.remove(id);
        Ok(self.list_summaries())
    }

    /// Updates `last_ok_at` in memory and returns an optional throttled persistence task
    /// to execute off the Tokio async runtime via `spawn_blocking`.
    pub fn mark_ok_throttled(&mut self, id: &str) -> Option<PendingSourcePersist> {
        let now = unix_timestamp_secs();
        let entry = self.data.sources.iter_mut().find(|s| s.id == id)?;
        entry.last_ok_at = Some(now);

        let should_persist = self
            .last_ok_persisted
            .get(id)
            .is_none_or(|&prev| now.saturating_sub(prev) >= MARK_OK_THROTTLE_SECS);
        if !should_persist {
            return None;
        }

        self.last_ok_persisted.insert(id.to_string(), now);
        self.revision = self.revision.saturating_add(1);
        Some(PendingSourcePersist {
            storage_path: self.storage_path.clone(),
            snapshot: self.data.clone(),
            revision: self.revision,
            persist_lock: self.persist_lock.clone(),
            persisted_revision: self.persisted_revision.clone(),
        })
    }
}

pub fn is_same_origin(a: &Url, b: &Url) -> bool {
    matches!(a.scheme(), "http" | "https")
        && matches!(b.scheme(), "http" | "https")
        && a.origin().is_tuple()
        && a.origin() == b.origin()
}

pub fn normalize_remote_url(raw: &str) -> Result<String, SourceStoreError> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Err(SourceStoreError::InvalidUrl(
            "Remote source URL must not be empty".to_string(),
        ));
    }

    let parsed = Url::parse(trimmed)
        .map_err(|e| SourceStoreError::InvalidUrl(format!("Invalid remote source URL: {e}")))?;

    match parsed.scheme() {
        "http" | "https" => {}
        scheme => {
            return Err(SourceStoreError::InvalidUrl(format!(
                "Unsupported URL scheme '{scheme}': only http and https are allowed"
            )));
        }
    }

    if parsed.host_str().is_none_or(|h| h.is_empty()) {
        return Err(SourceStoreError::InvalidUrl(
            "Remote source URL must include a valid host".to_string(),
        ));
    }

    if !parsed.username().is_empty() || parsed.password().is_some() {
        return Err(SourceStoreError::InvalidUrl(
            "Remote source URL must not contain embedded credentials".to_string(),
        ));
    }

    if parsed.fragment().is_some() {
        return Err(SourceStoreError::InvalidUrl(
            "Remote source URL must not contain a URL fragment".to_string(),
        ));
    }

    let serialized = if parsed.query().is_some() {
        parsed.as_str().to_string()
    } else {
        parsed.as_str().trim_end_matches('/').to_string()
    };
    Ok(serialized)
}

pub fn default_source_name(protocol: RemoteProtocol, normalized_url: &str) -> String {
    let label = match protocol {
        RemoteProtocol::Opds => "OPDS",
        RemoteProtocol::Subsonic => "Subsonic",
    };
    if let Ok(parsed) = Url::parse(normalized_url)
        && let Some(host) = parsed.host_str()
    {
        return match parsed.port() {
            Some(port) => format!("{label} ({host}:{port})"),
            None => format!("{label} ({host})"),
        };
    }
    format!("{label} ({normalized_url})")
}

fn unix_timestamp_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

fn backup_corrupt_file(path: &Path, reason: &str) {
    let file_name = path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("remote_sources.json");
    let backup = path.with_file_name(format!("{file_name}.corrupt.{}.bak", unix_timestamp_secs()));
    match fs::rename(path, &backup) {
        Ok(()) => {
            tracing::warn!(
                path = %path.display(),
                backup = %backup.display(),
                error = %reason,
                "remote sources file was corrupt; backed up and fell back to empty list"
            );
        }
        Err(rename_err) => {
            tracing::warn!(
                path = %path.display(),
                error = %reason,
                backup_error = %rename_err,
                "remote sources file was corrupt and could not be backed up; falling back to empty list"
            );
        }
    }
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

fn persist_atomic(path: &Path, data: &PersistedRemoteSources) -> Result<(), SourceStoreError> {
    if let Some(parent) = path.parent()
        && !parent.as_os_str().is_empty()
    {
        fs::create_dir_all(parent).map_err(|e| {
            SourceStoreError::Io(format!(
                "Failed to create config directory {}: {e}",
                parent.display()
            ))
        })?;
    }

    let payload = serde_json::to_vec_pretty(data)
        .map_err(|e| SourceStoreError::Io(format!("Failed to serialize remote sources: {e}")))?;

    let file_name = path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("remote_sources.json");
    let tmp_path = path.with_file_name(format!(".{file_name}.{}.tmp", Uuid::new_v4()));

    {
        let mut file = create_private_file(&tmp_path).map_err(|e| {
            SourceStoreError::Io(format!(
                "Failed to create temporary config file {}: {e}",
                tmp_path.display()
            ))
        })?;
        if let Err(err) = file.write_all(&payload).and_then(|()| file.sync_all()) {
            let _ = fs::remove_file(&tmp_path);
            return Err(SourceStoreError::Io(format!(
                "Failed to write temporary config file: {err}"
            )));
        }
    }

    if let Err(err) = fs::rename(&tmp_path, path) {
        let _ = fs::remove_file(&tmp_path);
        return Err(SourceStoreError::Io(format!(
            "Failed to atomically replace {}: {err}",
            path.display()
        )));
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn remote_source_store_redacts_password_and_preserves_on_empty_update() {
        let tmp = TempDir::new().unwrap();
        let file = tmp.path().join("remote_sources.json");
        let mut store = RemoteSourceStore::load(file.clone()).unwrap();

        let (created, list) = store
            .create(CreateRemoteSourceInput {
                name: "Home OPDS".into(),
                protocol: RemoteProtocol::Opds,
                url: "http://127.0.0.1:8080/opds/book/v1.2/".into(),
                username: Some("alice".into()),
                password: Some("super-secret".into()),
                allow_insecure_tls: false,
            })
            .unwrap();

        assert_eq!(list.len(), 1);
        assert!(created.has_password);
        assert_eq!(created.url, "http://127.0.0.1:8080/opds/book/v1.2");

        // Serialized summary must never contain the raw password.
        let summary_json = serde_json::to_string(&created).unwrap();
        assert!(!summary_json.contains("super-secret"));
        assert!(summary_json.contains("\"has_password\":true"));

        // Updating with None or empty password retains the existing secret.
        let (updated, _) = store
            .update(
                &created.id,
                UpdateRemoteSourceInput {
                    name: Some("Renamed OPDS".into()),
                    protocol: None,
                    url: None,
                    username: None,
                    password: Some(String::new()),
                    clear_password: false,
                    allow_insecure_tls: Some(true),
                },
            )
            .unwrap();
        assert_eq!(updated.name, "Renamed OPDS");
        assert!(updated.has_password);
        assert!(updated.allow_insecure_tls);
        assert_eq!(store.get(&created.id).unwrap().password, "super-secret");

        // Reload from disk and verify 0600 permissions on Unix.
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = fs::metadata(&file).unwrap().permissions().mode() & 0o777;
            assert_eq!(mode, 0o600);
        }

        let reloaded = RemoteSourceStore::load(file.clone()).unwrap();
        assert_eq!(reloaded.count(), 1);
        assert_eq!(reloaded.get(&created.id).unwrap().password, "super-secret");

        // Explicit clear_password removes the saved secret.
        let (cleared, _) = store
            .update(
                &created.id,
                UpdateRemoteSourceInput {
                    name: None,
                    protocol: None,
                    url: None,
                    username: None,
                    password: None,
                    clear_password: true,
                    allow_insecure_tls: None,
                },
            )
            .unwrap();
        assert!(!cleared.has_password);
        assert!(store.get(&created.id).unwrap().password.is_empty());

        // Throttled mark_ok returns a persist task first time, then throttles immediate repeat.
        let pending = store.mark_ok_throttled(&created.id);
        assert!(pending.is_some());
        pending.unwrap().persist().unwrap();
        assert!(store.mark_ok_throttled(&created.id).is_none());

        // Delete works.
        let after_delete = store.delete(&created.id).unwrap();
        assert!(after_delete.is_empty());
    }

    #[test]
    fn corrupt_remote_sources_json_is_backed_up_and_falls_back_to_empty() {
        let tmp = TempDir::new().unwrap();
        let file = tmp.path().join("remote_sources.json");
        fs::write(&file, b"{ not valid json !!!").unwrap();

        let store = RemoteSourceStore::load(file.clone()).unwrap();
        assert_eq!(store.count(), 0);
        assert!(!file.exists(), "corrupt file should be moved to backup");

        let has_backup = fs::read_dir(tmp.path())
            .unwrap()
            .flatten()
            .any(|e| e.file_name().to_string_lossy().contains(".corrupt."));
        assert!(has_backup, "expected a .corrupt.*.bak backup file");
    }

    #[test]
    fn normalize_remote_url_and_same_origin_checks() {
        assert!(normalize_remote_url("").is_err());
        assert!(normalize_remote_url("file:///etc/passwd").is_err());
        assert!(normalize_remote_url("ftp://example.com/opds").is_err());
        assert!(normalize_remote_url("http://user:pass@example.com/opds").is_err());
        assert!(normalize_remote_url("https://example.com/opds#frag").is_err());
        assert_eq!(
            normalize_remote_url("https://example.com/opds/").unwrap(),
            "https://example.com/opds"
        );

        let a = Url::parse("http://example.com:80/a").unwrap();
        let b = Url::parse("http://example.com/b").unwrap();
        let c = Url::parse("https://example.com/b").unwrap();
        let d = Url::parse("http://example.com:8080/b").unwrap();
        assert!(is_same_origin(&a, &b));
        assert!(!is_same_origin(&a, &c));
        assert!(!is_same_origin(&a, &d));
    }
}

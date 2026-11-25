mod crypto;
#[cfg(test)]
mod tests;

use std::collections::HashMap;
use std::fmt;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, RwLock};
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::Context;
use serde::{Deserialize, Serialize};
use url::Url;
use uuid::Uuid;

use crypto::DecryptedSecret;
pub use crypto::SourceSecretCipher;

use crate::local_engine::LocalIdentity;

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

#[derive(Clone, Serialize, Deserialize, PartialEq, Eq)]
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

impl fmt::Debug for RemoteSource {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let redacted_password = if self.password.is_empty() {
            ""
        } else {
            "[REDACTED]"
        };
        f.debug_struct("RemoteSource")
            .field("id", &self.id)
            .field("name", &self.name)
            .field("protocol", &self.protocol)
            .field("url", &self.url)
            .field("username", &self.username)
            .field("password", &redacted_password)
            .field("allow_insecure_tls", &self.allow_insecure_tls)
            .field("last_ok_at", &self.last_ok_at)
            .finish()
    }
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
    cipher: SourceSecretCipher,
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
        persist_atomic(&self.storage_path, &self.snapshot, &self.cipher)?;
        self.persisted_revision
            .fetch_max(self.revision, Ordering::AcqRel);
        Ok(())
    }
}

pub struct PendingSourceMutation {
    storage_path: PathBuf,
    next: PersistedRemoteSources,
    cipher: SourceSecretCipher,
    summary: Option<RemoteSourceSummaryDto>,
    removed_id: Option<String>,
    revision: u64,
    persist_lock: Arc<Mutex<()>>,
    persisted_revision: Arc<AtomicU64>,
}

impl PendingSourceMutation {
    pub fn persist(self) -> Result<PersistedSourceMutation, SourceStoreError> {
        let _guard = self
            .persist_lock
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        persist_atomic(&self.storage_path, &self.next, &self.cipher)?;
        self.persisted_revision
            .fetch_max(self.revision, Ordering::AcqRel);
        Ok(PersistedSourceMutation {
            next: self.next,
            summary: self.summary,
            removed_id: self.removed_id,
            revision: self.revision,
        })
    }
}

pub struct PersistedSourceMutation {
    next: PersistedRemoteSources,
    summary: Option<RemoteSourceSummaryDto>,
    removed_id: Option<String>,
    revision: u64,
}

#[derive(Debug)]
pub struct RemoteSourceStore {
    storage_path: PathBuf,
    data: PersistedRemoteSources,
    cipher: SourceSecretCipher,
    last_ok_persisted: HashMap<String, u64>,
    revision: u64,
    persist_lock: Arc<Mutex<()>>,
    persisted_revision: Arc<AtomicU64>,
}

impl RemoteSourceStore {
    pub fn load(storage_path: PathBuf) -> anyhow::Result<Self> {
        let identity_dir = storage_path
            .parent()
            .filter(|p| !p.as_os_str().is_empty())
            .unwrap_or_else(|| Path::new("."));
        let identity = LocalIdentity::load_or_create(&identity_dir.join("local_identity.json"))?;
        Self::load_with_key(storage_path, identity.derive_secret_encryption_key())
    }

    pub fn load_with_key(storage_path: PathBuf, key: [u8; 32]) -> anyhow::Result<Self> {
        let cipher = SourceSecretCipher::new(key);
        let mut data = match fs::read(&storage_path) {
            Ok(raw) if raw.iter().all(|b| b.is_ascii_whitespace()) => {
                PersistedRemoteSources::default()
            }
            Ok(raw) => match serde_json::from_slice::<PersistedRemoteSources>(&raw) {
                Ok(parsed) => parsed,
                Err(err) => {
                    discard_corrupt_sources_file(&storage_path, &err.to_string());
                    PersistedRemoteSources::default()
                }
            },
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
                PersistedRemoteSources::default()
            }
            Err(err) => {
                return Err(err).with_context(|| {
                    format!(
                        "failed to read remote sources file {}",
                        storage_path.display()
                    )
                });
            }
        };

        let mut last_ok_persisted = HashMap::new();
        let mut needs_migration_persist = false;
        for src in &mut data.sources {
            if let Ok(normalized) = normalize_remote_url(&src.url) {
                src.url = normalized;
            }
            match cipher.decrypt_or_migrate(&src.id, &src.password) {
                DecryptedSecret::Empty => {}
                DecryptedSecret::Decrypted(plain) => {
                    src.password = plain;
                }
                DecryptedSecret::MigratedPlaintext(plain) => {
                    src.password = plain;
                    needs_migration_persist = true;
                }
                DecryptedSecret::Undecryptable => {
                    tracing::warn!(
                        source_id = %src.id,
                        "remote source password could not be decrypted with current local identity; clearing stale secret"
                    );
                    src.password.clear();
                    needs_migration_persist = true;
                }
            }
            if let Some(ts) = src.last_ok_at {
                last_ok_persisted.insert(src.id.clone(), ts);
            }
        }

        if needs_migration_persist {
            persist_atomic(&storage_path, &data, &cipher).map_err(|e| {
                anyhow::anyhow!("failed migrating remote_sources.json secrets: {e}")
            })?;
        }

        Ok(Self {
            storage_path,
            data,
            cipher,
            last_ok_persisted,
            revision: 0,
            persist_lock: Arc::new(Mutex::new(())),
            persisted_revision: Arc::new(AtomicU64::new(0)),
        })
    }

    pub fn open_shared(storage_path: PathBuf) -> anyhow::Result<SharedRemoteSourceStore> {
        Ok(Arc::new(RwLock::new(Self::load(storage_path)?)))
    }

    pub fn open_shared_with_key(
        storage_path: PathBuf,
        key: [u8; 32],
    ) -> anyhow::Result<SharedRemoteSourceStore> {
        Ok(Arc::new(RwLock::new(Self::load_with_key(
            storage_path,
            key,
        )?)))
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

    fn make_pending_mutation(
        &self,
        next: PersistedRemoteSources,
        summary: Option<RemoteSourceSummaryDto>,
        removed_id: Option<String>,
    ) -> PendingSourceMutation {
        PendingSourceMutation {
            storage_path: self.storage_path.clone(),
            next,
            cipher: self.cipher.clone(),
            summary,
            removed_id,
            revision: self.revision.saturating_add(1),
            persist_lock: self.persist_lock.clone(),
            persisted_revision: self.persisted_revision.clone(),
        }
    }

    pub fn prepare_create(
        &self,
        input: CreateRemoteSourceInput,
    ) -> Result<PendingSourceMutation, SourceStoreError> {
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

        let summary = RemoteSourceSummaryDto::from(&source);
        let mut next = self.data.clone();
        next.sources.push(source);
        Ok(self.make_pending_mutation(next, Some(summary), None))
    }

    pub fn prepare_update(
        &self,
        id: &str,
        input: UpdateRemoteSourceInput,
    ) -> Result<PendingSourceMutation, SourceStoreError> {
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
        Ok(self.make_pending_mutation(next, Some(summary), None))
    }

    pub fn prepare_delete(&self, id: &str) -> Result<PendingSourceMutation, SourceStoreError> {
        let mut next = self.data.clone();
        let before = next.sources.len();
        next.sources.retain(|s| s.id != id);
        if next.sources.len() == before {
            return Err(SourceStoreError::NotFound(format!(
                "Remote source '{id}' not found"
            )));
        }
        Ok(self.make_pending_mutation(next, None, Some(id.to_string())))
    }

    pub fn commit_mutation(
        &mut self,
        mut persisted: PersistedSourceMutation,
    ) -> (Option<RemoteSourceSummaryDto>, Vec<RemoteSourceSummaryDto>) {
        // Preserve any concurrent in-memory `last_ok_at` updates that arrived while persisting.
        let latest_ok: HashMap<&str, u64> = self
            .data
            .sources
            .iter()
            .filter_map(|s| s.last_ok_at.map(|ts| (s.id.as_str(), ts)))
            .collect();
        for src in &mut persisted.next.sources {
            if let Some(&ts) = latest_ok.get(src.id.as_str()) {
                src.last_ok_at = Some(src.last_ok_at.map_or(ts, |prev| prev.max(ts)));
            }
        }
        let mut summary = persisted.summary;
        if let Some(ref mut sum) = summary
            && let Some(updated) = persisted.next.sources.iter().find(|s| s.id == sum.id)
        {
            sum.last_ok_at = updated.last_ok_at;
        }
        if let Some(ref removed_id) = persisted.removed_id {
            self.last_ok_persisted.remove(removed_id);
        }
        self.data = persisted.next;
        self.revision = self.revision.max(persisted.revision);
        (summary, self.list_summaries())
    }

    pub fn create(
        &mut self,
        input: CreateRemoteSourceInput,
    ) -> Result<(RemoteSourceSummaryDto, Vec<RemoteSourceSummaryDto>), SourceStoreError> {
        let persisted = self.prepare_create(input)?.persist()?;
        let (summary, list) = self.commit_mutation(persisted);
        Ok((
            summary.expect("create mutation always produces a summary"),
            list,
        ))
    }

    pub fn update(
        &mut self,
        id: &str,
        input: UpdateRemoteSourceInput,
    ) -> Result<(RemoteSourceSummaryDto, Vec<RemoteSourceSummaryDto>), SourceStoreError> {
        let persisted = self.prepare_update(id, input)?.persist()?;
        let (summary, list) = self.commit_mutation(persisted);
        Ok((
            summary.expect("update mutation always produces a summary"),
            list,
        ))
    }

    pub fn delete(&mut self, id: &str) -> Result<Vec<RemoteSourceSummaryDto>, SourceStoreError> {
        let persisted = self.prepare_delete(id)?.persist()?;
        let (_, list) = self.commit_mutation(persisted);
        Ok(list)
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
            cipher: self.cipher.clone(),
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

fn discard_corrupt_sources_file(path: &Path, reason: &str) {
    // Never rename or copy raw `remote_sources.json` into `.corrupt.*.bak`, since a manually edited
    // or legacy file could contain plaintext secrets.
    let _ = fs::remove_file(path);
    tracing::warn!(
        path = %path.display(),
        error = %reason,
        "remote sources file was corrupt; removed without writing plaintext backup and fell back to empty list"
    );
}

fn persist_atomic(
    path: &Path,
    data: &PersistedRemoteSources,
    cipher: &SourceSecretCipher,
) -> Result<(), SourceStoreError> {
    let mut on_disk = data.clone();
    for src in &mut on_disk.sources {
        if !src.password.is_empty() {
            src.password = cipher.encrypt(&src.id, &src.password)?;
        }
    }

    let payload = serde_json::to_vec_pretty(&on_disk)
        .map_err(|e| SourceStoreError::Io(format!("Failed to serialize remote sources: {e}")))?;

    crate::atomic_fs::write_private_file_atomic(path, &payload).map_err(|e| {
        SourceStoreError::Io(format!(
            "Failed to atomically persist {}: {e}",
            path.display()
        ))
    })
}

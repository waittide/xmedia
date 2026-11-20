use std::fmt;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::{Arc, RwLock};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use url::Url;
use uuid::Uuid;

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

#[derive(Debug)]
pub struct RemoteSourceStore {
    storage_path: PathBuf,
    data: PersistedRemoteSources,
}

impl RemoteSourceStore {
    pub fn load(storage_path: PathBuf) -> anyhow::Result<Self> {
        let mut data = if storage_path.exists() {
            let raw = fs::read(&storage_path)?;
            if raw.iter().all(|b| b.is_ascii_whitespace()) {
                PersistedRemoteSources::default()
            } else {
                serde_json::from_slice::<PersistedRemoteSources>(&raw)?
            }
        } else {
            PersistedRemoteSources::default()
        };

        for src in &mut data.sources {
            if let Ok(normalized) = normalize_remote_url(&src.url) {
                src.url = normalized;
            }
        }

        Ok(Self { storage_path, data })
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
        persist_atomic(&self.storage_path, &next)?;
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
        // Keep existing password when password is None or empty.
        if let Some(password) = input.password
            && !password.is_empty()
        {
            existing.password = password;
        }
        if let Some(allow_insecure_tls) = input.allow_insecure_tls {
            existing.allow_insecure_tls = allow_insecure_tls;
        }

        let summary = RemoteSourceSummaryDto::from(&*existing);
        persist_atomic(&self.storage_path, &next)?;
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
        persist_atomic(&self.storage_path, &next)?;
        self.data = next;
        Ok(self.list_summaries())
    }

    pub fn mark_ok(&mut self, id: &str) {
        let mut next = self.data.clone();
        let Some(entry) = next.sources.iter_mut().find(|s| s.id == id) else {
            return;
        };
        entry.last_ok_at = Some(unix_timestamp_secs());
        if persist_atomic(&self.storage_path, &next).is_ok() {
            self.data = next;
        }
    }
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
        let mut file = fs::File::create(&tmp_path).map_err(|e| {
            SourceStoreError::Io(format!(
                "Failed to create temporary config file {}: {e}",
                tmp_path.display()
            ))
        })?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = file.set_permissions(fs::Permissions::from_mode(0o600));
        }
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
                    allow_insecure_tls: Some(true),
                },
            )
            .unwrap();
        assert_eq!(updated.name, "Renamed OPDS");
        assert!(updated.has_password);
        assert!(updated.allow_insecure_tls);
        assert_eq!(store.get(&created.id).unwrap().password, "super-secret");

        // Reload from disk.
        let reloaded = RemoteSourceStore::load(file).unwrap();
        assert_eq!(reloaded.count(), 1);
        assert_eq!(reloaded.get(&created.id).unwrap().password, "super-secret");

        // Delete works.
        let after_delete = store.delete(&created.id).unwrap();
        assert!(after_delete.is_empty());
    }
}

use std::fmt;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::{Arc, RwLock};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use url::Url;
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ServerProfile {
    pub id: String,
    pub name: String,
    pub url: String,
    #[serde(default)]
    pub allow_insecure_tls: bool,
    #[serde(default)]
    pub last_connected_at: Option<u64>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct PersistedServers {
    #[serde(default)]
    pub active_server_id: Option<String>,
    #[serde(default)]
    pub servers: Vec<ServerProfile>,
}

#[derive(Debug, Clone)]
pub enum ConfigError {
    InvalidUrl(String),
    NotFound(String),
    Io(String),
}

impl fmt::Display for ConfigError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidUrl(msg) => write!(f, "{msg}"),
            Self::NotFound(msg) => write!(f, "{msg}"),
            Self::Io(msg) => write!(f, "{msg}"),
        }
    }
}

impl std::error::Error for ConfigError {}

#[derive(Debug, Clone)]
pub struct UpsertOutcome {
    pub server: ServerProfile,
    pub snapshot: PersistedServers,
    pub active_changed: bool,
}

#[derive(Debug, Clone)]
pub struct DeleteOutcome {
    pub snapshot: PersistedServers,
    pub cleared_active: bool,
}

pub type SharedServerStore = Arc<RwLock<AppServerStore>>;

#[derive(Debug)]
pub struct AppServerStore {
    storage_path: PathBuf,
    data: PersistedServers,
}

impl AppServerStore {
    pub fn load(storage_path: PathBuf) -> anyhow::Result<Self> {
        let mut data = if storage_path.exists() {
            let raw = fs::read(&storage_path)?;
            if raw.iter().all(|b| b.is_ascii_whitespace()) {
                PersistedServers::default()
            } else {
                serde_json::from_slice::<PersistedServers>(&raw)?
            }
        } else {
            PersistedServers::default()
        };

        // Normalize stored URLs and guard against dangling active_server_id.
        for server in &mut data.servers {
            if let Ok(normalized) = normalize_server_url(&server.url) {
                server.url = normalized;
            }
        }
        if let Some(ref active_id) = data.active_server_id
            && !data.servers.iter().any(|s| &s.id == active_id)
        {
            data.active_server_id = None;
        }

        Ok(Self { storage_path, data })
    }

    pub fn open_shared(storage_path: PathBuf) -> anyhow::Result<SharedServerStore> {
        Ok(Arc::new(RwLock::new(Self::load(storage_path)?)))
    }

    pub fn storage_path(&self) -> &Path {
        &self.storage_path
    }

    pub fn snapshot(&self) -> PersistedServers {
        self.data.clone()
    }

    pub fn has_servers(&self) -> bool {
        !self.data.servers.is_empty()
    }

    pub fn active_server(&self) -> Option<ServerProfile> {
        let active_id = self.data.active_server_id.as_deref()?;
        self.data
            .servers
            .iter()
            .find(|s| s.id == active_id)
            .cloned()
    }

    pub fn upsert_server(
        &mut self,
        id: Option<String>,
        name: String,
        url: String,
        allow_insecure_tls: bool,
        activate: Option<bool>,
    ) -> Result<UpsertOutcome, ConfigError> {
        let normalized_url = normalize_server_url(&url)?;
        let resolved_name = {
            let trimmed = name.trim();
            if trimmed.is_empty() {
                default_server_name(&normalized_url)
            } else {
                trimmed.to_string()
            }
        };

        let prev_active_id = self.data.active_server_id.clone();
        let mut next = self.data.clone();

        let existing_idx = if let Some(ref target_id) = id {
            let trimmed_id = target_id.trim();
            if trimmed_id.is_empty() {
                None
            } else {
                next.servers.iter().position(|s| s.id == trimmed_id)
            }
        } else {
            next.servers.iter().position(|s| s.url == normalized_url)
        };

        let server_id = if let Some(idx) = existing_idx {
            next.servers[idx].name = resolved_name;
            next.servers[idx].url = normalized_url;
            next.servers[idx].allow_insecure_tls = allow_insecure_tls;
            next.servers[idx].id.clone()
        } else {
            let new_id = id
                .map(|v| v.trim().to_string())
                .filter(|v| !v.is_empty())
                .unwrap_or_else(|| Uuid::new_v4().to_string());
            next.servers.push(ServerProfile {
                id: new_id.clone(),
                name: resolved_name,
                url: normalized_url,
                allow_insecure_tls,
                last_connected_at: None,
            });
            new_id
        };

        let should_activate = activate.unwrap_or_else(|| next.active_server_id.is_none());
        if should_activate {
            next.active_server_id = Some(server_id.clone());
            if let Some(entry) = next.servers.iter_mut().find(|s| s.id == server_id) {
                entry.last_connected_at = Some(unix_timestamp_secs());
            }
        }

        persist_atomic(&self.storage_path, &next)?;
        self.data = next;

        let server = self
            .data
            .servers
            .iter()
            .find(|s| s.id == server_id)
            .cloned()
            .expect("upserted server must exist");
        let active_changed = prev_active_id != self.data.active_server_id;

        Ok(UpsertOutcome {
            server,
            snapshot: self.data.clone(),
            active_changed,
        })
    }

    pub fn activate_server(&mut self, id: &str) -> Result<UpsertOutcome, ConfigError> {
        let prev_active_id = self.data.active_server_id.clone();
        let mut next = self.data.clone();

        let Some(entry) = next.servers.iter_mut().find(|s| s.id == id) else {
            return Err(ConfigError::NotFound(format!("Server '{id}' not found")));
        };
        entry.last_connected_at = Some(unix_timestamp_secs());
        let server = entry.clone();
        next.active_server_id = Some(server.id.clone());

        persist_atomic(&self.storage_path, &next)?;
        self.data = next;

        let active_changed = prev_active_id != self.data.active_server_id;
        Ok(UpsertOutcome {
            server,
            snapshot: self.data.clone(),
            active_changed,
        })
    }

    pub fn delete_server(&mut self, id: &str) -> Result<DeleteOutcome, ConfigError> {
        let mut next = self.data.clone();
        let before = next.servers.len();
        next.servers.retain(|s| s.id != id);
        if next.servers.len() == before {
            return Err(ConfigError::NotFound(format!("Server '{id}' not found")));
        }

        let cleared_active = if next.active_server_id.as_deref() == Some(id) {
            next.active_server_id = None;
            true
        } else {
            false
        };

        persist_atomic(&self.storage_path, &next)?;
        self.data = next;

        Ok(DeleteOutcome {
            snapshot: self.data.clone(),
            cleared_active,
        })
    }
}

pub fn normalize_server_url(raw: &str) -> Result<String, ConfigError> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Err(ConfigError::InvalidUrl(
            "Server URL must not be empty".to_string(),
        ));
    }

    let parsed = Url::parse(trimmed)
        .map_err(|e| ConfigError::InvalidUrl(format!("Invalid server URL: {e}")))?;

    match parsed.scheme() {
        "http" | "https" => {}
        scheme => {
            return Err(ConfigError::InvalidUrl(format!(
                "Unsupported URL scheme '{scheme}': only http and https are allowed"
            )));
        }
    }

    if parsed.host_str().is_none_or(|h| h.is_empty()) {
        return Err(ConfigError::InvalidUrl(
            "Server URL must include a valid host".to_string(),
        ));
    }

    if !parsed.username().is_empty() || parsed.password().is_some() {
        return Err(ConfigError::InvalidUrl(
            "Server URL must not contain embedded credentials".to_string(),
        ));
    }

    if parsed.query().is_some() || parsed.fragment().is_some() {
        return Err(ConfigError::InvalidUrl(
            "Server URL must not contain query parameters or fragments".to_string(),
        ));
    }

    let serialized = parsed.as_str().trim_end_matches('/').to_string();
    Ok(serialized)
}

pub fn default_server_name(normalized_url: &str) -> String {
    if let Ok(parsed) = Url::parse(normalized_url)
        && let Some(host) = parsed.host_str()
    {
        return match parsed.port() {
            Some(port) => format!("{host}:{port}"),
            None => host.to_string(),
        };
    }
    normalized_url.to_string()
}

fn unix_timestamp_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

fn persist_atomic(path: &Path, data: &PersistedServers) -> Result<(), ConfigError> {
    if let Some(parent) = path.parent()
        && !parent.as_os_str().is_empty()
    {
        fs::create_dir_all(parent).map_err(|e| {
            ConfigError::Io(format!(
                "Failed to create config directory {}: {e}",
                parent.display()
            ))
        })?;
    }

    let payload = serde_json::to_vec_pretty(data)
        .map_err(|e| ConfigError::Io(format!("Failed to serialize servers config: {e}")))?;

    let file_name = path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("servers.json");
    let tmp_name = format!(".{file_name}.{}.tmp", Uuid::new_v4());
    let tmp_path = path.with_file_name(tmp_name);

    {
        let mut file = fs::File::create(&tmp_path).map_err(|e| {
            ConfigError::Io(format!(
                "Failed to create temporary config file {}: {e}",
                tmp_path.display()
            ))
        })?;
        file.write_all(&payload).map_err(|e| {
            let _ = fs::remove_file(&tmp_path);
            ConfigError::Io(format!("Failed to write temporary config file: {e}"))
        })?;
        file.sync_all().map_err(|e| {
            let _ = fs::remove_file(&tmp_path);
            ConfigError::Io(format!("Failed to sync temporary config file: {e}"))
        })?;
    }

    if let Err(err) = fs::rename(&tmp_path, path) {
        let _ = fs::remove_file(&tmp_path);
        return Err(ConfigError::Io(format!(
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
    fn normalize_server_url_validates_and_strips_trailing_slash() {
        assert_eq!(
            normalize_server_url("  http://192.168.1.10:8080/  ").unwrap(),
            "http://192.168.1.10:8080"
        );
        assert_eq!(
            normalize_server_url("https://media.example.com/xmedia///").unwrap(),
            "https://media.example.com/xmedia"
        );
        assert!(normalize_server_url("").is_err());
        assert!(normalize_server_url("ftp://example.com").is_err());
        assert!(normalize_server_url("http://example.com/?a=1").is_err());
        assert!(normalize_server_url("https://user:pass@example.com").is_err());
    }

    #[test]
    fn server_store_crud_and_atomic_persistence() {
        let tmp = TempDir::new().unwrap();
        let file = tmp.path().join("nested").join("servers.json");

        let mut store = AppServerStore::load(file.clone()).unwrap();
        assert!(!store.has_servers());
        assert!(store.active_server().is_none());

        let first = store
            .upsert_server(
                None,
                String::new(),
                "http://127.0.0.1:8080/".into(),
                false,
                Some(true),
            )
            .unwrap();
        assert_eq!(first.server.name, "127.0.0.1:8080");
        assert_eq!(first.server.url, "http://127.0.0.1:8080");
        assert!(first.server.last_connected_at.is_some());
        assert!(first.active_changed);

        let second = store
            .upsert_server(
                None,
                "NAS".into(),
                "https://nas.local:8443/".into(),
                true,
                Some(false),
            )
            .unwrap();
        assert!(!second.active_changed);
        assert_eq!(
            store.active_server().unwrap().id,
            first.server.id,
            "adding non-active server must keep previous active server"
        );

        let activated = store.activate_server(&second.server.id).unwrap();
        assert!(activated.active_changed);
        assert_eq!(store.active_server().unwrap().id, second.server.id);

        // Reload from disk to verify persistence
        let reloaded = AppServerStore::load(file.clone()).unwrap();
        assert_eq!(reloaded.snapshot().servers.len(), 2);
        assert_eq!(
            reloaded.active_server().unwrap().url,
            "https://nas.local:8443"
        );

        let deleted = store.delete_server(&second.server.id).unwrap();
        assert!(deleted.cleared_active);
        assert!(store.active_server().is_none());
        assert_eq!(store.snapshot().servers.len(), 1);
    }
}

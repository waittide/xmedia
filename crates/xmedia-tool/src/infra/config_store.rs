#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

use async_trait::async_trait;
use tokio::sync::RwLock;
use xmedia_core::domain::error::{DomainError, DomainResult};

use crate::domain::config::ToolSettings;
use crate::domain::ports::ConfigRepositoryPort;
use crate::infra::atomic_fs::atomic_write_private_bytes;
use crate::infra::workspace_fs::open_managed_root;

pub struct JsonFileConfigRepository {
    file_path: PathBuf,
    cache: RwLock<ToolSettings>,
}

impl JsonFileConfigRepository {
    pub fn load_or_init(file_path: PathBuf, cli_roots: &[PathBuf]) -> DomainResult<Self> {
        if let Some(parent) = file_path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| {
                DomainError::Storage(format!(
                    "failed to create tool data directory '{}': {e}",
                    parent.display()
                ))
            })?;
        }

        let mut settings: ToolSettings = if file_path.exists() {
            let raw = std::fs::read(&file_path).map_err(|e| {
                DomainError::Storage(format!(
                    "failed to read config '{}': {e}",
                    file_path.display()
                ))
            })?;
            serde_json::from_slice(&raw).map_err(|e| {
                DomainError::Storage(format!(
                    "failed to parse config '{}' (original file preserved): {e}",
                    file_path.display()
                ))
            })?
        } else {
            ToolSettings::default()
        };

        // Re-canonicalize existing persisted roots & merge CLI `--root` directories
        settings.roots.retain_mut(|r| {
            if let Ok(canon) = std::fs::canonicalize(&r.path)
                && canon.is_dir()
            {
                r.path = canon;
                true
            } else {
                false
            }
        });

        let mut changed = !file_path.exists();
        if settings.rename_templates.upgrade_legacy_defaults() {
            changed = true;
        }
        if settings.scraper_priorities.upgrade_legacy_defaults() {
            changed = true;
        }
        for raw_dir in cli_roots {
            let root = open_managed_root("", raw_dir, None)?;
            if !settings
                .roots
                .iter()
                .any(|existing| existing.path == root.path)
            {
                settings.roots.push(root);
                changed = true;
            }
        }

        if changed {
            persist_to_disk(&file_path, &settings)?;
        } else {
            #[cfg(unix)]
            if file_path.exists() {
                let _ =
                    std::fs::set_permissions(&file_path, std::fs::Permissions::from_mode(0o600));
            }
        }

        Ok(Self {
            file_path,
            cache: RwLock::new(settings),
        })
    }
}

fn persist_to_disk(path: &Path, settings: &ToolSettings) -> DomainResult<()> {
    let serialized = serde_json::to_vec_pretty(settings)
        .map_err(|e| DomainError::Storage(format!("failed serializing tool settings: {e}")))?;
    atomic_write_private_bytes(path, &serialized)
}

#[async_trait]
impl ConfigRepositoryPort for JsonFileConfigRepository {
    async fn load(&self) -> DomainResult<ToolSettings> {
        Ok(self.cache.read().await.clone())
    }

    async fn save(&self, settings: &ToolSettings) -> DomainResult<()> {
        let mut cache = self.cache.write().await;
        persist_to_disk(&self.file_path, settings)?;
        *cache = settings.clone();
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn corrupt_config_returns_error_and_preserves_original_file() {
        let tmp = tempfile::tempdir().unwrap();
        let cfg_path = tmp.path().join("tool-config.json");
        let original = b"{ invalid json content";
        std::fs::write(&cfg_path, original).unwrap();

        let res = JsonFileConfigRepository::load_or_init(cfg_path.clone(), &[]);
        assert!(res.is_err());
        assert_eq!(std::fs::read(&cfg_path).unwrap(), original);
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn tool_config_is_created_and_saved_with_0600_permissions() {
        let tmp = tempfile::tempdir().unwrap();
        let cfg_path = tmp.path().join("tool-config.json");
        let repo = JsonFileConfigRepository::load_or_init(cfg_path.clone(), &[]).unwrap();
        let mode = std::fs::metadata(&cfg_path).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o600);

        // Even if permissions were loosened externally, save() restores 0600 atomically
        std::fs::set_permissions(&cfg_path, std::fs::Permissions::from_mode(0o644)).unwrap();
        let cur = repo.load().await.unwrap();
        repo.save(&cur).await.unwrap();
        let mode_after = std::fs::metadata(&cfg_path).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode_after, 0o600);
    }
}

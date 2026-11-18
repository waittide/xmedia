use std::path::{Path, PathBuf};

use async_trait::async_trait;
use tokio::sync::RwLock;
use xmedia_core::domain::error::{DomainError, DomainResult};

use crate::domain::config::ToolSettings;
use crate::domain::ports::ConfigRepositoryPort;
use crate::domain::workspace::ManagedRoot;
use crate::infra::atomic_fs::atomic_write_bytes;

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
            serde_json::from_slice(&raw).unwrap_or_default()
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
            if let Ok(root) = ManagedRoot::new("", raw_dir, None)
                && !settings
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
    atomic_write_bytes(path, &serialized)
}

#[async_trait]
impl ConfigRepositoryPort for JsonFileConfigRepository {
    async fn load(&self) -> DomainResult<ToolSettings> {
        Ok(self.cache.read().await.clone())
    }

    async fn save(&self, settings: &ToolSettings) -> DomainResult<()> {
        persist_to_disk(&self.file_path, settings)?;
        *self.cache.write().await = settings.clone();
        Ok(())
    }
}

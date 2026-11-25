use std::path::{Path, PathBuf};
use std::sync::{Arc, RwLock};

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AppIconVariant {
    #[default]
    Cobalt,
    Porcelain,
    Obsidian,
    Monolith,
}

impl AppIconVariant {
    pub const ALL: [Self; 4] = [
        Self::Cobalt,
        Self::Porcelain,
        Self::Obsidian,
        Self::Monolith,
    ];

    pub fn parse(raw: &str) -> Option<Self> {
        match raw.trim().to_ascii_lowercase().as_str() {
            "cobalt" => Some(Self::Cobalt),
            "porcelain" => Some(Self::Porcelain),
            "obsidian" => Some(Self::Obsidian),
            "monolith" => Some(Self::Monolith),
            _ => None,
        }
    }

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Cobalt => "cobalt",
            Self::Porcelain => "porcelain",
            Self::Obsidian => "obsidian",
            Self::Monolith => "monolith",
        }
    }

    #[cfg(desktop)]
    pub const fn desktop_png_bytes(self) -> &'static [u8] {
        match self {
            Self::Cobalt => include_bytes!("../icons/variants/cobalt-128.png"),
            Self::Porcelain => include_bytes!("../icons/variants/porcelain-128.png"),
            Self::Obsidian => include_bytes!("../icons/variants/obsidian-128.png"),
            Self::Monolith => include_bytes!("../icons/variants/monolith-128.png"),
        }
    }
}

pub type IconApplier = Arc<dyn Fn(AppIconVariant) + Send + Sync>;

#[derive(Clone)]
pub struct AppIconStore {
    path: PathBuf,
    current: Arc<RwLock<AppIconVariant>>,
    write_lock: Arc<tokio::sync::Mutex<()>>,
    applier: Option<IconApplier>,
}

impl AppIconStore {
    pub fn open(path: PathBuf, applier: Option<IconApplier>) -> Self {
        let initial = read_persisted_icon(&path).unwrap_or_default();
        Self {
            path,
            current: Arc::new(RwLock::new(initial)),
            write_lock: Arc::new(tokio::sync::Mutex::new(())),
            applier,
        }
    }

    pub fn current(&self) -> AppIconVariant {
        self.current.read().map(|g| *g).unwrap_or_default()
    }

    pub fn set(&self, variant: AppIconVariant) -> anyhow::Result<AppIconVariant> {
        persist_icon_atomically(&self.path, variant)?;
        self.commit_in_memory(variant);
        Ok(variant)
    }

    pub async fn set_async(&self, variant: AppIconVariant) -> anyhow::Result<AppIconVariant> {
        let _guard = self.write_lock.lock().await;
        let path = self.path.clone();
        tokio::task::spawn_blocking(move || persist_icon_atomically(&path, variant))
            .await
            .map_err(|e| anyhow::anyhow!("app icon persistence task failed: {e}"))??;
        self.commit_in_memory(variant);
        Ok(variant)
    }

    fn commit_in_memory(&self, variant: AppIconVariant) {
        if let Ok(mut guard) = self.current.write() {
            *guard = variant;
        }
        if let Some(ref apply) = self.applier {
            apply(variant);
        }
    }
}

fn read_persisted_icon(path: &Path) -> Option<AppIconVariant> {
    let raw = std::fs::read_to_string(path).ok()?;
    AppIconVariant::parse(&raw)
}

fn persist_icon_atomically(path: &Path, variant: AppIconVariant) -> anyhow::Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let tmp = path.with_extension("tmp");
    std::fs::write(&tmp, format!("{}\n", variant.as_str()))?;
    std::fs::rename(&tmp, path)?;
    Ok(())
}

#[cfg(desktop)]
pub fn apply_desktop_window_icon(app_handle: &tauri::AppHandle, variant: AppIconVariant) {
    use tauri::Manager;

    let Some(window) = app_handle.get_webview_window("main") else {
        return;
    };
    match tauri::image::Image::from_bytes(variant.desktop_png_bytes()) {
        Ok(image) => {
            if let Err(err) = window.set_icon(image) {
                tracing::warn!(
                    icon = variant.as_str(),
                    error = %err,
                    "failed to update desktop window icon"
                );
            }
        }
        Err(err) => {
            tracing::warn!(
                icon = variant.as_str(),
                error = %err,
                "failed to decode embedded desktop icon PNG"
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicUsize, Ordering};

    use super::*;

    #[test]
    fn parses_known_variants_and_rejects_unknown() {
        assert_eq!(
            AppIconVariant::parse("cobalt"),
            Some(AppIconVariant::Cobalt)
        );
        assert_eq!(
            AppIconVariant::parse(" Porcelain "),
            Some(AppIconVariant::Porcelain)
        );
        assert_eq!(
            AppIconVariant::parse("OBSIDIAN"),
            Some(AppIconVariant::Obsidian)
        );
        assert_eq!(
            AppIconVariant::parse("monolith"),
            Some(AppIconVariant::Monolith)
        );
        assert_eq!(AppIconVariant::parse("unknown"), None);
        assert_eq!(AppIconVariant::parse(""), None);
    }

    #[test]
    fn persists_and_reloads_selected_variant() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("app_icon.txt");
        let calls = Arc::new(AtomicUsize::new(0));
        let calls_clone = Arc::clone(&calls);

        let store = AppIconStore::open(
            path.clone(),
            Some(Arc::new(move |_| {
                calls_clone.fetch_add(1, Ordering::SeqCst);
            })),
        );
        assert_eq!(store.current(), AppIconVariant::Cobalt);

        store.set(AppIconVariant::Obsidian).expect("set obsidian");
        assert_eq!(store.current(), AppIconVariant::Obsidian);
        assert_eq!(calls.load(Ordering::SeqCst), 1);

        let reloaded = AppIconStore::open(path, None);
        assert_eq!(reloaded.current(), AppIconVariant::Obsidian);
    }

    #[cfg(desktop)]
    #[test]
    fn embedded_desktop_variant_pngs_decode_cleanly() {
        for variant in AppIconVariant::ALL {
            let img = tauri::image::Image::from_bytes(variant.desktop_png_bytes())
                .unwrap_or_else(|e| panic!("failed to decode {} PNG: {e}", variant.as_str()));
            assert_eq!(img.width(), 128);
            assert_eq!(img.height(), 128);
        }
    }
}

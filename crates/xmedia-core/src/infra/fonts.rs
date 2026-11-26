#[cfg(test)]
mod tests;

use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use tokio::io::AsyncWriteExt;
use tokio::sync::RwLock;
use uuid::Uuid;
use xmedia_formats::font::inspect_font_bytes;

use crate::application::dto::CustomFontDto;
use crate::application::ports::{BoxFuture, FontRepository};
use crate::domain::error::{DomainError, DomainResult};

pub const MAX_FONT_FILE_BYTES: u64 = 64 * 1024 * 1024;
pub const MAX_STORED_FONTS: usize = 64;
const MANIFEST_FILE_NAME: &str = "manifest.json";
const MAX_DISPLAY_NAME_CHARS: usize = 128;
const MAX_FILE_NAME_CHARS: usize = 255;

#[derive(Debug, Default, Serialize, Deserialize)]
struct FontManifest {
    #[serde(default)]
    fonts: Vec<CustomFontDto>,
}

#[derive(Debug)]
pub struct FsFontStore {
    fonts_dir: PathBuf,
    lock: RwLock<()>,
}

impl FsFontStore {
    pub fn new(fonts_dir: impl Into<PathBuf>) -> Self {
        let fonts_dir = fonts_dir.into();
        let _ = std::fs::create_dir_all(&fonts_dir);
        Self {
            fonts_dir,
            lock: RwLock::new(()),
        }
    }

    pub fn fonts_dir(&self) -> &Path {
        &self.fonts_dir
    }

    fn manifest_path(&self) -> PathBuf {
        self.fonts_dir.join(MANIFEST_FILE_NAME)
    }

    fn font_file_path(&self, dto: &CustomFontDto) -> PathBuf {
        self.fonts_dir
            .join(format!("{}.{}", dto.id, dto.format.extension()))
    }

    async fn read_manifest_unlocked(&self) -> DomainResult<FontManifest> {
        let path = self.manifest_path();
        match tokio::fs::read(&path).await {
            Ok(bytes) => {
                if bytes.iter().all(|b| b.is_ascii_whitespace()) {
                    return Ok(FontManifest::default());
                }
                serde_json::from_slice::<FontManifest>(&bytes).map_err(|e| {
                    DomainError::Storage(format!(
                        "failed to parse font manifest '{}': {e}",
                        path.display()
                    ))
                })
            }
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(FontManifest::default()),
            Err(err) => Err(DomainError::Storage(format!(
                "failed to read font manifest '{}': {err}",
                path.display()
            ))),
        }
    }

    async fn write_manifest_unlocked(&self, manifest: &FontManifest) -> DomainResult<()> {
        let payload = serde_json::to_vec_pretty(manifest)
            .map_err(|e| DomainError::Storage(format!("failed to serialize font manifest: {e}")))?;
        atomic_write_in_dir(&self.fonts_dir, "manifest", &self.manifest_path(), &payload).await
    }

    pub async fn list(&self) -> DomainResult<Vec<CustomFontDto>> {
        let _guard = self.lock.read().await;
        let manifest = self.read_manifest_unlocked().await?;
        Ok(manifest.fonts)
    }

    pub async fn add(
        &self,
        file_name: &str,
        custom_name: Option<&str>,
        bytes: &[u8],
    ) -> DomainResult<CustomFontDto> {
        if bytes.is_empty() {
            return Err(DomainError::Validation("font file cannot be empty".into()));
        }
        let size_bytes = bytes.len() as u64;
        if size_bytes > MAX_FONT_FILE_BYTES {
            return Err(DomainError::Validation(format!(
                "font file exceeds maximum allowed size ({MAX_FONT_FILE_BYTES} bytes)"
            )));
        }

        let detected = inspect_font_bytes(file_name, bytes)?;
        let name = custom_name
            .and_then(sanitize_custom_font_name)
            .unwrap_or(detected.family_name);
        let clean_file_name = sanitize_upload_file_name(file_name)
            .unwrap_or_else(|| format!("{name}.{}", detected.format.extension()));

        let _guard = self.lock.write().await;
        let mut manifest = self.read_manifest_unlocked().await?;
        if manifest.fonts.len() >= MAX_STORED_FONTS {
            return Err(DomainError::Validation(format!(
                "maximum number of stored custom fonts ({MAX_STORED_FONTS}) reached"
            )));
        }

        let id = Uuid::now_v7();
        let created_at = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs()
            .to_string();
        let dto = CustomFontDto {
            id,
            name,
            file_name: clean_file_name,
            format: detected.format,
            css_format: detected.format.css_format().to_string(),
            mime_type: detected.format.mime_type().to_string(),
            size_bytes,
            created_at,
        };

        let target_font_path = self.font_file_path(&dto);
        atomic_write_in_dir(&self.fonts_dir, "font", &target_font_path, bytes).await?;

        manifest.fonts.push(dto.clone());
        if let Err(err) = self.write_manifest_unlocked(&manifest).await {
            let _ = tokio::fs::remove_file(&target_font_path).await;
            return Err(err);
        }

        Ok(dto)
    }

    pub async fn delete(&self, id: Uuid) -> DomainResult<()> {
        let _guard = self.lock.write().await;
        let mut manifest = self.read_manifest_unlocked().await?;
        let Some(pos) = manifest.fonts.iter().position(|f| f.id == id) else {
            return Err(DomainError::NotFound(format!("custom font {id} not found")));
        };
        let removed = manifest.fonts.remove(pos);
        self.write_manifest_unlocked(&manifest).await?;

        let font_path = self.font_file_path(&removed);
        match tokio::fs::remove_file(&font_path).await {
            Ok(()) => {}
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => {}
            Err(err) => {
                tracing::warn!(
                    path = %font_path.display(),
                    error = %err,
                    "failed to remove deleted custom font file"
                );
            }
        }
        Ok(())
    }

    pub async fn open_file(&self, id: Uuid) -> DomainResult<(CustomFontDto, Vec<u8>)> {
        let _guard = self.lock.read().await;
        let manifest = self.read_manifest_unlocked().await?;
        let dto = manifest
            .fonts
            .into_iter()
            .find(|f| f.id == id)
            .ok_or_else(|| DomainError::NotFound(format!("custom font {id} not found")))?;
        let font_path = self.font_file_path(&dto);
        let bytes = tokio::fs::read(&font_path).await.map_err(|e| {
            if e.kind() == std::io::ErrorKind::NotFound {
                DomainError::NotFound(format!("custom font binary {id} is missing"))
            } else {
                DomainError::Storage(format!(
                    "failed to read custom font file '{}': {e}",
                    font_path.display()
                ))
            }
        })?;
        Ok((dto, bytes))
    }
}

impl FontRepository for FsFontStore {
    fn list(&self) -> BoxFuture<'_, DomainResult<Vec<CustomFontDto>>> {
        Box::pin(Self::list(self))
    }

    fn add<'a>(
        &'a self,
        file_name: &'a str,
        custom_name: Option<&'a str>,
        bytes: &'a [u8],
    ) -> BoxFuture<'a, DomainResult<CustomFontDto>> {
        Box::pin(Self::add(self, file_name, custom_name, bytes))
    }

    fn delete(&self, id: Uuid) -> BoxFuture<'_, DomainResult<()>> {
        Box::pin(Self::delete(self, id))
    }

    fn open_file(&self, id: Uuid) -> BoxFuture<'_, DomainResult<(CustomFontDto, Vec<u8>)>> {
        Box::pin(Self::open_file(self, id))
    }
}

fn sanitize_custom_font_name(raw: &str) -> Option<String> {
    let filtered: String = raw
        .chars()
        .filter(|&c| {
            !c.is_control() && !matches!(c, ';' | '{' | '}' | '<' | '>' | '\\' | '"' | '\'')
        })
        .collect();
    let trimmed = filtered.trim();
    if trimmed.is_empty() {
        return None;
    }
    let capped: String = trimmed.chars().take(MAX_DISPLAY_NAME_CHARS).collect();
    let out = capped.trim_end();
    if out.is_empty() {
        None
    } else {
        Some(out.to_string())
    }
}

fn sanitize_upload_file_name(raw: &str) -> Option<String> {
    let base = raw.rsplit(['/', '\\']).next().unwrap_or(raw);
    let filtered: String = base.chars().filter(|&c| !c.is_control()).collect();
    let trimmed = filtered.trim();
    if trimmed.is_empty() || trimmed == "." || trimmed == ".." {
        return None;
    }
    let capped: String = trimmed.chars().take(MAX_FILE_NAME_CHARS).collect();
    Some(capped)
}

async fn atomic_write_in_dir(
    dir: &Path,
    prefix: &str,
    target: &Path,
    bytes: &[u8],
) -> DomainResult<()> {
    tokio::fs::create_dir_all(dir).await.map_err(|e| {
        DomainError::Storage(format!(
            "failed to create fonts directory '{}': {e}",
            dir.display()
        ))
    })?;
    let tmp_path = dir.join(format!(".{prefix}-{}.tmp", Uuid::now_v7()));
    let write_res = async {
        let mut file = tokio::fs::File::create(&tmp_path).await?;
        file.write_all(bytes).await?;
        file.flush().await?;
        file.sync_all().await?;
        Ok::<(), std::io::Error>(())
    }
    .await;

    if let Err(err) = write_res {
        let _ = tokio::fs::remove_file(&tmp_path).await;
        return Err(DomainError::Storage(format!(
            "failed writing staging file '{}': {err}",
            tmp_path.display()
        )));
    }

    if let Err(err) = tokio::fs::rename(&tmp_path, target).await {
        let _ = tokio::fs::remove_file(&tmp_path).await;
        return Err(DomainError::Storage(format!(
            "failed atomically replacing '{}': {err}",
            target.display()
        )));
    }
    Ok(())
}

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, SystemTime};

use serde::Serialize;
use walkdir::WalkDir;
use xmedia_core::domain::error::{DomainError, DomainResult};

use crate::application::ports::{HttpWebhookPort, WorkspaceFsPort};
use crate::application::scraping::{AutoScrapeOptions, ScrapeApplyService};
use crate::application::settings::ToolSettingsService;
use crate::application::workbench::WorkspaceBrowserService;
use crate::domain::workspace::path_to_slash;

const INCOMPLETE_DOWNLOAD_SUFFIXES: &[&str] = &[
    ".part",
    ".tmp",
    ".crdownload",
    ".!qb",
    ".downloading",
    ".aria2",
];

#[derive(Debug, Clone, Serialize)]
pub struct InboxImportedItemDto {
    pub source_rel: String,
    pub target_rel: String,
    pub mode: String, // "hardlink" | "move" | "copy"
    pub scraped: bool,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct InboxScanReportDto {
    pub imported_count: usize,
    pub skipped_count: usize,
    pub failed_count: usize,
    pub items: Vec<InboxImportedItemDto>,
}

pub struct InboxScanDeps<'a> {
    pub settings: &'a ToolSettingsService,
    pub workspace: &'a WorkspaceBrowserService,
    pub scrape_apply: &'a ScrapeApplyService,
    pub fs: &'a dyn WorkspaceFsPort,
    pub webhook: &'a Arc<dyn HttpWebhookPort>,
}

fn is_incomplete_download(path: &Path) -> bool {
    let lower = path
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase();
    if lower.starts_with('.') {
        return true;
    }
    INCOMPLETE_DOWNLOAD_SUFFIXES
        .iter()
        .any(|suf| lower.ends_with(suf))
}

fn is_recently_modified(path: &Path, settle_secs: u64) -> bool {
    if settle_secs == 0 {
        return false;
    }
    let Ok(meta) = std::fs::metadata(path) else {
        return false;
    };
    let Ok(modified) = meta.modified() else {
        return false;
    };
    SystemTime::now()
        .duration_since(modified)
        .is_ok_and(|d| d < Duration::from_secs(settle_secs))
}

fn import_single_file(
    src: &Path,
    dst: &Path,
    use_hardlink: bool,
    delete_source: bool,
) -> DomainResult<&'static str> {
    if let Some(parent) = dst.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| DomainError::Storage(format!("create inbox target dir failed: {e}")))?;
    }

    if use_hardlink && !delete_source && std::fs::hard_link(src, dst).is_ok() {
        return Ok("hardlink");
    }

    if delete_source {
        if std::fs::rename(src, dst).is_ok() {
            return Ok("move");
        }
        std::fs::copy(src, dst)
            .map_err(|e| DomainError::Storage(format!("cross-volume copy failed: {e}")))?;
        let _ = std::fs::remove_file(src);
        return Ok("move");
    }

    std::fs::copy(src, dst)
        .map_err(|e| DomainError::Storage(format!("copy inbox file failed: {e}")))?;
    Ok("copy")
}

/// Scans the configured Inbox / Watch Folder and imports supported media files into
/// the target `ManagedRoot` using hardlink (preferred for seeding), move, or copy.
pub async fn scan_and_import_inbox(
    deps: InboxScanDeps<'_>,
    settle_secs: u64,
) -> DomainResult<InboxScanReportDto> {
    let settings = deps.settings.get_raw().await?;
    let inbox_cfg = &settings.inbox;
    let raw_inbox = inbox_cfg
        .inbox_path
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .ok_or_else(|| DomainError::Validation("inbox_path is not configured".into()))?;
    let target_root_id = inbox_cfg
        .target_root_id
        .ok_or_else(|| DomainError::Validation("inbox target_root_id is not configured".into()))?;

    let target_root = settings.find_root(target_root_id)?.clone();
    target_root.ensure_writable()?;

    let inbox_dir =
        deps.settings
            .validate_inbox_directory(raw_inbox, &settings.roots, Some(target_root_id))?;

    let mut candidates: Vec<(PathBuf, String)> = Vec::new();
    let mut skipped_count = 0usize;

    for entry in WalkDir::new(&inbox_dir)
        .follow_links(false)
        .into_iter()
        .flatten()
    {
        if !entry.file_type().is_file() {
            continue;
        }
        let p = entry.path();
        if is_incomplete_download(p) || is_recently_modified(p, settle_secs) {
            skipped_count += 1;
            continue;
        }
        if deps
            .workspace
            .resolve_inspector(p, target_root.default_kind)
            .is_none()
        {
            skipped_count += 1;
            continue;
        }
        let rel = p
            .strip_prefix(&inbox_dir)
            .map(path_to_slash)
            .unwrap_or_else(|_| {
                p.file_name()
                    .map(|s| s.to_string_lossy().into_owned())
                    .unwrap_or_default()
            });
        candidates.push((p.to_path_buf(), rel));
    }

    let mut items = Vec::with_capacity(candidates.len());
    let mut imported_count = 0usize;
    let mut failed_count = 0usize;

    for (src_abs, src_rel) in candidates {
        let target_abs = match deps.fs.resolve_target_in_root(&target_root, &src_rel) {
            Ok(p) => p,
            Err(e) => {
                failed_count += 1;
                items.push(InboxImportedItemDto {
                    source_rel: src_rel,
                    target_rel: String::new(),
                    mode: String::new(),
                    scraped: false,
                    error: Some(e.to_string()),
                });
                continue;
            }
        };

        if target_abs.exists() {
            skipped_count += 1;
            continue;
        }

        let src_parent = src_abs.parent().map(Path::to_path_buf);
        match import_single_file(
            &src_abs,
            &target_abs,
            inbox_cfg.use_hardlink,
            inbox_cfg.delete_source,
        ) {
            Ok(mode) => {
                imported_count += 1;
                if inbox_cfg.delete_source
                    && let Some(ref p) = src_parent
                {
                    deps.fs.cleanup_empty_parent_dirs(p, &inbox_dir);
                }

                let mut final_rel = src_rel.clone();
                let mut scraped = false;
                let mut scrape_err = None;
                if inbox_cfg.auto_scrape
                    && let Some(ins) = deps
                        .workspace
                        .resolve_inspector(&target_abs, target_root.default_kind)
                {
                    let opts = AutoScrapeOptions {
                        organize_by_template: inbox_cfg.organize_by_template,
                        ..Default::default()
                    };
                    match deps
                        .scrape_apply
                        .auto_scrape(target_root_id, &src_rel, ins.media_kind(), opts)
                        .await
                    {
                        Ok(res) => {
                            scraped = true;
                            final_rel = res.detail.entry.rel_path;
                        }
                        Err(e) => {
                            scrape_err = Some(e.to_string());
                        }
                    }
                }

                items.push(InboxImportedItemDto {
                    source_rel: src_rel,
                    target_rel: final_rel,
                    mode: mode.to_string(),
                    scraped,
                    error: scrape_err,
                });
            }
            Err(e) => {
                failed_count += 1;
                items.push(InboxImportedItemDto {
                    source_rel: src_rel,
                    target_rel: String::new(),
                    mode: String::new(),
                    scraped: false,
                    error: Some(e.to_string()),
                });
            }
        }
    }

    if imported_count > 0 && settings.xmedia_webhook.notify_on_inbox {
        let webhook_cfg = settings.xmedia_webhook.clone();
        let root_path = target_root.path.clone();
        let webhook = Arc::clone(deps.webhook);
        tokio::spawn(async move {
            webhook.notify_rescan(&webhook_cfg, &root_path).await;
        });
    }

    Ok(InboxScanReportDto {
        imported_count,
        skipped_count,
        failed_count,
        items,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn inbox_skips_partial_downloads_and_hardlinks_completed_files() {
        let tmp = tempfile::tempdir().unwrap();
        let src = tmp.path().join("movie.mkv");
        let partial = tmp.path().join("downloading.mkv.part");
        let dst = tmp.path().join("out").join("movie.mkv");
        std::fs::write(&src, b"video-data").unwrap();
        std::fs::write(&partial, b"partial").unwrap();

        assert!(is_incomplete_download(&partial));
        assert!(!is_incomplete_download(&src));

        let mode = import_single_file(&src, &dst, true, false).unwrap();
        assert_eq!(mode, "hardlink");
        assert!(src.is_file());
        assert!(dst.is_file());
        assert_eq!(std::fs::read(&dst).unwrap(), b"video-data");
    }
}

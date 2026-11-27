use std::collections::HashMap;
use std::sync::Arc;

use tracing::warn;
use xmedia_core::domain::error::{DomainError, DomainResult};
use xmedia_core::domain::library::MediaKind;

use super::ScrapeApplyService;
use super::mapping::map_scraped_to_patch;
use crate::application::ops::{
    JellyfinArchiveInput, ToolboxCommand, ToolboxOutput, VideoToolCommand, VideoToolOutput,
};
use crate::application::workbench::MetadataHistoryEvent;
use crate::domain::media::{MediaFileDetail, MediaMetadataPatch, MediaMetadataPayload};
use crate::domain::scraper::ScrapedMetadata;
use crate::domain::template::render_template_target;
use crate::domain::workspace::RootId;

impl ScrapeApplyService {
    pub(super) async fn apply_video_jellyfin_archive(
        &self,
        root_id: RootId,
        rel_path: &str,
        scraped: ScrapedMetadata,
        apply_cover: bool,
        only_fill_missing: bool,
    ) -> DomainResult<MediaFileDetail> {
        let root_lock = self.root_locks.lock_for(root_id);
        let root_guard = root_lock.lock().await;
        let raw_settings = self.settings.get_raw().await?;
        let root = raw_settings.find_root(root_id)?.clone();
        root.ensure_writable()?;

        let src_sandbox = self.fs.resolve_existing_in_root(&root, rel_path)?;
        if !src_sandbox.abs_path.is_file() {
            return Err(DomainError::Validation(format!(
                "'{}' is not a regular video file",
                src_sandbox.rel_slash_string()
            )));
        }

        let patch = map_scraped_to_patch(MediaKind::Video, &scraped, false, false);
        let MediaMetadataPatch::Video(vp) = patch else {
            unreachable!();
        };

        let src_rel_slash = src_sandbox.rel_slash_string();
        let src_abs_for_read = src_sandbox.abs_path.clone();
        let toolbox_for_read = Arc::clone(&self.toolbox);
        let out = tokio::task::spawn_blocking(move || {
            toolbox_for_read.execute(ToolboxCommand::Video(VideoToolCommand::ReadBaseMeta {
                path: src_abs_for_read,
            }))
        })
        .await
        .map_err(|e| DomainError::Storage(format!("video base inspect failed: {e}")))??;
        let ToolboxOutput::Video(VideoToolOutput::BaseMeta(mut meta)) = out else {
            return Err(DomainError::Storage(
                "unexpected video base metadata output".into(),
            ));
        };

        let pre_scrape_meta = (*meta).clone();
        let existing_poster = meta.has_poster;
        let existing_fanart = meta.has_fanart;
        let existing_thumb = meta.has_thumb;
        let fallback_stem = src_sandbox
            .abs_path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("");
        let mut effective_apply_cover = apply_cover;
        meta.prepare_and_apply_patch(
            vp,
            only_fill_missing,
            fallback_stem,
            &mut effective_apply_cover,
        );
        if meta.is_episode && meta.season_number.is_none() {
            meta.season_number = Some(1);
        }

        let configured_tpl = if meta.is_episode {
            raw_settings.rename_templates.video_episode.trim()
        } else {
            raw_settings.rename_templates.video_movie.trim()
        };
        let effective_tpl = if configured_tpl.contains('/') || configured_tpl.contains('\\') {
            configured_tpl.to_string()
        } else if meta.is_episode {
            format!("{{show}} ({{year}})/Season {{season:2}}/{configured_tpl}")
        } else {
            format!("{{title}} ({{year}})/{configured_tpl}")
        };

        let ext = src_sandbox
            .abs_path
            .extension()
            .and_then(|s| s.to_str())
            .unwrap_or("mp4");
        let (target_rel_path, target_stem) = render_template_target(
            &effective_tpl,
            "",
            &MediaMetadataPayload::Video((*meta).clone()),
            &src_sandbox.abs_path,
            ext,
        );
        let dst_abs = self.fs.resolve_target_in_root(&root, &target_rel_path)?;
        if dst_abs != src_sandbox.abs_path && dst_abs.exists() {
            return Err(DomainError::Conflict(format!(
                "target '{}' already exists on disk",
                target_rel_path
            )));
        }

        let mut downloaded_images: HashMap<String, (Vec<u8>, &'static str)> = HashMap::new();
        let skip_poster = !effective_apply_cover || (only_fill_missing && existing_poster);
        let skip_fanart = !effective_apply_cover || (only_fill_missing && existing_fanart);
        let skip_thumb = !effective_apply_cover || (only_fill_missing && existing_thumb);
        if effective_apply_cover {
            let urls_to_fetch: [Option<&str>; 4] = [
                (!skip_poster)
                    .then_some(scraped.cover_url.as_deref())
                    .flatten(),
                (!skip_fanart)
                    .then_some(scraped.backdrop_url.as_deref())
                    .flatten(),
                (!skip_poster)
                    .then_some(scraped.season_poster_url.as_deref())
                    .flatten(),
                (!skip_thumb)
                    .then_some(scraped.episode_still_url.as_deref())
                    .flatten(),
            ];
            for raw_url in urls_to_fetch.into_iter().flatten() {
                let u = raw_url.trim();
                if u.is_empty() || downloaded_images.contains_key(u) {
                    continue;
                }
                match self
                    .transport
                    .download_artwork(&raw_settings.credentials, u, self.allow_private_artwork_ips)
                    .await
                {
                    Ok((bytes, mime)) => {
                        let img_ext = if mime.contains("png") {
                            "png"
                        } else if mime.contains("webp") {
                            "webp"
                        } else {
                            "jpg"
                        };
                        downloaded_images.insert(u.to_string(), (bytes, img_ext));
                    }
                    Err(err) => {
                        warn!("skipping failed video artwork '{u}': {err}");
                    }
                }
            }
        }

        let archive_input = JellyfinArchiveInput {
            root_path: root.path.clone(),
            src_abs: src_sandbox.abs_path,
            dst_abs,
            target_stem,
            meta: *meta,
            scraped,
            downloaded_images,
            skip_poster,
            skip_fanart,
            skip_thumb,
            only_fill_missing,
        };
        let toolbox_exec = Arc::clone(&self.toolbox);
        tokio::task::spawn_blocking(move || {
            toolbox_exec.execute(ToolboxCommand::Video(VideoToolCommand::ArchiveJellyfin(
                Box::new(archive_input),
            )))
        })
        .await
        .map_err(|e| DomainError::Storage(format!("video Jellyfin archive task failed: {e}")))??;
        drop(root_guard);

        if target_rel_path != src_rel_slash {
            self.metadata
                .record_history_event(MetadataHistoryEvent::Relocated {
                    root_id,
                    moves: vec![(src_rel_slash, target_rel_path.clone())],
                })
                .await;
        }
        self.metadata
            .record_history_event(MetadataHistoryEvent::Snapshot {
                root_id,
                media_kind: MediaKind::Video,
                rel_path: target_rel_path.clone(),
                previous: Box::new(MediaMetadataPayload::Video(pre_scrape_meta)),
            })
            .await;

        if raw_settings.xmedia_webhook.notify_on_scrape {
            let webhook_cfg = raw_settings.xmedia_webhook;
            let root_path = root.path;
            let webhook = Arc::clone(&self.webhook);
            tokio::spawn(async move {
                webhook.notify_rescan(&webhook_cfg, &root_path).await;
            });
        }

        self.workspace
            .inspect(root_id, &target_rel_path, Some(MediaKind::Video))
            .await
    }
}

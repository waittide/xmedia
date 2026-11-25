use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;

use tracing::warn;
use xmedia_core::domain::error::{DomainError, DomainResult};
use xmedia_core::domain::library::MediaKind;

use super::ScrapingOrchestratorService;
use super::mapping::map_scraped_to_patch;
use crate::domain::media::{MediaFileDetail, MediaMetadataPatch, MediaMetadataPayload};
use crate::domain::scraper::ScrapedMetadata;
use crate::domain::template::render_template_target;
use crate::domain::workspace::RootId;

impl ScrapingOrchestratorService {
    pub(super) async fn apply_video_jellyfin_archive(
        &self,
        root_id: RootId,
        rel_path: &str,
        scraped: ScrapedMetadata,
        apply_cover: bool,
        only_fill_missing: bool,
    ) -> DomainResult<MediaFileDetail> {
        let root_lock = self.workbench.root_write_lock(root_id);
        let root_guard = root_lock.lock().await;
        let raw_settings = self.settings.get_raw().await?;
        let root = raw_settings
            .roots
            .iter()
            .find(|r| r.id == root_id)
            .cloned()
            .ok_or_else(|| DomainError::NotFound(format!("root '{root_id}' not found")))?;
        root.ensure_writable()?;
        let fs = self.workbench.fs();
        let src_sandbox = fs.resolve_existing_in_root(&root, rel_path)?;
        if !src_sandbox.abs_path.is_file() {
            return Err(DomainError::Validation(format!(
                "'{}' is not a regular video file",
                src_sandbox.rel_slash_string()
            )));
        }

        let patch = map_scraped_to_patch(MediaKind::Video, &scraped, false, false);
        let MediaMetadataPatch::Video(mut vp) = patch else {
            unreachable!();
        };

        let src_rel_slash = src_sandbox.rel_slash_string();
        let src_abs_for_read = src_sandbox.abs_path.clone();
        let toolbox_for_read = Arc::clone(self.workbench.toolbox());
        let mut meta = tokio::task::spawn_blocking(move || {
            toolbox_for_read.read_base_video_meta(&src_abs_for_read)
        })
        .await
        .map_err(|e| DomainError::Storage(format!("video base inspect failed: {e}")))?;
        let pre_scrape_meta = meta.clone();
        let existing_poster = meta.has_poster;
        let existing_fanart = meta.has_fanart;
        let existing_thumb = meta.has_thumb;
        if only_fill_missing {
            let fallback_stem = src_sandbox
                .abs_path
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or("");
            meta.filter_patch_for_missing(&mut vp, fallback_stem);
        }
        let mut effective_apply_cover = apply_cover;
        meta.strip_locked_fields(&mut vp, &mut effective_apply_cover);
        meta.apply_patch(vp);
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
            &MediaMetadataPayload::Video(meta.clone()),
            &src_sandbox.abs_path,
            ext,
        );
        let dst_abs = fs.resolve_target_in_root(&root, &target_rel_path)?;
        if dst_abs != src_sandbox.abs_path && dst_abs.exists() {
            return Err(DomainError::Conflict(format!(
                "target '{}' already exists on disk",
                target_rel_path
            )));
        }

        let dst_dir = dst_abs
            .parent()
            .ok_or_else(|| DomainError::Validation("invalid target video folder".into()))?
            .to_path_buf();
        let show_dir = if meta.is_episode
            && self.workbench.toolbox().is_season_dir(&dst_dir)
            && dst_dir != root.path
            && let Some(parent) = dst_dir.parent()
            && parent != root.path
            && parent.starts_with(&root.path)
        {
            parent.to_path_buf()
        } else {
            dst_dir.clone()
        };

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

        let will_replace_poster = !skip_poster
            && scraped
                .cover_url
                .as_deref()
                .map(str::trim)
                .is_some_and(|u| downloaded_images.contains_key(u));
        let will_replace_fanart = !skip_fanart
            && scraped
                .backdrop_url
                .as_deref()
                .map(str::trim)
                .is_some_and(|u| downloaded_images.contains_key(u));
        let will_replace_thumb = !skip_thumb
            && scraped
                .episode_still_url
                .as_deref()
                .map(str::trim)
                .is_some_and(|u| downloaded_images.contains_key(u));

        let root_path_buf = root.path.clone();
        let src_abs = src_sandbox.abs_path.clone();
        let dst_abs_clone = dst_abs.clone();
        let scraped_clone = scraped.clone();
        let fs_exec = Arc::clone(self.workbench.fs());
        let toolbox_exec = Arc::clone(self.workbench.toolbox());

        tokio::task::spawn_blocking(move || -> DomainResult<()> {
            std::fs::create_dir_all(&dst_dir)
                .map_err(|e| DomainError::Storage(format!("failed creating Jellyfin dir: {e}")))?;
            if show_dir != dst_dir {
                std::fs::create_dir_all(&show_dir).map_err(|e| {
                    DomainError::Storage(format!("failed creating Jellyfin show dir: {e}"))
                })?;
            }

            let old_parent = src_abs.parent().map(Path::to_path_buf);
            let existing_video_nfo_bytes = toolbox_exec
                .find_existing_video_nfo(&src_abs, meta.is_episode)
                .or_else(|| toolbox_exec.find_existing_video_nfo(&src_abs, !meta.is_episode))
                .and_then(|p| std::fs::read(p).ok());
            let existing_show_nfo_bytes = toolbox_exec
                .find_show_nfo_for_episode(&src_abs)
                .and_then(|p| std::fs::read(p).ok());
            let existing_old_season_nfo_bytes = old_parent
                .as_ref()
                .map(|p| p.join("season.nfo"))
                .filter(|p| p.is_file())
                .and_then(|p| std::fs::read(p).ok());
            let companions = fs_exec.find_companion_sidecars(&src_abs);

            if dst_abs_clone != src_abs {
                std::fs::rename(&src_abs, &dst_abs_clone).map_err(|e| {
                    DomainError::Storage(format!(
                        "failed archiving video into '{}': {e}",
                        dst_abs_clone.display()
                    ))
                })?;
            }

            for (comp_abs, suffix) in companions {
                let lower = suffix.to_ascii_lowercase();
                let is_sub = lower.ends_with(".srt")
                    || lower.ends_with(".ass")
                    || lower.ends_with(".ssa")
                    || lower.ends_with(".vtt");
                if is_sub {
                    let sub_dst = dst_dir.join(format!("{target_stem}{suffix}"));
                    if sub_dst != comp_abs && !sub_dst.exists() {
                        let _ = std::fs::rename(&comp_abs, &sub_dst);
                    }
                } else if dst_abs_clone != src_abs {
                    let image_ext = |p_lower: &str| -> &'static str {
                        if p_lower.ends_with(".png") {
                            "png"
                        } else if p_lower.ends_with(".webp") {
                            "webp"
                        } else {
                            "jpg"
                        }
                    };
                    let mut should_remove_old = true;
                    if lower.contains("-poster.") && !will_replace_poster {
                        let target_poster_dir = if meta.is_episode { &show_dir } else { &dst_dir };
                        let migrated =
                            target_poster_dir.join(format!("poster.{}", image_ext(&lower)));
                        if !migrated.exists() && std::fs::copy(&comp_abs, &migrated).is_err() {
                            should_remove_old = false;
                        }
                    } else if lower.contains("-fanart.") && !will_replace_fanart {
                        let target_fan_dir = if meta.is_episode { &show_dir } else { &dst_dir };
                        let migrated = target_fan_dir.join(format!("fanart.{}", image_ext(&lower)));
                        if !migrated.exists() && std::fs::copy(&comp_abs, &migrated).is_err() {
                            should_remove_old = false;
                        }
                    } else if lower.contains("-thumb.") && !will_replace_thumb {
                        let migrated =
                            dst_dir.join(format!("{target_stem}-thumb.{}", image_ext(&lower)));
                        if !migrated.exists() && std::fs::copy(&comp_abs, &migrated).is_err() {
                            should_remove_old = false;
                        }
                    }
                    if should_remove_old {
                        let _ = std::fs::remove_file(&comp_abs);
                    }
                }
            }

            if !meta.is_episode {
                let movie_nfo = dst_dir.join("movie.nfo");
                let prev_movie_nfo = std::fs::read(&movie_nfo).ok().or(existing_video_nfo_bytes);
                let xml = toolbox_exec.render_video_nfo_xml(&meta, prev_movie_nfo.as_deref());
                fs_exec.atomic_write_bytes(&movie_nfo, xml.as_bytes())?;

                let same_stem_nfo = dst_dir.join(format!("{target_stem}.nfo"));
                if same_stem_nfo.is_file() && same_stem_nfo != movie_nfo {
                    let _ = std::fs::remove_file(same_stem_nfo);
                }

                if !skip_poster
                    && let Some(u) = scraped_clone.cover_url.as_deref().map(str::trim)
                    && let Some((bytes, img_ext)) = downloaded_images.get(u)
                {
                    fs_exec
                        .write_image_replacing_opposite_ext(&dst_dir, "poster", img_ext, bytes)?;
                }
                if !skip_fanart
                    && let Some(u) = scraped_clone.backdrop_url.as_deref().map(str::trim)
                    && let Some((bytes, img_ext)) = downloaded_images.get(u)
                {
                    fs_exec
                        .write_image_replacing_opposite_ext(&dst_dir, "fanart", img_ext, bytes)?;
                }
            } else {
                let tvshow_nfo_path = show_dir.join("tvshow.nfo");
                if !only_fill_missing || !tvshow_nfo_path.is_file() {
                    let prev_tvshow = std::fs::read(&tvshow_nfo_path)
                        .ok()
                        .or(existing_show_nfo_bytes);
                    let show_plot = scraped_clone
                        .show_summary
                        .as_deref()
                        .or(meta.summary.as_deref());
                    let tvshow_xml = toolbox_exec.render_tvshow_nfo_xml(
                        &meta,
                        show_plot,
                        prev_tvshow.as_deref(),
                    );
                    fs_exec.atomic_write_bytes(&tvshow_nfo_path, tvshow_xml.as_bytes())?;
                }

                if !skip_poster
                    && let Some(u) = scraped_clone.cover_url.as_deref().map(str::trim)
                    && let Some((bytes, img_ext)) = downloaded_images.get(u)
                {
                    fs_exec
                        .write_image_replacing_opposite_ext(&show_dir, "poster", img_ext, bytes)?;
                }
                if !skip_fanart
                    && let Some(u) = scraped_clone.backdrop_url.as_deref().map(str::trim)
                    && let Some((bytes, img_ext)) = downloaded_images.get(u)
                {
                    fs_exec
                        .write_image_replacing_opposite_ext(&show_dir, "fanart", img_ext, bytes)?;
                }

                let season_num = meta.season_number.unwrap_or(1);
                let season_nfo_path = dst_dir.join("season.nfo");
                if !only_fill_missing || !season_nfo_path.is_file() {
                    let prev_season = std::fs::read(&season_nfo_path)
                        .ok()
                        .or(existing_old_season_nfo_bytes);
                    let season_plot = scraped_clone
                        .season_summary
                        .as_deref()
                        .or(scraped_clone.show_summary.as_deref())
                        .or(meta.summary.as_deref());
                    let season_xml = toolbox_exec.render_season_nfo_xml(
                        season_num,
                        scraped_clone.season_title.as_deref(),
                        meta.year,
                        season_plot,
                        meta.tmdb_id.as_deref(),
                        meta.bangumi_id.as_deref(),
                        prev_season.as_deref(),
                    );
                    fs_exec.atomic_write_bytes(&season_nfo_path, season_xml.as_bytes())?;
                }

                if dst_dir != show_dir {
                    let season_poster_u = scraped_clone
                        .season_poster_url
                        .as_deref()
                        .map(str::trim)
                        .filter(|s| !s.is_empty())
                        .or_else(|| scraped_clone.cover_url.as_deref().map(str::trim));
                    if !skip_poster
                        && let Some(u) = season_poster_u
                        && let Some((bytes, img_ext)) = downloaded_images.get(u)
                    {
                        fs_exec.write_image_replacing_opposite_ext(
                            &dst_dir, "poster", img_ext, bytes,
                        )?;
                    }
                    if !skip_fanart
                        && let Some(u) = scraped_clone.backdrop_url.as_deref().map(str::trim)
                        && let Some((bytes, img_ext)) = downloaded_images.get(u)
                    {
                        fs_exec.write_image_replacing_opposite_ext(
                            &dst_dir, "fanart", img_ext, bytes,
                        )?;
                    }
                }

                let ep_nfo = dst_dir.join(format!("{target_stem}.nfo"));
                let prev_ep_nfo = std::fs::read(&ep_nfo).ok().or(existing_video_nfo_bytes);
                let ep_xml = toolbox_exec.render_video_nfo_xml(&meta, prev_ep_nfo.as_deref());
                fs_exec.atomic_write_bytes(&ep_nfo, ep_xml.as_bytes())?;

                if !skip_thumb
                    && let Some(u) = scraped_clone.episode_still_url.as_deref().map(str::trim)
                    && let Some((bytes, img_ext)) = downloaded_images.get(u)
                {
                    let thumb_base = format!("{target_stem}-thumb");
                    fs_exec.write_image_replacing_opposite_ext(
                        &dst_dir,
                        &thumb_base,
                        img_ext,
                        bytes,
                    )?;
                }
            }

            if let Some(ref prev_dir) = old_parent {
                fs_exec.cleanup_empty_parent_dirs(prev_dir, &root_path_buf);
            }

            Ok(())
        })
        .await
        .map_err(|e| DomainError::Storage(format!("video Jellyfin archive task failed: {e}")))??;
        drop(root_guard);

        if target_rel_path != src_rel_slash {
            self.workbench
                .update_metadata_history_paths(root_id, &[(src_rel_slash, target_rel_path.clone())])
                .await;
        }
        self.workbench
            .record_external_metadata_snapshot(
                root_id,
                MediaKind::Video,
                &target_rel_path,
                MediaMetadataPayload::Video(pre_scrape_meta),
            )
            .await;

        if raw_settings.xmedia_webhook.notify_on_scrape {
            let webhook_cfg = raw_settings.xmedia_webhook;
            let root_path = root.path;
            let webhook = Arc::clone(self.workbench.webhook());
            tokio::spawn(async move {
                webhook.notify_rescan(&webhook_cfg, &root_path).await;
            });
        }

        self.workbench
            .inspect_file(root_id, &target_rel_path, Some(MediaKind::Video))
            .await
    }
}

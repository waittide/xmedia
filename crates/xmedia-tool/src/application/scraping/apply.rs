use std::collections::HashMap;
use std::path::Path;

use tracing::warn;
use xmedia_core::domain::error::{DomainError, DomainResult};
use xmedia_core::domain::library::MediaKind;

use super::ScrapingOrchestratorService;
use super::artwork::download_image_bytes;
use super::mapping::{
    apply_video_locked_fields, existing_artwork_present, filter_patch_for_missing,
    filter_video_patch_for_missing, map_scraped_to_patch,
};
use crate::application::webhook::notify_xmedia_rescan;
use crate::domain::media::{MediaFileDetail, MediaMetadataPatch, MediaMetadataPayload};
use crate::domain::scraper::{ScrapeTargetKind, ScrapedMetadata};
use crate::domain::template::render_template_target;
use crate::domain::workspace::RootId;
use crate::infra::atomic_fs::{
    atomic_write_bytes, cleanup_empty_parent_dirs, find_companion_sidecars,
};
use crate::infra::inspectors::book::{dir_has_single_book, is_generic_book_folder};
use crate::infra::inspectors::comic::is_generic_comic_folder;
use crate::infra::inspectors::music::is_generic_music_folder;
use crate::infra::inspectors::video::{
    apply_video_patch_onto, find_existing_nfo, find_show_nfo_for_episode, is_season_dir,
    read_base_video_meta, render_season_nfo_xml_preserving, render_tvshow_nfo_xml_preserving,
    render_video_nfo_xml_preserving, write_image_replacing_opposite_ext,
};
use crate::infra::scrapers::http_client::build_ssrf_safe_http_client;
use crate::infra::workspace_fs::{resolve_existing_in_root, resolve_target_in_root};

impl ScrapingOrchestratorService {
    /// Applies `ScrapedMetadata` directly onto a target media file (atomically writing tags/OPF/ComicInfo/NFO,
    /// lyrics, downloading artwork, and optionally organizing into templates / Jellyfin folder layout).
    #[allow(clippy::too_many_arguments)]
    pub async fn apply_to_file(
        &self,
        root_id: RootId,
        rel_path: &str,
        media_kind: MediaKind,
        scraped: ScrapedMetadata,
        apply_cover: bool,
        write_sidecar_lrc: bool,
        organize_by_template: Option<bool>,
        only_fill_missing: bool,
    ) -> DomainResult<MediaFileDetail> {
        self.apply_to_file_ext(
            root_id,
            rel_path,
            media_kind,
            scraped,
            apply_cover,
            write_sidecar_lrc,
            organize_by_template,
            only_fill_missing,
            false,
        )
        .await
    }

    #[allow(clippy::too_many_arguments)]
    pub async fn apply_to_file_ext(
        &self,
        root_id: RootId,
        rel_path: &str,
        media_kind: MediaKind,
        scraped: ScrapedMetadata,
        apply_cover: bool,
        write_sidecar_lrc: bool,
        organize_by_template: Option<bool>,
        only_fill_missing: bool,
        write_album_cover: bool,
    ) -> DomainResult<MediaFileDetail> {
        let should_organize = organize_by_template.unwrap_or(true);

        if media_kind == MediaKind::Video && should_organize {
            return self
                .apply_video_jellyfin_archive(
                    root_id,
                    rel_path,
                    scraped,
                    apply_cover,
                    only_fill_missing,
                )
                .await;
        }

        let need_existing = only_fill_missing
            || media_kind == MediaKind::Book
            || media_kind == MediaKind::Video
            || (media_kind == MediaKind::Music
                && scraped.target == Some(ScrapeTargetKind::MusicAlbum));
        let existing_detail = if need_existing {
            Some(
                self.workbench
                    .inspect_file(root_id, rel_path, Some(media_kind))
                    .await?,
            )
        } else {
            None
        };

        let existing_music_artist = existing_detail.as_ref().is_some_and(|d| {
            matches!(&d.metadata, MediaMetadataPayload::Music(m) if !m.artist.trim().is_empty())
        });

        let mut patch = map_scraped_to_patch(
            media_kind,
            &scraped,
            write_sidecar_lrc,
            existing_music_artist,
        );
        if only_fill_missing && let Some(ref cur) = existing_detail {
            let fallback_stem = Path::new(rel_path)
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or("");
            filter_patch_for_missing(&mut patch, &cur.metadata, fallback_stem);
        }

        let mut effective_apply_cover = apply_cover;
        if let Some(MediaMetadataPayload::Book(existing_book)) =
            existing_detail.as_ref().map(|d| &d.metadata)
            && !existing_book.locked_fields.is_empty()
        {
            let is_locked = |name: &str| {
                existing_book
                    .locked_fields
                    .iter()
                    .any(|f| f.eq_ignore_ascii_case(name))
            };
            if let MediaMetadataPatch::Book(ref mut bp) = patch {
                if is_locked("title") {
                    bp.title = None;
                }
                if is_locked("authors") {
                    bp.authors = None;
                }
                if is_locked("series") {
                    bp.series = None;
                }
                if is_locked("subseries") {
                    bp.subseries = None;
                }
                if is_locked("series_index") {
                    bp.series_index = None;
                }
                if is_locked("isbn") {
                    bp.isbn = None;
                }
                if is_locked("publisher") {
                    bp.publisher = None;
                }
                if is_locked("published_at") {
                    bp.published_at = None;
                }
                if is_locked("language") {
                    bp.language = None;
                }
                if is_locked("tags") {
                    bp.tags = None;
                }
                if is_locked("description") {
                    bp.description = None;
                }
            }
            if is_locked("cover") {
                effective_apply_cover = false;
            }
        }

        if let Some(MediaMetadataPayload::Video(existing_video)) =
            existing_detail.as_ref().map(|d| &d.metadata)
            && !existing_video.locked_fields.is_empty()
            && let MediaMetadataPatch::Video(ref mut vp) = patch
        {
            apply_video_locked_fields(
                vp,
                &existing_video.locked_fields,
                &mut effective_apply_cover,
            );
        }

        let mut detail = self
            .workbench
            .write_metadata(root_id, rel_path, patch)
            .await?;

        let defer_non_epub_book_cover = media_kind == MediaKind::Book
            && should_organize
            && !detail.entry.extension.eq_ignore_ascii_case("epub");
        let mut cached_cover_download: Option<(Vec<u8>, String)> = None;
        if effective_apply_cover && media_kind != MediaKind::Comic && !defer_non_epub_book_cover {
            let creds = self.settings.get_credentials().await?;
            let client = build_ssrf_safe_http_client(&creds, self.allow_private_artwork_ips)?;

            if media_kind == MediaKind::Video {
                let (has_poster, has_fanart, has_thumb) = match existing_detail
                    .as_ref()
                    .map(|d| &d.metadata)
                    .unwrap_or(&detail.metadata)
                {
                    MediaMetadataPayload::Video(v) => (v.has_poster, v.has_fanart, v.has_thumb),
                    _ => (false, false, false),
                };
                for (slot, url_opt, already_has) in [
                    ("poster", scraped.cover_url.as_deref(), has_poster),
                    ("fanart", scraped.backdrop_url.as_deref(), has_fanart),
                    ("thumb", scraped.episode_still_url.as_deref(), has_thumb),
                ] {
                    if only_fill_missing && already_has {
                        continue;
                    }
                    if let Some(u) = url_opt.map(str::trim).filter(|s| !s.is_empty()) {
                        match download_image_bytes(&client, u, self.allow_private_artwork_ips).await
                        {
                            Ok((bytes, mime)) => {
                                detail = self
                                    .workbench
                                    .write_artwork(
                                        root_id,
                                        rel_path,
                                        &bytes,
                                        &mime,
                                        slot,
                                        Some(media_kind),
                                    )
                                    .await?;
                            }
                            Err(err) => {
                                warn!("failed downloading video {slot} artwork '{u}': {err}");
                            }
                        }
                    }
                }
            } else {
                let already_has_cover = existing_detail
                    .as_ref()
                    .is_some_and(|d| existing_artwork_present(&d.metadata));
                if let Some(ref cover_url) = scraped.cover_url
                    && !cover_url.trim().is_empty()
                {
                    let need_embedded = !(only_fill_missing && already_has_cover);
                    if need_embedded || (media_kind == MediaKind::Music && write_album_cover) {
                        match download_image_bytes(
                            &client,
                            cover_url,
                            self.allow_private_artwork_ips,
                        )
                        .await
                        {
                            Ok((bytes, mime)) => {
                                if need_embedded {
                                    detail = self
                                        .workbench
                                        .write_artwork(
                                            root_id,
                                            rel_path,
                                            &bytes,
                                            &mime,
                                            "cover",
                                            Some(media_kind),
                                        )
                                        .await?;
                                }
                                cached_cover_download = Some((bytes, mime));
                            }
                            Err(err) => {
                                warn!("failed downloading cover artwork '{cover_url}': {err}");
                            }
                        }
                    }
                }
            }
        }

        if should_organize {
            let plans = self
                .workbench
                .preview_rename(
                    root_id,
                    &[detail.entry.rel_path.clone()],
                    None,
                    None,
                    Some(media_kind),
                )
                .await?;
            if let Some(plan) = plans.first()
                && !plan.unchanged
                && plan.conflict.is_none()
            {
                self.workbench.execute_rename(root_id, &plans).await?;
                if media_kind == MediaKind::Book
                    && !detail.entry.extension.eq_ignore_ascii_case("epub")
                    && let Ok(root) = self.settings.find_root(root_id).await
                    && let Ok(dst_sb) = resolve_existing_in_root(&root, &plan.to_rel_path)
                {
                    let root_path = root.path.clone();
                    let dst_abs = dst_sb.abs_path.clone();
                    let _ = tokio::task::spawn_blocking(move || {
                        if let Some(dst_dir) = dst_abs.parent()
                            && dst_dir != root_path
                            && dst_dir
                                .file_name()
                                .and_then(|s| s.to_str())
                                .is_some_and(|n| !is_generic_book_folder(n))
                            && dir_has_single_book(dst_dir)
                        {
                            let stem_opf = dst_abs.with_extension("opf");
                            let calibre_opf = dst_dir.join("metadata.opf");
                            if stem_opf.is_file() && !calibre_opf.exists() {
                                let _ = std::fs::rename(&stem_opf, &calibre_opf);
                            }
                            for ext in ["jpg", "jpeg", "png", "webp"] {
                                let stem_cov = dst_abs.with_extension(ext);
                                let calibre_cov = dst_dir.join(format!("cover.{ext}"));
                                if stem_cov.is_file() && !calibre_cov.exists() {
                                    let _ = std::fs::rename(&stem_cov, &calibre_cov);
                                }
                            }
                        }
                    })
                    .await;
                }
                detail = self
                    .workbench
                    .inspect_file(root_id, &plan.to_rel_path, Some(media_kind))
                    .await?;
            }
        }

        if effective_apply_cover && defer_non_epub_book_cover {
            let already_has_cover = existing_detail
                .as_ref()
                .is_some_and(|d| existing_artwork_present(&d.metadata));
            if !(only_fill_missing && already_has_cover)
                && let Some(ref cover_url) = scraped.cover_url
                && !cover_url.trim().is_empty()
            {
                let creds = self.settings.get_credentials().await?;
                let client = build_ssrf_safe_http_client(&creds, self.allow_private_artwork_ips)?;
                if let Ok((bytes, mime)) =
                    download_image_bytes(&client, cover_url, self.allow_private_artwork_ips).await
                {
                    let cur_rel = detail.entry.rel_path.clone();
                    detail = self
                        .workbench
                        .write_artwork(
                            root_id,
                            &cur_rel,
                            &bytes,
                            &mime,
                            "cover",
                            Some(MediaKind::Book),
                        )
                        .await?;
                }
            }
        }

        // Optional Music album directory `cover.*` (user opt-in, default false)
        if effective_apply_cover
            && write_album_cover
            && media_kind == MediaKind::Music
            && let Some((bytes, mime)) = cached_cover_download
        {
            let parent_dir_name = Path::new(&detail.entry.rel_path)
                .parent()
                .and_then(|p| p.file_name())
                .and_then(|s| s.to_str())
                .unwrap_or("");
            let in_album_dir =
                !parent_dir_name.is_empty() && !is_generic_music_folder(parent_dir_name);
            let already_has_dir_cover = matches!(
                &detail.metadata,
                MediaMetadataPayload::Music(m) if m.has_directory_cover
            );
            if in_album_dir && !(only_fill_missing && already_has_dir_cover) {
                let cur_rel = detail.entry.rel_path.clone();
                detail = self
                    .workbench
                    .write_artwork(
                        root_id,
                        &cur_rel,
                        &bytes,
                        &mime,
                        "album_cover",
                        Some(MediaKind::Music),
                    )
                    .await?;
            }
        }

        // For Comics (Komga/Kavita convention): write series-level `cover.*` in the dedicated
        // series directory AFTER organizing, without injecting an extra page into each chapter CBZ.
        if effective_apply_cover && media_kind == MediaKind::Comic {
            let parent_dir_name = Path::new(&detail.entry.rel_path)
                .parent()
                .and_then(|p| p.file_name())
                .and_then(|s| s.to_str())
                .unwrap_or("");
            let in_series_dir =
                !parent_dir_name.is_empty() && !is_generic_comic_folder(parent_dir_name);
            let already_has_series_cover = matches!(
                &detail.metadata,
                MediaMetadataPayload::Comic(c) if c.has_series_cover
            );
            if in_series_dir
                && !(only_fill_missing && already_has_series_cover)
                && let Some(ref cover_url) = scraped.cover_url
                && !cover_url.trim().is_empty()
            {
                let creds = self.settings.get_credentials().await?;
                let client = build_ssrf_safe_http_client(&creds, self.allow_private_artwork_ips)?;
                if let Ok((bytes, mime)) =
                    download_image_bytes(&client, cover_url, self.allow_private_artwork_ips).await
                {
                    let cur_rel = detail.entry.rel_path.clone();
                    detail = self
                        .workbench
                        .write_artwork(
                            root_id,
                            &cur_rel,
                            &bytes,
                            &mime,
                            "series_cover",
                            Some(MediaKind::Comic),
                        )
                        .await?;
                }
            }
        }

        if let Ok(settings) = self.settings.get_raw().await
            && settings.xmedia_webhook.notify_on_scrape
            && let Ok(root) = self.settings.find_root(root_id).await
        {
            let webhook_cfg = settings.xmedia_webhook;
            let root_path = root.path;
            tokio::spawn(async move {
                notify_xmedia_rescan(&webhook_cfg, &root_path).await;
            });
        }

        Ok(detail)
    }

    async fn apply_video_jellyfin_archive(
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
        let src_sandbox = resolve_existing_in_root(&root, rel_path)?;
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

        // 1. Merge scraped metadata onto base local video metadata (preserving filename SxxExx if scraper omitted it)
        let src_abs_for_read = src_sandbox.abs_path.clone();
        let mut meta = tokio::task::spawn_blocking(move || read_base_video_meta(&src_abs_for_read))
            .await
            .map_err(|e| DomainError::Storage(format!("video base inspect failed: {e}")))?;
        let existing_poster = meta.has_poster;
        let existing_fanart = meta.has_fanart;
        let existing_thumb = meta.has_thumb;
        if only_fill_missing {
            let fallback_stem = src_sandbox
                .abs_path
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or("");
            filter_video_patch_for_missing(&mut vp, &meta, fallback_stem);
        }
        let mut effective_apply_cover = apply_cover;
        if !meta.locked_fields.is_empty() {
            apply_video_locked_fields(&mut vp, &meta.locked_fields, &mut effective_apply_cover);
        }
        apply_video_patch_onto(&mut meta, vp);
        if meta.is_episode && meta.season_number.is_none() {
            meta.season_number = Some(1);
        }

        // 2. Compute Jellyfin folder hierarchy target anchored at `root`
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
        let dst_abs = resolve_target_in_root(&root, &target_rel_path)?;
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
            && is_season_dir(&dst_dir)
            && dst_dir != root.path
            && let Some(parent) = dst_dir.parent()
            && parent != root.path
            && parent.starts_with(&root.path)
        {
            parent.to_path_buf()
        } else {
            dst_dir.clone()
        };

        // 3. Pre-fetch requested scraped artwork bytes (with URL deduplication)
        let mut downloaded_images: HashMap<String, (Vec<u8>, &'static str)> = HashMap::new();
        let skip_poster = !effective_apply_cover || (only_fill_missing && existing_poster);
        let skip_fanart = !effective_apply_cover || (only_fill_missing && existing_fanart);
        let skip_thumb = !effective_apply_cover || (only_fill_missing && existing_thumb);
        if effective_apply_cover {
            let client = build_ssrf_safe_http_client(
                &raw_settings.credentials,
                self.allow_private_artwork_ips,
            )?;
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
                match download_image_bytes(&client, u, self.allow_private_artwork_ips).await {
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

        // 4. Perform filesystem move, subtitle migration, 3-tier NFO generation & Jellyfin artwork writes
        let root_path_buf = root.path.clone();
        let src_abs = src_sandbox.abs_path.clone();
        let dst_abs_clone = dst_abs.clone();
        let scraped_clone = scraped.clone();

        tokio::task::spawn_blocking(move || -> DomainResult<()> {
            std::fs::create_dir_all(&dst_dir)
                .map_err(|e| DomainError::Storage(format!("failed creating Jellyfin dir: {e}")))?;
            if show_dir != dst_dir {
                std::fs::create_dir_all(&show_dir).map_err(|e| {
                    DomainError::Storage(format!("failed creating Jellyfin show dir: {e}"))
                })?;
            }

            let old_parent = src_abs.parent().map(Path::to_path_buf);
            let existing_video_nfo_bytes = find_existing_nfo(&src_abs, meta.is_episode)
                .or_else(|| find_existing_nfo(&src_abs, !meta.is_episode))
                .and_then(|p| std::fs::read(p).ok());
            let existing_show_nfo_bytes =
                find_show_nfo_for_episode(&src_abs).and_then(|p| std::fs::read(p).ok());
            let existing_old_season_nfo_bytes = old_parent
                .as_ref()
                .map(|p| p.join("season.nfo"))
                .filter(|p| p.is_file())
                .and_then(|p| std::fs::read(p).ok());
            let companions = find_companion_sidecars(&src_abs);

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
                    // Preserve existing local poster/fanart/thumb whenever no replacement was downloaded
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
                    // Clean up flat legacy sidecars (`<old>.nfo`, `<old>-poster.*`, `<old>-fanart.*`, `<old>-thumb.*`)
                    if should_remove_old {
                        let _ = std::fs::remove_file(&comp_abs);
                    }
                }
            }

            if !meta.is_episode {
                // Jellyfin Movie layout: `<Movie Dir>/movie.nfo`, `poster.jpg`, `fanart.jpg`
                let movie_nfo = dst_dir.join("movie.nfo");
                let prev_movie_nfo = std::fs::read(&movie_nfo).ok().or(existing_video_nfo_bytes);
                let xml = render_video_nfo_xml_preserving(&meta, prev_movie_nfo.as_deref());
                atomic_write_bytes(&movie_nfo, xml.as_bytes())?;

                let same_stem_nfo = dst_dir.join(format!("{target_stem}.nfo"));
                if same_stem_nfo.is_file() && same_stem_nfo != movie_nfo {
                    let _ = std::fs::remove_file(same_stem_nfo);
                }

                if !skip_poster
                    && let Some(u) = scraped_clone.cover_url.as_deref().map(str::trim)
                    && let Some((bytes, img_ext)) = downloaded_images.get(u)
                {
                    write_image_replacing_opposite_ext(&dst_dir, "poster", img_ext, bytes)?;
                }
                if !skip_fanart
                    && let Some(u) = scraped_clone.backdrop_url.as_deref().map(str::trim)
                    && let Some((bytes, img_ext)) = downloaded_images.get(u)
                {
                    write_image_replacing_opposite_ext(&dst_dir, "fanart", img_ext, bytes)?;
                }
            } else {
                // Jellyfin 3-Tier TV Show layout:
                // 1) Series root (`show_dir`): `tvshow.nfo`, `poster.jpg`, `fanart.jpg`
                let tvshow_nfo_path = show_dir.join("tvshow.nfo");
                if !only_fill_missing || !tvshow_nfo_path.is_file() {
                    let prev_tvshow = std::fs::read(&tvshow_nfo_path)
                        .ok()
                        .or(existing_show_nfo_bytes);
                    let show_plot = scraped_clone
                        .show_summary
                        .as_deref()
                        .or(meta.summary.as_deref());
                    let tvshow_xml =
                        render_tvshow_nfo_xml_preserving(&meta, show_plot, prev_tvshow.as_deref());
                    atomic_write_bytes(&tvshow_nfo_path, tvshow_xml.as_bytes())?;
                }

                if !skip_poster
                    && let Some(u) = scraped_clone.cover_url.as_deref().map(str::trim)
                    && let Some((bytes, img_ext)) = downloaded_images.get(u)
                {
                    write_image_replacing_opposite_ext(&show_dir, "poster", img_ext, bytes)?;
                }
                if !skip_fanart
                    && let Some(u) = scraped_clone.backdrop_url.as_deref().map(str::trim)
                    && let Some((bytes, img_ext)) = downloaded_images.get(u)
                {
                    write_image_replacing_opposite_ext(&show_dir, "fanart", img_ext, bytes)?;
                }

                // 2) Season folder (`dst_dir` = `Season XX`): `season.nfo`, `poster.jpg`, `fanart.jpg`
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
                    let season_xml = render_season_nfo_xml_preserving(
                        season_num,
                        scraped_clone.season_title.as_deref(),
                        meta.year,
                        season_plot,
                        meta.tmdb_id.as_deref(),
                        meta.bangumi_id.as_deref(),
                        prev_season.as_deref(),
                    );
                    atomic_write_bytes(&season_nfo_path, season_xml.as_bytes())?;
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
                        write_image_replacing_opposite_ext(&dst_dir, "poster", img_ext, bytes)?;
                    }
                    if !skip_fanart
                        && let Some(u) = scraped_clone.backdrop_url.as_deref().map(str::trim)
                        && let Some((bytes, img_ext)) = downloaded_images.get(u)
                    {
                        write_image_replacing_opposite_ext(&dst_dir, "fanart", img_ext, bytes)?;
                    }
                }

                // 3) Episode level (`Season XX/<target_stem>.nfo` + `<target_stem>-thumb.jpg`)
                let ep_nfo = dst_dir.join(format!("{target_stem}.nfo"));
                let prev_ep_nfo = std::fs::read(&ep_nfo).ok().or(existing_video_nfo_bytes);
                let ep_xml = render_video_nfo_xml_preserving(&meta, prev_ep_nfo.as_deref());
                atomic_write_bytes(&ep_nfo, ep_xml.as_bytes())?;

                if !skip_thumb
                    && let Some(u) = scraped_clone.episode_still_url.as_deref().map(str::trim)
                    && let Some((bytes, img_ext)) = downloaded_images.get(u)
                {
                    let thumb_base = format!("{target_stem}-thumb");
                    write_image_replacing_opposite_ext(&dst_dir, &thumb_base, img_ext, bytes)?;
                }
            }

            if let Some(ref prev_dir) = old_parent {
                cleanup_empty_parent_dirs(prev_dir, &root_path_buf);
            }

            Ok(())
        })
        .await
        .map_err(|e| DomainError::Storage(format!("video Jellyfin archive task failed: {e}")))??;
        drop(root_guard);

        if raw_settings.xmedia_webhook.notify_on_scrape {
            let webhook_cfg = raw_settings.xmedia_webhook;
            let root_path = root.path;
            tokio::spawn(async move {
                notify_xmedia_rescan(&webhook_cfg, &root_path).await;
            });
        }

        self.workbench
            .inspect_file(root_id, &target_rel_path, Some(MediaKind::Video))
            .await
    }
}

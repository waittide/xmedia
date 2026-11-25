use std::path::Path;
use std::sync::Arc;

use tracing::warn;
use xmedia_core::domain::error::DomainResult;
use xmedia_core::domain::library::MediaKind;

use super::ScrapingOrchestratorService;
use super::mapping::{existing_artwork_present, map_scraped_to_patch};
use crate::domain::media::{MediaFileDetail, MediaMetadataPayload};
use crate::domain::scraper::{ScrapeTargetKind, ScrapedMetadata};
use crate::domain::workspace::RootId;

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
        let mut effective_apply_cover = apply_cover;
        if let Some(ref cur) = existing_detail {
            let fallback_stem = Path::new(rel_path)
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or("");
            cur.metadata.prepare_patch(
                &mut patch,
                only_fill_missing,
                true,
                fallback_stem,
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
                        match self
                            .transport
                            .download_artwork(&creds, u, self.allow_private_artwork_ips)
                            .await
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
                        match self
                            .transport
                            .download_artwork(&creds, cover_url, self.allow_private_artwork_ips)
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
                    && let Ok(dst_sb) = self
                        .workbench
                        .fs()
                        .resolve_existing_in_root(&root, &plan.to_rel_path)
                {
                    let root_path = root.path.clone();
                    let dst_abs = dst_sb.abs_path.clone();
                    let toolbox = Arc::clone(self.workbench.toolbox());
                    let _ = tokio::task::spawn_blocking(move || {
                        if let Some(dst_dir) = dst_abs.parent()
                            && dst_dir != root_path
                            && dst_dir
                                .file_name()
                                .and_then(|s| s.to_str())
                                .is_some_and(|n| !toolbox.is_generic_book_folder(n))
                            && toolbox.dir_has_single_book(dst_dir)
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
                if let Ok((bytes, mime)) = self
                    .transport
                    .download_artwork(&creds, cover_url, self.allow_private_artwork_ips)
                    .await
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
            let in_album_dir = !parent_dir_name.is_empty()
                && !self
                    .workbench
                    .toolbox()
                    .is_generic_music_folder(parent_dir_name);
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

        if effective_apply_cover && media_kind == MediaKind::Comic {
            let parent_dir_name = Path::new(&detail.entry.rel_path)
                .parent()
                .and_then(|p| p.file_name())
                .and_then(|s| s.to_str())
                .unwrap_or("");
            let in_series_dir = !parent_dir_name.is_empty()
                && !self
                    .workbench
                    .toolbox()
                    .is_generic_comic_folder(parent_dir_name);
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
                if let Ok((bytes, mime)) = self
                    .transport
                    .download_artwork(&creds, cover_url, self.allow_private_artwork_ips)
                    .await
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
            let webhook = Arc::clone(self.workbench.webhook());
            tokio::spawn(async move {
                webhook.notify_rescan(&webhook_cfg, &root_path).await;
            });
        }

        Ok(detail)
    }
}

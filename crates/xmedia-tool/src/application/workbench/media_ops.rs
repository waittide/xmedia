use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::Arc;

use xmedia_core::application::ports::BinaryPayload;
use xmedia_core::domain::error::{DomainError, DomainResult};
use xmedia_core::domain::library::MediaKind;

use super::MediaWorkbenchService;
use crate::domain::media::{
    ComicMetaPatch, MediaFileDetail, MediaMetadataPatch, MusicMetaPatch, VideoMetaPatch,
};
use crate::domain::ops::{
    BatchComicSeriesSyncInput, BatchComicSeriesSyncReportDto, BatchMusicAlbumSyncInput,
    BatchMusicAlbumSyncReportDto, BatchRemapEpisodesInput, BatchRemapEpisodesReportDto,
    ComicImageReencodeFormat, ComicOptimizeReportDto, ComicPagesSnapshotDto, CueSplitReportDto,
    ExtractSubtitleReportDto, MergeComicsReportDto, MusicCoverOptimizeReportDto,
    MusicLyricsToolInput, MusicLyricsToolReportDto, ProcessSubtitleInput, ProcessSubtitleReportDto,
    ReplayGainReportDto, SaveComicPageInput, VideoSubtitleInfoDto,
};
use crate::domain::workspace::{RootId, path_to_slash};

impl MediaWorkbenchService {
    pub async fn get_comic_pages(
        &self,
        root_id: RootId,
        rel_path: &str,
    ) -> DomainResult<ComicPagesSnapshotDto> {
        let (_, sandbox, _) = self
            .resolve_media_file(root_id, rel_path, Some(MediaKind::Comic))
            .await?;
        let abs = sandbox.abs_path;
        let toolbox = Arc::clone(&self.toolbox);
        tokio::task::spawn_blocking(move || toolbox.list_comic_pages(&abs))
            .await
            .map_err(|e| DomainError::Storage(format!("list comic pages task failed: {e}")))?
    }

    pub async fn read_comic_page(
        &self,
        root_id: RootId,
        rel_path: &str,
        entry_name: &str,
        max_width: Option<u32>,
    ) -> DomainResult<BinaryPayload> {
        let (_, sandbox, _) = self
            .resolve_media_file(root_id, rel_path, Some(MediaKind::Comic))
            .await?;
        let abs = sandbox.abs_path;
        let entry = entry_name.to_string();
        let toolbox = Arc::clone(&self.toolbox);
        tokio::task::spawn_blocking(move || toolbox.read_comic_page_image(&abs, &entry, max_width))
            .await
            .map_err(|e| DomainError::Storage(format!("read comic page task failed: {e}")))?
    }

    pub async fn save_comic_pages(
        &self,
        root_id: RootId,
        rel_path: &str,
        pages: Vec<SaveComicPageInput>,
        reencode: ComicImageReencodeFormat,
        jpeg_quality: u8,
        max_width: Option<u32>,
    ) -> DomainResult<(ComicOptimizeReportDto, MediaFileDetail)> {
        let (_, sandbox, _) = self
            .resolve_writable_media_file(root_id, rel_path, Some(MediaKind::Comic))
            .await?;
        let abs = sandbox.abs_path;
        let toolbox = Arc::clone(&self.toolbox);
        let report = tokio::task::spawn_blocking(move || {
            toolbox.save_comic_pages(&abs, &pages, reencode, jpeg_quality, max_width)
        })
        .await
        .map_err(|e| DomainError::Storage(format!("save comic pages task failed: {e}")))??;
        let detail = self
            .inspect_file(root_id, rel_path, Some(MediaKind::Comic))
            .await?;
        Ok((report, detail))
    }

    pub async fn optimize_comic(
        &self,
        root_id: RootId,
        rel_path: &str,
        reencode: ComicImageReencodeFormat,
        jpeg_quality: u8,
        max_width: Option<u32>,
    ) -> DomainResult<(ComicOptimizeReportDto, MediaFileDetail)> {
        let (_, sandbox, _) = self
            .resolve_writable_media_file(root_id, rel_path, Some(MediaKind::Comic))
            .await?;
        let abs = sandbox.abs_path;
        let toolbox = Arc::clone(&self.toolbox);
        let report = tokio::task::spawn_blocking(move || {
            toolbox.optimize_comic_cbz(&abs, reencode, jpeg_quality, max_width)
        })
        .await
        .map_err(|e| DomainError::Storage(format!("optimize comic task failed: {e}")))??;
        let detail = self
            .inspect_file(root_id, rel_path, Some(MediaKind::Comic))
            .await?;
        Ok((report, detail))
    }

    pub async fn merge_comics(
        &self,
        root_id: RootId,
        source_paths: Vec<String>,
        output_rel_path: &str,
        series: Option<String>,
        title: Option<String>,
        volume: Option<f64>,
    ) -> DomainResult<(MergeComicsReportDto, MediaFileDetail)> {
        let root = self.settings.find_root(root_id).await?;
        root.ensure_writable()?;
        let mut abs_sources = Vec::with_capacity(source_paths.len());
        for rel in &source_paths {
            let sb = self.fs.resolve_existing_in_root(&root, rel)?;
            abs_sources.push(sb.abs_path);
        }
        let target_rel = if output_rel_path.to_ascii_lowercase().ends_with(".cbz") {
            output_rel_path.to_string()
        } else {
            format!("{output_rel_path}.cbz")
        };
        let target_abs = self.fs.resolve_target_in_root(&root, &target_rel)?;
        if let Some(parent) = target_abs.parent() {
            std::fs::create_dir_all(parent).map_err(|e| {
                DomainError::Storage(format!("create merge output dir failed: {e}"))
            })?;
        }
        let target_rel_slash = target_abs
            .strip_prefix(&root.path)
            .map(path_to_slash)
            .unwrap_or_else(|_| target_rel.clone());
        let report_rel = target_rel_slash.clone();
        let toolbox = Arc::clone(&self.toolbox);
        let report = tokio::task::spawn_blocking(move || {
            toolbox.merge_comics_to_cbz(
                &abs_sources,
                &target_abs,
                report_rel,
                series,
                title,
                volume,
            )
        })
        .await
        .map_err(|e| DomainError::Storage(format!("merge comics task failed: {e}")))??;
        let detail = self
            .inspect_file(root_id, &target_rel_slash, Some(MediaKind::Comic))
            .await?;
        Ok((report, detail))
    }

    pub async fn batch_sync_comic_series(
        &self,
        root_id: RootId,
        paths: Vec<String>,
        input: BatchComicSeriesSyncInput,
    ) -> DomainResult<BatchComicSeriesSyncReportDto> {
        let root = self.settings.find_root(root_id).await?;
        root.ensure_writable()?;
        let mut updated_count = 0usize;
        let mut failed_paths = Vec::new();
        let mut snapshots = Vec::new();
        let width = input.pad_width.clamp(1, 6);

        for (idx, rel_path) in paths.iter().enumerate() {
            let seq = input.start_index + idx as i32;
            let (number, volume) = match input.numbering_mode.as_str() {
                "chapter" => (Some(format!("{seq:0width$}")), None),
                "volume" => (None, Some(f64::from(seq))),
                _ => (None, None),
            };
            let patch = MediaMetadataPatch::Comic(ComicMetaPatch {
                series: input.series.clone(),
                series_group: input.series_group.clone(),
                title: None,
                number,
                volume,
                summary: None,
                writers: input.writers.clone(),
                pencillers: input.pencillers.clone(),
                publisher: input.publisher.clone(),
                genres: input.genres.clone(),
                tags: input.tags.clone(),
                year: None,
                language_iso: None,
                reading_direction: input.reading_direction.clone(),
            });
            match self
                .write_metadata_capturing_snapshot(root_id, rel_path, patch)
                .await
            {
                Ok((_, snapshot)) => {
                    updated_count += 1;
                    if let Some(rec) = snapshot {
                        snapshots.push(rec);
                    }
                }
                Err(_) => failed_paths.push(rel_path.clone()),
            }
        }

        self.push_batch_metadata_snapshots(
            root_id,
            MediaKind::Comic,
            format!("批量同步漫画系列 ({updated_count} 项)"),
            snapshots,
        )
        .await;

        Ok(BatchComicSeriesSyncReportDto {
            updated_count,
            failed_paths,
        })
    }

    pub async fn run_music_lyrics_tool(
        &self,
        root_id: RootId,
        rel_path: &str,
        input: MusicLyricsToolInput,
    ) -> DomainResult<(MusicLyricsToolReportDto, MediaFileDetail)> {
        let (_, sandbox, _) = self
            .resolve_writable_media_file(root_id, rel_path, Some(MediaKind::Music))
            .await?;
        let abs = sandbox.abs_path;
        let toolbox = Arc::clone(&self.toolbox);
        let report =
            tokio::task::spawn_blocking(move || toolbox.process_music_lyrics(&abs, &input))
                .await
                .map_err(|e| {
                    DomainError::Storage(format!("music lyrics tool task failed: {e}"))
                })??;
        let detail = self
            .inspect_file(root_id, rel_path, Some(MediaKind::Music))
            .await?;
        Ok((report, detail))
    }

    pub async fn optimize_music_cover(
        &self,
        root_id: RootId,
        rel_path: &str,
        max_dimension: u32,
        jpeg_quality: u8,
    ) -> DomainResult<(MusicCoverOptimizeReportDto, MediaFileDetail)> {
        let (_, sandbox, _) = self
            .resolve_writable_media_file(root_id, rel_path, Some(MediaKind::Music))
            .await?;
        let abs = sandbox.abs_path;
        let toolbox = Arc::clone(&self.toolbox);
        let report = tokio::task::spawn_blocking(move || {
            toolbox.optimize_music_cover(&abs, max_dimension, jpeg_quality)
        })
        .await
        .map_err(|e| DomainError::Storage(format!("optimize music cover task failed: {e}")))??;
        let detail = self
            .inspect_file(root_id, rel_path, Some(MediaKind::Music))
            .await?;
        Ok((report, detail))
    }

    pub async fn apply_music_replaygain(
        &self,
        root_id: RootId,
        paths: Vec<String>,
    ) -> DomainResult<ReplayGainReportDto> {
        let root = self.settings.find_root(root_id).await?;
        root.ensure_writable()?;
        let mut resolved = Vec::with_capacity(paths.len());
        for rel in paths {
            if let Ok(sb) = self.fs.resolve_existing_in_root(&root, &rel)
                && sb.abs_path.is_file()
            {
                resolved.push((sb.rel_slash_string(), sb.abs_path));
            }
        }
        let ffmpeg_bin = self.ffmpeg.ffmpeg_bin().to_string();
        let toolbox = Arc::clone(&self.toolbox);
        tokio::task::spawn_blocking(move || toolbox.apply_replaygain_batch(&resolved, &ffmpeg_bin))
            .await
            .map_err(|e| DomainError::Storage(format!("replaygain task failed: {e}")))?
    }

    pub async fn split_music_cue(
        &self,
        root_id: RootId,
        rel_path: &str,
    ) -> DomainResult<CueSplitReportDto> {
        let root = self.settings.find_root(root_id).await?;
        root.ensure_writable()?;
        let sb = self.fs.resolve_existing_in_root(&root, rel_path)?;
        let abs = sb.abs_path;
        let ffmpeg_bin = self.ffmpeg.ffmpeg_bin().to_string();
        let toolbox = Arc::clone(&self.toolbox);
        tokio::task::spawn_blocking(move || toolbox.split_cue_album(&abs, &ffmpeg_bin))
            .await
            .map_err(|e| DomainError::Storage(format!("split cue task failed: {e}")))?
    }

    pub async fn batch_sync_music_album(
        &self,
        root_id: RootId,
        paths: Vec<String>,
        input: BatchMusicAlbumSyncInput,
    ) -> DomainResult<BatchMusicAlbumSyncReportDto> {
        let root = self.settings.find_root(root_id).await?;
        root.ensure_writable()?;
        let mut updated_count = 0usize;
        let mut failed_paths = Vec::new();
        let mut snapshots = Vec::new();

        for (idx, rel_path) in paths.iter().enumerate() {
            let track_number = if input.auto_number_tracks {
                Some(input.start_track + idx as u32)
            } else {
                None
            };
            let patch = MediaMetadataPatch::Music(MusicMetaPatch {
                title: None,
                artist: input.artist.clone(),
                album_artist: input.album_artist.clone(),
                album: input.album.clone(),
                track_number,
                disc_number: input.disc_number,
                year: input.year,
                genre: input.genre.clone(),
                comment: None,
                embedded_lyrics: None,
                sidecar_lrc: None,
            });
            match self
                .write_metadata_capturing_snapshot(root_id, rel_path, patch)
                .await
            {
                Ok((_, snapshot)) => {
                    updated_count += 1;
                    if let Some(rec) = snapshot {
                        snapshots.push(rec);
                    }
                }
                Err(_) => failed_paths.push(rel_path.clone()),
            }
        }

        self.push_batch_metadata_snapshots(
            root_id,
            MediaKind::Music,
            format!("批量同步专辑信息 ({updated_count} 项)"),
            snapshots,
        )
        .await;

        Ok(BatchMusicAlbumSyncReportDto {
            updated_count,
            failed_paths,
        })
    }

    pub async fn list_video_subtitles(
        &self,
        root_id: RootId,
        rel_path: &str,
    ) -> DomainResult<Vec<VideoSubtitleInfoDto>> {
        let (_, sandbox, _) = self
            .resolve_media_file(root_id, rel_path, Some(MediaKind::Video))
            .await?;
        let abs = sandbox.abs_path;
        let toolbox = Arc::clone(&self.toolbox);
        tokio::task::spawn_blocking(move || toolbox.list_video_subtitles(&abs))
            .await
            .map_err(|e| DomainError::Storage(format!("list video subtitles task failed: {e}")))?
    }

    pub async fn process_video_subtitle(
        &self,
        root_id: RootId,
        rel_path: &str,
        input: ProcessSubtitleInput,
    ) -> DomainResult<(ProcessSubtitleReportDto, MediaFileDetail)> {
        let (_, sandbox, _) = self
            .resolve_writable_media_file(root_id, rel_path, Some(MediaKind::Video))
            .await?;
        let abs = sandbox.abs_path;
        let toolbox = Arc::clone(&self.toolbox);
        let report =
            tokio::task::spawn_blocking(move || toolbox.process_video_subtitle(&abs, &input))
                .await
                .map_err(|e| {
                    DomainError::Storage(format!("process subtitle task failed: {e}"))
                })??;
        let detail = self
            .inspect_file(root_id, rel_path, Some(MediaKind::Video))
            .await?;
        Ok((report, detail))
    }

    pub async fn extract_video_embedded_subtitle(
        &self,
        root_id: RootId,
        rel_path: &str,
        stream_index: u32,
        language_tag: Option<String>,
        output_ext: String,
    ) -> DomainResult<(ExtractSubtitleReportDto, MediaFileDetail)> {
        let (_, sandbox, _) = self
            .resolve_writable_media_file(root_id, rel_path, Some(MediaKind::Video))
            .await?;
        let abs = sandbox.abs_path;
        let ffmpeg_bin = self.ffmpeg.ffmpeg_bin().to_string();
        let toolbox = Arc::clone(&self.toolbox);
        let report = tokio::task::spawn_blocking(move || {
            toolbox.extract_embedded_subtitle(
                &abs,
                stream_index,
                language_tag.as_deref(),
                &output_ext,
                &ffmpeg_bin,
            )
        })
        .await
        .map_err(|e| DomainError::Storage(format!("extract subtitle task failed: {e}")))??;
        let detail = self
            .inspect_file(root_id, rel_path, Some(MediaKind::Video))
            .await?;
        Ok((report, detail))
    }

    pub async fn batch_remap_video_episodes(
        &self,
        root_id: RootId,
        paths: Vec<String>,
        input: BatchRemapEpisodesInput,
    ) -> DomainResult<BatchRemapEpisodesReportDto> {
        let root = self.settings.find_root(root_id).await?;
        root.ensure_writable()?;
        let mut updated_count = 0usize;
        let mut failed_paths = Vec::new();
        let mut snapshots = Vec::new();
        let mut written_season_dirs: HashSet<PathBuf> = HashSet::new();

        for (idx, rel_path) in paths.iter().enumerate() {
            let sb = match self.fs.resolve_existing_in_root(&root, rel_path) {
                Ok(sb) => sb,
                Err(_) => {
                    failed_paths.push(rel_path.clone());
                    continue;
                }
            };
            let base = self.toolbox.read_base_video_meta(&sb.abs_path);
            let new_ep = if input.mode == "absolute_offset" {
                let cur = base.episode_number.unwrap_or((idx + 1) as u32) as i32;
                (cur + input.episode_offset).max(1) as u32
            } else {
                input.start_episode + idx as u32
            };

            let patch = MediaMetadataPatch::Video(VideoMetaPatch {
                is_episode: Some(true),
                show_title: input.show_title.clone(),
                season_number: input.season_number.map(|s| s as i32),
                episode_number: Some(new_ep),
                ..Default::default()
            });

            match self
                .write_metadata_capturing_snapshot(root_id, rel_path, patch)
                .await
            {
                Ok((_, snapshot)) => {
                    updated_count += 1;
                    if let Some(rec) = snapshot {
                        snapshots.push(rec);
                    }
                    if input.write_season_nfo
                        && let Some(season_num) = input.season_number
                        && let Some(parent) = sb.abs_path.parent()
                        && !written_season_dirs.contains(parent)
                    {
                        let season_nfo_path = parent.join("season.nfo");
                        let prev_season = std::fs::read(&season_nfo_path).ok();
                        let nfo = self.toolbox.render_season_nfo_xml(
                            season_num,
                            None,
                            base.year,
                            None,
                            base.tmdb_id.as_deref(),
                            base.bangumi_id.as_deref(),
                            prev_season.as_deref(),
                        );
                        if self
                            .fs
                            .atomic_write_bytes(&season_nfo_path, nfo.as_bytes())
                            .is_ok()
                        {
                            written_season_dirs.insert(parent.to_path_buf());
                        }
                    }
                }
                Err(_) => failed_paths.push(rel_path.clone()),
            }
        }

        self.push_batch_metadata_snapshots(
            root_id,
            MediaKind::Video,
            format!("批量重排剧集编号 ({updated_count} 项)"),
            snapshots,
        )
        .await;

        Ok(BatchRemapEpisodesReportDto {
            updated_count,
            failed_paths,
        })
    }
}

use std::path::Path;
use std::sync::Arc;

use xmedia_core::application::ports::BinaryPayload;
use xmedia_core::domain::error::{DomainError, DomainResult};
use xmedia_core::domain::library::MediaKind;
use xmedia_core::infra::ffmpeg::FfmpegToolkit;

use super::browse::WorkspaceBrowserService;
use crate::application::ops::{
    BookContentDocumentDto, BookToolCommand, BookToolOutput, ComicImageReencodeFormat,
    ComicOptimizeReportDto, ComicPagesSnapshotDto, ComicToolCommand, ComicToolOutput,
    CueSplitReportDto, EditableBookChapterDto, ExtractSubtitleReportDto, InsertedEpubImageDto,
    MergeComicsReportDto, MusicCoverOptimizeReportDto, MusicLyricsToolInput,
    MusicLyricsToolReportDto, MusicToolCommand, MusicToolOutput, NormalizeBookItemReportDto,
    NormalizeBookToEpubOptions, NormalizeBooksBatchReportDto, PolishBookReportDto,
    ProcessSubtitleInput, ProcessSubtitleReportDto, ReplayGainReportDto, SaveBookChapterInput,
    SaveComicPageInput, ToolboxCommand, ToolboxOutput, TransformBookContentResultDto,
    VideoSubtitleInfoDto, VideoToolCommand, VideoToolOutput,
};
use crate::application::ports::{MediaToolboxPort, WorkspaceFsPort};
use crate::application::settings::ToolSettingsService;
use crate::domain::media::MediaFileDetail;
use crate::domain::workspace::{RootId, path_to_slash};
use crate::domain::zh_text::BookTextTransformOptions;

pub enum BookWorkbenchOp {
    ReadEpubAsset {
        root_id: RootId,
        rel_path: String,
        asset_path: String,
    },
    ReadContent {
        root_id: RootId,
        rel_path: String,
    },
    ResegmentTxt {
        root_id: RootId,
        rel_path: String,
        chapter_pattern: Option<String>,
        volume_pattern: Option<String>,
    },
    SaveContent {
        root_id: RootId,
        rel_path: String,
        chapters: Vec<SaveBookChapterInput>,
        stylesheets: Option<Vec<String>>,
        force_horizontal_spine: bool,
    },
    ConvertTxtToEpub {
        root_id: RootId,
        rel_path: String,
        chapters: Option<Vec<SaveBookChapterInput>>,
        stylesheets: Option<Vec<String>>,
        delete_source_txt: bool,
    },
    TransformContent {
        chapters: Vec<EditableBookChapterDto>,
        stylesheets: Vec<String>,
        options: BookTextTransformOptions,
    },
    InsertEpubImage {
        root_id: RootId,
        rel_path: String,
        chapter_source_href: Option<String>,
        bytes: Vec<u8>,
        mime_hint: Option<String>,
    },
    Polish {
        root_id: RootId,
        rel_path: String,
    },
    NormalizeBatch {
        root_id: RootId,
        rel_paths: Vec<String>,
        options: NormalizeBookToEpubOptions,
    },
}

pub enum BookWorkbenchResult {
    Binary(BinaryPayload),
    Document(BookContentDocumentDto),
    Chapters(Vec<EditableBookChapterDto>),
    Detail(Box<MediaFileDetail>),
    Transform(TransformBookContentResultDto),
    InsertedImage(InsertedEpubImageDto),
    Polished(PolishBookReportDto, Box<MediaFileDetail>),
    NormalizedBatch(NormalizeBooksBatchReportDto),
}

pub enum ComicWorkbenchOp {
    ListPages {
        root_id: RootId,
        rel_path: String,
    },
    ReadPage {
        root_id: RootId,
        rel_path: String,
        entry_name: String,
        max_width: Option<u32>,
    },
    SavePages {
        root_id: RootId,
        rel_path: String,
        pages: Vec<SaveComicPageInput>,
        reencode: ComicImageReencodeFormat,
        jpeg_quality: u8,
        max_width: Option<u32>,
    },
    Optimize {
        root_id: RootId,
        rel_path: String,
        reencode: ComicImageReencodeFormat,
        jpeg_quality: u8,
        max_width: Option<u32>,
    },
    Merge {
        root_id: RootId,
        source_paths: Vec<String>,
        output_rel_path: String,
        series: Option<String>,
        title: Option<String>,
        volume: Option<f64>,
    },
}

pub enum ComicWorkbenchResult {
    Pages(ComicPagesSnapshotDto),
    Binary(BinaryPayload),
    Optimized(ComicOptimizeReportDto, Box<MediaFileDetail>),
    Merged(MergeComicsReportDto, Box<MediaFileDetail>),
}

pub enum MusicWorkbenchOp {
    Lyrics {
        root_id: RootId,
        rel_path: String,
        input: MusicLyricsToolInput,
    },
    OptimizeCover {
        root_id: RootId,
        rel_path: String,
        max_dimension: u32,
        jpeg_quality: u8,
    },
    ReplayGain {
        root_id: RootId,
        paths: Vec<String>,
    },
    SplitCue {
        root_id: RootId,
        rel_path: String,
    },
}

pub enum MusicWorkbenchResult {
    Lyrics(MusicLyricsToolReportDto, Box<MediaFileDetail>),
    Cover(MusicCoverOptimizeReportDto, Box<MediaFileDetail>),
    ReplayGain(ReplayGainReportDto),
    CueSplit(CueSplitReportDto),
}

pub enum VideoWorkbenchOp {
    ListSubtitles {
        root_id: RootId,
        rel_path: String,
    },
    ProcessSubtitle {
        root_id: RootId,
        rel_path: String,
        input: ProcessSubtitleInput,
    },
    ExtractSubtitle {
        root_id: RootId,
        rel_path: String,
        stream_index: u32,
        language_tag: Option<String>,
        output_ext: String,
    },
}

pub enum VideoWorkbenchResult {
    Subtitles(Vec<VideoSubtitleInfoDto>),
    ProcessedSubtitle(ProcessSubtitleReportDto, Box<MediaFileDetail>),
    ExtractedSubtitle(ExtractSubtitleReportDto, Box<MediaFileDetail>),
}

#[derive(Clone)]
pub struct FormatToolsService {
    settings: ToolSettingsService,
    workspace: WorkspaceBrowserService,
    fs: Arc<dyn WorkspaceFsPort>,
    toolbox: Arc<dyn MediaToolboxPort>,
    ffmpeg: FfmpegToolkit,
}

impl FormatToolsService {
    pub fn new(
        settings: ToolSettingsService,
        workspace: WorkspaceBrowserService,
        fs: Arc<dyn WorkspaceFsPort>,
        toolbox: Arc<dyn MediaToolboxPort>,
        ffmpeg: FfmpegToolkit,
    ) -> Self {
        Self {
            settings,
            workspace,
            fs,
            toolbox,
            ffmpeg,
        }
    }

    pub async fn run_book(&self, op: BookWorkbenchOp) -> DomainResult<BookWorkbenchResult> {
        match op {
            BookWorkbenchOp::ReadEpubAsset {
                root_id,
                rel_path,
                asset_path,
            } => {
                let (_, sb, _) = self
                    .workspace
                    .resolve_media_file(root_id, &rel_path, Some(MediaKind::Book))
                    .await?;
                let out = run_blocking_book(
                    &self.toolbox,
                    BookToolCommand::ReadEpubAsset {
                        path: sb.abs_path,
                        asset_path,
                    },
                )
                .await?;
                match out {
                    BookToolOutput::Binary(b) => Ok(BookWorkbenchResult::Binary(b)),
                    _ => Err(unexpected_tool_output()),
                }
            }
            BookWorkbenchOp::ReadContent { root_id, rel_path } => {
                let (_, sb, _) = self
                    .workspace
                    .resolve_media_file(root_id, &rel_path, Some(MediaKind::Book))
                    .await?;
                let out = run_blocking_book(
                    &self.toolbox,
                    BookToolCommand::ReadContent { path: sb.abs_path },
                )
                .await?;
                match out {
                    BookToolOutput::Document(d) => Ok(BookWorkbenchResult::Document(d)),
                    _ => Err(unexpected_tool_output()),
                }
            }
            BookWorkbenchOp::ResegmentTxt {
                root_id,
                rel_path,
                chapter_pattern,
                volume_pattern,
            } => {
                let (_, sb, _) = self
                    .workspace
                    .resolve_media_file(root_id, &rel_path, Some(MediaKind::Book))
                    .await?;
                let out = run_blocking_book(
                    &self.toolbox,
                    BookToolCommand::ResegmentTxt {
                        path: sb.abs_path,
                        chapter_pattern,
                        volume_pattern,
                    },
                )
                .await?;
                match out {
                    BookToolOutput::Chapters(c) => Ok(BookWorkbenchResult::Chapters(c)),
                    _ => Err(unexpected_tool_output()),
                }
            }
            BookWorkbenchOp::SaveContent {
                root_id,
                rel_path,
                chapters,
                stylesheets,
                force_horizontal_spine,
            } => {
                let (_, sb, _) = self
                    .workspace
                    .resolve_writable_media_file(root_id, &rel_path, Some(MediaKind::Book))
                    .await?;
                run_blocking_book(
                    &self.toolbox,
                    BookToolCommand::SaveContent {
                        path: sb.abs_path,
                        chapters,
                        stylesheets,
                        force_horizontal_spine,
                    },
                )
                .await?;
                let detail = self
                    .workspace
                    .inspect(root_id, &rel_path, Some(MediaKind::Book))
                    .await?;
                Ok(BookWorkbenchResult::Detail(Box::new(detail)))
            }
            BookWorkbenchOp::ConvertTxtToEpub {
                root_id,
                rel_path,
                chapters,
                stylesheets,
                delete_source_txt,
            } => {
                let (root, sb, _) = self
                    .workspace
                    .resolve_writable_media_file(root_id, &rel_path, Some(MediaKind::Book))
                    .await?;
                let out = run_blocking_book(
                    &self.toolbox,
                    BookToolCommand::ConvertTxtToEpub {
                        path: sb.abs_path,
                        chapters,
                        stylesheets,
                        delete_source_txt,
                    },
                )
                .await?;
                let BookToolOutput::Path(epub_abs) = out else {
                    return Err(unexpected_tool_output());
                };
                let new_rel = epub_abs
                    .strip_prefix(&root.path)
                    .map(path_to_slash)
                    .map_err(|_| DomainError::Storage("generated epub outside root".into()))?;
                let detail = self
                    .workspace
                    .inspect(root_id, &new_rel, Some(MediaKind::Book))
                    .await?;
                Ok(BookWorkbenchResult::Detail(Box::new(detail)))
            }
            BookWorkbenchOp::TransformContent {
                chapters,
                stylesheets,
                options,
            } => {
                let out = run_blocking_book(
                    &self.toolbox,
                    BookToolCommand::TransformContent {
                        chapters,
                        stylesheets,
                        options,
                    },
                )
                .await?;
                match out {
                    BookToolOutput::Transform(t) => Ok(BookWorkbenchResult::Transform(t)),
                    _ => Err(unexpected_tool_output()),
                }
            }
            BookWorkbenchOp::InsertEpubImage {
                root_id,
                rel_path,
                chapter_source_href,
                bytes,
                mime_hint,
            } => {
                let (_, sb, _) = self
                    .workspace
                    .resolve_writable_media_file(root_id, &rel_path, Some(MediaKind::Book))
                    .await?;
                let out = run_blocking_book(
                    &self.toolbox,
                    BookToolCommand::InsertEpubImage {
                        path: sb.abs_path,
                        chapter_source_href,
                        bytes,
                        mime_hint,
                    },
                )
                .await?;
                match out {
                    BookToolOutput::InsertedImage(img) => {
                        Ok(BookWorkbenchResult::InsertedImage(img))
                    }
                    _ => Err(unexpected_tool_output()),
                }
            }
            BookWorkbenchOp::Polish { root_id, rel_path } => {
                let (_, sb, _) = self
                    .workspace
                    .resolve_writable_media_file(root_id, &rel_path, Some(MediaKind::Book))
                    .await?;
                let out = run_blocking_book(
                    &self.toolbox,
                    BookToolCommand::PolishEpub { path: sb.abs_path },
                )
                .await?;
                let BookToolOutput::Polished(report) = out else {
                    return Err(unexpected_tool_output());
                };
                let detail = self
                    .workspace
                    .inspect(root_id, &rel_path, Some(MediaKind::Book))
                    .await?;
                Ok(BookWorkbenchResult::Polished(report, Box::new(detail)))
            }
            BookWorkbenchOp::NormalizeBatch {
                root_id,
                rel_paths,
                options,
            } => {
                let report = self
                    .normalize_books_batch(root_id, &rel_paths, options)
                    .await?;
                Ok(BookWorkbenchResult::NormalizedBatch(report))
            }
        }
    }

    async fn normalize_books_batch(
        &self,
        root_id: RootId,
        rel_paths: &[String],
        options: NormalizeBookToEpubOptions,
    ) -> DomainResult<NormalizeBooksBatchReportDto> {
        if rel_paths.is_empty() {
            return Err(DomainError::Validation("请至少选择一本图书".into()));
        }

        let mut items = Vec::with_capacity(rel_paths.len());
        let mut succeeded = 0usize;
        let mut skipped = 0usize;
        let mut failed = 0usize;
        let mut total_before_bytes = 0u64;
        let mut total_after_bytes = 0u64;

        for rel in rel_paths {
            let resolved = self
                .workspace
                .resolve_writable_media_file(root_id, rel, Some(MediaKind::Book))
                .await;
            let (root, sandbox, _) = match resolved {
                Ok(v) => v,
                Err(err) => {
                    failed += 1;
                    items.push(failed_normalize_item(rel, err));
                    continue;
                }
            };

            let root_path = root.path.clone();
            let res = run_blocking_book(
                &self.toolbox,
                BookToolCommand::NormalizeToEpub {
                    path: sandbox.abs_path,
                    options: options.clone(),
                },
            )
            .await;

            match res {
                Ok(BookToolOutput::Normalized(out)) => {
                    if out.skipped {
                        skipped += 1;
                    } else {
                        succeeded += 1;
                        total_before_bytes = total_before_bytes.saturating_add(out.before_bytes);
                        total_after_bytes = total_after_bytes.saturating_add(out.after_bytes);
                    }
                    let output_rel = out
                        .output_path
                        .strip_prefix(&root_path)
                        .map(path_to_slash)
                        .ok();
                    let backup_rel = out
                        .backup_path
                        .as_ref()
                        .and_then(|bp| bp.strip_prefix(&root_path).ok())
                        .map(path_to_slash);
                    items.push(NormalizeBookItemReportDto {
                        source_rel_path: rel.clone(),
                        output_rel_path: output_rel,
                        backup_rel_path: backup_rel,
                        source_format: out.source_format,
                        before_bytes: out.before_bytes,
                        after_bytes: out.after_bytes,
                        chapter_count: out.chapter_count,
                        compressed_images: out.compressed_images,
                        removed_orphans: out.removed_orphans,
                        deleted_source: out.deleted_source,
                        skipped: out.skipped,
                        skip_reason: out.skip_reason,
                        error: None,
                    });
                }
                Ok(_) => {
                    failed += 1;
                    items.push(failed_normalize_item(rel, unexpected_tool_output()));
                }
                Err(err) => {
                    failed += 1;
                    items.push(failed_normalize_item(rel, err));
                }
            }
        }

        Ok(NormalizeBooksBatchReportDto {
            total: rel_paths.len(),
            succeeded,
            skipped,
            failed,
            total_before_bytes,
            total_after_bytes,
            items,
        })
    }

    pub async fn run_comic(&self, op: ComicWorkbenchOp) -> DomainResult<ComicWorkbenchResult> {
        match op {
            ComicWorkbenchOp::ListPages { root_id, rel_path } => {
                let (_, sb, _) = self
                    .workspace
                    .resolve_media_file(root_id, &rel_path, Some(MediaKind::Comic))
                    .await?;
                let out = run_blocking_comic(
                    &self.toolbox,
                    ComicToolCommand::ListPages { path: sb.abs_path },
                )
                .await?;
                match out {
                    ComicToolOutput::Pages(p) => Ok(ComicWorkbenchResult::Pages(p)),
                    _ => Err(unexpected_tool_output()),
                }
            }
            ComicWorkbenchOp::ReadPage {
                root_id,
                rel_path,
                entry_name,
                max_width,
            } => {
                let (_, sb, _) = self
                    .workspace
                    .resolve_media_file(root_id, &rel_path, Some(MediaKind::Comic))
                    .await?;
                let out = run_blocking_comic(
                    &self.toolbox,
                    ComicToolCommand::ReadPageImage {
                        path: sb.abs_path,
                        entry_name,
                        max_width,
                    },
                )
                .await?;
                match out {
                    ComicToolOutput::Binary(b) => Ok(ComicWorkbenchResult::Binary(b)),
                    _ => Err(unexpected_tool_output()),
                }
            }
            ComicWorkbenchOp::SavePages {
                root_id,
                rel_path,
                pages,
                reencode,
                jpeg_quality,
                max_width,
            } => {
                let (_, sb, _) = self
                    .workspace
                    .resolve_writable_media_file(root_id, &rel_path, Some(MediaKind::Comic))
                    .await?;
                let out = run_blocking_comic(
                    &self.toolbox,
                    ComicToolCommand::SavePages {
                        path: sb.abs_path,
                        pages,
                        reencode,
                        jpeg_quality,
                        max_width,
                    },
                )
                .await?;
                let ComicToolOutput::Optimized(report) = out else {
                    return Err(unexpected_tool_output());
                };
                let detail = self
                    .workspace
                    .inspect(root_id, &rel_path, Some(MediaKind::Comic))
                    .await?;
                Ok(ComicWorkbenchResult::Optimized(report, Box::new(detail)))
            }
            ComicWorkbenchOp::Optimize {
                root_id,
                rel_path,
                reencode,
                jpeg_quality,
                max_width,
            } => {
                let (_, sb, _) = self
                    .workspace
                    .resolve_writable_media_file(root_id, &rel_path, Some(MediaKind::Comic))
                    .await?;
                let out = run_blocking_comic(
                    &self.toolbox,
                    ComicToolCommand::OptimizeCbz {
                        path: sb.abs_path,
                        reencode,
                        jpeg_quality,
                        max_width,
                    },
                )
                .await?;
                let ComicToolOutput::Optimized(report) = out else {
                    return Err(unexpected_tool_output());
                };
                let detail = self
                    .workspace
                    .inspect(root_id, &rel_path, Some(MediaKind::Comic))
                    .await?;
                Ok(ComicWorkbenchResult::Optimized(report, Box::new(detail)))
            }
            ComicWorkbenchOp::Merge {
                root_id,
                source_paths,
                output_rel_path,
                series,
                title,
                volume,
            } => {
                let root = self.settings.get_raw().await?.find_root(root_id)?.clone();
                root.ensure_writable()?;
                let mut abs_sources = Vec::with_capacity(source_paths.len());
                for rel in &source_paths {
                    let sb = self.fs.resolve_existing_in_root(&root, rel)?;
                    abs_sources.push(sb.abs_path);
                }
                let target_rel = if output_rel_path.to_ascii_lowercase().ends_with(".cbz") {
                    output_rel_path
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
                let out = run_blocking_comic(
                    &self.toolbox,
                    ComicToolCommand::MergeToCbz {
                        sources: abs_sources,
                        target_abs,
                        output_rel_path: target_rel_slash.clone(),
                        series,
                        title,
                        volume,
                    },
                )
                .await?;
                let ComicToolOutput::Merged(report) = out else {
                    return Err(unexpected_tool_output());
                };
                let detail = self
                    .workspace
                    .inspect(root_id, &target_rel_slash, Some(MediaKind::Comic))
                    .await?;
                Ok(ComicWorkbenchResult::Merged(report, Box::new(detail)))
            }
        }
    }

    pub async fn run_music(&self, op: MusicWorkbenchOp) -> DomainResult<MusicWorkbenchResult> {
        match op {
            MusicWorkbenchOp::Lyrics {
                root_id,
                rel_path,
                input,
            } => {
                let (_, sb, _) = self
                    .workspace
                    .resolve_writable_media_file(root_id, &rel_path, Some(MediaKind::Music))
                    .await?;
                let out = run_blocking_music(
                    &self.toolbox,
                    MusicToolCommand::ProcessLyrics {
                        path: sb.abs_path,
                        input,
                    },
                )
                .await?;
                let MusicToolOutput::Lyrics(report) = out else {
                    return Err(unexpected_tool_output());
                };
                let detail = self
                    .workspace
                    .inspect(root_id, &rel_path, Some(MediaKind::Music))
                    .await?;
                Ok(MusicWorkbenchResult::Lyrics(report, Box::new(detail)))
            }
            MusicWorkbenchOp::OptimizeCover {
                root_id,
                rel_path,
                max_dimension,
                jpeg_quality,
            } => {
                let (_, sb, _) = self
                    .workspace
                    .resolve_writable_media_file(root_id, &rel_path, Some(MediaKind::Music))
                    .await?;
                let out = run_blocking_music(
                    &self.toolbox,
                    MusicToolCommand::OptimizeCover {
                        path: sb.abs_path,
                        max_dimension,
                        jpeg_quality,
                    },
                )
                .await?;
                let MusicToolOutput::Cover(report) = out else {
                    return Err(unexpected_tool_output());
                };
                let detail = self
                    .workspace
                    .inspect(root_id, &rel_path, Some(MediaKind::Music))
                    .await?;
                Ok(MusicWorkbenchResult::Cover(report, Box::new(detail)))
            }
            MusicWorkbenchOp::ReplayGain { root_id, paths } => {
                let root = self.settings.get_raw().await?.find_root(root_id)?.clone();
                root.ensure_writable()?;
                let mut items = Vec::with_capacity(paths.len());
                for rel in paths {
                    if let Ok(sb) = self.fs.resolve_existing_in_root(&root, &rel)
                        && sb.abs_path.is_file()
                    {
                        items.push((sb.rel_slash_string(), sb.abs_path));
                    }
                }
                let out = run_blocking_music(
                    &self.toolbox,
                    MusicToolCommand::ApplyReplayGain {
                        items,
                        ffmpeg_bin: self.ffmpeg.ffmpeg_bin().to_string(),
                    },
                )
                .await?;
                match out {
                    MusicToolOutput::ReplayGain(r) => Ok(MusicWorkbenchResult::ReplayGain(r)),
                    _ => Err(unexpected_tool_output()),
                }
            }
            MusicWorkbenchOp::SplitCue { root_id, rel_path } => {
                let root = self.settings.get_raw().await?.find_root(root_id)?.clone();
                root.ensure_writable()?;
                let sb = self.fs.resolve_existing_in_root(&root, &rel_path)?;
                let out = run_blocking_music(
                    &self.toolbox,
                    MusicToolCommand::SplitCue {
                        path: sb.abs_path,
                        ffmpeg_bin: self.ffmpeg.ffmpeg_bin().to_string(),
                    },
                )
                .await?;
                match out {
                    MusicToolOutput::CueSplit(r) => Ok(MusicWorkbenchResult::CueSplit(r)),
                    _ => Err(unexpected_tool_output()),
                }
            }
        }
    }

    pub async fn run_video(&self, op: VideoWorkbenchOp) -> DomainResult<VideoWorkbenchResult> {
        match op {
            VideoWorkbenchOp::ListSubtitles { root_id, rel_path } => {
                let (_, sb, _) = self
                    .workspace
                    .resolve_media_file(root_id, &rel_path, Some(MediaKind::Video))
                    .await?;
                let out = run_blocking_video(
                    &self.toolbox,
                    VideoToolCommand::ListSubtitles { path: sb.abs_path },
                )
                .await?;
                match out {
                    VideoToolOutput::Subtitles(s) => Ok(VideoWorkbenchResult::Subtitles(s)),
                    _ => Err(unexpected_tool_output()),
                }
            }
            VideoWorkbenchOp::ProcessSubtitle {
                root_id,
                rel_path,
                input,
            } => {
                let (_, sb, _) = self
                    .workspace
                    .resolve_writable_media_file(root_id, &rel_path, Some(MediaKind::Video))
                    .await?;
                let out = run_blocking_video(
                    &self.toolbox,
                    VideoToolCommand::ProcessSubtitle {
                        path: sb.abs_path,
                        input,
                    },
                )
                .await?;
                let VideoToolOutput::ProcessedSubtitle(report) = out else {
                    return Err(unexpected_tool_output());
                };
                let detail = self
                    .workspace
                    .inspect(root_id, &rel_path, Some(MediaKind::Video))
                    .await?;
                Ok(VideoWorkbenchResult::ProcessedSubtitle(
                    report,
                    Box::new(detail),
                ))
            }
            VideoWorkbenchOp::ExtractSubtitle {
                root_id,
                rel_path,
                stream_index,
                language_tag,
                output_ext,
            } => {
                let (_, sb, _) = self
                    .workspace
                    .resolve_writable_media_file(root_id, &rel_path, Some(MediaKind::Video))
                    .await?;
                let out = run_blocking_video(
                    &self.toolbox,
                    VideoToolCommand::ExtractEmbeddedSubtitle {
                        path: sb.abs_path,
                        stream_index,
                        language_tag,
                        output_ext,
                        ffmpeg_bin: self.ffmpeg.ffmpeg_bin().to_string(),
                    },
                )
                .await?;
                let VideoToolOutput::ExtractedSubtitle(report) = out else {
                    return Err(unexpected_tool_output());
                };
                let detail = self
                    .workspace
                    .inspect(root_id, &rel_path, Some(MediaKind::Video))
                    .await?;
                Ok(VideoWorkbenchResult::ExtractedSubtitle(
                    report,
                    Box::new(detail),
                ))
            }
        }
    }
}

async fn run_blocking_book(
    toolbox: &Arc<dyn MediaToolboxPort>,
    cmd: BookToolCommand,
) -> DomainResult<BookToolOutput> {
    let tb = Arc::clone(toolbox);
    match tokio::task::spawn_blocking(move || tb.execute(ToolboxCommand::Book(cmd)))
        .await
        .map_err(|e| DomainError::Storage(format!("book tool task failed: {e}")))??
    {
        ToolboxOutput::Book(out) => Ok(out),
        _ => Err(unexpected_tool_output()),
    }
}

async fn run_blocking_comic(
    toolbox: &Arc<dyn MediaToolboxPort>,
    cmd: ComicToolCommand,
) -> DomainResult<ComicToolOutput> {
    let tb = Arc::clone(toolbox);
    match tokio::task::spawn_blocking(move || tb.execute(ToolboxCommand::Comic(cmd)))
        .await
        .map_err(|e| DomainError::Storage(format!("comic tool task failed: {e}")))??
    {
        ToolboxOutput::Comic(out) => Ok(out),
        _ => Err(unexpected_tool_output()),
    }
}

async fn run_blocking_music(
    toolbox: &Arc<dyn MediaToolboxPort>,
    cmd: MusicToolCommand,
) -> DomainResult<MusicToolOutput> {
    let tb = Arc::clone(toolbox);
    match tokio::task::spawn_blocking(move || tb.execute(ToolboxCommand::Music(cmd)))
        .await
        .map_err(|e| DomainError::Storage(format!("music tool task failed: {e}")))??
    {
        ToolboxOutput::Music(out) => Ok(out),
        _ => Err(unexpected_tool_output()),
    }
}

async fn run_blocking_video(
    toolbox: &Arc<dyn MediaToolboxPort>,
    cmd: VideoToolCommand,
) -> DomainResult<VideoToolOutput> {
    let tb = Arc::clone(toolbox);
    match tokio::task::spawn_blocking(move || tb.execute(ToolboxCommand::Video(cmd)))
        .await
        .map_err(|e| DomainError::Storage(format!("video tool task failed: {e}")))??
    {
        ToolboxOutput::Video(out) => Ok(out),
        _ => Err(unexpected_tool_output()),
    }
}

fn unexpected_tool_output() -> DomainError {
    DomainError::Storage("unexpected media toolbox output variant".into())
}

fn failed_normalize_item(rel: &str, err: DomainError) -> NormalizeBookItemReportDto {
    NormalizeBookItemReportDto {
        source_rel_path: rel.to_string(),
        output_rel_path: None,
        backup_rel_path: None,
        source_format: Path::new(rel)
            .extension()
            .and_then(|s| s.to_str())
            .unwrap_or_default()
            .to_ascii_lowercase(),
        before_bytes: 0,
        after_bytes: 0,
        chapter_count: 0,
        compressed_images: 0,
        removed_orphans: 0,
        deleted_source: false,
        skipped: false,
        skip_reason: None,
        error: Some(err.to_string()),
    }
}

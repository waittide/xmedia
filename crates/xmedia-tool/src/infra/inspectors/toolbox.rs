use std::path::{Path, PathBuf};

use xmedia_core::application::ports::BinaryPayload;
use xmedia_core::domain::error::DomainResult;

use crate::domain::media::VideoFileMeta;
use crate::domain::ops::{
    BookContentDocumentDto, ComicImageReencodeFormat, ComicOptimizeReportDto,
    ComicPagesSnapshotDto, CueSplitReportDto, EditableBookChapterDto, ExtractSubtitleReportDto,
    InsertedEpubImageDto, MergeComicsReportDto, MusicCoverOptimizeReportDto, MusicLyricsToolInput,
    MusicLyricsToolReportDto, NormalizeBookOutcome, NormalizeBookToEpubOptions,
    PolishBookReportDto, ProcessSubtitleInput, ProcessSubtitleReportDto, ReplayGainReportDto,
    SaveBookChapterInput, SaveComicPageInput, TransformBookContentResultDto, VideoSubtitleInfoDto,
};
use crate::domain::ports::MediaToolboxPort;
use crate::domain::zh_text::BookTextTransformOptions;

use super::{
    book, book_content, book_normalize, comic, comic_ops, music, music_ops, video, video_ops,
};

#[derive(Debug, Default, Clone, Copy)]
pub struct DefaultMediaToolbox;

impl MediaToolboxPort for DefaultMediaToolbox {
    fn read_epub_internal_asset(
        &self,
        path: &Path,
        asset_path: &str,
    ) -> DomainResult<BinaryPayload> {
        book_content::read_epub_internal_asset(path, asset_path)
    }

    fn read_book_content(&self, path: &Path) -> DomainResult<BookContentDocumentDto> {
        book_content::read_book_content(path)
    }

    fn resegment_txt_file(
        &self,
        path: &Path,
        chapter_pattern: Option<&str>,
        volume_pattern: Option<&str>,
    ) -> DomainResult<Vec<EditableBookChapterDto>> {
        book_content::resegment_txt_file(path, chapter_pattern, volume_pattern)
    }

    fn save_book_content(
        &self,
        path: &Path,
        chapters: &[SaveBookChapterInput],
        stylesheets: Option<&[String]>,
        force_horizontal_spine: bool,
    ) -> DomainResult<()> {
        book_content::save_book_content(path, chapters, stylesheets, force_horizontal_spine)
    }

    fn convert_txt_to_epub(
        &self,
        path: &Path,
        chapters: Option<&[SaveBookChapterInput]>,
        stylesheets: Option<&[String]>,
        delete_source_txt: bool,
    ) -> DomainResult<PathBuf> {
        book_content::convert_txt_to_epub(path, chapters, stylesheets, delete_source_txt)
    }

    fn transform_book_content_batch(
        &self,
        chapters: Vec<EditableBookChapterDto>,
        stylesheets: Vec<String>,
        options: &BookTextTransformOptions,
    ) -> DomainResult<TransformBookContentResultDto> {
        book_content::transform_book_content_batch(chapters, stylesheets, options)
    }

    fn insert_epub_image(
        &self,
        path: &Path,
        chapter_source_href: Option<&str>,
        bytes: &[u8],
        mime_hint: Option<&str>,
    ) -> DomainResult<InsertedEpubImageDto> {
        book_content::insert_epub_image(path, chapter_source_href, bytes, mime_hint)
    }

    fn polish_epub_book(&self, path: &Path) -> DomainResult<PolishBookReportDto> {
        book_content::polish_epub_book(path)
    }

    fn normalize_book_to_epub(
        &self,
        path: &Path,
        opts: &NormalizeBookToEpubOptions,
    ) -> DomainResult<NormalizeBookOutcome> {
        book_normalize::normalize_book_to_epub(path, opts)
    }

    fn list_comic_pages(&self, path: &Path) -> DomainResult<ComicPagesSnapshotDto> {
        comic_ops::list_comic_pages(path)
    }

    fn read_comic_page_image(
        &self,
        path: &Path,
        entry_name: &str,
        max_width: Option<u32>,
    ) -> DomainResult<BinaryPayload> {
        comic_ops::read_comic_page_image(path, entry_name, max_width)
    }

    fn save_comic_pages(
        &self,
        path: &Path,
        pages: &[SaveComicPageInput],
        reencode: ComicImageReencodeFormat,
        jpeg_quality: u8,
        max_width: Option<u32>,
    ) -> DomainResult<ComicOptimizeReportDto> {
        comic_ops::save_comic_pages(path, pages, reencode, jpeg_quality, max_width)
    }

    fn optimize_comic_cbz(
        &self,
        path: &Path,
        reencode: ComicImageReencodeFormat,
        jpeg_quality: u8,
        max_width: Option<u32>,
    ) -> DomainResult<ComicOptimizeReportDto> {
        comic_ops::optimize_comic_cbz(path, reencode, jpeg_quality, max_width)
    }

    fn merge_comics_to_cbz(
        &self,
        sources: &[PathBuf],
        target_abs: &Path,
        output_rel_path: String,
        series: Option<String>,
        title: Option<String>,
        volume: Option<f64>,
    ) -> DomainResult<MergeComicsReportDto> {
        comic_ops::merge_comics_to_cbz(sources, target_abs, output_rel_path, series, title, volume)
    }

    fn process_music_lyrics(
        &self,
        path: &Path,
        input: &MusicLyricsToolInput,
    ) -> DomainResult<MusicLyricsToolReportDto> {
        music_ops::process_music_lyrics(path, input)
    }

    fn optimize_music_cover(
        &self,
        path: &Path,
        max_dimension: u32,
        jpeg_quality: u8,
    ) -> DomainResult<MusicCoverOptimizeReportDto> {
        music_ops::optimize_music_cover(path, max_dimension, jpeg_quality)
    }

    fn apply_replaygain_batch(
        &self,
        items: &[(String, PathBuf)],
        ffmpeg_bin: &str,
    ) -> DomainResult<ReplayGainReportDto> {
        music_ops::apply_replaygain_batch(items, ffmpeg_bin)
    }

    fn split_cue_album(&self, path: &Path, ffmpeg_bin: &str) -> DomainResult<CueSplitReportDto> {
        music_ops::split_cue_album(path, ffmpeg_bin)
    }

    fn list_video_subtitles(&self, path: &Path) -> DomainResult<Vec<VideoSubtitleInfoDto>> {
        video_ops::list_video_subtitles(path)
    }

    fn process_video_subtitle(
        &self,
        path: &Path,
        input: &ProcessSubtitleInput,
    ) -> DomainResult<ProcessSubtitleReportDto> {
        video_ops::process_video_subtitle(path, input)
    }

    fn extract_embedded_subtitle(
        &self,
        path: &Path,
        stream_index: u32,
        language_tag: Option<&str>,
        output_ext: &str,
        ffmpeg_bin: &str,
    ) -> DomainResult<ExtractSubtitleReportDto> {
        video_ops::extract_embedded_subtitle(
            path,
            stream_index,
            language_tag,
            output_ext,
            ffmpeg_bin,
        )
    }

    fn read_base_video_meta(&self, path: &Path) -> VideoFileMeta {
        video::read_base_video_meta(path)
    }

    fn find_existing_video_nfo(&self, path: &Path, is_episode: bool) -> Option<PathBuf> {
        video::find_existing_nfo(path, is_episode)
    }

    fn find_show_nfo_for_episode(&self, path: &Path) -> Option<PathBuf> {
        video::find_show_nfo_for_episode(path)
    }

    fn render_video_nfo_xml(&self, meta: &VideoFileMeta, existing_xml: Option<&[u8]>) -> String {
        video::render_video_nfo_xml_preserving(meta, existing_xml)
    }

    fn render_tvshow_nfo_xml(
        &self,
        meta: &VideoFileMeta,
        show_plot: Option<&str>,
        existing_xml: Option<&[u8]>,
    ) -> String {
        video::render_tvshow_nfo_xml_preserving(meta, show_plot, existing_xml)
    }

    fn render_season_nfo_xml(
        &self,
        season_num: u32,
        title: Option<&str>,
        year: Option<i32>,
        plot: Option<&str>,
        tmdb_id: Option<&str>,
        bangumi_id: Option<&str>,
        existing_xml: Option<&[u8]>,
    ) -> String {
        video::render_season_nfo_xml_preserving(
            season_num,
            title,
            year,
            plot,
            tmdb_id,
            bangumi_id,
            existing_xml,
        )
    }

    fn dir_has_single_book(&self, dir: &Path) -> bool {
        book::dir_has_single_book(dir)
    }

    fn is_generic_book_folder(&self, name: &str) -> bool {
        book::is_generic_book_folder(name)
    }

    fn is_generic_comic_folder(&self, name: &str) -> bool {
        comic::is_generic_comic_folder(name)
    }

    fn is_generic_music_folder(&self, name: &str) -> bool {
        music::is_generic_music_folder(name)
    }

    fn dir_has_single_video(&self, dir: &Path) -> bool {
        video::dir_has_single_video(dir)
    }

    fn is_season_dir(&self, dir: &Path) -> bool {
        video::is_season_dir(dir)
    }
}

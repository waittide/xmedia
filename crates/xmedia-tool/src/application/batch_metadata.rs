mod patch_builder;
mod replacer;

#[cfg(test)]
mod tests;

use serde::{Deserialize, Serialize};
use uuid::Uuid;
use xmedia_core::domain::error::DomainResult;
use xmedia_core::domain::library::MediaKind;

use crate::domain::media::{
    BookMetaPatch, ComicMetaPatch, MediaMetadataPatch, MediaMetadataPayload, MusicMetaPatch,
    VideoMetaPatch,
};
use crate::infra::inspectors::zh_text::ZhConvertDirection;

pub use patch_builder::PreparedBatchMetadataEdit;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MetadataSnapshotRecord {
    pub rel_path: String,
    pub file_name: String,
    pub previous: MediaMetadataPayload,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MetadataHistoryBatchDto {
    pub batch_id: Uuid,
    pub root_id: Uuid,
    pub media_kind: MediaKind,
    pub label: String,
    pub created_at_unix: i64,
    pub updated_count: usize,
    pub records: Vec<MetadataSnapshotRecord>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UndoMetadataReportDto {
    pub batch_id: Uuid,
    pub restored_count: usize,
    pub failed_paths: Vec<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct BatchTextReplaceRule {
    pub find: String,
    #[serde(default)]
    pub replace: String,
    #[serde(default)]
    pub use_regex: bool,
    /// Target fields: "title", "series", "creator", "summary". Empty defaults to ["title", "series"].
    #[serde(default)]
    pub fields: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BatchMetadataEditInput {
    pub paths: Vec<String>,
    pub media_kind: MediaKind,
    /// Optional title template supporting `${filename}`, `${title}`, `${creator}`, `${series}`, `${year}`, `${counter}`.
    #[serde(default)]
    pub title_template: Option<String>,
    #[serde(default)]
    pub creator: Option<String>,
    #[serde(default)]
    pub series_or_album: Option<String>,
    #[serde(default)]
    pub publisher_or_studio: Option<String>,
    #[serde(default)]
    pub year: Option<i32>,
    #[serde(default)]
    pub language: Option<String>,
    #[serde(default)]
    pub genre: Option<String>,
    #[serde(default)]
    pub add_tags: Vec<String>,
    #[serde(default)]
    pub remove_tags: Vec<String>,
    #[serde(default)]
    pub replace_rule: Option<BatchTextReplaceRule>,
    #[serde(default)]
    pub zh_direction: Option<ZhConvertDirection>,
}

impl Default for BatchMetadataEditInput {
    fn default() -> Self {
        Self {
            paths: Vec::new(),
            media_kind: MediaKind::Book,
            title_template: None,
            creator: None,
            series_or_album: None,
            publisher_or_studio: None,
            year: None,
            language: None,
            genre: None,
            add_tags: Vec::new(),
            remove_tags: Vec::new(),
            replace_rule: None,
            zh_direction: None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BatchMetadataEditReportDto {
    pub batch_id: Option<Uuid>,
    pub updated_count: usize,
    pub skipped_count: usize,
    pub failed_count: usize,
    pub errors: Vec<String>,
}

/// Converts a full inspected `MediaMetadataPayload` back into a lossless `MediaMetadataPatch`
/// so a file can be rolled back to its exact pre-edit state.
pub fn payload_to_restore_patch(payload: &MediaMetadataPayload) -> MediaMetadataPatch {
    match payload {
        MediaMetadataPayload::Book(b) => MediaMetadataPatch::Book(BookMetaPatch {
            title: Some(b.title.clone()),
            authors: Some(b.authors.clone()),
            series: Some(b.series.clone().unwrap_or_default()),
            subseries: Some(b.subseries.clone().unwrap_or_default()),
            series_index: Some(b.series_index.clone().unwrap_or_default()),
            rating: Some(b.rating.unwrap_or(-1.0)),
            description: Some(b.description.clone().unwrap_or_default()),
            publisher: Some(b.publisher.clone().unwrap_or_default()),
            published_at: Some(b.published_at.clone().unwrap_or_default()),
            language: Some(b.language.clone().unwrap_or_default()),
            isbn: Some(b.isbn.clone().unwrap_or_default()),
            tags: Some(b.tags.clone()),
            locked_fields: Some(b.locked_fields.clone()),
        }),
        MediaMetadataPayload::Comic(c) => MediaMetadataPatch::Comic(ComicMetaPatch {
            series: Some(c.series.clone().unwrap_or_default()),
            series_group: Some(c.series_group.clone().unwrap_or_default()),
            title: Some(c.title.clone().unwrap_or_default()),
            number: Some(c.number.clone().unwrap_or_default()),
            volume: Some(c.volume.unwrap_or(-1.0)),
            summary: Some(c.summary.clone().unwrap_or_default()),
            writers: Some(c.writers.clone()),
            pencillers: Some(c.pencillers.clone()),
            publisher: Some(c.publisher.clone().unwrap_or_default()),
            genres: Some(c.genres.clone()),
            tags: Some(c.tags.clone()),
            year: Some(c.year.unwrap_or(0)),
            language_iso: Some(c.language_iso.clone().unwrap_or_default()),
            reading_direction: Some(c.reading_direction.clone()),
        }),
        MediaMetadataPayload::Music(m) => MediaMetadataPatch::Music(MusicMetaPatch {
            title: Some(m.title.clone()),
            artist: Some(m.artist.clone()),
            album_artist: Some(m.album_artist.clone().unwrap_or_default()),
            album: Some(m.album.clone()),
            track_number: Some(m.track_number.unwrap_or(0)),
            disc_number: Some(m.disc_number.unwrap_or(0)),
            year: Some(m.year.unwrap_or(0)),
            genre: Some(m.genre.clone().unwrap_or_default()),
            comment: Some(m.comment.clone().unwrap_or_default()),
            embedded_lyrics: Some(m.embedded_lyrics.clone().unwrap_or_default()),
            sidecar_lrc: Some(m.sidecar_lrc.clone().unwrap_or_default()),
        }),
        MediaMetadataPayload::Video(v) => MediaMetadataPatch::Video(VideoMetaPatch {
            is_episode: Some(v.is_episode),
            title: Some(v.title.clone()),
            original_title: Some(v.original_title.clone().unwrap_or_default()),
            show_title: Some(v.show_title.clone().unwrap_or_default()),
            season_number: Some(v.season_number.map(|s| s as i32).unwrap_or(-1)),
            episode_number: Some(v.episode_number.unwrap_or(0)),
            year: Some(v.year.unwrap_or(0)),
            rating: Some(v.rating.unwrap_or(0.0)),
            summary: Some(v.summary.clone().unwrap_or_default()),
            genres: Some(v.genres.clone()),
            tags: Some(v.tags.clone()),
            actors: Some(v.actors.clone()),
            directors: Some(v.directors.clone()),
            studio: Some(v.studio.clone().unwrap_or_default()),
            collection_set: Some(v.collection_set.clone().unwrap_or_default()),
            tmdb_id: Some(v.tmdb_id.clone().unwrap_or_default()),
            bangumi_id: Some(v.bangumi_id.clone().unwrap_or_default()),
            locked_fields: Some(v.locked_fields.clone()),
        }),
    }
}

/// Computes a domain-specific `MediaMetadataPatch` for one file from a unified `BatchMetadataEditInput`.
/// Returns `Ok(None)` when no effective field changed for this file.
pub fn build_batch_patch_for_payload(
    payload: &MediaMetadataPayload,
    file_stem: &str,
    counter: usize,
    input: &BatchMetadataEditInput,
) -> DomainResult<Option<MediaMetadataPatch>> {
    let prepared = PreparedBatchMetadataEdit::prepare(input)?;
    Ok(prepared.build_patch(payload, file_stem, counter))
}

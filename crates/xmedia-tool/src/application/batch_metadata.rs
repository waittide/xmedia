use std::collections::BTreeSet;

use regex::Regex;
use serde::{Deserialize, Serialize};
use uuid::Uuid;
use xmedia_core::domain::error::{DomainError, DomainResult};
use xmedia_core::domain::library::MediaKind;

use crate::domain::media::{
    BookMetaPatch, ComicMetaPatch, MediaMetadataPatch, MediaMetadataPayload, MusicMetaPatch,
    VideoMetaPatch,
};
use crate::infra::inspectors::zh_text::{ZhConvertDirection, convert_zh_plain};

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
            volume: Some(c.volume.unwrap_or(-1)),
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

fn non_empty_trimmed(opt: &Option<String>) -> Option<String> {
    opt.as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
}

fn split_tokens(raw: &str) -> Vec<String> {
    raw.split(['/', ';', '；', '、', '\n'])
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
        .collect()
}

fn merge_tag_edits(current: &[String], add: &[String], remove: &[String]) -> Option<Vec<String>> {
    if add.is_empty() && remove.is_empty() {
        return None;
    }
    let remove_lower: BTreeSet<String> = remove
        .iter()
        .map(|s| s.trim().to_lowercase())
        .filter(|s| !s.is_empty())
        .collect();
    let mut seen: BTreeSet<String> = BTreeSet::new();
    let mut out: Vec<String> = Vec::new();
    for t in current {
        let trimmed = t.trim();
        let low = trimmed.to_lowercase();
        if !trimmed.is_empty() && !remove_lower.contains(&low) && seen.insert(low) {
            out.push(trimmed.to_string());
        }
    }
    for t in add {
        let trimmed = t.trim();
        let low = trimmed.to_lowercase();
        if !trimmed.is_empty() && !remove_lower.contains(&low) && seen.insert(low) {
            out.push(trimmed.to_string());
        }
    }
    if out == current { None } else { Some(out) }
}

fn render_title_template(
    tpl: &str,
    file_stem: &str,
    cur_title: &str,
    cur_creator: &str,
    cur_series: &str,
    cur_year: &str,
    counter: usize,
) -> String {
    tpl.replace("${filename}", file_stem)
        .replace("${title}", cur_title)
        .replace("${creator}", cur_creator)
        .replace("${series}", cur_series)
        .replace("${year}", cur_year)
        .replace("${counter}", &counter.to_string())
        .replace("${counter:2}", &format!("{counter:02}"))
        .replace("${counter:3}", &format!("{counter:03}"))
        .trim()
        .to_string()
}

struct CompiledReplacer {
    regex: Option<Regex>,
    literal: String,
    replace: String,
    target_title: bool,
    target_series: bool,
    target_creator: bool,
    target_summary: bool,
}

impl CompiledReplacer {
    fn compile(rule: &BatchTextReplaceRule) -> DomainResult<Option<Self>> {
        if rule.find.is_empty() {
            return Ok(None);
        }
        let regex = if rule.use_regex {
            Some(
                Regex::new(&rule.find)
                    .map_err(|e| DomainError::Validation(format!("无效的正则表达式: {e}")))?,
            )
        } else {
            None
        };
        let has = |f: &str| {
            if rule.fields.is_empty() {
                f == "title" || f == "series"
            } else {
                rule.fields.iter().any(|x| x.eq_ignore_ascii_case(f))
            }
        };
        Ok(Some(Self {
            regex,
            literal: rule.find.clone(),
            replace: rule.replace.clone(),
            target_title: has("title"),
            target_series: has("series"),
            target_creator: has("creator"),
            target_summary: has("summary"),
        }))
    }

    fn apply(&self, input: &str) -> String {
        if let Some(ref re) = self.regex {
            re.replace_all(input, self.replace.as_str()).into_owned()
        } else {
            input.replace(&self.literal, &self.replace)
        }
    }
}

fn transform_str(
    cur: &str,
    replacer: Option<&CompiledReplacer>,
    should_replace: bool,
    zh_dir: Option<ZhConvertDirection>,
) -> Option<String> {
    let mut next = cur.to_string();
    if should_replace && let Some(r) = replacer {
        next = r.apply(&next);
    }
    if let Some(dir) = zh_dir {
        next = convert_zh_plain(&next, dir);
    }
    let next = next.trim().to_string();
    if next == cur.trim() { None } else { Some(next) }
}

/// Computes a domain-specific `MediaMetadataPatch` for one file from a unified `BatchMetadataEditInput`.
/// Returns `Ok(None)` when no effective field changed for this file.
pub fn build_batch_patch_for_payload(
    payload: &MediaMetadataPayload,
    file_stem: &str,
    counter: usize,
    input: &BatchMetadataEditInput,
) -> DomainResult<Option<MediaMetadataPatch>> {
    let replacer = match input.replace_rule.as_ref() {
        Some(rule) => CompiledReplacer::compile(rule)?,
        None => None,
    };
    let title_tpl = non_empty_trimmed(&input.title_template);
    let creator_val = non_empty_trimmed(&input.creator);
    let series_val = non_empty_trimmed(&input.series_or_album);
    let publisher_val = non_empty_trimmed(&input.publisher_or_studio);
    let lang_val = non_empty_trimmed(&input.language);
    let genre_val = non_empty_trimmed(&input.genre);
    let zh = input.zh_direction;

    match payload {
        MediaMetadataPayload::Book(b) => {
            let mut patch = BookMetaPatch::default();
            let mut changed = false;

            let cur_creator = b.authors.first().map(String::as_str).unwrap_or("");
            let cur_series = b.series.as_deref().unwrap_or("");
            let cur_year = b.published_at.as_deref().unwrap_or("");

            let base_title = if let Some(ref tpl) = title_tpl {
                render_title_template(
                    tpl,
                    file_stem,
                    &b.title,
                    cur_creator,
                    cur_series,
                    cur_year,
                    counter,
                )
            } else {
                b.title.clone()
            };
            let final_title = transform_str(
                &base_title,
                replacer.as_ref(),
                replacer.as_ref().is_some_and(|r| r.target_title),
                zh,
            )
            .unwrap_or(base_title);
            if final_title != b.title && !final_title.is_empty() {
                patch.title = Some(final_title);
                changed = true;
            }

            if let Some(ref c) = creator_val {
                let toks = split_tokens(c);
                if !toks.is_empty() && toks != b.authors {
                    patch.authors = Some(toks);
                    changed = true;
                }
            } else if replacer.as_ref().is_some_and(|r| r.target_creator) || zh.is_some() {
                let next_authors: Vec<String> = b
                    .authors
                    .iter()
                    .map(|a| {
                        transform_str(
                            a,
                            replacer.as_ref(),
                            replacer.as_ref().is_some_and(|r| r.target_creator),
                            zh,
                        )
                        .unwrap_or_else(|| a.clone())
                    })
                    .collect();
                if next_authors != b.authors {
                    patch.authors = Some(next_authors);
                    changed = true;
                }
            }

            if let Some(ref s) = series_val {
                if b.series.as_deref() != Some(s.as_str()) {
                    patch.series = Some(s.clone());
                    changed = true;
                }
            } else if let Some(ref s) = b.series
                && let Some(ns) = transform_str(
                    s,
                    replacer.as_ref(),
                    replacer.as_ref().is_some_and(|r| r.target_series),
                    zh,
                )
            {
                patch.series = Some(ns);
                changed = true;
            }

            if let Some(ref p) = publisher_val
                && b.publisher.as_deref() != Some(p.as_str())
            {
                patch.publisher = Some(p.clone());
                changed = true;
            }
            if let Some(y) = input.year
                && y > 0
            {
                let ys = y.to_string();
                if b.published_at.as_deref() != Some(ys.as_str()) {
                    patch.published_at = Some(ys);
                    changed = true;
                }
            }
            if let Some(ref l) = lang_val
                && b.language.as_deref() != Some(l.as_str())
            {
                patch.language = Some(l.clone());
                changed = true;
            }

            let mut tags_add = input.add_tags.clone();
            if let Some(ref g) = genre_val {
                tags_add.extend(split_tokens(g));
            }
            if let Some(next_tags) = merge_tag_edits(&b.tags, &tags_add, &input.remove_tags) {
                patch.tags = Some(next_tags);
                changed = true;
            }

            if let Some(ref desc) = b.description
                && let Some(nd) = transform_str(
                    desc,
                    replacer.as_ref(),
                    replacer.as_ref().is_some_and(|r| r.target_summary),
                    zh,
                )
            {
                patch.description = Some(nd);
                changed = true;
            }

            Ok(changed.then_some(MediaMetadataPatch::Book(patch)))
        }

        MediaMetadataPayload::Comic(c) => {
            let mut patch = ComicMetaPatch::default();
            let mut changed = false;

            let cur_title = c.title.as_deref().unwrap_or("");
            let cur_series = c.series.as_deref().unwrap_or("");
            let cur_creator = c.writers.first().map(String::as_str).unwrap_or("");
            let cur_year = c.year.map(|y| y.to_string()).unwrap_or_default();

            let base_title = if let Some(ref tpl) = title_tpl {
                render_title_template(
                    tpl,
                    file_stem,
                    cur_title,
                    cur_creator,
                    cur_series,
                    &cur_year,
                    counter,
                )
            } else {
                cur_title.to_string()
            };
            let final_title = transform_str(
                &base_title,
                replacer.as_ref(),
                replacer.as_ref().is_some_and(|r| r.target_title),
                zh,
            )
            .unwrap_or(base_title);
            if final_title != cur_title && !final_title.is_empty() {
                patch.title = Some(final_title);
                changed = true;
            }

            if let Some(ref s) = series_val {
                if c.series.as_deref() != Some(s.as_str()) {
                    patch.series = Some(s.clone());
                    changed = true;
                }
            } else if let Some(ref s) = c.series
                && let Some(ns) = transform_str(
                    s,
                    replacer.as_ref(),
                    replacer.as_ref().is_some_and(|r| r.target_series),
                    zh,
                )
            {
                patch.series = Some(ns);
                changed = true;
            }

            if let Some(ref cr) = creator_val {
                let toks = split_tokens(cr);
                if !toks.is_empty() && toks != c.writers {
                    patch.writers = Some(toks);
                    changed = true;
                }
            }
            if let Some(ref p) = publisher_val
                && c.publisher.as_deref() != Some(p.as_str())
            {
                patch.publisher = Some(p.clone());
                changed = true;
            }
            if let Some(y) = input.year
                && y > 0
                && c.year != Some(y)
            {
                patch.year = Some(y);
                changed = true;
            }
            if let Some(ref l) = lang_val
                && c.language_iso.as_deref() != Some(l.as_str())
            {
                patch.language_iso = Some(l.clone());
                changed = true;
            }
            if let Some(ref g) = genre_val {
                let toks = split_tokens(g);
                if !toks.is_empty() && toks != c.genres {
                    patch.genres = Some(toks);
                    changed = true;
                }
            }
            if let Some(next_tags) = merge_tag_edits(&c.tags, &input.add_tags, &input.remove_tags) {
                patch.tags = Some(next_tags);
                changed = true;
            }
            if let Some(ref sum) = c.summary
                && let Some(ns) = transform_str(
                    sum,
                    replacer.as_ref(),
                    replacer.as_ref().is_some_and(|r| r.target_summary),
                    zh,
                )
            {
                patch.summary = Some(ns);
                changed = true;
            }

            Ok(changed.then_some(MediaMetadataPatch::Comic(patch)))
        }

        MediaMetadataPayload::Music(m) => {
            let mut patch = MusicMetaPatch::default();
            let mut changed = false;

            let cur_year = m.year.map(|y| y.to_string()).unwrap_or_default();
            let base_title = if let Some(ref tpl) = title_tpl {
                render_title_template(
                    tpl, file_stem, &m.title, &m.artist, &m.album, &cur_year, counter,
                )
            } else {
                m.title.clone()
            };
            let final_title = transform_str(
                &base_title,
                replacer.as_ref(),
                replacer.as_ref().is_some_and(|r| r.target_title),
                zh,
            )
            .unwrap_or(base_title);
            if final_title != m.title && !final_title.is_empty() {
                patch.title = Some(final_title);
                changed = true;
            }

            if let Some(ref cr) = creator_val {
                if m.artist != *cr {
                    patch.artist = Some(cr.clone());
                    changed = true;
                }
            } else if let Some(nc) = transform_str(
                &m.artist,
                replacer.as_ref(),
                replacer.as_ref().is_some_and(|r| r.target_creator),
                zh,
            ) {
                patch.artist = Some(nc);
                changed = true;
            }

            if let Some(ref s) = series_val {
                if m.album != *s {
                    patch.album = Some(s.clone());
                    changed = true;
                }
            } else if let Some(na) = transform_str(
                &m.album,
                replacer.as_ref(),
                replacer.as_ref().is_some_and(|r| r.target_series),
                zh,
            ) {
                patch.album = Some(na);
                changed = true;
            }

            if let Some(ref p) = publisher_val
                && m.album_artist.as_deref() != Some(p.as_str())
            {
                patch.album_artist = Some(p.clone());
                changed = true;
            }
            if let Some(y) = input.year
                && y > 0
                && m.year != Some(y)
            {
                patch.year = Some(y);
                changed = true;
            }
            if let Some(ref g) = genre_val
                && m.genre.as_deref() != Some(g.as_str())
            {
                patch.genre = Some(g.clone());
                changed = true;
            }
            if let Some(ref cmt) = m.comment
                && let Some(nc) = transform_str(
                    cmt,
                    replacer.as_ref(),
                    replacer.as_ref().is_some_and(|r| r.target_summary),
                    zh,
                )
            {
                patch.comment = Some(nc);
                changed = true;
            }

            Ok(changed.then_some(MediaMetadataPatch::Music(patch)))
        }

        MediaMetadataPayload::Video(v) => {
            let mut patch = VideoMetaPatch::default();
            let mut changed = false;

            let cur_series = v.show_title.as_deref().unwrap_or("");
            let cur_creator = v.directors.first().map(String::as_str).unwrap_or("");
            let cur_year = v.year.map(|y| y.to_string()).unwrap_or_default();

            let base_title = if let Some(ref tpl) = title_tpl {
                render_title_template(
                    tpl,
                    file_stem,
                    &v.title,
                    cur_creator,
                    cur_series,
                    &cur_year,
                    counter,
                )
            } else {
                v.title.clone()
            };
            let final_title = transform_str(
                &base_title,
                replacer.as_ref(),
                replacer.as_ref().is_some_and(|r| r.target_title),
                zh,
            )
            .unwrap_or(base_title);
            if final_title != v.title && !final_title.is_empty() {
                patch.title = Some(final_title);
                changed = true;
            }

            if let Some(ref s) = series_val {
                if v.show_title.as_deref() != Some(s.as_str()) {
                    patch.show_title = Some(s.clone());
                    changed = true;
                }
            } else if let Some(ref s) = v.show_title
                && let Some(ns) = transform_str(
                    s,
                    replacer.as_ref(),
                    replacer.as_ref().is_some_and(|r| r.target_series),
                    zh,
                )
            {
                patch.show_title = Some(ns);
                changed = true;
            }

            if let Some(ref cr) = creator_val {
                let toks = split_tokens(cr);
                if !toks.is_empty() && toks != v.directors {
                    patch.directors = Some(toks);
                    changed = true;
                }
            }
            if let Some(ref p) = publisher_val
                && v.studio.as_deref() != Some(p.as_str())
            {
                patch.studio = Some(p.clone());
                changed = true;
            }
            if let Some(y) = input.year
                && y > 0
                && v.year != Some(y)
            {
                patch.year = Some(y);
                changed = true;
            }
            if let Some(ref g) = genre_val {
                let toks = split_tokens(g);
                if !toks.is_empty() && toks != v.genres {
                    patch.genres = Some(toks);
                    changed = true;
                }
            }
            if let Some(next_tags) = merge_tag_edits(&v.tags, &input.add_tags, &input.remove_tags) {
                patch.tags = Some(next_tags);
                changed = true;
            }
            if let Some(ref sum) = v.summary
                && let Some(ns) = transform_str(
                    sum,
                    replacer.as_ref(),
                    replacer.as_ref().is_some_and(|r| r.target_summary),
                    zh,
                )
            {
                patch.summary = Some(ns);
                changed = true;
            }

            Ok(changed.then_some(MediaMetadataPatch::Video(patch)))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::media::{BookFileMeta, MusicFileMeta};

    #[test]
    fn batch_edit_supports_regex_watermark_removal_tags_and_zh_convert() {
        let payload = MediaMetadataPayload::Book(BookFileMeta {
            title: "【XX下載站】三體全集".into(),
            authors: vec!["劉慈欣".into()],
            tags: vec!["舊標籤".into(), "科幻".into()],
            ..Default::default()
        });
        let input = BatchMetadataEditInput {
            media_kind: MediaKind::Book,
            add_tags: vec!["雨果奖".into()],
            remove_tags: vec!["舊標籤".into()],
            replace_rule: Some(BatchTextReplaceRule {
                find: r"【[^】]+】".into(),
                replace: String::new(),
                use_regex: true,
                fields: vec!["title".into()],
            }),
            zh_direction: Some(ZhConvertDirection::T2s),
            ..Default::default()
        };

        let patch = build_batch_patch_for_payload(&payload, "santi", 1, &input)
            .unwrap()
            .expect("should produce book patch");
        let MediaMetadataPatch::Book(bp) = patch else {
            panic!("expected Book patch");
        };
        assert_eq!(bp.title.as_deref(), Some("三体全集"));
        assert_eq!(bp.authors, Some(vec!["刘慈欣".to_string()]));
        assert_eq!(
            bp.tags,
            Some(vec!["科幻".to_string(), "雨果奖".to_string()])
        );
    }

    #[test]
    fn batch_edit_supports_title_template_variables_and_lossless_restore() {
        let payload = MediaMetadataPayload::Music(MusicFileMeta {
            title: "Track01".into(),
            artist: "周杰伦".into(),
            album: "范特西".into(),
            year: Some(2001),
            ..Default::default()
        });
        let input = BatchMetadataEditInput {
            media_kind: MediaKind::Music,
            title_template: Some("${counter:2}. ${filename} - ${creator}".into()),
            ..Default::default()
        };
        let patch = build_batch_patch_for_payload(&payload, "爱在西元前", 3, &input)
            .unwrap()
            .expect("should produce music patch");
        let MediaMetadataPatch::Music(mp) = patch else {
            panic!("expected Music patch");
        };
        assert_eq!(mp.title.as_deref(), Some("03. 爱在西元前 - 周杰伦"));

        let restore = payload_to_restore_patch(&payload);
        let MediaMetadataPatch::Music(rp) = restore else {
            panic!("expected Music restore patch");
        };
        assert_eq!(rp.title.as_deref(), Some("Track01"));
        assert_eq!(rp.artist.as_deref(), Some("周杰伦"));
        assert_eq!(rp.year, Some(2001));
    }
}

use xmedia_core::domain::error::DomainResult;

use crate::domain::media::{
    BookMetaPatch, ComicMetaPatch, MediaMetadataPatch, MediaMetadataPayload, MusicMetaPatch,
    VideoMetaPatch,
};
use crate::infra::inspectors::zh_text::ZhConvertDirection;

use super::BatchMetadataEditInput;
use super::replacer::{
    CompiledReplacer, merge_tag_edits, non_empty_trimmed, render_title_template, split_tokens,
    transform_str,
};

/// Pre-compiled batch metadata edit plan (compiles user regex & trims fields once outside the batch loop).
pub struct PreparedBatchMetadataEdit {
    replacer: Option<CompiledReplacer>,
    title_tpl: Option<String>,
    creator_val: Option<String>,
    series_val: Option<String>,
    publisher_val: Option<String>,
    lang_val: Option<String>,
    genre_val: Option<String>,
    year: Option<i32>,
    add_tags: Vec<String>,
    remove_tags: Vec<String>,
    zh: Option<ZhConvertDirection>,
}

impl PreparedBatchMetadataEdit {
    pub fn prepare(input: &BatchMetadataEditInput) -> DomainResult<Self> {
        let replacer = match input.replace_rule.as_ref() {
            Some(rule) => CompiledReplacer::compile(rule)?,
            None => None,
        };
        Ok(Self {
            replacer,
            title_tpl: non_empty_trimmed(&input.title_template),
            creator_val: non_empty_trimmed(&input.creator),
            series_val: non_empty_trimmed(&input.series_or_album),
            publisher_val: non_empty_trimmed(&input.publisher_or_studio),
            lang_val: non_empty_trimmed(&input.language),
            genre_val: non_empty_trimmed(&input.genre),
            year: input.year,
            add_tags: input.add_tags.clone(),
            remove_tags: input.remove_tags.clone(),
            zh: input.zh_direction,
        })
    }

    pub fn build_patch(
        &self,
        payload: &MediaMetadataPayload,
        file_stem: &str,
        counter: usize,
    ) -> Option<MediaMetadataPatch> {
        let replacer = self.replacer.as_ref();
        let title_tpl = &self.title_tpl;
        let creator_val = &self.creator_val;
        let series_val = &self.series_val;
        let publisher_val = &self.publisher_val;
        let lang_val = &self.lang_val;
        let genre_val = &self.genre_val;
        let zh = self.zh;

        match payload {
            MediaMetadataPayload::Book(b) => {
                let mut patch = BookMetaPatch::default();
                let mut changed = false;

                let cur_creator = b.authors.first().map(String::as_str).unwrap_or("");
                let cur_series = b.series.as_deref().unwrap_or("");
                let cur_year = b.published_at.as_deref().unwrap_or("");

                let base_title = if let Some(tpl) = title_tpl {
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
                    replacer,
                    replacer.is_some_and(|r| r.target_title),
                    zh,
                )
                .unwrap_or(base_title);
                if final_title != b.title && !final_title.is_empty() {
                    patch.title = Some(final_title);
                    changed = true;
                }

                if let Some(c) = creator_val {
                    let toks = split_tokens(c);
                    if !toks.is_empty() && toks != b.authors {
                        patch.authors = Some(toks);
                        changed = true;
                    }
                } else if replacer.is_some_and(|r| r.target_creator) || zh.is_some() {
                    let next_authors: Vec<String> = b
                        .authors
                        .iter()
                        .map(|a| {
                            transform_str(
                                a,
                                replacer,
                                replacer.is_some_and(|r| r.target_creator),
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

                if let Some(s) = series_val {
                    if b.series.as_deref() != Some(s.as_str()) {
                        patch.series = Some(s.clone());
                        changed = true;
                    }
                } else if let Some(ref s) = b.series
                    && let Some(ns) =
                        transform_str(s, replacer, replacer.is_some_and(|r| r.target_series), zh)
                {
                    patch.series = Some(ns);
                    changed = true;
                }

                if let Some(p) = publisher_val
                    && b.publisher.as_deref() != Some(p.as_str())
                {
                    patch.publisher = Some(p.clone());
                    changed = true;
                }
                if let Some(y) = self.year
                    && y > 0
                {
                    let ys = y.to_string();
                    if b.published_at.as_deref() != Some(ys.as_str()) {
                        patch.published_at = Some(ys);
                        changed = true;
                    }
                }
                if let Some(l) = lang_val
                    && b.language.as_deref() != Some(l.as_str())
                {
                    patch.language = Some(l.clone());
                    changed = true;
                }

                let mut tags_add = self.add_tags.clone();
                if let Some(g) = genre_val {
                    tags_add.extend(split_tokens(g));
                }
                if let Some(next_tags) = merge_tag_edits(&b.tags, &tags_add, &self.remove_tags) {
                    patch.tags = Some(next_tags);
                    changed = true;
                }

                if let Some(ref desc) = b.description
                    && let Some(nd) = transform_str(
                        desc,
                        replacer,
                        replacer.is_some_and(|r| r.target_summary),
                        zh,
                    )
                {
                    patch.description = Some(nd);
                    changed = true;
                }

                changed.then_some(MediaMetadataPatch::Book(patch))
            }

            MediaMetadataPayload::Comic(c) => {
                let mut patch = ComicMetaPatch::default();
                let mut changed = false;

                let cur_title = c.title.as_deref().unwrap_or("");
                let cur_series = c.series.as_deref().unwrap_or("");
                let cur_creator = c.writers.first().map(String::as_str).unwrap_or("");
                let cur_year = c.year.map(|y| y.to_string()).unwrap_or_default();

                let base_title = if let Some(tpl) = title_tpl {
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
                    replacer,
                    replacer.is_some_and(|r| r.target_title),
                    zh,
                )
                .unwrap_or(base_title);
                if final_title != cur_title && !final_title.is_empty() {
                    patch.title = Some(final_title);
                    changed = true;
                }

                if let Some(s) = series_val {
                    if c.series.as_deref() != Some(s.as_str()) {
                        patch.series = Some(s.clone());
                        changed = true;
                    }
                } else if let Some(ref s) = c.series
                    && let Some(ns) =
                        transform_str(s, replacer, replacer.is_some_and(|r| r.target_series), zh)
                {
                    patch.series = Some(ns);
                    changed = true;
                }

                if let Some(cr) = creator_val {
                    let toks = split_tokens(cr);
                    if !toks.is_empty() && toks != c.writers {
                        patch.writers = Some(toks);
                        changed = true;
                    }
                } else if !c.writers.is_empty() {
                    let mut next_writers = Vec::with_capacity(c.writers.len());
                    let mut writers_changed = false;
                    for w in &c.writers {
                        if let Some(nw) = transform_str(
                            w,
                            replacer,
                            replacer.is_some_and(|r| r.target_creator),
                            zh,
                        ) {
                            next_writers.push(nw);
                            writers_changed = true;
                        } else {
                            next_writers.push(w.clone());
                        }
                    }
                    if writers_changed {
                        patch.writers = Some(next_writers);
                        changed = true;
                    }
                }
                if let Some(p) = publisher_val
                    && c.publisher.as_deref() != Some(p.as_str())
                {
                    patch.publisher = Some(p.clone());
                    changed = true;
                }
                if let Some(y) = self.year
                    && y > 0
                    && c.year != Some(y)
                {
                    patch.year = Some(y);
                    changed = true;
                }
                if let Some(l) = lang_val
                    && c.language_iso.as_deref() != Some(l.as_str())
                {
                    patch.language_iso = Some(l.clone());
                    changed = true;
                }
                if let Some(g) = genre_val {
                    let toks = split_tokens(g);
                    if !toks.is_empty() && toks != c.genres {
                        patch.genres = Some(toks);
                        changed = true;
                    }
                }
                if let Some(next_tags) = merge_tag_edits(&c.tags, &self.add_tags, &self.remove_tags)
                {
                    patch.tags = Some(next_tags);
                    changed = true;
                }
                if let Some(ref sum) = c.summary
                    && let Some(ns) = transform_str(
                        sum,
                        replacer,
                        replacer.is_some_and(|r| r.target_summary),
                        zh,
                    )
                {
                    patch.summary = Some(ns);
                    changed = true;
                }

                changed.then_some(MediaMetadataPatch::Comic(patch))
            }

            MediaMetadataPayload::Music(m) => {
                let mut patch = MusicMetaPatch::default();
                let mut changed = false;

                let cur_year = m.year.map(|y| y.to_string()).unwrap_or_default();
                let base_title = if let Some(tpl) = title_tpl {
                    render_title_template(
                        tpl, file_stem, &m.title, &m.artist, &m.album, &cur_year, counter,
                    )
                } else {
                    m.title.clone()
                };
                let final_title = transform_str(
                    &base_title,
                    replacer,
                    replacer.is_some_and(|r| r.target_title),
                    zh,
                )
                .unwrap_or(base_title);
                if final_title != m.title && !final_title.is_empty() {
                    patch.title = Some(final_title);
                    changed = true;
                }

                if let Some(cr) = creator_val {
                    if m.artist != *cr {
                        patch.artist = Some(cr.clone());
                        changed = true;
                    }
                } else if let Some(nc) = transform_str(
                    &m.artist,
                    replacer,
                    replacer.is_some_and(|r| r.target_creator),
                    zh,
                ) {
                    patch.artist = Some(nc);
                    changed = true;
                }

                if let Some(s) = series_val {
                    if m.album != *s {
                        patch.album = Some(s.clone());
                        changed = true;
                    }
                } else if let Some(na) = transform_str(
                    &m.album,
                    replacer,
                    replacer.is_some_and(|r| r.target_series),
                    zh,
                ) {
                    patch.album = Some(na);
                    changed = true;
                }

                if let Some(p) = publisher_val
                    && m.album_artist.as_deref() != Some(p.as_str())
                {
                    patch.album_artist = Some(p.clone());
                    changed = true;
                }
                if let Some(y) = self.year
                    && y > 0
                    && m.year != Some(y)
                {
                    patch.year = Some(y);
                    changed = true;
                }
                if let Some(g) = genre_val
                    && m.genre.as_deref() != Some(g.as_str())
                {
                    patch.genre = Some(g.clone());
                    changed = true;
                }
                if let Some(ref cmt) = m.comment
                    && let Some(nc) = transform_str(
                        cmt,
                        replacer,
                        replacer.is_some_and(|r| r.target_summary),
                        zh,
                    )
                {
                    patch.comment = Some(nc);
                    changed = true;
                }

                changed.then_some(MediaMetadataPatch::Music(patch))
            }

            MediaMetadataPayload::Video(v) => {
                let mut patch = VideoMetaPatch::default();
                let mut changed = false;

                let cur_series = v.show_title.as_deref().unwrap_or("");
                let cur_creator = v.directors.first().map(String::as_str).unwrap_or("");
                let cur_year = v.year.map(|y| y.to_string()).unwrap_or_default();

                let base_title = if let Some(tpl) = title_tpl {
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
                    replacer,
                    replacer.is_some_and(|r| r.target_title),
                    zh,
                )
                .unwrap_or(base_title);
                if final_title != v.title && !final_title.is_empty() {
                    patch.title = Some(final_title);
                    changed = true;
                }

                if let Some(s) = series_val {
                    if v.show_title.as_deref() != Some(s.as_str()) {
                        patch.show_title = Some(s.clone());
                        changed = true;
                    }
                } else if let Some(ref s) = v.show_title
                    && let Some(ns) =
                        transform_str(s, replacer, replacer.is_some_and(|r| r.target_series), zh)
                {
                    patch.show_title = Some(ns);
                    changed = true;
                }

                if let Some(cr) = creator_val {
                    let toks = split_tokens(cr);
                    if !toks.is_empty() && toks != v.directors {
                        patch.directors = Some(toks);
                        changed = true;
                    }
                } else if !v.directors.is_empty() {
                    let mut next_directors = Vec::with_capacity(v.directors.len());
                    let mut directors_changed = false;
                    for d in &v.directors {
                        if let Some(nd) = transform_str(
                            d,
                            replacer,
                            replacer.is_some_and(|r| r.target_creator),
                            zh,
                        ) {
                            next_directors.push(nd);
                            directors_changed = true;
                        } else {
                            next_directors.push(d.clone());
                        }
                    }
                    if directors_changed {
                        patch.directors = Some(next_directors);
                        changed = true;
                    }
                }
                if let Some(p) = publisher_val
                    && v.studio.as_deref() != Some(p.as_str())
                {
                    patch.studio = Some(p.clone());
                    changed = true;
                }
                if let Some(y) = self.year
                    && y > 0
                    && v.year != Some(y)
                {
                    patch.year = Some(y);
                    changed = true;
                }
                if let Some(g) = genre_val {
                    let toks = split_tokens(g);
                    if !toks.is_empty() && toks != v.genres {
                        patch.genres = Some(toks);
                        changed = true;
                    }
                }
                if let Some(next_tags) = merge_tag_edits(&v.tags, &self.add_tags, &self.remove_tags)
                {
                    patch.tags = Some(next_tags);
                    changed = true;
                }
                if let Some(ref sum) = v.summary
                    && let Some(ns) = transform_str(
                        sum,
                        replacer,
                        replacer.is_some_and(|r| r.target_summary),
                        zh,
                    )
                {
                    patch.summary = Some(ns);
                    changed = true;
                }

                changed.then_some(MediaMetadataPatch::Video(patch))
            }
        }
    }
}

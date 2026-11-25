use std::path::{Path, PathBuf};

use quick_xml::Reader;
use quick_xml::escape::escape;
use quick_xml::events::Event;

use super::artwork::{find_clearlogo_path, find_fanart_path, find_poster_path, find_thumb_path};
use super::filename::{
    dir_has_single_video, guess_episode_and_year, is_season_dir, list_sidecar_subtitles,
};
use crate::domain::media::{VideoFileMeta, VideoMetaPatch};
use crate::infra::inspectors::append_xml_ref;

pub(crate) fn find_existing_nfo(video_path: &Path, is_episode_guess: bool) -> Option<PathBuf> {
    let same_stem = video_path.with_extension("nfo");
    if same_stem.is_file() {
        return Some(same_stem);
    }
    if !is_episode_guess
        && let Some(parent) = video_path.parent()
        && dir_has_single_video(parent)
    {
        let movie_nfo = parent.join("movie.nfo");
        if movie_nfo.is_file() {
            return Some(movie_nfo);
        }
    }
    None
}

pub(crate) fn find_show_nfo_for_episode(video_path: &Path) -> Option<PathBuf> {
    let parent = video_path.parent()?;
    let direct = parent.join("tvshow.nfo");
    if direct.is_file() {
        return Some(direct);
    }
    if is_season_dir(parent)
        && let Some(show_dir) = parent.parent()
    {
        let up = show_dir.join("tvshow.nfo");
        if up.is_file() {
            return Some(up);
        }
    }
    None
}

pub(super) fn parse_nfo_into(xml_bytes: &[u8], meta: &mut VideoFileMeta) {
    let xml_str = String::from_utf8_lossy(xml_bytes);
    let mut reader = Reader::from_str(&xml_str);
    reader.config_mut().trim_text(false);

    let mut stack: Vec<String> = Vec::new();
    let mut current_unique_type: Option<String> = None;
    let mut text_acc = String::new();

    loop {
        match reader.read_event() {
            Ok(Event::Start(ref e)) => {
                let name = e.local_name().as_ref().to_ascii_lowercase();
                if name == "episodedetails" {
                    meta.is_episode = true;
                } else if name == "movie" {
                    meta.is_episode = false;
                } else if name == "uniqueid" {
                    current_unique_type = None;
                    for attr in e.attributes().flatten() {
                        if attr.key.local_name().as_ref() == "type"
                            && let Ok(val) =
                                attr.normalized_value(quick_xml::XmlVersion::Implicit1_0)
                        {
                            current_unique_type = Some(val.to_ascii_lowercase());
                        }
                    }
                }
                stack.push(name);
                text_acc.clear();
            }
            Ok(Event::Text(t)) => {
                if !stack.is_empty() {
                    text_acc.push_str(&t.xml10_content());
                }
            }
            Ok(Event::GeneralRef(r)) => {
                if !stack.is_empty() {
                    append_xml_ref(&mut text_acc, &r);
                }
            }
            Ok(Event::CData(c)) => {
                if !stack.is_empty() {
                    text_acc.push_str(c.as_ref());
                }
            }
            Ok(Event::End(_)) => {
                let trimmed = text_acc.trim();
                let len = stack.len();
                let cur = stack.last().map(String::as_str).unwrap_or("");
                let parent = if len >= 2 {
                    stack[len - 2].as_str()
                } else {
                    ""
                };

                if !trimmed.is_empty() {
                    if parent == "actor" && cur == "name" {
                        if !meta.actors.iter().any(|a| a == trimmed) {
                            meta.actors.push(trimmed.to_string());
                        }
                    } else {
                        match cur {
                            "title"
                                if parent == "movie"
                                    || parent == "episodedetails"
                                    || parent == "tvshow"
                                    || parent == "season" =>
                            {
                                meta.title = trimmed.to_string();
                            }
                            "originaltitle" => meta.original_title = Some(trimmed.to_string()),
                            "showtitle" => meta.show_title = Some(trimmed.to_string()),
                            "season" | "seasonnumber" if !parent.is_empty() => {
                                meta.season_number = trimmed.parse::<u32>().ok();
                            }
                            "episode" => meta.episode_number = trimmed.parse::<u32>().ok(),
                            "year" => meta.year = trimmed.parse::<i32>().ok(),
                            "premiered" | "aired" if meta.year.is_none() => {
                                meta.year = trimmed.get(..4).and_then(|s| s.parse::<i32>().ok());
                            }
                            "rating" if parent == "movie" || parent == "episodedetails" => {
                                meta.rating = trimmed.parse::<f32>().ok();
                            }
                            "plot" | "outline" => {
                                if meta.summary.is_none() || cur == "plot" {
                                    meta.summary = Some(trimmed.to_string());
                                }
                            }
                            "genre" => {
                                if !meta.genres.iter().any(|g| g.eq_ignore_ascii_case(trimmed)) {
                                    meta.genres.push(trimmed.to_string());
                                }
                            }
                            "tag" => {
                                if !meta.tags.iter().any(|tg| tg.eq_ignore_ascii_case(trimmed)) {
                                    meta.tags.push(trimmed.to_string());
                                }
                            }
                            "director" => {
                                if !meta.directors.iter().any(|d| d == trimmed) {
                                    meta.directors.push(trimmed.to_string());
                                }
                            }
                            "studio" => meta.studio = Some(trimmed.to_string()),
                            "name" if parent == "set" => {
                                meta.collection_set = Some(trimmed.to_string());
                            }
                            "set" if meta.collection_set.is_none() => {
                                meta.collection_set = Some(trimmed.to_string());
                            }
                            "lockedfields" => {
                                for f in trimmed.split('|') {
                                    let lf = f.trim().to_ascii_lowercase();
                                    if !lf.is_empty() && !meta.locked_fields.contains(&lf) {
                                        meta.locked_fields.push(lf);
                                    }
                                }
                            }
                            "tmdbid" => meta.tmdb_id = Some(trimmed.to_string()),
                            "uniqueid" => match current_unique_type.as_deref() {
                                Some("tmdb") => meta.tmdb_id = Some(trimmed.to_string()),
                                Some("bangumi") | Some("bgm") => {
                                    meta.bangumi_id = Some(trimmed.to_string())
                                }
                                _ => {}
                            },
                            _ => {}
                        }
                    }
                }

                let popped = stack.pop();
                if popped.as_deref() == Some("uniqueid") {
                    current_unique_type = None;
                }
                text_acc.clear();
            }
            Ok(Event::Eof) | Err(_) => break,
            _ => {}
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum NfoPreserveKind {
    Video,
    TvShow,
    Season,
}

#[derive(Default)]
struct PreservedNfoNodes {
    fileinfo_xml: Option<String>,
    actor_blocks: Vec<(String, String)>,
    other_uniqueids: Vec<(String, bool)>,
    outline_xml: Option<String>,
    extra_elements: Vec<String>,
}

fn is_managed_nfo_tag(kind: NfoPreserveKind, tag_lower: &str) -> bool {
    match kind {
        NfoPreserveKind::Video => matches!(
            tag_lower,
            "title"
                | "originaltitle"
                | "showtitle"
                | "season"
                | "seasonnumber"
                | "episode"
                | "year"
                | "rating"
                | "plot"
                | "outline"
                | "studio"
                | "genre"
                | "tag"
                | "director"
                | "actor"
                | "set"
                | "lockedfields"
                | "tmdbid"
                | "uniqueid"
                | "fileinfo"
        ),
        NfoPreserveKind::TvShow => matches!(
            tag_lower,
            "title"
                | "originaltitle"
                | "showtitle"
                | "year"
                | "rating"
                | "plot"
                | "outline"
                | "studio"
                | "genre"
                | "tag"
                | "actor"
                | "tmdbid"
                | "uniqueid"
        ),
        NfoPreserveKind::Season => matches!(
            tag_lower,
            "season"
                | "seasonnumber"
                | "title"
                | "year"
                | "plot"
                | "outline"
                | "tmdbid"
                | "uniqueid"
        ),
    }
}

fn parse_uniqueid_attrs(e: &quick_xml::events::BytesStart<'_>) -> (Option<String>, bool) {
    let mut uid_type = None;
    let mut is_default = false;
    for attr in e.attributes().flatten() {
        let key = attr.key.local_name();
        let Ok(val) = attr.normalized_value(quick_xml::XmlVersion::Implicit1_0) else {
            continue;
        };
        if key.as_ref().eq_ignore_ascii_case("type") {
            uid_type = Some(val.trim().to_ascii_lowercase());
        } else if key.as_ref().eq_ignore_ascii_case("default") {
            is_default = val.trim().eq_ignore_ascii_case("true");
        }
    }
    (uid_type, is_default)
}

fn strip_default_attr_on_uniqueid(raw_uid: &str) -> String {
    let Some(gt_idx) = raw_uid.find('>') else {
        return raw_uid.to_string();
    };
    let head = raw_uid[..gt_idx]
        .replace(" default=\"true\"", "")
        .replace(" default='true'", "")
        .replace(" default=\"TRUE\"", "");
    format!("{head}{}", &raw_uid[gt_idx..])
}

fn extract_preserved_nfo_nodes(xml_bytes: &[u8], kind: NfoPreserveKind) -> PreservedNfoNodes {
    let xml_str = String::from_utf8_lossy(xml_bytes);
    let mut reader = Reader::from_str(&xml_str);
    reader.config_mut().trim_text(false);

    let mut out = PreservedNfoNodes::default();
    let mut depth = 0usize;
    let mut top_elem: Option<(String, usize, Option<String>, bool)> = None;
    let mut in_actor_name = false;
    let mut actor_name_acc = String::new();

    loop {
        let pos_before = reader.buffer_position() as usize;
        match reader.read_event() {
            Ok(Event::Start(ref e)) => {
                let name = e.local_name().as_ref().to_ascii_lowercase();
                if depth == 1 {
                    let (uid_type, uid_default) = if name == "uniqueid" {
                        parse_uniqueid_attrs(e)
                    } else {
                        (None, false)
                    };
                    top_elem = Some((name, pos_before, uid_type, uid_default));
                    actor_name_acc.clear();
                    in_actor_name = false;
                } else if depth == 2
                    && top_elem.as_ref().is_some_and(|(t, _, _, _)| t == "actor")
                    && name == "name"
                {
                    in_actor_name = true;
                    actor_name_acc.clear();
                }
                depth += 1;
            }
            Ok(Event::Empty(ref e)) => {
                if depth == 1 {
                    let pos_after = reader.buffer_position() as usize;
                    let name = e.local_name().as_ref().to_ascii_lowercase();
                    if let Some(raw_slice) = xml_str.get(pos_before..pos_after)
                        && !is_managed_nfo_tag(kind, &name)
                    {
                        out.extra_elements.push(format!("  {}\n", raw_slice.trim()));
                    }
                }
            }
            Ok(Event::Text(ref t)) if in_actor_name && depth == 3 => {
                actor_name_acc.push_str(&t.xml10_content());
            }
            Ok(Event::GeneralRef(ref r)) if in_actor_name && depth == 3 => {
                append_xml_ref(&mut actor_name_acc, r);
            }
            Ok(Event::CData(ref c)) if in_actor_name && depth == 3 => {
                actor_name_acc.push_str(c.as_ref());
            }
            Ok(Event::End(_)) => {
                if depth == 3 && in_actor_name {
                    in_actor_name = false;
                }
                depth = depth.saturating_sub(1);
                if depth == 1
                    && let Some((name, start_idx, uid_type, uid_default)) = top_elem.take()
                {
                    let pos_after = reader.buffer_position() as usize;
                    if let Some(raw_slice) = xml_str.get(start_idx..pos_after) {
                        let trimmed_raw = raw_slice.trim();
                        let formatted = format!("  {trimmed_raw}\n");
                        match name.as_str() {
                            "fileinfo" if kind == NfoPreserveKind::Video => {
                                out.fileinfo_xml = Some(formatted);
                            }
                            "actor" if kind != NfoPreserveKind::Season => {
                                let clean_actor = actor_name_acc.trim().to_string();
                                if !clean_actor.is_empty() {
                                    out.actor_blocks.push((clean_actor, formatted));
                                }
                            }
                            "uniqueid" => {
                                if !matches!(
                                    uid_type.as_deref(),
                                    Some("tmdb") | Some("bangumi") | Some("bgm")
                                ) {
                                    out.other_uniqueids
                                        .push((trimmed_raw.to_string(), uid_default));
                                }
                            }
                            "outline" => {
                                out.outline_xml = Some(formatted);
                            }
                            other if !is_managed_nfo_tag(kind, other) => {
                                out.extra_elements.push(formatted);
                            }
                            _ => {}
                        }
                    }
                }
            }
            Ok(Event::Eof) | Err(_) => break,
            _ => {}
        }
    }

    out
}

fn clean_id(raw: &str) -> &str {
    raw.trim().split(':').next().unwrap_or("").trim()
}

fn append_unique_ids_xml(
    xml: &mut String,
    tmdb_id: Option<&str>,
    bangumi_id: Option<&str>,
    preserved_uniqueids: &[(String, bool)],
) {
    let has_tmdb = tmdb_id.map(clean_id).is_some_and(|s| !s.is_empty());
    let has_bgm = bangumi_id.map(clean_id).is_some_and(|s| !s.is_empty());
    let has_managed = has_tmdb || has_bgm;

    if let Some(tmdb) = tmdb_id.map(clean_id).filter(|s| !s.is_empty()) {
        xml.push_str(&format!(
            "  <uniqueid type=\"tmdb\" default=\"true\">{}</uniqueid>\n",
            escape(tmdb)
        ));
    }
    if let Some(bgm) = bangumi_id.map(clean_id).filter(|s| !s.is_empty()) {
        let default_attr = if has_tmdb { "" } else { " default=\"true\"" };
        xml.push_str(&format!(
            "  <uniqueid type=\"bangumi\"{default_attr}>{}</uniqueid>\n",
            escape(bgm)
        ));
    }
    for (raw_uid, was_default) in preserved_uniqueids {
        if has_managed && *was_default {
            xml.push_str("  ");
            xml.push_str(&strip_default_attr_on_uniqueid(raw_uid));
            xml.push('\n');
        } else {
            xml.push_str("  ");
            xml.push_str(raw_uid);
            xml.push('\n');
        }
    }
}

fn append_actors_xml(xml: &mut String, actors: &[String], preserved_actors: &[(String, String)]) {
    let mut used = vec![false; preserved_actors.len()];
    for actor in actors {
        let clean = actor.trim();
        if clean.is_empty() {
            continue;
        }
        if let Some((idx, (_, block))) = preserved_actors
            .iter()
            .enumerate()
            .find(|(i, (saved_name, _))| !used[*i] && saved_name == clean)
        {
            used[idx] = true;
            xml.push_str(block);
        } else {
            xml.push_str("  <actor>\n");
            xml.push_str(&format!("    <name>{}</name>\n", escape(clean)));
            xml.push_str("  </actor>\n");
        }
    }
}

pub(crate) fn render_video_nfo_xml_preserving(
    meta: &VideoFileMeta,
    existing_xml: Option<&[u8]>,
) -> String {
    let preserved = existing_xml
        .map(|b| extract_preserved_nfo_nodes(b, NfoPreserveKind::Video))
        .unwrap_or_default();
    let root_tag = if meta.is_episode {
        "episodedetails"
    } else {
        "movie"
    };
    let mut xml = String::with_capacity(1024);
    xml.push_str("<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\n");
    xml.push_str(&format!("<{root_tag}>\n"));
    xml.push_str(&format!("  <title>{}</title>\n", escape(&meta.title)));

    if let Some(ref orig) = meta.original_title
        && !orig.trim().is_empty()
    {
        xml.push_str(&format!(
            "  <originaltitle>{}</originaltitle>\n",
            escape(orig.trim())
        ));
    }
    if meta.is_episode {
        if let Some(ref show) = meta.show_title
            && !show.trim().is_empty()
        {
            xml.push_str(&format!(
                "  <showtitle>{}</showtitle>\n",
                escape(show.trim())
            ));
        }
        if let Some(season) = meta.season_number {
            xml.push_str(&format!("  <season>{season}</season>\n"));
        }
        if let Some(ep) = meta.episode_number {
            xml.push_str(&format!("  <episode>{ep}</episode>\n"));
        }
    }
    if let Some(year) = meta.year {
        xml.push_str(&format!("  <year>{year}</year>\n"));
    }
    if let Some(rating) = meta.rating {
        xml.push_str(&format!("  <rating>{rating:.1}</rating>\n"));
    }
    let has_plot = meta
        .summary
        .as_deref()
        .map(str::trim)
        .is_some_and(|s| !s.is_empty());
    if let Some(ref plot) = meta.summary
        && !plot.trim().is_empty()
    {
        xml.push_str(&format!("  <plot>{}</plot>\n", escape(plot.trim())));
    }
    if has_plot && let Some(ref outline_raw) = preserved.outline_xml {
        xml.push_str(outline_raw);
    }
    if let Some(ref studio) = meta.studio
        && !studio.trim().is_empty()
    {
        xml.push_str(&format!("  <studio>{}</studio>\n", escape(studio.trim())));
    }
    for g in &meta.genres {
        let clean = g.trim();
        if !clean.is_empty() {
            xml.push_str(&format!("  <genre>{}</genre>\n", escape(clean)));
        }
    }
    for tg in &meta.tags {
        let clean = tg.trim();
        if !clean.is_empty() {
            xml.push_str(&format!("  <tag>{}</tag>\n", escape(clean)));
        }
    }
    for dir in &meta.directors {
        let clean = dir.trim();
        if !clean.is_empty() {
            xml.push_str(&format!("  <director>{}</director>\n", escape(clean)));
        }
    }
    append_actors_xml(&mut xml, &meta.actors, &preserved.actor_blocks);
    if !meta.is_episode
        && let Some(ref set_name) = meta.collection_set
        && !set_name.trim().is_empty()
    {
        xml.push_str("  <set>\n");
        xml.push_str(&format!("    <name>{}</name>\n", escape(set_name.trim())));
        xml.push_str("  </set>\n");
    }
    if !meta.locked_fields.is_empty() {
        xml.push_str(&format!(
            "  <lockedfields>{}</lockedfields>\n",
            escape(meta.locked_fields.join("|"))
        ));
    }
    append_unique_ids_xml(
        &mut xml,
        meta.tmdb_id.as_deref(),
        meta.bangumi_id.as_deref(),
        &preserved.other_uniqueids,
    );
    for extra in &preserved.extra_elements {
        xml.push_str(extra);
    }
    if meta.width.is_some()
        || meta.video_codec.is_some()
        || !meta.audio_streams.is_empty()
        || !meta.subtitle_streams.is_empty()
    {
        xml.push_str("  <fileinfo>\n    <streamdetails>\n");
        if meta.width.is_some() || meta.video_codec.is_some() {
            xml.push_str("      <video>\n");
            if let Some(ref codec) = meta.video_codec {
                xml.push_str(&format!("        <codec>{}</codec>\n", escape(codec)));
            }
            if let Some(w) = meta.width {
                xml.push_str(&format!("        <width>{w}</width>\n"));
            }
            if let Some(h) = meta.height {
                xml.push_str(&format!("        <height>{h}</height>\n"));
            }
            if meta.duration_secs > 0 {
                xml.push_str(&format!(
                    "        <durationinseconds>{}</durationinseconds>\n",
                    meta.duration_secs
                ));
            }
            xml.push_str("      </video>\n");
        }
        for a in &meta.audio_streams {
            xml.push_str("      <audio>\n");
            if !a.codec.trim().is_empty() {
                xml.push_str(&format!(
                    "        <codec>{}</codec>\n",
                    escape(a.codec.trim())
                ));
            }
            if let Some(ref lang) = a.language {
                xml.push_str(&format!("        <language>{}</language>\n", escape(lang)));
            }
            if a.channels > 0 {
                xml.push_str(&format!("        <channels>{}</channels>\n", a.channels));
            }
            xml.push_str("      </audio>\n");
        }
        for s in &meta.subtitle_streams {
            if let Some(ref lang) = s.language {
                xml.push_str("      <subtitle>\n");
                xml.push_str(&format!("        <language>{}</language>\n", escape(lang)));
                xml.push_str("      </subtitle>\n");
            }
        }
        xml.push_str("    </streamdetails>\n  </fileinfo>\n");
    } else if let Some(ref preserved_fileinfo) = preserved.fileinfo_xml {
        xml.push_str(preserved_fileinfo);
    }
    xml.push_str(&format!("</{root_tag}>\n"));
    xml
}

/// Renders Jellyfin series-level `tvshow.nfo` (`<tvshow>`).
pub(crate) fn render_tvshow_nfo_xml_preserving(
    meta: &VideoFileMeta,
    show_plot: Option<&str>,
    existing_xml: Option<&[u8]>,
) -> String {
    let preserved = existing_xml
        .map(|b| extract_preserved_nfo_nodes(b, NfoPreserveKind::TvShow))
        .unwrap_or_default();
    let show_title = meta
        .show_title
        .as_deref()
        .filter(|s| !s.trim().is_empty())
        .unwrap_or(&meta.title);
    let mut xml = String::with_capacity(1024);
    xml.push_str("<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\n");
    xml.push_str("<tvshow>\n");
    xml.push_str(&format!("  <title>{}</title>\n", escape(show_title.trim())));
    if let Some(ref orig) = meta.original_title
        && !orig.trim().is_empty()
    {
        xml.push_str(&format!(
            "  <originaltitle>{}</originaltitle>\n",
            escape(orig.trim())
        ));
    }
    if let Some(year) = meta.year {
        xml.push_str(&format!("  <year>{year}</year>\n"));
    }
    if let Some(rating) = meta.rating {
        xml.push_str(&format!("  <rating>{rating:.1}</rating>\n"));
    }
    let effective_plot = show_plot
        .or(meta.summary.as_deref())
        .map(str::trim)
        .filter(|s| !s.is_empty());
    if let Some(plot) = effective_plot {
        xml.push_str(&format!("  <plot>{}</plot>\n", escape(plot)));
        if let Some(ref outline_raw) = preserved.outline_xml {
            xml.push_str(outline_raw);
        }
    }
    if let Some(ref studio) = meta.studio
        && !studio.trim().is_empty()
    {
        xml.push_str(&format!("  <studio>{}</studio>\n", escape(studio.trim())));
    }
    for g in &meta.genres {
        let clean = g.trim();
        if !clean.is_empty() {
            xml.push_str(&format!("  <genre>{}</genre>\n", escape(clean)));
        }
    }
    for tg in &meta.tags {
        let clean = tg.trim();
        if !clean.is_empty() {
            xml.push_str(&format!("  <tag>{}</tag>\n", escape(clean)));
        }
    }
    append_actors_xml(&mut xml, &meta.actors, &preserved.actor_blocks);
    append_unique_ids_xml(
        &mut xml,
        meta.tmdb_id.as_deref(),
        meta.bangumi_id.as_deref(),
        &preserved.other_uniqueids,
    );
    for extra in &preserved.extra_elements {
        xml.push_str(extra);
    }
    xml.push_str("</tvshow>\n");
    xml
}

/// Renders Jellyfin season-level `season.nfo` (`<season>`), emitting both `<season>` and
/// `<seasonnumber>` for compatibility with both `xmedia-video` and Jellyfin.
pub(crate) fn render_season_nfo_xml_preserving(
    season_number: u32,
    season_title: Option<&str>,
    year: Option<i32>,
    season_plot: Option<&str>,
    tmdb_id: Option<&str>,
    bangumi_id: Option<&str>,
    existing_xml: Option<&[u8]>,
) -> String {
    let preserved = existing_xml
        .map(|b| extract_preserved_nfo_nodes(b, NfoPreserveKind::Season))
        .unwrap_or_default();
    let default_title = format!("Season {season_number:02}");
    let title = season_title
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .unwrap_or(&default_title);
    let mut xml = String::with_capacity(512);
    xml.push_str("<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\n");
    xml.push_str("<season>\n");
    xml.push_str(&format!("  <season>{season_number}</season>\n"));
    xml.push_str(&format!("  <seasonnumber>{season_number}</seasonnumber>\n"));
    xml.push_str(&format!("  <title>{}</title>\n", escape(title)));
    if let Some(y) = year {
        xml.push_str(&format!("  <year>{y}</year>\n"));
    }
    if let Some(plot) = season_plot.map(str::trim).filter(|s| !s.is_empty()) {
        xml.push_str(&format!("  <plot>{}</plot>\n", escape(plot)));
        if let Some(ref outline_raw) = preserved.outline_xml {
            xml.push_str(outline_raw);
        }
    }
    append_unique_ids_xml(&mut xml, tmdb_id, bangumi_id, &preserved.other_uniqueids);
    for extra in &preserved.extra_elements {
        xml.push_str(extra);
    }
    xml.push_str("</season>\n");
    xml
}

pub(crate) fn read_base_video_meta(path: &Path) -> VideoFileMeta {
    let ext = path
        .extension()
        .and_then(|s| s.to_str())
        .unwrap_or("mp4")
        .to_ascii_lowercase();

    let (is_ep, title_guess, show_guess, season_guess, ep_guess, year_guess) =
        guess_episode_and_year(path);
    let default_nfo = path.with_extension("nfo");
    let existing_nfo = find_existing_nfo(path, is_ep);

    let mut meta = VideoFileMeta {
        container: ext,
        is_episode: is_ep,
        title: title_guess,
        show_title: show_guess,
        season_number: season_guess,
        episode_number: ep_guess,
        year: year_guess,
        has_nfo: existing_nfo.is_some(),
        nfo_filename: existing_nfo
            .as_ref()
            .unwrap_or(&default_nfo)
            .file_name()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_default(),
        has_poster: find_poster_path(path).is_some(),
        has_fanart: find_fanart_path(path).is_some(),
        has_thumb: find_thumb_path(path).is_some(),
        has_clearlogo: find_clearlogo_path(path).is_some(),
        sidecar_subtitles: list_sidecar_subtitles(path),
        ..Default::default()
    };

    if let Some(ref nfo_path) = existing_nfo
        && let Ok(bytes) = std::fs::read(nfo_path)
    {
        parse_nfo_into(&bytes, &mut meta);
    }

    if meta.is_episode
        && let Some(show_nfo_path) = find_show_nfo_for_episode(path)
        && let Ok(bytes) = std::fs::read(show_nfo_path)
    {
        let mut show_meta = VideoFileMeta::default();
        parse_nfo_into(&bytes, &mut show_meta);
        if meta.show_title.is_none() && !show_meta.title.trim().is_empty() {
            meta.show_title = Some(show_meta.title);
        }
        if meta.year.is_none() {
            meta.year = show_meta.year;
        }
        if meta.genres.is_empty() {
            meta.genres = show_meta.genres;
        }
        if meta.tags.is_empty() {
            meta.tags = show_meta.tags;
        }
        if meta.actors.is_empty() {
            meta.actors = show_meta.actors;
        }
        if meta.studio.is_none() {
            meta.studio = show_meta.studio;
        }
        if meta.tmdb_id.is_none() {
            meta.tmdb_id = show_meta.tmdb_id;
        }
        if meta.bangumi_id.is_none() {
            meta.bangumi_id = show_meta.bangumi_id;
        }
    }

    meta
}

pub(crate) fn apply_video_patch_onto(meta: &mut VideoFileMeta, vp: VideoMetaPatch) {
    if let Some(is_ep) = vp.is_episode {
        meta.is_episode = is_ep;
    }
    if let Some(title) = vp.title
        && !title.trim().is_empty()
    {
        meta.title = title.trim().to_string();
    }
    if let Some(orig) = vp.original_title {
        meta.original_title = (!orig.trim().is_empty()).then(|| orig.trim().to_string());
    }
    if let Some(show) = vp.show_title {
        meta.show_title = (!show.trim().is_empty()).then(|| show.trim().to_string());
    }
    if let Some(s) = vp.season_number {
        meta.season_number = if s < 0 { None } else { Some(s as u32) };
    }
    if vp.episode_number.is_some() {
        meta.episode_number = vp.episode_number.filter(|&n| n > 0);
    }
    if vp.year.is_some() {
        meta.year = vp.year.filter(|&y| y > 0);
    }
    if vp.rating.is_some() {
        meta.rating = vp.rating.filter(|&r| r > 0.0);
    }
    if let Some(summary) = vp.summary {
        meta.summary = (!summary.trim().is_empty()).then(|| summary.trim().to_string());
    }
    if let Some(genres) = vp.genres {
        meta.genres = genres
            .into_iter()
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .collect();
    }
    if let Some(tags) = vp.tags {
        meta.tags = tags
            .into_iter()
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .collect();
    }
    if let Some(actors) = vp.actors {
        meta.actors = actors
            .into_iter()
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .collect();
    }
    if let Some(directors) = vp.directors {
        meta.directors = directors
            .into_iter()
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .collect();
    }
    if let Some(studio) = vp.studio {
        meta.studio = (!studio.trim().is_empty()).then(|| studio.trim().to_string());
    }
    if let Some(tmdb) = vp.tmdb_id {
        meta.tmdb_id = (!tmdb.trim().is_empty()).then(|| tmdb.trim().to_string());
    }
    if let Some(bgm) = vp.bangumi_id {
        meta.bangumi_id = (!bgm.trim().is_empty()).then(|| bgm.trim().to_string());
    }
    if let Some(set_name) = vp.collection_set {
        meta.collection_set = (!set_name.trim().is_empty()).then(|| set_name.trim().to_string());
    }
    if let Some(locks) = vp.locked_fields {
        let mut clean = Vec::new();
        for f in locks {
            let low = f.trim().to_ascii_lowercase();
            if !low.is_empty() && !clean.contains(&low) {
                clean.push(low);
            }
        }
        meta.locked_fields = clean;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn video_nfo_preserves_streamdetails_uniqueids_actor_roles_and_extended_tags() {
        let original_xml = br#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<movie>
  <title>The Matrix</title>
  <year>1999</year>
  <premiered>1999-03-31</premiered>
  <mpaa>Rated R</mpaa>
  <country>United States</country>
  <uniqueid type="imdb" default="true">tt0133093</uniqueid>
  <uniqueid type="tvdb">221</uniqueid>
  <uniqueid type="tmdb">603</uniqueid>
  <actor>
    <name>Keanu Reeves</name>
    <role>Neo</role>
    <thumb>https://img.example/neo.jpg</thumb>
  </actor>
  <actor>
    <name>Old Actor</name>
    <role>Extra</role>
  </actor>
  <fileinfo>
    <streamdetails>
      <video>
        <codec>hevc</codec>
        <width>3840</width>
        <height>2160</height>
      </video>
    </streamdetails>
  </fileinfo>
</movie>"#;

        let mut meta = VideoFileMeta::default();
        parse_nfo_into(original_xml, &mut meta);
        meta.title = "黑客帝国".to_string();
        meta.actors = vec!["Keanu Reeves".to_string(), "Carrie-Anne Moss".to_string()];

        let updated = render_video_nfo_xml_preserving(&meta, Some(original_xml));
        assert!(updated.contains("<title>黑客帝国</title>"));
        assert!(updated.contains("<premiered>1999-03-31</premiered>"));
        assert!(updated.contains("<mpaa>Rated R</mpaa>"));
        assert!(updated.contains("<country>United States</country>"));
        assert!(updated.contains("<uniqueid type=\"tmdb\" default=\"true\">603</uniqueid>"));
        assert!(updated.contains("<uniqueid type=\"imdb\">tt0133093</uniqueid>"));
        assert!(updated.contains("<uniqueid type=\"tvdb\">221</uniqueid>"));
        assert!(updated.contains("<role>Neo</role>"));
        assert!(updated.contains("<thumb>https://img.example/neo.jpg</thumb>"));
        assert!(!updated.contains("Old Actor"));
        assert!(updated.contains("<name>Carrie-Anne Moss</name>"));
        assert!(updated.contains("<codec>hevc</codec>"));
        assert!(updated.contains("<width>3840</width>"));
    }

    #[test]
    fn season_nfo_renders_both_season_and_seasonnumber_and_preserves_extra_nodes() {
        let prev = br#"<season>
  <seasonnumber>2</seasonnumber>
  <premiered>2024-01-05</premiered>
  <uniqueid type="tvdb">998877</uniqueid>
</season>"#;
        let rendered = render_season_nfo_xml_preserving(
            2,
            Some("第 2 季"),
            Some(2024),
            Some("第二季剧情"),
            Some("209867"),
            None,
            Some(prev),
        );
        assert!(rendered.contains("<season>2</season>"));
        assert!(rendered.contains("<seasonnumber>2</seasonnumber>"));
        assert!(rendered.contains("<premiered>2024-01-05</premiered>"));
        assert!(rendered.contains("<uniqueid type=\"tvdb\">998877</uniqueid>"));

        let mut parsed = VideoFileMeta::default();
        parse_nfo_into(rendered.as_bytes(), &mut parsed);
        assert_eq!(parsed.season_number, Some(2));
        assert_eq!(parsed.title, "第 2 季");
    }
}

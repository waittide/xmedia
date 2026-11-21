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

fn find_existing_nfo(video_path: &Path, is_episode_guess: bool) -> Option<PathBuf> {
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

pub(super) fn find_show_nfo_for_episode(video_path: &Path) -> Option<PathBuf> {
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

fn clean_id(raw: &str) -> &str {
    raw.trim().split(':').next().unwrap_or("").trim()
}

fn append_unique_ids_xml(xml: &mut String, tmdb_id: Option<&str>, bangumi_id: Option<&str>) {
    if let Some(tmdb) = tmdb_id.map(clean_id).filter(|s| !s.is_empty()) {
        xml.push_str(&format!(
            "  <uniqueid type=\"tmdb\" default=\"true\">{}</uniqueid>\n",
            escape(tmdb)
        ));
    }
    if let Some(bgm) = bangumi_id.map(clean_id).filter(|s| !s.is_empty()) {
        let default_attr = if tmdb_id.map(clean_id).is_some_and(|s| !s.is_empty()) {
            ""
        } else {
            " default=\"true\""
        };
        xml.push_str(&format!(
            "  <uniqueid type=\"bangumi\"{default_attr}>{}</uniqueid>\n",
            escape(bgm)
        ));
    }
}

pub(crate) fn render_video_nfo_xml(meta: &VideoFileMeta) -> String {
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
    if let Some(ref plot) = meta.summary
        && !plot.trim().is_empty()
    {
        xml.push_str(&format!("  <plot>{}</plot>\n", escape(plot.trim())));
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
    for actor in &meta.actors {
        let clean = actor.trim();
        if !clean.is_empty() {
            xml.push_str("  <actor>\n");
            xml.push_str(&format!("    <name>{}</name>\n", escape(clean)));
            xml.push_str("  </actor>\n");
        }
    }
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
    );
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
    }
    xml.push_str(&format!("</{root_tag}>\n"));
    xml
}

/// Renders Jellyfin series-level `tvshow.nfo` (`<tvshow>`).
pub(crate) fn render_tvshow_nfo_xml(meta: &VideoFileMeta, show_plot: Option<&str>) -> String {
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
    if let Some(plot) = show_plot.or(meta.summary.as_deref())
        && !plot.trim().is_empty()
    {
        xml.push_str(&format!("  <plot>{}</plot>\n", escape(plot.trim())));
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
    for actor in &meta.actors {
        let clean = actor.trim();
        if !clean.is_empty() {
            xml.push_str("  <actor>\n");
            xml.push_str(&format!("    <name>{}</name>\n", escape(clean)));
            xml.push_str("  </actor>\n");
        }
    }
    append_unique_ids_xml(
        &mut xml,
        meta.tmdb_id.as_deref(),
        meta.bangumi_id.as_deref(),
    );
    xml.push_str("</tvshow>\n");
    xml
}

/// Renders Jellyfin season-level `season.nfo` (`<season>`).
pub(crate) fn render_season_nfo_xml(
    season_number: u32,
    season_title: Option<&str>,
    year: Option<i32>,
    season_plot: Option<&str>,
    tmdb_id: Option<&str>,
    bangumi_id: Option<&str>,
) -> String {
    let default_title = format!("Season {season_number:02}");
    let title = season_title
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .unwrap_or(&default_title);
    let mut xml = String::with_capacity(512);
    xml.push_str("<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\n");
    xml.push_str("<season>\n");
    xml.push_str(&format!("  <seasonnumber>{season_number}</seasonnumber>\n"));
    xml.push_str(&format!("  <title>{}</title>\n", escape(title)));
    if let Some(y) = year {
        xml.push_str(&format!("  <year>{y}</year>\n"));
    }
    if let Some(plot) = season_plot.map(str::trim).filter(|s| !s.is_empty()) {
        xml.push_str(&format!("  <plot>{}</plot>\n", escape(plot)));
    }
    append_unique_ids_xml(&mut xml, tmdb_id, bangumi_id);
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

use std::path::{Path, PathBuf};

use async_trait::async_trait;
use quick_xml::Reader;
use quick_xml::escape::escape;
use quick_xml::events::Event;
use regex::Regex;
use xmedia_core::application::ports::BinaryPayload;
use xmedia_core::domain::error::{DomainError, DomainResult};
use xmedia_core::domain::library::MediaKind;
use xmedia_core::infra::ffmpeg::FfmpegToolkit;
use xmedia_core::infra::thumbnail::sniff_image_mime;

use crate::domain::media::{
    CompletenessFlags, MediaMetadataPatch, MediaMetadataPayload, VideoFileMeta, VideoMetaPatch,
};
use crate::domain::ports::{MediaInspectorPort, QuickFileSummary};
use crate::infra::atomic_fs::atomic_write_bytes;

const VIDEO_EXTENSIONS: &[&str] = &[
    "mp4", "mkv", "webm", "mov", "avi", "ts", "m2ts", "mts", "m4v", "flv",
];

pub struct VideoNfoInspector {
    ffmpeg: FfmpegToolkit,
}

impl VideoNfoInspector {
    pub fn new(ffmpeg: FfmpegToolkit) -> Self {
        Self { ffmpeg }
    }
}

pub(crate) fn is_season_dir(dir: &Path) -> bool {
    let Some(name) = dir.file_name().and_then(|s| s.to_str()) else {
        return false;
    };
    let trimmed = name.trim();
    let lower = trimmed.to_ascii_lowercase();
    if lower == "specials" {
        return true;
    }
    if let Some(rest) = lower.strip_prefix("season") {
        return rest.trim().chars().all(|c| c.is_ascii_digit()) && !rest.trim().is_empty();
    }
    if let Some(rest) = lower.strip_prefix('s')
        && !rest.is_empty()
        && rest.len() <= 3
        && rest.chars().all(|c| c.is_ascii_digit())
    {
        return true;
    }
    if let Some(rest) = trimmed.strip_prefix('第')
        && let Some(num_str) = rest.strip_suffix('季')
    {
        return !num_str.trim().is_empty() && num_str.trim().chars().all(|c| c.is_ascii_digit());
    }
    false
}

fn find_existing_nfo(video_path: &Path) -> Option<PathBuf> {
    let same_stem = video_path.with_extension("nfo");
    if same_stem.is_file() {
        return Some(same_stem);
    }
    if let Some(parent) = video_path.parent() {
        let movie_nfo = parent.join("movie.nfo");
        if movie_nfo.is_file() {
            return Some(movie_nfo);
        }
    }
    None
}

fn find_show_nfo_for_episode(video_path: &Path) -> Option<PathBuf> {
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

fn find_poster_path(video_path: &Path) -> Option<PathBuf> {
    let parent = video_path.parent()?;
    let stem = video_path.file_stem()?.to_str()?;
    let mut candidates = vec![
        parent.join(format!("{stem}-poster.jpg")),
        parent.join(format!("{stem}-poster.png")),
        parent.join("poster.jpg"),
        parent.join("poster.png"),
        parent.join(format!("{stem}-thumb.jpg")),
        parent.join(format!("{stem}-thumb.png")),
        parent.join("folder.jpg"),
    ];
    if is_season_dir(parent)
        && let Some(show_dir) = parent.parent()
    {
        candidates.push(show_dir.join("poster.jpg"));
        candidates.push(show_dir.join("poster.png"));
        candidates.push(show_dir.join("folder.jpg"));
    }
    candidates.into_iter().find(|p| p.is_file())
}

fn find_fanart_path(video_path: &Path) -> Option<PathBuf> {
    let parent = video_path.parent()?;
    let stem = video_path.file_stem()?.to_str()?;
    let mut candidates = vec![
        parent.join(format!("{stem}-fanart.jpg")),
        parent.join(format!("{stem}-fanart.png")),
        parent.join("fanart.jpg"),
        parent.join("fanart.png"),
        parent.join("backdrop.jpg"),
        parent.join("backdrop.png"),
    ];
    if is_season_dir(parent)
        && let Some(show_dir) = parent.parent()
    {
        candidates.push(show_dir.join("fanart.jpg"));
        candidates.push(show_dir.join("fanart.png"));
        candidates.push(show_dir.join("backdrop.jpg"));
        candidates.push(show_dir.join("backdrop.png"));
    }
    candidates.into_iter().find(|p| p.is_file())
}

fn list_sidecar_subtitles(video_path: &Path) -> Vec<String> {
    let Some(parent) = video_path.parent() else {
        return Vec::new();
    };
    let Some(stem) = video_path.file_stem().and_then(|s| s.to_str()) else {
        return Vec::new();
    };
    let Ok(entries) = std::fs::read_dir(parent) else {
        return Vec::new();
    };
    let mut subs = Vec::new();
    for entry in entries.flatten() {
        let p = entry.path();
        if !p.is_file() {
            continue;
        }
        let Some(fname) = p.file_name().and_then(|s| s.to_str()) else {
            continue;
        };
        let lower = fname.to_ascii_lowercase();
        let is_sub = lower.ends_with(".srt")
            || lower.ends_with(".ass")
            || lower.ends_with(".ssa")
            || lower.ends_with(".vtt");
        if is_sub && fname.starts_with(stem) {
            subs.push(fname.to_string());
        }
    }
    subs.sort();
    subs
}

fn guess_episode_and_year(
    video_path: &Path,
) -> (bool, Option<String>, Option<u32>, Option<u32>, Option<i32>) {
    let stem = video_path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or_default();
    let se_re = Regex::new(r"(?i)S(\d{1,2})E(\d{1,3})").expect("valid regex");
    let year_re = Regex::new(r"\b(19\d{2}|20\d{2})\b").expect("valid regex");

    let year = year_re
        .captures(stem)
        .and_then(|c| c.get(1))
        .and_then(|m| m.as_str().parse::<i32>().ok());

    if let Some(caps) = se_re.captures(stem) {
        let season = caps.get(1).and_then(|m| m.as_str().parse::<u32>().ok());
        let ep = caps.get(2).and_then(|m| m.as_str().parse::<u32>().ok());
        let prefix = stem[..caps.get(0).unwrap().start()]
            .trim_matches([' ', '-', '_', '.'])
            .replace('.', " ");
        let show_guess = if !prefix.is_empty() {
            Some(prefix)
        } else {
            video_path.parent().and_then(|p| {
                let dir_name = p.file_name()?.to_string_lossy();
                if dir_name.to_ascii_lowercase().starts_with("season") {
                    p.parent()?
                        .file_name()
                        .map(|g| g.to_string_lossy().into_owned())
                } else {
                    Some(dir_name.into_owned())
                }
            })
        };
        return (true, show_guess, season, ep, year);
    }

    (false, None, None, None, year)
}

fn parse_nfo_into(xml_bytes: &[u8], meta: &mut VideoFileMeta) {
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
                    super::append_xml_ref(&mut text_acc, &r);
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
                                    || parent == "tvshow" =>
                            {
                                meta.title = trimmed.to_string();
                            }
                            "originaltitle" => meta.original_title = Some(trimmed.to_string()),
                            "showtitle" => meta.show_title = Some(trimmed.to_string()),
                            "season" => meta.season_number = trimmed.parse::<u32>().ok(),
                            "episode" => meta.episode_number = trimmed.parse::<u32>().ok(),
                            "year" => meta.year = trimmed.parse::<i32>().ok(),
                            "premiered" | "aired" if meta.year.is_none() => {
                                if trimmed.len() >= 4 {
                                    meta.year = trimmed[..4].parse::<i32>().ok();
                                }
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
    append_unique_ids_xml(
        &mut xml,
        meta.tmdb_id.as_deref(),
        meta.bangumi_id.as_deref(),
    );
    xml.push_str(&format!("</{root_tag}>\n"));
    xml
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
    let fallback_stem = path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("Untitled");
    let ext = path
        .extension()
        .and_then(|s| s.to_str())
        .unwrap_or("mp4")
        .to_ascii_lowercase();

    let (is_ep, show_guess, season_guess, ep_guess, year_guess) = guess_episode_and_year(path);
    let default_nfo = path.with_extension("nfo");
    let existing_nfo = find_existing_nfo(path);

    let mut meta = VideoFileMeta {
        container: ext,
        is_episode: is_ep,
        title: fallback_stem.to_string(),
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
    if vp.season_number.is_some() {
        meta.season_number = vp.season_number.filter(|&n| n > 0);
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
}

/// Atomically writes `{stem_base}.{ext}` inside `dir` and removes any stale sibling with the
/// opposite `.jpg`/`.png` extension so old artwork never shadows the updated file.
pub(crate) fn write_image_replacing_opposite_ext(
    dir: &Path,
    stem_base: &str,
    ext: &str,
    bytes: &[u8],
) -> DomainResult<()> {
    let target = dir.join(format!("{stem_base}.{ext}"));
    atomic_write_bytes(&target, bytes)?;
    let stale_ext = if ext == "png" { "jpg" } else { "png" };
    let stale_path = dir.join(format!("{stem_base}.{stale_ext}"));
    if stale_path.is_file() {
        let _ = std::fs::remove_file(stale_path);
    }
    Ok(())
}

#[async_trait]
impl MediaInspectorPort for VideoNfoInspector {
    fn media_kind(&self) -> MediaKind {
        MediaKind::Video
    }

    fn supports_extension(&self, ext_lower: &str) -> bool {
        VIDEO_EXTENSIONS.contains(&ext_lower)
    }

    async fn summarize_file(&self, path: &Path, fallback_stem: &str) -> QuickFileSummary {
        let p = path.to_path_buf();
        let stem = fallback_stem.to_string();
        tokio::task::spawn_blocking(move || {
            let meta = read_base_video_meta(&p);
            let sub = if meta.is_episode {
                match (&meta.show_title, meta.season_number, meta.episode_number) {
                    (Some(sh), Some(s), Some(e)) => format!("{sh} · S{s:02}E{e:02}"),
                    (Some(sh), _, _) => sh.clone(),
                    _ => String::new(),
                }
            } else {
                meta.genres.join(" / ")
            };
            QuickFileSummary {
                display_title: if meta.title.is_empty() {
                    stem
                } else {
                    meta.title.clone()
                },
                display_subtitle: sub,
                year: meta.year,
                completeness: CompletenessFlags {
                    has_title: !meta.title.trim().is_empty(),
                    has_creator: !meta.directors.is_empty()
                        || !meta.actors.is_empty()
                        || meta.show_title.is_some(),
                    has_cover: meta.has_poster,
                    has_summary_or_lyrics: meta
                        .summary
                        .as_ref()
                        .is_some_and(|s| !s.trim().is_empty()),
                    has_nfo_or_tag_block: meta.has_nfo,
                },
            }
        })
        .await
        .unwrap_or_else(|_| QuickFileSummary {
            display_title: fallback_stem.to_string(),
            display_subtitle: String::new(),
            year: None,
            completeness: CompletenessFlags::default(),
        })
    }

    async fn inspect_file(&self, path: &Path) -> DomainResult<MediaMetadataPayload> {
        let p = path.to_path_buf();
        let mut meta = tokio::task::spawn_blocking(move || read_base_video_meta(&p))
            .await
            .map_err(|e| DomainError::Storage(format!("video inspect task failed: {e}")))?;

        if let Ok(probed) = self.ffmpeg.probe_media(path).await {
            meta.duration_secs = probed.duration_secs;
            meta.bit_rate_kbps = probed.bit_rate_kbps;
            meta.width = probed.width;
            meta.height = probed.height;
            meta.video_codec = probed.video_codec;
            meta.audio_streams = probed.audio_streams;
            meta.subtitle_streams = probed.subtitle_streams;
        }

        Ok(MediaMetadataPayload::Video(meta))
    }

    async fn write_metadata(
        &self,
        path: &Path,
        patch: &MediaMetadataPatch,
    ) -> DomainResult<MediaMetadataPayload> {
        let MediaMetadataPatch::Video(vp) = patch else {
            return Err(DomainError::Validation(
                "expected video metadata patch for video file".into(),
            ));
        };
        let p = path.to_path_buf();
        let vp_cloned: VideoMetaPatch = vp.clone();

        tokio::task::spawn_blocking(move || -> DomainResult<()> {
            let mut meta = read_base_video_meta(&p);
            apply_video_patch_onto(&mut meta, vp_cloned);

            let nfo_target = if !meta.is_episode
                && let Some(parent) = p.parent()
                && parent.join("movie.nfo").is_file()
                && !p.with_extension("nfo").is_file()
            {
                parent.join("movie.nfo")
            } else {
                p.with_extension("nfo")
            };
            let xml = render_video_nfo_xml(&meta);
            atomic_write_bytes(&nfo_target, xml.as_bytes())
        })
        .await
        .map_err(|e| DomainError::Storage(format!("video nfo write failed: {e}")))??;

        self.inspect_file(path).await
    }

    async fn read_artwork(&self, path: &Path) -> DomainResult<Option<BinaryPayload>> {
        let p = path.to_path_buf();
        tokio::task::spawn_blocking(move || {
            let Some(img_path) = find_poster_path(&p).or_else(|| find_fanart_path(&p)) else {
                return Ok(None);
            };
            let raw = std::fs::read(&img_path)
                .map_err(|e| DomainError::Storage(format!("failed reading poster: {e}")))?;
            let mime = sniff_image_mime(&raw).to_string();
            Ok(Some(BinaryPayload {
                bytes: raw,
                mime_type: mime,
            }))
        })
        .await
        .map_err(|e| DomainError::Storage(format!("video poster read failed: {e}")))?
    }

    async fn write_artwork(
        &self,
        path: &Path,
        bytes: &[u8],
        mime_type: &str,
        slot: &str,
    ) -> DomainResult<()> {
        let parent = path
            .parent()
            .ok_or_else(|| DomainError::Validation("video file has no parent folder".into()))?
            .to_path_buf();
        let stem = path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("video")
            .to_string();
        let detected = if mime_type.trim().is_empty() {
            sniff_image_mime(bytes)
        } else {
            mime_type.trim()
        };
        let ext = if detected.contains("png") {
            "png"
        } else {
            "jpg"
        };
        let is_jellyfin_movie_dir = parent.join("movie.nfo").is_file();
        let stem_base = if slot.eq_ignore_ascii_case("thumb") {
            format!("{stem}-thumb")
        } else if slot.eq_ignore_ascii_case("fanart") || slot.eq_ignore_ascii_case("backdrop") {
            if is_jellyfin_movie_dir {
                "fanart".to_string()
            } else {
                format!("{stem}-fanart")
            }
        } else if is_jellyfin_movie_dir {
            "poster".to_string()
        } else {
            format!("{stem}-poster")
        };
        let raw = bytes.to_vec();

        tokio::task::spawn_blocking(move || {
            write_image_replacing_opposite_ext(&parent, &stem_base, ext, &raw)
        })
        .await
        .map_err(|e| DomainError::Storage(format!("video artwork write failed: {e}")))?
    }
}

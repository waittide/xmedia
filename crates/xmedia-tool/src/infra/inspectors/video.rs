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

pub(crate) fn parse_season_dir_number(dir: &Path) -> Option<u32> {
    let name = dir.file_name().and_then(|s| s.to_str())?;
    let trimmed = name.trim();
    let lower = trimmed.to_ascii_lowercase();
    if lower == "specials" {
        return Some(0);
    }
    if let Some(rest) = lower.strip_prefix("season") {
        let digits = rest.trim();
        if !digits.is_empty() && digits.chars().all(|c| c.is_ascii_digit()) {
            return digits.parse::<u32>().ok();
        }
    }
    if let Some(rest) = lower.strip_prefix('s')
        && !rest.is_empty()
        && rest.len() <= 3
        && rest.chars().all(|c| c.is_ascii_digit())
    {
        return rest.parse::<u32>().ok();
    }
    if let Some(rest) = trimmed.strip_prefix('第')
        && let Some(num_str) = rest.strip_suffix('季')
    {
        let digits = num_str.trim();
        if !digits.is_empty() && digits.chars().all(|c| c.is_ascii_digit()) {
            return digits.parse::<u32>().ok();
        }
    }
    None
}

pub(crate) fn is_season_dir(dir: &Path) -> bool {
    parse_season_dir_number(dir).is_some()
}

pub(crate) fn dir_has_single_video(dir: &Path) -> bool {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return false;
    };
    let mut count = 0usize;
    for entry in entries.flatten() {
        let p = entry.path();
        if p.is_file()
            && p.extension()
                .and_then(|s| s.to_str())
                .is_some_and(|e| VIDEO_EXTENSIONS.contains(&e.to_ascii_lowercase().as_str()))
        {
            count += 1;
            if count > 1 {
                return false;
            }
        }
    }
    count == 1
}

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

fn allow_dir_level_artwork(parent: &Path) -> bool {
    is_season_dir(parent) || parent.join("tvshow.nfo").is_file() || dir_has_single_video(parent)
}

fn find_thumb_path(video_path: &Path) -> Option<PathBuf> {
    let parent = video_path.parent()?;
    let stem = video_path.file_stem()?.to_str()?;
    ["jpg", "jpeg", "png", "webp"]
        .into_iter()
        .map(|ext| parent.join(format!("{stem}-thumb.{ext}")))
        .find(|p| p.is_file())
}

fn find_poster_path(video_path: &Path) -> Option<PathBuf> {
    let parent = video_path.parent()?;
    let stem = video_path.file_stem()?.to_str()?;
    let mut candidates = vec![
        parent.join(format!("{stem}-poster.jpg")),
        parent.join(format!("{stem}-poster.jpeg")),
        parent.join(format!("{stem}-poster.png")),
        parent.join(format!("{stem}-poster.webp")),
    ];
    if allow_dir_level_artwork(parent) {
        candidates.extend([
            parent.join("poster.jpg"),
            parent.join("poster.jpeg"),
            parent.join("poster.png"),
            parent.join("poster.webp"),
            parent.join("folder.jpg"),
            parent.join("folder.png"),
            parent.join("folder.webp"),
        ]);
    }
    if is_season_dir(parent)
        && let Some(show_dir) = parent.parent()
    {
        candidates.extend([
            show_dir.join("poster.jpg"),
            show_dir.join("poster.jpeg"),
            show_dir.join("poster.png"),
            show_dir.join("poster.webp"),
            show_dir.join("folder.jpg"),
            show_dir.join("folder.png"),
            show_dir.join("folder.webp"),
        ]);
    }
    candidates.into_iter().find(|p| p.is_file())
}

fn find_fanart_path(video_path: &Path) -> Option<PathBuf> {
    let parent = video_path.parent()?;
    let stem = video_path.file_stem()?.to_str()?;
    let mut candidates = vec![
        parent.join(format!("{stem}-fanart.jpg")),
        parent.join(format!("{stem}-fanart.jpeg")),
        parent.join(format!("{stem}-fanart.png")),
        parent.join(format!("{stem}-fanart.webp")),
    ];
    if allow_dir_level_artwork(parent) {
        candidates.extend([
            parent.join("fanart.jpg"),
            parent.join("fanart.jpeg"),
            parent.join("fanart.png"),
            parent.join("fanart.webp"),
            parent.join("backdrop.jpg"),
            parent.join("backdrop.jpeg"),
            parent.join("backdrop.png"),
            parent.join("backdrop.webp"),
        ]);
    }
    if is_season_dir(parent)
        && let Some(show_dir) = parent.parent()
    {
        candidates.extend([
            show_dir.join("fanart.jpg"),
            show_dir.join("fanart.jpeg"),
            show_dir.join("fanart.png"),
            show_dir.join("fanart.webp"),
            show_dir.join("backdrop.jpg"),
            show_dir.join("backdrop.jpeg"),
            show_dir.join("backdrop.png"),
            show_dir.join("backdrop.webp"),
        ]);
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

fn strip_release_noise(raw: &str) -> String {
    let tag_re = Regex::new(
        r"(?i)(?:[\(\[]\s*(?:19\d{2}|20\d{2})\s*[\)\]]|\b(?:2160p|1080p|720p|480p|4k|uhd|bluray|bdrip|web-dl|webrip|hdtv|remux|x264|x265|h\.?264|h\.?265|hevc|av1|10bit|8bit|aac|ac3|dts|flac|hdr10|dv)\b.*$)",
    )
    .expect("valid release noise regex");
    let leading_group_re = Regex::new(r"^(?:\[[^\]]+\]\s*)+").expect("valid group regex");
    let without_group = leading_group_re.replace(raw.trim(), "");
    let without_noise = tag_re.replace(&without_group, "");
    without_noise
        .replace(['.', '_'], " ")
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .trim_matches([' ', '-', '–', '_'])
        .to_string()
}

fn is_unlikely_episode_num(n: u32) -> bool {
    matches!(n, 264 | 265 | 480 | 720 | 1080 | 2160) || (1900..=2099).contains(&n)
}

fn infer_show_from_parent(video_path: &Path) -> Option<String> {
    video_path.parent().and_then(|p| {
        let target_dir = if is_season_dir(p) { p.parent()? } else { p };
        let dir_name = target_dir.file_name()?.to_string_lossy();
        let cleaned = strip_release_noise(&dir_name);
        (!cleaned.is_empty()).then_some(cleaned)
    })
}

fn guess_episode_and_year(
    video_path: &Path,
) -> (
    bool,
    String,
    Option<String>,
    Option<u32>,
    Option<u32>,
    Option<i32>,
) {
    let stem = video_path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("Untitled");
    let se_re = Regex::new(r"(?i)S(\d{1,2})E(\d{1,4})").expect("valid regex");
    let zh_ep_re = Regex::new(r"第\s*0*(\d{1,4})\s*[话話集回]").expect("valid zh ep regex");
    let ep_word_re =
        Regex::new(r"(?i)(?:^|[\s._\-\[\(])(?:EP?|Episode)\s*0*(\d{1,4})(?:$|[\s._\-\]\)])")
            .expect("valid ep word regex");
    let dash_ep_re =
        Regex::new(r"\s+-\s+0*(\d{1,4})(?:v\d)?(?:$|[\s.\[\(])").expect("valid dash ep regex");
    let bracket_ep_re = Regex::new(r"\[0*(\d{1,3})(?:v\d)?\]").expect("valid bracket ep regex");
    let year_re = Regex::new(r"(?:^|[\s\.\(\[_-])(19\d{2}|20\d{2})(?:$|[\s\.\)\]_-])")
        .expect("valid year regex");

    let mut year = year_re
        .captures(stem)
        .and_then(|c| c.get(1))
        .and_then(|m| m.as_str().parse::<i32>().ok());
    if year.is_none()
        && let Some(parent_str) = video_path.parent().and_then(|p| p.to_str())
    {
        year = year_re
            .captures(parent_str)
            .and_then(|c| c.get(1))
            .and_then(|m| m.as_str().parse::<i32>().ok());
    }

    let season_from_dir = video_path.parent().and_then(parse_season_dir_number);

    if let Some(caps) = se_re.captures(stem) {
        let full_match = caps.get(0).unwrap();
        let season = caps.get(1).and_then(|m| m.as_str().parse::<u32>().ok());
        let ep = caps.get(2).and_then(|m| m.as_str().parse::<u32>().ok());
        let prefix = strip_release_noise(&stem[..full_match.start()]);
        let suffix = strip_release_noise(&stem[full_match.end()..]);

        let show_guess = if !prefix.is_empty() {
            Some(prefix)
        } else {
            infer_show_from_parent(video_path)
        };
        let ep_title = if !suffix.is_empty() {
            suffix
        } else {
            format!("S{:02}E{:02}", season.unwrap_or(1), ep.unwrap_or(1))
        };
        return (true, ep_title, show_guess, season, ep, year);
    }

    // Check anime/Chinese episode naming: `第02话`, `EP02`, ` - 02 `, `[02]`, or bare `02` inside a Season folder
    let extra_ep_match: Option<(usize, usize, u32)> = zh_ep_re
        .captures(stem)
        .and_then(|c| {
            let full = c.get(0)?;
            let ep = c.get(1)?.as_str().parse::<u32>().ok()?;
            Some((full.start(), full.end(), ep))
        })
        .or_else(|| {
            ep_word_re.captures(stem).and_then(|c| {
                let full = c.get(0)?;
                let ep = c.get(1)?.as_str().parse::<u32>().ok()?;
                Some((full.start(), full.end(), ep))
            })
        })
        .or_else(|| {
            dash_ep_re.captures(stem).and_then(|c| {
                let full = c.get(0)?;
                let ep = c.get(1)?.as_str().parse::<u32>().ok()?;
                (!is_unlikely_episode_num(ep)).then_some((full.start(), full.end(), ep))
            })
        })
        .or_else(|| {
            bracket_ep_re.captures(stem).and_then(|c| {
                let full = c.get(0)?;
                let ep = c.get(1)?.as_str().parse::<u32>().ok()?;
                (!is_unlikely_episode_num(ep)).then_some((full.start(), full.end(), ep))
            })
        })
        .or_else(|| {
            if season_from_dir.is_some() {
                let trimmed = stem.trim();
                let digits_len = trimmed.bytes().take_while(|b| b.is_ascii_digit()).count();
                if digits_len > 0 && digits_len <= 3 {
                    let ep = trimmed[..digits_len].parse::<u32>().ok()?;
                    if !is_unlikely_episode_num(ep) {
                        return Some((0, digits_len, ep));
                    }
                }
            }
            None
        });

    if let Some((m_start, m_end, ep)) = extra_ep_match {
        let prefix = strip_release_noise(&stem[..m_start]);
        let suffix = strip_release_noise(&stem[m_end..]);
        let season = season_from_dir.or(Some(1));
        let show_guess = if !prefix.is_empty() {
            Some(prefix)
        } else {
            infer_show_from_parent(video_path)
        };
        let ep_title = if !suffix.is_empty() {
            suffix
        } else {
            format!("S{:02}E{:02}", season.unwrap_or(1), ep)
        };
        return (true, ep_title, show_guess, season, Some(ep), year);
    }

    // Movie / standalone video: strip `(YYYY)` or `.YYYY.1080p...` from clean title
    let mut clean_movie_title = strip_release_noise(stem);
    if let Some(y) = year {
        let trailing_year = format!(" {y}");
        if let Some(stripped) = clean_movie_title.strip_suffix(&trailing_year)
            && !stripped.trim().is_empty()
        {
            clean_movie_title = stripped.trim_matches([' ', '-', '_']).to_string();
        }
    }
    if clean_movie_title.is_empty() {
        clean_movie_title = stem.to_string();
    }

    (false, clean_movie_title, None, None, None, year)
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

fn find_clearlogo_path(video_path: &Path) -> Option<PathBuf> {
    let parent = video_path.parent()?;
    let stem = video_path.file_stem()?.to_str()?;
    for ext in ["png", "webp", "jpg"] {
        let p = parent.join(format!("{stem}-clearlogo.{ext}"));
        if p.is_file() {
            return Some(p);
        }
    }
    if allow_dir_level_artwork(parent) {
        for ext in ["png", "webp", "jpg"] {
            let p = parent.join(format!("clearlogo.{ext}"));
            if p.is_file() {
                return Some(p);
            }
        }
    }
    None
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

/// Atomically writes `{stem_base}.{ext}` inside `dir` and removes any stale sibling with other
/// image extensions (`.jpg`/`.jpeg`/`.png`/`.webp`) so old artwork never shadows the updated file.
pub(crate) fn write_image_replacing_opposite_ext(
    dir: &Path,
    stem_base: &str,
    ext: &str,
    bytes: &[u8],
) -> DomainResult<()> {
    let target = dir.join(format!("{stem_base}.{ext}"));
    atomic_write_bytes(&target, bytes)?;
    for stale_ext in ["jpg", "jpeg", "png", "webp"] {
        if stale_ext != ext {
            let stale_path = dir.join(format!("{stem_base}.{stale_ext}"));
            if stale_path.is_file() {
                let _ = std::fs::remove_file(stale_path);
            }
        }
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
                    has_cover: meta.has_poster || meta.has_thumb,
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
                && dir_has_single_video(parent)
                && !p.with_extension("nfo").is_file()
            {
                parent.join("movie.nfo")
            } else {
                p.with_extension("nfo")
            };
            let xml = render_video_nfo_xml(&meta);
            atomic_write_bytes(&nfo_target, xml.as_bytes())?;

            // Keep Jellyfin `season.nfo` and `tvshow.nfo` synchronized when editing an archived episode
            if meta.is_episode {
                if let Some(parent) = p.parent() {
                    let season_nfo = parent.join("season.nfo");
                    if season_nfo.is_file() {
                        let mut existing_season = VideoFileMeta::default();
                        if let Ok(raw) = std::fs::read(&season_nfo) {
                            parse_nfo_into(&raw, &mut existing_season);
                        }
                        let s_num = meta.season_number.unwrap_or(1);
                        let s_title = (!existing_season.title.trim().is_empty())
                            .then_some(existing_season.title.as_str());
                        let s_xml = render_season_nfo_xml(
                            s_num,
                            s_title,
                            meta.year,
                            existing_season.summary.as_deref(),
                            meta.tmdb_id.as_deref(),
                            meta.bangumi_id.as_deref(),
                        );
                        let _ = atomic_write_bytes(&season_nfo, s_xml.as_bytes());
                    }
                }
                if let Some(show_nfo) = find_show_nfo_for_episode(&p) {
                    let mut existing_show = VideoFileMeta::default();
                    if let Ok(raw) = std::fs::read(&show_nfo) {
                        parse_nfo_into(&raw, &mut existing_show);
                    }
                    let show_xml = render_tvshow_nfo_xml(&meta, existing_show.summary.as_deref());
                    let _ = atomic_write_bytes(&show_nfo, show_xml.as_bytes());
                }
            }

            Ok(())
        })
        .await
        .map_err(|e| DomainError::Storage(format!("video nfo write failed: {e}")))??;

        self.inspect_file(path).await
    }

    async fn read_artwork(
        &self,
        path: &Path,
        slot: Option<&str>,
    ) -> DomainResult<Option<BinaryPayload>> {
        let p = path.to_path_buf();
        let slot_owned = slot.map(|s| s.trim().to_ascii_lowercase());
        tokio::task::spawn_blocking(move || {
            let resolved = match slot_owned.as_deref() {
                Some("fanart") | Some("backdrop") => find_fanart_path(&p),
                Some("thumb") => find_thumb_path(&p),
                Some("clearlogo") | Some("logo") => find_clearlogo_path(&p),
                Some("poster") | Some("cover") => find_poster_path(&p),
                _ => find_poster_path(&p)
                    .or_else(|| find_fanart_path(&p))
                    .or_else(|| find_thumb_path(&p)),
            };
            let Some(img_path) = resolved else {
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
        let sniffed = sniff_image_mime(bytes);
        let detected = if sniffed != "image/jpeg" {
            sniffed
        } else if !mime_type.trim().is_empty() {
            mime_type.trim()
        } else {
            sniffed
        };
        let ext = if detected.contains("png") {
            "png"
        } else if detected.contains("webp") {
            "webp"
        } else {
            "jpg"
        };
        let is_jellyfin_movie_dir =
            parent.join("movie.nfo").is_file() && dir_has_single_video(&parent);
        let is_jellyfin_season = is_season_dir(&parent) || parent.join("season.nfo").is_file();
        let is_jellyfin_show = parent.join("tvshow.nfo").is_file();
        let use_dir_artwork = is_jellyfin_movie_dir || is_jellyfin_season || is_jellyfin_show;

        let is_thumb = slot.eq_ignore_ascii_case("thumb");
        let is_clearlogo =
            slot.eq_ignore_ascii_case("clearlogo") || slot.eq_ignore_ascii_case("logo");
        let is_fanart =
            slot.eq_ignore_ascii_case("fanart") || slot.eq_ignore_ascii_case("backdrop");
        let stem_base = if is_thumb {
            format!("{stem}-thumb")
        } else if is_clearlogo {
            if use_dir_artwork {
                "clearlogo".to_string()
            } else {
                format!("{stem}-clearlogo")
            }
        } else if is_fanart {
            if use_dir_artwork {
                "fanart".to_string()
            } else {
                format!("{stem}-fanart")
            }
        } else if use_dir_artwork {
            "poster".to_string()
        } else {
            format!("{stem}-poster")
        };
        let raw = bytes.to_vec();

        tokio::task::spawn_blocking(move || -> DomainResult<()> {
            write_image_replacing_opposite_ext(&parent, &stem_base, ext, &raw)?;
            // If inside `Season XX/` and parent show directory has `tvshow.nfo`, also keep series poster/fanart present
            if !is_thumb
                && is_jellyfin_season
                && let Some(show_dir) = parent.parent()
                && show_dir.join("tvshow.nfo").is_file()
            {
                write_image_replacing_opposite_ext(show_dir, &stem_base, ext, &raw)?;
            }
            Ok(())
        })
        .await
        .map_err(|e| DomainError::Storage(format!("video artwork write failed: {e}")))??;
        Ok(())
    }
}

use std::path::{Path, PathBuf};

use quick_xml::Reader;
use quick_xml::events::Event;
use serde::{Deserialize, Serialize};

use super::filename::{dir_has_single_video, is_season_dir};
use crate::common::{append_xml_ref, decode_text_smart};

/// Root element type of a Kodi/Jellyfin `.nfo` document.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum NfoRootKind {
    #[default]
    Unknown,
    Movie,
    EpisodeDetails,
    TvShow,
    Season,
}

/// Detailed `<actor>` block from Kodi/Jellyfin `.nfo`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct NfoActor {
    pub name: String,
    pub role: Option<String>,
    pub thumb: Option<String>,
}

/// `<uniqueid type="..." default="...">value</uniqueid>` entry.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct NfoUniqueId {
    pub id_type: String,
    pub value: String,
    pub is_default: bool,
}

/// Audio stream summary inside `<fileinfo><streamdetails><audio>`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct NfoAudioStream {
    pub codec: String,
    pub language: Option<String>,
    pub channels: u32,
}

/// Subtitle stream summary inside `<fileinfo><streamdetails><subtitle>`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct NfoSubtitleStream {
    pub language: Option<String>,
}

/// Canonical Kodi/Jellyfin `.nfo` metadata model supporting `<movie>`, `<episodedetails>`,
/// `<tvshow>`, and `<season>` documents.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct VideoNfoMeta {
    pub root_kind: NfoRootKind,
    pub is_episode: bool,
    pub title: Option<String>,
    pub raw_title: Option<String>,
    pub season_title: Option<String>,
    pub original_title: Option<String>,
    pub show_title: Option<String>,
    pub season_number: Option<u32>,
    pub episode_number: Option<u32>,
    pub year: Option<i32>,
    pub premiered: Option<String>,
    pub mpaa: Option<String>,
    pub country: Option<String>,
    pub rating: Option<f32>,
    pub summary: Option<String>,
    pub outline: Option<String>,
    pub studio: Option<String>,
    pub genres: Vec<String>,
    pub tags: Vec<String>,
    pub directors: Vec<String>,
    pub actors: Vec<String>,
    pub actor_details: Vec<NfoActor>,
    pub unique_ids: Vec<NfoUniqueId>,
    pub tmdb_id: Option<String>,
    pub bangumi_id: Option<String>,
    pub collection_set: Option<String>,
    pub locked_fields: Vec<String>,
    pub video_codec: Option<String>,
    pub width: Option<u32>,
    pub height: Option<u32>,
    pub duration_secs: Option<u32>,
    pub audio_streams: Vec<NfoAudioStream>,
    pub subtitle_streams: Vec<NfoSubtitleStream>,
}

/// Finds an existing video-level `.nfo` (`{stem}.nfo`, or `movie.nfo` in a single-video folder).
pub fn find_existing_nfo(video_path: &Path, is_episode_guess: bool) -> Option<PathBuf> {
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

/// Finds `season.nfo` in an episode's parent directory if present.
pub fn find_season_nfo_for_episode(video_path: &Path) -> Option<PathBuf> {
    let season_nfo = video_path.parent()?.join("season.nfo");
    season_nfo.is_file().then_some(season_nfo)
}

/// Finds `tvshow.nfo` in an episode's parent directory or one level up when inside a Season folder.
pub fn find_show_nfo_for_episode(video_path: &Path) -> Option<PathBuf> {
    let parent = video_path.parent()?;
    let direct = parent.join("tvshow.nfo");
    if direct.is_file() {
        return Some(direct);
    }
    if (is_season_dir(parent) || parent.join("season.nfo").is_file())
        && let Some(show_dir) = parent.parent()
    {
        let up = show_dir.join("tvshow.nfo");
        if up.is_file() {
            return Some(up);
        }
    }
    None
}

/// Parses Kodi/Jellyfin `.nfo` XML bytes into `Some(VideoNfoMeta)` if a valid root element is present.
pub fn parse_nfo_xml(xml_bytes: &[u8]) -> Option<VideoNfoMeta> {
    let mut meta = VideoNfoMeta::default();
    if parse_nfo_into(xml_bytes, &mut meta) {
        Some(meta)
    } else {
        None
    }
}

/// Parses Kodi/Jellyfin `.nfo` XML bytes onto an existing `VideoNfoMeta`, returning `true` if any
/// recognized NFO root or metadata field was parsed.
pub fn parse_nfo_into(xml_bytes: &[u8], meta: &mut VideoNfoMeta) -> bool {
    let (decoded, _) = decode_text_smart(xml_bytes);
    let xml_str = decoded.trim_start_matches('\u{feff}');
    let mut reader = Reader::from_str(xml_str);
    reader.config_mut().trim_text(false);

    let mut stack: Vec<String> = Vec::new();
    let mut current_unique_attrs: Option<(String, bool)> = None;
    let mut current_actor: Option<NfoActor> = None;
    let mut current_audio: Option<NfoAudioStream> = None;
    let mut current_sub: Option<NfoSubtitleStream> = None;
    let mut text_acc = String::new();
    let mut found_any = false;

    loop {
        match reader.read_event() {
            Ok(Event::Start(ref e)) => {
                let name = e.local_name().as_ref().to_ascii_lowercase();
                if stack.is_empty() {
                    match name.as_str() {
                        "movie" => {
                            meta.root_kind = NfoRootKind::Movie;
                            meta.is_episode = false;
                            found_any = true;
                        }
                        "episodedetails" => {
                            meta.root_kind = NfoRootKind::EpisodeDetails;
                            meta.is_episode = true;
                            found_any = true;
                        }
                        "tvshow" => {
                            meta.root_kind = NfoRootKind::TvShow;
                            found_any = true;
                        }
                        "season" => {
                            meta.root_kind = NfoRootKind::Season;
                            found_any = true;
                        }
                        _ => {}
                    }
                } else if stack.len() == 1 {
                    match name.as_str() {
                        "uniqueid" => {
                            let mut uid_type = String::new();
                            let mut is_default = false;
                            for attr in e.attributes().flatten() {
                                let k = attr.key.local_name();
                                let Ok(val) =
                                    attr.normalized_value(quick_xml::XmlVersion::Implicit1_0)
                                else {
                                    continue;
                                };
                                if k.as_ref().eq_ignore_ascii_case("type") {
                                    uid_type = val.trim().to_ascii_lowercase();
                                } else if k.as_ref().eq_ignore_ascii_case("default") {
                                    is_default = val.trim().eq_ignore_ascii_case("true");
                                }
                            }
                            current_unique_attrs = Some((uid_type, is_default));
                        }
                        "actor" => current_actor = Some(NfoActor::default()),
                        _ => {}
                    }
                } else if stack.last().map(String::as_str) == Some("streamdetails") {
                    match name.as_str() {
                        "audio" => current_audio = Some(NfoAudioStream::default()),
                        "subtitle" => current_sub = Some(NfoSubtitleStream::default()),
                        _ => {}
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
                    if parent == "actor" {
                        if let Some(ref mut act) = current_actor {
                            match cur {
                                "name" => act.name = trimmed.to_string(),
                                "role" => act.role = Some(trimmed.to_string()),
                                "thumb" => act.thumb = Some(trimmed.to_string()),
                                _ => {}
                            }
                        }
                    } else if parent == "video" && len >= 4 && stack[len - 3] == "streamdetails" {
                        match cur {
                            "codec" => meta.video_codec = Some(trimmed.to_string()),
                            "width" => meta.width = trimmed.parse::<u32>().ok(),
                            "height" => meta.height = trimmed.parse::<u32>().ok(),
                            "durationinseconds" => meta.duration_secs = trimmed.parse::<u32>().ok(),
                            _ => {}
                        }
                    } else if parent == "audio"
                        && let Some(ref mut aud) = current_audio
                    {
                        match cur {
                            "codec" => aud.codec = trimmed.to_string(),
                            "language" => aud.language = Some(trimmed.to_string()),
                            "channels" => aud.channels = trimmed.parse::<u32>().unwrap_or(0),
                            _ => {}
                        }
                    } else if parent == "subtitle"
                        && let Some(ref mut sub) = current_sub
                    {
                        if cur == "language" {
                            sub.language = Some(trimmed.to_string());
                        }
                    } else if parent == "set" && cur == "name" {
                        meta.collection_set = Some(trimmed.to_string());
                        found_any = true;
                    } else if len == 2 {
                        apply_top_level_nfo_field(
                            meta,
                            cur,
                            trimmed,
                            current_unique_attrs.as_ref(),
                            &mut found_any,
                        );
                    }
                }

                if let Some(popped) = stack.pop() {
                    match popped.as_str() {
                        "uniqueid" => current_unique_attrs = None,
                        "actor" => {
                            if let Some(act) = current_actor.take()
                                && !act.name.trim().is_empty()
                            {
                                if !meta.actors.iter().any(|n| n == &act.name) {
                                    meta.actors.push(act.name.clone());
                                }
                                meta.actor_details.push(act);
                                found_any = true;
                            }
                        }
                        "audio" => {
                            if let Some(aud) = current_audio.take()
                                && (!aud.codec.is_empty()
                                    || aud.language.is_some()
                                    || aud.channels > 0)
                            {
                                meta.audio_streams.push(aud);
                            }
                        }
                        "subtitle" => {
                            if let Some(sub) = current_sub.take()
                                && sub.language.is_some()
                            {
                                meta.subtitle_streams.push(sub);
                            }
                        }
                        _ => {}
                    }
                }
                text_acc.clear();
            }
            Ok(Event::Eof) | Err(_) => break,
            _ => {}
        }
    }

    found_any
}

fn apply_top_level_nfo_field(
    meta: &mut VideoNfoMeta,
    cur: &str,
    trimmed: &str,
    unique_attrs: Option<&(String, bool)>,
    found_any: &mut bool,
) {
    match cur {
        "title" => {
            meta.raw_title = Some(trimmed.to_string());
            match meta.root_kind {
                NfoRootKind::TvShow => {
                    if meta.show_title.is_none() {
                        meta.show_title = Some(trimmed.to_string());
                    }
                }
                NfoRootKind::Season => meta.season_title = Some(trimmed.to_string()),
                _ => meta.title = Some(trimmed.to_string()),
            }
            *found_any = true;
        }
        "originaltitle" => {
            meta.original_title = Some(trimmed.to_string());
            *found_any = true;
        }
        "showtitle" => {
            meta.show_title = Some(trimmed.to_string());
            *found_any = true;
        }
        "season" | "seasonnumber" => {
            if let Ok(s) = trimmed.parse::<u32>() {
                meta.season_number = Some(s);
                *found_any = true;
            }
        }
        "episode" => {
            if let Ok(ep) = trimmed.parse::<u32>() {
                meta.episode_number = Some(ep);
                *found_any = true;
            }
        }
        "year" => {
            if let Ok(y) = trimmed.parse::<i32>() {
                meta.year = Some(y);
                *found_any = true;
            }
        }
        "premiered" | "aired" => {
            if meta.premiered.is_none() || cur == "premiered" {
                meta.premiered = Some(trimmed.to_string());
            }
            if meta.year.is_none() {
                meta.year = trimmed.get(..4).and_then(|s| s.parse::<i32>().ok());
            }
            *found_any = true;
        }
        "mpaa" => {
            meta.mpaa = Some(trimmed.to_string());
            *found_any = true;
        }
        "country" => {
            if meta.country.is_none() {
                meta.country = Some(trimmed.to_string());
            }
            *found_any = true;
        }
        "rating" => {
            meta.rating = trimmed.parse::<f32>().ok();
            *found_any = true;
        }
        "plot" => {
            meta.summary = Some(trimmed.to_string());
            *found_any = true;
        }
        "outline" => {
            meta.outline = Some(trimmed.to_string());
            if meta.summary.is_none() {
                meta.summary = Some(trimmed.to_string());
            }
            *found_any = true;
        }
        "studio" => {
            meta.studio = Some(trimmed.to_string());
            *found_any = true;
        }
        "genre" => {
            if !meta.genres.iter().any(|g| g.eq_ignore_ascii_case(trimmed)) {
                meta.genres.push(trimmed.to_string());
            }
            *found_any = true;
        }
        "tag" => {
            if !meta.tags.iter().any(|tg| tg.eq_ignore_ascii_case(trimmed)) {
                meta.tags.push(trimmed.to_string());
            }
            *found_any = true;
        }
        "director" => {
            if !meta.directors.iter().any(|d| d == trimmed) {
                meta.directors.push(trimmed.to_string());
            }
            *found_any = true;
        }
        "set" if meta.collection_set.is_none() => {
            meta.collection_set = Some(trimmed.to_string());
            *found_any = true;
        }
        "lockedfields" => {
            for f in trimmed.split('|') {
                let lf = f.trim().to_ascii_lowercase();
                if !lf.is_empty() && !meta.locked_fields.contains(&lf) {
                    meta.locked_fields.push(lf);
                }
            }
            *found_any = true;
        }
        "tmdbid" => {
            meta.tmdb_id = Some(trimmed.to_string());
            *found_any = true;
        }
        "uniqueid" => {
            let (uid_type, is_default) = unique_attrs
                .cloned()
                .unwrap_or_else(|| (String::new(), false));
            match uid_type.as_str() {
                "tmdb" => meta.tmdb_id = Some(trimmed.to_string()),
                "bangumi" | "bgm" => meta.bangumi_id = Some(trimmed.to_string()),
                _ => {}
            }
            meta.unique_ids.push(NfoUniqueId {
                id_type: uid_type,
                value: trimmed.to_string(),
                is_default,
            });
            *found_any = true;
        }
        _ => {}
    }
}

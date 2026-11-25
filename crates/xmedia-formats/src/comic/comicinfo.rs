use std::collections::HashMap;
use std::fs::File;
use std::io::{BufReader, Read};
use std::path::Path;

use quick_xml::Reader;
use quick_xml::events::Event;
use serde::{Deserialize, Serialize};
use zip::ZipArchive;

use super::ComicReadingDirection;
pub use super::comicinfo_write::render_comic_info_xml_preserving;
use crate::book::BookFormatExt;
use crate::book::opf::parse_opf_xml;
use crate::common::{append_xml_ref, decode_text_smart, xml_attr_value};

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct ComicInfoMeta {
    pub series: Option<String>,
    pub series_group: Option<String>,
    pub title: Option<String>,
    pub number: Option<f64>,
    pub number_raw: Option<String>,
    pub count: Option<u32>,
    pub volume: Option<f64>,
    pub summary: Option<String>,
    pub writers: Vec<String>,
    pub pencillers: Vec<String>,
    pub inkers: Vec<String>,
    pub colorists: Vec<String>,
    pub letterers: Vec<String>,
    pub cover_artists: Vec<String>,
    pub publisher: Option<String>,
    pub genres: Vec<String>,
    pub tags: Vec<String>,
    pub characters: Vec<String>,
    pub age_rating: Option<String>,
    pub web: Option<String>,
    pub year: Option<i32>,
    pub month: Option<u32>,
    pub day: Option<u32>,
    pub language_iso: Option<String>,
    pub page_count: Option<u32>,
    pub reading_direction: Option<ComicReadingDirection>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ComicInfoPageAttr {
    pub page_type: Option<String>,
    pub double_page: Option<bool>,
    pub width: Option<u32>,
    pub height: Option<u32>,
}

fn split_csv(raw: &str) -> Vec<String> {
    raw.split([',', ';', '、'])
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_owned)
        .collect()
}

fn push_unique_csv(target: &mut Vec<String>, raw: &str) {
    for item in split_csv(raw) {
        if !target.contains(&item) {
            target.push(item);
        }
    }
}

/// Parses `ComicInfo.xml` bytes (with smart charset decoding), returning `None` if malformed or empty.
pub fn parse_comic_info_xml(xml_bytes: &[u8]) -> Option<ComicInfoMeta> {
    let (decoded, _) = decode_text_smart(xml_bytes);
    let mut reader = Reader::from_str(&decoded);
    reader.config_mut().trim_text(false);

    let mut tag_stack: Vec<(String, String)> = Vec::new();
    let mut meta = ComicInfoMeta::default();
    let mut manga_raw: Option<String> = None;
    let mut format_raw: Option<String> = None;
    let mut found_any = false;

    loop {
        match reader.read_event() {
            Ok(Event::Start(ref e)) => {
                let tag = e.local_name().as_ref().to_ascii_lowercase();
                if tag_stack.len() < 64 {
                    tag_stack.push((tag, String::new()));
                }
            }
            Ok(Event::Text(t)) => {
                if let Some((_, acc)) = tag_stack.last_mut() {
                    acc.push_str(&t.xml10_content());
                }
            }
            Ok(Event::GeneralRef(r)) => {
                if let Some((_, acc)) = tag_stack.last_mut() {
                    append_xml_ref(acc, &r);
                }
            }
            Ok(Event::CData(c)) => {
                if let Some((_, acc)) = tag_stack.last_mut() {
                    acc.push_str(c.as_ref());
                }
            }
            Ok(Event::End(_)) => {
                if let Some((tag, acc)) = tag_stack.pop() {
                    let trimmed = acc.trim();
                    if !trimmed.is_empty() {
                        found_any = true;
                        match tag.as_str() {
                            "series" => meta.series = Some(trimmed.to_owned()),
                            "seriesgroup" => meta.series_group = Some(trimmed.to_owned()),
                            "title" => meta.title = Some(trimmed.to_owned()),
                            "number" => {
                                meta.number_raw = Some(trimmed.to_owned());
                                meta.number = trimmed
                                    .parse::<f64>()
                                    .ok()
                                    .filter(|v| v.is_finite() && *v >= 0.0);
                            }
                            "count" => meta.count = trimmed.parse::<u32>().ok(),
                            "volume" => {
                                meta.volume = trimmed
                                    .parse::<f64>()
                                    .ok()
                                    .filter(|v| v.is_finite() && *v >= 0.0);
                            }
                            "summary" => meta.summary = Some(trimmed.to_owned()),
                            "writer" => push_unique_csv(&mut meta.writers, trimmed),
                            "penciller" | "artist" => {
                                push_unique_csv(&mut meta.pencillers, trimmed)
                            }
                            "inker" => push_unique_csv(&mut meta.inkers, trimmed),
                            "colorist" => push_unique_csv(&mut meta.colorists, trimmed),
                            "letterer" => push_unique_csv(&mut meta.letterers, trimmed),
                            "coverartist" => push_unique_csv(&mut meta.cover_artists, trimmed),
                            "publisher" => meta.publisher = Some(trimmed.to_owned()),
                            "genre" => push_unique_csv(&mut meta.genres, trimmed),
                            "tags" => push_unique_csv(&mut meta.tags, trimmed),
                            "characters" => push_unique_csv(&mut meta.characters, trimmed),
                            "agerating" => meta.age_rating = Some(trimmed.to_owned()),
                            "web" => meta.web = Some(trimmed.to_owned()),
                            "year" => meta.year = trimmed.parse::<i32>().ok(),
                            "month" => meta.month = trimmed.parse::<u32>().ok(),
                            "day" => meta.day = trimmed.parse::<u32>().ok(),
                            "languageiso" => meta.language_iso = Some(trimmed.to_owned()),
                            "pagecount" => meta.page_count = trimmed.parse::<u32>().ok(),
                            "manga" => manga_raw = Some(trimmed.to_ascii_lowercase()),
                            "format" => format_raw = Some(trimmed.to_ascii_lowercase()),
                            _ => {}
                        }
                    }
                }
            }
            Ok(Event::Eof) => break,
            Err(_) => return None,
            _ => {}
        }
    }

    if !found_any {
        return None;
    }

    let is_webtoon_format = format_raw
        .as_deref()
        .is_some_and(|f| f.contains("webtoon") || f.contains("long strip") || f.contains("条漫"));
    let is_webtoon_tag = meta.genres.iter().chain(meta.tags.iter()).any(|t| {
        let low = t.to_ascii_lowercase();
        low.contains("webtoon") || low.contains("long strip") || t.contains("条漫")
    });

    if is_webtoon_format {
        meta.reading_direction = Some(ComicReadingDirection::Webtoon);
    } else if let Some(m) = manga_raw {
        if matches!(m.as_str(), "yesandrighttoleft" | "yes" | "true") {
            meta.reading_direction = Some(ComicReadingDirection::Rtl);
        } else if matches!(m.as_str(), "no" | "false") {
            meta.reading_direction = Some(if is_webtoon_tag {
                ComicReadingDirection::Webtoon
            } else {
                ComicReadingDirection::Ltr
            });
        }
    } else if is_webtoon_tag {
        meta.reading_direction = Some(ComicReadingDirection::Webtoon);
    }

    Some(meta)
}

/// Parses `<Pages><Page Image="..." .../></Pages>` attributes indexed by page number.
pub fn parse_comic_info_pages_map(xml_bytes: &[u8]) -> HashMap<usize, ComicInfoPageAttr> {
    let (xml_str, _) = decode_text_smart(xml_bytes);
    let mut reader = Reader::from_str(&xml_str);
    reader.config_mut().trim_text(true);
    let mut map = HashMap::new();

    loop {
        match reader.read_event() {
            Ok(Event::Empty(ref e) | Event::Start(ref e))
                if e.local_name().as_ref().eq_ignore_ascii_case("Page") =>
            {
                if let Some(idx) = xml_attr_value(e, "Image").and_then(|v| v.trim().parse().ok()) {
                    map.insert(
                        idx,
                        ComicInfoPageAttr {
                            page_type: xml_attr_value(e, "Type").filter(|s| !s.trim().is_empty()),
                            double_page: xml_attr_value(e, "DoublePage")
                                .map(|v| v.trim().eq_ignore_ascii_case("true")),
                            width: xml_attr_value(e, "ImageWidth")
                                .and_then(|v| v.trim().parse().ok()),
                            height: xml_attr_value(e, "ImageHeight")
                                .and_then(|v| v.trim().parse().ok()),
                        },
                    );
                }
            }
            Ok(Event::Eof) | Err(_) => break,
            _ => {}
        }
    }
    map
}

/// Extracts `ComicInfo.xml` bytes from a ZIP/CBZ archive or comic image directory if present.
pub fn extract_existing_comic_info_bytes(path: &Path) -> Option<Vec<u8>> {
    if path.is_dir() {
        let entries = std::fs::read_dir(path).ok()?;
        for entry in entries.flatten() {
            let p = entry.path();
            if p.is_file()
                && p.file_name()
                    .and_then(|s| s.to_str())
                    .is_some_and(|n| n.eq_ignore_ascii_case("comicinfo.xml"))
            {
                return std::fs::read(&p).ok();
            }
        }
        return None;
    }
    let file = File::open(path).ok()?;
    let mut archive = ZipArchive::new(BufReader::new(file)).ok()?;
    for idx in 0..archive.len() {
        let Ok(mut entry) = archive.by_index(idx) else {
            continue;
        };
        let Ok(name_raw) = entry.name() else {
            continue;
        };
        let lower = name_raw.to_ascii_lowercase();
        if lower == "comicinfo.xml" || lower.ends_with("/comicinfo.xml") {
            let mut buf = Vec::new();
            if entry.read_to_end(&mut buf).is_ok() {
                return Some(buf);
            }
        }
    }
    None
}

/// Applies fallback metadata from an EPUB `content.opf` document into `ComicInfoMeta`.
pub fn apply_epub_opf_into_comic_meta(opf_bytes: &[u8], meta: &mut ComicInfoMeta) {
    let opf = parse_opf_xml(opf_bytes, BookFormatExt::Epub, "");
    if !opf.meta.title.is_empty() {
        if meta.series.is_none() {
            meta.series = Some(
                opf.meta
                    .series
                    .clone()
                    .unwrap_or_else(|| opf.meta.title.clone()),
            );
        }
        meta.title = Some(opf.meta.title);
    } else if meta.series.is_none() {
        meta.series = opf.meta.series;
    }
    if meta.volume.is_none() {
        meta.volume = opf.meta.series_index;
    }
    for author in opf.meta.authors {
        if !meta.writers.contains(&author) {
            meta.writers.push(author);
        }
    }
    if meta.publisher.is_none() {
        meta.publisher = opf.meta.publisher;
    }
    if meta.summary.is_none() {
        meta.summary = opf.meta.description;
    }
    if meta.language_iso.is_none() {
        meta.language_iso = opf.meta.language;
    }
    for tag in opf.meta.tags {
        if !meta.genres.contains(&tag) {
            meta.genres.push(tag);
        }
    }
    if meta.year.is_none() {
        meta.year = opf
            .meta
            .published_at
            .as_deref()
            .and_then(|d| d.get(0..4))
            .and_then(|y| y.parse::<i32>().ok());
    }

    if opf
        .rendition_flow
        .as_deref()
        .is_some_and(|f| f.contains("scrolled"))
    {
        meta.reading_direction = Some(ComicReadingDirection::Webtoon);
    } else if let Some(dir) = opf.spine_progression_direction {
        if dir.eq_ignore_ascii_case("ltr") {
            meta.reading_direction = Some(ComicReadingDirection::Ltr);
        } else if dir.eq_ignore_ascii_case("rtl") {
            meta.reading_direction = Some(ComicReadingDirection::Rtl);
        }
    } else if meta.language_iso.as_deref().is_some_and(|l| {
        let low = l.trim().to_ascii_lowercase();
        matches!(low.get(0..2), Some("en" | "fr" | "es" | "de"))
    }) {
        meta.reading_direction = Some(ComicReadingDirection::Ltr);
    }
}

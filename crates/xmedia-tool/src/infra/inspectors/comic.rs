use std::fs::File;
use std::io::{BufReader, BufWriter, Read, Write};
use std::path::{Path, PathBuf};

use async_trait::async_trait;
use quick_xml::Reader;
use quick_xml::escape::escape;
use quick_xml::events::Event;
use regex::Regex;
use xmedia_core::application::ports::BinaryPayload;
use xmedia_core::domain::error::{DomainError, DomainResult};
use xmedia_core::domain::library::MediaKind;
use xmedia_core::infra::thumbnail::sniff_image_mime;
use zip::write::SimpleFileOptions;
use zip::{CompressionMethod, ZipArchive, ZipWriter};

use crate::domain::media::{
    ComicFileMeta, ComicMetaPatch, CompletenessFlags, MediaMetadataPatch, MediaMetadataPayload,
};
use crate::domain::ports::{MediaInspectorPort, QuickFileSummary};
use crate::infra::atomic_fs::atomic_write_via_temp;
use crate::infra::inspectors::book::attr_value;
use crate::infra::inspectors::video::write_image_replacing_opposite_ext;

pub struct CbzComicInspector;

const COMIC_EXTENSIONS: &[&str] = &["cbz", "zip", "epub"];

fn is_image_entry(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    if lower.ends_with('/') || lower.contains("__macosx") {
        return false;
    }
    lower.ends_with(".jpg")
        || lower.ends_with(".jpeg")
        || lower.ends_with(".png")
        || lower.ends_with(".webp")
        || lower.ends_with(".gif")
        || lower.ends_with(".avif")
        || lower.ends_with(".bmp")
}

fn natural_sort_key(name: &str) -> (u8, Vec<(bool, u64, String)>) {
    let lower = name.to_ascii_lowercase();
    let base = lower.rsplit('/').next().unwrap_or(&lower);
    let priority = if base.starts_with("000_cover.") || base.starts_with("cover.") {
        0
    } else {
        1
    };
    let mut parts = Vec::new();
    let mut cur_digits = String::new();
    let mut cur_text = String::new();
    for ch in lower.chars() {
        if ch.is_ascii_digit() {
            if !cur_text.is_empty() {
                parts.push((false, 0, std::mem::take(&mut cur_text)));
            }
            cur_digits.push(ch);
        } else {
            if !cur_digits.is_empty() {
                let n = cur_digits.parse::<u64>().unwrap_or(0);
                cur_digits.clear();
                parts.push((true, n, String::new()));
            }
            cur_text.push(ch);
        }
    }
    if !cur_digits.is_empty() {
        let n = cur_digits.parse::<u64>().unwrap_or(0);
        parts.push((true, n, String::new()));
    }
    if !cur_text.is_empty() {
        parts.push((false, 0, cur_text));
    }
    (priority, parts)
}

fn apply_epub_opf_fallback_into_comic(opf_bytes: &[u8], meta: &mut ComicFileMeta) {
    let xml_str = String::from_utf8_lossy(opf_bytes);
    let mut reader = Reader::from_str(&xml_str);
    reader.config_mut().trim_text(false);

    let mut cur_tag: Option<String> = None;
    let mut cur_meta_prop: Option<String> = None;
    let mut text_acc = String::new();
    let mut spine_dir: Option<String> = None;
    let mut rendition_flow: Option<String> = None;
    let mut opf_title: Option<String> = None;

    loop {
        match reader.read_event() {
            Ok(Event::Start(ref e)) => {
                let local = e.local_name().as_ref().to_ascii_lowercase();
                if local == "spine" {
                    spine_dir = attr_value(e, "page-progression-direction");
                } else if local == "meta" {
                    cur_meta_prop = attr_value(e, "property");
                    if attr_value(e, "name")
                        .is_some_and(|n| n.eq_ignore_ascii_case("calibre:series"))
                        && let Some(c) = attr_value(e, "content").filter(|s| !s.trim().is_empty())
                    {
                        meta.series = Some(c.trim().to_string());
                    }
                }
                cur_tag = Some(local);
                text_acc.clear();
            }
            Ok(Event::Empty(ref e)) => {
                let local = e.local_name().as_ref().to_ascii_lowercase();
                if local == "spine" {
                    spine_dir = attr_value(e, "page-progression-direction");
                } else if local == "meta"
                    && attr_value(e, "name")
                        .is_some_and(|n| n.eq_ignore_ascii_case("calibre:series"))
                    && let Some(c) = attr_value(e, "content").filter(|s| !s.trim().is_empty())
                {
                    meta.series = Some(c.trim().to_string());
                }
            }
            Ok(Event::Text(t)) if cur_tag.is_some() => {
                text_acc.push_str(&t.xml10_content());
            }
            Ok(Event::GeneralRef(r)) if cur_tag.is_some() => {
                super::append_xml_ref(&mut text_acc, &r);
            }
            Ok(Event::End(_)) => {
                if let Some(tag) = cur_tag.take() {
                    let trimmed = text_acc.trim();
                    if !trimmed.is_empty() {
                        match tag.as_str() {
                            "title" if opf_title.is_none() => {
                                opf_title = Some(trimmed.to_string());
                            }
                            "creator" if !meta.writers.iter().any(|w| w == trimmed) => {
                                meta.writers.push(trimmed.to_string());
                            }
                            "publisher" if meta.publisher.is_none() => {
                                meta.publisher = Some(trimmed.to_string());
                            }
                            "description" if meta.summary.is_none() => {
                                meta.summary = Some(trimmed.to_string());
                            }
                            "language" if meta.language_iso.is_none() => {
                                meta.language_iso = Some(trimmed.to_string());
                            }
                            "subject" if !meta.genres.iter().any(|g| g == trimmed) => {
                                meta.genres.push(trimmed.to_string());
                            }
                            "date" if meta.year.is_none() => {
                                meta.year = trimmed.get(0..4).and_then(|y| y.parse::<i32>().ok());
                            }
                            "meta" => {
                                if let Some(prop) = cur_meta_prop.as_deref() {
                                    let pl = prop.trim().to_ascii_lowercase();
                                    if pl == "belongs-to-collection" && meta.series.is_none() {
                                        meta.series = Some(trimmed.to_string());
                                    } else if pl == "rendition:flow" {
                                        rendition_flow = Some(trimmed.to_ascii_lowercase());
                                    }
                                }
                            }
                            _ => {}
                        }
                    }
                }
                cur_meta_prop = None;
                text_acc.clear();
            }
            Ok(Event::Eof) | Err(_) => break,
            _ => {}
        }
    }

    if let Some(t) = opf_title {
        if meta.series.is_none() {
            meta.series = Some(t.clone());
        }
        meta.title = Some(t);
    }

    if rendition_flow
        .as_deref()
        .is_some_and(|f| f.contains("scrolled"))
    {
        meta.reading_direction = "webtoon".to_string();
    } else if let Some(dir) = spine_dir {
        if dir.eq_ignore_ascii_case("ltr") {
            meta.reading_direction = "ltr".to_string();
        } else if dir.eq_ignore_ascii_case("rtl") {
            meta.reading_direction = "rtl".to_string();
        }
    } else if meta.language_iso.as_deref().is_some_and(|l| {
        let low = l.trim().to_ascii_lowercase();
        matches!(low.get(0..2), Some("en" | "fr" | "es" | "de"))
    }) {
        meta.reading_direction = "ltr".to_string();
    }
}

fn split_csv(raw: &str) -> Vec<String> {
    raw.split([',', ';', '、'])
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_owned)
        .collect()
}

fn parse_comic_info_into(xml_bytes: &[u8], meta: &mut ComicFileMeta) {
    let xml_str = String::from_utf8_lossy(xml_bytes);
    let mut reader = Reader::from_str(&xml_str);
    reader.config_mut().trim_text(false);

    let mut cur_tag: Option<String> = None;
    let mut text_acc = String::new();
    let mut manga_raw: Option<String> = None;
    let mut format_raw: Option<String> = None;

    loop {
        match reader.read_event() {
            Ok(Event::Start(ref e)) => {
                cur_tag = Some(e.local_name().as_ref().to_ascii_lowercase());
                text_acc.clear();
            }
            Ok(Event::Text(t)) => {
                if cur_tag.is_some() {
                    text_acc.push_str(&t.xml10_content());
                }
            }
            Ok(Event::GeneralRef(r)) => {
                if cur_tag.is_some() {
                    super::append_xml_ref(&mut text_acc, &r);
                }
            }
            Ok(Event::CData(c)) => {
                if cur_tag.is_some() {
                    text_acc.push_str(c.as_ref());
                }
            }
            Ok(Event::End(_)) => {
                if let Some(tag) = cur_tag.take() {
                    let trimmed = text_acc.trim();
                    if !trimmed.is_empty() {
                        match tag.as_str() {
                            "series" => meta.series = Some(trimmed.to_string()),
                            "seriesgroup" => meta.series_group = Some(trimmed.to_string()),
                            "title" => meta.title = Some(trimmed.to_string()),
                            "number" => meta.number = Some(trimmed.to_string()),
                            "volume" => {
                                meta.volume = trimmed.parse::<i32>().ok().filter(|&v| v >= 0)
                            }
                            "summary" => meta.summary = Some(trimmed.to_string()),
                            "writer" => meta.writers = split_csv(trimmed),
                            "penciller" | "artist" => {
                                for p in split_csv(trimmed) {
                                    if !meta.pencillers.contains(&p) {
                                        meta.pencillers.push(p);
                                    }
                                }
                            }
                            "publisher" => meta.publisher = Some(trimmed.to_string()),
                            "genre" => meta.genres = split_csv(trimmed),
                            "tags" => meta.tags = split_csv(trimmed),
                            "year" => meta.year = trimmed.parse::<i32>().ok(),
                            "languageiso" => meta.language_iso = Some(trimmed.to_string()),
                            "manga" => manga_raw = Some(trimmed.to_ascii_lowercase()),
                            "format" => format_raw = Some(trimmed.to_ascii_lowercase()),
                            _ => {}
                        }
                    }
                }
                text_acc.clear();
            }
            Ok(Event::Eof) | Err(_) => break,
            _ => {}
        }
    }

    let is_webtoon_format = format_raw
        .as_deref()
        .is_some_and(|f| f.contains("webtoon") || f.contains("条漫"));
    let is_webtoon_tag = meta.genres.iter().chain(meta.tags.iter()).any(|t| {
        let l = t.to_ascii_lowercase();
        l == "webtoon" || l == "条漫"
    });

    if is_webtoon_format {
        meta.reading_direction = "webtoon".to_string();
    } else if let Some(m) = manga_raw {
        if m == "yesandrighttoleft" || m == "yes" || m == "true" {
            meta.reading_direction = "rtl".to_string();
        } else if m == "no" || m == "false" {
            meta.reading_direction = if is_webtoon_tag {
                "webtoon".to_string()
            } else {
                "ltr".to_string()
            };
        }
    } else if is_webtoon_tag {
        meta.reading_direction = "webtoon".to_string();
    }
}

fn render_comic_info_xml(meta: &ComicFileMeta) -> String {
    let mut xml = String::with_capacity(768);
    xml.push_str("<?xml version=\"1.0\" encoding=\"utf-8\"?>\n");
    xml.push_str("<ComicInfo xmlns:xsd=\"http://www.w3.org/2001/XMLSchema\" xmlns:xsi=\"http://www.w3.org/2001/XMLSchema-instance\">\n");

    if let Some(ref s) = meta.series
        && !s.trim().is_empty()
    {
        xml.push_str(&format!("  <Series>{}</Series>\n", escape(s.trim())));
    }
    if let Some(ref sg) = meta.series_group
        && !sg.trim().is_empty()
    {
        xml.push_str(&format!(
            "  <SeriesGroup>{}</SeriesGroup>\n",
            escape(sg.trim())
        ));
    }
    if let Some(ref t) = meta.title
        && !t.trim().is_empty()
    {
        xml.push_str(&format!("  <Title>{}</Title>\n", escape(t.trim())));
    }
    if let Some(ref n) = meta.number
        && !n.trim().is_empty()
    {
        xml.push_str(&format!("  <Number>{}</Number>\n", escape(n.trim())));
    }
    if let Some(v) = meta.volume {
        xml.push_str(&format!("  <Volume>{v}</Volume>\n"));
    }
    if let Some(ref sum) = meta.summary
        && !sum.trim().is_empty()
    {
        xml.push_str(&format!("  <Summary>{}</Summary>\n", escape(sum.trim())));
    }
    if !meta.writers.is_empty() {
        xml.push_str(&format!(
            "  <Writer>{}</Writer>\n",
            escape(meta.writers.join(", "))
        ));
    }
    if !meta.pencillers.is_empty() {
        xml.push_str(&format!(
            "  <Penciller>{}</Penciller>\n",
            escape(meta.pencillers.join(", "))
        ));
    }
    if let Some(ref pub_name) = meta.publisher
        && !pub_name.trim().is_empty()
    {
        xml.push_str(&format!(
            "  <Publisher>{}</Publisher>\n",
            escape(pub_name.trim())
        ));
    }
    if !meta.genres.is_empty() {
        xml.push_str(&format!(
            "  <Genre>{}</Genre>\n",
            escape(meta.genres.join(", "))
        ));
    }
    if !meta.tags.is_empty() {
        xml.push_str(&format!(
            "  <Tags>{}</Tags>\n",
            escape(meta.tags.join(", "))
        ));
    }
    if let Some(y) = meta.year {
        xml.push_str(&format!("  <Year>{y}</Year>\n"));
    }
    if let Some(ref lang) = meta.language_iso
        && !lang.trim().is_empty()
    {
        xml.push_str(&format!(
            "  <LanguageISO>{}</LanguageISO>\n",
            escape(lang.trim())
        ));
    }
    xml.push_str(&format!("  <PageCount>{}</PageCount>\n", meta.page_count));
    let manga_tag = match meta.reading_direction.as_str() {
        "ltr" => "No",
        "webtoon" => "No",
        _ => "YesAndRightToLeft",
    };
    xml.push_str(&format!("  <Manga>{manga_tag}</Manga>\n"));
    if meta.reading_direction == "webtoon" {
        xml.push_str("  <Format>Webtoon</Format>\n");
    }
    xml.push_str("</ComicInfo>\n");
    xml
}

const GENERIC_COMIC_FOLDER_NAMES: &[&str] = &[
    "comic",
    "comics",
    "manga",
    "mangas",
    "book",
    "books",
    "video",
    "videos",
    "music",
    "sample",
    "samples",
    "download",
    "downloads",
    "inbox",
    "library",
    "media",
    "漫画",
    "图书",
    "书籍",
];

pub(crate) fn is_generic_comic_folder(name: &str) -> bool {
    let trimmed = name.trim();
    if trimmed.is_empty() {
        return true;
    }
    let lower = trimmed.to_ascii_lowercase();
    GENERIC_COMIC_FOLDER_NAMES
        .iter()
        .any(|g| lower == *g || trimmed == *g)
}

fn find_series_cover_path(comic_path: &Path) -> Option<PathBuf> {
    let parent = comic_path.parent()?;
    let dir_name = parent.file_name()?.to_str()?;
    if is_generic_comic_folder(dir_name) {
        return None;
    }
    const CANDIDATES: &[&str] = &[
        "cover.jpg",
        "cover.jpeg",
        "cover.png",
        "cover.webp",
        "folder.jpg",
        "folder.jpeg",
        "folder.png",
        "folder.webp",
    ];
    CANDIDATES
        .iter()
        .map(|name| parent.join(name))
        .find(|p| p.is_file())
}

/// Infers `(series, number, volume, optional_chapter_title)` from a comic filename stem and non-generic parent folder.
pub(crate) fn guess_comic_details(
    comic_path: &Path,
) -> (Option<String>, Option<String>, Option<i32>, Option<String>) {
    let raw_stem = comic_path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or_default()
        .trim();

    // Strip leading bracket tags such as `[Author]` or `(C103)`
    let bracket_re = Regex::new(r"^(?:\[[^\]]*\]|\([^)]*\)|\s)+").expect("valid bracket regex");
    let cleaned_stem = bracket_re.replace(raw_stem, "").trim().to_string();

    let vol_re =
        Regex::new(r"(?i)(?:第\s*0*(\d+)\s*[卷册]|(?:^|[\s_\-\.\[\(])(?:vol\.?|v)\s*0*(\d+))")
            .expect("valid vol regex");
    let ch_re = Regex::new(
        r"(?i)(?:第\s*0*(\d+(?:\.\d+)?)\s*[话話回章集]|(?:(?:^|[\s_\-\.\[\(])(?:ch(?:apter)?\.?|c)|#)\s*0*(\d+(?:\.\d+)?))",
    )
    .expect("valid ch regex");

    let mut cut_idx = cleaned_stem.len();
    let mut matched_end = 0usize;
    let mut volume: Option<i32> = None;
    let mut number: Option<String> = None;

    if let Some(m) = vol_re.captures(&cleaned_stem) {
        let full = m.get(0).expect("full match");
        cut_idx = cut_idx.min(full.start());
        matched_end = matched_end.max(full.end());
        volume = m
            .get(1)
            .or_else(|| m.get(2))
            .and_then(|g| g.as_str().parse::<i32>().ok());
    }
    if let Some(m) = ch_re.captures(&cleaned_stem) {
        let full = m.get(0).expect("full match");
        cut_idx = cut_idx.min(full.start());
        matched_end = matched_end.max(full.end());
        number = m
            .get(1)
            .or_else(|| m.get(2))
            .map(|g| g.as_str().to_string());
    }
    let after_idx = if volume.is_some() || number.is_some() {
        matched_end
    } else {
        cleaned_stem.len()
    };

    let prefix = cleaned_stem[..cut_idx]
        .trim_matches([' ', '_', '-', '.', '·', '—'])
        .replace('_', " ")
        .trim()
        .to_string();

    let is_bare_digits = !prefix.is_empty() && prefix.chars().all(|c| c.is_ascii_digit());
    if is_bare_digits && number.is_none() {
        let trimmed_zero = prefix.trim_start_matches('0');
        number = Some(if trimmed_zero.is_empty() {
            "0".to_string()
        } else {
            trimmed_zero.to_string()
        });
    }

    let series = if !prefix.is_empty() && !is_bare_digits {
        Some(prefix)
    } else {
        comic_path
            .parent()
            .and_then(|p| p.file_name())
            .map(|s| s.to_string_lossy().into_owned())
            .filter(|dir_name| !is_generic_comic_folder(dir_name))
    };

    let guessed_title = if after_idx < cleaned_stem.len() {
        let suffix = cleaned_stem[after_idx..]
            .trim_matches([' ', '_', '-', '.', '·', '—', ':'])
            .replace('_', " ")
            .trim()
            .to_string();
        (!suffix.is_empty()).then_some(suffix)
    } else {
        None
    };

    (series, number, volume, guessed_title)
}

/// Infers `(series, number, volume)` from a comic filename stem and non-generic parent folder.
#[cfg(test)]
pub(crate) fn guess_comic_from_filename(
    comic_path: &Path,
) -> (Option<String>, Option<String>, Option<i32>) {
    let (series, number, volume, _) = guess_comic_details(comic_path);
    (series, number, volume)
}

fn read_comic_meta(path: &Path) -> DomainResult<ComicFileMeta> {
    let ext = path
        .extension()
        .and_then(|s| s.to_str())
        .unwrap_or("cbz")
        .to_ascii_lowercase();
    let fallback_stem = path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("Untitled")
        .to_string();

    let file = File::open(path)
        .map_err(|e| DomainError::Storage(format!("failed opening '{}': {e}", path.display())))?;
    let mut archive = ZipArchive::new(BufReader::new(file))
        .map_err(|e| DomainError::Validation(format!("invalid comic archive: {e}")))?;

    let mut page_count = 0u32;
    let mut comic_info_bytes: Option<Vec<u8>> = None;
    let mut opf_bytes: Option<Vec<u8>> = None;

    for idx in 0..archive.len() {
        let Ok(mut entry) = archive.by_index(idx) else {
            continue;
        };
        let Ok(name_raw) = entry.name() else {
            continue;
        };
        let name = name_raw.to_string();
        let lower = name.to_ascii_lowercase();
        if is_image_entry(&name) {
            page_count += 1;
        } else if lower == "comicinfo.xml" || lower.ends_with("/comicinfo.xml") {
            let mut buf = Vec::new();
            if entry.read_to_end(&mut buf).is_ok() {
                comic_info_bytes = Some(buf);
            }
        } else if ext == "epub" && opf_bytes.is_none() && lower.ends_with(".opf") {
            let mut buf = Vec::new();
            if entry.read_to_end(&mut buf).is_ok() {
                opf_bytes = Some(buf);
            }
        }
    }

    let (guessed_series, guessed_number, guessed_volume, guessed_title) = guess_comic_details(path);
    let initial_title = guessed_title.or_else(|| {
        (guessed_series.is_none() && guessed_number.is_none() && guessed_volume.is_none())
            .then_some(fallback_stem)
    });

    let mut meta = ComicFileMeta {
        format: ext,
        series: guessed_series,
        title: initial_title,
        number: guessed_number,
        volume: guessed_volume,
        reading_direction: "rtl".to_string(),
        page_count,
        has_comic_info_xml: comic_info_bytes.is_some(),
        has_cover: page_count > 0,
        has_series_cover: find_series_cover_path(path).is_some(),
        ..Default::default()
    };

    if let Some(ref opf) = opf_bytes {
        apply_epub_opf_fallback_into_comic(opf, &mut meta);
    }
    if let Some(ref xml) = comic_info_bytes {
        parse_comic_info_into(xml, &mut meta);
    }

    Ok(meta)
}

fn rewrite_zip_with_replacements(
    source_path: &Path,
    replacements: Vec<(String, Vec<u8>)>,
    skip_predicate: impl Fn(&str) -> bool,
) -> DomainResult<()> {
    atomic_write_via_temp(source_path, |staging| {
        let src_file = File::open(source_path)
            .map_err(|e| DomainError::Storage(format!("failed reading archive: {e}")))?;
        let mut src_archive = ZipArchive::new(BufReader::new(src_file))
            .map_err(|e| DomainError::Validation(format!("invalid zip archive: {e}")))?;
        let out_file = File::create(staging)
            .map_err(|e| DomainError::Storage(format!("failed creating staging zip: {e}")))?;
        let mut writer = ZipWriter::new(BufWriter::new(out_file));

        let mut preserved_mimetype = false;
        if let Ok(mut mt_entry) = src_archive.by_name("mimetype") {
            let mut mt_bytes = Vec::new();
            if mt_entry.read_to_end(&mut mt_bytes).is_ok() {
                let stored =
                    SimpleFileOptions::default().compression_method(CompressionMethod::Stored);
                writer
                    .start_file("mimetype", stored)
                    .map_err(|e| DomainError::Storage(format!("zip mimetype start error: {e}")))?;
                writer
                    .write_all(&mt_bytes)
                    .map_err(|e| DomainError::Storage(format!("zip mimetype write error: {e}")))?;
                preserved_mimetype = true;
            }
        }

        for (rep_name, rep_data) in &replacements {
            let method = if is_image_entry(rep_name) {
                CompressionMethod::Stored
            } else {
                CompressionMethod::Deflated
            };
            let opts = SimpleFileOptions::default().compression_method(method);
            writer
                .start_file(rep_name.as_str(), opts)
                .map_err(|e| DomainError::Storage(format!("zip start_file error: {e}")))?;
            writer
                .write_all(rep_data)
                .map_err(|e| DomainError::Storage(format!("zip write error: {e}")))?;
        }

        for idx in 0..src_archive.len() {
            let entry = src_archive
                .by_index(idx)
                .map_err(|e| DomainError::Storage(format!("zip read entry error: {e}")))?;
            let Ok(name_ref) = entry.name() else {
                continue;
            };
            let name = name_ref.to_string();
            if entry.is_dir() || (preserved_mimetype && name == "mimetype") || skip_predicate(&name)
            {
                continue;
            }
            writer
                .raw_copy_file(entry)
                .map_err(|e| DomainError::Storage(format!("zip raw copy error: {e}")))?;
        }

        let buf_writer = writer
            .finish()
            .map_err(|e| DomainError::Storage(format!("zip finish error: {e}")))?;
        buf_writer
            .into_inner()
            .map_err(|e| DomainError::Storage(format!("zip flush error: {e}")))?;
        Ok(())
    })
}

#[async_trait]
impl MediaInspectorPort for CbzComicInspector {
    fn media_kind(&self) -> MediaKind {
        MediaKind::Comic
    }

    fn supports_extension(&self, ext_lower: &str) -> bool {
        COMIC_EXTENSIONS.contains(&ext_lower)
    }

    async fn summarize_file(&self, path: &Path, fallback_stem: &str) -> QuickFileSummary {
        let p = path.to_path_buf();
        let stem = fallback_stem.to_string();
        tokio::task::spawn_blocking(move || match read_comic_meta(&p) {
            Ok(m) => {
                let title = m.title.clone().unwrap_or_else(|| stem.clone());
                let sub = match (&m.series, &m.number) {
                    (Some(s), Some(n)) => format!("{s} · #{n}"),
                    (Some(s), None) => s.clone(),
                    _ => format!("{}P", m.page_count),
                };
                QuickFileSummary {
                    display_title: title,
                    display_subtitle: sub,
                    year: m.year,
                    completeness: CompletenessFlags {
                        has_title: m.title.as_ref().is_some_and(|t| !t.trim().is_empty())
                            || m.series.as_ref().is_some_and(|s| !s.trim().is_empty()),
                        has_creator: !m.writers.is_empty() || !m.pencillers.is_empty(),
                        has_cover: m.has_cover,
                        has_summary_or_lyrics: m
                            .summary
                            .as_ref()
                            .is_some_and(|s| !s.trim().is_empty()),
                        has_nfo_or_tag_block: m.has_comic_info_xml,
                    },
                }
            }
            Err(_) => QuickFileSummary {
                display_title: stem,
                display_subtitle: String::new(),
                year: None,
                completeness: CompletenessFlags::default(),
            },
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
        let meta = tokio::task::spawn_blocking(move || read_comic_meta(&p))
            .await
            .map_err(|e| DomainError::Storage(format!("comic inspect task failed: {e}")))??;
        Ok(MediaMetadataPayload::Comic(meta))
    }

    async fn write_metadata(
        &self,
        path: &Path,
        patch: &MediaMetadataPatch,
    ) -> DomainResult<MediaMetadataPayload> {
        let MediaMetadataPatch::Comic(cp) = patch else {
            return Err(DomainError::Validation(
                "expected comic metadata patch for cbz/zip file".into(),
            ));
        };
        let p = path.to_path_buf();
        let cp_cloned: ComicMetaPatch = cp.clone();

        let updated = tokio::task::spawn_blocking(move || -> DomainResult<ComicFileMeta> {
            let mut meta = read_comic_meta(&p)?;
            if let Some(series) = cp_cloned.series {
                meta.series = (!series.trim().is_empty()).then(|| series.trim().to_string());
            }
            if let Some(sg) = cp_cloned.series_group {
                meta.series_group = (!sg.trim().is_empty()).then(|| sg.trim().to_string());
            }
            if let Some(title) = cp_cloned.title {
                meta.title = (!title.trim().is_empty()).then(|| title.trim().to_string());
            }
            if let Some(num) = cp_cloned.number {
                meta.number = (!num.trim().is_empty()).then(|| num.trim().to_string());
            }
            if cp_cloned.volume.is_some() {
                meta.volume = cp_cloned.volume.filter(|&v| v >= 0);
            }
            if let Some(sum) = cp_cloned.summary {
                meta.summary = (!sum.trim().is_empty()).then(|| sum.trim().to_string());
            }
            if let Some(w) = cp_cloned.writers {
                meta.writers = w
                    .into_iter()
                    .map(|s| s.trim().to_string())
                    .filter(|s| !s.is_empty())
                    .collect();
            }
            if let Some(pen) = cp_cloned.pencillers {
                meta.pencillers = pen
                    .into_iter()
                    .map(|s| s.trim().to_string())
                    .filter(|s| !s.is_empty())
                    .collect();
            }
            if let Some(pub_name) = cp_cloned.publisher {
                meta.publisher = (!pub_name.trim().is_empty()).then(|| pub_name.trim().to_string());
            }
            if let Some(g) = cp_cloned.genres {
                meta.genres = g
                    .into_iter()
                    .map(|s| s.trim().to_string())
                    .filter(|s| !s.is_empty())
                    .collect();
            }
            if let Some(tg) = cp_cloned.tags {
                meta.tags = tg
                    .into_iter()
                    .map(|s| s.trim().to_string())
                    .filter(|s| !s.is_empty())
                    .collect();
            }
            if cp_cloned.year.is_some() {
                meta.year = cp_cloned.year.filter(|&y| y > 0);
            }
            if let Some(lang) = cp_cloned.language_iso {
                meta.language_iso = (!lang.trim().is_empty()).then(|| lang.trim().to_string());
            }
            if let Some(dir) = cp_cloned.reading_direction
                && !dir.trim().is_empty()
            {
                meta.reading_direction = dir.trim().to_ascii_lowercase();
            }

            let xml = render_comic_info_xml(&meta);
            rewrite_zip_with_replacements(
                &p,
                vec![("ComicInfo.xml".to_string(), xml.into_bytes())],
                |entry_name| {
                    entry_name.eq_ignore_ascii_case("comicinfo.xml")
                        || entry_name.to_ascii_lowercase().ends_with("/comicinfo.xml")
                },
            )?;
            read_comic_meta(&p)
        })
        .await
        .map_err(|e| DomainError::Storage(format!("comic write task failed: {e}")))??;

        Ok(MediaMetadataPayload::Comic(updated))
    }

    async fn read_artwork(
        &self,
        path: &Path,
        slot: Option<&str>,
    ) -> DomainResult<Option<BinaryPayload>> {
        let p = path.to_path_buf();
        let slot_owned = slot.map(str::to_ascii_lowercase);
        tokio::task::spawn_blocking(move || {
            if slot_owned.as_deref() == Some("series_cover")
                && let Some(cov_path) = find_series_cover_path(&p)
                && let Ok(bytes) = std::fs::read(&cov_path)
                && !bytes.is_empty()
            {
                let mime = sniff_image_mime(&bytes).to_string();
                return Ok(Some(BinaryPayload {
                    bytes,
                    mime_type: mime,
                }));
            }
            let file = File::open(&p).map_err(|e| {
                DomainError::Storage(format!("failed opening '{}': {e}", p.display()))
            })?;
            let mut archive = ZipArchive::new(BufReader::new(file))
                .map_err(|e| DomainError::Validation(format!("invalid comic archive: {e}")))?;

            let mut image_names: Vec<String> = Vec::new();
            for idx in 0..archive.len() {
                if let Ok(entry) = archive.by_index(idx)
                    && let Ok(name) = entry.name()
                    && is_image_entry(&name)
                {
                    image_names.push(name.into_owned());
                }
            }
            image_names.sort_by_key(|n| natural_sort_key(n));
            let Some(first_img) = image_names.first() else {
                return Ok(None);
            };
            let mut entry = archive
                .by_name(first_img)
                .map_err(|e| DomainError::Storage(format!("failed reading cover page: {e}")))?;
            let mut buf = Vec::new();
            entry
                .read_to_end(&mut buf)
                .map_err(|e| DomainError::Storage(format!("failed reading cover bytes: {e}")))?;
            let mime = sniff_image_mime(&buf).to_string();
            Ok(Some(BinaryPayload {
                bytes: buf,
                mime_type: mime,
            }))
        })
        .await
        .map_err(|e| DomainError::Storage(format!("comic cover read failed: {e}")))?
    }

    async fn write_artwork(
        &self,
        path: &Path,
        bytes: &[u8],
        mime_type: &str,
        slot: &str,
    ) -> DomainResult<()> {
        let p = path.to_path_buf();
        let raw = bytes.to_vec();
        let detected = if mime_type.trim().is_empty() {
            sniff_image_mime(&raw)
        } else {
            mime_type.trim()
        };
        let ext = if detected.contains("png") {
            "png"
        } else if detected.contains("webp") {
            "webp"
        } else {
            "jpg"
        };

        if slot.eq_ignore_ascii_case("series_cover") {
            let parent = p
                .parent()
                .ok_or_else(|| {
                    DomainError::Validation("comic file has no parent directory".into())
                })?
                .to_path_buf();
            return tokio::task::spawn_blocking(move || {
                write_image_replacing_opposite_ext(&parent, "cover", ext, &raw)
            })
            .await
            .map_err(|e| DomainError::Storage(format!("comic series cover write failed: {e}")))?;
        }

        let cover_entry = format!("000_cover.{ext}");

        tokio::task::spawn_blocking(move || {
            rewrite_zip_with_replacements(&p, vec![(cover_entry, raw)], |entry_name| {
                let lower = entry_name.to_ascii_lowercase();
                lower.starts_with("000_cover.")
            })
        })
        .await
        .map_err(|e| DomainError::Storage(format!("comic cover write failed: {e}")))?
    }
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::guess_comic_from_filename;

    #[test]
    fn comic_filename_inference_ignores_generic_parent_dir_and_parses_series_chapter() {
        let (series, number, volume) =
            guess_comic_from_filename(Path::new("/data/samples/comic/葬送的芙莉莲_第01话.cbz"));
        assert_eq!(series.as_deref(), Some("葬送的芙莉莲"));
        assert_eq!(number.as_deref(), Some("1"));
        assert_eq!(volume, None);

        let (series2, number2, volume2) = guess_comic_from_filename(Path::new(
            "/data/samples/comic/[九井谅子] 迷宫饭 Vol.02 Ch.14.cbz",
        ));
        assert_eq!(series2.as_deref(), Some("迷宫饭"));
        assert_eq!(volume2, Some(2));
        assert_eq!(number2.as_deref(), Some("14"));

        // Bare chapter filename inside a dedicated series directory uses that series directory
        let (series3, number3, _) =
            guess_comic_from_filename(Path::new("/data/manga/孤独摇滚/第05话.cbz"));
        assert_eq!(series3.as_deref(), Some("孤独摇滚"));
        assert_eq!(number3.as_deref(), Some("5"));

        // Bare chapter filename directly inside generic `comic` bucket does NOT set series = "comic"
        let (series4, number4, _) =
            guess_comic_from_filename(Path::new("/data/samples/comic/003.cbz"));
        assert_eq!(series4, None);
        assert_eq!(number4.as_deref(), Some("3"));
    }

    #[tokio::test]
    async fn epub_manga_opf_rendition_detection_and_ocf_mimetype_preservation() {
        use std::io::Write;
        use zip::write::SimpleFileOptions;
        use zip::{CompressionMethod, ZipArchive, ZipWriter};

        use super::CbzComicInspector;
        use crate::domain::media::{ComicMetaPatch, MediaMetadataPatch, MediaMetadataPayload};
        use crate::domain::ports::MediaInspectorPort;

        let tmp = tempfile::tempdir().unwrap();
        let epub_path = tmp.path().join("テスト条漫.epub");
        {
            let f = std::fs::File::create(&epub_path).unwrap();
            let mut zw = ZipWriter::new(f);
            let stored = SimpleFileOptions::default().compression_method(CompressionMethod::Stored);
            let deflated =
                SimpleFileOptions::default().compression_method(CompressionMethod::Deflated);
            zw.start_file("mimetype", stored).unwrap();
            zw.write_all(b"application/epub+zip").unwrap();
            zw.start_file("OEBPS/content.opf", deflated).unwrap();
            zw.write_all(
                r#"<?xml version="1.0" encoding="UTF-8"?>
<package xmlns="http://www.idpf.org/2007/opf" version="3.0">
  <metadata xmlns:dc="http://purl.org/dc/elements/1.1/">
    <dc:title>縦スクロール漫画</dc:title>
    <dc:creator>佐藤太郎</dc:creator>
    <meta property="rendition:flow">scrolled-continuous</meta>
  </metadata>
  <manifest><item id="p1" href="img/1.png" media-type="image/png"/></manifest>
  <spine><itemref idref="p1"/></spine>
</package>"#
                    .as_bytes(),
            )
            .unwrap();
            zw.start_file("OEBPS/img/1.png", stored).unwrap();
            zw.write_all(b"\x89PNG\r\n\x1a\nfake").unwrap();
            zw.finish().unwrap();
        }

        let inspector = CbzComicInspector;
        let payload = inspector.inspect_file(&epub_path).await.unwrap();
        let MediaMetadataPayload::Comic(meta) = payload else {
            panic!("expected comic payload");
        };
        assert_eq!(meta.format, "epub");
        assert_eq!(meta.reading_direction, "webtoon");
        assert_eq!(meta.writers, vec!["佐藤太郎"]);
        assert_eq!(meta.page_count, 1);

        // Writing ComicInfo.xml into EPUB manga preserves uncompressed `mimetype` as entry #0
        inspector
            .write_metadata(
                &epub_path,
                &MediaMetadataPatch::Comic(ComicMetaPatch {
                    series: Some("縦スクロール漫画".into()),
                    number: Some("1".into()),
                    ..Default::default()
                }),
            )
            .await
            .unwrap();

        let rf = std::fs::File::open(&epub_path).unwrap();
        let mut za = ZipArchive::new(rf).unwrap();
        let first = za.by_index(0).unwrap();
        assert_eq!(first.name().unwrap(), "mimetype");
        assert_eq!(first.compression(), CompressionMethod::Stored);
    }
}

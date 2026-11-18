use std::fs::File;
use std::io::{BufReader, BufWriter, Read, Write};
use std::path::Path;

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

pub struct CbzComicInspector;

const COMIC_EXTENSIONS: &[&str] = &["cbz", "zip"];

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

fn is_generic_comic_folder(name: &str) -> bool {
    let trimmed = name.trim();
    if trimmed.is_empty() {
        return true;
    }
    let lower = trimmed.to_ascii_lowercase();
    GENERIC_COMIC_FOLDER_NAMES
        .iter()
        .any(|g| lower == *g || trimmed == *g)
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

    let vol_re = Regex::new(r"(?i)(?:第\s*0*(\d+)\s*[卷册]|(?:\bvol\.?|\bv)\s*0*(\d+))")
        .expect("valid vol regex");
    let ch_re = Regex::new(
        r"(?i)(?:第\s*0*(\d+(?:\.\d+)?)\s*[话話回章集]|(?:\bch(?:apter)?\.?|\bc|#)\s*0*(\d+(?:\.\d+)?))",
    )
    .expect("valid ch regex");

    let mut cut_idx = cleaned_stem.len();
    let mut after_idx = cleaned_stem.len();
    let mut volume: Option<i32> = None;
    let mut number: Option<String> = None;

    if let Some(m) = vol_re.captures(&cleaned_stem) {
        let full = m.get(0).expect("full match");
        cut_idx = cut_idx.min(full.start());
        after_idx = full.end();
        volume = m
            .get(1)
            .or_else(|| m.get(2))
            .and_then(|g| g.as_str().parse::<i32>().ok());
    }
    if let Some(m) = ch_re.captures(&cleaned_stem) {
        let full = m.get(0).expect("full match");
        cut_idx = cut_idx.min(full.start());
        after_idx = full.end();
        number = m
            .get(1)
            .or_else(|| m.get(2))
            .map(|g| g.as_str().to_string());
    }

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

    for idx in 0..archive.len() {
        let Ok(mut entry) = archive.by_index(idx) else {
            continue;
        };
        let Ok(name_raw) = entry.name() else {
            continue;
        };
        let name = name_raw.to_string();
        if is_image_entry(&name) {
            page_count += 1;
        } else if name.eq_ignore_ascii_case("comicinfo.xml")
            || name.to_ascii_lowercase().ends_with("/comicinfo.xml")
        {
            let mut buf = Vec::new();
            if entry.read_to_end(&mut buf).is_ok() {
                comic_info_bytes = Some(buf);
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
        ..Default::default()
    };

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
            if entry.is_dir() || skip_predicate(&name) {
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
        _slot: Option<&str>,
    ) -> DomainResult<Option<BinaryPayload>> {
        let p = path.to_path_buf();
        tokio::task::spawn_blocking(move || {
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
            image_names.sort();
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
        _slot: &str,
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
}

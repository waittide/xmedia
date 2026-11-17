use std::fs::File;
use std::io::{Read, Write};
use std::path::Path;

use async_trait::async_trait;
use quick_xml::Reader;
use quick_xml::escape::escape;
use quick_xml::events::Event;
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
use crate::infra::atomic_fs::atomic_mutate_copy;

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

    if let Some(m) = manga_raw {
        if m == "yesandrighttoleft" || m == "yes" || m == "true" {
            meta.reading_direction = "rtl".to_string();
        } else if m == "no" || m == "false" {
            meta.reading_direction = "ltr".to_string();
        }
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
    xml.push_str("</ComicInfo>\n");
    xml
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
    let mut archive = ZipArchive::new(file)
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

    let mut meta = ComicFileMeta {
        format: ext,
        series: path
            .parent()
            .and_then(|p| p.file_name())
            .map(|s| s.to_string_lossy().into_owned()),
        title: Some(fallback_stem),
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
    let src_bytes = std::fs::read(source_path)
        .map_err(|e| DomainError::Storage(format!("failed reading archive: {e}")))?;

    atomic_mutate_copy(source_path, |staging| {
        let mut src_archive = ZipArchive::new(std::io::Cursor::new(&src_bytes))
            .map_err(|e| DomainError::Validation(format!("invalid zip archive: {e}")))?;
        let out_file = File::create(staging)
            .map_err(|e| DomainError::Storage(format!("failed creating staging zip: {e}")))?;
        let mut writer = ZipWriter::new(out_file);

        for (rep_name, rep_data) in &replacements {
            let opts = SimpleFileOptions::default().compression_method(CompressionMethod::Deflated);
            writer
                .start_file(rep_name.as_str(), opts)
                .map_err(|e| DomainError::Storage(format!("zip start_file error: {e}")))?;
            writer
                .write_all(rep_data)
                .map_err(|e| DomainError::Storage(format!("zip write error: {e}")))?;
        }

        for idx in 0..src_archive.len() {
            let mut entry = src_archive
                .by_index(idx)
                .map_err(|e| DomainError::Storage(format!("zip read entry error: {e}")))?;
            let Ok(name_ref) = entry.name() else {
                continue;
            };
            let name = name_ref.to_string();
            if entry.is_dir() || skip_predicate(&name) {
                continue;
            }
            let method = match entry.compression() {
                CompressionMethod::Stored => CompressionMethod::Stored,
                _ => CompressionMethod::Deflated,
            };
            let opts = SimpleFileOptions::default().compression_method(method);
            let mut buf = Vec::with_capacity(usize::try_from(entry.size()).unwrap_or(0));
            entry
                .read_to_end(&mut buf)
                .map_err(|e| DomainError::Storage(format!("zip read bytes error: {e}")))?;
            writer
                .start_file(name, opts)
                .map_err(|e| DomainError::Storage(format!("zip copy start error: {e}")))?;
            writer
                .write_all(&buf)
                .map_err(|e| DomainError::Storage(format!("zip copy write error: {e}")))?;
        }

        writer
            .finish()
            .map_err(|e| DomainError::Storage(format!("zip finish error: {e}")))?;
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
                        has_title: m.title.as_ref().is_some_and(|t| !t.trim().is_empty()),
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

    async fn read_artwork(&self, path: &Path) -> DomainResult<Option<BinaryPayload>> {
        let p = path.to_path_buf();
        tokio::task::spawn_blocking(move || {
            let file = File::open(&p).map_err(|e| {
                DomainError::Storage(format!("failed opening '{}': {e}", p.display()))
            })?;
            let mut archive = ZipArchive::new(file)
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

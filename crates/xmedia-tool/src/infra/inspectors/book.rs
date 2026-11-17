use std::fs::File;
use std::io::{Cursor, Read, Write};
use std::path::{Component, Path, PathBuf};

use async_trait::async_trait;
use quick_xml::Reader;
use quick_xml::escape::escape;
use quick_xml::events::{BytesStart, Event};
use regex::Regex;
use xmedia_core::application::ports::BinaryPayload;
use xmedia_core::domain::error::{DomainError, DomainResult};
use xmedia_core::domain::library::MediaKind;
use xmedia_core::infra::thumbnail::sniff_image_mime;
use zip::write::SimpleFileOptions;
use zip::{CompressionMethod, ZipArchive, ZipWriter};

use crate::domain::media::{
    BookFileMeta, BookMetaPatch, CompletenessFlags, MediaMetadataPatch, MediaMetadataPayload,
};
use crate::domain::ports::{MediaInspectorPort, QuickFileSummary};
use crate::infra::atomic_fs::atomic_mutate_copy;

pub struct EpubBookInspector;

const BOOK_EXTENSIONS: &[&str] = &["epub", "pdf", "txt"];

fn attr_value(e: &BytesStart<'_>, local_key: &str) -> Option<String> {
    for attr in e.attributes().flatten() {
        if attr.key.local_name().as_ref() == local_key
            && let Ok(val) = attr.normalized_value(quick_xml::XmlVersion::Implicit1_0)
        {
            return Some(val.into_owned());
        }
    }
    None
}

fn normalize_zip_rel(base_dir: &Path, href: &str) -> String {
    let clean = href.split('#').next().unwrap_or(href);
    let joined = base_dir.join(clean);
    let mut out = PathBuf::new();
    for comp in joined.components() {
        match comp {
            Component::Normal(p) => out.push(p),
            Component::ParentDir => {
                out.pop();
            }
            _ => {}
        }
    }
    out.components()
        .map(|c| c.as_os_str().to_string_lossy().into_owned())
        .collect::<Vec<_>>()
        .join("/")
}

fn find_opf_path_in_container(container_xml: &[u8]) -> Option<String> {
    let s = String::from_utf8_lossy(container_xml);
    let mut reader = Reader::from_str(&s);
    reader.config_mut().trim_text(true);
    loop {
        match reader.read_event() {
            Ok(Event::Start(ref e) | Event::Empty(ref e))
                if e.local_name().as_ref() == "rootfile" =>
            {
                if let Some(fp) = attr_value(e, "full-path") {
                    return Some(fp);
                }
            }
            Ok(Event::Eof) | Err(_) => break,
            _ => {}
        }
    }
    None
}

#[derive(Default)]
struct ParsedEpubPackage {
    opf_path: String,
    cover_entry_path: Option<String>,
    meta: BookFileMeta,
}

fn parse_epub_archive(path: &Path) -> DomainResult<ParsedEpubPackage> {
    let raw_zip = std::fs::read(path)
        .map_err(|e| DomainError::Storage(format!("failed reading '{}': {e}", path.display())))?;
    let mut archive = ZipArchive::new(Cursor::new(raw_zip.as_slice()))
        .map_err(|e| DomainError::Validation(format!("invalid epub archive: {e}")))?;

    let mut container_xml = Vec::new();
    archive
        .by_name("META-INF/container.xml")
        .map_err(|_| DomainError::Validation("missing META-INF/container.xml in epub".into()))?
        .read_to_end(&mut container_xml)
        .map_err(|e| DomainError::Validation(format!("failed reading container.xml: {e}")))?;

    let opf_path = find_opf_path_in_container(&container_xml)
        .ok_or_else(|| DomainError::Validation("rootfile missing in container.xml".into()))?;
    let mut opf_bytes = Vec::new();
    archive
        .by_name(&opf_path)
        .map_err(|_| DomainError::Validation(format!("OPF '{opf_path}' missing in epub")))?
        .read_to_end(&mut opf_bytes)
        .map_err(|e| DomainError::Validation(format!("failed reading OPF: {e}")))?;

    let opf_dir = Path::new(&opf_path).parent().unwrap_or(Path::new(""));
    let opf_str = String::from_utf8_lossy(&opf_bytes);
    let mut reader = Reader::from_str(&opf_str);
    reader.config_mut().trim_text(false);

    let fallback_title = path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("Untitled")
        .to_string();

    let mut meta = BookFileMeta {
        format: "epub".to_string(),
        title: fallback_title,
        writable: true,
        ..Default::default()
    };

    let mut in_metadata = false;
    let mut cur_tag: Option<String> = None;
    let mut text_acc = String::new();
    let mut cover_meta_id: Option<String> = None;
    let mut manifest_items: Vec<(String, String, String, String)> = Vec::new(); // (id, href, media_type, properties)

    loop {
        match reader.read_event() {
            Ok(Event::Start(ref e)) => {
                let local = e.local_name().as_ref().to_ascii_lowercase();
                if local == "metadata" {
                    in_metadata = true;
                } else if in_metadata {
                    if local == "meta"
                        && attr_value(e, "name").is_some_and(|n| n.eq_ignore_ascii_case("cover"))
                    {
                        cover_meta_id = attr_value(e, "content");
                    }
                    cur_tag = Some(local);
                    text_acc.clear();
                } else if local == "item" {
                    manifest_items.push((
                        attr_value(e, "id").unwrap_or_default(),
                        attr_value(e, "href").unwrap_or_default(),
                        attr_value(e, "media-type").unwrap_or_default(),
                        attr_value(e, "properties").unwrap_or_default(),
                    ));
                }
            }
            Ok(Event::Empty(ref e)) => {
                let local = e.local_name().as_ref().to_ascii_lowercase();
                if in_metadata
                    && local == "meta"
                    && attr_value(e, "name").is_some_and(|n| n.eq_ignore_ascii_case("cover"))
                {
                    cover_meta_id = attr_value(e, "content");
                } else if local == "item" {
                    manifest_items.push((
                        attr_value(e, "id").unwrap_or_default(),
                        attr_value(e, "href").unwrap_or_default(),
                        attr_value(e, "media-type").unwrap_or_default(),
                        attr_value(e, "properties").unwrap_or_default(),
                    ));
                }
            }
            Ok(Event::Text(t)) if in_metadata => {
                if cur_tag.is_some() {
                    text_acc.push_str(&t.xml10_content());
                }
            }
            Ok(Event::GeneralRef(r)) if in_metadata => {
                if cur_tag.is_some() {
                    super::append_xml_ref(&mut text_acc, &r);
                }
            }
            Ok(Event::CData(c)) if in_metadata => {
                if cur_tag.is_some() {
                    text_acc.push_str(c.as_ref());
                }
            }
            Ok(Event::End(ref e)) => {
                let local = e.local_name().as_ref().to_ascii_lowercase();
                if local == "metadata" {
                    in_metadata = false;
                } else if in_metadata && let Some(tag) = cur_tag.take() {
                    let trimmed = text_acc.trim();
                    if !trimmed.is_empty() {
                        match tag.as_str() {
                            "title" => meta.title = trimmed.to_string(),
                            "creator" if !meta.authors.iter().any(|a| a == trimmed) => {
                                meta.authors.push(trimmed.to_string());
                            }
                            "description" => meta.description = Some(trimmed.to_string()),
                            "publisher" => meta.publisher = Some(trimmed.to_string()),
                            "date" => meta.published_at = Some(trimmed.to_string()),
                            "language" => meta.language = Some(trimmed.to_string()),
                            "identifier" => {
                                let lower = trimmed.to_ascii_lowercase();
                                let stripped = if lower.starts_with("urn:isbn:") {
                                    &trimmed[9..]
                                } else if lower.starts_with("isbn:") {
                                    &trimmed[5..]
                                } else {
                                    trimmed
                                }
                                .trim();
                                let digits: String = stripped
                                    .chars()
                                    .filter(|c| c.is_ascii_digit() || *c == 'X' || *c == 'x')
                                    .collect();
                                if meta.isbn.is_none() && (digits.len() == 10 || digits.len() == 13)
                                {
                                    meta.isbn = Some(stripped.to_string());
                                }
                            }
                            "subject"
                                if !meta.tags.iter().any(|s| s.eq_ignore_ascii_case(trimmed)) =>
                            {
                                meta.tags.push(trimmed.to_string());
                            }
                            _ => {}
                        }
                    }
                }
                cur_tag = None;
                text_acc.clear();
            }
            Ok(Event::Eof) | Err(_) => break,
            _ => {}
        }
    }

    let cover_href = manifest_items
        .iter()
        .find(|(_, _, mt, props)| {
            mt.starts_with("image/") && props.split_whitespace().any(|p| p == "cover-image")
        })
        .or_else(|| {
            cover_meta_id.as_ref().and_then(|cid| {
                manifest_items
                    .iter()
                    .find(|(id, _, mt, _)| id == cid && mt.starts_with("image/"))
            })
        })
        .or_else(|| {
            manifest_items.iter().find(|(id, href, mt, _)| {
                mt.starts_with("image/")
                    && (id.to_ascii_lowercase().contains("cover")
                        || href.to_ascii_lowercase().contains("cover"))
            })
        })
        .map(|(_, href, _, _)| normalize_zip_rel(opf_dir, href));

    meta.has_cover = cover_href
        .as_ref()
        .is_some_and(|cp| archive.by_name(cp).is_ok());

    Ok(ParsedEpubPackage {
        opf_path,
        cover_entry_path: cover_href,
        meta,
    })
}

fn replace_opf_metadata_block(
    original_opf: &str,
    meta: &BookFileMeta,
    cover_id_hint: Option<&str>,
) -> String {
    let mut block = String::with_capacity(768);
    block.push_str("<metadata xmlns:dc=\"http://purl.org/dc/elements/1.1/\" xmlns:opf=\"http://www.idpf.org/2007/opf\">\n");
    block.push_str(&format!(
        "    <dc:title>{}</dc:title>\n",
        escape(meta.title.trim())
    ));
    for author in &meta.authors {
        let clean = author.trim();
        if !clean.is_empty() {
            block.push_str(&format!("    <dc:creator>{}</dc:creator>\n", escape(clean)));
        }
    }
    let lang = meta
        .language
        .as_deref()
        .filter(|s| !s.trim().is_empty())
        .unwrap_or("zh");
    block.push_str(&format!(
        "    <dc:language>{}</dc:language>\n",
        escape(lang)
    ));

    let book_id = meta
        .isbn
        .as_deref()
        .filter(|s| !s.trim().is_empty())
        .map(|s| format!("urn:isbn:{}", s.trim()))
        .unwrap_or_else(|| format!("urn:uuid:{}", uuid::Uuid::now_v7()));
    block.push_str(&format!(
        "    <dc:identifier id=\"bookid\">{}</dc:identifier>\n",
        escape(&book_id)
    ));

    if let Some(ref desc) = meta.description
        && !desc.trim().is_empty()
    {
        block.push_str(&format!(
            "    <dc:description>{}</dc:description>\n",
            escape(desc.trim())
        ));
    }
    if let Some(ref pub_name) = meta.publisher
        && !pub_name.trim().is_empty()
    {
        block.push_str(&format!(
            "    <dc:publisher>{}</dc:publisher>\n",
            escape(pub_name.trim())
        ));
    }
    if let Some(ref date) = meta.published_at
        && !date.trim().is_empty()
    {
        block.push_str(&format!("    <dc:date>{}</dc:date>\n", escape(date.trim())));
    }
    for subject in &meta.tags {
        let clean = subject.trim();
        if !clean.is_empty() {
            block.push_str(&format!("    <dc:subject>{}</dc:subject>\n", escape(clean)));
        }
    }
    if let Some(cid) = cover_id_hint {
        block.push_str(&format!(
            "    <meta name=\"cover\" content=\"{}\"/>\n",
            escape(cid)
        ));
    }
    block.push_str("  </metadata>");

    let re = Regex::new(r"(?is)<(?:\w+:)?metadata\b[^>]*>.*?</(?:\w+:)?metadata>")
        .expect("valid opf metadata regex");
    if re.is_match(original_opf) {
        re.replace(original_opf, block.as_str()).into_owned()
    } else {
        original_opf.to_string()
    }
}

fn rewrite_epub_entries(
    epub_path: &Path,
    replacements: Vec<(String, Vec<u8>)>,
) -> DomainResult<()> {
    let src_bytes = std::fs::read(epub_path)
        .map_err(|e| DomainError::Storage(format!("failed reading epub: {e}")))?;

    atomic_mutate_copy(epub_path, |staging| {
        let mut src_archive = ZipArchive::new(Cursor::new(src_bytes.as_slice()))
            .map_err(|e| DomainError::Validation(format!("invalid epub zip: {e}")))?;
        let out_file = File::create(staging)
            .map_err(|e| DomainError::Storage(format!("failed creating staging epub: {e}")))?;
        let mut writer = ZipWriter::new(out_file);

        // Per EPUB OCF specification, `mimetype` MUST be the first entry and stored uncompressed
        let stored = SimpleFileOptions::default().compression_method(CompressionMethod::Stored);
        writer
            .start_file("mimetype", stored)
            .map_err(|e| DomainError::Storage(format!("epub start mimetype failed: {e}")))?;
        writer
            .write_all(b"application/epub+zip")
            .map_err(|e| DomainError::Storage(format!("epub write mimetype failed: {e}")))?;

        let mut written: std::collections::HashSet<String> =
            std::collections::HashSet::from(["mimetype".to_string()]);

        for idx in 0..src_archive.len() {
            let mut entry = src_archive
                .by_index(idx)
                .map_err(|e| DomainError::Storage(format!("epub entry error: {e}")))?;
            let Ok(name_ref) = entry.name() else {
                continue;
            };
            let name = name_ref.to_string();
            if entry.is_dir() || name == "mimetype" {
                continue;
            }

            if let Some((_, rep_bytes)) = replacements.iter().find(|(rn, _)| rn == &name) {
                let opts =
                    SimpleFileOptions::default().compression_method(CompressionMethod::Deflated);
                writer
                    .start_file(&name, opts)
                    .map_err(|e| DomainError::Storage(format!("epub replace start error: {e}")))?;
                writer
                    .write_all(rep_bytes)
                    .map_err(|e| DomainError::Storage(format!("epub replace write error: {e}")))?;
                written.insert(name);
            } else {
                let method = match entry.compression() {
                    CompressionMethod::Stored => CompressionMethod::Stored,
                    _ => CompressionMethod::Deflated,
                };
                let opts = SimpleFileOptions::default().compression_method(method);
                let mut buf = Vec::with_capacity(usize::try_from(entry.size()).unwrap_or(0));
                entry
                    .read_to_end(&mut buf)
                    .map_err(|e| DomainError::Storage(format!("epub copy read error: {e}")))?;
                writer
                    .start_file(&name, opts)
                    .map_err(|e| DomainError::Storage(format!("epub copy start error: {e}")))?;
                writer
                    .write_all(&buf)
                    .map_err(|e| DomainError::Storage(format!("epub copy write error: {e}")))?;
                written.insert(name);
            }
        }

        for (rep_name, rep_bytes) in replacements {
            if !written.contains(&rep_name) {
                let opts =
                    SimpleFileOptions::default().compression_method(CompressionMethod::Deflated);
                writer
                    .start_file(rep_name, opts)
                    .map_err(|e| DomainError::Storage(format!("epub add entry error: {e}")))?;
                writer.write_all(&rep_bytes).map_err(|e| {
                    DomainError::Storage(format!("epub write new entry error: {e}"))
                })?;
            }
        }

        writer
            .finish()
            .map_err(|e| DomainError::Storage(format!("epub finish error: {e}")))?;
        Ok(())
    })
}

fn read_book_meta_any(path: &Path) -> DomainResult<BookFileMeta> {
    let ext = path
        .extension()
        .and_then(|s| s.to_str())
        .unwrap_or("epub")
        .to_ascii_lowercase();
    let fallback_title = path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("Untitled")
        .to_string();

    if ext == "epub" {
        return Ok(parse_epub_archive(path)?.meta);
    }

    let sidecar_cover =
        path.with_extension("jpg").is_file() || path.with_extension("png").is_file();
    Ok(BookFileMeta {
        format: ext,
        title: fallback_title,
        has_cover: sidecar_cover,
        writable: false,
        ..Default::default()
    })
}

#[async_trait]
impl MediaInspectorPort for EpubBookInspector {
    fn media_kind(&self) -> MediaKind {
        MediaKind::Book
    }

    fn supports_extension(&self, ext_lower: &str) -> bool {
        BOOK_EXTENSIONS.contains(&ext_lower)
    }

    async fn summarize_file(&self, path: &Path, fallback_stem: &str) -> QuickFileSummary {
        let p = path.to_path_buf();
        let stem = fallback_stem.to_string();
        tokio::task::spawn_blocking(move || match read_book_meta_any(&p) {
            Ok(m) => {
                let year = m
                    .published_at
                    .as_deref()
                    .and_then(|d| d.get(..4))
                    .and_then(|y| y.parse::<i32>().ok());
                QuickFileSummary {
                    display_title: m.title.clone(),
                    display_subtitle: m.authors.join(", "),
                    year,
                    completeness: CompletenessFlags {
                        has_title: !m.title.trim().is_empty(),
                        has_creator: !m.authors.is_empty(),
                        has_cover: m.has_cover,
                        has_summary_or_lyrics: m
                            .description
                            .as_ref()
                            .is_some_and(|d| !d.trim().is_empty()),
                        has_nfo_or_tag_block: !m.tags.is_empty() || m.isbn.is_some(),
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
        let meta = tokio::task::spawn_blocking(move || read_book_meta_any(&p))
            .await
            .map_err(|e| DomainError::Storage(format!("book inspect task failed: {e}")))??;
        Ok(MediaMetadataPayload::Book(meta))
    }

    async fn write_metadata(
        &self,
        path: &Path,
        patch: &MediaMetadataPatch,
    ) -> DomainResult<MediaMetadataPayload> {
        let MediaMetadataPatch::Book(bp) = patch else {
            return Err(DomainError::Validation(
                "expected book metadata patch".into(),
            ));
        };
        let ext = path
            .extension()
            .and_then(|s| s.to_str())
            .unwrap_or_default()
            .to_ascii_lowercase();
        if ext != "epub" {
            return Err(DomainError::UnsupportedFormat(
                "embedded book metadata writing is currently supported for EPUB files; TXT/PDF support filename organizing".into(),
            ));
        }

        let p = path.to_path_buf();
        let bp_cloned: BookMetaPatch = bp.clone();

        let updated = tokio::task::spawn_blocking(move || -> DomainResult<BookFileMeta> {
            let parsed = parse_epub_archive(&p)?;
            let mut meta = parsed.meta;

            if let Some(title) = bp_cloned.title
                && !title.trim().is_empty()
            {
                meta.title = title.trim().to_string();
            }
            if let Some(authors) = bp_cloned.authors {
                meta.authors = authors
                    .into_iter()
                    .map(|s| s.trim().to_string())
                    .filter(|s| !s.is_empty())
                    .collect();
            }
            if let Some(desc) = bp_cloned.description {
                meta.description = (!desc.trim().is_empty()).then(|| desc.trim().to_string());
            }
            if let Some(pub_name) = bp_cloned.publisher {
                meta.publisher = (!pub_name.trim().is_empty()).then(|| pub_name.trim().to_string());
            }
            if let Some(pub_at) = bp_cloned.published_at {
                meta.published_at = (!pub_at.trim().is_empty()).then(|| pub_at.trim().to_string());
            }
            if let Some(lang) = bp_cloned.language {
                meta.language = (!lang.trim().is_empty()).then(|| lang.trim().to_string());
            }
            if let Some(isbn) = bp_cloned.isbn {
                meta.isbn = (!isbn.trim().is_empty()).then(|| isbn.trim().to_string());
            }
            if let Some(tags) = bp_cloned.tags {
                meta.tags = tags
                    .into_iter()
                    .map(|s| s.trim().to_string())
                    .filter(|s| !s.is_empty())
                    .collect();
            }

            let raw_zip = std::fs::read(&p)
                .map_err(|e| DomainError::Storage(format!("failed reading epub: {e}")))?;
            let mut archive = ZipArchive::new(Cursor::new(raw_zip.as_slice()))
                .map_err(|e| DomainError::Validation(format!("invalid epub: {e}")))?;
            let mut opf_bytes = Vec::new();
            archive
                .by_name(&parsed.opf_path)
                .map_err(|_| DomainError::Validation("OPF missing".into()))?
                .read_to_end(&mut opf_bytes)
                .map_err(|e| DomainError::Storage(format!("read OPF error: {e}")))?;

            let opf_str = String::from_utf8_lossy(&opf_bytes);
            let next_opf = replace_opf_metadata_block(&opf_str, &meta, None);
            rewrite_epub_entries(&p, vec![(parsed.opf_path, next_opf.into_bytes())])?;
            Ok(parse_epub_archive(&p)?.meta)
        })
        .await
        .map_err(|e| DomainError::Storage(format!("epub write task failed: {e}")))??;

        Ok(MediaMetadataPayload::Book(updated))
    }

    async fn read_artwork(&self, path: &Path) -> DomainResult<Option<BinaryPayload>> {
        let p = path.to_path_buf();
        tokio::task::spawn_blocking(move || {
            let ext = p
                .extension()
                .and_then(|s| s.to_str())
                .unwrap_or_default()
                .to_ascii_lowercase();
            if ext == "epub" {
                let parsed = parse_epub_archive(&p)?;
                let Some(cover_entry) = parsed.cover_entry_path else {
                    return Ok(None);
                };
                let raw_zip = std::fs::read(&p)
                    .map_err(|e| DomainError::Storage(format!("failed reading epub: {e}")))?;
                let mut archive = ZipArchive::new(Cursor::new(raw_zip.as_slice()))
                    .map_err(|e| DomainError::Validation(format!("invalid epub: {e}")))?;
                let mut entry = archive
                    .by_name(&cover_entry)
                    .map_err(|e| DomainError::Storage(format!("cover missing: {e}")))?;
                let mut buf = Vec::new();
                entry
                    .read_to_end(&mut buf)
                    .map_err(|e| DomainError::Storage(format!("cover read error: {e}")))?;
                let mime = sniff_image_mime(&buf).to_string();
                return Ok(Some(BinaryPayload {
                    bytes: buf,
                    mime_type: mime,
                }));
            }

            for ext_cand in ["jpg", "png"] {
                let sidecar = p.with_extension(ext_cand);
                if sidecar.is_file() {
                    let buf = std::fs::read(&sidecar)
                        .map_err(|e| DomainError::Storage(format!("sidecar read error: {e}")))?;
                    let mime = sniff_image_mime(&buf).to_string();
                    return Ok(Some(BinaryPayload {
                        bytes: buf,
                        mime_type: mime,
                    }));
                }
            }
            Ok(None)
        })
        .await
        .map_err(|e| DomainError::Storage(format!("book cover read failed: {e}")))?
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
        }
        .to_string();

        tokio::task::spawn_blocking(move || {
            let ext = p
                .extension()
                .and_then(|s| s.to_str())
                .unwrap_or_default()
                .to_ascii_lowercase();
            if ext != "epub" {
                let img_ext = if detected.contains("png") {
                    "png"
                } else {
                    "jpg"
                };
                let sidecar = p.with_extension(img_ext);
                return crate::infra::atomic_fs::atomic_write_bytes(&sidecar, &raw);
            }

            let parsed = parse_epub_archive(&p)?;
            if let Some(existing_cover) = parsed.cover_entry_path {
                return rewrite_epub_entries(&p, vec![(existing_cover, raw)]);
            }

            // Inject a new cover image entry + manifest item into OPF
            let img_ext = if detected.contains("png") {
                "png"
            } else {
                "jpg"
            };
            let opf_dir = Path::new(&parsed.opf_path).parent().unwrap_or(Path::new(""));
            let rel_href = format!("xmedia-cover.{img_ext}");
            let zip_cover_path = normalize_zip_rel(opf_dir, &rel_href);

            let raw_zip = std::fs::read(&p)
                .map_err(|e| DomainError::Storage(format!("failed reading epub: {e}")))?;
            let mut archive = ZipArchive::new(Cursor::new(raw_zip.as_slice()))
                .map_err(|e| DomainError::Validation(format!("invalid epub: {e}")))?;
            let mut opf_bytes = Vec::new();
            archive
                .by_name(&parsed.opf_path)
                .map_err(|_| DomainError::Validation("OPF missing".into()))?
                .read_to_end(&mut opf_bytes)
                .map_err(|e| DomainError::Storage(format!("read OPF error: {e}")))?;

            let mut opf_str = String::from_utf8_lossy(&opf_bytes).into_owned();
            let item_tag = format!(
                "<item id=\"xmedia-cover-img\" href=\"{}\" media-type=\"{}\" properties=\"cover-image\"/>\n  </manifest>",
                escape(&rel_href),
                escape(&detected)
            );
            opf_str = opf_str.replacen("</manifest>", &item_tag, 1);
            opf_str = replace_opf_metadata_block(&opf_str, &parsed.meta, Some("xmedia-cover-img"));

            rewrite_epub_entries(
                &p,
                vec![
                    (zip_cover_path, raw),
                    (parsed.opf_path, opf_str.into_bytes()),
                ],
            )
        })
        .await
        .map_err(|e| DomainError::Storage(format!("book cover write failed: {e}")))?
    }
}

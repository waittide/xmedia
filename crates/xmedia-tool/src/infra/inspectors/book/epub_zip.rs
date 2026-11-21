use std::collections::HashSet;
use std::fs::File;
use std::io::{BufReader, BufWriter, Read, Write};
use std::path::{Component, Path, PathBuf};

use quick_xml::Reader;
use quick_xml::events::{BytesStart, Event};
use xmedia_core::domain::error::{DomainError, DomainResult};
use zip::write::SimpleFileOptions;
use zip::{CompressionMethod, ZipArchive, ZipWriter};

use super::opf::{ParsedOpfData, parse_opf_bytes};
use crate::domain::media::BookFileMeta;
use crate::infra::atomic_fs::atomic_write_via_temp;

pub(crate) fn attr_value(e: &BytesStart<'_>, local_key: &str) -> Option<String> {
    for attr in e.attributes().flatten() {
        if attr.key.local_name().as_ref() == local_key
            && let Ok(val) = attr.normalized_value(quick_xml::XmlVersion::Implicit1_0)
        {
            return Some(val.into_owned());
        }
    }
    None
}

pub(crate) fn normalize_zip_rel(base_dir: &Path, href: &str) -> String {
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

pub(crate) fn find_opf_path_in_container(container_xml: &[u8]) -> Option<String> {
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
pub(super) struct ParsedEpubPackage {
    pub(super) opf_path: String,
    pub(super) package_version: String,
    pub(super) unique_identifier_id: String,
    pub(super) cover_entry_path: Option<String>,
    pub(super) cover_item_id: Option<String>,
    pub(super) cover_media_type: Option<String>,
    pub(super) cover_raw_href: Option<String>,
    pub(super) meta: BookFileMeta,
}

pub(super) fn parse_epub_archive(path: &Path) -> DomainResult<ParsedEpubPackage> {
    let file = File::open(path)
        .map_err(|e| DomainError::Storage(format!("failed opening '{}': {e}", path.display())))?;
    let mut archive = ZipArchive::new(BufReader::new(file))
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
    let fallback_title = path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("Untitled");

    let ParsedOpfData {
        mut meta,
        package_version,
        unique_identifier_id,
        cover_meta_id,
        manifest_items,
    } = parse_opf_bytes(&opf_bytes, "epub", fallback_title);

    let cover_item = manifest_items
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
        });

    let cover_item_id = cover_item.map(|(id, _, _, _)| id.clone());
    let cover_raw_href = cover_item.map(|(_, href, _, _)| href.clone());
    let cover_media_type = cover_item.map(|(_, _, mt, _)| mt.clone());
    let cover_href = cover_raw_href
        .as_deref()
        .map(|href| normalize_zip_rel(opf_dir, href));

    meta.has_cover = cover_href
        .as_ref()
        .is_some_and(|cp| archive.by_name(cp).is_ok());

    Ok(ParsedEpubPackage {
        opf_path,
        package_version,
        unique_identifier_id,
        cover_entry_path: cover_href,
        cover_item_id,
        cover_media_type,
        cover_raw_href,
        meta,
    })
}

pub(crate) fn rewrite_epub_entries(
    epub_path: &Path,
    replacements: Vec<(String, Vec<u8>)>,
    skip_paths: &[String],
) -> DomainResult<()> {
    atomic_write_via_temp(epub_path, |staging| {
        let src_file = File::open(epub_path)
            .map_err(|e| DomainError::Storage(format!("failed opening epub: {e}")))?;
        let mut src_archive = ZipArchive::new(BufReader::new(src_file))
            .map_err(|e| DomainError::Validation(format!("invalid epub zip: {e}")))?;
        let out_file = File::create(staging)
            .map_err(|e| DomainError::Storage(format!("failed creating staging epub: {e}")))?;
        let mut writer = ZipWriter::new(BufWriter::new(out_file));

        // Per EPUB OCF specification, `mimetype` MUST be the first entry and stored uncompressed
        let stored = SimpleFileOptions::default().compression_method(CompressionMethod::Stored);
        writer
            .start_file("mimetype", stored)
            .map_err(|e| DomainError::Storage(format!("epub start mimetype failed: {e}")))?;
        writer
            .write_all(b"application/epub+zip")
            .map_err(|e| DomainError::Storage(format!("epub write mimetype failed: {e}")))?;

        let mut written: HashSet<String> = HashSet::from(["mimetype".to_string()]);

        for idx in 0..src_archive.len() {
            let entry = src_archive
                .by_index(idx)
                .map_err(|e| DomainError::Storage(format!("epub entry error: {e}")))?;
            let Ok(name_ref) = entry.name() else {
                continue;
            };
            let name = name_ref.to_string();
            if entry.is_dir() || name == "mimetype" || skip_paths.iter().any(|s| s == &name) {
                continue;
            }

            if let Some((_, rep_bytes)) = replacements.iter().find(|(rn, _)| rn == &name) {
                drop(entry);
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
                writer
                    .raw_copy_file(entry)
                    .map_err(|e| DomainError::Storage(format!("epub raw copy error: {e}")))?;
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

        let buf_writer = writer
            .finish()
            .map_err(|e| DomainError::Storage(format!("epub finish error: {e}")))?;
        buf_writer
            .into_inner()
            .map_err(|e| DomainError::Storage(format!("epub flush error: {e}")))?;
        Ok(())
    })
}

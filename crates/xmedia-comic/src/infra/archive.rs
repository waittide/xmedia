use std::collections::{HashMap, HashSet};
use std::fs::{self, File};
use std::io::{Cursor, Read, Seek};
use std::path::{Path, PathBuf};
use std::sync::RwLock;
use std::time::UNIX_EPOCH;

use async_trait::async_trait;
use quick_xml::events::Event;
use quick_xml::reader::Reader;
use xmedia_core::application::ports::{ByteRangeSpec, ByteRangeStream};
use xmedia_core::domain::error::{DomainError, DomainResult};
use xmedia_core::infra::stream::open_file_range_stream_spec;
use zip::ZipArchive;

use crate::application::ports::{
    ArchiveInspection, ArchiveReaderPort, ComicInfoMetadata, PageContent, PageDescriptor,
};
use crate::domain::chapter::ChapterSourceKind;
use crate::domain::natural::natural_cmp;
use crate::infra::comicinfo::{append_xml_ref, parse_comic_info_xml};

#[derive(Debug, Clone)]
struct CachedPageIndex {
    fingerprint: String,
    entries: Vec<PageEntryRef>,
}

#[derive(Debug, Clone)]
struct PageEntryRef {
    entry_name: String,
    mime_type: String,
}

pub struct PureRustArchiveReader {
    index_cache: RwLock<HashMap<String, CachedPageIndex>>,
}

impl Default for PureRustArchiveReader {
    fn default() -> Self {
        Self::new()
    }
}

impl PureRustArchiveReader {
    pub fn new() -> Self {
        Self {
            index_cache: RwLock::new(HashMap::new()),
        }
    }

    fn cache_entries(&self, source_path: &str, fingerprint: &str, entries: Vec<PageEntryRef>) {
        if let Ok(mut guard) = self.index_cache.write() {
            if guard.len() > 2048 {
                guard.clear();
            }
            guard.insert(
                source_path.to_owned(),
                CachedPageIndex {
                    fingerprint: fingerprint.to_owned(),
                    entries,
                },
            );
        }
    }

    fn get_cached_entry(
        &self,
        source_path: &str,
        fingerprint: &str,
        page_number: u32,
    ) -> Option<PageEntryRef> {
        let guard = self.index_cache.read().ok()?;
        let cached = guard.get(source_path)?;
        if !fingerprint.is_empty() && cached.fingerprint != fingerprint {
            return None;
        }
        let idx = page_number.checked_sub(1)? as usize;
        cached.entries.get(idx).cloned()
    }
}

#[async_trait]
impl ArchiveReaderPort for PureRustArchiveReader {
    async fn inspect_source(
        &self,
        source_kind: ChapterSourceKind,
        source_path: &str,
    ) -> DomainResult<ArchiveInspection> {
        let path = source_path.to_owned();
        let reader_cache_entries: DomainResult<(ArchiveInspection, Vec<PageEntryRef>)> =
            tokio::task::spawn_blocking(move || match source_kind {
                ChapterSourceKind::ArchiveFile => inspect_archive_file(Path::new(&path)),
                ChapterSourceKind::ImageDirectory => inspect_image_directory(Path::new(&path)),
            })
            .await
            .map_err(|e| DomainError::Storage(format!("archive task join failed: {e}")))?;

        let (inspection, entry_refs) = reader_cache_entries?;
        self.cache_entries(source_path, &inspection.file_hash, entry_refs);
        Ok(inspection)
    }

    async fn inspect_bytes(&self, filename: &str, bytes: &[u8]) -> DomainResult<ArchiveInspection> {
        let is_epub = filename.to_ascii_lowercase().ends_with(".epub");
        let owned = bytes.to_vec();
        tokio::task::spawn_blocking(move || {
            let file_size = owned.len() as u64;
            let file_hash = blake3::hash(&owned).to_hex().to_string();
            let cursor = Cursor::new(owned);
            let (pages, _, comic_info) = inspect_zip_reader(cursor, is_epub)?;
            Ok(ArchiveInspection {
                page_count: pages.len() as u32,
                pages,
                comic_info,
                file_size,
                file_hash,
            })
        })
        .await
        .map_err(|e| DomainError::Storage(format!("archive inspect bytes join failed: {e}")))?
    }

    async fn read_page(
        &self,
        source_kind: ChapterSourceKind,
        source_path: &str,
        page_number: u32,
        file_hash_hint: &str,
    ) -> DomainResult<PageContent> {
        if page_number == 0 {
            return Err(DomainError::Validation("page_number must be >= 1".into()));
        }

        let entry_ref =
            if let Some(hit) = self.get_cached_entry(source_path, file_hash_hint, page_number) {
                hit
            } else {
                let inspection = self.inspect_source(source_kind, source_path).await?;
                let idx = (page_number - 1) as usize;
                let desc = inspection.pages.get(idx).ok_or_else(|| {
                    DomainError::NotFound(format!("page {page_number} does not exist"))
                })?;
                PageEntryRef {
                    entry_name: desc.entry_name.clone(),
                    mime_type: desc.mime_type.clone(),
                }
            };

        let path_owned = source_path.to_owned();
        let entry_name = entry_ref.entry_name;
        let mime_type = entry_ref.mime_type;
        let hash_part = if file_hash_hint.is_empty() {
            blake3::hash(source_path.as_bytes()).to_hex().to_string()
        } else {
            file_hash_hint.to_owned()
        };

        tokio::task::spawn_blocking(move || {
            const MAX_COMIC_PAGE_BYTES: u64 = 64 * 1024 * 1024;
            let bytes = match source_kind {
                ChapterSourceKind::ArchiveFile => {
                    let file = File::open(&path_owned).map_err(|e| {
                        if e.kind() == std::io::ErrorKind::NotFound {
                            DomainError::NotFound(format!("archive '{path_owned}' not found"))
                        } else {
                            DomainError::Storage(format!(
                                "failed to open archive '{path_owned}': {e}"
                            ))
                        }
                    })?;
                    let mut zip = ZipArchive::new(file).map_err(|e| {
                        DomainError::Storage(format!("invalid zip archive '{path_owned}': {e}"))
                    })?;
                    let entry = zip.by_name(&entry_name).map_err(|e| {
                        DomainError::NotFound(format!(
                            "page entry '{entry_name}' not found in '{path_owned}': {e}"
                        ))
                    })?;
                    let prealloc = entry.size().min(1024 * 1024) as usize;
                    let mut buf = Vec::with_capacity(prealloc);
                    entry
                        .take(MAX_COMIC_PAGE_BYTES + 1)
                        .read_to_end(&mut buf)
                        .map_err(|e| {
                            DomainError::Storage(format!("failed reading page bytes: {e}"))
                        })?;
                    if (buf.len() as u64) > MAX_COMIC_PAGE_BYTES {
                        return Err(DomainError::Validation(format!(
                            "comic page '{entry_name}' exceeds {} MiB safety limit",
                            MAX_COMIC_PAGE_BYTES / (1024 * 1024)
                        )));
                    }
                    buf
                }
                ChapterSourceKind::ImageDirectory => {
                    let base = Path::new(&path_owned);
                    let full_path = base.join(&entry_name);
                    let sym_meta = fs::symlink_metadata(&full_path).map_err(|e| {
                        if e.kind() == std::io::ErrorKind::NotFound {
                            DomainError::NotFound(format!("page image '{entry_name}' not found"))
                        } else {
                            DomainError::Storage(format!(
                                "failed stating image '{}': {e}",
                                full_path.display()
                            ))
                        }
                    })?;
                    if sym_meta.file_type().is_symlink() || !sym_meta.is_file() {
                        return Err(DomainError::Forbidden(
                            "symlink or non-regular comic page file rejected".into(),
                        ));
                    }
                    if sym_meta.len() > MAX_COMIC_PAGE_BYTES {
                        return Err(DomainError::Validation(format!(
                            "comic page '{entry_name}' exceeds {} MiB safety limit",
                            MAX_COMIC_PAGE_BYTES / (1024 * 1024)
                        )));
                    }
                    let canon_base = fs::canonicalize(base).map_err(|e| {
                        DomainError::Storage(format!("failed resolving comic dir: {e}"))
                    })?;
                    let canon_file = fs::canonicalize(&full_path).map_err(|e| {
                        DomainError::Storage(format!("failed resolving comic page: {e}"))
                    })?;
                    if !canon_file.starts_with(&canon_base) {
                        return Err(DomainError::Forbidden(
                            "comic page path escapes chapter directory".into(),
                        ));
                    }
                    fs::read(&canon_file).map_err(|e| {
                        DomainError::Storage(format!(
                            "failed reading image '{}': {e}",
                            full_path.display()
                        ))
                    })?
                }
            };

            let short_hash = &hash_part[..hash_part.len().min(20)];
            let etag = format!("\"p-{short_hash}-{page_number}\"");
            Ok(PageContent {
                bytes,
                mime_type,
                etag,
            })
        })
        .await
        .map_err(|e| DomainError::Storage(format!("read_page join error: {e}")))?
    }

    async fn read_raw_archive(&self, source_path: &str) -> DomainResult<(Vec<u8>, String, String)> {
        let path_buf = PathBuf::from(source_path);
        tokio::task::spawn_blocking(move || {
            let bytes = fs::read(&path_buf).map_err(|e| {
                DomainError::Storage(format!(
                    "failed to read archive '{}': {e}",
                    path_buf.display()
                ))
            })?;
            let filename = path_buf
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_else(|| "chapter.cbz".into());
            let mime = if filename.to_ascii_lowercase().ends_with(".epub") {
                "application/epub+zip".to_owned()
            } else {
                "application/vnd.comicbook+zip".to_owned()
            };
            Ok((bytes, filename, mime))
        })
        .await
        .map_err(|e| DomainError::Storage(format!("read_raw_archive join error: {e}")))?
    }

    async fn open_raw_archive_stream(
        &self,
        source_path: &str,
        spec: ByteRangeSpec,
    ) -> DomainResult<(ByteRangeStream, String, String)> {
        let path = Path::new(source_path);
        let stream = open_file_range_stream_spec(path, spec).await?;
        let filename = path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| "chapter.cbz".into());
        let mime = if filename.to_ascii_lowercase().ends_with(".epub") {
            "application/epub+zip".to_owned()
        } else {
            "application/vnd.comicbook+zip".to_owned()
        };
        Ok((stream, filename, mime))
    }
}

pub fn is_supported_archive_path(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .is_some_and(|ext| matches!(ext.to_ascii_lowercase().as_str(), "cbz" | "zip" | "epub"))
}

pub fn is_supported_image_name(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    if lower.contains("__macosx/") || lower.ends_with(".ds_store") {
        return false;
    }
    let filename = lower.rsplit('/').next().unwrap_or(&lower);
    if filename.starts_with('.') {
        return false;
    }
    Path::new(filename)
        .extension()
        .and_then(|e| e.to_str())
        .is_some_and(|ext| {
            matches!(
                ext,
                "jpg" | "jpeg" | "png" | "webp" | "gif" | "bmp" | "avif"
            )
        })
}

pub fn guess_image_mime(name: &str) -> String {
    let ext = Path::new(name)
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    match ext.as_str() {
        "jpg" | "jpeg" => "image/jpeg".into(),
        "png" => "image/png".into(),
        "webp" => "image/webp".into(),
        "gif" => "image/gif".into(),
        "bmp" => "image/bmp".into(),
        "avif" => "image/avif".into(),
        _ => "application/octet-stream".into(),
    }
}

pub fn fast_file_fingerprint(path: &Path) -> DomainResult<(u64, String)> {
    xmedia_core::infra::fingerprint::fast_content_fingerprint(path)
}

fn inspect_archive_file(path: &Path) -> DomainResult<(ArchiveInspection, Vec<PageEntryRef>)> {
    let (file_size, file_hash) = fast_file_fingerprint(path)?;
    let is_epub = path
        .extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| e.eq_ignore_ascii_case("epub"));
    let file = File::open(path).map_err(|e| {
        DomainError::Storage(format!("failed to open archive '{}': {e}", path.display()))
    })?;
    let (pages, refs, comic_info) = inspect_zip_reader(file, is_epub)?;
    Ok((
        ArchiveInspection {
            page_count: pages.len() as u32,
            pages,
            comic_info,
            file_size,
            file_hash,
        },
        refs,
    ))
}

#[allow(clippy::type_complexity)]
fn inspect_zip_reader<R: Read + Seek>(
    reader: R,
    is_epub: bool,
) -> DomainResult<(
    Vec<PageDescriptor>,
    Vec<PageEntryRef>,
    Option<ComicInfoMetadata>,
)> {
    let mut archive = ZipArchive::new(reader)
        .map_err(|e| DomainError::Validation(format!("invalid zip/cbz/epub archive: {e}")))?;

    let mut image_names: Vec<String> = Vec::new();
    let mut comic_info_entry: Option<String> = None;
    let mut opf_candidates: Vec<String> = Vec::new();

    for i in 0..archive.len() {
        let Ok(entry) = archive.by_index_raw(i) else {
            continue;
        };
        if entry.is_dir() {
            continue;
        }
        let Ok(name_cow) = entry.name() else {
            continue;
        };
        let name = name_cow.into_owned();
        let lower = name.to_ascii_lowercase();
        let base = lower.rsplit('/').next().unwrap_or(&lower);
        if base == "comicinfo.xml" {
            comic_info_entry = Some(name);
            continue;
        }
        if is_epub && lower.ends_with(".opf") {
            opf_candidates.push(name);
            continue;
        }
        if is_supported_image_name(&name) {
            image_names.push(name);
        }
    }

    let mut comic_info = if let Some(ci_name) = comic_info_entry {
        if let Ok(mut entry) = archive.by_name(&ci_name) {
            let mut xml = Vec::new();
            let _ = entry.read_to_end(&mut xml);
            parse_comic_info_xml(&xml)
        } else {
            None
        }
    } else {
        None
    };

    if is_epub
        && let Some(opf_path) = opf_candidates.first()
        && let Ok(mut opf_entry) = archive.by_name(opf_path)
    {
        let mut opf_bytes = Vec::new();
        if opf_entry.read_to_end(&mut opf_bytes).is_ok() {
            let (opf_meta, ordered_imgs) = parse_epub_opf(&opf_bytes, opf_path);
            if comic_info.is_none() && opf_meta.is_some() {
                comic_info = opf_meta;
            }
            if !ordered_imgs.is_empty() {
                let available: HashSet<String> = image_names.iter().cloned().collect();
                let filtered: Vec<String> = ordered_imgs
                    .into_iter()
                    .filter(|p| available.contains(p))
                    .collect();
                if !filtered.is_empty() {
                    image_names = filtered;
                } else {
                    image_names.sort_by(|a, b| natural_cmp(a, b));
                }
            } else {
                image_names.sort_by(|a, b| natural_cmp(a, b));
            }
        }
    } else {
        image_names.sort_by(|a, b| natural_cmp(a, b));
    }

    let mut pages = Vec::with_capacity(image_names.len());
    let mut refs = Vec::with_capacity(image_names.len());
    for (idx, entry_name) in image_names.into_iter().enumerate() {
        let mime_type = guess_image_mime(&entry_name);
        pages.push(PageDescriptor {
            page_number: (idx + 1) as u32,
            entry_name: entry_name.clone(),
            mime_type: mime_type.clone(),
        });
        refs.push(PageEntryRef {
            entry_name,
            mime_type,
        });
    }

    Ok((pages, refs, comic_info))
}

fn inspect_image_directory(dir: &Path) -> DomainResult<(ArchiveInspection, Vec<PageEntryRef>)> {
    let entries = fs::read_dir(dir).map_err(|e| {
        DomainError::Storage(format!("cannot read directory '{}': {e}", dir.display()))
    })?;

    let mut files: Vec<(String, u64, u128)> = Vec::new();
    let mut comic_info: Option<ComicInfoMetadata> = None;

    for item in entries.flatten() {
        let path = item.path();
        let Ok(ft) = item.file_type() else {
            continue;
        };
        if !ft.is_file() {
            continue;
        }
        let name = item.file_name().to_string_lossy().into_owned();
        if name.eq_ignore_ascii_case("ComicInfo.xml") {
            if let Ok(bytes) = fs::read(&path) {
                comic_info = parse_comic_info_xml(&bytes);
            }
            continue;
        }
        if is_supported_image_name(&name)
            && let Ok(meta) = item.metadata()
        {
            let mtime = meta
                .modified()
                .ok()
                .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
                .map(|d| d.as_nanos())
                .unwrap_or(0);
            files.push((name, meta.len(), mtime));
        }
    }

    files.sort_by(|a, b| natural_cmp(&a.0, &b.0));

    let mut total_size = 0_u64;
    let mut hasher = blake3::Hasher::new();
    let mut pages = Vec::with_capacity(files.len());
    let mut refs = Vec::with_capacity(files.len());

    for (idx, (name, size, mtime)) in files.into_iter().enumerate() {
        total_size += size;
        hasher.update(name.as_bytes());
        hasher.update(&size.to_le_bytes());
        hasher.update(&mtime.to_le_bytes());

        let mime_type = guess_image_mime(&name);
        pages.push(PageDescriptor {
            page_number: (idx + 1) as u32,
            entry_name: name.clone(),
            mime_type: mime_type.clone(),
        });
        refs.push(PageEntryRef {
            entry_name: name,
            mime_type,
        });
    }

    let file_hash = hasher.finalize().to_hex().to_string();
    Ok((
        ArchiveInspection {
            page_count: pages.len() as u32,
            pages,
            comic_info,
            file_size: total_size,
            file_hash,
        },
        refs,
    ))
}

fn parse_epub_opf(opf_bytes: &[u8], opf_path: &str) -> (Option<ComicInfoMetadata>, Vec<String>) {
    let base_dir = opf_path
        .rsplit_once('/')
        .map(|(dir, _)| format!("{dir}/"))
        .unwrap_or_default();

    let mut reader = Reader::from_reader(opf_bytes);
    reader.config_mut().trim_text(false);
    let mut buf = Vec::new();

    let mut meta = ComicInfoMetadata::default();
    let mut current_tag: Option<String> = None;
    let mut text_acc = String::new();
    let mut images: Vec<String> = Vec::new();
    let mut found_meta = false;

    loop {
        match reader.read_event_into(&mut buf) {
            Ok(Event::Start(e)) => {
                let local = e.local_name().as_ref().to_ascii_lowercase();
                if local == "item" {
                    collect_opf_image_item(&e, &base_dir, &mut images);
                }
                current_tag = Some(local);
                text_acc.clear();
            }
            Ok(Event::Empty(e)) => {
                let local = e.local_name().as_ref().to_ascii_lowercase();
                if local == "item" {
                    collect_opf_image_item(&e, &base_dir, &mut images);
                }
            }
            Ok(Event::End(_)) => {
                if let Some(tag) = current_tag.take() {
                    let trimmed = text_acc.trim();
                    if !trimmed.is_empty() {
                        match tag.as_str() {
                            "title" => {
                                meta.title = Some(trimmed.to_owned());
                                found_meta = true;
                            }
                            "creator" => {
                                meta.writers.push(trimmed.to_owned());
                                found_meta = true;
                            }
                            "description" => {
                                meta.summary = Some(trimmed.to_owned());
                                found_meta = true;
                            }
                            "publisher" => {
                                meta.publisher = Some(trimmed.to_owned());
                                found_meta = true;
                            }
                            "language" => {
                                meta.language_iso = Some(trimmed.to_owned());
                                found_meta = true;
                            }
                            _ => {}
                        }
                    }
                }
                text_acc.clear();
            }
            Ok(Event::Text(t)) => {
                if current_tag.is_some() {
                    text_acc.push_str(&t.xml10_content());
                }
            }
            Ok(Event::GeneralRef(r)) => {
                if current_tag.is_some() {
                    append_xml_ref(&mut text_acc, &r);
                }
            }
            Ok(Event::CData(c)) => {
                if current_tag.is_some() {
                    text_acc.push_str(c.as_ref());
                }
            }
            Ok(Event::Eof) | Err(_) => break,
            _ => {}
        }
        buf.clear();
    }

    images.sort_by(|a, b| natural_cmp(a, b));
    images.dedup();
    (if found_meta { Some(meta) } else { None }, images)
}

fn collect_opf_image_item(
    elem: &quick_xml::events::BytesStart<'_>,
    base_dir: &str,
    images: &mut Vec<String>,
) {
    let mut href: Option<String> = None;
    let mut media_type: Option<String> = None;
    for attr in elem.attributes().flatten() {
        let key = attr.key.local_name().as_ref().to_ascii_lowercase();
        let val = attr.value.into_owned();
        if key == "href" {
            href = Some(val);
        } else if key == "media-type" {
            media_type = Some(val);
        }
    }
    if let Some(h) = href {
        let is_img = media_type
            .as_deref()
            .is_some_and(|m| m.starts_with("image/"))
            || is_supported_image_name(&h);
        if is_img {
            let full = format!("{base_dir}{}", h.trim_start_matches("./"));
            images.push(full);
        }
    }
}

use std::collections::HashMap;
use std::fs::{self, File};
use std::io::{Cursor, Read};
use std::path::Path;
use std::sync::RwLock;
use std::time::UNIX_EPOCH;

use async_trait::async_trait;
use xmedia_core::application::ports::{ByteRangeSpec, ByteRangeStream};
use xmedia_core::domain::error::{DomainError, DomainResult};
use xmedia_core::infra::stream::open_file_range_stream_spec;
use xmedia_formats::FormatError;
use xmedia_formats::comic::{
    ComicFormatExt, ProbedComicPage, inspect_comic_path, inspect_comic_reader,
};
use zip::ZipArchive;

use crate::application::ports::{
    ArchiveInspection, ArchiveReaderPort, ComicInfoMetadata, PageContent, PageDescriptor,
};
use crate::domain::chapter::ChapterSourceKind;

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

fn ensure_zip_based_format(path: &Path) -> DomainResult<Option<ComicFormatExt>> {
    let fmt = ComicFormatExt::from_path(path);
    if let Some(ext) = fmt
        && !ext.is_zip_based()
    {
        return Err(DomainError::UnsupportedFormat(format!(
            ".{} comic archives require an external extractor and are not supported",
            ext.as_str()
        )));
    }
    Ok(fmt)
}

fn map_format_error(err: FormatError) -> DomainError {
    match err {
        FormatError::UnsupportedFormat(msg) => DomainError::UnsupportedFormat(msg),
        FormatError::Validation(msg) => DomainError::Validation(msg),
        FormatError::NotFound(msg) => DomainError::NotFound(msg),
        FormatError::Conflict(msg) => DomainError::Conflict(msg),
        FormatError::Io(msg) => DomainError::Storage(msg),
    }
}

fn probed_to_descriptors(pages: Vec<ProbedComicPage>) -> (Vec<PageDescriptor>, Vec<PageEntryRef>) {
    let mut descriptors = Vec::with_capacity(pages.len());
    let mut refs = Vec::with_capacity(pages.len());
    for page in pages {
        let mime_type = guess_image_mime(&page.entry_name);
        descriptors.push(PageDescriptor {
            page_number: (page.index + 1) as u32,
            entry_name: page.entry_name.clone(),
            mime_type: mime_type.clone(),
            file_size: page.file_size,
            width: page.width,
            height: page.height,
            page_type: page.page_type,
            double_page: page.double_page,
        });
        refs.push(PageEntryRef {
            entry_name: page.entry_name,
            mime_type,
        });
    }
    (descriptors, refs)
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
                ChapterSourceKind::OnlineTracked => Err(DomainError::NotFound(
                    "online tracked chapter is not cached yet".into(),
                )),
            })
            .await
            .map_err(|e| DomainError::Storage(format!("archive task join failed: {e}")))?;

        let (inspection, entry_refs) = reader_cache_entries?;
        self.cache_entries(source_path, &inspection.file_hash, entry_refs);
        Ok(inspection)
    }

    async fn inspect_bytes(&self, filename: &str, bytes: &[u8]) -> DomainResult<ArchiveInspection> {
        let fmt = ensure_zip_based_format(Path::new(filename))?;
        let is_epub = fmt == Some(ComicFormatExt::Epub);
        let owned = bytes.to_vec();
        tokio::task::spawn_blocking(move || {
            let file_size = owned.len() as u64;
            let file_hash = blake3::hash(&owned).to_hex().to_string();
            let inspected =
                inspect_comic_reader(Cursor::new(owned), is_epub).map_err(map_format_error)?;
            let (pages, _) = probed_to_descriptors(inspected.pages);
            Ok(ArchiveInspection {
                page_count: pages.len() as u32,
                pages,
                comic_info: inspected.comic_info.map(ComicInfoMetadata::from),
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
                ChapterSourceKind::OnlineTracked => {
                    return Err(DomainError::NotFound(
                        "online tracked chapter is not cached yet".into(),
                    ));
                }
                ChapterSourceKind::ArchiveFile => {
                    let archive_path = path_owned.clone();
                    let _ = ensure_zip_based_format(Path::new(&archive_path))?;
                    let file = File::open(&archive_path).map_err(|e| {
                        if e.kind() == std::io::ErrorKind::NotFound {
                            DomainError::NotFound(format!("archive '{archive_path}' not found"))
                        } else {
                            DomainError::Storage(format!(
                                "failed to open archive '{archive_path}': {e}"
                            ))
                        }
                    })?;
                    let mut zip = ZipArchive::new(file).map_err(|e| {
                        DomainError::Storage(format!("invalid zip archive '{archive_path}': {e}"))
                    })?;
                    let entry = zip.by_name(&entry_name).map_err(|e| {
                        DomainError::NotFound(format!(
                            "page entry '{entry_name}' not found in '{archive_path}': {e}"
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

    async fn open_raw_archive_stream(
        &self,
        source_path: &str,
        spec: ByteRangeSpec,
    ) -> DomainResult<(ByteRangeStream, String, String)> {
        let path = Path::new(source_path);
        let fmt = ensure_zip_based_format(path)?;
        let stream = open_file_range_stream_spec(path, spec).await?;
        let filename = path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| "chapter.cbz".into());
        let mime = fmt
            .map_or("application/vnd.comicbook+zip", ComicFormatExt::mime_type)
            .to_owned();
        Ok((stream, filename, mime))
    }
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
    ensure_zip_based_format(path)?;
    let (file_size, file_hash) = fast_file_fingerprint(path)?;
    let inspected = inspect_comic_path(path).map_err(map_format_error)?;
    let (pages, refs) = probed_to_descriptors(inspected.pages);
    Ok((
        ArchiveInspection {
            page_count: pages.len() as u32,
            pages,
            comic_info: inspected.comic_info.map(ComicInfoMetadata::from),
            file_size,
            file_hash,
        },
        refs,
    ))
}

fn inspect_image_directory(dir: &Path) -> DomainResult<(ArchiveInspection, Vec<PageEntryRef>)> {
    if !dir.is_dir() {
        return Err(DomainError::Storage(format!(
            "cannot read directory '{}'",
            dir.display()
        )));
    }
    let inspected = inspect_comic_path(dir).map_err(map_format_error)?;
    let probed = inspected.pages;
    let comic_info = inspected.comic_info.map(ComicInfoMetadata::from);

    let mut total_size = 0_u64;
    let mut hasher = blake3::Hasher::new();
    for page in &probed {
        total_size += page.file_size;
        let mtime = fs::metadata(dir.join(&page.entry_name))
            .ok()
            .and_then(|m| m.modified().ok())
            .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        hasher.update(page.entry_name.as_bytes());
        hasher.update(&page.file_size.to_le_bytes());
        hasher.update(&mtime.to_le_bytes());
    }

    let file_hash = hasher.finalize().to_hex().to_string();
    let (pages, refs) = probed_to_descriptors(probed);
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

#[cfg(test)]
mod tests {
    use std::io::Write;

    use image::{ImageBuffer, ImageFormat, Rgba};
    use tempfile::TempDir;
    use zip::ZipWriter;
    use zip::write::SimpleFileOptions;

    use super::*;

    fn tiny_png(w: u32, h: u32) -> Vec<u8> {
        let img: ImageBuffer<Rgba<u8>, Vec<u8>> =
            ImageBuffer::from_fn(w, h, |_, _| Rgba([40, 80, 120, 255]));
        let mut out = Cursor::new(Vec::new());
        img.write_to(&mut out, ImageFormat::Png).unwrap();
        out.into_inner()
    }

    #[tokio::test]
    async fn cbr_and_cb7_report_clear_unsupported_format_errors() {
        let tmp = TempDir::new().unwrap();
        let cbr_path = tmp.path().join("Vol.01.cbr");
        let cb7_path = tmp.path().join("Vol.02.cb7");
        fs::write(&cbr_path, b"Rar!\x1a\x07\x00").unwrap();
        fs::write(&cb7_path, b"7z\xbc\xaf\x27\x1c").unwrap();

        assert_eq!(
            ComicFormatExt::from_path(&cbr_path),
            Some(ComicFormatExt::Cbr)
        );
        assert_eq!(
            ComicFormatExt::from_path(&cb7_path),
            Some(ComicFormatExt::Cb7)
        );

        let reader = PureRustArchiveReader::new();
        for p in [&cbr_path, &cb7_path] {
            let err = reader
                .inspect_source(ChapterSourceKind::ArchiveFile, &p.to_string_lossy())
                .await
                .unwrap_err();
            assert!(matches!(err, DomainError::UnsupportedFormat(_)));
        }
        let bytes_err = reader
            .inspect_bytes("issue1.cbr", b"Rar!\x1a\x07\x00")
            .await
            .unwrap_err();
        assert!(matches!(bytes_err, DomainError::UnsupportedFormat(_)));
    }

    #[tokio::test]
    async fn inspect_probes_page_dimensions_for_cbz_and_image_folder() {
        let tmp = TempDir::new().unwrap();
        let p1 = tiny_png(320, 480);
        let p2 = tiny_png(640, 480);

        let cbz_path = tmp.path().join("test.cbz");
        {
            let f = File::create(&cbz_path).unwrap();
            let mut zw = ZipWriter::new(f);
            let opts = SimpleFileOptions::default();
            zw.start_file("002.png", opts).unwrap();
            zw.write_all(&p2).unwrap();
            zw.start_file("001.png", opts).unwrap();
            zw.write_all(&p1).unwrap();
            zw.finish().unwrap();
        }

        let reader = PureRustArchiveReader::new();
        let cbz_ins = reader
            .inspect_source(ChapterSourceKind::ArchiveFile, &cbz_path.to_string_lossy())
            .await
            .unwrap();
        assert_eq!(cbz_ins.page_count, 2);
        assert_eq!(cbz_ins.pages[0].entry_name, "001.png");
        assert_eq!(cbz_ins.pages[0].width, Some(320));
        assert_eq!(cbz_ins.pages[0].height, Some(480));
        assert_eq!(cbz_ins.pages[1].width, Some(640));

        let img_dir = tmp.path().join("Ch.1");
        fs::create_dir_all(&img_dir).unwrap();
        fs::write(img_dir.join("01.png"), &p1).unwrap();
        fs::write(img_dir.join("02.png"), &p2).unwrap();

        let dir_ins = reader
            .inspect_source(
                ChapterSourceKind::ImageDirectory,
                &img_dir.to_string_lossy(),
            )
            .await
            .unwrap();
        assert_eq!(dir_ins.page_count, 2);
        assert_eq!(dir_ins.pages[0].width, Some(320));
        assert_eq!(dir_ins.pages[1].width, Some(640));
    }
}

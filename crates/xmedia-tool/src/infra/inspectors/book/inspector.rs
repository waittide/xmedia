use std::fs::File;
use std::io::{BufReader, Read};
use std::path::Path;

use async_trait::async_trait;
use quick_xml::escape::escape;
use xmedia_core::application::ports::BinaryPayload;
use xmedia_core::domain::error::{DomainError, DomainResult};
use xmedia_core::domain::library::MediaKind;
use xmedia_core::infra::thumbnail::sniff_image_mime;
use zip::ZipArchive;

use super::epub_zip::{normalize_zip_rel, parse_epub_archive, rewrite_epub_entries};
use super::opf::{
    file_meta_to_parsed_book, render_sidecar_opf, replace_opf_metadata_block, swap_href_extension,
    update_manifest_cover_item,
};
use super::sidecar::{
    BOOK_EXTENSIONS, dir_has_single_book, find_existing_sidecar_opf, find_sidecar_cover,
    is_dedicated_calibre_book_dir, preferred_write_opf_path, read_book_meta_any,
};
use crate::domain::media::{
    BookFileMeta, BookMetaPatch, CompletenessFlags, MediaMetadataPatch, MediaMetadataPayload,
};
use crate::domain::ports::{MediaInspectorPort, QuickFileSummary};
use crate::infra::atomic_fs::atomic_write_bytes;
use crate::infra::inspectors::video::write_image_replacing_opposite_ext;
use xmedia_formats::book::{BookFormatExt, parse_book_bytes};

pub struct EpubBookInspector;

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
                        has_nfo_or_tag_block: !m.tags.is_empty()
                            || m.isbn.is_some()
                            || m.publisher
                                .as_ref()
                                .is_some_and(|pub_name| !pub_name.trim().is_empty()),
                        is_normalized: m.is_normalized,
                        is_organized: false,
                    },
                    metadata: Some(MediaMetadataPayload::Book(m)),
                }
            }
            Err(_) => QuickFileSummary {
                display_title: stem,
                display_subtitle: String::new(),
                year: None,
                completeness: CompletenessFlags::default(),
                metadata: None,
            },
        })
        .await
        .unwrap_or_else(|_| QuickFileSummary {
            display_title: fallback_stem.to_string(),
            display_subtitle: String::new(),
            year: None,
            completeness: CompletenessFlags::default(),
            metadata: None,
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
        if !BOOK_EXTENSIONS.contains(&ext.as_str()) {
            return Err(DomainError::UnsupportedFormat(format!(
                "unsupported book format '.{ext}'"
            )));
        }

        let p = path.to_path_buf();
        let bp_cloned: BookMetaPatch = bp.clone();

        let updated = tokio::task::spawn_blocking(move || -> DomainResult<BookFileMeta> {
            let apply_patch = |meta: &mut BookFileMeta, patch: BookMetaPatch| {
                if let Some(title) = patch.title
                    && !title.trim().is_empty()
                {
                    meta.title = title.trim().to_string();
                }
                if let Some(authors) = patch.authors {
                    meta.authors = authors
                        .into_iter()
                        .map(|s| s.trim().to_string())
                        .filter(|s| !s.is_empty())
                        .collect();
                }
                if let Some(series) = patch.series {
                    meta.series = (!series.trim().is_empty()).then(|| series.trim().to_string());
                }
                if let Some(subseries) = patch.subseries {
                    meta.subseries =
                        (!subseries.trim().is_empty()).then(|| subseries.trim().to_string());
                }
                if let Some(idx) = patch.series_index {
                    meta.series_index = (!idx.trim().is_empty()).then(|| idx.trim().to_string());
                }
                if let Some(rating) = patch.rating {
                    meta.rating = (rating.is_finite() && rating >= 0.0).then_some(rating);
                }
                if let Some(desc) = patch.description {
                    meta.description = (!desc.trim().is_empty()).then(|| desc.trim().to_string());
                }
                if let Some(pub_name) = patch.publisher {
                    meta.publisher =
                        (!pub_name.trim().is_empty()).then(|| pub_name.trim().to_string());
                }
                if let Some(pub_at) = patch.published_at {
                    meta.published_at =
                        (!pub_at.trim().is_empty()).then(|| pub_at.trim().to_string());
                }
                if let Some(lang) = patch.language {
                    meta.language = (!lang.trim().is_empty()).then(|| lang.trim().to_string());
                }
                if let Some(isbn) = patch.isbn {
                    meta.isbn = (!isbn.trim().is_empty()).then(|| isbn.trim().to_string());
                }
                if let Some(tags) = patch.tags {
                    meta.tags = tags
                        .into_iter()
                        .map(|s| s.trim().to_string())
                        .filter(|s| !s.is_empty())
                        .collect();
                }
                if let Some(locked) = patch.locked_fields {
                    let mut deduped = Vec::new();
                    for item in locked {
                        let f = item.trim().to_ascii_lowercase();
                        if !f.is_empty() && !deduped.contains(&f) {
                            deduped.push(f);
                        }
                    }
                    meta.locked_fields = deduped;
                }
            };

            if ext != "epub" {
                let mut meta = read_book_meta_any(&p)?;
                apply_patch(&mut meta, bp_cloned);
                let opf_path = preferred_write_opf_path(&p);
                let xml = if opf_path.is_file()
                    && let Ok(existing) = std::fs::read_to_string(&opf_path)
                {
                    replace_opf_metadata_block(&existing, &meta, None, "bookid", "2.0")
                } else {
                    render_sidecar_opf(&meta)
                };
                atomic_write_bytes(&opf_path, xml.as_bytes())?;
                return read_book_meta_any(&p);
            }

            let parsed = parse_epub_archive(&p)?;
            let mut meta = read_book_meta_any(&p)?;
            apply_patch(&mut meta, bp_cloned);

            let src_file = File::open(&p)
                .map_err(|e| DomainError::Storage(format!("failed reading epub: {e}")))?;
            let mut archive = ZipArchive::new(BufReader::new(src_file))
                .map_err(|e| DomainError::Validation(format!("invalid epub: {e}")))?;
            let mut opf_bytes = Vec::new();
            archive
                .by_name(&parsed.opf_path)
                .map_err(|_| DomainError::Validation("OPF missing".into()))?
                .read_to_end(&mut opf_bytes)
                .map_err(|e| DomainError::Storage(format!("read OPF error: {e}")))?;
            drop(archive);

            let opf_str = String::from_utf8_lossy(&opf_bytes);
            let next_opf = replace_opf_metadata_block(
                &opf_str,
                &meta,
                None,
                &parsed.unique_identifier_id,
                &parsed.package_version,
            );
            rewrite_epub_entries(&p, vec![(parsed.opf_path, next_opf.into_bytes())], &[])?;

            // Keep any existing sidecar OPF (e.g. Calibre metadata.opf) synchronized so it never shadows updated EPUB tags
            if let Some(sidecar_opf) = find_existing_sidecar_opf(&p) {
                let side_xml = if let Ok(existing) = std::fs::read_to_string(&sidecar_opf) {
                    replace_opf_metadata_block(&existing, &meta, None, "bookid", "2.0")
                } else {
                    render_sidecar_opf(&meta)
                };
                let _ = atomic_write_bytes(&sidecar_opf, side_xml.as_bytes());
            }

            read_book_meta_any(&p)
        })
        .await
        .map_err(|e| DomainError::Storage(format!("book write task failed: {e}")))??;

        Ok(MediaMetadataPayload::Book(updated))
    }

    async fn read_artwork(
        &self,
        path: &Path,
        _slot: Option<&str>,
    ) -> DomainResult<Option<BinaryPayload>> {
        let p = path.to_path_buf();
        tokio::task::spawn_blocking(move || {
            let ext = p
                .extension()
                .and_then(|s| s.to_str())
                .unwrap_or_default()
                .to_ascii_lowercase();
            if ext == "epub" {
                let parsed = parse_epub_archive(&p)?;
                if let Some(cover_entry) = parsed.cover_entry_path {
                    let src_file = File::open(&p)
                        .map_err(|e| DomainError::Storage(format!("failed reading epub: {e}")))?;
                    let mut archive = ZipArchive::new(BufReader::new(src_file))
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
            }

            if let Some(sidecar) = find_sidecar_cover(&p) {
                let buf = std::fs::read(&sidecar)
                    .map_err(|e| DomainError::Storage(format!("sidecar read error: {e}")))?;
                let mime = sniff_image_mime(&buf).to_string();
                return Ok(Some(BinaryPayload {
                    bytes: buf,
                    mime_type: mime,
                }));
            }

            if let Some(fmt_ext) = BookFormatExt::from_extension(&ext)
                && matches!(
                    fmt_ext,
                    BookFormatExt::Fb2 | BookFormatExt::Mobi | BookFormatExt::Azw3
                )
                && let Ok(raw) = std::fs::read(&p)
                && let Ok(pkg) = parse_book_bytes(fmt_ext, &raw, "Untitled")
                && let Some(cov) = pkg.cover
            {
                return Ok(Some(BinaryPayload {
                    bytes: cov.bytes,
                    mime_type: cov.mime_type,
                }));
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
        let sniffed = sniff_image_mime(&raw).to_string();
        let detected = if sniffed.starts_with("image/") && sniffed != "image/jpeg" {
            sniffed
        } else if mime_type.trim().starts_with("image/") {
            mime_type.trim().to_string()
        } else {
            sniffed
        };

        tokio::task::spawn_blocking(move || {
            let ext = p
                .extension()
                .and_then(|s| s.to_str())
                .unwrap_or_default()
                .to_ascii_lowercase();
            let img_ext = if detected.contains("png") {
                "png"
            } else if detected.contains("webp") {
                "webp"
            } else {
                "jpg"
            };

            if ext != "epub" {
                let has_same_stem = ["jpg", "jpeg", "png", "webp"]
                    .into_iter()
                    .any(|e| p.with_extension(e).is_file());
                if !has_same_stem
                    && let Some(parent) = p.parent()
                    && (is_dedicated_calibre_book_dir(parent)
                        || ["jpg", "jpeg", "png", "webp"]
                            .into_iter()
                            .any(|e| parent.join(format!("cover.{e}")).is_file()))
                {
                    return write_image_replacing_opposite_ext(parent, "cover", img_ext, &raw);
                }
                let sidecar = p.with_extension(img_ext);
                atomic_write_bytes(&sidecar, &raw)?;
                for old_ext in ["jpg", "jpeg", "png", "webp"] {
                    if old_ext != img_ext {
                        let stale = p.with_extension(old_ext);
                        if stale.is_file() {
                            let _ = std::fs::remove_file(stale);
                        }
                    }
                }
                return Ok(());
            }

            if let Some(parent) = p.parent()
                && dir_has_single_book(parent)
                && ["jpg", "jpeg", "png", "webp"]
                    .into_iter()
                    .any(|e| parent.join(format!("cover.{e}")).is_file())
            {
                let _ = write_image_replacing_opposite_ext(parent, "cover", img_ext, &raw);
            }

            let parsed = parse_epub_archive(&p)?;
            if let Some(existing_cover) = parsed.cover_entry_path {
                let same_mime = parsed
                    .cover_media_type
                    .as_deref()
                    .is_some_and(|m| m.eq_ignore_ascii_case(&detected));
                if same_mime {
                    return rewrite_epub_entries(&p, vec![(existing_cover, raw)], &[]);
                }

                let old_raw_href = parsed
                    .cover_raw_href
                    .as_deref()
                    .unwrap_or(&existing_cover);
                let new_raw_href = swap_href_extension(old_raw_href, img_ext);
                let opf_dir = Path::new(&parsed.opf_path).parent().unwrap_or(Path::new(""));
                let new_zip_cover = normalize_zip_rel(opf_dir, &new_raw_href);

                let src_file = File::open(&p)
                    .map_err(|e| DomainError::Storage(format!("failed reading epub: {e}")))?;
                let mut archive = ZipArchive::new(BufReader::new(src_file))
                    .map_err(|e| DomainError::Validation(format!("invalid epub: {e}")))?;
                let mut opf_bytes = Vec::new();
                archive
                    .by_name(&parsed.opf_path)
                    .map_err(|_| DomainError::Validation("OPF missing".into()))?
                    .read_to_end(&mut opf_bytes)
                    .map_err(|e| DomainError::Storage(format!("read OPF error: {e}")))?;
                drop(archive);

                let opf_str = String::from_utf8_lossy(&opf_bytes);
                let updated_opf = update_manifest_cover_item(
                    &opf_str,
                    parsed.cover_item_id.as_deref(),
                    old_raw_href,
                    &new_raw_href,
                    &detected,
                );
                let skip = if new_zip_cover != existing_cover {
                    vec![existing_cover]
                } else {
                    Vec::new()
                };
                return rewrite_epub_entries(
                    &p,
                    vec![
                        (new_zip_cover, raw),
                        (parsed.opf_path, updated_opf.into_bytes()),
                    ],
                    &skip,
                );
            }

            // Inject a new cover image entry + manifest item into OPF
            let opf_dir = Path::new(&parsed.opf_path).parent().unwrap_or(Path::new(""));
            let rel_href = format!("xmedia-cover.{img_ext}");
            let zip_cover_path = normalize_zip_rel(opf_dir, &rel_href);

            let src_file = File::open(&p)
                .map_err(|e| DomainError::Storage(format!("failed reading epub: {e}")))?;
            let mut archive = ZipArchive::new(BufReader::new(src_file))
                .map_err(|e| DomainError::Validation(format!("invalid epub: {e}")))?;
            let mut opf_bytes = Vec::new();
            archive
                .by_name(&parsed.opf_path)
                .map_err(|_| DomainError::Validation("OPF missing".into()))?
                .read_to_end(&mut opf_bytes)
                .map_err(|e| DomainError::Storage(format!("read OPF error: {e}")))?;
            drop(archive);

            let mut opf_str = String::from_utf8_lossy(&opf_bytes).into_owned();
            let item_tag = format!(
                "<item id=\"xmedia-cover-img\" href=\"{}\" media-type=\"{}\" properties=\"cover-image\"/>\n  </manifest>",
                escape(&rel_href),
                escape(&detected)
            );
            opf_str = opf_str.replacen("</manifest>", &item_tag, 1);
            opf_str = xmedia_formats::book::opf::replace_opf_metadata_block(
                &opf_str,
                &file_meta_to_parsed_book(&parsed.meta),
                Some("xmedia-cover-img"),
                &parsed.unique_identifier_id,
                &parsed.package_version,
            );

            rewrite_epub_entries(
                &p,
                vec![
                    (zip_cover_path, raw),
                    (parsed.opf_path, opf_str.into_bytes()),
                ],
                &[],
            )
        })
        .await
        .map_err(|e| DomainError::Storage(format!("book cover write failed: {e}")))?
    }
}

use std::collections::HashSet;
use std::fs::File;
use std::io::{BufReader, BufWriter, Cursor, Read, Seek, Write};
use std::path::Path;

use jiff::Timestamp;
use uuid::Uuid;
use zip::write::SimpleFileOptions;
use zip::{CompressionMethod, ZipArchive, ZipWriter};

use crate::common::{ext_for_raster_mime, xml_escape_clean};
use crate::error::{FormatError, FormatResult};

pub const DEFAULT_PACKAGED_EPUB_CSS: &str = r#"body {
  font-family: "Noto Serif SC", "Songti SC", Georgia, serif;
  line-height: 1.8;
  margin: 0;
  padding: 1em 1.2em;
}
h1, h2 {
  font-weight: 700;
  line-height: 1.4;
  margin: 0 0 1em 0;
}
p {
  text-indent: 2em;
  margin: 0 0 0.75em 0;
}
.cover-page {
  text-align: center;
  padding: 1em 0;
}
.cover-page img {
  max-width: 100%;
  max-height: 92vh;
}
"#;

#[derive(Debug, Clone)]
pub struct PackChapterInput {
    pub title: String,
    pub content: String,
}

#[derive(Debug, Clone)]
pub struct PackBookMetadata {
    pub identifier: Option<Uuid>,
    pub title: String,
    pub author: String,
    pub description: Option<String>,
    pub cover_bytes: Option<(Vec<u8>, String)>,
}

/// Streams an existing EPUB archive from `src` into `dst`, ensuring `mimetype` is written
/// first uncompressed and raw-copying untouched compressed entries without re-compression.
pub fn rewrite_epub_entries_stream<R: Read + Seek, W: Write + Seek>(
    src: R,
    dst: W,
    replacements: Vec<(String, Vec<u8>)>,
    skip_paths: &[String],
) -> FormatResult<W> {
    let mut src_archive = ZipArchive::new(src)
        .map_err(|e| FormatError::Validation(format!("invalid epub zip: {e}")))?;
    let mut writer = ZipWriter::new(dst);

    let stored = SimpleFileOptions::default().compression_method(CompressionMethod::Stored);
    writer
        .start_file("mimetype", stored)
        .map_err(|e| FormatError::Io(format!("epub start mimetype failed: {e}")))?;
    writer
        .write_all(b"application/epub+zip")
        .map_err(|e| FormatError::Io(format!("epub write mimetype failed: {e}")))?;

    let mut written: HashSet<String> = HashSet::from(["mimetype".to_string()]);

    for idx in 0..src_archive.len() {
        let entry = src_archive
            .by_index(idx)
            .map_err(|e| FormatError::Io(format!("epub entry error: {e}")))?;
        let Ok(name_ref) = entry.name() else {
            continue;
        };
        let name = name_ref.to_string();
        if entry.is_dir() || name == "mimetype" || skip_paths.iter().any(|s| s == &name) {
            continue;
        }
        if let Some((_, rep_bytes)) = replacements.iter().find(|(rn, _)| rn == &name) {
            drop(entry);
            let opts = SimpleFileOptions::default().compression_method(CompressionMethod::Deflated);
            writer
                .start_file(&name, opts)
                .map_err(|e| FormatError::Io(format!("epub replace start error: {e}")))?;
            writer
                .write_all(rep_bytes)
                .map_err(|e| FormatError::Io(format!("epub replace write error: {e}")))?;
            written.insert(name);
        } else {
            writer
                .raw_copy_file(entry)
                .map_err(|e| FormatError::Io(format!("epub raw copy error: {e}")))?;
            written.insert(name);
        }
    }

    for (rep_name, rep_bytes) in replacements {
        if !written.contains(&rep_name) {
            let opts = SimpleFileOptions::default().compression_method(CompressionMethod::Deflated);
            writer
                .start_file(rep_name, opts)
                .map_err(|e| FormatError::Io(format!("epub add entry error: {e}")))?;
            writer
                .write_all(&rep_bytes)
                .map_err(|e| FormatError::Io(format!("epub write new entry error: {e}")))?;
        }
    }

    writer
        .finish()
        .map_err(|e| FormatError::Io(format!("epub finish error: {e}")))
}

/// Atomically rewrites an EPUB file on disk via a sibling temporary file.
pub fn rewrite_epub_file_entries(
    epub_path: &Path,
    replacements: Vec<(String, Vec<u8>)>,
    skip_paths: &[String],
) -> FormatResult<()> {
    let parent = epub_path.parent().unwrap_or_else(|| Path::new("."));
    let stem = epub_path
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("book.epub");
    let staging = parent.join(format!(".{stem}.{}.tmp", Uuid::now_v7().simple()));

    let write_res = (|| -> FormatResult<()> {
        let src_file = File::open(epub_path)?;
        let out_file = File::create(&staging)?;
        let buf_writer = rewrite_epub_entries_stream(
            BufReader::new(src_file),
            BufWriter::new(out_file),
            replacements,
            skip_paths,
        )?;
        let inner = buf_writer
            .into_inner()
            .map_err(|e| FormatError::Io(format!("epub flush error: {e}")))?;
        inner.sync_all()?;
        Ok(())
    })();

    if let Err(err) = write_res {
        let _ = std::fs::remove_file(&staging);
        return Err(err);
    }
    if let Err(err) = std::fs::rename(&staging, epub_path) {
        let _ = std::fs::remove_file(&staging);
        return Err(FormatError::Io(format!(
            "failed committing staged epub '{}': {err}",
            epub_path.display()
        )));
    }
    Ok(())
}

/// Packages plain-text chapters into a formatted UTF-8 TXT document.
pub fn package_chapters_to_txt(meta: &PackBookMetadata, chapters: &[PackChapterInput]) -> Vec<u8> {
    let mut out = String::new();
    out.push_str(meta.title.trim());
    out.push('\n');
    if !meta.author.trim().is_empty() {
        out.push_str("作者：");
        out.push_str(meta.author.trim());
        out.push('\n');
    }
    if let Some(ref desc) = meta.description
        && !desc.trim().is_empty()
    {
        out.push_str("\n简介：\n");
        out.push_str(desc.trim());
        out.push('\n');
    }
    out.push('\n');

    for ch in chapters {
        out.push_str(ch.title.trim());
        out.push_str("\n\n");
        for para in ch.content.split("\n\n") {
            let p = para.trim();
            if !p.is_empty() {
                out.push_str("　　");
                out.push_str(p);
                out.push_str("\n\n");
            }
        }
        out.push('\n');
    }
    out.into_bytes()
}

/// Packages chapters into a standards-compliant EPUB 3.0 byte buffer (rejecting SVG covers).
pub fn package_chapters_to_epub(
    meta: &PackBookMetadata,
    chapters: &[PackChapterInput],
    custom_css: Option<&str>,
) -> FormatResult<Vec<u8>> {
    let cursor = Cursor::new(Vec::new());
    let mut zip = ZipWriter::new(cursor);

    let stored = SimpleFileOptions::default().compression_method(CompressionMethod::Stored);
    let deflated = SimpleFileOptions::default()
        .compression_method(CompressionMethod::Deflated)
        .compression_level(Some(6));

    zip.start_file("mimetype", stored)
        .map_err(|e| FormatError::Io(format!("写入 EPUB mimetype 失败: {e}")))?;
    zip.write_all(b"application/epub+zip")?;

    zip.start_file("META-INF/container.xml", deflated)
        .map_err(|e| FormatError::Io(format!("写入 container.xml 失败: {e}")))?;
    zip.write_all(
        br#"<?xml version="1.0" encoding="UTF-8"?>
<container version="1.0" xmlns="urn:oasis:names:tc:opendocument:xmlns:container">
  <rootfiles>
    <rootfile full-path="OEBPS/content.opf" media-type="application/oebps-package+xml"/>
  </rootfiles>
</container>
"#,
    )?;

    let css = custom_css
        .filter(|s| !s.trim().is_empty())
        .unwrap_or(DEFAULT_PACKAGED_EPUB_CSS);
    zip.start_file("OEBPS/style.css", deflated)
        .map_err(|e| FormatError::Io(e.to_string()))?;
    zip.write_all(css.as_bytes())?;

    let cover_info = if let Some((ref bytes, ref mime)) = meta.cover_bytes
        && !bytes.is_empty()
        && let Some(ext) = ext_for_raster_mime(mime)
    {
        let normalized_mime = match ext {
            "jpg" => "image/jpeg",
            "png" => "image/png",
            "webp" => "image/webp",
            "gif" => "image/gif",
            "avif" => "image/avif",
            "bmp" => "image/bmp",
            _ => "image/jpeg",
        };
        let cover_file = format!("cover.{ext}");
        zip.start_file(format!("OEBPS/{cover_file}"), stored)
            .map_err(|e| FormatError::Io(e.to_string()))?;
        zip.write_all(bytes)?;
        Some((cover_file, normalized_mime))
    } else {
        None
    };

    let mut manifest_items = String::new();
    let mut spine_items = String::new();
    let mut nav_lis = String::new();
    let mut ncx_points = String::new();

    if let Some((ref cover_file, cover_mime)) = cover_info {
        manifest_items.push_str(&format!(
            "    <item id=\"cover-image\" href=\"{cover_file}\" media-type=\"{cover_mime}\" properties=\"cover-image\"/>\n"
        ));
    }

    for (idx, ch) in chapters.iter().enumerate() {
        let num = idx + 1;
        let file_name = format!("ch_{num:05}.xhtml");
        let item_id = format!("ch_{num:05}");
        let safe_title = xml_escape_clean(ch.title.trim());

        let mut body_html = format!("  <h2>{safe_title}</h2>\n");
        for para in ch.content.split('\n') {
            let trimmed = para.trim();
            if !trimmed.is_empty() {
                body_html.push_str("  <p>");
                body_html.push_str(&xml_escape_clean(trimmed));
                body_html.push_str("</p>\n");
            }
        }

        let xhtml = format!(
            "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<!DOCTYPE html>\n<html xmlns=\"http://www.w3.org/1999/xhtml\" xml:lang=\"zh-CN\">\n<head>\n  <meta charset=\"UTF-8\"/>\n  <title>{safe_title}</title>\n  <link rel=\"stylesheet\" type=\"text/css\" href=\"style.css\"/>\n</head>\n<body>\n{body_html}</body>\n</html>\n"
        );

        zip.start_file(format!("OEBPS/{file_name}"), deflated)
            .map_err(|e| FormatError::Io(e.to_string()))?;
        zip.write_all(xhtml.as_bytes())?;

        manifest_items.push_str(&format!(
            "    <item id=\"{item_id}\" href=\"{file_name}\" media-type=\"application/xhtml+xml\"/>\n"
        ));
        spine_items.push_str(&format!("    <itemref idref=\"{item_id}\"/>\n"));
        nav_lis.push_str(&format!(
            "      <li><a href=\"{file_name}\">{safe_title}</a></li>\n"
        ));
        ncx_points.push_str(&format!(
            "    <navPoint id=\"nav_{num}\" playOrder=\"{num}\">\n      <navLabel><text>{safe_title}</text></navLabel>\n      <content src=\"{file_name}\"/>\n    </navPoint>\n"
        ));
    }

    let safe_book_title = xml_escape_clean(meta.title.trim());
    let safe_author = xml_escape_clean(if meta.author.trim().is_empty() {
        "佚名"
    } else {
        meta.author.trim()
    });
    let book_uuid = meta.identifier.unwrap_or_else(Uuid::new_v4);
    let now_iso = Timestamp::now().strftime("%Y-%m-%dT%H:%M:%SZ").to_string();

    if chapters.is_empty() {
        let cover_html = cover_info
            .as_ref()
            .map(|(cover_file, _)| {
                format!(
                    "  <div class=\"cover-page\"><img src=\"{cover_file}\" alt=\"{safe_book_title}\"/></div>\n"
                )
            })
            .unwrap_or_default();
        let intro_html = meta
            .description
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(|d| {
                d.split('\n')
                    .map(str::trim)
                    .filter(|t| !t.is_empty())
                    .map(|t| format!("  <p>{}</p>\n", xml_escape_clean(t)))
                    .collect::<String>()
            })
            .unwrap_or_default();
        let intro_xhtml = format!(
            "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<!DOCTYPE html>\n<html xmlns=\"http://www.w3.org/1999/xhtml\" xml:lang=\"zh-CN\">\n<head>\n  <meta charset=\"UTF-8\"/>\n  <title>{safe_book_title}</title>\n  <link rel=\"stylesheet\" type=\"text/css\" href=\"style.css\"/>\n</head>\n<body>\n{cover_html}  <h1>{safe_book_title}</h1>\n  <p>{safe_author}</p>\n{intro_html}</body>\n</html>\n"
        );
        zip.start_file("OEBPS/intro.xhtml", deflated)
            .map_err(|e| FormatError::Io(e.to_string()))?;
        zip.write_all(intro_xhtml.as_bytes())?;
        manifest_items.push_str(
            "    <item id=\"intro\" href=\"intro.xhtml\" media-type=\"application/xhtml+xml\"/>\n",
        );
        spine_items.push_str("    <itemref idref=\"intro\"/>\n");
        nav_lis.push_str(&format!(
            "      <li><a href=\"intro.xhtml\">{safe_book_title}</a></li>\n"
        ));
        ncx_points.push_str(&format!(
            "    <navPoint id=\"nav_1\" playOrder=\"1\">\n      <navLabel><text>{safe_book_title}</text></navLabel>\n      <content src=\"intro.xhtml\"/>\n    </navPoint>\n"
        ));
    }

    let nav_xhtml = format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<!DOCTYPE html>\n<html xmlns=\"http://www.w3.org/1999/xhtml\" xmlns:epub=\"http://www.idpf.org/2007/ops\" xml:lang=\"zh-CN\">\n<head>\n  <meta charset=\"UTF-8\"/>\n  <title>目录</title>\n  <link rel=\"stylesheet\" type=\"text/css\" href=\"style.css\"/>\n</head>\n<body>\n  <nav epub:type=\"toc\" id=\"toc\">\n    <h1>目录</h1>\n    <ol>\n{nav_lis}    </ol>\n  </nav>\n</body>\n</html>\n"
    );
    zip.start_file("OEBPS/nav.xhtml", deflated)
        .map_err(|e| FormatError::Io(e.to_string()))?;
    zip.write_all(nav_xhtml.as_bytes())?;

    let toc_ncx = format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<ncx xmlns=\"http://www.daisy.org/z3986/2005/ncx/\" version=\"2005-1\">\n  <head>\n    <meta name=\"dtb:uid\" content=\"urn:uuid:{book_uuid}\"/>\n    <meta name=\"dtb:depth\" content=\"1\"/>\n  </head>\n  <docTitle><text>{safe_book_title}</text></docTitle>\n  <navMap>\n{ncx_points}  </navMap>\n</ncx>\n"
    );
    zip.start_file("OEBPS/toc.ncx", deflated)
        .map_err(|e| FormatError::Io(e.to_string()))?;
    zip.write_all(toc_ncx.as_bytes())?;

    let desc_xml = meta
        .description
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(|d| {
            format!(
                "    <dc:description>{}</dc:description>\n",
                xml_escape_clean(d)
            )
        })
        .unwrap_or_default();
    let cover_meta = if cover_info.is_some() {
        "    <meta name=\"cover\" content=\"cover-image\"/>\n"
    } else {
        ""
    };
    let opf = format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<package xmlns=\"http://www.idpf.org/2007/opf\" version=\"3.0\" unique-identifier=\"BookId\" xml:lang=\"zh-CN\">\n  <metadata xmlns:dc=\"http://purl.org/dc/elements/1.1/\">\n    <dc:identifier id=\"BookId\">urn:uuid:{book_uuid}</dc:identifier>\n    <dc:title>{safe_book_title}</dc:title>\n    <dc:creator>{safe_author}</dc:creator>\n    <dc:language>zh-CN</dc:language>\n{desc_xml}    <meta property=\"dcterms:modified\">{now_iso}</meta>\n{cover_meta}  </metadata>\n  <manifest>\n    <item id=\"nav\" href=\"nav.xhtml\" media-type=\"application/xhtml+xml\" properties=\"nav\"/>\n    <item id=\"ncx\" href=\"toc.ncx\" media-type=\"application/x-dtbncx+xml\"/>\n    <item id=\"style\" href=\"style.css\" media-type=\"text/css\"/>\n{manifest_items}  </manifest>\n  <spine toc=\"ncx\">\n{spine_items}  </spine>\n</package>\n"
    );
    zip.start_file("OEBPS/content.opf", deflated)
        .map_err(|e| FormatError::Io(e.to_string()))?;
    zip.write_all(opf.as_bytes())?;

    let finished = zip
        .finish()
        .map_err(|e| FormatError::Io(format!("封装 EPUB 失败: {e}")))?;
    Ok(finished.into_inner())
}

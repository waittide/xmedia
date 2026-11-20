use std::io::{Cursor, Write};

use zip::write::SimpleFileOptions;
use zip::{CompressionMethod, ZipWriter};

#[derive(Debug, Clone)]
pub struct PackChapterInput {
    pub title: String,
    pub content: String,
}

#[derive(Debug, Clone)]
pub struct PackBookMetadata {
    pub title: String,
    pub author: String,
    pub description: Option<String>,
    pub cover_bytes: Option<(Vec<u8>, String)>, // (bytes, mime_type)
}

fn xml_escape(raw: &str) -> String {
    if raw
        .chars()
        .any(|c| c.is_control() && c != '\n' && c != '\r' && c != '\t')
    {
        let clean: String = raw
            .chars()
            .filter(|&c| !c.is_control() || matches!(c, '\n' | '\r' | '\t'))
            .collect();
        xmedia_core::domain::xml::xml_escape(&clean)
    } else {
        xmedia_core::domain::xml::xml_escape(raw)
    }
}

/// Packages online chapters into a clean UTF-8 TXT document.
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

/// Packages online chapters into a normalized, Deflate-compressed EPUB 3.0 archive.
pub fn package_chapters_to_epub(
    meta: &PackBookMetadata,
    chapters: &[PackChapterInput],
) -> Result<Vec<u8>, String> {
    let cursor = Cursor::new(Vec::new());
    let mut zip = ZipWriter::new(cursor);

    let stored = SimpleFileOptions::default().compression_method(CompressionMethod::Stored);
    let deflated = SimpleFileOptions::default()
        .compression_method(CompressionMethod::Deflated)
        .compression_level(Some(6));

    // 1. mimetype (must be first and uncompressed per OCF spec)
    zip.start_file("mimetype", stored)
        .map_err(|e| format!("写入 EPUB mimetype 失败: {e}"))?;
    zip.write_all(b"application/epub+zip")
        .map_err(|e| format!("写入 EPUB mimetype 内容失败: {e}"))?;

    // 2. META-INF/container.xml
    zip.start_file("META-INF/container.xml", deflated)
        .map_err(|e| format!("写入 container.xml 失败: {e}"))?;
    zip.write_all(
        br#"<?xml version="1.0" encoding="UTF-8"?>
<container version="1.0" xmlns="urn:oasis:names:tc:opendocument:xmlns:container">
  <rootfiles>
    <rootfile full-path="OEBPS/content.opf" media-type="application/oebps-package+xml"/>
  </rootfiles>
</container>
"#,
    )
    .map_err(|e| e.to_string())?;

    // 3. OEBPS/style.css
    zip.start_file("OEBPS/style.css", deflated)
        .map_err(|e| e.to_string())?;
    zip.write_all(
        br#"body {
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
"#,
    )
    .map_err(|e| e.to_string())?;

    // 4. Optional Cover Image
    let cover_info = if let Some((ref bytes, ref mime)) = meta.cover_bytes {
        if !bytes.is_empty() {
            let ext = match mime.as_str() {
                "image/png" => "png",
                "image/webp" => "webp",
                "image/gif" => "gif",
                _ => "jpg",
            };
            let cover_file = format!("cover.{ext}");
            zip.start_file(format!("OEBPS/{cover_file}"), stored)
                .map_err(|e| e.to_string())?;
            zip.write_all(bytes).map_err(|e| e.to_string())?;
            Some((cover_file, mime.clone()))
        } else {
            None
        }
    } else {
        None
    };

    // 5. Chapter XHTML files
    let mut manifest_items = String::new();
    let mut spine_items = String::new();
    let mut nav_lis = String::new();
    let mut ncx_points = String::new();

    if let Some((ref cover_file, ref cover_mime)) = cover_info {
        manifest_items.push_str(&format!(
            "    <item id=\"cover-image\" href=\"{cover_file}\" media-type=\"{cover_mime}\" properties=\"cover-image\"/>\n"
        ));
    }

    for (idx, ch) in chapters.iter().enumerate() {
        let num = idx + 1;
        let file_name = format!("ch_{num:05}.xhtml");
        let item_id = format!("ch_{num:05}");
        let safe_title = xml_escape(ch.title.trim());

        let mut body_html = String::new();
        body_html.push_str(&format!("  <h2>{safe_title}</h2>\n"));
        for para in ch.content.split('\n') {
            let trimmed = para.trim();
            if !trimmed.is_empty() {
                body_html.push_str("  <p>");
                body_html.push_str(&xml_escape(trimmed));
                body_html.push_str("</p>\n");
            }
        }

        let xhtml = format!(
            r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE html>
<html xmlns="http://www.w3.org/1999/xhtml" xml:lang="zh-CN">
<head>
  <meta charset="UTF-8"/>
  <title>{safe_title}</title>
  <link rel="stylesheet" type="text/css" href="style.css"/>
</head>
<body>
{body_html}</body>
</html>
"#
        );

        zip.start_file(format!("OEBPS/{file_name}"), deflated)
            .map_err(|e| e.to_string())?;
        zip.write_all(xhtml.as_bytes()).map_err(|e| e.to_string())?;

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

    let safe_book_title = xml_escape(meta.title.trim());
    let safe_author = xml_escape(if meta.author.trim().is_empty() {
        "佚名"
    } else {
        meta.author.trim()
    });
    let book_uuid = uuid::Uuid::new_v4();
    let now_iso = jiff::Timestamp::now()
        .strftime("%Y-%m-%dT%H:%M:%SZ")
        .to_string();

    // 6. OEBPS/nav.xhtml
    let nav_xhtml = format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE html>
<html xmlns="http://www.w3.org/1999/xhtml" xmlns:epub="http://www.idpf.org/2007/ops" xml:lang="zh-CN">
<head>
  <meta charset="UTF-8"/>
  <title>目录</title>
  <link rel="stylesheet" type="text/css" href="style.css"/>
</head>
<body>
  <nav epub:type="toc" id="toc">
    <h1>目录</h1>
    <ol>
{nav_lis}    </ol>
  </nav>
</body>
</html>
"#
    );
    zip.start_file("OEBPS/nav.xhtml", deflated)
        .map_err(|e| e.to_string())?;
    zip.write_all(nav_xhtml.as_bytes())
        .map_err(|e| e.to_string())?;

    // 7. OEBPS/toc.ncx
    let toc_ncx = format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<ncx xmlns="http://www.daisy.org/z3986/2005/ncx/" version="2005-1">
  <head>
    <meta name="dtb:uid" content="urn:uuid:{book_uuid}"/>
    <meta name="dtb:depth" content="1"/>
  </head>
  <docTitle><text>{safe_book_title}</text></docTitle>
  <navMap>
{ncx_points}  </navMap>
</ncx>
"#
    );
    zip.start_file("OEBPS/toc.ncx", deflated)
        .map_err(|e| e.to_string())?;
    zip.write_all(toc_ncx.as_bytes())
        .map_err(|e| e.to_string())?;

    // 8. OEBPS/content.opf
    let desc_xml = meta
        .description
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(|d| format!("    <dc:description>{}</dc:description>\n", xml_escape(d)))
        .unwrap_or_default();
    let cover_meta = if cover_info.is_some() {
        "    <meta name=\"cover\" content=\"cover-image\"/>\n"
    } else {
        ""
    };

    let opf = format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<package xmlns="http://www.idpf.org/2007/opf" version="3.0" unique-identifier="BookId" xml:lang="zh-CN">
  <metadata xmlns:dc="http://purl.org/dc/elements/1.1/">
    <dc:identifier id="BookId">urn:uuid:{book_uuid}</dc:identifier>
    <dc:title>{safe_book_title}</dc:title>
    <dc:creator>{safe_author}</dc:creator>
    <dc:language>zh-CN</dc:language>
{desc_xml}    <meta property="dcterms:modified">{now_iso}</meta>
{cover_meta}  </metadata>
  <manifest>
    <item id="nav" href="nav.xhtml" media-type="application/xhtml+xml" properties="nav"/>
    <item id="ncx" href="toc.ncx" media-type="application/x-dtbncx+xml"/>
    <item id="style" href="style.css" media-type="text/css"/>
{manifest_items}  </manifest>
  <spine toc="ncx">
{spine_items}  </spine>
</package>
"#
    );
    zip.start_file("OEBPS/content.opf", deflated)
        .map_err(|e| e.to_string())?;
    zip.write_all(opf.as_bytes()).map_err(|e| e.to_string())?;

    let finished = zip.finish().map_err(|e| format!("封装 EPUB 失败: {e}"))?;
    Ok(finished.into_inner())
}

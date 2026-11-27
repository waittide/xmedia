use std::fs::File;
use std::io::{Cursor, Read, Write};
use std::path::Path;

use image::{DynamicImage, ImageBuffer, ImageFormat};
use lopdf::{Document, Object};
use zip::write::SimpleFileOptions;
use zip::{CompressionMethod, ZipArchive, ZipWriter};

use super::*;
use crate::infra::inspectors::book::read_book_meta_any;

fn make_png_bytes(w: u32, h: u32) -> Vec<u8> {
    let mut img = ImageBuffer::<image::Rgb<u8>, Vec<u8>>::new(w, h);
    for (x, y, px) in img.enumerate_pixels_mut() {
        *px = image::Rgb([
            ((x * 7) % 256) as u8,
            ((y * 11) % 256) as u8,
            (((x + y) * 5) % 256) as u8,
        ]);
    }
    let mut cursor = Cursor::new(Vec::new());
    DynamicImage::ImageRgb8(img)
        .write_to(&mut cursor, ImageFormat::Png)
        .unwrap();
    cursor.into_inner()
}

fn write_sample_epub_with_png_and_junk(path: &Path) {
    let f = File::create(path).unwrap();
    let mut zw = ZipWriter::new(f);
    let stored = SimpleFileOptions::default().compression_method(CompressionMethod::Stored);
    zw.start_file("mimetype", stored).unwrap();
    zw.write_all(b"application/epub+zip").unwrap();
    zw.start_file("META-INF/container.xml", stored).unwrap();
    zw.write_all(
        br#"<?xml version="1.0" encoding="UTF-8"?>
<container version="1.0" xmlns="urn:oasis:names:tc:opendocument:xmlns:container">
  <rootfiles>
    <rootfile full-path="OEBPS/content.opf" media-type="application/oebps-package+xml"/>
  </rootfiles>
</container>"#,
    )
    .unwrap();
    zw.start_file("OEBPS/content.opf", stored).unwrap();
    zw.write_all(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<package xmlns="http://www.idpf.org/2007/opf" version="2.0" unique-identifier="bookid">
  <metadata xmlns:dc="http://purl.org/dc/elements/1.1/">
    <dc:title>测试压缩书籍</dc:title>
    <dc:creator>测试作者</dc:creator>
    <dc:identifier id="bookid">urn:uuid:1234</dc:identifier>
  </metadata>
  <manifest>
    <item id="ch1" href="Text/ch1.xhtml" media-type="application/xhtml+xml"/>
    <item id="pic1" href="Images/pic1.png" media-type="image/png"/>
    <item id="orphan-img" href="Images/unused.png" media-type="image/png"/>
  </manifest>
  <spine>
    <itemref idref="ch1"/>
  </spine>
</package>"#
            .as_bytes(),
    )
    .unwrap();
    zw.start_file("OEBPS/Text/ch1.xhtml", stored).unwrap();
    zw.write_all(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<html xmlns="http://www.w3.org/1999/xhtml"><head><title>第一章</title></head><body><h2>第一章</h2><p>正文内容测试重复文本重复文本重复文本重复文本。</p><img src="../Images/pic1.png" alt="p"/></body></html>"#
            .as_bytes(),
    )
    .unwrap();
    let big_png = make_png_bytes(600, 800);
    zw.start_file("OEBPS/Images/pic1.png", stored).unwrap();
    zw.write_all(&big_png).unwrap();
    zw.start_file("OEBPS/Images/unused.png", stored).unwrap();
    zw.write_all(&big_png).unwrap();
    zw.start_file("__MACOSX/._junk", stored).unwrap();
    zw.write_all(b"junk").unwrap();
    zw.finish().unwrap();
}

#[test]
fn normalize_txt_to_epub_keeps_or_deletes_source() {
    let tmp = tempfile::tempdir().unwrap();
    let txt_path = tmp.path().join("demo.txt");
    std::fs::write(
        &txt_path,
        "第一章 初遇\n这是一段测试正文内容。\n\n第二章 再见\n另一段正文内容。\n",
    )
    .unwrap();

    let opts = NormalizeBookToEpubOptions::default();
    assert!(opts.keep_source);
    assert_eq!(opts.image_format, BookImageCompressFormat::Webp);

    let res = normalize_book_to_epub(&txt_path, &opts).unwrap();
    assert!(txt_path.exists());
    assert!(res.output_path.exists());
    assert_eq!(res.chapter_count, 2);
    assert!(!res.deleted_source);

    // Verify valid EPUB 3.0 with Stored mimetype as entry #0
    let mut za = ZipArchive::new(File::open(&res.output_path).unwrap()).unwrap();
    let first = za.by_index(0).unwrap();
    assert_eq!(first.name().unwrap(), "mimetype");
    assert_eq!(first.compression(), CompressionMethod::Stored);
}

#[test]
fn normalize_epub_backs_up_orig_cleans_orphans_and_compresses_to_webp() {
    let tmp = tempfile::tempdir().unwrap();
    let epub_path = tmp.path().join("novel.epub");
    write_sample_epub_with_png_and_junk(&epub_path);

    let opts = NormalizeBookToEpubOptions {
        keep_source: true,
        compress_images: true,
        image_format: BookImageCompressFormat::Webp,
        max_image_dimension: Some(400),
        image_quality: 82,
        generate_jacket: true,
        remove_orphans: true,
        pdf_scanned_fallback: true,
        skip_normalized: true,
    };

    let res = normalize_book_to_epub(&epub_path, &opts).unwrap();
    assert!(
        res.backup_path
            .as_ref()
            .is_some_and(|p| p.ends_with("novel.orig.epub") && p.is_file())
    );
    assert!(res.after_bytes < res.before_bytes);
    assert!(res.removed_orphans >= 2);
    assert_eq!(res.compressed_images, 1);

    let mut za = ZipArchive::new(File::open(&epub_path).unwrap()).unwrap();
    let first = za.by_index(0).unwrap();
    assert_eq!(first.name().unwrap(), "mimetype");
    assert_eq!(first.compression(), CompressionMethod::Stored);
    drop(first);

    // Verify pic1.png was converted to pic1.webp and ch1.xhtml reference was rewritten
    assert!(za.by_name("OEBPS/Images/pic1.webp").is_ok());
    assert!(za.by_name("OEBPS/Images/unused.png").is_err());
    let mut ch1 = String::new();
    za.by_name("OEBPS/Text/ch1.xhtml")
        .unwrap()
        .read_to_string(&mut ch1)
        .unwrap();
    assert!(ch1.contains("pic1.webp"));

    // Verify xmedia:normalized marker was persisted and read back
    let reread = read_book_meta_any(&epub_path).unwrap();
    assert!(reread.is_normalized);

    // Re-running with skip_normalized=true should skip both the normalized EPUB and its .orig.epub backup
    let skipped_main = normalize_book_to_epub(&epub_path, &opts).unwrap();
    assert!(skipped_main.skipped);
    assert!(skipped_main.backup_path.is_none());
    assert!(!tmp.path().join("novel.orig.2.epub").exists());

    let orig_backup = tmp.path().join("novel.orig.epub");
    let skipped_backup = normalize_book_to_epub(&orig_backup, &opts).unwrap();
    assert!(skipped_backup.skipped);

    // Force re-run with skip_normalized=false should process normally
    let mut force_opts = opts.clone();
    force_opts.skip_normalized = false;
    force_opts.keep_source = false;
    let forced = normalize_book_to_epub(&epub_path, &force_opts).unwrap();
    assert!(!forced.skipped);
    assert!(read_book_meta_any(&epub_path).unwrap().is_normalized);
}

#[test]
fn normalize_pdf_to_epub_extracts_text_and_respects_keep_source() {
    use lopdf::content::{Content, Operation};
    use lopdf::{Dictionary, Stream, dictionary};

    let tmp = tempfile::tempdir().unwrap();
    let pdf_path = tmp.path().join("sample.pdf");

    let mut doc = Document::with_version("1.5");
    let pages_id = doc.new_object_id();
    let font_id = doc.add_object(dictionary! {
        "Type" => "Font",
        "Subtype" => "Type1",
        "BaseFont" => "Helvetica",
    });
    let resources_id = doc.add_object(dictionary! {
        "Font" => dictionary! {
            "F1" => font_id,
        },
    });
    let content = Content {
        operations: vec![
            Operation::new("BT", vec![]),
            Operation::new("Tf", vec!["F1".into(), 14.into()]),
            Operation::new("Td", vec![50.into(), 720.into()]),
            Operation::new(
                "Tj",
                vec![Object::string_literal(
                    "Chapter 1 Getting Started with Rust and Normalized EPUB Generation.",
                )],
            ),
            Operation::new("ET", vec![]),
        ],
    };
    let content_id = doc.add_object(Stream::new(Dictionary::new(), content.encode().unwrap()));
    let page_id = doc.add_object(dictionary! {
        "Type" => "Page",
        "Parent" => pages_id,
        "Contents" => content_id,
        "Resources" => resources_id,
        "MediaBox" => vec![0.into(), 0.into(), 595.into(), 842.into()],
    });
    let pages = dictionary! {
        "Type" => "Pages",
        "Kids" => vec![page_id.into()],
        "Count" => 1,
    };
    doc.objects.insert(pages_id, Object::Dictionary(pages));
    let catalog_id = doc.add_object(dictionary! {
        "Type" => "Catalog",
        "Pages" => pages_id,
    });
    doc.trailer.set("Root", catalog_id);
    doc.save(&pdf_path).unwrap();

    let opts = NormalizeBookToEpubOptions::default();
    let res = normalize_book_to_epub(&pdf_path, &opts).unwrap();
    assert!(pdf_path.exists());
    assert!(res.output_path.exists());
    assert_eq!(res.source_format, "pdf");
    assert!(res.chapter_count >= 1);

    let mut za = ZipArchive::new(File::open(&res.output_path).unwrap()).unwrap();
    let mut ch1 = String::new();
    za.by_name("OEBPS/Text/chapter_0001.xhtml")
        .unwrap()
        .read_to_string(&mut ch1)
        .unwrap();
    assert!(ch1.contains("Normalized EPUB Generation"));
}

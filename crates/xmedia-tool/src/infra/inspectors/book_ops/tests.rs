use std::fs::File;
use std::io::{BufReader, Write};

use zip::write::SimpleFileOptions;
use zip::{CompressionMethod, ZipArchive, ZipWriter};

use crate::application::ports::MediaInspectorPort;
use crate::domain::media::{BookMetaPatch, MediaMetadataPatch};
use crate::domain::zh_text::{BookTextTransformOptions, ZhConvertDirection};
use crate::infra::inspectors::book::{EpubBookInspector, read_book_meta_any};

use super::epub_io::read_epub_internal_asset;
use super::epub_ops::{convert_txt_to_epub, insert_epub_image, polish_epub_book};
use super::txt::segment_txt_chapters;
use super::*;

#[tokio::test]
async fn txt_segmentation_convert_to_epub_and_dual_mode_epub_edit_roundtrip() {
    let tmp = tempfile::tempdir().unwrap();
    let txt_path = tmp.path().join("三体.txt");
    let raw_txt = "楔子\n疯狂年代的记忆。\n\n第一卷 地球往事\n\n第一章 科学边界\n叶文洁看着星空。\n第二段文字。\n\n第二章 台球\n丁仪推开了桌球。\n";
    std::fs::write(&txt_path, raw_txt.as_bytes()).unwrap();

    // Write sidecar OPF metadata (author, series, description)
    let inspector = EpubBookInspector;
    inspector
        .write_metadata(
            &txt_path,
            &MediaMetadataPatch::Book(BookMetaPatch {
                title: Some("三体".into()),
                authors: Some(vec!["刘慈欣".into()]),
                series: Some("地球往事三部曲".into()),
                series_index: Some("1".into()),
                description: Some("地球文明向宇宙发出的第一声啼鸣。".into()),
                ..Default::default()
            }),
        )
        .await
        .unwrap();

    // 1. Read TXT structured chapters
    let txt_doc = read_book_content(&txt_path).unwrap();
    assert_eq!(txt_doc.format, "txt");
    assert_eq!(txt_doc.title, "三体");
    assert_eq!(txt_doc.chapters.len(), 4);
    assert_eq!(txt_doc.chapters[0].title, "楔子");
    assert_eq!(txt_doc.chapters[0].level, 0);
    assert_eq!(txt_doc.chapters[1].title, "第一卷 地球往事");
    assert_eq!(txt_doc.chapters[1].level, 0);
    assert_eq!(txt_doc.chapters[2].title, "第一章 科学边界");
    assert_eq!(txt_doc.chapters[2].level, 1);

    // 2. Convert TXT -> EPUB (keeping source TXT first)
    let epub_path = convert_txt_to_epub(&txt_path, None, None, false).unwrap();
    assert!(epub_path.is_file());
    assert!(txt_path.is_file());

    // Verify EPUB metadata & TOC + chapters
    let epub_doc = read_book_content(&epub_path).unwrap();
    assert_eq!(epub_doc.format, "epub");
    assert_eq!(epub_doc.title, "三体");
    assert_eq!(epub_doc.authors, vec!["刘慈欣"]);
    assert_eq!(epub_doc.chapters.len(), 4);
    assert_eq!(epub_doc.chapters[2].title, "第一章 科学边界");
    assert_eq!(epub_doc.chapters[2].level, 1);
    assert!(epub_doc.chapters[2].plain_text.contains("叶文洁看着星空。"));

    // 3. Edit EPUB chapters in BOTH Plain mode and XHTML mode, reorder & add a new chapter
    let edited_inputs = vec![
        SaveBookChapterInput {
            title: "第一卷 地球往事（修订版）".into(),
            level: 0,
            mode: ChapterEditMode::Plain,
            plain_text: "卷首导读。".into(),
            xhtml_body: String::new(),
            source_href: epub_doc.chapters[1].source_href.clone(),
        },
        SaveBookChapterInput {
            title: "第一章 科学边界·改".into(),
            level: 1,
            mode: ChapterEditMode::Xhtml,
            plain_text: String::new(),
            xhtml_body: "  <h2>第一章 科学边界·改</h2>\n  <p class=\"lead\">自定义 <strong>XHTML</strong> 正文段落。</p>".into(),
            source_href: epub_doc.chapters[2].source_href.clone(),
        },
        SaveBookChapterInput {
            title: "第二章 新增番外".into(),
            level: 1,
            mode: ChapterEditMode::Plain,
            plain_text: "这是新插入的一章正文。".into(),
            xhtml_body: String::new(),
            source_href: None,
        },
    ];
    save_book_content(&epub_path, &edited_inputs, None, false).unwrap();

    // 4. Reload EPUB and verify TOC hierarchy + plain & XHTML edits persisted cleanly
    let reloaded = read_book_content(&epub_path).unwrap();
    assert_eq!(reloaded.chapters.len(), 3);
    assert_eq!(reloaded.chapters[0].title, "第一卷 地球往事（修订版）");
    assert_eq!(reloaded.chapters[0].level, 0);
    assert_eq!(reloaded.chapters[0].plain_text, "卷首导读。");

    assert_eq!(reloaded.chapters[1].title, "第一章 科学边界·改");
    assert_eq!(reloaded.chapters[1].level, 1);
    assert!(
        reloaded.chapters[1]
            .xhtml_body
            .contains("<strong>XHTML</strong>")
    );
    assert_eq!(reloaded.chapters[1].plain_text, "自定义 XHTML 正文段落。");

    assert_eq!(reloaded.chapters[2].title, "第二章 新增番外");
    assert_eq!(reloaded.chapters[2].level, 1);
    assert_eq!(reloaded.chapters[2].plain_text, "这是新插入的一章正文。");

    // 5. Insert two images into EPUB; reference only the first in chapter 1, leave the second orphaned
    let png_1x1: &[u8] = &[
        0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A, 0x00, 0x00, 0x00, 0x0D, b'I', b'H', b'D',
        b'R', 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01, 0x08, 0x06, 0x00, 0x00, 0x00, 0x1F,
        0x15, 0xC4, 0x89, 0x00, 0x00, 0x00, 0x0A, b'I', b'D', b'A', b'T', 0x78, 0x9C, 0x63, 0x00,
        0x01, 0x00, 0x00, 0x05, 0x00, 0x01, 0x0D, 0x0A, 0x2D, 0xB4, 0x00, 0x00, 0x00, 0x00, b'I',
        b'E', b'N', b'D', 0xAE, 0x42, 0x60, 0x82,
    ];
    let img_used = insert_epub_image(
        &epub_path,
        reloaded.chapters[0].source_href.as_deref(),
        png_1x1,
        Some("image/png"),
    )
    .unwrap();
    let img_orphan = insert_epub_image(
        &epub_path,
        reloaded.chapters[0].source_href.as_deref(),
        png_1x1,
        Some("image/png"),
    )
    .unwrap();
    assert!(img_used.chapter_rel_href.contains("Images/ximg_"));

    // Untrusted SVG or HTML/script payloads must be rejected by insert_epub_image
    let svg_xss = b"<svg xmlns=\"http://www.w3.org/2000/svg\" onload=\"alert(1)\"><script>alert(1)</script></svg>";
    assert!(
        insert_epub_image(
            &epub_path,
            reloaded.chapters[0].source_href.as_deref(),
            svg_xss,
            Some("image/svg+xml"),
        )
        .is_err()
    );
    assert!(
        insert_epub_image(
            &epub_path,
            reloaded.chapters[0].source_href.as_deref(),
            b"<html><script>alert(1)</script></html>",
            Some("image/png"),
        )
        .is_err()
    );

    let with_img_inputs = vec![SaveBookChapterInput {
        title: "第一卷 含插图章".into(),
        level: 0,
        mode: ChapterEditMode::Xhtml,
        plain_text: String::new(),
        xhtml_body: format!(
            "  <h2>第一卷 含插图章</h2>\n  <figure><img src=\"{}\" alt=\"插图\"/></figure>\n  <p>「軟體測試」</p>",
            img_used.chapter_rel_href
        ),
        source_href: reloaded.chapters[0].source_href.clone(),
    }];
    let custom_css = vec!["body { writing-mode: vertical-rl; font-size: 18px; }".to_string()];
    save_book_content(&epub_path, &with_img_inputs, Some(&custom_css), false).unwrap();

    // 6. Batch transform: T2S + vertical_to_horizontal
    let before_tf = read_book_content(&epub_path).unwrap();
    let tf_res = transform_book_content_batch(
        before_tf.chapters,
        before_tf.stylesheets,
        &BookTextTransformOptions {
            zh_direction: Some(ZhConvertDirection::T2s),
            vertical_to_horizontal: true,
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(tf_res.changed_chapters, 1);
    assert!(tf_res.chapters[0].xhtml_body.contains("“软件测试”"));
    assert!(tf_res.stylesheets[0].contains("writing-mode: horizontal-tb"));

    // 7. Polish EPUB: generates xmedia_jacket.xhtml and cleans up unreferenced img_orphan!
    let polish_res = polish_epub_book(&epub_path).unwrap();
    assert!(polish_res.jacket_updated);
    assert_eq!(polish_res.removed_orphan_files, 1);
    assert!(read_epub_internal_asset(&epub_path, &img_used.asset_path).is_ok());
    assert!(read_epub_internal_asset(&epub_path, &img_orphan.asset_path).is_err());

    // Idempotent second polish removes 0 orphans and updates jacket in-place
    let polish_again = polish_epub_book(&epub_path).unwrap();
    assert_eq!(polish_again.removed_orphan_files, 0);
    let after_polish = read_book_content(&epub_path).unwrap();
    assert_eq!(after_polish.chapters[0].title, "书籍简介");
    assert!(
        after_polish.chapters[0]
            .plain_text
            .contains("地球往事三部曲")
    );

    // 8. Verify locked_fields persistence in OPF
    inspector
        .write_metadata(
            &epub_path,
            &MediaMetadataPatch::Book(BookMetaPatch {
                locked_fields: Some(vec!["title".into(), "series".into(), "cover".into()]),
                ..Default::default()
            }),
        )
        .await
        .unwrap();
    let locked_meta = read_book_meta_any(&epub_path).unwrap();
    assert_eq!(locked_meta.locked_fields, vec!["title", "series", "cover"]);
}

#[test]
fn polish_epub_preserves_percent_encoded_cjk_chapters_and_images() {
    let tmp = tempfile::tempdir().unwrap();
    let epub_path = tmp.path().join("cjk_encoded.epub");
    {
        let f = File::create(&epub_path).unwrap();
        let mut zw = ZipWriter::new(f);
        let stored = SimpleFileOptions::default().compression_method(CompressionMethod::Stored);
        let deflated = SimpleFileOptions::default().compression_method(CompressionMethod::Deflated);
        zw.start_file("mimetype", stored).unwrap();
        zw.write_all(b"application/epub+zip").unwrap();
        zw.start_file("META-INF/container.xml", deflated).unwrap();
        zw.write_all(
            br#"<?xml version="1.0"?><container version="1.0" xmlns="urn:oasis:names:tc:opendocument:xmlns:container"><rootfiles><rootfile full-path="OEBPS/content.opf" media-type="application/oebps-package+xml"/></rootfiles></container>"#,
        )
        .unwrap();
        zw.start_file("OEBPS/content.opf", deflated).unwrap();
        zw.write_all(
            r#"<?xml version="1.0" encoding="UTF-8"?>
<package xmlns="http://www.idpf.org/opf" version="3.0" unique-identifier="uid">
  <metadata xmlns:dc="http://purl.org/dc/elements/1.1/">
    <dc:identifier id="uid">urn:uuid:test-cjk</dc:identifier>
    <dc:title>中文编码测试</dc:title>
    <dc:language>zh-CN</dc:language>
  </metadata>
  <manifest>
    <item id="ch1" href="Text/%E7%AC%AC%E4%B8%80%E7%AB%A0.xhtml" media-type="application/xhtml+xml"/>
    <item id="img1" href="Images/%E6%8F%92%E5%9B%BE%201.png" media-type="image/png"/>
  </manifest>
  <spine>
    <itemref idref="ch1"/>
  </spine>
</package>"#
                .as_bytes(),
        )
        .unwrap();
        zw.start_file("OEBPS/Text/第一章.xhtml", deflated).unwrap();
        zw.write_all(
            r#"<?xml version="1.0" encoding="UTF-8"?><html xmlns="http://www.w3.org/1999/xhtml"><head><title>第一章</title></head><body><h1>第一章</h1><p><img src="../Images/插图 1.png" alt="图"/></p></body></html>"#
                .as_bytes(),
        )
        .unwrap();
        zw.start_file("OEBPS/Images/插图 1.png", stored).unwrap();
        zw.write_all(&[0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A])
            .unwrap();
        zw.finish().unwrap();
    }

    let report = polish_epub_book(&epub_path).unwrap();
    assert_eq!(report.removed_orphan_files, 0);
    // Active XHTML must NOT be served via the raw epub-asset HTTP endpoint, while remaining
    // intact inside the polished archive alongside its percent-encoded CJK image asset.
    assert!(read_epub_internal_asset(&epub_path, "OEBPS/Text/第一章.xhtml").is_err());
    {
        let f = File::open(&epub_path).unwrap();
        let mut archive = ZipArchive::new(BufReader::new(f)).unwrap();
        assert!(archive.by_name("OEBPS/Text/第一章.xhtml").is_ok());
    }
    assert!(read_epub_internal_asset(&epub_path, "OEBPS/Images/插图 1.png").is_ok());
}

#[test]
fn oversized_user_regexes_are_rejected_cleanly() {
    let huge = "(?:[a-zA-Z0-9_]{1000}){1000}";
    assert!(segment_txt_chapters("第一章 测试\n正文", Some(huge), None).is_err());
    assert!(segment_txt_chapters("第一卷 序\n正文", None, Some(huge)).is_err());
    assert!(
        transform_book_content_batch(
            Vec::new(),
            Vec::new(),
            &BookTextTransformOptions {
                custom_strip_patterns: vec![huge.to_string()],
                ..Default::default()
            },
        )
        .is_err()
    );
}

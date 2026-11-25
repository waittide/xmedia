use std::cmp::Ordering;
use std::io::{Cursor, Read, Seek, SeekFrom};
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering as AtomicOrdering};

use xmedia_formats::book::epub::read_epub_resource_from_reader;
use xmedia_formats::book::epub_write::{
    PackBookMetadata, PackChapterInput, package_chapters_to_epub,
};
use xmedia_formats::book::opf::{parse_opf_xml, replace_opf_metadata_block};
use xmedia_formats::book::sidecar::{
    find_existing_sidecar_opf, find_sidecar_cover, is_dedicated_book_dir,
};
use xmedia_formats::book::{
    BookFormatExt, list_book_toc_bytes, parse_book_bytes, read_book_text_chapter_bytes,
};
use xmedia_formats::comic::comicinfo::{
    ComicInfoMeta, parse_comic_info_xml, render_comic_info_xml_preserving,
};
use xmedia_formats::comic::filename::{
    ComicChapterKind, natural_cmp, parse_comic_chapter_filename,
};
use xmedia_formats::comic::{
    ComicFormatExt, ComicReadingDirection, is_comic_image_dir, probe_comic_pages,
};

fn make_png(width: u32, height: u32) -> Vec<u8> {
    let img = image::RgbImage::from_pixel(width, height, image::Rgb([12, 34, 56]));
    let mut buf = Vec::new();
    img.write_to(&mut Cursor::new(&mut buf), image::ImageFormat::Png)
        .expect("encode test png");
    buf
}

#[test]
fn book_opf_roundtrip_preserves_series_rating_epub3_collections_and_unknown_meta() {
    let raw_opf = r#"<?xml version="1.0" encoding="UTF-8"?>
<package xmlns="http://www.idpf.org/2007/opf" version="2.0" unique-identifier="bookid">
  <metadata xmlns:dc="http://purl.org/dc/elements/1.1/" xmlns:opf="http://www.idpf.org/2007/opf">
    <dc:identifier id="bookid">urn:uuid:11111111-2222-3333-4444-555555555555</dc:identifier>
    <dc:title>Three-Body</dc:title>
    <dc:creator>Liu Cixin</dc:creator>
    <meta name="calibre:series" content="Remembrance of Earth's Past"/>
    <meta name="calibre:series_index" content="1.5"/>
    <meta name="calibre:rating" content="9.5"/>
    <meta name="custom:keep_me" content="secret-value"/>
  </metadata>
  <manifest></manifest>
  <spine></spine>
</package>"#;

    let mut parsed = parse_opf_xml(raw_opf.as_bytes(), BookFormatExt::Epub, "Fallback");
    assert_eq!(parsed.meta.title, "Three-Body");
    assert_eq!(
        parsed.meta.series.as_deref(),
        Some("Remembrance of Earth's Past")
    );
    assert_eq!(parsed.meta.series_index, Some(1.5));
    assert_eq!(parsed.meta.rating, Some(9.5));

    parsed.meta.title = "三体".into();
    parsed.meta.series_index = Some(2.0);
    parsed.meta.rating = Some(10.0);

    let updated = replace_opf_metadata_block(
        raw_opf,
        &parsed.meta,
        None,
        &parsed.unique_identifier_id,
        &parsed.package_version,
    );
    assert!(updated.contains("custom:keep_me"));
    assert!(updated.contains("<meta name=\"calibre:rating\" content=\"10\"/>"));

    let reparsed = parse_opf_xml(updated.as_bytes(), BookFormatExt::Epub, "Fallback");
    assert_eq!(reparsed.meta.title, "三体");
    assert_eq!(reparsed.meta.series_index, Some(2.0));
    assert_eq!(reparsed.meta.rating, Some(10.0));

    let epub3_opf = replace_opf_metadata_block(
        &raw_opf.replace("version=\"2.0\"", "version=\"3.0\""),
        &parsed.meta,
        None,
        "bookid",
        "3.0",
    );
    assert!(epub3_opf.contains("belongs-to-collection"));
    let reparsed_epub3 = parse_opf_xml(epub3_opf.as_bytes(), BookFormatExt::Epub, "Fallback");
    assert_eq!(
        reparsed_epub3.meta.series.as_deref(),
        Some("Remembrance of Earth's Past")
    );
    assert_eq!(reparsed_epub3.meta.series_index, Some(2.0));
    assert_eq!(reparsed_epub3.meta.rating, Some(10.0));
}

#[test]
fn epub_packager_and_random_access_reader_work() {
    struct CountingReader<R> {
        inner: R,
        bytes_read: Arc<AtomicUsize>,
    }
    impl<R: Read> Read for CountingReader<R> {
        fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
            let n = self.inner.read(buf)?;
            self.bytes_read.fetch_add(n, AtomicOrdering::Relaxed);
            Ok(n)
        }
    }
    impl<R: Seek> Seek for CountingReader<R> {
        fn seek(&mut self, pos: SeekFrom) -> std::io::Result<u64> {
            self.inner.seek(pos)
        }
    }

    let meta = PackBookMetadata {
        identifier: None,
        title: "测试小说".into(),
        author: "佚名作者".into(),
        description: Some("一段测试简介".into()),
        cover_bytes: Some((make_png(2, 3), "image/png".into())),
    };
    let chapters = vec![
        PackChapterInput {
            title: "第一章 启程".into(),
            content: "正文第一段\n正文第二段".into(),
        },
        PackChapterInput {
            title: "第二章 归途".into(),
            content: "结局段落".into(),
        },
    ];
    let epub_bytes = package_chapters_to_epub(&meta, &chapters, None).unwrap();
    let parsed = parse_book_bytes(BookFormatExt::Epub, &epub_bytes, "Fallback").unwrap();
    assert_eq!(parsed.metadata.title, "测试小说");
    assert_eq!(parsed.metadata.authors, vec!["佚名作者"]);
    assert_eq!(parsed.toc.len(), 2);
    assert_eq!(parsed.cover.as_ref().unwrap().mime_type, "image/png");

    let counter = Arc::new(AtomicUsize::new(0));
    let css = read_epub_resource_from_reader(
        CountingReader {
            inner: Cursor::new(epub_bytes.as_slice()),
            bytes_read: Arc::clone(&counter),
        },
        "/OEBPS/style.css",
    )
    .unwrap();
    assert!(css.mime_type.starts_with("text/css"));
    assert!(String::from_utf8_lossy(&css.bytes).contains("font-family"));
    assert!(counter.load(AtomicOrdering::Relaxed) < epub_bytes.len());
}

#[test]
fn fb2_parser_extracts_metadata_sequence_cover_and_chapters() {
    use base64::Engine;
    let png = make_png(2, 3);
    let b64_png = base64::engine::general_purpose::STANDARD.encode(&png);
    let fb2 = format!(
        r##"<?xml version="1.0" encoding="UTF-8"?>
<FictionBook xmlns="http://www.gribuser.ru/xml/fictionbook/2.0" xmlns:l="http://www.w3.org/1999/xlink">
  <description>
    <title-info>
      <genre>sf_space</genre>
      <author><first-name>Isaac</first-name><last-name>Asimov</last-name></author>
      <book-title>Foundation</book-title>
      <annotation><p>Psychohistory predicts the fall.</p></annotation>
      <date>1951</date>
      <coverpage><image l:href="#cover.png"/></coverpage>
      <lang>en</lang>
      <sequence name="Foundation Cycle" number="1"/>
    </title-info>
    <publish-info>
      <publisher>Gnome Press</publisher>
      <isbn>9780553293357</isbn>
    </publish-info>
  </description>
  <body>
    <section>
      <title><p>Part I: The Psychohistorians</p></title>
      <p>Hari Seldon arrived on Trantor.</p>
    </section>
    <section>
      <title><p>Part II: The Encyclopedists</p></title>
      <p>Terminus stood at the edge of the Galaxy.</p>
    </section>
  </body>
  <binary id="cover.png" content-type="image/png">{b64_png}</binary>
</FictionBook>"##
    );

    let pkg = parse_book_bytes(BookFormatExt::Fb2, fb2.as_bytes(), "Fallback").unwrap();
    assert_eq!(pkg.metadata.title, "Foundation");
    assert_eq!(pkg.metadata.authors, vec!["Isaac Asimov"]);
    assert_eq!(pkg.metadata.series.as_deref(), Some("Foundation Cycle"));
    assert_eq!(pkg.metadata.series_index, Some(1.0));
    assert_eq!(pkg.metadata.isbn.as_deref(), Some("9780553293357"));
    assert_eq!(pkg.metadata.publisher.as_deref(), Some("Gnome Press"));
    assert!(pkg.cover.is_some());
    assert_eq!(pkg.toc.len(), 2);

    let (ch1, total) = read_book_text_chapter_bytes(BookFormatExt::Fb2, fb2.as_bytes(), 1).unwrap();
    assert_eq!(total, 2);
    assert!(ch1.title.contains("Encyclopedists"));
    assert!(ch1.content.contains("Terminus"));
}

fn build_synthetic_mobi(title: &str, author: &str, html_body: &str, cover: &[u8]) -> Vec<u8> {
    // Build a synthetic uncompressed (compression = 1) MOBI/AZW3 PalmDB container with EXTH + image record
    let mut rec0 = vec![0u8; 16 + 232];
    rec0[0..2].copy_from_slice(&1u16.to_be_bytes()); // uncompressed
    rec0[4..8].copy_from_slice(&(html_body.len() as u32).to_be_bytes());
    rec0[8..10].copy_from_slice(&1u16.to_be_bytes()); // 1 text record
    rec0[10..12].copy_from_slice(&4096u16.to_be_bytes());

    // MOBI header at offset 16 (length 232)
    rec0[16..20].copy_from_slice(b"MOBI");
    rec0[20..24].copy_from_slice(&232u32.to_be_bytes());
    rec0[24..28].copy_from_slice(&2u32.to_be_bytes()); // book type
    rec0[28..32].copy_from_slice(&65001u32.to_be_bytes()); // UTF-8
    rec0[108..112].copy_from_slice(&2u32.to_be_bytes()); // first image record = index 2
    rec0[128..132].copy_from_slice(&0x40u32.to_be_bytes()); // EXTH flag set

    // EXTH block right after 16 + 232 = 248
    let mut exth_records = Vec::new();
    // 100 = author
    let author_b = author.as_bytes();
    exth_records.extend_from_slice(&100u32.to_be_bytes());
    exth_records.extend_from_slice(&((8 + author_b.len()) as u32).to_be_bytes());
    exth_records.extend_from_slice(author_b);
    // 201 = cover offset (0 -> record 2)
    exth_records.extend_from_slice(&201u32.to_be_bytes());
    exth_records.extend_from_slice(&12u32.to_be_bytes());
    exth_records.extend_from_slice(&0u32.to_be_bytes());

    let exth_total_len = 12 + exth_records.len();
    let pad = (4 - (exth_total_len % 4)) % 4;
    rec0.extend_from_slice(b"EXTH");
    rec0.extend_from_slice(&((exth_total_len + pad) as u32).to_be_bytes());
    rec0.extend_from_slice(&2u32.to_be_bytes());
    rec0.extend_from_slice(&exth_records);
    rec0.resize(rec0.len() + pad, 0);

    let full_title_off = rec0.len() as u32;
    let title_b = title.as_bytes();
    rec0.extend_from_slice(title_b);
    rec0[84..88].copy_from_slice(&full_title_off.to_be_bytes());
    rec0[88..92].copy_from_slice(&(title_b.len() as u32).to_be_bytes());

    let rec1 = html_body.as_bytes().to_vec();
    let rec2 = cover.to_vec();

    // PalmDB header (78 bytes) + 3 * 8-byte record entries = 102 bytes
    let mut pdb = vec![0u8; 78];
    pdb[0..4].copy_from_slice(b"MOBI");
    pdb[60..68].copy_from_slice(b"BOOKMOBI");
    pdb[76..78].copy_from_slice(&3u16.to_be_bytes());

    let off0 = 78 + 3 * 8;
    let off1 = off0 + rec0.len();
    let off2 = off1 + rec1.len();
    for (idx, off) in [off0, off1, off2].into_iter().enumerate() {
        pdb.extend_from_slice(&(off as u32).to_be_bytes());
        pdb.extend_from_slice(&(idx as u32).to_be_bytes());
    }
    pdb.extend_from_slice(&rec0);
    pdb.extend_from_slice(&rec1);
    pdb.extend_from_slice(&rec2);
    pdb
}

#[test]
fn mobi_and_azw3_parser_extracts_exth_cover_and_pagebreak_chapters() {
    let html = "<html><body><h1>Chapter 1 Dawn</h1><p>First scene.</p><mbp:pagebreak/><h1>Chapter 2 Dusk</h1><p>Second scene.</p></body></html>";
    let png = make_png(2, 3);
    let mobi_bytes = build_synthetic_mobi("Solaris", "Stanislaw Lem", html, &png);

    for fmt in [BookFormatExt::Mobi, BookFormatExt::Azw3] {
        let pkg = parse_book_bytes(fmt, &mobi_bytes, "Fallback").unwrap();
        assert_eq!(pkg.metadata.format, fmt);
        assert_eq!(pkg.metadata.title, "Solaris");
        assert_eq!(pkg.metadata.authors, vec!["Stanislaw Lem"]);
        assert_eq!(pkg.cover.as_ref().unwrap().mime_type, "image/png");
        assert_eq!(pkg.toc.len(), 2);

        let toc = list_book_toc_bytes(fmt, &mobi_bytes).unwrap();
        assert_eq!(toc.len(), 2);
        let (ch, total) = read_book_text_chapter_bytes(fmt, &mobi_bytes, 1).unwrap();
        assert_eq!(total, 2);
        assert!(ch.title.contains("Chapter 2"));
        assert!(ch.content.contains("Second scene."));
    }
}

#[test]
fn dedicated_book_sidecar_detection_respects_single_book_and_generic_folders() {
    let tmp = tempfile::TempDir::new().unwrap();
    let root = tmp.path();
    let dedicated = root.join("Dune (101)");
    std::fs::create_dir_all(&dedicated).unwrap();
    let mobi = dedicated.join("Dune.mobi");
    std::fs::write(&mobi, b"dummy").unwrap();
    std::fs::write(dedicated.join("metadata.opf"), b"<package/>").unwrap();
    std::fs::write(dedicated.join("cover.jpg"), b"\xFF\xD8\xFF").unwrap();

    assert!(is_dedicated_book_dir(&dedicated, Some(root)));
    assert_eq!(
        find_existing_sidecar_opf(&mobi, Some(root)),
        Some(dedicated.join("metadata.opf"))
    );
    assert_eq!(
        find_sidecar_cover(&mobi, Some(root)),
        Some(dedicated.join("cover.jpg"))
    );

    // Adding a second book disables shared directory-level `metadata.opf` / `cover.jpg` attribution
    std::fs::write(dedicated.join("Dune Messiah.fb2"), b"dummy2").unwrap();
    assert!(!is_dedicated_book_dir(&dedicated, Some(root)));
    assert_eq!(find_existing_sidecar_opf(&mobi, Some(root)), None);
}

#[test]
fn comic_info_non_destructive_roundtrip_and_image_dir_probing() {
    let original_xml = br#"<?xml version="1.0" encoding="utf-8"?>
<ComicInfo>
  <Series>Chainsaw Man</Series>
  <Volume>1.5</Volume>
  <Number>12.5</Number>
  <Count>97</Count>
  <Inker>Tatsuki Fujimoto</Inker>
  <Colorist>Studio Color</Colorist>
  <AgeRating>Mature 17+</AgeRating>
  <Characters>Denji, Power</Characters>
  <Web>https://manga.example/csm</Web>
  <CustomVendorTag>keep-me</CustomVendorTag>
  <Pages>
    <Page Image="0" Type="FrontCover" ImageWidth="1200" ImageHeight="1800" />
    <Page Image="1" DoublePage="True" ImageWidth="2400" ImageHeight="1800" />
  </Pages>
</ComicInfo>"#;

    let mut meta = parse_comic_info_xml(original_xml).expect("valid ComicInfo.xml");
    assert_eq!(meta.volume, Some(1.5));
    assert_eq!(meta.number, Some(12.5));
    assert_eq!(meta.count, Some(97));
    assert_eq!(meta.inkers, vec!["Tatsuki Fujimoto"]);
    assert_eq!(meta.age_rating.as_deref(), Some("Mature 17+"));
    assert_eq!(meta.characters, vec!["Denji", "Power"]);

    // Simulate an editor that only knows basic fields (`ComicInfoMeta` without touching inkers/custom tags)
    let minimal = ComicInfoMeta {
        series: Some("电锯人".into()),
        volume: meta.volume,
        number: meta.number,
        page_count: Some(2),
        reading_direction: Some(ComicReadingDirection::Rtl),
        ..Default::default()
    };
    let rendered = render_comic_info_xml_preserving(&minimal, Some(original_xml), None);
    assert!(rendered.contains("<Series>电锯人</Series>"));
    assert!(rendered.contains("<Volume>1.5</Volume>"));
    assert!(rendered.contains("<Inker>Tatsuki Fujimoto</Inker>"));
    assert!(rendered.contains("<Colorist>Studio Color</Colorist>"));
    assert!(rendered.contains("<AgeRating>Mature 17+</AgeRating>"));
    assert!(rendered.contains("<Characters>Denji, Power</Characters>"));
    assert!(rendered.contains("<Web>https://manga.example/csm</Web>"));
    assert!(rendered.contains("<CustomVendorTag>keep-me</CustomVendorTag>"));
    assert!(rendered.contains("Type=\"FrontCover\""));
    assert!(rendered.contains("DoublePage=\"True\""));

    // Updating extended fields directly replaces them cleanly without duplication
    meta.age_rating = Some("Teen".into());
    let updated_ar = render_comic_info_xml_preserving(&meta, Some(original_xml), None);
    assert!(updated_ar.contains("<AgeRating>Teen</AgeRating>"));
    assert!(!updated_ar.contains("Mature 17+"));

    // Fractional natural sort & filename parser + ComicFormatExt coverage
    assert_eq!(natural_cmp("ch_1.5.jpg", "ch_2.jpg"), Ordering::Less);
    assert_eq!(natural_cmp("ch_2.jpg", "ch_10.jpg"), Ordering::Less);
    assert_eq!(
        ComicFormatExt::from_extension("cbr"),
        Some(ComicFormatExt::Cbr)
    );
    assert_eq!(
        ComicFormatExt::from_extension("cb7"),
        Some(ComicFormatExt::Cb7)
    );
    let parsed_fn = parse_comic_chapter_filename("[Group] Frieren - Vol.03 Ch.24.5.cb7", 1);
    assert_eq!(parsed_fn.inferred_series.as_deref(), Some("Frieren"));
    assert_eq!(parsed_fn.kind, ComicChapterKind::Chapter);
    assert_eq!(parsed_fn.volume_number, Some(3.0));
    assert_eq!(parsed_fn.number, 24.5);

    // Image directory comic detection + header-only dimension probing
    let tmp = tempfile::TempDir::new().unwrap();
    let ch_dir = tmp.path().join("Chapter 01");
    std::fs::create_dir_all(&ch_dir).unwrap();
    let png = make_png(2, 3);
    std::fs::write(ch_dir.join("001.png"), &png).unwrap();
    std::fs::write(ch_dir.join("002.png"), &png).unwrap();
    assert!(is_comic_image_dir(&ch_dir));
    let pages = probe_comic_pages(&ch_dir).unwrap();
    assert_eq!(pages.len(), 2);
    assert_eq!(pages[0].width, Some(2));
    assert_eq!(pages[0].height, Some(3));
}

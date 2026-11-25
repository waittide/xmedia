use std::path::Path;

use super::guess_comic_from_filename;

#[test]
fn comic_filename_inference_ignores_generic_parent_dir_and_parses_series_chapter() {
    let (series, number, volume) =
        guess_comic_from_filename(Path::new("/data/samples/comic/葬送的芙莉莲_第01话.cbz"));
    assert_eq!(series.as_deref(), Some("葬送的芙莉莲"));
    assert_eq!(number.as_deref(), Some("1"));
    assert_eq!(volume, None);

    let (series2, number2, volume2) = guess_comic_from_filename(Path::new(
        "/data/samples/comic/[九井谅子] 迷宫饭 Vol.02 Ch.14.cbz",
    ));
    assert_eq!(series2.as_deref(), Some("迷宫饭"));
    assert_eq!(volume2, Some(2.0));
    assert_eq!(number2.as_deref(), Some("14"));

    // Bare chapter filename inside a dedicated series directory uses that series directory
    let (series3, number3, _) =
        guess_comic_from_filename(Path::new("/data/manga/孤独摇滚/第05话.cbz"));
    assert_eq!(series3.as_deref(), Some("孤独摇滚"));
    assert_eq!(number3.as_deref(), Some("5"));

    // Bare chapter filename directly inside generic `comic` bucket does NOT set series = "comic"
    let (series4, number4, _) = guess_comic_from_filename(Path::new("/data/samples/comic/003.cbz"));
    assert_eq!(series4, None);
    assert_eq!(number4.as_deref(), Some("3"));
}

#[tokio::test]
async fn epub_manga_opf_rendition_detection_and_ocf_mimetype_preservation() {
    use std::io::Write;
    use zip::write::SimpleFileOptions;
    use zip::{CompressionMethod, ZipArchive, ZipWriter};

    use super::CbzComicInspector;
    use crate::domain::media::{ComicMetaPatch, MediaMetadataPatch, MediaMetadataPayload};
    use crate::domain::ports::MediaInspectorPort;

    let tmp = tempfile::tempdir().unwrap();
    let epub_path = tmp.path().join("テスト条漫.epub");
    {
        let f = std::fs::File::create(&epub_path).unwrap();
        let mut zw = ZipWriter::new(f);
        let stored = SimpleFileOptions::default().compression_method(CompressionMethod::Stored);
        let deflated = SimpleFileOptions::default().compression_method(CompressionMethod::Deflated);
        zw.start_file("mimetype", stored).unwrap();
        zw.write_all(b"application/epub+zip").unwrap();
        zw.start_file("OEBPS/content.opf", deflated).unwrap();
        zw.write_all(
            r#"<?xml version="1.0" encoding="UTF-8"?>
<package xmlns="http://www.idpf.org/2007/opf" version="3.0">
  <metadata xmlns:dc="http://purl.org/dc/elements/1.1/">
    <dc:title>縦スクロール漫画</dc:title>
    <dc:creator>佐藤太郎</dc:creator>
    <meta property="rendition:flow">scrolled-continuous</meta>
  </metadata>
  <manifest><item id="p1" href="img/1.png" media-type="image/png"/></manifest>
  <spine><itemref idref="p1"/></spine>
</package>"#
                .as_bytes(),
        )
        .unwrap();
        zw.start_file("OEBPS/img/1.png", stored).unwrap();
        zw.write_all(b"\x89PNG\r\n\x1a\nfake").unwrap();
        zw.finish().unwrap();
    }

    let inspector = CbzComicInspector;
    let payload = inspector.inspect_file(&epub_path).await.unwrap();
    let MediaMetadataPayload::Comic(meta) = payload else {
        panic!("expected comic payload");
    };
    assert_eq!(meta.format, "epub");
    assert_eq!(meta.reading_direction, "webtoon");
    assert_eq!(meta.writers, vec!["佐藤太郎"]);
    assert_eq!(meta.page_count, 1);

    // Writing ComicInfo.xml into EPUB manga preserves uncompressed `mimetype` as entry #0
    inspector
        .write_metadata(
            &epub_path,
            &MediaMetadataPatch::Comic(ComicMetaPatch {
                series: Some("縦スクロール漫画".into()),
                number: Some("1".into()),
                ..Default::default()
            }),
        )
        .await
        .unwrap();

    let rf = std::fs::File::open(&epub_path).unwrap();
    let mut za = ZipArchive::new(rf).unwrap();
    let first = za.by_index(0).unwrap();
    assert_eq!(first.name().unwrap(), "mimetype");
    assert_eq!(first.compression(), CompressionMethod::Stored);
}

#[tokio::test]
async fn write_metadata_preserves_pages_extended_comicinfo_elements_and_fractional_volume() {
    use std::io::{Read, Write};
    use zip::write::SimpleFileOptions;
    use zip::{CompressionMethod, ZipArchive, ZipWriter};

    use super::CbzComicInspector;
    use crate::domain::media::{ComicMetaPatch, MediaMetadataPatch, MediaMetadataPayload};
    use crate::domain::ports::MediaInspectorPort;

    let tmp = tempfile::tempdir().unwrap();
    let cbz_path = tmp.path().join("frieren_v1.5.cbz");
    let initial_xml = r#"<?xml version="1.0" encoding="utf-8"?>
<ComicInfo>
  <Series>Old Series</Series>
  <Volume>1.5</Volume>
  <Count>12</Count>
  <Month>9</Month>
  <Day>29</Day>
  <CoverArtist>Abe Tsukasa</CoverArtist>
  <Colorist>Color Studio</Colorist>
  <Inker>Ink Person</Inker>
  <Letterer>Letter Person</Letterer>
  <Characters>Frieren, Fern</Characters>
  <AgeRating>Teen</AgeRating>
  <Web>https://example.com/frieren</Web>
  <Pages>
    <Page Image="0" Type="FrontCover" ImageWidth="800" ImageHeight="1200" />
    <Page Image="1" DoublePage="true" ImageWidth="1600" ImageHeight="1200" />
  </Pages>
</ComicInfo>"#;
    {
        let f = std::fs::File::create(&cbz_path).unwrap();
        let mut zw = ZipWriter::new(f);
        let stored = SimpleFileOptions::default().compression_method(CompressionMethod::Stored);
        zw.start_file("001.png", stored).unwrap();
        zw.write_all(b"\x89PNG\r\n\x1a\nfake1").unwrap();
        zw.start_file("002.png", stored).unwrap();
        zw.write_all(b"\x89PNG\r\n\x1a\nfake2").unwrap();
        zw.start_file("ComicInfo.xml", stored).unwrap();
        zw.write_all(initial_xml.as_bytes()).unwrap();
        zw.finish().unwrap();
    }

    let inspector = CbzComicInspector;
    let updated = inspector
        .write_metadata(
            &cbz_path,
            &MediaMetadataPatch::Comic(ComicMetaPatch {
                series: Some("葬送的芙莉莲".into()),
                title: Some("番外篇".into()),
                ..Default::default()
            }),
        )
        .await
        .unwrap();
    let MediaMetadataPayload::Comic(c) = updated else {
        panic!("expected comic payload");
    };
    assert_eq!(c.series.as_deref(), Some("葬送的芙莉莲"));
    assert_eq!(c.volume, Some(1.5));

    let rf = std::fs::File::open(&cbz_path).unwrap();
    let mut za = ZipArchive::new(rf).unwrap();
    let mut out_xml = String::new();
    za.by_name("ComicInfo.xml")
        .unwrap()
        .read_to_string(&mut out_xml)
        .unwrap();

    assert!(out_xml.contains("<Volume>1.5</Volume>"));
    assert!(out_xml.contains("<Count>12</Count>"));
    assert!(out_xml.contains("<Month>9</Month>"));
    assert!(out_xml.contains("<Day>29</Day>"));
    assert!(out_xml.contains("<CoverArtist>Abe Tsukasa</CoverArtist>"));
    assert!(out_xml.contains("<Colorist>Color Studio</Colorist>"));
    assert!(out_xml.contains("<Inker>Ink Person</Inker>"));
    assert!(out_xml.contains("<Letterer>Letter Person</Letterer>"));
    assert!(out_xml.contains("<Characters>Frieren, Fern</Characters>"));
    assert!(out_xml.contains("<AgeRating>Teen</AgeRating>"));
    assert!(out_xml.contains("<Web>https://example.com/frieren</Web>"));
    assert!(out_xml.contains("<Pages>"));
    assert!(out_xml.contains("DoublePage=\"true\""));
}

#[tokio::test]
async fn comic_image_directory_inspect_write_metadata_and_artwork_roundtrip() {
    use super::CbzComicInspector;
    use crate::domain::media::{ComicMetaPatch, MediaMetadataPatch, MediaMetadataPayload};
    use crate::domain::ports::MediaInspectorPort;

    let tmp = tempfile::tempdir().unwrap();
    let series_dir = tmp.path().join("葬送的芙莉莲");
    let ch_dir = series_dir.join("第07话");
    std::fs::create_dir_all(&ch_dir).unwrap();
    let png_bytes = b"\x89PNG\r\n\x1a\nfake";
    std::fs::write(ch_dir.join("001.png"), png_bytes).unwrap();
    std::fs::write(ch_dir.join("002.png"), png_bytes).unwrap();

    let inspector = CbzComicInspector;
    assert!(inspector.supports_directory(&ch_dir));
    assert!(!inspector.supports_directory(&series_dir));

    let payload = inspector.inspect_file(&ch_dir).await.unwrap();
    let MediaMetadataPayload::Comic(meta) = payload else {
        panic!("expected comic payload");
    };
    assert_eq!(meta.format, "folder");
    assert_eq!(meta.series.as_deref(), Some("葬送的芙莉莲"));
    assert_eq!(meta.number.as_deref(), Some("7"));
    assert_eq!(meta.page_count, 2);
    assert!(!meta.has_comic_info_xml);
    assert!(meta.has_cover);

    let updated = inspector
        .write_metadata(
            &ch_dir,
            &MediaMetadataPatch::Comic(ComicMetaPatch {
                title: Some("魂之眠地".into()),
                writers: Some(vec!["山田钟人".into()]),
                ..Default::default()
            }),
        )
        .await
        .unwrap();
    let MediaMetadataPayload::Comic(meta2) = updated else {
        panic!("expected comic payload");
    };
    assert!(meta2.has_comic_info_xml);
    assert_eq!(meta2.title.as_deref(), Some("魂之眠地"));
    assert!(ch_dir.join("ComicInfo.xml").is_file());

    let art = inspector.read_artwork(&ch_dir, None).await.unwrap();
    assert!(art.is_some());

    inspector
        .write_artwork(&ch_dir, png_bytes, "image/png", "cover")
        .await
        .unwrap();
    assert!(ch_dir.join("000_cover.png").is_file());
}

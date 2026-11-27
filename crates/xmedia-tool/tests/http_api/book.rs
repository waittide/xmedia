use std::io::{Read, Write};

use tempfile::TempDir;
use xmedia_core::domain::library::MediaKind;
use xmedia_tool::application::ports::ArtworkIo;
use xmedia_tool::application::scraping::ApplyScrapeInput;
use xmedia_tool::application::workbench::{PreviewRenameOptions, WorkspaceArtworkOutput};
use xmedia_tool::domain::media::{BookMetaPatch, MediaMetadataPatch, MediaMetadataPayload};
use xmedia_tool::domain::scraper::{ScrapeTargetKind, ScrapedMetadata};
use xmedia_tool::infra::atomic_fs::find_companion_sidecars;
use xmedia_tool::{XMediaToolApp, XMediaToolConfig};
use zip::write::SimpleFileOptions;
use zip::{CompressionMethod, ZipArchive, ZipWriter};

use crate::common::{TINY_PNG, spawn_mock_image_server, write_minimal_epub};

#[test]
fn companion_sidecars_match_opf_and_reject_prefix_collisions() {
    let tmp = TempDir::new().unwrap();
    let dir = tmp.path();

    let mp4 = dir.join("01.mp4");
    std::fs::write(&mp4, b"v").unwrap();
    std::fs::write(dir.join("01.zh.srt"), b"s1").unwrap();
    std::fs::write(dir.join("01-poster.jpg"), b"p1").unwrap();
    std::fs::write(dir.join("01-02.srt"), b"s2").unwrap();
    std::fs::write(dir.join("010.nfo"), b"n").unwrap();

    let video_sidecars: Vec<String> = find_companion_sidecars(&mp4)
        .into_iter()
        .filter_map(|(p, _)| p.file_name().and_then(|n| n.to_str()).map(String::from))
        .collect();
    assert!(video_sidecars.contains(&"01.zh.srt".to_string()));
    assert!(video_sidecars.contains(&"01-poster.jpg".to_string()));
    assert!(!video_sidecars.contains(&"01-02.srt".to_string()));
    assert!(!video_sidecars.contains(&"010.nfo".to_string()));

    let pdf = dir.join("CleanCode.pdf");
    std::fs::write(&pdf, b"%PDF").unwrap();
    std::fs::write(dir.join("CleanCode.opf"), b"<package/>").unwrap();
    std::fs::write(dir.join("CleanCode.webp"), b"img").unwrap();
    let pdf_sidecars: Vec<String> = find_companion_sidecars(&pdf)
        .into_iter()
        .filter_map(|(p, _)| p.file_name().and_then(|n| n.to_str()).map(String::from))
        .collect();
    assert!(pdf_sidecars.contains(&"CleanCode.opf".to_string()));
    assert!(pdf_sidecars.contains(&"CleanCode.webp".to_string()));
}

#[tokio::test]
async fn epub_preserves_rendition_uid_and_updates_cover_manifest_media_type() {
    let tmp = TempDir::new().unwrap();
    let root = tmp.path().join("books");
    std::fs::create_dir_all(&root).unwrap();
    let epub = root.join("fixed.epub");

    {
        let f = std::fs::File::create(&epub).unwrap();
        let mut zip = ZipWriter::new(f);
        let stored = SimpleFileOptions::default().compression_method(CompressionMethod::Stored);
        let deflated = SimpleFileOptions::default().compression_method(CompressionMethod::Deflated);
        zip.start_file("mimetype", stored).unwrap();
        zip.write_all(b"application/epub+zip").unwrap();
        zip.start_file("META-INF/container.xml", deflated).unwrap();
        zip.write_all(
            br#"<?xml version="1.0" encoding="UTF-8"?>
<container version="1.0" xmlns="urn:oasis:names:tc:opendocument:xmlns:container">
  <rootfiles>
    <rootfile full-path="OEBPS/content.opf" media-type="application/oebps-package+xml"/>
  </rootfiles>
</container>"#,
        )
        .unwrap();
        zip.start_file("OEBPS/content.opf", deflated).unwrap();
        zip.write_all(
            br#"<?xml version="1.0" encoding="UTF-8"?>
<package xmlns="http://www.idpf.org/2007/opf" version="3.0" unique-identifier="pub-id">
  <metadata xmlns:dc="http://purl.org/dc/elements/1.1/">
    <dc:identifier id="pub-id">urn:uuid:aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee</dc:identifier>
    <dc:title>Original</dc:title>
    <meta property="rendition:layout">pre-paginated</meta>
    <meta name="cover" content="cover-img"/>
  </metadata>
  <manifest>
    <item id="cover-img" href="images/cover.png" media-type="image/png" properties="cover-image"/>
    <item id="chap1" href="chap1.xhtml" media-type="application/xhtml+xml"/>
  </manifest>
  <spine><itemref idref="chap1"/></spine>
</package>"#,
        )
        .unwrap();
        zip.start_file("OEBPS/images/cover.png", deflated).unwrap();
        zip.write_all(TINY_PNG).unwrap();
        zip.start_file("OEBPS/chap1.xhtml", deflated).unwrap();
        zip.write_all(b"<html/>").unwrap();
        zip.finish().unwrap();
    }

    let app = XMediaToolApp::init(XMediaToolConfig {
        data_dir: tmp.path().join("data"),
        initial_roots: vec![root],
        password: None,
        ffmpeg_bin: "__missing_ffmpeg__".into(),
        external_web_dir: None,
    })
    .await
    .unwrap();
    let root_id = app.settings.get_public_settings().await.unwrap().roots[0].id;

    app.metadata
        .write(
            root_id,
            "fixed.epub",
            MediaMetadataPatch::Book(BookMetaPatch {
                title: Some("Updated Title".into()),
                isbn: Some("9787111123456".into()),
                ..Default::default()
            }),
        )
        .await
        .unwrap();

    let fake_jpg = [0xFFu8, 0xD8, 0xFF, 0xE0, 0x00, 0x10, 0x4A, 0x46, 0x49, 0x46];
    app.workspace
        .artwork(
            root_id,
            "fixed.epub",
            Some(MediaKind::Book),
            ArtworkIo::Write {
                slot: "cover",
                bytes: &fake_jpg,
                mime_type: "image/jpeg",
            },
        )
        .await
        .unwrap();

    let f = std::fs::File::open(&epub).unwrap();
    let mut archive = ZipArchive::new(f).unwrap();
    let mut opf_xml = String::new();
    archive
        .by_name("OEBPS/content.opf")
        .unwrap()
        .read_to_string(&mut opf_xml)
        .unwrap();
    assert!(opf_xml.contains("rendition:layout"));
    assert!(opf_xml.contains("pre-paginated"));
    assert!(opf_xml.contains("id=\"pub-id\""));
    assert!(opf_xml.contains("9787111123456"));
    assert!(opf_xml.contains("dcterms:modified"));
    assert!(opf_xml.contains("image/jpeg"));
    assert!(opf_xml.contains("images/cover.jpg"));
    assert!(!opf_xml.contains("image/png"));
}

#[tokio::test]
async fn txt_and_pdf_sidecar_opf_cover_and_rename_roundtrip() {
    let tmp = TempDir::new().unwrap();
    let root = tmp.path().join("docs");
    std::fs::create_dir_all(&root).unwrap();

    let txt = root.join("draft_novel.txt");
    std::fs::write(&txt, "Chapter 1\nHello world.\n").unwrap();

    let mut doc = lopdf::Document::with_version("1.4");
    let pages_id = doc.new_object_id();
    doc.objects.insert(
        pages_id,
        lopdf::Object::Dictionary(lopdf::Dictionary::from_iter([
            ("Type", lopdf::Object::Name(b"Pages".to_vec())),
            ("Count", lopdf::Object::Integer(0)),
            ("Kids", lopdf::Object::Array(Vec::new())),
        ])),
    );
    let catalog_id = doc.add_object(lopdf::Object::Dictionary(lopdf::Dictionary::from_iter([
        ("Type", lopdf::Object::Name(b"Catalog".to_vec())),
        ("Pages", lopdf::Object::Reference(pages_id)),
    ])));
    doc.trailer.set("Root", catalog_id);
    let pdf = root.join("guide.pdf");
    doc.save(&pdf).unwrap();

    let app = XMediaToolApp::init(XMediaToolConfig {
        data_dir: tmp.path().join("data"),
        initial_roots: vec![root.clone()],
        password: None,
        ffmpeg_bin: "__missing_ffmpeg__".into(),
        external_web_dir: None,
    })
    .await
    .unwrap();
    let root_id = app.settings.get_public_settings().await.unwrap().roots[0].id;

    for file in ["draft_novel.txt", "guide.pdf"] {
        let detail = app
            .metadata
            .write(
                root_id,
                file,
                MediaMetadataPatch::Book(BookMetaPatch {
                    title: Some(format!("Title {file}")),
                    authors: Some(vec!["Alice".into()]),
                    publisher: Some("Acme Press".into()),
                    tags: Some(vec!["Tech".into()]),
                    ..Default::default()
                }),
            )
            .await
            .unwrap();
        match &detail.metadata {
            MediaMetadataPayload::Book(b) => assert!(b.writable),
            _ => panic!("expected book payload"),
        }
        app.workspace
            .artwork(
                root_id,
                file,
                Some(MediaKind::Book),
                ArtworkIo::Write {
                    slot: "cover",
                    bytes: TINY_PNG,
                    mime_type: "image/png",
                },
            )
            .await
            .unwrap();
        assert!(matches!(
            app.workspace
                .artwork(
                    root_id,
                    file,
                    Some(MediaKind::Book),
                    ArtworkIo::Read { slot: None },
                )
                .await
                .unwrap(),
            WorkspaceArtworkOutput::Read(Some(_))
        ));
    }
    assert!(root.join("draft_novel.opf").is_file());
    assert!(root.join("draft_novel.png").is_file());
    assert!(root.join("guide.opf").is_file());
    assert!(root.join("guide.png").is_file());

    let plans = app
        .rename
        .preview(
            root_id,
            &["draft_novel.txt".to_string()],
            PreviewRenameOptions {
                template_override: Some("{author}/{title}"),
                preferred_kind: Some(MediaKind::Book),
                ..Default::default()
            },
            None,
        )
        .await
        .unwrap();
    assert_eq!(plans[0].to_rel_path, "Alice/Title draft_novel.txt.txt");
    app.rename.execute(root_id, &plans, None).await.unwrap();
    assert!(root.join("Alice/Title draft_novel.txt.opf").is_file());
    assert!(root.join("Alice/Title draft_novel.txt.png").is_file());
}

#[tokio::test]
async fn book_calibre_metadata_opf_cover_and_epub3_series_subseries_roundtrip() {
    let addr = spawn_mock_image_server().await;

    let tmp = TempDir::new().unwrap();
    let root = tmp.path().join("books");
    let inbox = root.join("inbox");
    std::fs::create_dir_all(&inbox).unwrap();

    write_minimal_epub(&inbox.join("index_nt1.epub"));
    std::fs::write(
        inbox.join("dark_forest.txt"),
        "Chapter 1\nThe Dark Forest.\n",
    )
    .unwrap();

    let mut app = XMediaToolApp::init(XMediaToolConfig {
        data_dir: tmp.path().join("data"),
        initial_roots: vec![root.clone()],
        password: None,
        ffmpeg_bin: "__missing_ffmpeg__".into(),
        external_web_dir: None,
    })
    .await
    .unwrap();
    app.scrape_apply.set_allow_private_artwork_ips(true);
    let root_id = app.settings.get_public_settings().await.unwrap().roots[0].id;

    let epub_patched = app
        .metadata
        .write(
            root_id,
            "inbox/index_nt1.epub",
            MediaMetadataPatch::Book(BookMetaPatch {
                title: Some("新约 魔法禁书目录".into()),
                authors: Some(vec!["镰池和马".into()]),
                series: Some("魔法禁书目录".into()),
                subseries: Some("新约".into()),
                series_index: Some("1".into()),
                published_at: Some("2011-03-10".into()),
                ..Default::default()
            }),
        )
        .await
        .unwrap();
    if let MediaMetadataPayload::Book(b) = &epub_patched.metadata {
        assert_eq!(b.series.as_deref(), Some("魔法禁书目录"));
        assert_eq!(b.subseries.as_deref(), Some("新约"));
        assert_eq!(b.series_index.as_deref(), Some("1"));
    } else {
        panic!("expected book payload");
    }

    let epub_plans = app
        .rename
        .preview(
            root_id,
            &["inbox/index_nt1.epub".to_string()],
            PreviewRenameOptions {
                preferred_kind: Some(MediaKind::Book),
                ..Default::default()
            },
            None,
        )
        .await
        .unwrap();
    assert_eq!(
        epub_plans[0].to_rel_path,
        "镰池和马/魔法禁书目录/新约/Vol.01 新约 魔法禁书目录 (2011)/新约 魔法禁书目录 - 镰池和马.epub"
    );
    app.rename
        .execute(root_id, &epub_plans, None)
        .await
        .unwrap();

    let txt_detail = app
        .scrape_apply
        .apply(ApplyScrapeInput {
            root_id,
            rel_path: "inbox/dark_forest.txt".into(),
            media_kind: MediaKind::Book,
            scraped: ScrapedMetadata {
                provider_id: "openlibrary".into(),
                external_id: "ol-df".into(),
                target: Some(ScrapeTargetKind::Book),
                title: Some("黑暗森林".into()),
                creators: vec!["刘慈欣".into()],
                series_or_album: Some("三体".into()),
                track_or_episode_number: Some(2),
                year: Some(2008),
                cover_url: Some(format!("http://{addr}/df.png")),
                ..Default::default()
            },
            apply_cover: true,
            write_sidecar_lrc: false,
            organize_by_template: None,
            only_fill_missing: false,
            write_album_cover: false,
        })
        .await
        .unwrap();

    let expected_dir = root.join("刘慈欣/三体/Vol.02 黑暗森林 (2008)");
    assert_eq!(
        txt_detail.entry.rel_path,
        "刘慈欣/三体/Vol.02 黑暗森林 (2008)/黑暗森林 - 刘慈欣.txt"
    );
    assert!(expected_dir.join("metadata.opf").is_file());
    assert!(expected_dir.join("cover.png").is_file());
    assert!(!inbox.exists());
    if let MediaMetadataPayload::Book(b) = &txt_detail.metadata {
        assert_eq!(b.series.as_deref(), Some("三体"));
        assert_eq!(b.series_index.as_deref(), Some("2"));
        assert!(b.has_cover);
    } else {
        panic!("expected book payload");
    }

    let cur_rel = txt_detail.entry.rel_path.clone();
    app.metadata
        .write(
            root_id,
            &cur_rel,
            MediaMetadataPatch::Book(BookMetaPatch {
                locked_fields: Some(vec!["title".into(), "series".into(), "series_index".into()]),
                ..Default::default()
            }),
        )
        .await
        .unwrap();

    let rescraped = app
        .scrape_apply
        .apply(ApplyScrapeInput {
            root_id,
            rel_path: cur_rel,
            media_kind: MediaKind::Book,
            scraped: ScrapedMetadata {
                provider_id: "douban".into(),
                external_id: "db-df".into(),
                target: Some(ScrapeTargetKind::Book),
                title: Some("被错误覆盖的书名".into()),
                creators: vec!["刘慈欣".into()],
                series_or_album: Some("错误丛书".into()),
                track_or_episode_number: Some(99),
                publisher_or_studio: Some("重庆出版社".into()),
                ..Default::default()
            },
            apply_cover: false,
            write_sidecar_lrc: false,
            organize_by_template: Some(false),
            only_fill_missing: false,
            write_album_cover: false,
        })
        .await
        .unwrap();

    if let MediaMetadataPayload::Book(b) = &rescraped.metadata {
        assert_eq!(b.title, "黑暗森林");
        assert_eq!(b.series.as_deref(), Some("三体"));
        assert_eq!(b.series_index.as_deref(), Some("2"));
        assert_eq!(b.publisher.as_deref(), Some("重庆出版社"));
        assert_eq!(b.locked_fields, vec!["title", "series", "series_index"]);
    } else {
        panic!("expected book payload");
    }
}

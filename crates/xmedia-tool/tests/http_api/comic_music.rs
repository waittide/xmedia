use std::io::{Read, Write};

use lofty::config::WriteOptions;
use lofty::picture::{MimeType, Picture, PictureType};
use lofty::prelude::{AudioFile, TaggedFileExt};
use lofty::tag::Tag;
use tempfile::TempDir;
use xmedia_core::domain::library::MediaKind;
use xmedia_tool::application::ports::ArtworkIo;
use xmedia_tool::application::scraping::ApplyScrapeInput;
use xmedia_tool::application::workbench::PreviewRenameOptions;
use xmedia_tool::domain::media::{
    ComicMetaPatch, MediaMetadataPatch, MediaMetadataPayload, MusicMetaPatch,
};
use xmedia_tool::domain::scraper::{ScrapeTargetKind, ScrapedMetadata};
use xmedia_tool::{XMediaToolApp, XMediaToolConfig};
use zip::write::SimpleFileOptions;
use zip::{CompressionMethod, ZipArchive, ZipWriter};

use crate::common::{TINY_PNG, spawn_mock_image_server, write_minimal_cbz, write_minimal_wav};

#[tokio::test]
async fn music_id3v2_year_persists_and_replacing_front_cover_keeps_back_cover() {
    let tmp = TempDir::new().unwrap();
    let root = tmp.path().join("music");
    std::fs::create_dir_all(&root).unwrap();
    let wav = root.join("track.wav");
    write_minimal_wav(&wav);

    {
        let mut tagged = lofty::read_from_path(&wav).unwrap();
        let tt = tagged.primary_tag_type();
        if tagged.primary_tag().is_none() {
            tagged.insert_tag(Tag::new(tt));
        }
        let tag = tagged.primary_tag_mut().unwrap();
        tag.push_picture(
            Picture::unchecked(TINY_PNG.to_vec())
                .pic_type(PictureType::CoverBack)
                .mime_type(MimeType::Png)
                .build(),
        );
        tagged.save_to_path(&wav, WriteOptions::default()).unwrap();
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

    let updated = app
        .metadata
        .write(
            root_id,
            "track.wav",
            MediaMetadataPatch::Music(MusicMetaPatch {
                title: Some("Seven Horizons".into()),
                year: Some(2004),
                ..Default::default()
            }),
        )
        .await
        .unwrap();
    if let MediaMetadataPayload::Music(m) = updated.metadata {
        assert_eq!(m.year, Some(2004));
    } else {
        panic!("expected music payload");
    }

    app.workspace
        .artwork(
            root_id,
            "track.wav",
            Some(MediaKind::Music),
            ArtworkIo::Write {
                slot: "cover",
                bytes: TINY_PNG,
                mime_type: "image/png",
            },
        )
        .await
        .unwrap();
    let tagged = lofty::read_from_path(&wav).unwrap();
    let pics = tagged.primary_tag().unwrap().pictures();
    assert!(pics.iter().any(|p| p.pic_type() == PictureType::CoverFront));
    assert!(pics.iter().any(|p| p.pic_type() == PictureType::CoverBack));
}

#[tokio::test]
async fn comic_webtoon_roundtrip_and_template_avoids_duplicate_title() {
    let tmp = TempDir::new().unwrap();
    let root = tmp.path().join("comics");
    std::fs::create_dir_all(&root).unwrap();
    let cbz = root.join("Solo_Ch01.cbz");
    {
        let f = std::fs::File::create(&cbz).unwrap();
        let mut zip = ZipWriter::new(f);
        let opts = SimpleFileOptions::default().compression_method(CompressionMethod::Deflated);
        zip.start_file("001.png", opts).unwrap();
        zip.write_all(TINY_PNG).unwrap();
        zip.start_file("ComicInfo.xml", opts).unwrap();
        zip.write_all(
            br#"<ComicInfo><Series>Solo</Series><Number>1</Number><Format>Webtoon</Format></ComicInfo>"#,
        )
        .unwrap();
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

    let detail = app
        .workspace
        .inspect(root_id, "Solo_Ch01.cbz", Some(MediaKind::Comic))
        .await
        .unwrap();
    if let MediaMetadataPayload::Comic(ref c) = detail.metadata {
        assert_eq!(c.reading_direction, "webtoon");
        let plans = app
            .rename
            .preview(
                root_id,
                &["Solo_Ch01.cbz".to_string()],
                PreviewRenameOptions {
                    template_override: Some("{series}/{series} Ch.{number:03} - {title}"),
                    preferred_kind: Some(MediaKind::Comic),
                    ..Default::default()
                },
                None,
            )
            .await
            .unwrap();
        assert_eq!(plans[0].to_rel_path, "Solo/Solo Ch.001.cbz");
    } else {
        panic!("expected comic payload");
    }

    app.metadata
        .write(
            root_id,
            "Solo_Ch01.cbz",
            MediaMetadataPatch::Comic(ComicMetaPatch {
                title: Some("Awakening".into()),
                ..Default::default()
            }),
        )
        .await
        .unwrap();

    let f = std::fs::File::open(&cbz).unwrap();
    let mut archive = ZipArchive::new(f).unwrap();
    let mut xml = String::new();
    archive
        .by_name("ComicInfo.xml")
        .unwrap()
        .read_to_string(&mut xml)
        .unwrap();
    assert!(xml.contains("<Format>Webtoon</Format>"));
    assert!(xml.contains("<Title>Awakening</Title>"));
}

#[tokio::test]
async fn comic_komga_archive_series_group_and_series_cover_without_polluting_cbz() {
    let addr = spawn_mock_image_server().await;

    let tmp = TempDir::new().unwrap();
    let root = tmp.path().join("manga");
    std::fs::create_dir_all(&root).unwrap();

    let raw_cbz = root.join("JOJO4_Ch01.cbz");
    write_minimal_cbz(&raw_cbz);

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

    let detail = app
        .scrape_apply
        .apply(ApplyScrapeInput {
            root_id,
            rel_path: "JOJO4_Ch01.cbz".into(),
            media_kind: MediaKind::Comic,
            scraped: ScrapedMetadata {
                provider_id: "bangumi".into(),
                external_id: "bgm-jojo4".into(),
                target: Some(ScrapeTargetKind::Comic),
                title: Some("JOJO的奇妙冒险 第4部 不灭钻石".into()),
                creators: vec!["荒木飞吕彦".into()],
                cover_url: Some(format!("http://{addr}/series_cover.png")),
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

    assert_eq!(
        detail.entry.rel_path,
        "JOJO的奇妙冒险 第4部 不灭钻石/JOJO的奇妙冒险 第4部 不灭钻石 Ch.01.cbz"
    );
    assert!(
        root.join("JOJO的奇妙冒险 第4部 不灭钻石/cover.png")
            .is_file()
    );
    if let MediaMetadataPayload::Comic(c) = &detail.metadata {
        assert_eq!(c.page_count, 2);
        assert!(c.has_series_cover);
    } else {
        panic!("expected comic payload");
    }

    let patched = app
        .metadata
        .write(
            root_id,
            &detail.entry.rel_path,
            MediaMetadataPatch::Comic(ComicMetaPatch {
                series: Some("不灭钻石".into()),
                series_group: Some("JOJO的奇妙冒险".into()),
                volume: Some(1.0),
                ..Default::default()
            }),
        )
        .await
        .unwrap();
    if let MediaMetadataPayload::Comic(c) = &patched.metadata {
        assert_eq!(c.series_group.as_deref(), Some("JOJO的奇妙冒险"));
    } else {
        panic!("expected comic payload");
    }

    let plans = app
        .rename
        .preview(
            root_id,
            std::slice::from_ref(&detail.entry.rel_path),
            PreviewRenameOptions {
                preferred_kind: Some(MediaKind::Comic),
                ..Default::default()
            },
            None,
        )
        .await
        .unwrap();
    assert_eq!(plans[0].to_rel_path, "不灭钻石/不灭钻石 Vol.01 Ch.01.cbz");
    app.rename.execute(root_id, &plans, None).await.unwrap();
    assert!(root.join("不灭钻石/cover.png").is_file());
    assert!(!root.join("JOJO的奇妙冒险 第4部 不灭钻石").exists());
}

#[tokio::test]
async fn music_picard_archive_and_opt_in_album_directory_cover() {
    let addr = spawn_mock_image_server().await;

    let tmp = TempDir::new().unwrap();
    let root = tmp.path().join("music");
    std::fs::create_dir_all(&root).unwrap();

    write_minimal_wav(&root.join("t1.wav"));
    write_minimal_wav(&root.join("t2.wav"));

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

    let d1 = app
        .scrape_apply
        .apply(ApplyScrapeInput {
            root_id,
            rel_path: "t1.wav".into(),
            media_kind: MediaKind::Music,
            scraped: ScrapedMetadata {
                provider_id: "itunes".into(),
                external_id: "it-1".into(),
                target: Some(ScrapeTargetKind::MusicTrack),
                title: Some("以父之名".into()),
                creators: vec!["周杰伦".into()],
                series_or_album: Some("叶惠美".into()),
                track_or_episode_number: Some(1),
                year: Some(2003),
                cover_url: Some(format!("http://{addr}/alb.png")),
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
    assert_eq!(d1.entry.rel_path, "周杰伦/叶惠美 (2003)/01 - 以父之名.wav");
    assert!(!root.join("周杰伦/叶惠美 (2003)/cover.png").exists());

    let d2 = app
        .scrape_apply
        .apply(ApplyScrapeInput {
            root_id,
            rel_path: "t2.wav".into(),
            media_kind: MediaKind::Music,
            scraped: ScrapedMetadata {
                provider_id: "itunes".into(),
                external_id: "it-3".into(),
                target: Some(ScrapeTargetKind::MusicTrack),
                title: Some("晴天".into()),
                creators: vec!["周杰伦".into()],
                series_or_album: Some("叶惠美".into()),
                track_or_episode_number: Some(3),
                year: Some(2003),
                cover_url: Some(format!("http://{addr}/alb.png")),
                ..Default::default()
            },
            apply_cover: true,
            write_sidecar_lrc: false,
            organize_by_template: None,
            only_fill_missing: false,
            write_album_cover: true,
        })
        .await
        .unwrap();
    assert_eq!(d2.entry.rel_path, "周杰伦/叶惠美 (2003)/03 - 晴天.wav");
    assert!(root.join("周杰伦/叶惠美 (2003)/cover.png").is_file());
    if let MediaMetadataPayload::Music(m) = &d2.metadata {
        assert!(m.has_embedded_cover);
        assert!(m.has_directory_cover);
    } else {
        panic!("expected music payload");
    }
}

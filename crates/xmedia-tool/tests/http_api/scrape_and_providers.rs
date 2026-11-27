use std::collections::BTreeMap;

use tempfile::TempDir;
use xmedia_core::domain::library::MediaKind;
use xmedia_tool::application::scraping::ApplyScrapeInput;
use xmedia_tool::application::settings::UpdateSettingsInput;
use xmedia_tool::domain::config::ProviderConfigOverride;
use xmedia_tool::domain::media::{
    BookMetaPatch, ComicMetaPatch, MediaMetadataPatch, MediaMetadataPayload, MusicMetaPatch,
};
use xmedia_tool::domain::scraper::{
    FieldSourceBadge, ScrapeTargetKind, ScrapedMetadata, is_compatible_secondary_source,
    merge_scraped_by_priority,
};
use xmedia_tool::{XMediaToolApp, XMediaToolConfig};

use crate::common::{
    spawn_mock_image_server, write_minimal_cbz, write_minimal_epub, write_minimal_wav,
};

#[tokio::test]
async fn scrape_subitem_field_protection_only_fill_missing_and_untimed_lrc_guard() {
    let tmp = TempDir::new().unwrap();
    let root = tmp.path().join("lib");
    std::fs::create_dir_all(&root).unwrap();

    let cbz = root.join("ch01.cbz");
    let wav = root.join("track01.wav");
    let epub = root.join("book.epub");
    write_minimal_cbz(&cbz);
    write_minimal_wav(&wav);
    write_minimal_epub(&epub);

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

    app.metadata
        .write(
            root_id,
            "ch01.cbz",
            MediaMetadataPatch::Comic(ComicMetaPatch {
                title: Some("第一话 犬与电锯".into()),
                number: Some("1".into()),
                volume: Some(1.0),
                ..Default::default()
            }),
        )
        .await
        .unwrap();
    let comic_after = app
        .scrape_apply
        .apply(ApplyScrapeInput {
            root_id,
            rel_path: "ch01.cbz".into(),
            media_kind: MediaKind::Comic,
            scraped: ScrapedMetadata {
                provider_id: "bangumi".into(),
                external_id: "321".into(),
                target: Some(ScrapeTargetKind::Comic),
                title: Some("链锯人".into()),
                creators: vec!["藤本树".into()],
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
    if let MediaMetadataPayload::Comic(c) = comic_after.metadata {
        assert_eq!(c.series.as_deref(), Some("链锯人"));
        assert_eq!(c.title.as_deref(), Some("第一话 犬与电锯"));
        assert_eq!(c.number.as_deref(), Some("1"));
        assert_eq!(c.volume, Some(1.0));
    } else {
        panic!("expected comic payload");
    }

    app.metadata
        .write(
            root_id,
            "track01.wav",
            MediaMetadataPatch::Music(MusicMetaPatch {
                title: Some("晴天".into()),
                artist: Some("周杰伦".into()),
                track_number: Some(3),
                disc_number: Some(1),
                ..Default::default()
            }),
        )
        .await
        .unwrap();
    let music_after_album = app
        .scrape_apply
        .apply(ApplyScrapeInput {
            root_id,
            rel_path: "track01.wav".into(),
            media_kind: MediaKind::Music,
            scraped: ScrapedMetadata {
                provider_id: "itunes".into(),
                external_id: "alb-1".into(),
                target: Some(ScrapeTargetKind::MusicAlbum),
                title: Some("叶惠美".into()),
                series_or_album: Some("叶惠美".into()),
                creators: vec!["Jay Chou".into()],
                year: Some(2003),
                genres: vec!["Mandopop".into()],
                ..Default::default()
            },
            apply_cover: false,
            write_sidecar_lrc: true,
            organize_by_template: Some(false),
            only_fill_missing: false,
            write_album_cover: false,
        })
        .await
        .unwrap();
    if let MediaMetadataPayload::Music(m) = music_after_album.metadata {
        assert_eq!(m.title, "晴天");
        assert_eq!(m.artist, "周杰伦");
        assert_eq!(m.album, "叶惠美");
        assert_eq!(m.album_artist.as_deref(), Some("Jay Chou"));
        assert_eq!(m.track_number, Some(3));
        assert_eq!(m.year, Some(2003));
    } else {
        panic!("expected music payload");
    }

    app.scrape_apply
        .apply(ApplyScrapeInput {
            root_id,
            rel_path: "track01.wav".into(),
            media_kind: MediaKind::Music,
            scraped: ScrapedMetadata {
                provider_id: "lrclib".into(),
                external_id: "lrc-1".into(),
                target: Some(ScrapeTargetKind::Lyrics),
                title: Some("WRONG TITLE".into()),
                creators: vec!["WRONG ARTIST".into()],
                synced_lyrics: Some("纯文本无时间戳歌词".into()),
                ..Default::default()
            },
            apply_cover: false,
            write_sidecar_lrc: true,
            organize_by_template: Some(false),
            only_fill_missing: false,
            write_album_cover: false,
        })
        .await
        .unwrap();
    assert!(!root.join("track01.lrc").exists());

    let music_after_lrc = app
        .scrape_apply
        .apply(ApplyScrapeInput {
            root_id,
            rel_path: "track01.wav".into(),
            media_kind: MediaKind::Music,
            scraped: ScrapedMetadata {
                provider_id: "lrclib".into(),
                external_id: "lrc-2".into(),
                target: Some(ScrapeTargetKind::Lyrics),
                title: Some("WRONG TITLE".into()),
                synced_lyrics: Some("[00:12.34]故事的小黄花".into()),
                ..Default::default()
            },
            apply_cover: false,
            write_sidecar_lrc: true,
            organize_by_template: Some(false),
            only_fill_missing: false,
            write_album_cover: false,
        })
        .await
        .unwrap();
    assert!(root.join("track01.lrc").is_file());
    if let MediaMetadataPayload::Music(m) = music_after_lrc.metadata {
        assert_eq!(m.title, "晴天");
        assert_eq!(m.sidecar_lrc.as_deref(), Some("[00:12.34]故事的小黄花"));
    } else {
        panic!("expected music payload");
    }

    app.metadata
        .write(
            root_id,
            "book.epub",
            MediaMetadataPatch::Book(BookMetaPatch {
                publisher: Some("Local Publisher".into()),
                ..Default::default()
            }),
        )
        .await
        .unwrap();
    let book_after = app
        .scrape_apply
        .apply(ApplyScrapeInput {
            root_id,
            rel_path: "book.epub".into(),
            media_kind: MediaKind::Book,
            scraped: ScrapedMetadata {
                provider_id: "openlibrary".into(),
                external_id: "OL1".into(),
                target: Some(ScrapeTargetKind::Book),
                title: Some("Remote Title".into()),
                creators: vec!["Bob".into()],
                publisher_or_studio: Some("Remote Publisher".into()),
                summary: Some("Filled Summary".into()),
                isbn: Some("9780134494166".into()),
                ..Default::default()
            },
            apply_cover: false,
            write_sidecar_lrc: false,
            organize_by_template: Some(false),
            only_fill_missing: true,
            write_album_cover: false,
        })
        .await
        .unwrap();
    if let MediaMetadataPayload::Book(b) = book_after.metadata {
        assert_eq!(b.title, "Draft Title");
        assert_eq!(b.publisher.as_deref(), Some("Local Publisher"));
        assert_eq!(b.description.as_deref(), Some("Filled Summary"));
        assert_eq!(b.isbn.as_deref(), Some("9780134494166"));
    } else {
        panic!("expected book payload");
    }
}

#[tokio::test]
async fn scrape_organize_by_template_toggle_and_season_zero_compatibility() {
    let addr = spawn_mock_image_server().await;

    let tmp = TempDir::new().unwrap();
    let root = tmp.path().join("media");
    let inbox = root.join("inbox");
    std::fs::create_dir_all(&inbox).unwrap();

    std::fs::write(inbox.join("movie_raw.mkv"), b"dummy-mkv").unwrap();
    write_minimal_epub(&inbox.join("book_raw.epub"));

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

    let vid = app
        .scrape_apply
        .apply(ApplyScrapeInput {
            root_id,
            rel_path: "inbox/movie_raw.mkv".into(),
            media_kind: MediaKind::Video,
            scraped: ScrapedMetadata {
                provider_id: "tmdb".into(),
                external_id: "99".into(),
                target: Some(ScrapeTargetKind::Movie),
                title: Some("In-Place Movie".into()),
                year: Some(2024),
                cover_url: Some(format!("http://{addr}/p.png")),
                backdrop_url: Some(format!("http://{addr}/f.png")),
                ..Default::default()
            },
            apply_cover: true,
            write_sidecar_lrc: false,
            organize_by_template: Some(false),
            only_fill_missing: false,
            write_album_cover: false,
        })
        .await
        .unwrap();
    assert_eq!(vid.entry.rel_path, "inbox/movie_raw.mkv");
    assert!(!vid.entry.completeness.is_organized);
    assert!(inbox.join("movie_raw.nfo").is_file());
    assert!(inbox.join("movie_raw-poster.png").is_file());
    assert!(inbox.join("movie_raw-fanart.png").is_file());

    let book = app
        .scrape_apply
        .apply(ApplyScrapeInput {
            root_id,
            rel_path: "inbox/book_raw.epub".into(),
            media_kind: MediaKind::Book,
            scraped: ScrapedMetadata {
                provider_id: "openlibrary".into(),
                external_id: "OL2".into(),
                target: Some(ScrapeTargetKind::Book),
                title: Some("Clean Architecture".into()),
                creators: vec!["Robert Martin".into()],
                ..Default::default()
            },
            apply_cover: false,
            write_sidecar_lrc: false,
            organize_by_template: Some(true),
            only_fill_missing: false,
            write_album_cover: false,
        })
        .await
        .unwrap();
    assert_eq!(
        book.entry.rel_path,
        "Robert Martin/Clean Architecture/Clean Architecture - Robert Martin.epub"
    );
    assert!(book.entry.completeness.is_organized);
    assert!(
        root.join("Robert Martin/Clean Architecture/Clean Architecture - Robert Martin.epub")
            .is_file()
    );
    let history = app.metadata.list_history(Some(root_id)).await;
    let book_batch = history
        .iter()
        .find(|b| b.media_kind == MediaKind::Book)
        .expect("book metadata history batch recorded");
    assert_eq!(
        book_batch.records[0].rel_path,
        "Robert Martin/Clean Architecture/Clean Architecture - Robert Martin.epub"
    );
    let undo_rep = app.metadata.undo_batch(book_batch.batch_id).await.unwrap();
    assert_eq!(undo_rep.restored_count, 1);
    assert!(undo_rep.failed_paths.is_empty());
    let restored_book = app
        .workspace
        .inspect(
            root_id,
            "Robert Martin/Clean Architecture/Clean Architecture - Robert Martin.epub",
            Some(MediaKind::Book),
        )
        .await
        .unwrap();
    if let MediaMetadataPayload::Book(b) = restored_book.metadata {
        assert_eq!(b.title, "Draft Title");
    } else {
        panic!("expected restored book metadata");
    }

    let primary = ScrapedMetadata {
        provider_id: "tmdb".into(),
        target: Some(ScrapeTargetKind::TvShow),
        title: Some("Inception".into()),
        year: Some(2010),
        isbn: Some("978-0-13-449416-6".into()),
        disc_or_season_number: Some(0),
        ..Default::default()
    };
    let bad_isbn = ScrapedMetadata {
        isbn: Some("9780132350884".into()),
        title: Some("Inception".into()),
        ..Default::default()
    };
    assert!(!is_compatible_secondary_source(
        &primary,
        &bad_isbn,
        Some(ScrapeTargetKind::Book)
    ));
    let bad_year = ScrapedMetadata {
        title: Some("Inception".into()),
        year: Some(2020),
        ..Default::default()
    };
    assert!(!is_compatible_secondary_source(
        &primary,
        &bad_year,
        Some(ScrapeTargetKind::Movie)
    ));
    let bad_ascii = ScrapedMetadata {
        title: Some("Completely Unrelated Work".into()),
        creators: vec!["Nobody".into()],
        ..Default::default()
    };
    assert!(!is_compatible_secondary_source(
        &primary,
        &bad_ascii,
        Some(ScrapeTargetKind::Movie)
    ));
    let zh_primary = ScrapedMetadata {
        title: Some("三体".into()),
        ..Default::default()
    };
    let en_secondary = ScrapedMetadata {
        title: Some("The Three-Body Problem".into()),
        ..Default::default()
    };
    assert!(is_compatible_secondary_source(
        &zh_primary,
        &en_secondary,
        Some(ScrapeTargetKind::Book)
    ));

    let merged_s0 = merge_scraped_by_priority(
        &[(
            FieldSourceBadge {
                provider_id: "tmdb".into(),
                provider_name: "TMDB".into(),
            },
            primary,
        )],
        Some(ScrapeTargetKind::TvShow),
    );
    assert_eq!(merged_s0.metadata.disc_or_season_number, Some(0));
}

#[tokio::test]
async fn per_provider_switch_and_overrides_work_independently() {
    let dir = tempfile::tempdir().unwrap();
    let app = XMediaToolApp::init(XMediaToolConfig {
        data_dir: dir.path().join("data"),
        initial_roots: Vec::new(),
        password: None,
        ffmpeg_bin: "__missing_ffmpeg__".into(),
        external_web_dir: None,
    })
    .await
    .unwrap();

    let initial = app.scrapers.list_providers(None, None).await.unwrap();
    let douban = initial
        .iter()
        .find(|p| p.descriptor.id == "douban")
        .unwrap();
    let nhentai = initial
        .iter()
        .find(|p| p.descriptor.id == "nhentai")
        .unwrap();
    assert!(douban.enabled && douban.configured);
    assert!(!nhentai.enabled && !nhentai.configured);

    let mut overrides = BTreeMap::new();
    overrides.insert(
        "douban".to_string(),
        ProviderConfigOverride {
            enabled: Some(false),
            ..Default::default()
        },
    );
    overrides.insert(
        "nhentai".to_string(),
        ProviderConfigOverride {
            enabled: Some(true),
            http_proxy: Some("http://127.0.0.1:7890".to_string()),
            locale: Some("ja-JP".to_string()),
            timeout_secs: Some(45),
            user_agent: Some("CustomUA/1.0".to_string()),
        },
    );
    let pub_settings = app
        .settings
        .update_settings(UpdateSettingsInput {
            provider_configs: Some(overrides),
            qishui_cookie: Some("session=abc123456".to_string()),
            ..Default::default()
        })
        .await
        .unwrap();
    assert!(pub_settings.has_qishui_cookie);
    assert_eq!(
        pub_settings
            .provider_configs
            .get("nhentai")
            .and_then(|c| c.timeout_secs),
        Some(45)
    );

    let updated = app.scrapers.list_providers(None, None).await.unwrap();
    let douban2 = updated
        .iter()
        .find(|p| p.descriptor.id == "douban")
        .unwrap();
    let nhentai2 = updated
        .iter()
        .find(|p| p.descriptor.id == "nhentai")
        .unwrap();
    assert!(!douban2.enabled && !douban2.configured);
    assert!(nhentai2.enabled && nhentai2.configured);

    let raw = app.settings.get_raw().await.unwrap();
    let nh_creds = raw.credentials_for_provider("nhentai");
    let bgm_creds = raw.credentials_for_provider("bangumi");
    assert_eq!(
        nh_creds.http_proxy.as_deref(),
        Some("http://127.0.0.1:7890")
    );
    assert_eq!(nh_creds.timeout_secs, Some(45));
    assert_eq!(nh_creds.user_agent.as_deref(), Some("CustomUA/1.0"));
    assert_eq!(bgm_creds.http_proxy, None);
    assert_eq!(bgm_creds.timeout_secs, None);
    assert_eq!(raw.locale_for_provider("nhentai", "zh-CN"), "ja-JP");
    assert_eq!(raw.locale_for_provider("bangumi", "zh-CN"), "zh-CN");

    let err = app
        .scrapers
        .fetch_detail("douban", "12345", ScrapeTargetKind::Book, None)
        .await
        .unwrap_err();
    assert!(err.to_string().contains("已禁用"));
}

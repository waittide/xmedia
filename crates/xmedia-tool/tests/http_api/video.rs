use tempfile::TempDir;
use xmedia_core::domain::library::MediaKind;
use xmedia_tool::application::ports::ArtworkIo;
use xmedia_tool::application::scraping::ApplyScrapeInput;
use xmedia_tool::application::workbench::WorkspaceArtworkOutput;
use xmedia_tool::domain::media::MediaMetadataPayload;
use xmedia_tool::domain::scraper::{ScrapeTargetKind, ScrapedMetadata};
use xmedia_tool::{XMediaToolApp, XMediaToolConfig};

use crate::common::{TINY_PNG, spawn_mock_image_server};

#[tokio::test]
async fn video_scrape_apply_creates_full_jellyfin_movie_and_three_tier_tvshow_layout() {
    let addr = spawn_mock_image_server().await;

    let tmp = TempDir::new().unwrap();
    let video_root = tmp.path().join("videos");
    std::fs::create_dir_all(&video_root).unwrap();

    std::fs::write(video_root.join("spirited_away.mp4"), b"dummy-mp4").unwrap();
    std::fs::write(
        video_root.join("spirited_away.nfo"),
        r#"<movie>
  <title>Old</title>
  <premiered>2001-07-20</premiered>
  <mpaa>PG</mpaa>
  <country>Japan</country>
  <uniqueid type="imdb">tt0245429</uniqueid>
  <actor><name>柊瑠美</name><role>荻野千寻</role><thumb>https://img.example/chihiro.jpg</thumb></actor>
  <fileinfo><streamdetails><video><codec>h264</codec><width>1920</width><height>1080</height></video></streamdetails></fileinfo>
</movie>"#,
    )
    .unwrap();
    std::fs::write(video_root.join("Frieren.S01E01.1080p.mkv"), b"dummy-mkv").unwrap();
    std::fs::write(
        video_root.join("Frieren.S01E01.1080p.zh.srt"),
        b"1\n00:00:01,000 --> 00:00:02,000\nSub\n",
    )
    .unwrap();

    let mut app = XMediaToolApp::init(XMediaToolConfig {
        data_dir: tmp.path().join("data"),
        initial_roots: vec![video_root.clone()],
        password: None,
        ffmpeg_bin: "__missing_ffmpeg__".into(),
        external_web_dir: None,
    })
    .await
    .unwrap();
    app.scrape_apply.set_allow_private_artwork_ips(true);

    let root_id = app.settings.get_public_settings().await.unwrap().roots[0].id;

    let movie_detail = app
        .scrape_apply
        .apply(ApplyScrapeInput {
            root_id,
            rel_path: "spirited_away.mp4".into(),
            media_kind: MediaKind::Video,
            scraped: ScrapedMetadata {
                provider_id: "tmdb".into(),
                external_id: "129".into(),
                target: Some(ScrapeTargetKind::Movie),
                title: Some("千与千寻".into()),
                year: Some(2001),
                summary: Some("少女千寻误入神灵世界。".into()),
                cast: vec!["柊瑠美".into()],
                cover_url: Some(format!("http://{addr}/movie_poster.png")),
                backdrop_url: Some(format!("http://{addr}/movie_fanart.png")),
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
        movie_detail.entry.rel_path,
        "千与千寻 (2001)/千与千寻 (2001).mp4"
    );
    assert!(movie_detail.entry.completeness.has_cover);
    assert!(movie_detail.entry.completeness.has_nfo_or_tag_block);
    assert!(!video_root.join("spirited_away.mp4").exists());
    assert!(!video_root.join("spirited_away.nfo").exists());

    let movie_dir = video_root.join("千与千寻 (2001)");
    assert!(movie_dir.join("千与千寻 (2001).mp4").is_file());
    assert!(movie_dir.join("movie.nfo").is_file());
    assert!(movie_dir.join("poster.png").is_file());
    assert!(movie_dir.join("fanart.png").is_file());
    let movie_nfo_str = std::fs::read_to_string(movie_dir.join("movie.nfo")).unwrap();
    assert!(movie_nfo_str.contains("<premiered>2001-07-20</premiered>"));
    assert!(movie_nfo_str.contains("<mpaa>PG</mpaa>"));
    assert!(movie_nfo_str.contains("<country>Japan</country>"));
    assert!(movie_nfo_str.contains("<uniqueid type=\"imdb\">tt0245429</uniqueid>"));
    assert!(movie_nfo_str.contains("<role>荻野千寻</role>"));
    assert!(movie_nfo_str.contains("<codec>h264</codec>"));

    let ep_detail = app
        .scrape_apply
        .apply(ApplyScrapeInput {
            root_id,
            rel_path: "Frieren.S01E01.1080p.mkv".into(),
            media_kind: MediaKind::Video,
            scraped: ScrapedMetadata {
                provider_id: "tmdb".into(),
                external_id: "209867".into(),
                target: Some(ScrapeTargetKind::TvShow),
                series_or_album: Some("葬送的芙莉莲".into()),
                title: Some("冒险的终点".into()),
                disc_or_season_number: Some(1),
                track_or_episode_number: Some(1),
                year: Some(2023),
                summary: Some("第一集剧情简介。".into()),
                show_summary: Some("全剧总览简介。".into()),
                season_title: Some("第 1 季".into()),
                season_summary: Some("第一季剧情简介。".into()),
                cover_url: Some(format!("http://{addr}/show_poster.png")),
                backdrop_url: Some(format!("http://{addr}/show_fanart.png")),
                season_poster_url: Some(format!("http://{addr}/season1_poster.png")),
                episode_still_url: Some(format!("http://{addr}/ep1_still.png")),
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
        ep_detail.entry.rel_path,
        "葬送的芙莉莲 (2023)/Season 01/葬送的芙莉莲 - S01E01 - 冒险的终点.mkv"
    );
    assert!(ep_detail.entry.completeness.has_cover);

    let show_dir = video_root.join("葬送的芙莉莲 (2023)");
    let season_dir = show_dir.join("Season 01");
    assert!(show_dir.join("tvshow.nfo").is_file());
    assert!(show_dir.join("poster.png").is_file());
    assert!(show_dir.join("fanart.png").is_file());

    assert!(season_dir.join("season.nfo").is_file());
    assert!(season_dir.join("poster.png").is_file());
    assert!(season_dir.join("fanart.png").is_file());
    assert!(
        season_dir
            .join("葬送的芙莉莲 - S01E01 - 冒险的终点.mkv")
            .is_file()
    );
    assert!(
        season_dir
            .join("葬送的芙莉莲 - S01E01 - 冒险的终点.nfo")
            .is_file()
    );
    assert!(
        season_dir
            .join("葬送的芙莉莲 - S01E01 - 冒险的终点-thumb.png")
            .is_file()
    );
    assert!(
        season_dir
            .join("葬送的芙莉莲 - S01E01 - 冒险的终点.zh.srt")
            .is_file()
    );

    let tvshow_nfo_str = std::fs::read_to_string(show_dir.join("tvshow.nfo")).unwrap();
    let season_nfo_str = std::fs::read_to_string(season_dir.join("season.nfo")).unwrap();
    let ep_nfo_str =
        std::fs::read_to_string(season_dir.join("葬送的芙莉莲 - S01E01 - 冒险的终点.nfo")).unwrap();
    assert!(tvshow_nfo_str.contains("全剧总览简介。"));
    assert!(season_nfo_str.contains("<season>1</season>"));
    assert!(season_nfo_str.contains("<seasonnumber>1</seasonnumber>"));
    assert!(season_nfo_str.contains("第一季剧情简介。"));
    assert!(ep_nfo_str.contains("第一集剧情简介。"));

    let snap = app
        .workspace
        .browse(root_id, "", Some(MediaKind::Video), true, None)
        .await
        .unwrap();
    assert_eq!(snap.files.len(), 2);
    let frieren_folder = snap
        .folders
        .iter()
        .find(|f| f.name == "葬送的芙莉莲 (2023)")
        .unwrap();
    assert_eq!(frieren_folder.media_file_count, 1);

    let rescrape_detail = app
        .scrape_apply
        .apply(ApplyScrapeInput {
            root_id,
            rel_path: "葬送的芙莉莲 (2023)/Season 01/葬送的芙莉莲 - S01E01 - 冒险的终点.mkv".into(),
            media_kind: MediaKind::Video,
            scraped: ScrapedMetadata {
                provider_id: "tmdb".into(),
                external_id: "209867".into(),
                target: Some(ScrapeTargetKind::TvShow),
                series_or_album: Some("Frieren Beyond Journey's End".into()),
                title: Some("The Journey's End".into()),
                disc_or_season_number: Some(1),
                track_or_episode_number: Some(1),
                year: Some(2023),
                cover_url: Some(format!("http://{addr}/show_poster.png")),
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
        rescrape_detail.entry.rel_path,
        "Frieren Beyond Journey's End (2023)/Season 01/Frieren Beyond Journey's End - S01E01 - The Journey's End.mkv"
    );
    assert!(!show_dir.exists());
}

#[tokio::test]
async fn video_specials_anime_episode_guess_and_mixed_dir_isolation() {
    let tmp = TempDir::new().unwrap();
    let root = tmp.path().join("videos");
    let specials = root.join("Frieren/Specials");
    let mixed = root.join("Mixed");
    std::fs::create_dir_all(&specials).unwrap();
    std::fs::create_dir_all(&mixed).unwrap();

    let sp = specials.join("[Sub] Frieren - 02 [1080p].mkv");
    std::fs::write(&sp, b"v").unwrap();

    std::fs::write(mixed.join("Alpha.mp4"), b"v1").unwrap();
    std::fs::write(mixed.join("Beta.mp4"), b"v2").unwrap();
    std::fs::write(
        mixed.join("movie.nfo"),
        b"<movie><title>Shared</title></movie>",
    )
    .unwrap();
    std::fs::write(mixed.join("poster.jpg"), TINY_PNG).unwrap();
    std::fs::write(mixed.join("Alpha-fanart.webp"), TINY_PNG).unwrap();
    std::fs::write(mixed.join("Alpha-thumb.jpg"), TINY_PNG).unwrap();

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

    let sp_detail = app
        .workspace
        .inspect(
            root_id,
            "Frieren/Specials/[Sub] Frieren - 02 [1080p].mkv",
            Some(MediaKind::Video),
        )
        .await
        .unwrap();
    if let MediaMetadataPayload::Video(v) = sp_detail.metadata {
        assert!(v.is_episode);
        assert_eq!(v.season_number, Some(0));
        assert_eq!(v.episode_number, Some(2));
    } else {
        panic!("expected video payload");
    }

    let alpha = app
        .workspace
        .inspect(root_id, "Mixed/Alpha.mp4", Some(MediaKind::Video))
        .await
        .unwrap();
    if let MediaMetadataPayload::Video(v) = alpha.metadata {
        assert!(!v.has_nfo);
        assert!(!v.has_poster);
        assert!(v.has_fanart);
        assert!(v.has_thumb);
    } else {
        panic!("expected video payload");
    }
    assert!(matches!(
        app.workspace
            .artwork(
                root_id,
                "Mixed/Alpha.mp4",
                Some(MediaKind::Video),
                ArtworkIo::Read {
                    slot: Some("fanart"),
                },
            )
            .await
            .unwrap(),
        WorkspaceArtworkOutput::Read(Some(_))
    ));
    assert!(matches!(
        app.workspace
            .artwork(
                root_id,
                "Mixed/Alpha.mp4",
                Some(MediaKind::Video),
                ArtworkIo::Read {
                    slot: Some("thumb"),
                },
            )
            .await
            .unwrap(),
        WorkspaceArtworkOutput::Read(Some(_))
    ));
}

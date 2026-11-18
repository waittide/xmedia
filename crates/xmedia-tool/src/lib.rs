pub mod application;
pub mod domain;
pub mod http;
pub mod infra;

use std::path::PathBuf;
use std::sync::Arc;

use anyhow::Context;
use axum::Router;
use tower_http::compression::CompressionLayer;
use tower_http::compression::predicate::{DefaultPredicate, NotForContentType, Predicate};
use tower_http::cors::CorsLayer;
use tower_http::trace::TraceLayer;
use xmedia_core::infra::ffmpeg::FfmpegToolkit;

use crate::application::auth::ToolAuthService;
use crate::application::scraping::ScrapingOrchestratorService;
use crate::application::settings::ToolSettingsService;
use crate::application::workbench::MediaWorkbenchService;
use crate::domain::ports::ScraperProviderPort;
use crate::http::ToolHttpState;
use crate::http::routes::tool_api_router;
use crate::http::static_files::serve_tool_spa;
use crate::infra::config_store::JsonFileConfigRepository;
use crate::infra::inspectors::build_default_inspectors;
use crate::infra::scrapers::build_default_scraper_providers;

#[derive(Debug, Clone)]
pub struct XMediaToolConfig {
    pub data_dir: PathBuf,
    pub initial_roots: Vec<PathBuf>,
    pub password: Option<String>,
    pub ffmpeg_bin: String,
    pub external_web_dir: Option<PathBuf>,
}

pub struct XMediaToolApp {
    pub settings: ToolSettingsService,
    pub workbench: MediaWorkbenchService,
    pub scraping: ScrapingOrchestratorService,
    pub router: Router,
}

impl XMediaToolApp {
    pub async fn init(cfg: XMediaToolConfig) -> anyhow::Result<Self> {
        Self::init_with_providers(cfg, build_default_scraper_providers()).await
    }

    pub async fn init_with_providers(
        cfg: XMediaToolConfig,
        providers: Vec<Arc<dyn ScraperProviderPort>>,
    ) -> anyhow::Result<Self> {
        std::fs::create_dir_all(&cfg.data_dir).with_context(|| {
            format!(
                "failed to create xmedia-tool data directory {}",
                cfg.data_dir.display()
            )
        })?;

        let config_file = cfg.data_dir.join("tool-config.json");
        let config_repo = Arc::new(
            JsonFileConfigRepository::load_or_init(config_file, &[])
                .map_err(|e| anyhow::anyhow!(e.to_string()))?,
        );
        let settings = ToolSettingsService::new(config_repo);

        for root_dir in &cfg.initial_roots {
            let fallback_name = root_dir
                .file_name()
                .map(|s| s.to_string_lossy().into_owned())
                .filter(|s| !s.is_empty())
                .unwrap_or_else(|| root_dir.display().to_string());
            settings
                .add_root(fallback_name, root_dir.to_string_lossy().into_owned(), None)
                .await
                .with_context(|| format!("failed registering --root '{}'", root_dir.display()))?;
        }

        let ffmpeg = FfmpegToolkit::from_ffmpeg_bin(&cfg.ffmpeg_bin);
        let inspectors = build_default_inspectors(ffmpeg.clone());
        let workbench = MediaWorkbenchService::new(settings.clone(), inspectors);
        let scraping =
            ScrapingOrchestratorService::new(settings.clone(), workbench.clone(), providers);
        let auth = ToolAuthService::new(cfg.password);

        let state = ToolHttpState {
            auth,
            settings: settings.clone(),
            workbench: workbench.clone(),
            scraping: scraping.clone(),
            ffmpeg,
            external_web_dir: cfg.external_web_dir,
        };

        let compression_predicate = DefaultPredicate::new()
            .and(NotForContentType::const_new("image/"))
            .and(NotForContentType::const_new("application/octet-stream"));

        let spa_service = axum::routing::get(serve_tool_spa).with_state(state.clone());

        let router = Router::new()
            .nest("/api/v1/tool", tool_api_router().with_state(state))
            .fallback_service(spa_service)
            .layer(CorsLayer::permissive())
            .layer(CompressionLayer::new().compress_when(compression_predicate))
            .layer(TraceLayer::new_for_http());

        Ok(Self {
            settings,
            workbench,
            scraping,
            router,
        })
    }
}

#[cfg(test)]
mod tests {
    use std::io::Write;
    use std::path::Path;

    use axum::body::{Body, to_bytes};
    use axum::http::{Request, StatusCode, header};
    use tempfile::TempDir;
    use tower::ServiceExt;
    use xmedia_core::domain::library::MediaKind;
    use zip::CompressionMethod;
    use zip::write::SimpleFileOptions;

    use crate::domain::media::{
        BookMetaPatch, ComicMetaPatch, MediaMetadataPatch, MediaMetadataPayload, MusicMetaPatch,
        VideoMetaPatch,
    };
    use crate::{XMediaToolApp, XMediaToolConfig};

    const TINY_PNG: &[u8] = &[
        0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A, 0x00, 0x00, 0x00, 0x0D, b'I', b'H', b'D',
        b'R', 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01, 0x08, 0x06, 0x00, 0x00, 0x00, 0x1F,
        0x15, 0xC4, 0x89, 0x00, 0x00, 0x00, 0x0A, b'I', b'D', b'A', b'T', 0x78, 0x9C, 0x63, 0x00,
        0x01, 0x00, 0x00, 0x05, 0x00, 0x01, 0x0D, 0x0A, 0x2D, 0xB4, 0x00, 0x00, 0x00, 0x00, b'I',
        b'E', b'N', b'D', 0xAE, 0x42, 0x60, 0x82,
    ];

    fn write_minimal_wav(path: &Path) {
        // 1-second 8kHz mono 8-bit PCM WAV so Lofty parses valid audio properties + RIFF/ID3v2 tags
        let sample_rate = 8_000u32;
        let num_samples = 8_000u32;
        let data_size = num_samples;
        let riff_size = 36 + data_size;
        let mut buf = Vec::with_capacity((44 + data_size) as usize);
        buf.extend_from_slice(b"RIFF");
        buf.extend_from_slice(&riff_size.to_le_bytes());
        buf.extend_from_slice(b"WAVEfmt ");
        buf.extend_from_slice(&16u32.to_le_bytes());
        buf.extend_from_slice(&1u16.to_le_bytes()); // PCM
        buf.extend_from_slice(&1u16.to_le_bytes()); // mono
        buf.extend_from_slice(&sample_rate.to_le_bytes());
        buf.extend_from_slice(&sample_rate.to_le_bytes());
        buf.extend_from_slice(&1u16.to_le_bytes());
        buf.extend_from_slice(&8u16.to_le_bytes());
        buf.extend_from_slice(b"data");
        buf.extend_from_slice(&data_size.to_le_bytes());
        buf.resize((44 + data_size) as usize, 128u8);
        std::fs::write(path, buf).unwrap();
    }

    fn write_minimal_cbz(path: &Path) {
        let f = std::fs::File::create(path).unwrap();
        let mut zip = zip::ZipWriter::new(f);
        let opts = SimpleFileOptions::default().compression_method(CompressionMethod::Deflated);
        zip.start_file("001.png", opts).unwrap();
        zip.write_all(TINY_PNG).unwrap();
        zip.start_file("002.png", opts).unwrap();
        zip.write_all(TINY_PNG).unwrap();
        zip.finish().unwrap();
    }

    fn write_minimal_epub(path: &Path) {
        let f = std::fs::File::create(path).unwrap();
        let mut zip = zip::ZipWriter::new(f);
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
<package xmlns="http://www.idpf.org/2007/opf" version="3.0" unique-identifier="bookid">
  <metadata xmlns:dc="http://purl.org/dc/elements/1.1/">
    <dc:identifier id="bookid">urn:uuid:11111111-1111-1111-1111-111111111111</dc:identifier>
    <dc:title>Draft Title</dc:title>
    <dc:creator>Draft Author</dc:creator>
    <dc:language>en</dc:language>
  </metadata>
  <manifest>
    <item id="chap1" href="chap1.xhtml" media-type="application/xhtml+xml"/>
  </manifest>
  <spine>
    <itemref idref="chap1"/>
  </spine>
</package>"#,
        )
        .unwrap();

        zip.start_file("OEBPS/chap1.xhtml", deflated).unwrap();
        zip.write_all(
            b"<html xmlns=\"http://www.w3.org/1999/xhtml\"><body><p>Hello</p></body></html>",
        )
        .unwrap();
        zip.finish().unwrap();
    }

    #[tokio::test]
    async fn password_guard_and_root_sandbox_traversal_rejection() {
        let tmp = TempDir::new().unwrap();
        let media_root = tmp.path().join("media");
        std::fs::create_dir_all(&media_root).unwrap();
        std::fs::write(tmp.path().join("outside_secret.txt"), b"top-secret").unwrap();

        let app = XMediaToolApp::init(XMediaToolConfig {
            data_dir: tmp.path().join("data"),
            initial_roots: vec![media_root],
            password: Some("tool-pass-2026".into()),
            ffmpeg_bin: "__missing_ffmpeg__".into(),
            external_web_dir: None,
        })
        .await
        .unwrap();

        // Unauthenticated settings call returns 401
        let unauth_res = app
            .router
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/api/v1/tool/settings")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(unauth_res.status(), StatusCode::UNAUTHORIZED);

        // Unlock with password returns token & cookie
        let unlock_res = app
            .router
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/tool/auth/unlock")
                    .header(header::CONTENT_TYPE, "application/json")
                    .body(Body::from(r#"{"password":"tool-pass-2026"}"#))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(unlock_res.status(), StatusCode::OK);
        let unlock_json: serde_json::Value =
            serde_json::from_slice(&to_bytes(unlock_res.into_body(), usize::MAX).await.unwrap())
                .unwrap();
        let token = unlock_json["token"].as_str().unwrap();

        // Authenticated settings call succeeds
        let settings_res = app
            .router
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/api/v1/tool/settings")
                    .header(header::AUTHORIZATION, format!("Bearer {token}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(settings_res.status(), StatusCode::OK);
        let settings_json: serde_json::Value = serde_json::from_slice(
            &to_bytes(settings_res.into_body(), usize::MAX)
                .await
                .unwrap(),
        )
        .unwrap();
        let root_id = settings_json["roots"][0]["id"].as_str().unwrap();

        // Path traversal attempt (`../outside_secret.txt`) is strictly rejected with 403 Forbidden
        let trav_res = app
            .router
            .clone()
            .oneshot(
                Request::builder()
                    .uri(format!(
                        "/api/v1/tool/fs/browse?root_id={root_id}&path=../outside_secret.txt"
                    ))
                    .header(header::AUTHORIZATION, format!("Bearer {token}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(trav_res.status(), StatusCode::FORBIDDEN);
    }

    #[tokio::test]
    async fn all_four_domains_atomic_metadata_artwork_and_rename_roundtrip() {
        let tmp = TempDir::new().unwrap();
        let media_root = tmp.path().join("library");
        std::fs::create_dir_all(&media_root).unwrap();

        let wav_path = media_root.join("raw_song.wav");
        let cbz_path = media_root.join("raw_chapter.cbz");
        let epub_path = media_root.join("raw_book.epub");
        let mkv_path = media_root.join("raw_ep.mkv");
        let srt_path = media_root.join("raw_ep.zh.srt");

        write_minimal_wav(&wav_path);
        write_minimal_cbz(&cbz_path);
        write_minimal_epub(&epub_path);
        std::fs::write(&mkv_path, b"dummy-matroska-bytes").unwrap();
        std::fs::write(&srt_path, b"1\n00:00:01,000 --> 00:00:02,000\nHello\n").unwrap();

        let app = XMediaToolApp::init(XMediaToolConfig {
            data_dir: tmp.path().join("data"),
            initial_roots: vec![media_root.clone()],
            password: None,
            ffmpeg_bin: "__missing_ffmpeg__".into(),
            external_web_dir: None,
        })
        .await
        .unwrap();

        let root_id = app.settings.get_public_settings().await.unwrap().roots[0].id;

        // 1. Music: write tags + embedded lyrics + sidecar .lrc + cover, then batch rename
        let music_detail = app
            .workbench
            .write_metadata(
                root_id,
                "raw_song.wav",
                MediaMetadataPatch::Music(MusicMetaPatch {
                    title: Some("晴天".into()),
                    artist: Some("周杰伦".into()),
                    album: Some("叶惠美".into()),
                    track_number: Some(3),
                    year: Some(2003),
                    embedded_lyrics: Some("[00:01.00]故事的小黄花".into()),
                    sidecar_lrc: Some("[00:01.00]故事的小黄花".into()),
                    ..Default::default()
                }),
            )
            .await
            .unwrap();
        if let MediaMetadataPayload::Music(m) = music_detail.metadata {
            assert_eq!(m.title, "晴天");
            assert_eq!(m.artist, "周杰伦");
            assert_eq!(m.track_number, Some(3));
            assert_eq!(m.embedded_lyrics.as_deref(), Some("[00:01.00]故事的小黄花"));
            assert_eq!(m.sidecar_lrc.as_deref(), Some("[00:01.00]故事的小黄花"));
        } else {
            panic!("expected music payload");
        }
        app.workbench
            .write_artwork(
                root_id,
                "raw_song.wav",
                TINY_PNG,
                "image/png",
                "cover",
                Some(MediaKind::Music),
            )
            .await
            .unwrap();
        assert!(
            app.workbench
                .read_artwork(root_id, "raw_song.wav", Some(MediaKind::Music), None)
                .await
                .unwrap()
                .is_some()
        );

        let music_plans = app
            .workbench
            .preview_rename(
                root_id,
                &["raw_song.wav".to_string()],
                Some("{track:2} - {artist} - {title}"),
                None,
                Some(MediaKind::Music),
            )
            .await
            .unwrap();
        assert_eq!(music_plans[0].to_rel_path, "03 - 周杰伦 - 晴天.wav");
        assert_eq!(music_plans[0].companion_moves.len(), 1);
        app.workbench
            .execute_rename(root_id, &music_plans)
            .await
            .unwrap();
        assert!(media_root.join("03 - 周杰伦 - 晴天.wav").exists());
        assert!(media_root.join("03 - 周杰伦 - 晴天.lrc").exists());

        // 2. Comic: write ComicInfo.xml + 000_cover.png inside CBZ
        let comic_detail = app
            .workbench
            .write_metadata(
                root_id,
                "raw_chapter.cbz",
                MediaMetadataPatch::Comic(ComicMetaPatch {
                    series: Some("葬送的芙莉莲".into()),
                    title: Some("冒险的终点".into()),
                    number: Some("1".into()),
                    writers: Some(vec!["山田钟人".into()]),
                    pencillers: Some(vec!["阿部司".into()]),
                    year: Some(2020),
                    ..Default::default()
                }),
            )
            .await
            .unwrap();
        assert!(comic_detail.entry.completeness.has_nfo_or_tag_block);
        app.workbench
            .write_artwork(
                root_id,
                "raw_chapter.cbz",
                TINY_PNG,
                "image/png",
                "cover",
                Some(MediaKind::Comic),
            )
            .await
            .unwrap();
        let comic_reloaded = app
            .workbench
            .inspect_file(root_id, "raw_chapter.cbz", Some(MediaKind::Comic))
            .await
            .unwrap();
        if let MediaMetadataPayload::Comic(c) = comic_reloaded.metadata {
            assert_eq!(c.series.as_deref(), Some("葬送的芙莉莲"));
            assert!(c.has_comic_info_xml);
            assert!(c.has_cover);
        } else {
            panic!("expected comic payload");
        }

        // 3. Book: write OPF metadata + cover into EPUB while preserving uncompressed first `mimetype`
        app.workbench
            .write_metadata(
                root_id,
                "raw_book.epub",
                MediaMetadataPatch::Book(BookMetaPatch {
                    title: Some("三体".into()),
                    authors: Some(vec!["刘慈欣".into()]),
                    description: Some("地球往事三部曲之一".into()),
                    isbn: Some("9787536692930".into()),
                    tags: Some(vec!["科幻".into(), "硬科幻".into()]),
                    ..Default::default()
                }),
            )
            .await
            .unwrap();
        app.workbench
            .write_artwork(
                root_id,
                "raw_book.epub",
                TINY_PNG,
                "image/png",
                "cover",
                Some(MediaKind::Book),
            )
            .await
            .unwrap();
        let book_reloaded = app
            .workbench
            .inspect_file(root_id, "raw_book.epub", Some(MediaKind::Book))
            .await
            .unwrap();
        if let MediaMetadataPayload::Book(b) = book_reloaded.metadata {
            assert_eq!(b.title, "三体");
            assert_eq!(b.authors, vec!["刘慈欣"]);
            assert_eq!(b.isbn.as_deref(), Some("9787536692930"));
            assert!(b.has_cover);
        } else {
            panic!("expected book payload");
        }
        // Check OCF rule: first zip entry in rewritten EPUB is uncompressed `mimetype`
        {
            let f = std::fs::File::open(media_root.join("raw_book.epub")).unwrap();
            let mut archive = zip::ZipArchive::new(f).unwrap();
            let first = archive.by_index(0).unwrap();
            assert_eq!(first.name().unwrap(), "mimetype");
            assert_eq!(first.compression(), CompressionMethod::Stored);
        }

        // 4. Video: write episode .nfo + poster + batch rename moving both .nfo and .zh.srt
        app.workbench
            .write_metadata(
                root_id,
                "raw_ep.mkv",
                MediaMetadataPatch::Video(VideoMetaPatch {
                    is_episode: Some(true),
                    show_title: Some("Frieren".into()),
                    title: Some("The Journey's End".into()),
                    season_number: Some(1),
                    episode_number: Some(1),
                    year: Some(2023),
                    tmdb_id: Some("209867".into()),
                    ..Default::default()
                }),
            )
            .await
            .unwrap();
        app.workbench
            .write_artwork(
                root_id,
                "raw_ep.mkv",
                TINY_PNG,
                "image/png",
                "poster",
                Some(MediaKind::Video),
            )
            .await
            .unwrap();

        let video_plans = app
            .workbench
            .preview_rename(
                root_id,
                &["raw_ep.mkv".to_string()],
                None,
                None,
                Some(MediaKind::Video),
            )
            .await
            .unwrap();
        assert_eq!(
            video_plans[0].to_rel_path,
            "Frieren (2023)/Season 01/Frieren - S01E01 - The Journey's End.mkv"
        );
        app.workbench
            .execute_rename(root_id, &video_plans)
            .await
            .unwrap();
        let expected_ep_dir = media_root.join("Frieren (2023)").join("Season 01");
        assert!(
            expected_ep_dir
                .join("Frieren - S01E01 - The Journey's End.mkv")
                .exists()
        );
        assert!(
            expected_ep_dir
                .join("Frieren - S01E01 - The Journey's End.nfo")
                .exists()
        );
        assert!(
            expected_ep_dir
                .join("Frieren - S01E01 - The Journey's End.zh.srt")
                .exists()
        );
    }

    #[tokio::test]
    async fn video_scrape_apply_creates_full_jellyfin_movie_and_three_tier_tvshow_layout() {
        use crate::domain::scraper::{ScrapeTargetKind, ScrapedMetadata};
        use axum::Router;
        use axum::routing::get;

        // Spin up a local loopback image server so `apply_to_file(..., apply_cover=true)` downloads real artwork
        let img_app = Router::new().route(
            "/{name}",
            get(|| async { ([(header::CONTENT_TYPE, "image/png")], TINY_PNG) }),
        );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            let _ = axum::serve(listener, img_app).await;
        });

        let tmp = TempDir::new().unwrap();
        let video_root = tmp.path().join("videos");
        std::fs::create_dir_all(&video_root).unwrap();

        // Scattered flat files + legacy sidecars in root
        std::fs::write(video_root.join("spirited_away.mp4"), b"dummy-mp4").unwrap();
        std::fs::write(
            video_root.join("spirited_away.nfo"),
            b"<movie><title>Old</title></movie>",
        )
        .unwrap();
        std::fs::write(video_root.join("Frieren.S01E01.1080p.mkv"), b"dummy-mkv").unwrap();
        std::fs::write(
            video_root.join("Frieren.S01E01.1080p.zh.srt"),
            b"1\n00:00:01,000 --> 00:00:02,000\nSub\n",
        )
        .unwrap();

        let app = XMediaToolApp::init(XMediaToolConfig {
            data_dir: tmp.path().join("data"),
            initial_roots: vec![video_root.clone()],
            password: None,
            ffmpeg_bin: "__missing_ffmpeg__".into(),
            external_web_dir: None,
        })
        .await
        .unwrap();

        let root_id = app.settings.get_public_settings().await.unwrap().roots[0].id;

        // 1. Apply Movie scrape -> `<Root>/千与千寻 (2001)/{千与千寻 (2001).mp4, movie.nfo, poster.png, fanart.png}`
        let movie_detail = app
            .scraping
            .apply_to_file(
                root_id,
                "spirited_away.mp4",
                MediaKind::Video,
                ScrapedMetadata {
                    provider_id: "tmdb".into(),
                    external_id: "129".into(),
                    target: Some(ScrapeTargetKind::Movie),
                    title: Some("千与千寻".into()),
                    year: Some(2001),
                    summary: Some("少女千寻误入神灵世界。".into()),
                    cover_url: Some(format!("http://{addr}/movie_poster.png")),
                    backdrop_url: Some(format!("http://{addr}/movie_fanart.png")),
                    ..Default::default()
                },
                true,
                false,
                None,
                false,
            )
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

        // 2. Apply TV Episode scrape -> 3-tier Jellyfin layout (`tvshow.nfo`, `season.nfo`, `<ep>.nfo`, + all images + migrated `.srt`)
        let ep_detail = app
            .scraping
            .apply_to_file(
                root_id,
                "Frieren.S01E01.1080p.mkv",
                MediaKind::Video,
                ScrapedMetadata {
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
                true,
                false,
                None,
                false,
            )
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
            std::fs::read_to_string(season_dir.join("葬送的芙莉莲 - S01E01 - 冒险的终点.nfo"))
                .unwrap();
        assert!(tvshow_nfo_str.contains("全剧总览简介。"));
        assert!(season_nfo_str.contains("<seasonnumber>1</seasonnumber>"));
        assert!(season_nfo_str.contains("第一季剧情简介。"));
        assert!(ep_nfo_str.contains("第一集剧情简介。"));

        // 3. Recursive browse at root reports both subfolders with accurate recursive file counts AND all nested media files
        let snap = app
            .workbench
            .browse_directory(root_id, "", Some(MediaKind::Video), true)
            .await
            .unwrap();
        assert_eq!(snap.files.len(), 2);
        let frieren_folder = snap
            .folders
            .iter()
            .find(|f| f.name == "葬送的芙莉莲 (2023)")
            .unwrap();
        assert_eq!(frieren_folder.media_file_count, 1);

        // 4. Re-scraping an already archived episode into a corrected series name cleanly removes the vacated ghost folder
        let rescrape_detail = app
            .scraping
            .apply_to_file(
                root_id,
                "葬送的芙莉莲 (2023)/Season 01/葬送的芙莉莲 - S01E01 - 冒险的终点.mkv",
                MediaKind::Video,
                ScrapedMetadata {
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
                true,
                false,
                None,
                false,
            )
            .await
            .unwrap();
        assert_eq!(
            rescrape_detail.entry.rel_path,
            "Frieren Beyond Journey's End (2023)/Season 01/Frieren Beyond Journey's End - S01E01 - The Journey's End.mkv"
        );
        // Old show folder and its orphan `tvshow.nfo`/`season.nfo`/`poster.png` are completely reclaimed
        assert!(!show_dir.exists());
    }

    #[test]
    fn companion_sidecars_match_opf_and_reject_prefix_collisions() {
        use crate::infra::atomic_fs::find_companion_sidecars;

        let tmp = TempDir::new().unwrap();
        let dir = tmp.path();

        // Video `01.mp4` must NOT grab `01-02.srt` or `010.nfo`, but MUST grab `01.zh.srt` & `01-poster.jpg`
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

        // PDF / TXT sidecars include `.opf` and cover images
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
        use std::io::{Read, Write};
        use zip::write::SimpleFileOptions;
        use zip::{CompressionMethod, ZipArchive, ZipWriter};

        let tmp = TempDir::new().unwrap();
        let root = tmp.path().join("books");
        std::fs::create_dir_all(&root).unwrap();
        let epub = root.join("fixed.epub");

        // Build EPUB with rendition:layout meta, custom unique-identifier="pub-id", and PNG cover
        {
            let f = std::fs::File::create(&epub).unwrap();
            let mut zip = ZipWriter::new(f);
            let stored = SimpleFileOptions::default().compression_method(CompressionMethod::Stored);
            let deflated =
                SimpleFileOptions::default().compression_method(CompressionMethod::Deflated);
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

        // Write metadata without touching cover first
        app.workbench
            .write_metadata(
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

        // Replace PNG cover with JPEG bytes; manifest must update href + media-type
        let fake_jpg = [0xFFu8, 0xD8, 0xFF, 0xE0, 0x00, 0x10, 0x4A, 0x46, 0x49, 0x46];
        app.workbench
            .write_artwork(
                root_id,
                "fixed.epub",
                &fake_jpg,
                "image/jpeg",
                "cover",
                Some(MediaKind::Book),
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
                .workbench
                .write_metadata(
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
            app.workbench
                .write_artwork(
                    root_id,
                    file,
                    TINY_PNG,
                    "image/png",
                    "cover",
                    Some(MediaKind::Book),
                )
                .await
                .unwrap();
            assert!(
                app.workbench
                    .read_artwork(root_id, file, Some(MediaKind::Book), None)
                    .await
                    .unwrap()
                    .is_some()
            );
        }
        assert!(root.join("draft_novel.opf").is_file());
        assert!(root.join("draft_novel.png").is_file());
        assert!(root.join("guide.opf").is_file());
        assert!(root.join("guide.png").is_file());

        // Rename moves sidecar .opf and cover together
        let plans = app
            .workbench
            .preview_rename(
                root_id,
                &["draft_novel.txt".to_string()],
                Some("{author}/{title}"),
                None,
                Some(MediaKind::Book),
            )
            .await
            .unwrap();
        assert_eq!(plans[0].to_rel_path, "Alice/Title draft_novel.txt.txt");
        app.workbench.execute_rename(root_id, &plans).await.unwrap();
        assert!(root.join("Alice/Title draft_novel.txt.opf").is_file());
        assert!(root.join("Alice/Title draft_novel.txt.png").is_file());
    }

    #[tokio::test]
    async fn music_id3v2_year_persists_and_replacing_front_cover_keeps_back_cover() {
        use lofty::config::WriteOptions;
        use lofty::picture::{MimeType, Picture, PictureType};
        use lofty::prelude::{AudioFile, TaggedFileExt};
        use lofty::tag::Tag;

        let tmp = TempDir::new().unwrap();
        let root = tmp.path().join("music");
        std::fs::create_dir_all(&root).unwrap();
        let wav = root.join("track.wav");
        write_minimal_wav(&wav);

        // Pre-seed CoverBack in ID3v2 tag
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
            .workbench
            .write_metadata(
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

        app.workbench
            .write_artwork(
                root_id,
                "track.wav",
                TINY_PNG,
                "image/png",
                "cover",
                Some(MediaKind::Music),
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
        use std::io::{Read, Write};
        use zip::write::SimpleFileOptions;
        use zip::{CompressionMethod, ZipArchive, ZipWriter};

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
            .workbench
            .inspect_file(root_id, "Solo_Ch01.cbz", Some(MediaKind::Comic))
            .await
            .unwrap();
        if let MediaMetadataPayload::Comic(ref c) = detail.metadata {
            assert_eq!(c.reading_direction, "webtoon");
            // Comic filename `Solo_Ch01` shouldn't pollute chapter title or duplicate `{series}` in template
            let plans = app
                .workbench
                .preview_rename(
                    root_id,
                    &["Solo_Ch01.cbz".to_string()],
                    Some("{series}/{series} Ch.{number:03} - {title}"),
                    None,
                    Some(MediaKind::Comic),
                )
                .await
                .unwrap();
            assert_eq!(plans[0].to_rel_path, "Solo/Solo Ch.001.cbz");
        } else {
            panic!("expected comic payload");
        }

        app.workbench
            .write_metadata(
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
    async fn video_specials_anime_episode_guess_and_mixed_dir_isolation() {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path().join("videos");
        let specials = root.join("Frieren/Specials");
        let mixed = root.join("Mixed");
        std::fs::create_dir_all(&specials).unwrap();
        std::fs::create_dir_all(&mixed).unwrap();

        let sp = specials.join("[Sub] Frieren - 02 [1080p].mkv");
        std::fs::write(&sp, b"v").unwrap();

        // Mixed folder has movie.nfo and two separate movies: movie.nfo & poster.jpg must NOT leak onto unrelated video
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
            .workbench
            .inspect_file(
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
            .workbench
            .inspect_file(root_id, "Mixed/Alpha.mp4", Some(MediaKind::Video))
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
        assert!(
            app.workbench
                .read_artwork(
                    root_id,
                    "Mixed/Alpha.mp4",
                    Some(MediaKind::Video),
                    Some("fanart")
                )
                .await
                .unwrap()
                .is_some()
        );
        assert!(
            app.workbench
                .read_artwork(
                    root_id,
                    "Mixed/Alpha.mp4",
                    Some(MediaKind::Video),
                    Some("thumb")
                )
                .await
                .unwrap()
                .is_some()
        );
    }

    #[tokio::test]
    async fn scrape_subitem_field_protection_only_fill_missing_and_untimed_lrc_guard() {
        use crate::domain::scraper::{ScrapeTargetKind, ScrapedMetadata};

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

        // 1. Pre-seed Comic chapter metadata, then apply Series-level scrape -> chapter fields survive
        app.workbench
            .write_metadata(
                root_id,
                "ch01.cbz",
                MediaMetadataPatch::Comic(ComicMetaPatch {
                    title: Some("第一话 犬与电锯".into()),
                    number: Some("1".into()),
                    volume: Some(1),
                    ..Default::default()
                }),
            )
            .await
            .unwrap();
        let comic_after = app
            .scraping
            .apply_to_file(
                root_id,
                "ch01.cbz",
                MediaKind::Comic,
                ScrapedMetadata {
                    provider_id: "bangumi".into(),
                    external_id: "321".into(),
                    target: Some(ScrapeTargetKind::Comic),
                    title: Some("链锯人".into()),
                    creators: vec!["藤本树".into()],
                    ..Default::default()
                },
                false,
                false,
                Some(false),
                false,
            )
            .await
            .unwrap();
        if let MediaMetadataPayload::Comic(c) = comic_after.metadata {
            assert_eq!(c.series.as_deref(), Some("链锯人"));
            assert_eq!(c.title.as_deref(), Some("第一话 犬与电锯"));
            assert_eq!(c.number.as_deref(), Some("1"));
            assert_eq!(c.volume, Some(1));
        } else {
            panic!("expected comic payload");
        }

        // 2. Music: pre-seed track tags, apply MusicAlbum scrape -> track title/artist/track# preserved
        app.workbench
            .write_metadata(
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
            .scraping
            .apply_to_file(
                root_id,
                "track01.wav",
                MediaKind::Music,
                ScrapedMetadata {
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
                false,
                true,
                Some(false),
                false,
            )
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

        // 3. Lyrics scrape: untimed lyrics must NOT create .lrc; timed LRC creates .lrc without touching tags
        app.scraping
            .apply_to_file(
                root_id,
                "track01.wav",
                MediaKind::Music,
                ScrapedMetadata {
                    provider_id: "lrclib".into(),
                    external_id: "lrc-1".into(),
                    target: Some(ScrapeTargetKind::Lyrics),
                    title: Some("WRONG TITLE".into()),
                    creators: vec!["WRONG ARTIST".into()],
                    synced_lyrics: Some("纯文本无时间戳歌词".into()),
                    ..Default::default()
                },
                false,
                true,
                Some(false),
                false,
            )
            .await
            .unwrap();
        assert!(!root.join("track01.lrc").exists());

        let music_after_lrc = app
            .scraping
            .apply_to_file(
                root_id,
                "track01.wav",
                MediaKind::Music,
                ScrapedMetadata {
                    provider_id: "lrclib".into(),
                    external_id: "lrc-2".into(),
                    target: Some(ScrapeTargetKind::Lyrics),
                    title: Some("WRONG TITLE".into()),
                    synced_lyrics: Some("[00:12.34]故事的小黄花".into()),
                    ..Default::default()
                },
                false,
                true,
                Some(false),
                false,
            )
            .await
            .unwrap();
        assert!(root.join("track01.lrc").is_file());
        if let MediaMetadataPayload::Music(m) = music_after_lrc.metadata {
            assert_eq!(m.title, "晴天");
            assert_eq!(m.sidecar_lrc.as_deref(), Some("[00:12.34]故事的小黄花"));
        } else {
            panic!("expected music payload");
        }

        // 4. `only_fill_missing = true`: keeps existing publisher while filling missing description & isbn
        app.workbench
            .write_metadata(
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
            .scraping
            .apply_to_file(
                root_id,
                "book.epub",
                MediaKind::Book,
                ScrapedMetadata {
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
                false,
                false,
                Some(false),
                true,
            )
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
        use crate::domain::scraper::{
            FieldSourceBadge, ScrapeTargetKind, ScrapedMetadata, is_compatible_secondary_source,
            merge_scraped_by_priority,
        };
        use axum::Router;
        use axum::routing::get;

        let img_app = Router::new().route(
            "/{name}",
            get(|| async { ([(header::CONTENT_TYPE, "image/png")], TINY_PNG) }),
        );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            let _ = axum::serve(listener, img_app).await;
        });

        let tmp = TempDir::new().unwrap();
        let root = tmp.path().join("media");
        let inbox = root.join("inbox");
        std::fs::create_dir_all(&inbox).unwrap();

        std::fs::write(inbox.join("movie_raw.mkv"), b"dummy-mkv").unwrap();
        write_minimal_epub(&inbox.join("book_raw.epub"));

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

        // 1. Video with `organize_by_template = Some(false)` updates NFO & artwork in place without moving
        let vid = app
            .scraping
            .apply_to_file(
                root_id,
                "inbox/movie_raw.mkv",
                MediaKind::Video,
                ScrapedMetadata {
                    provider_id: "tmdb".into(),
                    external_id: "99".into(),
                    target: Some(ScrapeTargetKind::Movie),
                    title: Some("In-Place Movie".into()),
                    year: Some(2024),
                    cover_url: Some(format!("http://{addr}/p.png")),
                    backdrop_url: Some(format!("http://{addr}/f.png")),
                    ..Default::default()
                },
                true,
                false,
                Some(false),
                false,
            )
            .await
            .unwrap();
        assert_eq!(vid.entry.rel_path, "inbox/movie_raw.mkv");
        assert!(inbox.join("movie_raw.nfo").is_file());
        assert!(inbox.join("movie_raw-poster.png").is_file());
        assert!(inbox.join("movie_raw-fanart.png").is_file());

        // 2. Book with `organize_by_template = Some(true)` auto-organizes by template after scrape
        let book = app
            .scraping
            .apply_to_file(
                root_id,
                "inbox/book_raw.epub",
                MediaKind::Book,
                ScrapedMetadata {
                    provider_id: "openlibrary".into(),
                    external_id: "OL2".into(),
                    target: Some(ScrapeTargetKind::Book),
                    title: Some("Clean Architecture".into()),
                    creators: vec!["Robert Martin".into()],
                    ..Default::default()
                },
                false,
                false,
                Some(true),
                false,
            )
            .await
            .unwrap();
        assert_eq!(
            book.entry.rel_path,
            "inbox/Robert Martin - Clean Architecture.epub"
        );
        assert!(
            inbox
                .join("Robert Martin - Clean Architecture.epub")
                .is_file()
        );

        // 3. Secondary source compatibility + Season 0 merge
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
}

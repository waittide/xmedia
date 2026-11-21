use std::collections::BTreeMap;
use std::sync::Arc;

use xmedia_core::domain::error::DomainError;
use xmedia_core::domain::library::MediaKind;

use super::ScrapingOrchestratorService;
use super::mapping::{
    apply_video_locked_fields, compute_scrape_confidence, fallback_metadata_from_candidate,
    filter_scraped_metadata_by_groups,
};
use crate::application::settings::ToolSettingsService;
use crate::application::workbench::MediaWorkbenchService;
use crate::domain::media::VideoMetaPatch;
use crate::domain::scraper::{
    AggregateScrapeResponse, MergedScrapedResult, ScrapeCandidate, ScrapeQuery, ScrapeTargetKind,
    ScrapedMetadata,
};
use crate::infra::scrapers::http_client::validate_public_http_url;

#[test]
fn compute_confidence_rewards_exact_title_creator_and_year() {
    let query = ScrapeQuery {
        keyword: "三体".into(),
        artist_or_author: Some("刘慈欣".into()),
        year: Some(2008),
        ..Default::default()
    };
    let agg = AggregateScrapeResponse {
        provider_order: vec![],
        candidates: vec![],
        selected_by_provider: BTreeMap::new(),
        details_by_provider: BTreeMap::new(),
        merged: MergedScrapedResult {
            metadata: ScrapedMetadata {
                title: Some("三体".into()),
                creators: vec!["刘慈欣".into()],
                year: Some(2008),
                ..Default::default()
            },
            field_sources: BTreeMap::new(),
        },
    };
    assert!(compute_scrape_confidence(&query, &agg) >= 90);

    let unrelated = AggregateScrapeResponse {
        merged: MergedScrapedResult {
            metadata: ScrapedMetadata {
                title: Some("完全不同的书".into()),
                ..Default::default()
            },
            field_sources: BTreeMap::new(),
        },
        ..agg
    };
    assert!(compute_scrape_confidence(&query, &unrelated) < 70);
}

#[test]
fn filter_metadata_by_groups_preserves_selected_groups_only() {
    let raw = ScrapedMetadata {
        title: Some("Test".into()),
        creators: vec!["Author".into()],
        summary: Some("Plot".into()),
        tags: vec!["Sci-Fi".into()],
        synced_lyrics: Some("[00:01.00]Hello".into()),
        cover_url: Some("https://example.com/c.jpg".into()),
        ..Default::default()
    };
    let only_summary_and_tags =
        filter_scraped_metadata_by_groups(raw.clone(), &["summary".into(), "tags".into()]);
    assert!(only_summary_and_tags.title.is_none());
    assert!(only_summary_and_tags.creators.is_empty());
    assert_eq!(only_summary_and_tags.summary.as_deref(), Some("Plot"));
    assert_eq!(only_summary_and_tags.tags, vec!["Sci-Fi"]);
    assert!(only_summary_and_tags.synced_lyrics.is_none());
    assert_eq!(
        only_summary_and_tags.cover_url.as_deref(),
        Some("https://example.com/c.jpg")
    );

    let empty_means_all = filter_scraped_metadata_by_groups(raw, &[]);
    assert_eq!(empty_means_all.title.as_deref(), Some("Test"));
    assert!(empty_means_all.synced_lyrics.is_some());
}

#[test]
fn fallback_metadata_from_candidate_preserves_novel_author_and_category() {
    let cand = ScrapeCandidate {
        provider_id: "fanqie".into(),
        provider_name: "番茄小说".into(),
        external_id: "7171641987538357284".into(),
        target: ScrapeTargetKind::Book,
        title: "测试小说".into(),
        subtitle: Some("测试作者 · 玄幻脑洞".into()),
        year: Some(2023),
        summary: Some("一段简介".into()),
        cover_url: Some("https://example.com/c.jpg".into()),
        synced_lyrics: None,
        plain_lyrics: None,
    };
    let meta = fallback_metadata_from_candidate(&cand);
    assert_eq!(meta.title.as_deref(), Some("测试小说"));
    assert_eq!(meta.creators, vec!["测试作者"]);
    assert_eq!(meta.genres, vec!["玄幻脑洞"]);
    assert_eq!(meta.publisher_or_studio.as_deref(), Some("番茄小说"));
    assert_eq!(meta.language.as_deref(), Some("zh-CN"));
    assert_eq!(
        meta.external_ids.get("fanqie").map(String::as_str),
        Some("7171641987538357284")
    );
}

#[test]
fn fallback_metadata_recognizes_qq_migu_and_17k_provider_ids() {
    for (pid, pname) in [("qq", "QQ 音乐"), ("migu", "咪咕音乐")] {
        let cand = ScrapeCandidate {
            provider_id: pid.into(),
            provider_name: pname.into(),
            external_id: "song1".into(),
            target: ScrapeTargetKind::MusicTrack,
            title: "晴天".into(),
            subtitle: Some("周杰伦 · 叶惠美".into()),
            year: Some(2003),
            summary: None,
            cover_url: None,
            synced_lyrics: None,
            plain_lyrics: None,
        };
        let meta = fallback_metadata_from_candidate(&cand);
        assert_eq!(meta.creators, vec!["周杰伦"]);
        assert_eq!(meta.series_or_album.as_deref(), Some("叶惠美"));
    }

    let k17 = ScrapeCandidate {
        provider_id: "17k".into(),
        provider_name: "17K小说网".into(),
        external_id: "123".into(),
        target: ScrapeTargetKind::Book,
        title: "小说".into(),
        subtitle: Some("作者A · 都市".into()),
        year: None,
        summary: None,
        cover_url: None,
        synced_lyrics: None,
        plain_lyrics: None,
    };
    let meta = fallback_metadata_from_candidate(&k17);
    assert_eq!(meta.creators, vec!["作者A"]);
    assert_eq!(meta.genres, vec!["都市"]);
    assert_eq!(meta.language.as_deref(), Some("zh-CN"));
}

#[test]
fn validate_public_http_url_blocks_private_cgnat_and_ipv6_ranges() {
    for blocked in [
        "http://127.0.0.1/a.jpg",
        "http://localhost/a.jpg",
        "http://foo.local/a.jpg",
        "http://10.0.0.1/a.jpg",
        "http://172.16.0.1/a.jpg",
        "http://172.31.255.1/a.jpg",
        "http://192.168.0.1/a.jpg",
        "http://169.254.169.254/latest/meta-data/",
        "http://100.64.0.1/a.jpg",
        "http://0.0.0.0/a.jpg",
        "http://[::1]/a.jpg",
        "http://[fc00::1]/a.jpg",
        "http://[fe80::1]/a.jpg",
        "http://[::ffff:127.0.0.1]/a.jpg",
        "http://[::ffff:172.16.1.1]/a.jpg",
        "file:///etc/passwd",
        "https://user:pass@1.1.1.1/a.jpg",
    ] {
        assert!(
            validate_public_http_url(blocked).is_err(),
            "expected {blocked} to be rejected"
        );
    }
    assert!(validate_public_http_url("https://1.1.1.1/cover.jpg").is_ok());
}

#[test]
fn apply_video_locked_fields_strips_locked_metadata_and_cover() {
    let mut vp = VideoMetaPatch {
        title: Some("New Title".into()),
        summary: Some("New Plot".into()),
        genres: Some(vec!["Action".into()]),
        year: Some(2025),
        ..Default::default()
    };
    let mut apply_cover = true;
    apply_video_locked_fields(
        &mut vp,
        &["title".into(), "overview".into(), "poster".into()],
        &mut apply_cover,
    );
    assert!(vp.title.is_none());
    assert!(vp.summary.is_none());
    assert_eq!(vp.genres, Some(vec!["Action".into()]));
    assert_eq!(vp.year, Some(2025));
    assert!(!apply_cover);
}

#[tokio::test]
async fn apply_video_jellyfin_archive_enforces_read_only_root() {
    use crate::infra::config_store::JsonFileConfigRepository;
    use crate::infra::inspectors::build_default_inspectors;

    let tmp = tempfile::tempdir().unwrap();
    let video_dir = tmp.path().join("videos");
    std::fs::create_dir_all(&video_dir).unwrap();
    std::fs::write(video_dir.join("Movie (2024).mkv"), b"dummy").unwrap();

    let repo = Arc::new(
        JsonFileConfigRepository::load_or_init(tmp.path().join("tool-config.json"), &[]).unwrap(),
    );
    let settings = ToolSettingsService::new(repo);
    let root = settings
        .add_root_with_options(
            "ReadOnly".into(),
            video_dir.to_string_lossy().into_owned(),
            Some(MediaKind::Video),
            true,
        )
        .await
        .unwrap();
    let ffmpeg = xmedia_core::infra::ffmpeg::FfmpegToolkit::from_ffmpeg_bin("ffmpeg");
    let workbench = MediaWorkbenchService::new(
        settings.clone(),
        build_default_inspectors(ffmpeg.clone()),
        ffmpeg,
    );
    let scraping = ScrapingOrchestratorService::new(settings, workbench, vec![]);

    let err = scraping
        .apply_to_file(
            root.id,
            "Movie (2024).mkv",
            MediaKind::Video,
            ScrapedMetadata {
                title: Some("Movie".into()),
                year: Some(2024),
                ..Default::default()
            },
            false,
            false,
            Some(true),
            false,
        )
        .await
        .unwrap_err();
    assert!(matches!(err, DomainError::Forbidden(_)));
}

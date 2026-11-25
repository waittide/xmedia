use std::collections::BTreeMap;

use super::mapping::{
    compute_scrape_confidence, fallback_metadata_from_candidate, filter_scraped_metadata_by_groups,
};
use crate::domain::media::VideoMetaPatch;
use crate::domain::media_patch::strip_video_locked_fields;
use crate::domain::scraper::{
    AggregateScrapeResponse, MergedScrapedResult, ScrapeCandidate, ScrapeQuery, ScrapeTargetKind,
    ScrapedMetadata,
};

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
fn apply_video_locked_fields_strips_locked_metadata_and_cover() {
    let mut vp = VideoMetaPatch {
        title: Some("New Title".into()),
        summary: Some("New Plot".into()),
        genres: Some(vec!["Action".into()]),
        year: Some(2025),
        ..Default::default()
    };
    let mut apply_cover = true;
    strip_video_locked_fields(
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

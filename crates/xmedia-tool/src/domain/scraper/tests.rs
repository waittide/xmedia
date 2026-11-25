use super::{
    FieldSourceBadge, ScrapeQuery, ScrapeTargetKind, ScrapedMetadata,
    is_compatible_secondary_source, merge_scraped_by_priority, promote_exact_match_anchor,
};

#[test]
fn waterfall_merge_fills_missing_fields_by_priority_and_tracks_provenance() {
    let primary = (
        FieldSourceBadge {
            provider_id: "bangumi".into(),
            provider_name: "Bangumi 番组计划".into(),
        },
        ScrapedMetadata {
            provider_id: "bangumi".into(),
            external_id: "1001".into(),
            title: Some("三体".into()),
            creators: vec!["刘慈欣".into()],
            summary: Some("地球往事中文简介".into()),
            cover_url: None,
            isbn: None,
            ..Default::default()
        },
    );
    let secondary = (
        FieldSourceBadge {
            provider_id: "openlibrary".into(),
            provider_name: "OpenLibrary".into(),
        },
        ScrapedMetadata {
            provider_id: "openlibrary".into(),
            external_id: "OL17365W".into(),
            title: Some("The Three-Body Problem".into()),
            summary: Some("English summary".into()),
            cover_url: Some("https://covers.openlibrary.org/b/id/1-L.jpg".into()),
            isbn: Some("9787536692930".into()),
            ..Default::default()
        },
    );

    let merged = merge_scraped_by_priority(&[primary, secondary], Some(ScrapeTargetKind::Book));
    assert_eq!(merged.metadata.title.as_deref(), Some("三体"));
    assert_eq!(merged.metadata.summary.as_deref(), Some("地球往事中文简介"));
    assert_eq!(merged.field_sources["title"].provider_id, "bangumi");
    assert_eq!(merged.field_sources["summary"].provider_id, "bangumi");

    assert_eq!(
        merged.metadata.cover_url.as_deref(),
        Some("https://covers.openlibrary.org/b/id/1-L.jpg")
    );
    assert_eq!(merged.metadata.isbn.as_deref(), Some("9787536692930"));
    assert_eq!(merged.field_sources["cover_url"].provider_id, "openlibrary");
    assert_eq!(merged.field_sources["isbn"].provider_id, "openlibrary");
}

#[test]
fn promote_exact_match_anchor_elevates_canonical_exclusive_novel_over_fanfic() {
    let mut raw = vec![
        (
            FieldSourceBadge {
                provider_id: "qidian".into(),
                provider_name: "起点中文网".into(),
            },
            ScrapedMetadata {
                provider_id: "qidian".into(),
                external_id: "999".into(),
                title: Some("十日终焉：我在异界斩神".into()),
                creators: vec!["路人甲".into()],
                ..Default::default()
            },
        ),
        (
            FieldSourceBadge {
                provider_id: "fanqie".into(),
                provider_name: "番茄小说".into(),
            },
            ScrapedMetadata {
                provider_id: "fanqie".into(),
                external_id: "7143038691944959011".into(),
                title: Some("十日终焉".into()),
                creators: vec!["杀虫队队员".into()],
                ..Default::default()
            },
        ),
    ];

    let query = ScrapeQuery {
        keyword: "十日终焉".into(),
        target: Some(ScrapeTargetKind::Book),
        ..Default::default()
    };
    promote_exact_match_anchor(&mut raw, &query);
    assert_eq!(raw[0].0.provider_id, "fanqie");
    assert_eq!(raw[0].1.title.as_deref(), Some("十日终焉"));
    assert!(!is_compatible_secondary_source(
        &raw[0].1,
        &raw[1].1,
        Some(ScrapeTargetKind::Book)
    ));
}

#[test]
fn music_secondary_source_blocks_cover_singer_and_allows_cross_script_artist() {
    let qq_primary = ScrapedMetadata {
        provider_id: "qq".into(),
        external_id: "0039MnYb0qxYhV".into(),
        target: Some(ScrapeTargetKind::MusicTrack),
        title: Some("晴天".into()),
        series_or_album: Some("叶惠美".into()),
        creators: vec!["周杰伦".into()],
        year: Some(2003),
        ..Default::default()
    };

    let cover_song = ScrapedMetadata {
        provider_id: "netease".into(),
        external_id: "2668397359".into(),
        target: Some(ScrapeTargetKind::MusicTrack),
        title: Some("晴天 (原唱 周杰伦)".into()),
        creators: vec!["RyaVocal".into()],
        ..Default::default()
    };
    assert!(!is_compatible_secondary_source(
        &qq_primary,
        &cover_song,
        Some(ScrapeTargetKind::MusicTrack)
    ));

    let other_cjk_singer = ScrapedMetadata {
        provider_id: "lrclib".into(),
        external_id: "123".into(),
        target: Some(ScrapeTargetKind::MusicTrack),
        title: Some("晴天".into()),
        creators: vec!["别的歌手".into()],
        ..Default::default()
    };
    assert!(!is_compatible_secondary_source(
        &qq_primary,
        &other_cjk_singer,
        Some(ScrapeTargetKind::MusicTrack)
    ));

    let cross_script_same_artist = ScrapedMetadata {
        provider_id: "itunes".into(),
        external_id: "US:1".into(),
        target: Some(ScrapeTargetKind::MusicTrack),
        title: Some("晴天".into()),
        creators: vec!["Jay Chou".into()],
        year: Some(2003),
        ..Default::default()
    };
    assert!(is_compatible_secondary_source(
        &qq_primary,
        &cross_script_same_artist,
        Some(ScrapeTargetKind::MusicTrack)
    ));
}

#[test]
fn promote_anchor_ignores_empty_or_short_normalized_titles() {
    let mut sources = vec![
        (
            FieldSourceBadge {
                provider_id: "bangumi".into(),
                provider_name: "Bangumi".into(),
            },
            ScrapedMetadata {
                provider_id: "bangumi".into(),
                external_id: "1".into(),
                title: Some("Page Blanche Vol.1".into()),
                creators: vec!["Other".into()],
                ..Default::default()
            },
        ),
        (
            FieldSourceBadge {
                provider_id: "douban".into(),
                provider_name: "Douban".into(),
            },
            ScrapedMetadata {
                provider_id: "douban".into(),
                external_id: "2".into(),
                title: Some("...".into()),
                creators: vec!["Boulet".into()],
                ..Default::default()
            },
        ),
    ];
    let query = ScrapeQuery {
        target: Some(ScrapeTargetKind::Comic),
        keyword: "Page Blanche".into(),
        artist_or_author: Some("Boulet".into()),
        ..Default::default()
    };
    promote_exact_match_anchor(&mut sources, &query);
    assert_eq!(sources[0].0.provider_id, "bangumi");
}

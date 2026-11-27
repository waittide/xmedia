use crate::domain::scraper::ScrapeTargetKind;

use super::anilist::{
    AlCover, AlDate, AlMedia, AlPersonName, AlStaffConnection, AlStaffEdge, AlStaffNode, AlTitle,
    map_anilist_detail,
};
use super::bangumi::{BgmEpisodeItem, BgmSubject, map_bgm_detail};
use super::tmdb::{
    TmdbDetailBody, TmdbEpisodeBody, TmdbSeasonBody, map_tmdb_detail, parse_tmdb_external_id,
};

#[test]
fn acgn_and_video_mappers_normalize_bangumi_tmdb_and_anilist() {
    let bgm: BgmSubject = serde_json::from_str(
        r#"{
            "id": 400602,
            "type": 2,
            "name": "葬送のフリーレン",
            "name_cn": "葬送的芙莉莲",
            "summary": "打倒魔王之后的故事。",
            "date": "2023-09-29",
            "rating": {"score": 8.9},
            "tags": [{"name": "奇幻"}, {"name": "MADHOUSE"}],
            "images": {"large": "https://lain.bgm.tv/pic/cover/l/frieren.jpg"},
            "infobox": [
                {"key": "原作", "value": "山田鐘人、アベツカサ"},
                {"key": "导演", "value": "斎藤圭一郎"},
                {"key": "动画制作", "value": "MADHOUSE"}
            ]
        }"#,
    )
    .unwrap();
    let bgm_meta = map_bgm_detail(bgm, ScrapeTargetKind::TvShow, Some((1, 1)), None);
    assert_eq!(bgm_meta.title.as_deref(), Some("葬送的芙莉莲"));
    assert_eq!(bgm_meta.illustrators_or_directors, vec!["斎藤圭一郎"]);
    assert_eq!(bgm_meta.publisher_or_studio.as_deref(), Some("MADHOUSE"));
    assert_eq!(bgm_meta.year, Some(2023));

    let bgm_music_meta = map_bgm_detail(
        BgmSubject {
            id: 46023,
            subject_type: 3,
            name: "紅蓮華 音楽全集".into(),
            date: Some("2012-09-28".into()),
            ..Default::default()
        },
        ScrapeTargetKind::MusicTrack,
        None,
        Some(BgmEpisodeItem {
            id: 189876,
            ep: Some(1.0),
            sort: 1.0,
            disc: 2,
            name: "BLAZE MOMENT".into(),
            name_cn: "佐咲紗花".into(),
            ..Default::default()
        }),
    );
    assert_eq!(bgm_music_meta.title.as_deref(), Some("BLAZE MOMENT"));
    assert_eq!(
        bgm_music_meta.series_or_album.as_deref(),
        Some("紅蓮華 音楽全集")
    );
    assert_eq!(bgm_music_meta.track_or_episode_number, Some(1));
    assert_eq!(bgm_music_meta.disc_or_season_number, Some(2));
    assert_eq!(bgm_music_meta.creators, vec!["佐咲紗花".to_string()]);

    assert_eq!(
        parse_tmdb_external_id("209867:S1E2"),
        ("209867", Some((1, 2)))
    );
    let tmdb_tv: TmdbDetailBody = serde_json::from_str(
        r#"{
            "id": 209867,
            "name": "葬送的芙莉莲",
            "original_name": "葬送のフリーレン",
            "first_air_date": "2023-09-29",
            "poster_path": "/poster.jpg",
            "backdrop_path": "/backdrop.jpg",
            "genres": [{"name": "动画"}, {"name": "奇幻"}],
            "networks": [{"name": "Nippon TV"}],
            "credits": {
                "cast": [{"name": "种崎敦美"}],
                "crew": [{"name": "斋藤圭一郎", "job": "Director"}]
            }
        }"#,
    )
    .unwrap();
    let tmdb_season = TmdbSeasonBody {
        name: Some("第 1 季".into()),
        overview: Some("第一季总览简介。".into()),
        air_date: Some("2023-09-29".into()),
        poster_path: Some("/season1.jpg".into()),
        season_number: Some(1),
        episodes: Vec::new(),
    };
    let tmdb_ep = TmdbEpisodeBody {
        name: Some("不必是魔法也无妨".into()),
        overview: Some("芙莉莲与菲伦继续旅行。".into()),
        air_date: Some("2023-09-29".into()),
        vote_average: Some(8.8),
        season_number: Some(1),
        episode_number: Some(2),
        still_path: Some("/ep2_still.jpg".into()),
    };
    let tmdb_meta = map_tmdb_detail(
        tmdb_tv,
        ScrapeTargetKind::TvShow,
        Some(tmdb_season),
        Some(tmdb_ep),
    );
    assert_eq!(tmdb_meta.title.as_deref(), Some("不必是魔法也无妨"));
    assert_eq!(tmdb_meta.series_or_album.as_deref(), Some("葬送的芙莉莲"));
    assert_eq!(tmdb_meta.disc_or_season_number, Some(1));
    assert_eq!(tmdb_meta.track_or_episode_number, Some(2));
    assert_eq!(tmdb_meta.cast, vec!["种崎敦美"]);
    assert_eq!(
        tmdb_meta.cover_url.as_deref(),
        Some("https://image.tmdb.org/t/p/w780/poster.jpg")
    );
    assert_eq!(
        tmdb_meta.season_poster_url.as_deref(),
        Some("https://image.tmdb.org/t/p/w780/season1.jpg")
    );
    assert_eq!(
        tmdb_meta.episode_still_url.as_deref(),
        Some("https://image.tmdb.org/t/p/w780/ep2_still.jpg")
    );

    let al_meta = map_anilist_detail(
        AlMedia {
            id: 118586,
            country_of_origin: Some("JP".into()),
            title: AlTitle {
                native: Some("葬送のフリーレン".into()),
                english: Some("Frieren: Beyond Journey's End".into()),
                romaji: Some("Sousou no Frieren".into()),
            },
            synonyms: vec!["葬送的芙莉莲".into()],
            start_date: Some(AlDate {
                year: Some(2020),
                month: Some(4),
                day: Some(28),
            }),
            average_score: Some(91.0),
            cover_image: Some(AlCover {
                extra_large: Some("https://s4.anilist.co/cover.jpg".into()),
                large: None,
            }),
            staff: Some(AlStaffConnection {
                edges: vec![
                    AlStaffEdge {
                        role: "Story".into(),
                        node: AlStaffNode {
                            name: AlPersonName {
                                native: Some("山田鐘人".into()),
                                full: None,
                            },
                        },
                    },
                    AlStaffEdge {
                        role: "Art".into(),
                        node: AlStaffNode {
                            name: AlPersonName {
                                native: Some("アベツカサ".into()),
                                full: None,
                            },
                        },
                    },
                ],
            }),
            ..Default::default()
        },
        ScrapeTargetKind::Comic,
        "zh-CN",
    );
    assert_eq!(al_meta.title.as_deref(), Some("葬送的芙莉莲"));
    assert_eq!(al_meta.creators, vec!["山田鐘人".to_string()]);
    assert_eq!(
        al_meta.illustrators_or_directors,
        vec!["アベツカサ".to_string()]
    );
}

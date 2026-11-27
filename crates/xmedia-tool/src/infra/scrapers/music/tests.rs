use base64::Engine;
use base64::engine::general_purpose::STANDARD as BASE64_STANDARD;

use crate::application::ports::ScraperProviderPort;
use crate::domain::config::ProviderCredentials;
use crate::domain::scraper::{ScrapeQuery, ScrapeTargetKind};

use super::itunes::{ItunesItem, map_itunes_detail};
use super::kugou::{KgAlbumItem, KgPlaySongInfo, KugouMusicProvider, map_kugou_song_detail};
use super::kuwo::{
    KuwoMusicProvider, KwAlbumSearchItem, KwAlbumTrackItem, KwLrcItem, KwSongAndLrcData,
    KwSongDetailInfo, map_kuwo_song_detail,
};
use super::lrclib::LrclibRecord;
use super::migu::{
    MiguAlbumResource, MiguImgItem, MiguMusicProvider, MiguSongResource, map_migu_song_detail,
};
use super::musicbrainz::{MbRecordingItem, map_recording_detail};
use super::netease::{
    NeAlbumInfo, NeArtist, NeLyricBlock, NeLyricEnvelope, NeSearchSong, NeteaseMusicProvider,
    map_netease_album_detail, map_netease_song_detail,
};
use super::qishui::{
    QishuiMusicProvider, QsAlbumInfo, QsAlbumTrackRef, QsArtist, QsLyricPayload,
    QsSeoTrackEnvelope, QsSeoTrackWrap, QsTrackCore, QsUrlCover, map_qishui_seo_track,
};
use super::qq_music::{
    QqAlbumBrief, QqInfoContentEntry, QqInfoField, QqLyricResp, QqMusicProvider, QqSinger,
    QqSongDetailData, QqSongExtraInfo, QqSongItem, map_qq_song_detail,
};
use super::taihe::{
    TaiheArtist, TaiheMusicProvider, TaiheTrackItem, build_taihe_signed_params,
    map_taihe_song_detail,
};

#[test]
fn western_and_global_music_mappers_normalize_itunes_lrclib_and_musicbrainz() {
    let itunes_item: ItunesItem = serde_json::from_str(
        r#"{
            "wrapperType": "track",
            "trackId": 535824738,
            "collectionName": "葉惠美",
            "trackName": "晴天",
            "artistName": "周杰倫",
            "artworkUrl100": "https://is1-ssl.mzstatic.com/image/thumb/Music115/JAY.jpg/100x100bb.jpg",
            "releaseDate": "2003-07-31T07:00:00Z",
            "trackNumber": 3,
            "discNumber": 1,
            "primaryGenreName": "Mandopop"
        }"#,
    )
    .unwrap();
    let it_meta = map_itunes_detail(itunes_item, "TW:535824738", ScrapeTargetKind::MusicTrack);
    assert_eq!(it_meta.title.as_deref(), Some("晴天"));
    assert_eq!(it_meta.track_or_episode_number, Some(3));
    assert_eq!(
        it_meta.cover_url.as_deref(),
        Some("https://is1-ssl.mzstatic.com/image/thumb/Music115/JAY.jpg/1000x1000bb.jpg")
    );

    let lrc: LrclibRecord = serde_json::from_str(
        r#"{
            "id": 42,
            "trackName": "晴天",
            "artistName": "周杰伦",
            "albumName": "叶惠美",
            "duration": 269.5,
            "syncedLyrics": "[00:28.90]故事的小黄花",
            "plainLyrics": "故事的小黄花"
        }"#,
    )
    .unwrap();
    let lrc_meta = lrc.into_metadata(ScrapeTargetKind::Lyrics);
    assert_eq!(lrc_meta.title.as_deref(), Some("晴天"));
    assert_eq!(lrc_meta.creators, vec!["周杰伦"]);
    assert_eq!(
        lrc_meta.synced_lyrics.as_deref(),
        Some("[00:28.90]故事的小黄花")
    );

    let mb: MbRecordingItem = serde_json::from_str(
        r#"{
            "id": "rec-123",
            "title": "Blue Bird",
            "first-release-date": "2008-07-09",
            "artist-credit": [{"name": "いきものがかり"}],
            "releases": [{
                "id": "rel-999",
                "title": "My song Your song",
                "media": [{"position": 1, "track": [{"number": "3", "position": 3}]}]
            }],
            "genres": [{"name": "j-pop"}]
        }"#,
    )
    .unwrap();
    let mb_meta = map_recording_detail(mb);
    assert_eq!(
        mb_meta.series_or_album.as_deref(),
        Some("My song Your song")
    );
    assert_eq!(mb_meta.track_or_episode_number, Some(3));
    assert_eq!(mb_meta.year, Some(2008));
    assert_eq!(
        mb_meta.cover_url.as_deref(),
        Some("https://coverartarchive.org/release/rel-999/front-500")
    );
}

#[test]
fn chinese_music_parsers_normalize_all_seven_platforms_and_bilingual_lrc() {
    let qq_orig_b64 = BASE64_STANDARD.encode(
        "[ti:Lemon]\n[00:00.85]夢ならばどれほどよかったでしょう\n[00:06.65]未だにあなたのことを夢にみる",
    );
    let qq_trans_b64 = BASE64_STANDARD.encode("[00:00.85]如果这一切都是梦境该有多好\n[00:06.65]//");
    let qq_meta = map_qq_song_detail(
        QqSongDetailData {
            track_info: QqSongItem {
                id: 1,
                mid: "0039MnYb0qxYhV".into(),
                name: "Lemon".into(),
                index_album: Some(1),
                index_cd: Some(0),
                time_public: "2018-03-14".into(),
                singer: vec![QqSinger {
                    name: "米津玄師".into(),
                    ..Default::default()
                }],
                album: QqAlbumBrief {
                    mid: "000MkMni19ClKG".into(),
                    name: "Lemon".into(),
                    ..Default::default()
                },
                ..Default::default()
            },
            info: QqSongExtraInfo {
                company: Some(QqInfoField {
                    content: vec![QqInfoContentEntry {
                        value: "Sony Music".into(),
                    }],
                }),
                genre: Some(QqInfoField {
                    content: vec![QqInfoContentEntry {
                        value: "J-Pop".into(),
                    }],
                }),
                ..Default::default()
            },
        },
        "0039MnYb0qxYhV",
        ScrapeTargetKind::MusicTrack,
        Some(QqLyricResp {
            retcode: 0,
            lyric: qq_orig_b64,
            trans: qq_trans_b64,
        }),
    );
    assert_eq!(qq_meta.title.as_deref(), Some("Lemon"));
    assert_eq!(qq_meta.track_or_episode_number, Some(1));
    assert_eq!(qq_meta.disc_or_season_number, Some(1));
    assert_eq!(qq_meta.year, Some(2018));
    assert_eq!(
        qq_meta.cover_url.as_deref(),
        Some("https://y.gtimg.cn/music/photo_new/T002R800x800M000000MkMni19ClKG.jpg")
    );
    let synced = qq_meta.synced_lyrics.as_deref().unwrap();
    assert!(synced.contains("[00:00.85]夢ならばどれほどよかったでしょう"));
    assert!(synced.contains("[00:00.85]如果这一切都是梦境该有多好"));
    assert!(!synced.contains("//"));

    let ne_song = map_netease_song_detail(
        NeSearchSong {
            id: 536622304,
            name: "新的心跳".into(),
            artists: vec![NeArtist {
                name: "G.E.M.邓紫棋".into(),
            }],
            album: Some(NeAlbumInfo {
                id: 3189002,
                name: "新的心跳".into(),
                pic_url: Some("http://p2.music.126.net/abc/123.jpg".into()),
                publish_time: Some(1446739200007),
                company: Some("蜂鸟音乐".into()),
                ..Default::default()
            }),
            no: Some(2),
            disc: Some("01".into()),
            ..Default::default()
        },
        "536622304",
        ScrapeTargetKind::MusicTrack,
        Some(NeLyricEnvelope {
            lrc: Some(NeLyricBlock {
                lyric: "[00:12.34]心跳".into(),
            }),
            tlyric: None,
        }),
    );
    assert_eq!(ne_song.year, Some(2015));
    assert_eq!(ne_song.published_date.as_deref(), Some("2015-11-06"));
    assert_eq!(ne_song.track_or_episode_number, Some(2));
    assert_eq!(ne_song.disc_or_season_number, Some(1));

    let ne_alb = map_netease_album_detail(
        NeAlbumInfo {
            id: 3189002,
            name: "新的心跳".into(),
            description: Some("全创作视听专辑".into()),
            ..Default::default()
        },
        "3189002",
    );
    assert_eq!(ne_alb.target, Some(ScrapeTargetKind::MusicAlbum));
    assert_eq!(ne_alb.summary.as_deref(), Some("全创作视听专辑"));

    let kg_meta = map_kugou_song_detail(
        "b3a52a7a958bf0aed0ebfba2e9a818b7:966846",
        ScrapeTargetKind::MusicTrack,
        KgPlaySongInfo {
            song_name: "晴天".into(),
            singer_name: "周杰伦".into(),
            album_img: "http://imge.kugou.com/stdmusic/{size}/20230920/test.jpg".into(),
            ..Default::default()
        },
        Some(KgAlbumItem {
            albumid: serde_json::json!(966846),
            albumname: "叶惠美".into(),
            publishtime: "2003-07-31 00:00:00".into(),
            imgurl: "http://imge.kugou.com/stdmusic/{size}/20230920/test.jpg".into(),
            ..Default::default()
        }),
        Some(3),
        Some("[ti:晴天]\r\n[00:28.90]故事的小黄花"),
    );
    assert_eq!(kg_meta.track_or_episode_number, Some(3));
    assert_eq!(kg_meta.year, Some(2003));
    assert_eq!(
        kg_meta.cover_url.as_deref(),
        Some("https://imge.kugou.com/stdmusic/1000/20230920/test.jpg")
    );
    assert_eq!(kg_meta.plain_lyrics.as_deref(), Some("故事的小黄花"));

    let kw_meta = map_kuwo_song_detail(
        "228908",
        ScrapeTargetKind::MusicTrack,
        KwSongAndLrcData {
            songinfo: Some(KwSongDetailInfo {
                id: "228908".into(),
                song_name: "晴天".into(),
                artist: "周杰伦".into(),
                album: "叶惠美".into(),
                album_id: "1293".into(),
                pic: "http://img1.kwcdn.kuwo.cn/star/albumcover/240/s3s94/93/211513640.jpg".into(),
                ..Default::default()
            }),
            lrclist: Some(vec![KwLrcItem {
                time: "28.9".into(),
                line_lyric: "故事的小黄花".into(),
            }]),
        },
        Some(KwAlbumSearchItem {
            albumid: "1293".into(),
            name: "叶惠美".into(),
            pub_date: "2003-07-31".into(),
            company: "杰威尔音乐有限公司".into(),
            musiclist: vec![KwAlbumTrackItem {
                id: "228908".into(),
                track: "3".into(),
                ..Default::default()
            }],
            ..Default::default()
        }),
        None,
    );
    assert_eq!(kw_meta.track_or_episode_number, Some(3));
    assert_eq!(kw_meta.year, Some(2003));
    assert_eq!(
        kw_meta.synced_lyrics.as_deref(),
        Some("[00:28.90]故事的小黄花")
    );

    let migu_meta = map_migu_song_detail(
        "3790007:8592",
        ScrapeTargetKind::MusicTrack,
        MiguSongResource {
            song_name: "晴天".into(),
            singer: "周杰伦".into(),
            album: "叶惠美".into(),
            track_number: Some(serde_json::json!(3)),
            disc: Some(serde_json::json!("1")),
            album_imgs: vec![MiguImgItem {
                img_size_type: "03".into(),
                img: "https://d.musicapp.migu.cn/cover.webp".into(),
                ..Default::default()
            }],
            ..Default::default()
        },
        Some(MiguAlbumResource {
            album_id: "8592".into(),
            publish_date: "2003-07-31".into(),
            publish_company: "杰威尔音乐".into(),
            ..Default::default()
        }),
        Some("[00:01.00]晴天 - 周杰伦"),
    );
    assert_eq!(migu_meta.track_or_episode_number, Some(3));
    assert_eq!(migu_meta.disc_or_season_number, Some(1));
    assert_eq!(migu_meta.year, Some(2003));

    let signed = build_taihe_signed_params(&[("TSID", "T1001")], 1700000000);
    assert!(signed.iter().any(|(k, v)| k == "sign" && v.len() == 32));
    let taihe_meta = map_taihe_song_detail(
        TaiheTrackItem {
            tsid: "T1001".into(),
            title: "千百度".into(),
            artist: vec![
                TaiheArtist {
                    name: "作词人".into(),
                    artist_type: 4,
                    artist_type_name: "作词者".into(),
                },
                TaiheArtist {
                    name: "许嵩".into(),
                    artist_type: 38,
                    artist_type_name: "歌手".into(),
                },
            ],
            album_title: "苏格拉没有底".into(),
            release_date: "2011-04-01T00:00:00.000Z".into(),
            sort: Some(2),
            pic: "https://img01.dmhmusic.com/cover.jpg".into(),
            ..Default::default()
        },
        "T1001",
        ScrapeTargetKind::MusicTrack,
        Some("[00:10.00]关外野店"),
    );
    assert_eq!(taihe_meta.creators, vec!["许嵩".to_string()]);
    assert_eq!(taihe_meta.track_or_episode_number, Some(2));

    let qs_meta = map_qishui_seo_track(
        QsSeoTrackEnvelope {
            seo_track: Some(QsSeoTrackWrap {
                track: Some(QsTrackCore {
                    id: "7651571345679583248".into(),
                    name: "粗糙".into(),
                    artists: vec![QsArtist {
                        name: "许嵩".into(),
                    }],
                    album: Some(QsAlbumInfo {
                        id: "7651571345679566864".into(),
                        name: "安泊猜想".into(),
                        release_date: 1781539200,
                        url_cover: Some(QsUrlCover {
                            uri: "tos-cn-v-2774c002/demo".into(),
                            urls: vec!["https://p3-luna.douyinpic.com/img/".into()],
                        }),
                        ..Default::default()
                    }),
                    ..Default::default()
                }),
                genre_tag: "摇滚".into(),
            }),
            lyric: Some(QsLyricPayload {
                content: "[9560,1950]<0,180,0>我<180,170,0>们<400,180,0>一<580,170,0>起".into(),
            }),
            album_tracks: vec![
                QsAlbumTrackRef {
                    id: "1".into(),
                    track: None,
                },
                QsAlbumTrackRef {
                    id: "7651571345679583248".into(),
                    track: None,
                },
            ],
        },
        "7651571345679583248",
        ScrapeTargetKind::MusicTrack,
    )
    .unwrap();
    assert_eq!(qs_meta.track_or_episode_number, Some(2));
    assert_eq!(qs_meta.synced_lyrics.as_deref(), Some("[00:09.56]我们一起"));
}

#[tokio::test]
#[ignore = "live network smoke check across Chinese music providers"]
async fn live_smoke_check_chinese_music_providers() {
    let creds = ProviderCredentials::default();
    let cases: Vec<(&str, Box<dyn ScraperProviderPort>)> = vec![
        ("qq", Box::new(QqMusicProvider)),
        ("netease", Box::new(NeteaseMusicProvider)),
        ("kugou", Box::new(KugouMusicProvider)),
        ("kuwo", Box::new(KuwoMusicProvider)),
        ("migu", Box::new(MiguMusicProvider)),
        ("taihe", Box::new(TaiheMusicProvider)),
        ("qishui", Box::new(QishuiMusicProvider)),
    ];
    for (name, prov) in cases {
        let _ = (
            name,
            prov.search(
                &ScrapeQuery {
                    keyword: "晴天".into(),
                    target: Some(ScrapeTargetKind::MusicTrack),
                    ..Default::default()
                },
                &creds,
            )
            .await,
        );
    }
}

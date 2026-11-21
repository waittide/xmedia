use crate::domain::config::ProviderCredentials;
use crate::domain::scraper::ScrapeTargetKind;
use crate::infra::scrapers::bangumi::{BgmSubject, map_bgm_detail};
use crate::infra::scrapers::build_default_scraper_providers;
use crate::infra::scrapers::itunes::{ItunesItem, map_itunes_detail};
use crate::infra::scrapers::lrclib::LrclibRecord;
use crate::infra::scrapers::musicbrainz::{MbRecordingItem, map_recording_detail};
use crate::infra::scrapers::openlibrary::{OlSearchDoc, map_ol_doc_to_candidate};
use crate::infra::scrapers::tmdb::{
    TmdbDetailBody, TmdbEpisodeBody, TmdbSeasonBody, map_tmdb_detail, parse_tmdb_external_id,
};

#[test]
fn default_registry_exposes_twenty_eight_providers_and_checks_credentials_and_adult_gate() {
    let providers = build_default_scraper_providers();
    assert_eq!(providers.len(), 28);

    let empty_creds = ProviderCredentials::default();
    let configured_ids: Vec<String> = providers
        .iter()
        .filter(|p| p.is_configured(&empty_creds))
        .map(|p| p.descriptor().id)
        .collect();
    assert_eq!(
        configured_ids,
        vec![
            "douban",
            "neodb",
            "xinhua",
            "qidian",
            "fanqie",
            "qimao",
            "17k",
            "ciweimao",
            "sfacg",
            "faloo",
            "jjwxc",
            "qq",
            "netease",
            "kugou",
            "kuwo",
            "migu",
            "taihe",
            "qishui",
            "itunes",
            "musicbrainz",
            "lrclib",
            "openlibrary",
            "bangumi",
            "anilist",
            "mangadex"
        ]
    );

    let with_tmdb = ProviderCredentials {
        tmdb_api_key: Some("0123456789abcdef0123456789abcdef".into()),
        ..Default::default()
    };
    let without_adult_ids: Vec<String> = providers
        .iter()
        .filter(|p| !p.is_configured(&with_tmdb))
        .map(|p| p.descriptor().id)
        .collect();
    assert_eq!(without_adult_ids, vec!["nhentai", "hanime1"]);

    let with_all = ProviderCredentials {
        tmdb_api_key: Some("0123456789abcdef0123456789abcdef".into()),
        enable_adult_scrapers: true,
        ..Default::default()
    };
    assert!(providers.iter().all(|p| p.is_configured(&with_all)));
}

#[test]
fn provider_payload_mappers_normalize_all_sources_cleanly() {
    // 0. Apple Music / iTunes
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
    // 1. LRCLIB
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

    // 2. MusicBrainz + Cover Art Archive
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

    // 3. OpenLibrary
    let ol: OlSearchDoc = serde_json::from_str(
        r#"{
            "key": "/works/OL17365W",
            "title": "The Three-Body Problem",
            "author_name": ["Cixin Liu"],
            "first_publish_year": 2006,
            "publisher": ["Tor Books"],
            "cover_i": 8231990
        }"#,
    )
    .unwrap();
    let ol_cand = map_ol_doc_to_candidate(ol);
    assert_eq!(ol_cand.external_id, "OL17365W");
    assert_eq!(
        ol_cand.cover_url.as_deref(),
        Some("https://covers.openlibrary.org/b/id/8231990-L.jpg")
    );

    // 4. Bangumi
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

    // 5. TMDB TV Show + Episode S01E02
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
}

#[test]
fn chinese_webnovel_parsers_normalize_all_eight_platforms_and_skip_captcha() {
    use crate::domain::scraper::ScrapeQuery;
    use crate::infra::scrapers::ciweimao::{
        parse_ciweimao_detail_html, parse_ciweimao_search_html,
    };
    use crate::infra::scrapers::faloo::{parse_faloo_detail_html, parse_faloo_search_html};
    use crate::infra::scrapers::fanqie::{
        FanqieSearchItem, map_fanqie_search_items, parse_fanqie_detail_html,
    };
    use crate::infra::scrapers::jjwxc::{
        JjwxcNovelDetail, map_jjwxc_detail, parse_jjwxc_wap_search_html,
    };
    use crate::infra::scrapers::k17::{map_k17_search_values, parse_k17_detail_html};
    use crate::infra::scrapers::qidian::{parse_qidian_detail_html, parse_qidian_search_html};
    use crate::infra::scrapers::qimao::{
        QimaoSearchItem, map_qimao_search_items, parse_qimao_detail_html,
    };
    use crate::infra::scrapers::sfacg::{parse_sfacg_detail_html, parse_sfacg_search_html};

    let q = ScrapeQuery {
        keyword: "诡秘之主".into(),
        ..Default::default()
    };

    // 1. Qidian SSR search + detail
    let qd_search_html = r#"<html><script id="vite-plugin-ssr_pageContext">{"pageContext":{"pageProps":{"pageData":{"bookInfo":{"records":[{"bid":1010868264,"bName":"诡秘之主","bAuth":"爱潜水的乌贼","desc":"蒸汽与机械的浪潮中<br>愚者的传说","subCateName":"异世大陆","imgUrl":"//bookcover.yuewen.com/qdbimg/349573/1010868264/180","lastUpdateTime":"2022-11-25","recomendCnt":37503230}]}}}}}</script></html>"#;
    let qd_cands = parse_qidian_search_html(qd_search_html, &q);
    assert_eq!(qd_cands.len(), 1);
    assert_eq!(qd_cands[0].external_id, "1010868264");
    assert_eq!(
        qd_cands[0].cover_url.as_deref(),
        Some("https://bookcover.yuewen.com/qdbimg/349573/1010868264/600")
    );
    let qd_detail_html = r#"<html><script id="vite-plugin-ssr_pageContext">{"pageContext":{"pageProps":{"pageData":{"bookInfo":{"bName":"诡秘之主","authorName":"爱潜水的乌贼","chanName":"玄幻","subCateName":"异世大陆","bookTag":{"tagName":"蒸汽朋克-克苏鲁"},"updTime":"2022-11-25","desc":"　　蒸汽与机械<br>　　愚者传说"}}}}}</script></html>"#;
    let qd_meta = parse_qidian_detail_html("1010868264", qd_detail_html).unwrap();
    assert_eq!(qd_meta.title.as_deref(), Some("诡秘之主"));
    assert_eq!(qd_meta.creators, vec!["爱潜水的乌贼"]);
    assert_eq!(qd_meta.genres, vec!["玄幻"]);
    assert_eq!(qd_meta.tags, vec!["异世大陆", "蒸汽朋克", "克苏鲁"]);
    assert_eq!(qd_meta.year, Some(2022));

    // 2. Fanqie search + detail
    let fq_cands = map_fanqie_search_items(
        vec![FanqieSearchItem {
            book_id: "7143038691944959011".into(),
            title: "<em>十日终焉</em>".into(),
            author: "杀虫队队员".into(),
            abstract_text: "终焉之地".into(),
            category: "推理".into(),
            thumb_url: "https://p3.byteimg.com/cover.webp".into(),
            ..Default::default()
        }],
        &ScrapeQuery {
            keyword: "十日终焉".into(),
            ..Default::default()
        },
    );
    assert_eq!(fq_cands[0].title, "十日终焉");
    let fq_detail_html = r#"<html><script type="application/ld+json">{"author":[{"name":"杀虫队队员"}],"dateModified":"2025-10-31T22:10:23"}</script><img class="book-cover-img" src="//p1-tt.byteimg.com/origin/novel-static/5cb0"/><div class="info-name"><h1>十日终焉</h1></div><div class="info-label"><span class="info-label-yellow">已完结</span><span class="info-label-grey">悬疑脑洞</span><span class="info-label-grey">无限流</span></div><div class="page-abstract-content"><p>终焉之地。</p></div></html>"#;
    let fq_meta = parse_fanqie_detail_html("7143038691944959011", fq_detail_html).unwrap();
    assert_eq!(fq_meta.title.as_deref(), Some("十日终焉"));
    assert_eq!(fq_meta.creators, vec!["杀虫队队员"]);
    assert_eq!(fq_meta.genres, vec!["悬疑脑洞"]);
    assert_eq!(fq_meta.tags, vec!["悬疑脑洞", "无限流"]);
    assert_eq!(
        fq_meta.cover_url.as_deref(),
        Some("https://p1-tt.byteimg.com/origin/novel-static/5cb0")
    );

    // 3. Qimao search + detail
    let qm_cands = map_qimao_search_items(
        vec![QimaoSearchItem {
            book_id: "1790589".into(),
            title: "万剑圣帝".into(),
            author: "月下乘船来".into(),
            intro: "一剑在手".into(),
            image_link: "https://cdn.wtzw.com/cover.jpg".into(),
            category2_name: "东方玄幻".into(),
            update_time: "2024-01-26 14:13:44".into(),
        }],
        &ScrapeQuery {
            keyword: "万剑圣帝".into(),
            ..Default::default()
        },
    );
    assert_eq!(qm_cands[0].year, Some(2024));
    let qm_detail_html = r#"<div class="wrap-pic"><img src="https://cdn.wtzw.com/cover.jpg"></div><div class="wrap-txt"><div class="title clearfix"><span class="txt">万剑圣帝</span><span class="score">8.4<em>分</em></span></div><div class="tags-wrap"><em><a href="/a">玄幻奇幻</a></em><em><a href="/b">东方玄幻</a></em></div><div class="sub-title">作者：<em><a href="/z">月下乘船来</a></em></div></div><p class="intro">一剑在手，风云由我。</p>"#;
    let qm_meta = parse_qimao_detail_html("1790589", qm_detail_html).unwrap();
    assert_eq!(qm_meta.title.as_deref(), Some("万剑圣帝"));
    assert_eq!(qm_meta.creators, vec!["月下乘船来"]);
    assert_eq!(qm_meta.rating, Some(8.4));
    assert_eq!(qm_meta.genres, vec!["玄幻奇幻"]);
    assert_eq!(qm_meta.tags, vec!["东方玄幻"]);

    // 4. 17K search (skipping author card) + detail
    let k17_vals: Vec<serde_json::Value> = serde_json::from_str(
        r#"[{"type":1,"lists":[]},{"id":493239,"book_name":"修罗武神","author_name":"善良的蜜蜂","intro":"论潜力","cover":"http://cdn.static.17k.com/493239.jpg","category_name_2":"东方玄幻"}]"#,
    )
    .unwrap();
    let k17_cands = map_k17_search_values(
        k17_vals,
        &ScrapeQuery {
            keyword: "修罗武神".into(),
            ..Default::default()
        },
    );
    assert_eq!(k17_cands.len(), 1);
    assert_eq!(k17_cands[0].external_id, "493239");
    let k17_detail_html = r#"<meta property="og:title" content="《修罗武神》善良的蜜蜂著-17K小说网"/><meta property="og:image" content="http://cdn.static.17k.com/493239.jpg"/><h1><a href="/list/493239.html">修罗武神</a></h1><em>更新:2026-11-17 23:32:35</em><p class="intro"><a>论潜力<br>以修罗成武神</a></p><th>作品类别：</th><td><a>东方玄幻</a></td>"#;
    let k17_meta = parse_k17_detail_html("493239", k17_detail_html).unwrap();
    assert_eq!(k17_meta.creators, vec!["善良的蜜蜂"]);
    assert_eq!(k17_meta.genres, vec!["东方玄幻"]);
    assert_eq!(k17_meta.year, Some(2026));

    // 5. Ciweimao captcha skip + detail OpenGraph
    let captcha_html = "<!DOCTYPE html><html><head><title>验证码 - 刺猬猫</title></head></html>";
    assert!(parse_ciweimao_search_html(captcha_html, &q).is_empty());
    let cwm_detail_html = r#"<meta property="og:novel:book_name" content="幻想乡玩家"/><meta property="og:novel:author" content="才不是H萝莉"/><meta property="og:novel:category" content="免费同人"/><meta property="og:image" content="https://e1.kuangxiangit.com/cwm.jpg"/><meta property="og:description" content="曾经身为少年的某人\r\n幻想乡真是太糟糕了。"/><p class="label-box"><span>东方Project</span><span>二次元</span></p>"#;
    let cwm_meta = parse_ciweimao_detail_html("100012892", cwm_detail_html).unwrap();
    assert_eq!(cwm_meta.title.as_deref(), Some("幻想乡玩家"));
    assert_eq!(cwm_meta.creators, vec!["才不是H萝莉"]);
    assert_eq!(cwm_meta.tags, vec!["东方Project", "二次元"]);

    // 6. SFACG search + detail
    let sf_search_html = r#"<ul style="width:100%"><li class="Conjunction"><img src="//rs.sfacg.com/web/novel/images/NovelCover/big/a.jpg"/></li><li><strong><a href="https://book.sfacg.com/Novel/48704">英雄再临</a></strong><br />综合信息： 无聊看看天/2024/3/25 20:57:58<br />人类第一个真正意义上的超级英雄。</li></ul>"#;
    let sf_cands = parse_sfacg_search_html(
        sf_search_html,
        &ScrapeQuery {
            keyword: "英雄再临".into(),
            ..Default::default()
        },
    );
    assert_eq!(sf_cands[0].external_id, "48704");
    assert_eq!(sf_cands[0].year, Some(2024));
    let sf_detail_html = r#"<meta name="keywords" content="英雄再临,无聊看看天"/><div class="author-name"><span>无聊看看天</span></div><span class="text">类型：科幻</span><span class="text">更新：2024/3/25 20:57:58</span><img src="http://rs.sfacg.com/web/novel/images/NovelCover/Big/a.jpg"/><p class="introduce">超级英雄故事。</p><li class="tag"><a><span class="text">战斗</span></a></li><li class="tag"><a><span class="text">主角强大(228)</span></a></li>"#;
    let sf_meta = parse_sfacg_detail_html("48704", sf_detail_html).unwrap();
    assert_eq!(sf_meta.title.as_deref(), Some("英雄再临"));
    assert_eq!(sf_meta.genres, vec!["科幻"]);
    assert_eq!(sf_meta.tags, vec!["战斗", "主角强大"]);

    // 7. Faloo search + detail
    let fl_search_html = r#"<div class="TwoBox02_02"><div class="TwoBox02_03"><img class="TYImg" src="http://img.faloo.com/Novel/a.jpg"></div><div class="TwoBox02_08"><h1><a href="//b.faloo.com/1384726.html">三国：悟性逆天</a></h1></div><div class="TwoBox02_09"><a>三国大秦11</a></div><a href="//b.faloo.com/l_3_73_1.html">三国梦想</a><div class="TwoBox02_06"><a>秦昭穿越汉末</a></div></div>"#;
    let fl_cands = parse_faloo_search_html(
        fl_search_html,
        &ScrapeQuery {
            keyword: "三国".into(),
            ..Default::default()
        },
    );
    assert_eq!(fl_cands[0].external_id, "1384726");
    let fl_detail_html = r#"<meta property="og:novel:book_name" content="三国：悟性逆天"/><meta property="og:novel:author" content="三国大秦11"/><meta property="og:novel:category" content="三国梦想"/><meta property="og:image" content="http://img.faloo.com/Novel/a.jpg"/><meta property="og:novel:update_time" content="2026-04-20 21:08"/><div class="T-L-T-C-Box1"><p>秦昭穿越汉末。</p><p>飞卢小说网提醒您：本小说及人物纯属虚构，如有雷同，纯属巧合，切勿模仿。</p></div>"#;
    let fl_meta = parse_faloo_detail_html("1384726", fl_detail_html).unwrap();
    assert_eq!(fl_meta.summary.as_deref(), Some("秦昭穿越汉末。"));
    assert_eq!(fl_meta.year, Some(2026));

    // 8. JJWXC wap search + androidapi detail
    let jj_search_html = r#"<ul><li><a href="/book2/3200611"><span style="color:red">天</span>官赐福</a> - <a href="/wapauthor/1322620">墨香铜臭</a></li></ul>"#;
    let jj_cands = parse_jjwxc_wap_search_html(
        jj_search_html,
        &ScrapeQuery {
            keyword: "天官赐福".into(),
            ..Default::default()
        },
    );
    assert_eq!(jj_cands[0].external_id, "3200611");
    assert_eq!(jj_cands[0].title, "天官赐福");
    let jj_meta = map_jjwxc_detail(
        "3200611",
        JjwxcNovelDetail {
            novel_id: "3200611".into(),
            novel_name: "天官赐福".into(),
            author_name: "墨香铜臭".into(),
            novel_class: "原创-纯爱-架空历史-仙侠".into(),
            novel_tags: "天作之合,灵异神怪".into(),
            original_cover: "https://i5-static.jjwxc.net/cover.png".into(),
            novel_intro: "为你，所向披靡！&lt;br/&gt;C天R地小妖精攻×仙风道骨收破烂受".into(),
            renew_date: "2024-02-10 12:00:00".into(),
            series: "天官".into(),
            novel_review_score: "9.8分".into(),
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(jj_meta.title.as_deref(), Some("天官赐福"));
    assert_eq!(jj_meta.series_or_album.as_deref(), Some("天官"));
    assert_eq!(jj_meta.rating, Some(9.8));
    assert!(jj_meta.summary.as_deref().unwrap().contains('\n'));
}

#[test]
fn chinese_music_parsers_normalize_all_four_platforms_and_bilingual_lrc() {
    use base64::Engine;
    use base64::engine::general_purpose::STANDARD as BASE64_STANDARD;

    use crate::infra::scrapers::kugou::{KgAlbumItem, KgPlaySongInfo, map_kugou_song_detail};
    use crate::infra::scrapers::kuwo::{
        KwAlbumSearchItem, KwAlbumTrackItem, KwLrcItem, KwSongAndLrcData, KwSongDetailInfo,
        map_kuwo_song_detail,
    };
    use crate::infra::scrapers::netease::{
        NeAlbumInfo, NeArtist, NeLyricBlock, NeLyricEnvelope, NeSearchSong,
        map_netease_album_detail, map_netease_song_detail,
    };
    use crate::infra::scrapers::qq_music::{
        QqAlbumBrief, QqInfoContentEntry, QqInfoField, QqLyricResp, QqSinger, QqSongDetailData,
        QqSongExtraInfo, QqSongItem, map_qq_song_detail,
    };

    // 1. QQ Music (track + bilingual Base64 LRC + HTML entity unescaping)
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

    // 2. NetEase Cloud Music (track + CST midnight publishTime + album)
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
                publish_time: Some(1446739200007), // 2015-11-06 00:00:00 +08:00
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
    assert_eq!(
        ne_song.cover_url.as_deref(),
        Some("https://p2.music.126.net/abc/123.jpg?param=1000y1000")
    );

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

    // 3. Kugou Music (track + `{size}` cover upgrade + LRC)
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

    // 4. Kuwo Music (`lrclist` float seconds -> `[mm:ss.xx]` + track order + 1000px cover)
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
        kw_meta.publisher_or_studio.as_deref(),
        Some("杰威尔音乐有限公司")
    );
    assert_eq!(
        kw_meta.cover_url.as_deref(),
        Some("https://img1.kwcdn.kuwo.cn/star/albumcover/1000/s3s94/93/211513640.jpg")
    );
    assert_eq!(
        kw_meta.synced_lyrics.as_deref(),
        Some("[00:28.90]故事的小黄花")
    );

    // 5. Migu, Taihe, Qishui, and Bangumi MusicTrack mappers
    use crate::infra::scrapers::migu::{
        MiguAlbumResource, MiguImgItem, MiguSongResource, map_migu_song_detail,
    };
    use crate::infra::scrapers::qishui::{
        QsAlbumInfo, QsAlbumTrackRef, QsArtist, QsLyricPayload, QsSeoTrackEnvelope, QsSeoTrackWrap,
        QsTrackCore, QsUrlCover, map_qishui_seo_track,
    };
    use crate::infra::scrapers::taihe::{
        TaiheArtist, TaiheTrackItem, build_taihe_signed_params, map_taihe_song_detail,
    };

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
    assert_eq!(migu_meta.publisher_or_studio.as_deref(), Some("杰威尔音乐"));

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
    assert_eq!(taihe_meta.year, Some(2011));

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
    assert_eq!(
        qs_meta.cover_url.as_deref(),
        Some("https://p3-luna.douyinpic.com/img/tos-cn-v-2774c002/demo~c5_1000x1000.jpg")
    );

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
        Some(crate::infra::scrapers::bangumi::BgmEpisodeItem {
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

    // 6. Douban, NeoDB, Xinhua, and AniList mappers
    use crate::infra::scrapers::anilist::{
        AlCover, AlDate, AlMedia, AlPersonName, AlStaffConnection, AlStaffEdge, AlStaffNode,
        AlTitle, map_anilist_detail,
    };
    use crate::infra::scrapers::douban::parse_douban_subject_html;
    use crate::infra::scrapers::neodb::{NeoDbBookItem, map_neodb_detail};
    use crate::infra::scrapers::xinhua::parse_xinhua_detail_html;

    let db_html = r#"
        <html>
        <head>
          <script type="application/ld+json">
          {"@type":"Book","name":"三体","author":[{"name":"刘慈欣"}],"isbn":"9787536692930"}
          </script>
        </head>
        <body>
          <a class="nbg" href="https://img3.doubanio.com/view/subject/s/public/s2768378.jpg"></a>
          <div id="info">
            <span class="pl">出版社:</span> 重庆出版社<br/>
            <span class="pl">出版年:</span> 2008-1<br/>
            <span class="pl">丛书:</span> 科幻世界·中国科幻基石丛书<br/>
          </div>
          <strong property="v:average">8.9</strong>
          <div id="link-report"><div class="intro"><p>文化大革命如火如荼进行的同时。</p></div></div>
          <script>var criteria = '7:科幻|7:刘慈欣';</script>
        </body>
        </html>
    "#;
    let db_meta = parse_douban_subject_html("2567698", db_html, ScrapeTargetKind::Book).unwrap();
    assert_eq!(db_meta.title.as_deref(), Some("三体"));
    assert_eq!(db_meta.isbn.as_deref(), Some("9787536692930"));
    assert_eq!(db_meta.year, Some(2008));
    assert_eq!(db_meta.published_date.as_deref(), Some("2008-01-01"));
    assert_eq!(
        db_meta.cover_url.as_deref(),
        Some("https://img3.doubanio.com/view/subject/l/public/s2768378.jpg")
    );
    assert!(db_meta.tags.contains(&"科幻".to_string()));

    let neo_meta = map_neodb_detail(
        NeoDbBookItem {
            uuid: "7OGqnJkxiQZImAxD9aAgMr".into(),
            title: "三体".into(),
            subtitle: Some(serde_json::json!("地球往事")),
            author: serde_json::json!(["刘慈欣"]),
            pub_house: Some(serde_json::json!("重庆出版社")),
            pub_year: Some(2008),
            pub_month: Some(1),
            isbn: Some(serde_json::json!("9787536692930")),
            rating: Some(8.6),
            ..Default::default()
        },
        ScrapeTargetKind::Book,
    );
    assert_eq!(neo_meta.title.as_deref(), Some("三体：地球往事"));
    assert_eq!(neo_meta.published_date.as_deref(), Some("2008-01-01"));

    let xh_html = r#"
        <div class="item-title" data-item="{&quot;name&quot;:&quot;三体&quot;,&quot;mainImage&quot;:&quot;//img1.xinhuashudian.com/book.jpg&quot;,&quot;extra&quot;:{&quot;author&quot;:&quot;刘慈欣&quot;}}"></div>
        <li class="attribute-li"><span class="attr-label">商品编码（ISBN）</span><span class="attr-val">9787536692930</span></li>
        <li class="attribute-li"><span class="attr-label">出版社</span><span class="attr-val">重庆出版社</span></li>
        <li class="attribute-li"><span class="attr-label">中图分类号</span><span class="attr-val">I247.5</span></li>
    "#;
    let xh_meta = parse_xinhua_detail_html("1001", xh_html).unwrap();
    assert_eq!(xh_meta.isbn.as_deref(), Some("9787536692930"));
    assert!(xh_meta.genres.contains(&"中国当代小说".to_string()));

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

#[tokio::test]
#[ignore = "live network smoke check across QQ, NetEase, Kugou, and Kuwo music providers"]
async fn live_smoke_check_four_chinese_music_providers() {
    use crate::domain::ports::ScraperProviderPort;
    use crate::domain::scraper::ScrapeQuery;
    use crate::infra::scrapers::{
        kugou::KugouMusicProvider, kuwo::KuwoMusicProvider, netease::NeteaseMusicProvider,
        qq_music::QqMusicProvider,
    };

    struct MusicSmokeCase {
        name: &'static str,
        prov: Box<dyn ScraperProviderPort>,
        song_kw: &'static str,
        artist_kw: &'static str,
        album_kw: &'static str,
    }

    let creds = ProviderCredentials::default();
    let cases = vec![
        MusicSmokeCase {
            name: "qq",
            prov: Box::new(QqMusicProvider),
            song_kw: "晴天",
            artist_kw: "周杰伦",
            album_kw: "叶惠美",
        },
        MusicSmokeCase {
            name: "netease",
            prov: Box::new(NeteaseMusicProvider),
            song_kw: "泡沫",
            artist_kw: "邓紫棋",
            album_kw: "新的心跳",
        },
        MusicSmokeCase {
            name: "kugou",
            prov: Box::new(KugouMusicProvider),
            song_kw: "晴天",
            artist_kw: "周杰伦",
            album_kw: "叶惠美",
        },
        MusicSmokeCase {
            name: "kuwo",
            prov: Box::new(KuwoMusicProvider),
            song_kw: "晴天",
            artist_kw: "周杰伦",
            album_kw: "叶惠美",
        },
    ];

    for MusicSmokeCase {
        name,
        prov,
        song_kw,
        artist_kw,
        album_kw,
    } in cases
    {
        let track_cands = prov
            .search(
                &ScrapeQuery {
                    keyword: song_kw.to_string(),
                    artist_or_author: Some(artist_kw.to_string()),
                    target: Some(ScrapeTargetKind::MusicTrack),
                    ..Default::default()
                },
                &creds,
            )
            .await
            .unwrap_or_else(|e| panic!("{name} track search failed: {e}"));
        assert!(
            !track_cands.is_empty(),
            "{name} returned empty track search for '{song_kw}'"
        );
        let top = &track_cands[0];
        let track_detail = prov
            .fetch_detail(
                &top.external_id,
                ScrapeTargetKind::MusicTrack,
                "zh-CN",
                &creds,
            )
            .await
            .unwrap_or_else(|e| panic!("{name} track detail failed: {e}"));
        println!(
            "[{name} TRACK] id={} title={:?} creators={:?} album={:?} track={:?} year={:?} cover={:?} synced_lrc_len={}",
            track_detail.external_id,
            track_detail.title,
            track_detail.creators,
            track_detail.series_or_album,
            track_detail.track_or_episode_number,
            track_detail.year,
            track_detail.cover_url,
            track_detail.synced_lyrics.as_deref().map_or(0, str::len)
        );
        assert!(track_detail.title.is_some(), "{name} track missing title");
        assert!(
            track_detail.cover_url.is_some(),
            "{name} track missing cover_url"
        );
        assert!(
            track_detail.synced_lyrics.is_some(),
            "{name} track missing synced_lyrics"
        );

        let album_cands = prov
            .search(
                &ScrapeQuery {
                    keyword: album_kw.to_string(),
                    artist_or_author: Some(artist_kw.to_string()),
                    target: Some(ScrapeTargetKind::MusicAlbum),
                    ..Default::default()
                },
                &creds,
            )
            .await
            .unwrap_or_else(|e| panic!("{name} album search failed: {e}"));
        assert!(
            !album_cands.is_empty(),
            "{name} returned empty album search for '{album_kw}'"
        );
        let top_alb = &album_cands[0];
        let alb_detail = prov
            .fetch_detail(
                &top_alb.external_id,
                ScrapeTargetKind::MusicAlbum,
                "zh-CN",
                &creds,
            )
            .await
            .unwrap_or_else(|e| panic!("{name} album detail failed: {e}"));
        println!(
            "[{name} ALBUM] id={} title={:?} creators={:?} year={:?} company={:?} cover={:?}",
            alb_detail.external_id,
            alb_detail.title,
            alb_detail.creators,
            alb_detail.year,
            alb_detail.publisher_or_studio,
            alb_detail.cover_url
        );
        assert!(alb_detail.title.is_some(), "{name} album missing title");
        assert!(
            alb_detail.cover_url.is_some(),
            "{name} album missing cover_url"
        );
    }

    // Additional live smoke checks: Migu, Taihe, Qishui (direct link/ID), and Bangumi MusicTrack
    use crate::infra::scrapers::{
        bangumi::BangumiProvider, migu::MiguMusicProvider, qishui::QishuiMusicProvider,
        taihe::TaiheMusicProvider,
    };

    for (mname, mprov, skw, akw, alb_kw) in [
        (
            "migu",
            Box::new(MiguMusicProvider) as Box<dyn ScraperProviderPort>,
            "晴天",
            "周杰伦",
            "叶惠美",
        ),
        (
            "taihe",
            Box::new(TaiheMusicProvider) as Box<dyn ScraperProviderPort>,
            "许嵩",
            "",
            "许嵩",
        ),
    ] {
        let tc = mprov
            .search(
                &ScrapeQuery {
                    keyword: skw.to_string(),
                    artist_or_author: (!akw.is_empty()).then(|| akw.to_string()),
                    target: Some(ScrapeTargetKind::MusicTrack),
                    ..Default::default()
                },
                &creds,
            )
            .await
            .unwrap();
        assert!(!tc.is_empty(), "{mname} track search empty");
        let td = mprov
            .fetch_detail(
                &tc[0].external_id,
                ScrapeTargetKind::MusicTrack,
                "zh-CN",
                &creds,
            )
            .await
            .unwrap();
        println!(
            "[{mname} TRACK] id={} title={:?} creators={:?} album={:?} year={:?} cover={:?} lrc_len={}",
            td.external_id,
            td.title,
            td.creators,
            td.series_or_album,
            td.year,
            td.cover_url,
            td.synced_lyrics.as_deref().map_or(0, str::len)
        );
        assert!(td.title.is_some(), "{mname} missing title");

        let ac = mprov
            .search(
                &ScrapeQuery {
                    keyword: alb_kw.to_string(),
                    artist_or_author: (!akw.is_empty()).then(|| akw.to_string()),
                    target: Some(ScrapeTargetKind::MusicAlbum),
                    ..Default::default()
                },
                &creds,
            )
            .await
            .unwrap();
        assert!(!ac.is_empty(), "{mname} album search empty");
        let ad = mprov
            .fetch_detail(
                &ac[0].external_id,
                ScrapeTargetKind::MusicAlbum,
                "zh-CN",
                &creds,
            )
            .await
            .unwrap();
        println!(
            "[{mname} ALBUM] id={} title={:?} year={:?} cover={:?}",
            ad.external_id, ad.title, ad.year, ad.cover_url
        );
        assert!(ad.title.is_some(), "{mname} album missing title");
    }

    // Qishui track + album via share URL / ID
    let qs_prov = QishuiMusicProvider;
    let qs_cands = qs_prov
        .search(
            &ScrapeQuery {
                keyword: "https://music.douyin.com/qishui/share/track?track_id=7651571345679583248"
                    .into(),
                target: Some(ScrapeTargetKind::MusicTrack),
                ..Default::default()
            },
            &creds,
        )
        .await
        .unwrap();
    assert_eq!(qs_cands.len(), 1);
    let qs_td = qs_prov
        .fetch_detail(
            "7651571345679583248",
            ScrapeTargetKind::MusicTrack,
            "zh-CN",
            &creds,
        )
        .await
        .unwrap();
    println!(
        "[qishui TRACK] title={:?} creators={:?} album={:?} lrc_len={}",
        qs_td.title,
        qs_td.creators,
        qs_td.series_or_album,
        qs_td.synced_lyrics.as_deref().map_or(0, str::len)
    );
    assert!(qs_td.synced_lyrics.is_some());

    let qs_ad = qs_prov
        .fetch_detail(
            "7651571345679566864",
            ScrapeTargetKind::MusicAlbum,
            "zh-CN",
            &creds,
        )
        .await
        .unwrap();
    println!(
        "[qishui ALBUM] title={:?} company={:?} year={:?}",
        qs_ad.title, qs_ad.publisher_or_studio, qs_ad.year
    );
    assert_eq!(qs_ad.title.as_deref(), Some("安泊猜想"));

    // Bangumi MusicTrack episode matching
    let bgm_prov = BangumiProvider;
    let bgm_tc = bgm_prov
        .search(
            &ScrapeQuery {
                keyword: "BLAZE MOMENT".into(),
                album_or_series: Some("紅蓮華".into()),
                target: Some(ScrapeTargetKind::MusicTrack),
                ..Default::default()
            },
            &creds,
        )
        .await
        .unwrap();
    assert!(!bgm_tc.is_empty());
    let bgm_td = bgm_prov
        .fetch_detail(
            &bgm_tc[0].external_id,
            ScrapeTargetKind::MusicTrack,
            "zh-CN",
            &creds,
        )
        .await
        .unwrap();
    println!(
        "[bangumi TRACK] id={} title={:?} album={:?} track={:?}",
        bgm_td.external_id, bgm_td.title, bgm_td.series_or_album, bgm_td.track_or_episode_number
    );
    assert!(bgm_td.track_or_episode_number.is_some());
}

#[tokio::test]
#[ignore = "live network smoke check across Douban, NeoDB, Xinhua, and AniList"]
async fn live_smoke_check_published_book_and_comic_providers() {
    use crate::domain::ports::ScraperProviderPort;
    use crate::domain::scraper::ScrapeQuery;
    use crate::infra::scrapers::{
        anilist::AniListProvider, douban::DoubanBookProvider, neodb::NeoDbProvider,
        xinhua::XinhuaBookProvider,
    };

    let creds = ProviderCredentials::default();

    // 1. NeoDB
    let neo = NeoDbProvider;
    let neo_cands = neo
        .search(
            &ScrapeQuery {
                keyword: "三体".into(),
                isbn: Some("9787536692930".into()),
                target: Some(ScrapeTargetKind::Book),
                ..Default::default()
            },
            &creds,
        )
        .await
        .unwrap();
    assert!(!neo_cands.is_empty(), "NeoDB returned empty search");
    let neo_det = neo
        .fetch_detail(
            &neo_cands[0].external_id,
            ScrapeTargetKind::Book,
            "zh-CN",
            &creds,
        )
        .await
        .unwrap();
    println!(
        "[neodb] title={:?} creators={:?} isbn={:?} rating={:?} cover={:?}",
        neo_det.title, neo_det.creators, neo_det.isbn, neo_det.rating, neo_det.cover_url
    );
    assert!(neo_det.title.is_some());

    // 2. Douban
    let db = DoubanBookProvider;
    let db_cands = db
        .search(
            &ScrapeQuery {
                keyword: "三体".into(),
                target: Some(ScrapeTargetKind::Book),
                ..Default::default()
            },
            &creds,
        )
        .await
        .unwrap();
    assert!(!db_cands.is_empty(), "Douban returned empty search");
    let db_det = db
        .fetch_detail(
            &db_cands[0].external_id,
            ScrapeTargetKind::Book,
            "zh-CN",
            &creds,
        )
        .await
        .unwrap();
    println!(
        "[douban] id={} title={:?} creators={:?} isbn={:?} rating={:?} cover={:?}",
        db_det.external_id,
        db_det.title,
        db_det.creators,
        db_det.isbn,
        db_det.rating,
        db_det.cover_url
    );
    assert!(db_det.title.is_some());

    // 3. Xinhua Bookstore
    let xh = XinhuaBookProvider;
    let xh_cands = xh
        .search(
            &ScrapeQuery {
                keyword: "三体".into(),
                isbn: Some("9787536692930".into()),
                target: Some(ScrapeTargetKind::Book),
                ..Default::default()
            },
            &creds,
        )
        .await
        .unwrap();
    assert!(!xh_cands.is_empty(), "Xinhua returned empty search");
    let xh_det = xh
        .fetch_detail(
            &xh_cands[0].external_id,
            ScrapeTargetKind::Book,
            "zh-CN",
            &creds,
        )
        .await
        .unwrap();
    println!(
        "[xinhua] id={} title={:?} publisher={:?} isbn={:?} genres={:?} cover={:?}",
        xh_det.external_id,
        xh_det.title,
        xh_det.publisher_or_studio,
        xh_det.isbn,
        xh_det.genres,
        xh_det.cover_url
    );
    assert!(xh_det.title.is_some());

    // 4. AniList
    let al = AniListProvider;
    let al_cands = al
        .search(
            &ScrapeQuery {
                keyword: "Frieren".into(),
                target: Some(ScrapeTargetKind::Comic),
                locale: Some("zh-CN".into()),
                ..Default::default()
            },
            &creds,
        )
        .await
        .unwrap();
    assert!(!al_cands.is_empty(), "AniList returned empty search");
    let al_det = al
        .fetch_detail(
            &al_cands[0].external_id,
            ScrapeTargetKind::Comic,
            "zh-CN",
            &creds,
        )
        .await
        .unwrap();
    println!(
        "[anilist] id={} title={:?} creators={:?} illustrators={:?} rating={:?} cover={:?}",
        al_det.external_id,
        al_det.title,
        al_det.creators,
        al_det.illustrators_or_directors,
        al_det.rating,
        al_det.cover_url
    );
    assert!(al_det.title.is_some());
}

#[tokio::test]
#[ignore = "live network smoke check across all 8 Chinese novel platforms"]
async fn live_smoke_check_eight_chinese_novel_providers() {
    use crate::domain::ports::ScraperProviderPort;
    use crate::domain::scraper::ScrapeQuery;
    use crate::infra::scrapers::{
        ciweimao::CiweimaoProvider, faloo::FalooProvider, fanqie::FanqieProvider,
        jjwxc::JjwxcProvider, k17::K17Provider, qidian::QidianProvider, qimao::QimaoProvider,
        sfacg::SfacgProvider,
    };

    let creds = ProviderCredentials::default();
    let cases: Vec<(&str, Box<dyn ScraperProviderPort>, &str)> = vec![
        ("qidian", Box::new(QidianProvider), "诡秘之主"),
        ("fanqie", Box::new(FanqieProvider), "十日终焉"),
        ("qimao", Box::new(QimaoProvider), "万剑圣帝"),
        ("17k", Box::new(K17Provider), "修罗武神"),
        ("ciweimao", Box::new(CiweimaoProvider), "100012892"),
        ("sfacg", Box::new(SfacgProvider), "英雄再临"),
        ("faloo", Box::new(FalooProvider), "三国"),
        ("jjwxc", Box::new(JjwxcProvider), "天官赐福"),
    ];

    for (name, prov, kw) in cases {
        let cands = match prov
            .search(
                &ScrapeQuery {
                    keyword: kw.to_string(),
                    target: Some(ScrapeTargetKind::Book),
                    ..Default::default()
                },
                &creds,
            )
            .await
        {
            Ok(c) => c,
            Err(e) => {
                println!("[{name}] SEARCH ERR: {e}");
                continue;
            }
        };
        assert!(!cands.is_empty(), "{name} returned empty search for '{kw}'");
        let first = &cands[0];
        println!(
            "[{name}] SEARCH #1 id={} title={:?} sub={:?}",
            first.external_id, first.title, first.subtitle
        );
        match prov
            .fetch_detail(&first.external_id, ScrapeTargetKind::Book, "zh-CN", &creds)
            .await
        {
            Ok(detail) => {
                println!(
                    "[{name}] DETAIL OK title={:?} creators={:?} genres={:?} tags={:?} cover={:?}",
                    detail.title, detail.creators, detail.genres, detail.tags, detail.cover_url
                );
            }
            Err(e) => {
                panic!("[{name}] DETAIL ERR for id={}: {e}", first.external_id);
            }
        }
    }
}

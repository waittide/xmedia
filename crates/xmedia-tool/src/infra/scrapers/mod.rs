pub mod bangumi;
pub mod ciweimao;
pub mod cn_novel_common;
pub mod faloo;
pub mod fanqie;
pub mod http_client;
pub mod itunes;
pub mod jjwxc;
pub mod k17;
pub mod lrclib;
pub mod musicbrainz;
pub mod openlibrary;
pub mod qidian;
pub mod qimao;
pub mod sfacg;
pub mod tmdb;

use std::sync::Arc;

use crate::domain::ports::ScraperProviderPort;

/// Builds the default registry of pluggable `ScraperProviderPort` implementations.
///
/// Extending `xmedia-tool` with a new scraper source requires only:
/// 1. Implementing `ScraperProviderPort` in `infra/scrapers/<new_source>.rs`;
/// 2. Appending `Arc::new(...)` here.
///
/// Both the HTTP API (`/api/v1/tool/scrapers/providers`) and the Web UI discover new sources automatically.
pub fn build_default_scraper_providers() -> Vec<Arc<dyn ScraperProviderPort>> {
    vec![
        Arc::new(qidian::QidianProvider),
        Arc::new(fanqie::FanqieProvider),
        Arc::new(qimao::QimaoProvider),
        Arc::new(k17::K17Provider),
        Arc::new(ciweimao::CiweimaoProvider),
        Arc::new(sfacg::SfacgProvider),
        Arc::new(faloo::FalooProvider),
        Arc::new(jjwxc::JjwxcProvider),
        Arc::new(itunes::ItunesMusicProvider),
        Arc::new(musicbrainz::MusicBrainzProvider),
        Arc::new(lrclib::LrclibProvider),
        Arc::new(openlibrary::OpenLibraryProvider),
        Arc::new(bangumi::BangumiProvider),
        Arc::new(tmdb::TmdbProvider),
    ]
}

#[cfg(test)]
mod tests {
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
    fn default_registry_exposes_fourteen_providers_and_checks_tmdb_credentials() {
        let providers = build_default_scraper_providers();
        assert_eq!(providers.len(), 14);

        let empty_creds = ProviderCredentials::default();
        let configured_ids: Vec<String> = providers
            .iter()
            .filter(|p| p.is_configured(&empty_creds))
            .map(|p| p.descriptor().id)
            .collect();
        assert_eq!(
            configured_ids,
            vec![
                "qidian",
                "fanqie",
                "qimao",
                "17k",
                "ciweimao",
                "sfacg",
                "faloo",
                "jjwxc",
                "itunes",
                "musicbrainz",
                "lrclib",
                "openlibrary",
                "bangumi"
            ]
        );

        let with_tmdb = ProviderCredentials {
            tmdb_api_key: Some("0123456789abcdef0123456789abcdef".into()),
            ..Default::default()
        };
        assert!(providers.iter().all(|p| p.is_configured(&with_tmdb)));
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
        let captcha_html =
            "<!DOCTYPE html><html><head><title>验证码 - 刺猬猫</title></head></html>";
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
}

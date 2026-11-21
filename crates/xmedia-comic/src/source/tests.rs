use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use std::time::Duration;

use axum::Router;
use axum::body::Body;
use axum::http::{HeaderMap, StatusCode};
use axum::response::Response;
use axum::routing::get;
use xmedia_source::http::HttpClientConfig;

use super::ComicSourceEngine;
use super::model::{
    ComicChapterJsContext, ComicJsContext, ComicSourceSpec, SourceRuntimeState,
    parse_reading_direction_opt,
};
use super::session::extract_img_srcs_from_html;
use crate::domain::online::ComicSourceRecord;
use crate::domain::series::ReadingDirection;

#[test]
fn parse_comic_source_spec_supports_comic_and_legado_aliases_and_object_headers() {
    let raw = r##"[
      {
        "sourceName": "Comic Native",
        "sourceUrl": "https://comic.example.com",
        "sourceGroup": "Manga",
        "sourceType": "comic",
        "readingDirection": "webtoon",
        "header": {"User-Agent": "ComicUA/1.0", "Host": "evil.example.com"},
        "imageHeader": {"Referer": "https://comic.example.com/", "Connection": "close"},
        "ruleDetail": {"name": "h1@text", "readingDirection": "@js:'ltr'"},
        "ruleChapters": {"chapterList": "li.ep", "name": "a@text", "url": "a@href"},
        "rulePages": {
          "pages": "#_imageList img@data-url",
          "imageHeader": "{\"X-Img-Token\":\"abc\"}"
        }
      },
      {
        "bookSourceName": "Legado Comic",
        "bookSourceUrl": "https://legado.example.com",
        "bookSourceGroup": "Legado",
        "bookSourceType": 2,
        "header": "{\"User-Agent\":\"LegadoUA\",\"Referer\":\"https://old.example/\"}",
        "imageHeader": "{\"referer\":\"https://new.example/\"}",
        "ruleBookInfo": {"name": ".title@text"},
        "ruleToc": {"chapterList": ".ch", "chapterName": "a@text", "chapterUrl": "a@href"},
        "ruleContent": {"content": ".pages img@src"}
      }
    ]"##;

    let batch = ComicSourceSpec::parse_json_batch(raw).unwrap();
    assert_eq!(batch.len(), 2);

    let (native, _) = &batch[0];
    assert_eq!(native.source_name, "Comic Native");
    assert_eq!(native.source_type, 2);
    assert_eq!(
        native.effective_reading_direction(),
        ReadingDirection::Webtoon
    );
    let native_headers = native.merged_image_headers();
    assert_eq!(
        native_headers.get("User-Agent").map(String::as_str),
        Some("ComicUA/1.0")
    );
    assert_eq!(
        native_headers.get("Referer").map(String::as_str),
        Some("https://comic.example.com/")
    );
    assert_eq!(
        native_headers.get("X-Img-Token").map(String::as_str),
        Some("abc")
    );
    assert!(!native_headers.contains_key("Host"));
    assert!(!native_headers.contains_key("Connection"));

    let (legado, _) = &batch[1];
    assert_eq!(legado.source_name, "Legado Comic");
    assert_eq!(legado.source_type, 2);
    assert_eq!(legado.effective_reading_direction(), ReadingDirection::Rtl);
    let legado_headers = legado.merged_image_headers();
    assert_eq!(
        legado_headers.get("referer").map(String::as_str),
        Some("https://new.example/")
    );
    assert!(!legado_headers.contains_key("Referer"));
}

#[test]
fn seeds_comic_json_parses_cleanly_with_all_five_verified_sources() {
    let raw = include_str!("../../../../seeds/sources/comic.json");
    let batch =
        ComicSourceSpec::parse_json_batch(raw).expect("seeds/sources/comic.json must parse");
    assert_eq!(batch.len(), 5);

    let names: Vec<&str> = batch.iter().map(|(s, _)| s.source_name.as_str()).collect();
    assert_eq!(
        names,
        vec![
            "MangaDex (EN)",
            "MangaDex (中文)",
            "WEBTOON (EN)",
            "WEBTOON (繁體中文)",
            "XKCD"
        ]
    );

    // MangaDex EN
    assert_eq!(
        batch[0].0.effective_reading_direction(),
        ReadingDirection::Rtl
    );
    assert_eq!(
        batch[0]
            .0
            .merged_image_headers()
            .get("User-Agent")
            .map(String::as_str),
        Some("xmedia/0.1.0")
    );

    // WEBTOON EN & ZH-Hant must enforce Referer in imageHeader
    for idx in [2usize, 3usize] {
        assert_eq!(
            batch[idx].0.effective_reading_direction(),
            ReadingDirection::Webtoon
        );
        let h = batch[idx].0.merged_image_headers();
        assert_eq!(
            h.get("Referer").map(String::as_str),
            Some("https://www.webtoons.com/")
        );
        assert!(
            h.get("User-Agent")
                .is_some_and(|ua| ua.contains("Windows NT 10.0"))
        );
    }

    // XKCD
    assert_eq!(
        batch[4].0.effective_reading_direction(),
        ReadingDirection::Ltr
    );
}

#[test]
fn sync_raw_json_fields_preserves_legado_vs_comic_group_key() {
    let legado_raw =
        r#"{"bookSourceName":"A","bookSourceUrl":"https://a","bookSourceGroup":"Old"}"#;
    let updated_legado =
        ComicSourceRecord::sync_raw_json_fields(legado_raw, Some(false), Some(50), Some("New"));
    let v1: serde_json::Value = serde_json::from_str(&updated_legado).unwrap();
    assert_eq!(v1["bookSourceGroup"], "New");
    assert!(v1.get("sourceGroup").is_none());
    assert_eq!(v1["enabled"], false);
    assert_eq!(v1["weight"], 50);

    let comic_raw = r#"{"sourceName":"B","sourceUrl":"https://b","sourceGroup":"Old"}"#;
    let updated_comic =
        ComicSourceRecord::sync_raw_json_fields(comic_raw, None, None, Some("Manga"));
    let v2: serde_json::Value = serde_json::from_str(&updated_comic).unwrap();
    assert_eq!(v2["sourceGroup"], "Manga");
    assert!(v2.get("bookSourceGroup").is_none());
}

#[test]
fn parse_reading_direction_and_html_img_extraction() {
    assert_eq!(
        parse_reading_direction_opt("RTL"),
        Some(ReadingDirection::Rtl)
    );
    assert_eq!(
        parse_reading_direction_opt("ltr"),
        Some(ReadingDirection::Ltr)
    );
    assert_eq!(
        parse_reading_direction_opt("webtoon"),
        Some(ReadingDirection::Webtoon)
    );
    assert_eq!(parse_reading_direction_opt("unknown"), None);

    let mut urls = Vec::new();
    extract_img_srcs_from_html(
        r#"<div><img data-url="/img/1.jpg"><img data-original="/img/2.png"><img src="https://cdn.test/3.webp"></div>"#,
        &mut urls,
    );
    assert_eq!(
        urls,
        vec!["/img/1.jpg", "/img/2.png", "https://cdn.test/3.webp"]
    );
}

#[test]
fn parse_explore_categories_supports_json_lines_and_js() {
    let engine = ComicSourceEngine::default();

    let json_src: ComicSourceSpec = serde_json::from_str(
        r#"{
            "sourceName": "JsonCat",
            "sourceUrl": "https://cat.test",
            "exploreUrl": "[{\"title\":\"Section\",\"url\":\"\"},{\"title\":\"Hot\",\"url\":\"/hot?p={{page}}\"},{\"title\":\"New\",\"url\":\"https://cat.test/new\"}]"
        }"#,
    )
    .unwrap();
    let cats1 = engine
        .parse_explore_categories(&json_src, SourceRuntimeState::default())
        .unwrap()
        .data;
    assert_eq!(cats1.len(), 2);
    assert_eq!(cats1[0].title, "Hot");
    assert_eq!(cats1[0].url, "https://cat.test/hot?p={{page}}");
    assert_eq!(cats1[1].title, "New");

    let js_src: ComicSourceSpec = serde_json::from_str(
        r#"{
            "sourceName": "JsCat",
            "sourceUrl": "https://cat.test",
            "exploreUrl": "@js:'Action::/genre/action?page={{page}}\\nDrama::/genre/drama?page={{page}}'"
        }"#,
    )
    .unwrap();
    let cats2 = engine
        .parse_explore_categories(&js_src, SourceRuntimeState::default())
        .unwrap()
        .data;
    assert_eq!(cats2.len(), 2);
    assert_eq!(cats2[0].title, "Action");
    assert_eq!(cats2[0].url, "https://cat.test/genre/action?page={{page}}");
    assert_eq!(cats2[1].title, "Drama");
}

#[tokio::test]
async fn engine_handles_paginated_chapters_paginated_pages_and_explore_fallback() {
    let app = Router::new()
        .route(
            "/explore",
            get(|headers: HeaderMap| async move {
                let _ = headers;
                Response::builder()
                    .status(StatusCode::OK)
                    .header("content-type", "text/html; charset=utf-8")
                    .body(Body::from(
                        r#"<ul><li><a href="/comic/1"><span class="t">Explore Title</span></a><img src="/cov.jpg"></li></ul>"#,
                    ))
                    .unwrap()
            }),
        )
        .route(
            "/toc1",
            get(|| async {
                Response::builder()
                    .status(StatusCode::OK)
                    .header("content-type", "text/html; charset=utf-8")
                    .body(Body::from(
                        r#"<ul id="ch">
                            <li class="vol"><span>Volume 1</span></li>
                            <li><a href="/ch/1">Ch.1</a></li>
                        </ul>
                        <a class="next" href="/toc2">Next</a>"#,
                    ))
                    .unwrap()
            }),
        )
        .route(
            "/toc2",
            get(|| async {
                Response::builder()
                    .status(StatusCode::OK)
                    .header("content-type", "text/html; charset=utf-8")
                    .body(Body::from(
                        r#"<ul id="ch"><li><a href="/ch/2">Ch.2</a></li></ul>"#,
                    ))
                    .unwrap()
            }),
        )
        .route(
            "/ch/1",
            get(|| async {
                Response::builder()
                    .status(StatusCode::OK)
                    .header("content-type", "text/html; charset=utf-8")
                    .body(Body::from(
                        r#"<div id="imgs">
                            <img data-url="/p1.jpg">
                            <img src="/p2.jpg">
                            <img src="data:image/svg+xml;utf8,<svg></svg>">
                        </div>
                        <a class="next-page" href="/ch/1?p=2">Next Page</a>"#,
                    ))
                    .unwrap()
            }),
        )
        .route(
            "/ch/1-p2",
            get(|| async {
                Response::builder()
                    .status(StatusCode::OK)
                    .header("content-type", "text/html; charset=utf-8")
                    .body(Body::from(
                        r#"<div id="imgs"><img data-original="/p3.jpg"></div>"#,
                    ))
                    .unwrap()
            }),
        );

    // Note: axum route `/ch/1` matches `/ch/1?p=2` too, so differentiate via Query if needed;
    // let's make `/ch/1` inspect query or use separate route `/ch/1_p2`!
    let app = app.route(
        "/ch/1_p2",
        get(|| async {
            Response::builder()
                .status(StatusCode::OK)
                .header("content-type", "text/html; charset=utf-8")
                .body(Body::from(
                    r#"<div id="imgs"><img data-original="/p3.jpg"></div>"#,
                ))
                .unwrap()
        }),
    );

    let app = app.route(
        "/ch/1_start",
        get(|| async {
            Response::builder()
                .status(StatusCode::OK)
                .header("content-type", "text/html; charset=utf-8")
                .body(Body::from(
                    r#"<div id="imgs">
                        <img data-url="/p1.jpg">
                        <img src="/p2.jpg">
                        <img src="data:image/svg+xml;utf8,<svg></svg>">
                    </div>
                    <a class="next-page" href="/ch/1_p2">Next Page</a>"#,
                ))
                .unwrap()
        }),
    );

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let server = tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });
    let base = format!("http://{addr}");

    let source: ComicSourceSpec = serde_json::from_value(serde_json::json!({
        "sourceName": "MultiPageTest",
        "sourceUrl": base,
        "exploreUrl": format!("All::{base}/explore?p={{{{page}}}}"),
        "ruleSearch": {
            "bookList": "ul li",
            "name": ".t@text",
            "bookUrl": "a@href",
            "coverUrl": "img@src"
        },
        "ruleExplore": {
            "bookList": "ul li"
        },
        "ruleToc": {
            "chapterList": "#ch li",
            "chapterName": "a@text || span@text",
            "chapterUrl": "a@href",
            "nextTocUrl": "a.next@href"
        },
        "ruleContent": {
            "content": "#imgs@html",
            "nextContentUrl": "a.next-page@href"
        }
    }))
    .unwrap();

    let engine = ComicSourceEngine::new(
        HttpClientConfig {
            allow_private_ips: true,
            ..Default::default()
        },
        Duration::from_secs(10),
    );

    let base_c = base.clone();
    let src_c = source.clone();
    let (explore_res, chapters_res, pages_res) = tokio::task::spawn_blocking(move || {
        let explore_res = engine
            .explore_sync_with_control(
                &src_c,
                SourceRuntimeState::default(),
                &format!("{base_c}/explore?p={{{{page}}}}"),
                2,
                Duration::from_secs(10),
                Arc::new(AtomicBool::new(false)),
            )
            .unwrap();
        let chapters_res = engine
            .fetch_chapters_sync(
                &src_c,
                SourceRuntimeState::default(),
                &format!("{base_c}/toc1"),
                &ComicJsContext::default(),
            )
            .unwrap();
        let pages_res = engine
            .fetch_pages_sync(
                &src_c,
                SourceRuntimeState::default(),
                &format!("{base_c}/ch/1_start"),
                &ComicJsContext::default(),
                &ComicChapterJsContext::default(),
            )
            .unwrap();
        (explore_res, chapters_res, pages_res)
    })
    .await
    .unwrap();

    // Explore fell back to ruleSearch fields cleanly
    assert_eq!(explore_res.data.len(), 1);
    assert_eq!(explore_res.data[0].title, "Explore Title");
    assert_eq!(explore_res.data[0].comic_url, format!("{base}/comic/1"));
    assert_eq!(
        explore_res.data[0].cover_url.as_deref(),
        Some(format!("{base}/cov.jpg").as_str())
    );

    // Chapters paginated across /toc1 -> /toc2 and flagged volume header
    assert_eq!(chapters_res.data.len(), 3);
    assert_eq!(chapters_res.data[0].title, "Volume 1");
    assert!(chapters_res.data[0].is_volume);
    assert_eq!(chapters_res.data[1].title, "Ch.1");
    assert!(!chapters_res.data[1].is_volume);
    assert_eq!(chapters_res.data[2].title, "Ch.2");

    // Pages extracted from HTML <img> tags across /ch/1_start -> /ch/1_p2 without stripping HTML,
    // filtering out data:image/svg+xml and resolving relative URLs
    assert_eq!(
        pages_res.data,
        vec![
            format!("{base}/p1.jpg"),
            format!("{base}/p2.jpg"),
            format!("{base}/p3.jpg"),
        ]
    );

    let _ = HashMap::<String, String>::new();
    server.abort();
}

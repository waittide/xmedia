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
            "exploreUrl": "[{\"title\":\"Section\",\"url\":\"\"},{\"title\":\"Hot\",\"url\":\"/hot?p={{page}}\"},{\"title\":\"Static\",\"url\":\"/static\"},{\"title\":\"New\",\"url\":\"https://cat.test/new\"}]"
        }"#,
    )
    .unwrap();
    let cats1 = engine
        .parse_explore_categories(&json_src, SourceRuntimeState::default())
        .unwrap()
        .data;
    assert_eq!(cats1.len(), 3);
    assert_eq!(cats1[0].title, "Hot");
    assert_eq!(cats1[0].url, "/hot?p={{page}}");
    assert_eq!(cats1[1].title, "Static");
    assert_eq!(cats1[1].url, "https://cat.test/static");
    assert_eq!(cats1[2].title, "New");

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
    assert_eq!(cats2[0].url, "/genre/action?page={{page}}");
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

#[test]
fn sync_raw_json_fields_preserves_book_source_group_after_clear_and_restore() {
    let legado_raw = r#"{"bookSourceName":"BugRepro","bookSourceUrl":"https://ex.test","bookSourceGroup":"Original"}"#;
    let cleared = ComicSourceRecord::sync_raw_json_fields(legado_raw, None, None, Some(""));
    let v_cleared: serde_json::Value = serde_json::from_str(&cleared).unwrap();
    assert!(v_cleared.get("bookSourceGroup").is_none());
    assert!(v_cleared.get("sourceGroup").is_none());

    let restored = ComicSourceRecord::sync_raw_json_fields(&cleared, None, None, Some("Restored"));
    let v_restored: serde_json::Value = serde_json::from_str(&restored).unwrap();
    assert_eq!(v_restored["bookSourceGroup"], "Restored");
    assert!(v_restored.get("sourceGroup").is_none());
}

#[tokio::test]
async fn extract_page_urls_handles_html_wrappers_multiline_img_json_rule_and_svg_fallback() {
    let app = Router::new()
        .route(
            "/ch/wrapped",
            get(|| async {
                Response::builder()
                    .status(StatusCode::OK)
                    .header("content-type", "text/html; charset=utf-8")
                    .body(Body::from(
                        r#"<div id="imgs">
                            <div class="page-wrap"><p>Page 1</p><img class="lazy" data-url="/wrapped1.jpg" src="/placeholder.jpg"></div>
                            <div class="page-wrap"><img src="/wrapped2.jpg"></div>
                        </div>"#,
                    ))
                    .unwrap()
            }),
        )
        .route(
            "/ch/multiline",
            get(|| async {
                Response::builder()
                    .status(StatusCode::OK)
                    .header("content-type", "text/html; charset=utf-8")
                    .body(Body::from(
                        "<div class=\"p\">\n<img\n data-url=\"/ml1.jpg\"\n src=\"/ml2.jpg\">\n</div>",
                    ))
                    .unwrap()
            }),
        )
        .route(
            "/ch/svg-fallback",
            get(|| async {
                Response::builder()
                    .status(StatusCode::OK)
                    .header("content-type", "text/html; charset=utf-8")
                    .body(Body::from(
                        r#"<div id="imgs">
                            <img data-url="data:image/svg+xml;utf8,<svg/>" src="/real_fallback.jpg">
                            <img data-url="javascript:void(0)" data-src="/real_fallback2.jpg">
                        </div>"#,
                    ))
                    .unwrap()
            }),
        )
        .route(
            "/ch/json-strings",
            get(|| async {
                Response::builder()
                    .status(StatusCode::OK)
                    .header("content-type", "application/json")
                    .body(Body::from(r#"{"images":["/j1.jpg","/j2.jpg"]}"#))
                    .unwrap()
            }),
        );

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let server = tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });
    let base = format!("http://{addr}");

    let engine = ComicSourceEngine::new(
        HttpClientConfig {
            allow_private_ips: true,
            ..Default::default()
        },
        Duration::from_secs(10),
    );

    let html_src: ComicSourceSpec = serde_json::from_value(serde_json::json!({
        "sourceName": "HtmlFrag",
        "sourceUrl": base,
        "ruleContent": { "content": "#imgs@html" }
    }))
    .unwrap();
    let multiline_src: ComicSourceSpec = serde_json::from_value(serde_json::json!({
        "sourceName": "MultilineFrag",
        "sourceUrl": base,
        "ruleContent": { "content": "@js:result" }
    }))
    .unwrap();
    let json_rule_src: ComicSourceSpec = serde_json::from_value(serde_json::json!({
        "sourceName": "JsonStrRule",
        "sourceUrl": base,
        "ruleContent": {
            "content": "@js:result.images",
            "imageUrl": "@js:result + '?auth=1'"
        }
    }))
    .unwrap();

    let base_c = base.clone();
    let (wrapped_pages, ml_pages, svg_pages, json_pages) = tokio::task::spawn_blocking(move || {
        let p1 = engine
            .fetch_pages_sync(
                &html_src,
                SourceRuntimeState::default(),
                &format!("{base_c}/ch/wrapped"),
                &ComicJsContext::default(),
                &ComicChapterJsContext::default(),
            )
            .unwrap()
            .data;
        let p2 = engine
            .fetch_pages_sync(
                &multiline_src,
                SourceRuntimeState::default(),
                &format!("{base_c}/ch/multiline"),
                &ComicJsContext::default(),
                &ComicChapterJsContext::default(),
            )
            .unwrap()
            .data;
        let p3 = engine
            .fetch_pages_sync(
                &html_src,
                SourceRuntimeState::default(),
                &format!("{base_c}/ch/svg-fallback"),
                &ComicJsContext::default(),
                &ComicChapterJsContext::default(),
            )
            .unwrap()
            .data;
        let p4 = engine
            .fetch_pages_sync(
                &json_rule_src,
                SourceRuntimeState::default(),
                &format!("{base_c}/ch/json-strings"),
                &ComicJsContext::default(),
                &ComicChapterJsContext::default(),
            )
            .unwrap()
            .data;
        (p1, p2, p3, p4)
    })
    .await
    .unwrap();

    assert_eq!(
        wrapped_pages,
        vec![
            format!("{base}/wrapped1.jpg"),
            format!("{base}/wrapped2.jpg")
        ]
    );
    assert_eq!(ml_pages, vec![format!("{base}/ml1.jpg")]);
    assert_eq!(
        svg_pages,
        vec![
            format!("{base}/real_fallback.jpg"),
            format!("{base}/real_fallback2.jpg")
        ]
    );
    assert_eq!(
        json_pages,
        vec![
            format!("{base}/j1.jpg?auth=1"),
            format!("{base}/j2.jpg?auth=1")
        ]
    );

    server.abort();
}

#[tokio::test]
async fn explore_relative_templates_js_expr_and_post_options_work_end_to_end() {
    use axum::extract::Query;
    use axum::routing::post;

    #[derive(serde::Deserialize)]
    struct ListQ {
        p: Option<u32>,
        tag: Option<String>,
    }

    let app = Router::new()
        .route(
            "/page/3",
            get(|| async {
                Response::builder()
                    .status(StatusCode::OK)
                    .header("content-type", "application/json")
                    .body(Body::from(r#"{"items":[{"t":"Page3","u":"/c/p3"}]}"#))
                    .unwrap()
            }),
        )
        .route(
            "/list",
            get(|Query(q): Query<ListQ>| async move {
                if q.p == Some(40) && q.tag.as_deref() == Some("js") {
                    Response::builder()
                        .status(StatusCode::OK)
                        .header("content-type", "application/json")
                        .body(Body::from(
                            r#"{"items":[{"t":"JsOffset40","u":"/c/js40"}]}"#,
                        ))
                        .unwrap()
                } else {
                    Response::builder()
                        .status(StatusCode::BAD_REQUEST)
                        .body(Body::from("{}"))
                        .unwrap()
                }
            }),
        )
        .route(
            "/post_list",
            post(|body: String| async move {
                if body.trim() == "page=3" {
                    Response::builder()
                        .status(StatusCode::OK)
                        .header("content-type", "application/json")
                        .body(Body::from(
                            r#"{"items":[{"t":"PostPage3","u":"/c/post3"}]}"#,
                        ))
                        .unwrap()
                } else {
                    Response::builder()
                        .status(StatusCode::BAD_REQUEST)
                        .body(Body::from("{}"))
                        .unwrap()
                }
            }),
        );

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let server = tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });
    let base = format!("http://{addr}");

    let source: ComicSourceSpec = serde_json::from_value(serde_json::json!({
        "sourceName": "RelExplore",
        "sourceUrl": base,
        "exploreUrl": "PathPage::/page/{{page}}\nJsExpr::/list?p={{(page - 1) * 20}}&tag=js\nPostOpt::/post_list,{\"method\":\"POST\",\"body\":\"page={{page}}\"}\nStatic::/static",
        "ruleSearch": {
            "bookList": "$.items[*]",
            "name": "$.t",
            "bookUrl": "$.u"
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

    let src_c = source.clone();
    let base_c = base.clone();
    let (cats, r0, r1, r2) = tokio::task::spawn_blocking(move || {
        let cats = engine
            .parse_explore_categories(&src_c, SourceRuntimeState::default())
            .unwrap()
            .data;
        assert_eq!(cats.len(), 4);
        assert_eq!(cats[0].url, "/page/{{page}}");
        assert_eq!(cats[1].url, "/list?p={{(page - 1) * 20}}&tag=js");
        assert_eq!(
            cats[2].url,
            "/post_list,{\"method\":\"POST\",\"body\":\"page={{page}}\"}"
        );
        assert_eq!(cats[3].url, format!("{base_c}/static"));

        let a = engine
            .explore_sync_with_control(
                &src_c,
                SourceRuntimeState::default(),
                &cats[0].url,
                3,
                Duration::from_secs(10),
                Arc::new(AtomicBool::new(false)),
            )
            .unwrap()
            .data;
        let b = engine
            .explore_sync_with_control(
                &src_c,
                SourceRuntimeState::default(),
                &cats[1].url,
                3,
                Duration::from_secs(10),
                Arc::new(AtomicBool::new(false)),
            )
            .unwrap()
            .data;
        let c = engine
            .explore_sync_with_control(
                &src_c,
                SourceRuntimeState::default(),
                &cats[2].url,
                3,
                Duration::from_secs(10),
                Arc::new(AtomicBool::new(false)),
            )
            .unwrap()
            .data;
        (cats, a, b, c)
    })
    .await
    .unwrap();
    let _ = cats;

    assert_eq!(r0[0].title, "Page3");
    assert_eq!(r1[0].title, "JsOffset40");
    assert_eq!(r2[0].title, "PostPage3");

    server.abort();
}

#[tokio::test]
async fn chapters_and_pages_ignore_hash_javascript_and_redirect_cycles() {
    use axum::extract::Query;
    use std::sync::atomic::{AtomicUsize, Ordering};

    #[derive(serde::Deserialize)]
    struct PQuery {
        p: Option<u32>,
    }

    let toc_p1_hits = Arc::new(AtomicUsize::new(0));
    let ch_p1_hits = Arc::new(AtomicUsize::new(0));
    let toc_p1_hits_c = toc_p1_hits.clone();
    let ch_p1_hits_c = ch_p1_hits.clone();

    let app = Router::new()
        .route(
            "/toc/entry",
            get(|| async {
                Response::builder()
                    .status(StatusCode::FOUND)
                    .header("location", "/toc/real?p=1")
                    .body(Body::empty())
                    .unwrap()
            }),
        )
        .route(
            "/toc/real",
            get(move |Query(q): Query<PQuery>| {
                let hits = toc_p1_hits_c.clone();
                async move {
                    if q.p.unwrap_or(1) == 1 {
                        hits.fetch_add(1, Ordering::SeqCst);
                        Response::builder()
                            .status(StatusCode::OK)
                            .header("content-type", "text/html; charset=utf-8")
                            .body(Body::from(
                                r##"<ul>
                                    <li><a href="#">Hash Vol</a></li>
                                    <li><a href="#sec">Frag Vol</a></li>
                                    <li><a href="javascript:void(0)">JS Vol</a></li>
                                    <li><a href="/ch/entry">Ch 1</a></li>
                                </ul>
                                <a class="next" href="/toc/real?p=2">Next</a>"##,
                            ))
                            .unwrap()
                    } else {
                        Response::builder()
                            .status(StatusCode::OK)
                            .header("content-type", "text/html; charset=utf-8")
                            .body(Body::from(
                                r##"<ul><li><a href="/ch/2">Ch 2</a></li></ul>
                                <a class="next" href="/toc/real?p=1">Back Cycle</a>"##,
                            ))
                            .unwrap()
                    }
                }
            }),
        )
        .route(
            "/toc/hash-next",
            get(|| async {
                Response::builder()
                    .status(StatusCode::OK)
                    .header("content-type", "text/html; charset=utf-8")
                    .body(Body::from(
                        r##"<ul><li><a href="/ch/only">Only</a></li></ul>
                        <a class="next" href="#">No Next</a>"##,
                    ))
                    .unwrap()
            }),
        )
        .route(
            "/ch/entry",
            get(|| async {
                Response::builder()
                    .status(StatusCode::FOUND)
                    .header("location", "/ch/real?p=1")
                    .body(Body::empty())
                    .unwrap()
            }),
        )
        .route(
            "/ch/real",
            get(move |Query(q): Query<PQuery>| {
                let hits = ch_p1_hits_c.clone();
                async move {
                    if q.p.unwrap_or(1) == 1 {
                        hits.fetch_add(1, Ordering::SeqCst);
                        Response::builder()
                            .status(StatusCode::OK)
                            .header("content-type", "text/html; charset=utf-8")
                            .body(Body::from(
                                r##"<div id="imgs"><img src="/img1.jpg"></div>
                                <a class="next-page" href="/ch/real?p=2">Next</a>"##,
                            ))
                            .unwrap()
                    } else {
                        Response::builder()
                            .status(StatusCode::OK)
                            .header("content-type", "text/html; charset=utf-8")
                            .body(Body::from(
                                r##"<div id="imgs"><img src="/img2.jpg"></div>
                                <a class="next-page" href="/ch/real?p=1">Loop</a>"##,
                            ))
                            .unwrap()
                    }
                }
            }),
        );

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let server = tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });
    let base = format!("http://{addr}");

    let source: ComicSourceSpec = serde_json::from_value(serde_json::json!({
        "sourceName": "CycleAndHashTest",
        "sourceUrl": base,
        "ruleToc": {
            "chapterList": "ul li",
            "chapterName": "a@text",
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
    let (chapters, hash_next_chapters, pages) = tokio::task::spawn_blocking(move || {
        let ch = engine
            .fetch_chapters_sync(
                &src_c,
                SourceRuntimeState::default(),
                &format!("{base_c}/toc/entry"),
                &ComicJsContext::default(),
            )
            .unwrap()
            .data;
        let hn = engine
            .fetch_chapters_sync(
                &src_c,
                SourceRuntimeState::default(),
                &format!("{base_c}/toc/hash-next"),
                &ComicJsContext::default(),
            )
            .unwrap()
            .data;
        let pg = engine
            .fetch_pages_sync(
                &src_c,
                SourceRuntimeState::default(),
                &format!("{base_c}/ch/entry"),
                &ComicJsContext::default(),
                &ComicChapterJsContext::default(),
            )
            .unwrap()
            .data;
        (ch, hn, pg)
    })
    .await
    .unwrap();

    // 5 entries: 3 volume placeholders + Ch 1 + Ch 2 (Ch 1 NOT duplicated on redirect cycle!)
    assert_eq!(chapters.len(), 5);
    assert!(chapters[0].is_volume && chapters[0].url.is_empty());
    assert!(chapters[1].is_volume && chapters[1].url.is_empty());
    assert!(chapters[2].is_volume && chapters[2].url.is_empty());
    assert!(!chapters[3].is_volume && chapters[3].title == "Ch 1");
    assert!(!chapters[4].is_volume && chapters[4].title == "Ch 2");
    assert_eq!(toc_p1_hits.load(Ordering::SeqCst), 1);

    assert_eq!(hash_next_chapters.len(), 1);
    assert_eq!(
        pages,
        vec![format!("{base}/img1.jpg"), format!("{base}/img2.jpg")]
    );
    assert_eq!(ch_p1_hits.load(Ordering::SeqCst), 1);

    server.abort();
}

#[tokio::test]
async fn extract_page_urls_handles_mixed_plain_and_img_and_html_wrapper_with_image_url_rule() {
    let app = Router::new()
        .route(
            "/ch/mixed",
            get(|| async {
                Response::builder()
                    .status(StatusCode::OK)
                    .header("content-type", "text/plain; charset=utf-8")
                    .body(Body::from(
                        "https://cdn.example/plain1.jpg\n<img src=\"/img2.jpg\">\n/plain3.jpg",
                    ))
                    .unwrap()
            }),
        )
        .route(
            "/ch/html-wrapper-rule",
            get(|| async {
                Response::builder()
                    .status(StatusCode::OK)
                    .header("content-type", "text/html; charset=utf-8")
                    .body(Body::from(
                        "<div id=\"imgs\">\n<div class=\"wrap\">\n<img\n data-url=\"/a.jpg\">\n</div>\n</div>",
                    ))
                    .unwrap()
            }),
        )
        .route(
            "/ch/mixed-multiline-rule",
            get(|| async {
                Response::builder()
                    .status(StatusCode::OK)
                    .header("content-type", "text/html; charset=utf-8")
                    .body(Body::from(
                        "<div class=\"wrap\">\n<img\n data-url=\"/a.jpg\">\n</div>\n<!-- multi\nline -->\n/plain_after.jpg",
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

    let engine = ComicSourceEngine::new(
        HttpClientConfig {
            allow_private_ips: true,
            ..Default::default()
        },
        Duration::from_secs(10),
    );

    let mixed_plain_src: ComicSourceSpec = serde_json::from_value(serde_json::json!({
        "sourceName": "MixedPlain",
        "sourceUrl": base,
        "ruleContent": { "content": "@js:result" }
    }))
    .unwrap();
    let mixed_rule_src: ComicSourceSpec = serde_json::from_value(serde_json::json!({
        "sourceName": "MixedRule",
        "sourceUrl": base,
        "ruleContent": {
            "content": "@js:result",
            "imageUrl": "@js:result + '?tok=1'"
        }
    }))
    .unwrap();
    let wrapper_rule_src: ComicSourceSpec = serde_json::from_value(serde_json::json!({
        "sourceName": "WrapperRule",
        "sourceUrl": base,
        "ruleContent": {
            "content": "#imgs@html",
            "imageUrl": "@js:result + '?tok=1'"
        }
    }))
    .unwrap();

    let base_c = base.clone();
    let (mixed_no_rule, mixed_with_rule, wrapper_with_rule, mixed_ml_rule) =
        tokio::task::spawn_blocking(move || {
            let p1 = engine
                .fetch_pages_sync(
                    &mixed_plain_src,
                    SourceRuntimeState::default(),
                    &format!("{base_c}/ch/mixed"),
                    &ComicJsContext::default(),
                    &ComicChapterJsContext::default(),
                )
                .unwrap()
                .data;
            let p2 = engine
                .fetch_pages_sync(
                    &mixed_rule_src,
                    SourceRuntimeState::default(),
                    &format!("{base_c}/ch/mixed"),
                    &ComicJsContext::default(),
                    &ComicChapterJsContext::default(),
                )
                .unwrap()
                .data;
            let p3 = engine
                .fetch_pages_sync(
                    &wrapper_rule_src,
                    SourceRuntimeState::default(),
                    &format!("{base_c}/ch/html-wrapper-rule"),
                    &ComicJsContext::default(),
                    &ComicChapterJsContext::default(),
                )
                .unwrap()
                .data;
            let p4 = engine
                .fetch_pages_sync(
                    &mixed_rule_src,
                    SourceRuntimeState::default(),
                    &format!("{base_c}/ch/mixed-multiline-rule"),
                    &ComicJsContext::default(),
                    &ComicChapterJsContext::default(),
                )
                .unwrap()
                .data;
            (p1, p2, p3, p4)
        })
        .await
        .unwrap();

    assert_eq!(
        mixed_no_rule,
        vec![
            "https://cdn.example/plain1.jpg".to_string(),
            format!("{base}/img2.jpg"),
            format!("{base}/plain3.jpg"),
        ]
    );
    assert_eq!(
        mixed_with_rule,
        vec![
            "https://cdn.example/plain1.jpg?tok=1".to_string(),
            format!("{base}/img2.jpg?tok=1"),
            format!("{base}/plain3.jpg?tok=1"),
        ]
    );
    assert_eq!(wrapper_with_rule, vec![format!("{base}/a.jpg?tok=1")]);
    assert_eq!(
        mixed_ml_rule,
        vec![
            format!("{base}/a.jpg?tok=1"),
            format!("{base}/plain_after.jpg?tok=1"),
        ]
    );

    server.abort();
}

#[tokio::test]
async fn fetch_chapters_and_pages_stop_on_302_redirect_alias_cycle() {
    use axum::extract::Query;
    use std::sync::atomic::{AtomicUsize, Ordering};

    #[derive(serde::Deserialize)]
    struct PQuery {
        p: Option<u32>,
    }

    let toc_p1_hits = Arc::new(AtomicUsize::new(0));
    let ch_p1_hits = Arc::new(AtomicUsize::new(0));
    let toc_p1_hits_c = toc_p1_hits.clone();
    let ch_p1_hits_c = ch_p1_hits.clone();

    let app = Router::new()
        .route(
            "/toc/alias_entry",
            get(|| async {
                Response::builder()
                    .status(StatusCode::FOUND)
                    .header("location", "/toc/alias_real?p=1")
                    .body(Body::empty())
                    .unwrap()
            }),
        )
        .route(
            "/toc/alias_redir",
            get(|| async {
                Response::builder()
                    .status(StatusCode::FOUND)
                    .header("location", "/toc/alias_real?p=1")
                    .body(Body::empty())
                    .unwrap()
            }),
        )
        .route(
            "/toc/alias_real",
            get(move |Query(q): Query<PQuery>| {
                let hits = toc_p1_hits_c.clone();
                async move {
                    if q.p.unwrap_or(1) == 1 {
                        hits.fetch_add(1, Ordering::SeqCst);
                        Response::builder()
                            .status(StatusCode::OK)
                            .header("content-type", "text/html; charset=utf-8")
                            .body(Body::from(
                                r#"<ul><li><a href="/ch/1">Alias Ch 1</a></li></ul>
                                <a class="next" href="/toc/alias_real?p=2">Next</a>"#,
                            ))
                            .unwrap()
                    } else {
                        Response::builder()
                            .status(StatusCode::OK)
                            .header("content-type", "text/html; charset=utf-8")
                            .body(Body::from(
                                r#"<ul><li><a href="/ch/2">Alias Ch 2</a></li></ul>
                                <a class="next" href="/toc/alias_redir">Alias Loop</a>"#,
                            ))
                            .unwrap()
                    }
                }
            }),
        )
        .route(
            "/ch/alias_entry",
            get(|| async {
                Response::builder()
                    .status(StatusCode::FOUND)
                    .header("location", "/ch/alias_real?p=1")
                    .body(Body::empty())
                    .unwrap()
            }),
        )
        .route(
            "/ch/alias_redir",
            get(|| async {
                Response::builder()
                    .status(StatusCode::FOUND)
                    .header("location", "/ch/alias_real?p=1")
                    .body(Body::empty())
                    .unwrap()
            }),
        )
        .route(
            "/ch/alias_real",
            get(move |Query(q): Query<PQuery>| {
                let hits = ch_p1_hits_c.clone();
                async move {
                    if q.p.unwrap_or(1) == 1 {
                        hits.fetch_add(1, Ordering::SeqCst);
                        Response::builder()
                            .status(StatusCode::OK)
                            .header("content-type", "text/html; charset=utf-8")
                            .body(Body::from(
                                r#"<div id="imgs"><img src="/ap1.jpg"></div>
                                <a class="next-page" href="/ch/alias_real?p=2">Next</a>"#,
                            ))
                            .unwrap()
                    } else {
                        Response::builder()
                            .status(StatusCode::OK)
                            .header("content-type", "text/html; charset=utf-8")
                            .body(Body::from(
                                r#"<div id="imgs"><img src="/ap2.jpg"></div>
                                <a class="next-page" href="/ch/alias_redir">Alias Loop</a>"#,
                            ))
                            .unwrap()
                    }
                }
            }),
        );

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let server = tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });
    let base = format!("http://{addr}");

    let source: ComicSourceSpec = serde_json::from_value(serde_json::json!({
        "sourceName": "AliasCycleTest",
        "sourceUrl": base,
        "ruleToc": {
            "chapterList": "ul li",
            "chapterName": "a@text",
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
    let (chapters, pages) = tokio::task::spawn_blocking(move || {
        let ch = engine
            .fetch_chapters_sync(
                &source,
                SourceRuntimeState::default(),
                &format!("{base_c}/toc/alias_entry"),
                &ComicJsContext::default(),
            )
            .unwrap()
            .data;
        let pg = engine
            .fetch_pages_sync(
                &source,
                SourceRuntimeState::default(),
                &format!("{base_c}/ch/alias_entry"),
                &ComicJsContext::default(),
                &ComicChapterJsContext::default(),
            )
            .unwrap()
            .data;
        (ch, pg)
    })
    .await
    .unwrap();

    assert_eq!(chapters.len(), 2);
    assert_eq!(chapters[0].title, "Alias Ch 1");
    assert_eq!(chapters[1].title, "Alias Ch 2");
    // /toc/alias_entry -> 302 -> p=1 (hit 1), then /toc/alias_redir -> 302 -> p=1 (hit 2),
    // after which fetch_chapters breaks before parsing p=1 a second time.
    assert_eq!(toc_p1_hits.load(Ordering::SeqCst), 2);

    assert_eq!(
        pages,
        vec![format!("{base}/ap1.jpg"), format!("{base}/ap2.jpg")]
    );
    assert_eq!(ch_p1_hits.load(Ordering::SeqCst), 2);

    server.abort();
}

#[tokio::test]
async fn explore_blocks_unmatched_js_mustache_options_and_non_http_while_allowing_configured_categories()
 {
    use axum::extract::Query;
    use axum::routing::post;
    use std::sync::atomic::{AtomicUsize, Ordering};

    #[derive(serde::Deserialize)]
    struct ListQ {
        p: Option<u32>,
    }

    let exfil_hits = Arc::new(AtomicUsize::new(0));
    let exfil_hits_c = exfil_hits.clone();

    let app = Router::new()
        .route(
            "/exfil",
            get(move || {
                let hits = exfil_hits_c.clone();
                async move {
                    hits.fetch_add(1, Ordering::SeqCst);
                    Response::builder()
                        .status(StatusCode::OK)
                        .header("content-type", "application/json")
                        .body(Body::from(r#"{"items":[{"t":"Exfil","u":"/c/e"}]}"#))
                        .unwrap()
                }
            }),
        )
        .route(
            "/list",
            get(|Query(q): Query<ListQ>| async move {
                if q.p == Some(20) {
                    Response::builder()
                        .status(StatusCode::OK)
                        .header("content-type", "application/json")
                        .body(Body::from(r#"{"items":[{"t":"CatOff20","u":"/c/20"}]}"#))
                        .unwrap()
                } else {
                    Response::builder()
                        .status(StatusCode::BAD_REQUEST)
                        .body(Body::from("{}"))
                        .unwrap()
                }
            }),
        )
        .route(
            "/post_cat",
            post(|body: String| async move {
                if body.trim() == "page=2" {
                    Response::builder()
                        .status(StatusCode::OK)
                        .header("content-type", "application/json")
                        .body(Body::from(r#"{"items":[{"t":"CatPost2","u":"/c/p2"}]}"#))
                        .unwrap()
                } else {
                    Response::builder()
                        .status(StatusCode::BAD_REQUEST)
                        .body(Body::from("{}"))
                        .unwrap()
                }
            }),
        )
        .route(
            "/custom",
            get(|Query(q): Query<ListQ>| async move {
                let p = q.p.unwrap_or(0);
                Response::builder()
                    .status(StatusCode::OK)
                    .header("content-type", "application/json")
                    .body(Body::from(format!(
                        r#"{{"items":[{{"t":"CustomP{p}","u":"/c/custom{p}"}}]}}"#
                    )))
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
        "sourceName": "ExploreGuardTest",
        "sourceUrl": base,
        "enabledExplore": true,
        "exploreUrl": "JsOffset::/list?p={{(page - 1) * 20}}\nPostOpt::/post_cat,{\"method\":\"POST\",\"body\":\"page={{page}}\"}",
        "ruleSearch": {
            "bookList": "$.items[*]",
            "name": "$.t",
            "bookUrl": "$.u"
        }
    }))
    .unwrap();

    let mut initial_state = SourceRuntimeState {
        variable: "ADMIN_SECRET".to_string(),
        ..Default::default()
    };
    initial_state
        .login_info
        .insert("apiToken".into(), "TOP_SECRET_TOKEN".into());

    let engine = ComicSourceEngine::new(
        HttpClientConfig {
            allow_private_ips: true,
            ..Default::default()
        },
        Duration::from_secs(10),
    );

    let base_c = base.clone();
    let src_c = source.clone();
    let st_c = initial_state.clone();
    tokio::task::spawn_blocking(move || {
        // 1. Configured categories still evaluate JS/options
        let cats = engine
            .parse_explore_categories(&src_c, st_c.clone())
            .unwrap()
            .data;
        assert_eq!(cats.len(), 2);

        let r_js = engine
            .explore_sync_with_control(
                &src_c,
                st_c.clone(),
                &cats[0].url,
                2,
                Duration::from_secs(10),
                Arc::new(AtomicBool::new(false)),
            )
            .unwrap();
        assert_eq!(r_js.data[0].title, "CatOff20");

        let r_post = engine
            .explore_sync_with_control(
                &src_c,
                st_c.clone(),
                &cats[1].url,
                2,
                Duration::from_secs(10),
                Arc::new(AtomicBool::new(false)),
            )
            .unwrap();
        assert_eq!(r_post.data[0].title, "CatPost2");

        // 2. Unmatched @js:, <js>, {{expr}}, ,{options}, and data: must ALL fail without touching QuickJS
        let bad_inputs = [
            format!("@js:source.setVariable('PWNED');java.toast('hi');java.ajax('{base_c}/exfil?v=' + source.getVariable());'{base_c}/exfil'"),
            format!("<js>source.setVariable('PWNED');'{base_c}/exfil'</js>"),
            format!("{base_c}/exfil?v={{{{source.getVariable()}}}}&k={{{{source.getLoginInfoMap().apiToken}}}}"),
            format!("{base_c}/post_cat,{{\"method\":\"POST\",\"body\":\"page=2\"}}"),
            "data:text/plain;base64,eyJpdGVtcyI6W119".to_string(),
        ];
        for bad in bad_inputs {
            let err = engine
                .explore_sync_with_control(
                    &src_c,
                    st_c.clone(),
                    &bad,
                    1,
                    Duration::from_secs(10),
                    Arc::new(AtomicBool::new(false)),
                )
                .expect_err(&format!("expected unmatched exploreUrl to fail: {bad}"));
            assert!(
                err.diagnostics.toasts.is_empty(),
                "QuickJS should never execute for unmatched exploreUrl: {bad}"
            );
        }

        // 3. Unmatched plain HTTP(S) URL with {{page}} substitutes {{page}} statically without QuickJS
        let r_plain = engine
            .explore_sync_with_control(
                &src_c,
                st_c,
                &format!("{base_c}/custom?p={{{{page}}}}"),
                3,
                Duration::from_secs(10),
                Arc::new(AtomicBool::new(false)),
            )
            .unwrap();
        assert_eq!(r_plain.data[0].title, "CustomP3");
    })
    .await
    .unwrap();

    assert_eq!(exfil_hits.load(Ordering::SeqCst), 0);
    server.abort();
}

#[tokio::test]
async fn search_blocks_keyword_options_smuggling() {
    use axum::routing::post;
    use std::sync::atomic::{AtomicUsize, Ordering};

    let post_hits = Arc::new(AtomicUsize::new(0));
    let post_hits_c = post_hits.clone();

    let app = Router::new().route(
        "/s",
        post(move || {
            let hits = post_hits_c.clone();
            async move {
                hits.fetch_add(1, Ordering::SeqCst);
                Response::builder()
                    .status(StatusCode::OK)
                    .header("content-type", "application/json")
                    .body(Body::from(r#"{"items":[{"t":"Smuggled","u":"/c/s"}]}"#))
                    .unwrap()
            }
        }),
    );

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let server = tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });
    let base = format!("http://{addr}");

    let source: ComicSourceSpec = serde_json::from_value(serde_json::json!({
        "sourceName": "SearchSmuggle",
        "sourceUrl": base,
        "searchUrl": format!("{base}/s?q={{{{key}}}}"),
        "ruleSearch": {
            "bookList": "$.items[*]",
            "name": "$.t",
            "bookUrl": "$.u"
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

    let err = tokio::task::spawn_blocking(move || {
        engine.search_sync(
            &source,
            SourceRuntimeState::default(),
            r#"x,{"method":"POST","body":"evil=1"}"#,
            1,
        )
    })
    .await
    .unwrap()
    .expect_err("keyword smuggling ,{options} should be rejected");
    assert!(err.contains("选项"));
    assert_eq!(post_hits.load(Ordering::SeqCst), 0);

    server.abort();
}

#[tokio::test]
async fn reader_op_timeout_and_cancel_interrupt_paginated_chapters_and_pages() {
    let app = Router::new()
        .route(
            "/detail_slow",
            get(|| async {
                tokio::time::sleep(Duration::from_millis(600)).await;
                Response::builder()
                    .status(StatusCode::OK)
                    .body(Body::from("<h1>Slow</h1>"))
                    .unwrap()
            }),
        )
        .route(
            "/toc1",
            get(|| async {
                tokio::time::sleep(Duration::from_millis(120)).await;
                Response::builder()
                    .status(StatusCode::OK)
                    .header("content-type", "text/html")
                    .body(Body::from(
                        r#"<ul><li><a href="/ch/1">Ch 1</a></li></ul><a class="next" href="/toc2">Next</a>"#,
                    ))
                    .unwrap()
            }),
        )
        .route(
            "/toc2",
            get(|| async {
                tokio::time::sleep(Duration::from_millis(600)).await;
                Response::builder()
                    .status(StatusCode::OK)
                    .header("content-type", "text/html")
                    .body(Body::from(r#"<ul><li><a href="/ch/2">Ch 2</a></li></ul>"#))
                    .unwrap()
            }),
        )
        .route(
            "/ch1",
            get(|| async {
                tokio::time::sleep(Duration::from_millis(120)).await;
                Response::builder()
                    .status(StatusCode::OK)
                    .header("content-type", "text/html")
                    .body(Body::from(
                        r#"<div id="imgs"><img src="/p1.jpg"></div><a class="next-page" href="/ch2">Next</a>"#,
                    ))
                    .unwrap()
            }),
        )
        .route(
            "/ch2",
            get(|| async {
                tokio::time::sleep(Duration::from_millis(600)).await;
                Response::builder()
                    .status(StatusCode::OK)
                    .header("content-type", "text/html")
                    .body(Body::from(r#"<div id="imgs"><img src="/p2.jpg"></div>"#))
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
        "sourceName": "SlowReader",
        "sourceUrl": base,
        "ruleDetail": { "name": "h1@text" },
        "ruleToc": {
            "chapterList": "ul li",
            "chapterName": "a@text",
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
    tokio::task::spawn_blocking(move || {
        // Short op_timeout interrupts detail_slow
        let t0 = std::time::Instant::now();
        let err_det = engine
            .fetch_detail_sync_with_control(
                &source,
                SourceRuntimeState::default(),
                &format!("{base_c}/detail_slow"),
                None,
                Duration::from_millis(220),
                Arc::new(AtomicBool::new(false)),
            )
            .expect_err("slow detail should time out");
        assert!(
            t0.elapsed() < Duration::from_secs(2),
            "detail took too long: {}",
            err_det.message
        );

        // Short op_timeout interrupts paginated /toc1 -> /toc2
        let t1 = std::time::Instant::now();
        let err_ch = engine
            .fetch_chapters_sync_with_control(
                &source,
                SourceRuntimeState::default(),
                &format!("{base_c}/toc1"),
                &ComicJsContext::default(),
                Duration::from_millis(250),
                Arc::new(AtomicBool::new(false)),
            )
            .expect_err("slow paginated chapters should time out");
        assert!(
            t1.elapsed() < Duration::from_secs(2),
            "chapters took too long: {}",
            err_ch.message
        );

        // Pre-triggered cancel_flag interrupts paginated /ch1 -> /ch2 immediately
        let cancel_pg = Arc::new(AtomicBool::new(true));
        let err_pg = engine
            .fetch_pages_sync_with_control(
                &source,
                SourceRuntimeState::default(),
                &format!("{base_c}/ch1"),
                &ComicJsContext::default(),
                &ComicChapterJsContext::default(),
                Duration::from_secs(5),
                cancel_pg,
            )
            .expect_err("cancelled pages should abort");
        assert!(err_pg.message.contains("取消") || err_pg.message.contains("超时"));
    })
    .await
    .unwrap();

    server.abort();
}

#[tokio::test]
async fn extract_page_urls_html5_segmenter_blocks_inner_text_style_script_and_preserves_mixed_plain_urls()
 {
    let app = Router::new()
        .route(
            "/ch/wrapper-with-text",
            get(|| async {
                Response::builder()
                    .status(StatusCode::OK)
                    .header("content-type", "text/html; charset=utf-8")
                    .body(Body::from(
                        r#"<div id="imgs">
  <div class="notice">
    Loading images, please wait
  </div>
  <p>
    Page 1
  </p>
  <style>
    .page { width: 100%; }
  </style>
  <script>
    var ad = 1; var t = "</div>"; if (a < b) {}
  </script>
  <noscript>
    Please enable JavaScript
  </noscript>
  <img src="/real1.jpg">
</div>"#,
                    ))
                    .unwrap()
            }),
        )
        .route(
            "/ch/loose-prose",
            get(|| async {
                Response::builder()
                    .status(StatusCode::OK)
                    .header("content-type", "text/html; charset=utf-8")
                    .body(Body::from(
                        "Loading images, please wait\n<img src=\"/real1.jpg\">\nEnd of chapter",
                    ))
                    .unwrap()
            }),
        )
        .route(
            "/ch/comment-inline",
            get(|| async {
                Response::builder()
                    .status(StatusCode::OK)
                    .header("content-type", "text/html; charset=utf-8")
                    .body(Body::from(
                        "<!-- comment -->/after_comment.jpg\n<!-->\n<img src=\"/ok.jpg\">\n/plain_after.jpg",
                    ))
                    .unwrap()
            }),
        )
        .route(
            "/ch/entity-query",
            get(|| async {
                Response::builder()
                    .status(StatusCode::OK)
                    .header("content-type", "text/plain; charset=utf-8")
                    .body(Body::from(
                        "https://cdn.example/img?a=1&copy=2&lang=en\n<img src=\"/b.jpg\">",
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

    let engine = ComicSourceEngine::new(
        HttpClientConfig {
            allow_private_ips: true,
            ..Default::default()
        },
        Duration::from_secs(10),
    );

    let imgs_no_rule: ComicSourceSpec = serde_json::from_value(serde_json::json!({
        "sourceName": "Html5NoRule",
        "sourceUrl": base,
        "ruleContent": { "content": "#imgs@html" }
    }))
    .unwrap();
    let imgs_with_rule: ComicSourceSpec = serde_json::from_value(serde_json::json!({
        "sourceName": "Html5WithRule",
        "sourceUrl": base,
        "ruleContent": {
            "content": "#imgs@html",
            "imageUrl": "@js:result + '?tok=1'"
        }
    }))
    .unwrap();
    let raw_js_src: ComicSourceSpec = serde_json::from_value(serde_json::json!({
        "sourceName": "RawJs",
        "sourceUrl": base,
        "ruleContent": { "content": "@js:result" }
    }))
    .unwrap();

    let base_c = base.clone();
    let (p_no, p_yes, p_loose, p_comment, p_entity) = tokio::task::spawn_blocking(move || {
        let a = engine
            .fetch_pages_sync(
                &imgs_no_rule,
                SourceRuntimeState::default(),
                &format!("{base_c}/ch/wrapper-with-text"),
                &ComicJsContext::default(),
                &ComicChapterJsContext::default(),
            )
            .unwrap()
            .data;
        let b = engine
            .fetch_pages_sync(
                &imgs_with_rule,
                SourceRuntimeState::default(),
                &format!("{base_c}/ch/wrapper-with-text"),
                &ComicJsContext::default(),
                &ComicChapterJsContext::default(),
            )
            .unwrap()
            .data;
        let c = engine
            .fetch_pages_sync(
                &raw_js_src,
                SourceRuntimeState::default(),
                &format!("{base_c}/ch/loose-prose"),
                &ComicJsContext::default(),
                &ComicChapterJsContext::default(),
            )
            .unwrap()
            .data;
        let d = engine
            .fetch_pages_sync(
                &raw_js_src,
                SourceRuntimeState::default(),
                &format!("{base_c}/ch/comment-inline"),
                &ComicJsContext::default(),
                &ComicChapterJsContext::default(),
            )
            .unwrap()
            .data;
        let e = engine
            .fetch_pages_sync(
                &raw_js_src,
                SourceRuntimeState::default(),
                &format!("{base_c}/ch/entity-query"),
                &ComicJsContext::default(),
                &ComicChapterJsContext::default(),
            )
            .unwrap()
            .data;
        (a, b, c, d, e)
    })
    .await
    .unwrap();

    assert_eq!(p_no, vec![format!("{base}/real1.jpg")]);
    assert_eq!(p_yes, vec![format!("{base}/real1.jpg?tok=1")]);
    assert_eq!(p_loose, vec![format!("{base}/real1.jpg")]);
    assert_eq!(
        p_comment,
        vec![
            format!("{base}/after_comment.jpg"),
            format!("{base}/ok.jpg"),
            format!("{base}/plain_after.jpg"),
        ]
    );
    assert_eq!(
        p_entity,
        vec![
            "https://cdn.example/img?a=1&copy=2&lang=en".to_string(),
            format!("{base}/b.jpg"),
        ]
    );

    server.abort();
}

#[tokio::test]
async fn chapters_and_pages_support_post_options_pagination_relative_options_and_redirect_cycles() {
    use axum::extract::Query;
    use axum::routing::post;
    use std::sync::atomic::{AtomicUsize, Ordering};

    #[derive(serde::Deserialize)]
    struct PQuery {
        p: Option<u32>,
    }

    let toc_p1_hits = Arc::new(AtomicUsize::new(0));
    let ch_p1_hits = Arc::new(AtomicUsize::new(0));
    let toc_p1_hits_c = toc_p1_hits.clone();
    let ch_p1_hits_c = ch_p1_hits.clone();

    let app = Router::new()
        // Same-endpoint POST pagination for chapters + empty url part + relative options (JSON API)
        .route(
            "/toc/post",
            post(|body: String| async move {
                match body.trim() {
                    "page=1" => Response::builder()
                        .status(StatusCode::OK)
                        .header("content-type", "application/json")
                        .body(Body::from(
                            r#"{"items":[{"t":"Post Ch 1","u":"/ch/post,{\"method\":\"POST\",\"body\":\"ch=1\"}"}],"next":",{\"method\":\"POST\",\"body\":\"page=2\"}"}"#,
                        ))
                        .unwrap(),
                    "page=2" => Response::builder()
                        .status(StatusCode::OK)
                        .header("content-type", "application/json")
                        .body(Body::from(
                            r#"{"items":[{"t":"Post Ch 2","u":"/ch/post2"}],"next":"/toc/post2,{\"method\":\"POST\",\"body\":\"page=3\"}"}"#,
                        ))
                        .unwrap(),
                    _ => Response::builder().status(StatusCode::BAD_REQUEST).body(Body::empty()).unwrap(),
                }
            }),
        )
        .route(
            "/toc/post2",
            post(|body: String| async move {
                if body.trim() == "page=3" {
                    Response::builder()
                        .status(StatusCode::OK)
                        .header("content-type", "application/json")
                        .body(Body::from(
                            r#"{"items":[{"t":"Post Ch 3","u":"/ch/post3"}],"next":"/toc/post,{\"method\":\"POST\",\"body\":\"page=1\"}"}"#,
                        ))
                        .unwrap()
                } else {
                    Response::builder().status(StatusCode::BAD_REQUEST).body(Body::empty()).unwrap()
                }
            }),
        )
        // Same-endpoint POST pagination for pages + chapterUrl POST options
        .route(
            "/ch/post",
            post(|body: String| async move {
                match body.trim() {
                    "ch=1" | "page=1" => Response::builder()
                        .status(StatusCode::OK)
                        .header("content-type", "application/json")
                        .body(Body::from(
                            r#"{"imgs":["/post1.jpg"],"next":",{\"method\":\"POST\",\"body\":\"page=2\"}"}"#,
                        ))
                        .unwrap(),
                    "page=2" => Response::builder()
                        .status(StatusCode::OK)
                        .header("content-type", "application/json")
                        .body(Body::from(
                            r#"{"imgs":["/post2.jpg"],"next":"/ch/post2,{\"method\":\"POST\",\"body\":\"page=3\"}"}"#,
                        ))
                        .unwrap(),
                    _ => Response::builder().status(StatusCode::BAD_REQUEST).body(Body::empty()).unwrap(),
                }
            }),
        )
        .route(
            "/ch/post2",
            post(|body: String| async move {
                if body.trim() == "page=3" {
                    Response::builder()
                        .status(StatusCode::OK)
                        .header("content-type", "application/json")
                        .body(Body::from(
                            r#"{"imgs":["/post3.jpg"],"next":"/ch/post,{\"method\":\"POST\",\"body\":\"ch=1\"}"}"#,
                        ))
                        .unwrap()
                } else {
                    Response::builder().status(StatusCode::BAD_REQUEST).body(Body::empty()).unwrap()
                }
            }),
        )
        // Search + detail returning relative ,{options} for bookUrl & tocUrl
        .route(
            "/search_opt",
            get(|| async {
                Response::builder()
                    .status(StatusCode::OK)
                    .header("content-type", "application/json")
                    .body(Body::from(
                        r#"{"items":[{"t":"OptComic","u":"/detail_opt,{\"method\":\"POST\",\"body\":\"id=9\"}"}]}"#,
                    ))
                    .unwrap()
            }),
        )
        .route(
            "/detail_opt",
            post(|body: String| async move {
                if body.trim() == "id=9" {
                    Response::builder()
                        .status(StatusCode::OK)
                        .header("content-type", "application/json")
                        .body(Body::from(
                            r#"{"title":"OptComicDetail","toc":"/toc/post,{\"method\":\"POST\",\"body\":\"page=1\"}"}"#,
                        ))
                        .unwrap()
                } else {
                    Response::builder().status(StatusCode::BAD_REQUEST).body(Body::empty()).unwrap()
                }
            }),
        )
        // Combined multi-hop 302 redirect cycle with query & fragment
        .route(
            "/toc/hop1",
            get(|| async {
                Response::builder()
                    .status(StatusCode::FOUND)
                    .header("location", "/toc/hop2")
                    .body(Body::empty())
                    .unwrap()
            }),
        )
        .route(
            "/toc/hop2",
            get(|| async {
                Response::builder()
                    .status(StatusCode::FOUND)
                    .header("location", "/toc/real?p=1#top")
                    .body(Body::empty())
                    .unwrap()
            }),
        )
        .route(
            "/toc/real",
            get(move |Query(q): Query<PQuery>| {
                let hits = toc_p1_hits_c.clone();
                async move {
                    if q.p.unwrap_or(1) == 1 {
                        hits.fetch_add(1, Ordering::SeqCst);
                        Response::builder()
                            .status(StatusCode::OK)
                            .header("content-type", "text/html")
                            .body(Body::from(
                                r##"<ul><li><a href="/ch/1">RCh1</a></li></ul><a class="next" href="/toc/real?p=2">Next</a>"##,
                            ))
                            .unwrap()
                    } else {
                        Response::builder()
                            .status(StatusCode::OK)
                            .header("content-type", "text/html")
                            .body(Body::from(
                                r##"<ul><li><a href="/ch/2">RCh2</a></li></ul><a class="next" href="/toc/hop1#frag">Back</a>"##,
                            ))
                            .unwrap()
                    }
                }
            }),
        )
        .route(
            "/ch/hop1",
            get(|| async {
                Response::builder()
                    .status(StatusCode::FOUND)
                    .header("location", "/ch/real?p=1#sec")
                    .body(Body::empty())
                    .unwrap()
            }),
        )
        .route(
            "/ch/real",
            get(move |Query(q): Query<PQuery>| {
                let hits = ch_p1_hits_c.clone();
                async move {
                    if q.p.unwrap_or(1) == 1 {
                        hits.fetch_add(1, Ordering::SeqCst);
                        Response::builder()
                            .status(StatusCode::OK)
                            .header("content-type", "text/html")
                            .body(Body::from(
                                r##"<div id="imgs"><img src="/rp1.jpg"></div><a class="next-page" href="/ch/real?p=2">Next</a>"##,
                            ))
                            .unwrap()
                    } else {
                        Response::builder()
                            .status(StatusCode::OK)
                            .header("content-type", "text/html")
                            .body(Body::from(
                                r##"<div id="imgs"><img src="/rp2.jpg"></div><a class="next-page" href="/ch/hop1#again">Back</a>"##,
                            ))
                            .unwrap()
                    }
                }
            }),
        );

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let server = tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });
    let base = format!("http://{addr}");

    let source: ComicSourceSpec = serde_json::from_value(serde_json::json!({
        "sourceName": "PostPaginationTest",
        "sourceUrl": base,
        "searchUrl": format!("{base}/search_opt?q={{{{key}}}}"),
        "ruleSearch": {
            "bookList": "$.items[*]",
            "name": "$.t",
            "bookUrl": "$.u"
        },
        "ruleDetail": {
            "name": "$.title",
            "tocUrl": "$.toc"
        },
        "ruleToc": {
            "chapterList": "$.items[*] || ul li",
            "chapterName": "$.t || a@text",
            "chapterUrl": "$.u || a@href",
            "nextTocUrl": "$.next || a.next@href"
        },
        "ruleContent": {
            "content": "$.imgs || #imgs@html",
            "nextContentUrl": "$.next || a.next-page@href"
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
    let (search_res, detail_res, chapters_post, pages_post, chapters_redir, pages_redir) =
        tokio::task::spawn_blocking(move || {
            let sr = engine
                .search_sync(&source, SourceRuntimeState::default(), "opt", 1)
                .unwrap()
                .data;
            let dr = engine
                .fetch_detail_sync(
                    &source,
                    SourceRuntimeState::default(),
                    &sr[0].comic_url,
                    None,
                )
                .unwrap()
                .data;
            let ch_post = engine
                .fetch_chapters_sync(
                    &source,
                    SourceRuntimeState::default(),
                    &dr.toc_url,
                    &ComicJsContext::default(),
                )
                .unwrap()
                .data;
            let pg_post = engine
                .fetch_pages_sync(
                    &source,
                    SourceRuntimeState::default(),
                    &ch_post[0].url,
                    &ComicJsContext::default(),
                    &ComicChapterJsContext::default(),
                )
                .unwrap()
                .data;
            let ch_redir = engine
                .fetch_chapters_sync(
                    &source,
                    SourceRuntimeState::default(),
                    &format!("{base_c}/toc/hop1"),
                    &ComicJsContext::default(),
                )
                .unwrap()
                .data;
            let pg_redir = engine
                .fetch_pages_sync(
                    &source,
                    SourceRuntimeState::default(),
                    &format!("{base_c}/ch/hop1"),
                    &ComicJsContext::default(),
                    &ComicChapterJsContext::default(),
                )
                .unwrap()
                .data;
            (sr, dr, ch_post, pg_post, ch_redir, pg_redir)
        })
        .await
        .unwrap();

    assert_eq!(
        search_res[0].comic_url,
        format!("{base}/detail_opt,{{\"method\":\"POST\",\"body\":\"id=9\"}}")
    );
    assert_eq!(detail_res.title, "OptComicDetail");
    assert_eq!(
        detail_res.toc_url,
        format!("{base}/toc/post,{{\"method\":\"POST\",\"body\":\"page=1\"}}")
    );

    let ch_titles: Vec<&str> = chapters_post.iter().map(|c| c.title.as_str()).collect();
    assert_eq!(ch_titles, vec!["Post Ch 1", "Post Ch 2", "Post Ch 3"]);
    assert_eq!(
        chapters_post[0].url,
        format!("{base}/ch/post,{{\"method\":\"POST\",\"body\":\"ch=1\"}}")
    );

    assert_eq!(
        pages_post,
        vec![
            format!("{base}/post1.jpg"),
            format!("{base}/post2.jpg"),
            format!("{base}/post3.jpg"),
        ]
    );

    let redir_titles: Vec<&str> = chapters_redir.iter().map(|c| c.title.as_str()).collect();
    assert_eq!(redir_titles, vec!["RCh1", "RCh2"]);
    assert_eq!(toc_p1_hits.load(Ordering::SeqCst), 1);
    assert_eq!(
        pages_redir,
        vec![format!("{base}/rp1.jpg"), format!("{base}/rp2.jpg")]
    );
    assert_eq!(ch_p1_hits.load(Ordering::SeqCst), 1);

    server.abort();
}

#[tokio::test]
async fn search_blocks_js_mustache_and_json_options_breakout_smuggling() {
    use axum::routing::{delete, post};
    use std::sync::atomic::{AtomicUsize, Ordering};

    let smuggled_post_hits = Arc::new(AtomicUsize::new(0));
    let smuggled_hdr_hits = Arc::new(AtomicUsize::new(0));
    let quote_hdr_hits = Arc::new(AtomicUsize::new(0));
    let delete_hits = Arc::new(AtomicUsize::new(0));

    let post_hits_c = smuggled_post_hits.clone();
    let hdr_hits_c = smuggled_hdr_hits.clone();
    let quote_hits_c = quote_hdr_hits.clone();
    let del_hits_c = delete_hits.clone();

    let app = Router::new()
        .route(
            "/search",
            get(|headers: HeaderMap| async move {
                if headers.get("x-smuggled").is_some() {
                    return Response::builder()
                        .status(StatusCode::BAD_REQUEST)
                        .body(Body::empty())
                        .unwrap();
                }
                Response::builder()
                    .status(StatusCode::OK)
                    .header("content-type", "application/json")
                    .body(Body::from(r#"{"items":[{"t":"LegitGet","u":"/c/get"}]}"#))
                    .unwrap()
            }),
        )
        .route(
            "/search",
            post(move |headers: HeaderMap, body: String| {
                let post_hits = post_hits_c.clone();
                let hdr_hits = hdr_hits_c.clone();
                let quote_hits = quote_hits_c.clone();
                async move {
                    if headers.get("x-smuggled").is_some() {
                        hdr_hits.fetch_add(1, Ordering::SeqCst);
                    }
                    if headers.get("x-quote").and_then(|v| v.to_str().ok()) == Some("\"") {
                        quote_hits.fetch_add(1, Ordering::SeqCst);
                    }
                    if body == "smuggled" || body == "evil" || body == "[1,2]" {
                        post_hits.fetch_add(1, Ordering::SeqCst);
                    }
                    Response::builder()
                        .status(StatusCode::OK)
                        .header("content-type", "application/json")
                        .body(Body::from(
                            serde_json::json!({
                                "items": [{ "t": format!("Post:{body}"), "u": "/c/post" }]
                            })
                            .to_string(),
                        ))
                        .unwrap()
                }
            }),
        )
        .route(
            "/search",
            delete(move || {
                let del_hits = del_hits_c.clone();
                async move {
                    del_hits.fetch_add(1, Ordering::SeqCst);
                    Response::builder()
                        .status(StatusCode::OK)
                        .header("content-type", "application/json")
                        .body(Body::from(r#"{"items":[]}"#))
                        .unwrap()
                }
            }),
        );

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let server = tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });
    let base = format!("http://{addr}");

    let engine = ComicSourceEngine::new(
        HttpClientConfig {
            allow_private_ips: true,
            ..Default::default()
        },
        Duration::from_secs(10),
    );

    let make_src = |search_url: String| -> ComicSourceSpec {
        serde_json::from_value(serde_json::json!({
            "sourceName": "SmuggleTest",
            "sourceUrl": base,
            "searchUrl": search_url,
            "ruleSearch": {
                "bookList": "$.items[*]",
                "name": "$.t",
                "bookUrl": "$.u"
            }
        }))
        .unwrap()
    };

    let src_mustache_arith = make_src(format!(
        "{base}/search?offset={{{{((page - 1) * 20)}}}}&q={{{{key}}}}"
    ));
    let src_mustache_str = make_src(format!("{base}/search?q={{{{String(key)}}}}"));
    let src_js_plain = make_src(format!("@js:'{base}/search?q=' + key"));
    let src_js_concat_opts = make_src(format!(
        "@js:'{base}/search,{{\"method\":\"POST\",\"body\":\"q=' + key + '\"}}'"
    ));
    let src_js_nbsp_concat_opts = make_src(format!(
        "@js:'{base}/search?v=1,\u{00A0}{{\"method\":\"POST\",\"body\":\"q=' + key + '\"}}'"
    ));
    let src_tag_js_nbsp_concat_opts = make_src(format!(
        "<js>'{base}/search?v=1,\u{00A0}{{\"method\":\"POST\",\"body\":\"q=' + key + '\"}}'</js>"
    ));
    let src_js_unquoted_body = make_src(format!(
        "@js:'{base}/search,{{\"method\":\"POST\",\"body\":' + key + '}}'"
    ));
    let src_opts_simple = make_src(format!(
        "{base}/search,{{\"method\":\"POST\",\"body\":\"q={{{{key}}}}\"}}"
    ));
    let src_opts_mustache_js = make_src(format!(
        "{base}/search,{{\"method\":\"POST\",\"body\":\"q={{{{String(key)}}}}&off={{{{((page - 1) * 20)}}}}\"}}"
    ));
    let src_opts_unquoted_quote_expr = make_src(format!(
        "{base}/search,{{\"method\":\"POST\",\"headers\":{{\"X-Quote\":{{{{'\"'}}}}}},\"body\":\"q={{{{key}}}}\"}}"
    ));
    let src_js_stringify = make_src(format!(
        "@js:'{base}/search,' + JSON.stringify({{method:'POST', body:'q=' + key}})"
    ));
    let src_encode_uri = make_src(format!("{base}/search?q={{{{java.encodeURI(key)}}}}"));

    tokio::task::spawn_blocking(move || {
        let smuggle_payload = r#"a,{"method":"POST","headers":{"X-Smuggled":"YES"},"body":"evil"}"#;
        let breakout_payload = r#"a","headers":{"X-Smuggled":"PWNED"},"x":""#;
        let dup_method_payload = r#"a","method":"DELETE"#;
        let replacement_opts_payload = r#"x"},{"method":"DELETE","body":"evil"#;
        let replacement_junk_payload = r#"x"},junk,{"method":"DELETE","body":"evil"#;
        let replacement_comma_brace_payload = r#"x","k":"v,{"method":"DELETE","body":"evil"#;
        let replacement_nbsp_junk_payload =
            "x\"},junk,\u{00A0}{\"method\":\"DELETE\",\"body\":\"evil";

        // Bypass A: Mustache arithmetic safely percent-encodes {{key}} (GET LegitGet, zero POST smuggling),
        // while {{String(key)}} and @js: without options reject post-eval options injection.
        let ma_res = engine
            .search_sync(
                &src_mustache_arith,
                SourceRuntimeState::default(),
                smuggle_payload,
                1,
            )
            .unwrap()
            .data;
        assert_eq!(ma_res[0].title, "LegitGet");
        for src in [&src_mustache_str, &src_js_plain] {
            let err = engine
                .search_sync(src, SourceRuntimeState::default(), smuggle_payload, 1)
                .expect_err("options smuggling via keyword must be rejected");
            assert!(err.contains("选项"), "unexpected error: {err}");
        }

        // @js: manual JSON concat breakout (new key, duplicate key, and replacement ,{options}) must reject
        for payload in [
            breakout_payload,
            dup_method_payload,
            replacement_opts_payload,
            replacement_junk_payload,
            replacement_comma_brace_payload,
        ] {
            let err = engine
                .search_sync(
                    &src_js_concat_opts,
                    SourceRuntimeState::default(),
                    payload,
                    1,
                )
                .expect_err("JS JSON breakout must be rejected");
            assert!(
                err.contains("选项") || err.contains("结构"),
                "unexpected error: {err}"
            );
        }

        // @js: / <js> templates with Unicode whitespace (\u{00A0}) between ',' and '{' must
        // equally reject replacement options smuggling payloads.
        for src in [&src_js_nbsp_concat_opts, &src_tag_js_nbsp_concat_opts] {
            let err = engine
                .search_sync(
                    src,
                    SourceRuntimeState::default(),
                    replacement_nbsp_junk_payload,
                    1,
                )
                .expect_err("Unicode whitespace replacement ,{options} smuggling must be rejected");
            assert!(
                err.contains("选项") || err.contains("结构"),
                "unexpected error: {err}"
            );
        }

        // @js: unquoted JSON body slot must trigger structural probe on array brackets `[1,2]`
        let arr_err = engine
            .search_sync(
                &src_js_unquoted_body,
                SourceRuntimeState::default(),
                "[1,2]",
                1,
            )
            .expect_err("unquoted array injection in @js: options must be rejected");
        assert!(
            arr_err.contains("选项") || arr_err.contains("结构"),
            "unexpected error: {arr_err}"
        );

        // Bypass B: simple & Mustache JS ,{options} templates safely percent-encode/JSON-escape keyword inside body without injecting headers
        let res_opts = engine
            .search_sync(
                &src_opts_simple,
                SourceRuntimeState::default(),
                breakout_payload,
                1,
            )
            .unwrap()
            .data;
        assert_eq!(
            res_opts[0].title,
            format!(
                "Post:q={}",
                xmedia_source::http::url_encode_component(breakout_payload)
            )
        );

        let res_mjs_opts = engine
            .search_sync(
                &src_opts_mustache_js,
                SourceRuntimeState::default(),
                breakout_payload,
                2,
            )
            .unwrap()
            .data;
        assert_eq!(
            res_mjs_opts[0].title,
            format!("Post:q={breakout_payload}&off=20")
        );

        // Unquoted {{'"'}} expression in ,{options} before {{key}} must not corrupt in_string state
        let res_unquoted_quote = engine
            .search_sync(
                &src_opts_unquoted_quote_expr,
                SourceRuntimeState::default(),
                breakout_payload,
                1,
            )
            .unwrap()
            .data;
        assert_eq!(
            res_unquoted_quote[0].title,
            format!(
                "Post:q={}",
                xmedia_source::http::url_encode_component(breakout_payload)
            )
        );

        // Legitimate @js: JSON.stringify and encodeURI work even with braces/quotes in keyword
        let res_js_stringify = engine
            .search_sync(
                &src_js_stringify,
                SourceRuntimeState::default(),
                breakout_payload,
                1,
            )
            .unwrap()
            .data;
        assert_eq!(
            res_js_stringify[0].title,
            format!("Post:q={breakout_payload}")
        );

        let res_enc = engine
            .search_sync(
                &src_encode_uri,
                SourceRuntimeState::default(),
                smuggle_payload,
                1,
            )
            .unwrap()
            .data;
        assert_eq!(res_enc[0].title, "LegitGet");
    })
    .await
    .unwrap();

    assert_eq!(smuggled_post_hits.load(Ordering::SeqCst), 0);
    assert_eq!(smuggled_hdr_hits.load(Ordering::SeqCst), 0);
    assert_eq!(quote_hdr_hits.load(Ordering::SeqCst), 1);
    assert_eq!(delete_hits.load(Ordering::SeqCst), 0);
    server.abort();
}

#[tokio::test]
async fn pagination_cycle_guard_handles_302_post_to_get_mixed_get_307_post_and_post_200_307_alias()
{
    use axum::extract::Query;
    use axum::routing::post;
    use std::sync::atomic::{AtomicUsize, Ordering};

    #[derive(serde::Deserialize)]
    struct PQuery {
        p: Option<u32>,
    }

    let p1_302_hits = Arc::new(AtomicUsize::new(0));
    let p1_302_hits_c = p1_302_hits.clone();
    let ch_302_hits = Arc::new(AtomicUsize::new(0));
    let ch_302_hits_c = ch_302_hits.clone();

    let app = Router::new()
        // 2a: POST -> 302 -> GET -> GET#frag
        .route(
            "/toc/302-post-entry",
            post(|| async {
                Response::builder()
                    .status(StatusCode::FOUND)
                    .header("location", "/toc/302-real?p=1")
                    .body(Body::empty())
                    .unwrap()
            }),
        )
        .route(
            "/toc/302-real",
            get(move |Query(q): Query<PQuery>| {
                let hits = p1_302_hits_c.clone();
                async move {
                    if q.p.unwrap_or(1) == 1 {
                        hits.fetch_add(1, Ordering::SeqCst);
                        Response::builder()
                            .status(StatusCode::OK)
                            .header("content-type", "text/html")
                            .body(Body::from(
                                r##"<ul><li><a href="/ch/1">302-Ch1</a></li></ul><a class="next" href="/toc/302-real?p=2">Next</a>"##,
                            ))
                            .unwrap()
                    } else {
                        Response::builder()
                            .status(StatusCode::OK)
                            .header("content-type", "text/html")
                            .body(Body::from(
                                r##"<ul><li><a href="/ch/2">302-Ch2</a></li></ul><a class="next" href="/toc/302-real?p=1#frag">Loop</a>"##,
                            ))
                            .unwrap()
                    }
                }
            }),
        )
        .route(
            "/ch/302-post-entry",
            post(|| async {
                Response::builder()
                    .status(StatusCode::FOUND)
                    .header("location", "/ch/302-real?p=1")
                    .body(Body::empty())
                    .unwrap()
            }),
        )
        .route(
            "/ch/302-real",
            get(move |Query(q): Query<PQuery>| {
                let hits = ch_302_hits_c.clone();
                async move {
                    if q.p.unwrap_or(1) == 1 {
                        hits.fetch_add(1, Ordering::SeqCst);
                        Response::builder()
                            .status(StatusCode::OK)
                            .header("content-type", "text/html")
                            .body(Body::from(
                                r##"<div id="imgs"><img src="/302_1.jpg"></div><a class="next-page" href="/ch/302-real?p=2">Next</a>"##,
                            ))
                            .unwrap()
                    } else {
                        Response::builder()
                            .status(StatusCode::OK)
                            .header("content-type", "text/html")
                            .body(Body::from(
                                r##"<div id="imgs"><img src="/302_2.jpg"></div><a class="next-page" href="/ch/302-real?p=1#frag">Loop</a>"##,
                            ))
                            .unwrap()
                    }
                }
            }),
        )
        // 2b: GET /mixed -> POST 307 to /mixed
        .route(
            "/toc/mixed-endpoint",
            get(|| async {
                Response::builder()
                    .status(StatusCode::OK)
                    .header("content-type", "application/json")
                    .body(Body::from(
                        r#"{"items":[{"t":"MixedGet1","u":"/ch/m1"}],"next":"/toc/307-to-mixed,{\"method\":\"POST\",\"body\":\"p=2\"}"}"#,
                    ))
                    .unwrap()
            })
            .post(|body: String| async move {
                Response::builder()
                    .status(StatusCode::OK)
                    .header("content-type", "application/json")
                    .body(Body::from(format!(
                        r#"{{"items":[{{"t":"MixedPost2({body})","u":"/ch/m2"}}]}}"#
                    )))
                    .unwrap()
            }),
        )
        .route(
            "/toc/307-to-mixed",
            post(|| async {
                Response::builder()
                    .status(StatusCode::TEMPORARY_REDIRECT)
                    .header("location", "/toc/mixed-endpoint")
                    .body(Body::empty())
                    .unwrap()
            }),
        )
        // 2c: POST 200 -> POST 307 alias with same (method, body, headers)
        .route(
            "/toc/post-200-then-307-alias",
            post(|body: String| async move {
                assert_eq!(body.trim(), "p=1");
                Response::builder()
                    .status(StatusCode::OK)
                    .header("content-type", "application/json")
                    .body(Body::from(
                        r#"{"items":[{"t":"Ch1","u":"/ch/c1"}],"next":"/toc/307-to-post200,{\"method\":\"POST\",\"body\":\"p=1\"}"}"#,
                    ))
                    .unwrap()
            }),
        )
        .route(
            "/toc/307-to-post200",
            post(|| async {
                Response::builder()
                    .status(StatusCode::TEMPORARY_REDIRECT)
                    .header("location", "/toc/post-200-then-307-alias")
                    .body(Body::empty())
                    .unwrap()
            }),
        )
        // 307 cycle: POST -> 307 -> POST /307-real?p=1 -> GET /307-real?p=2 -> POST /307-entry
        .route(
            "/toc/307-entry",
            post(|| async {
                Response::builder()
                    .status(StatusCode::TEMPORARY_REDIRECT)
                    .header("location", "/toc/307-real?p=1#top")
                    .body(Body::empty())
                    .unwrap()
            }),
        )
        .route(
            "/toc/307-real",
            post(|| async {
                Response::builder()
                    .status(StatusCode::OK)
                    .header("content-type", "text/html")
                    .body(Body::from(
                        r#"<ul><li><a href="/ch/1">307-Ch1-POST</a></li></ul><a class="next" href="/toc/307-real?p=2">Next</a>"#,
                    ))
                    .unwrap()
            })
            .get(|| async {
                Response::builder()
                    .status(StatusCode::OK)
                    .header("content-type", "text/html")
                    .body(Body::from(
                        r#"<ul><li><a href="/ch/2">307-Ch2-GET</a></li></ul><a class="next" href="/toc/307-entry,{&quot;method&quot;:&quot;POST&quot;,&quot;body&quot;:&quot;p=1&quot;}">Loop</a>"#,
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
        "sourceName": "Redirect302_307Test",
        "sourceUrl": base,
        "ruleToc": {
            "chapterList": "$.items[*] || ul li",
            "chapterName": "$.t || a@text",
            "chapterUrl": "$.u || a@href",
            "nextTocUrl": "$.next || a.next@href"
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
    let (ch_2a, pg_2a, ch_2b, ch_2c, ch_307) = tokio::task::spawn_blocking(move || {
        let ch_2a = engine
            .fetch_chapters_sync(
                &source,
                SourceRuntimeState::default(),
                &format!("{base_c}/toc/302-post-entry,{{\"method\":\"POST\",\"body\":\"p=1\"}}"),
                &ComicJsContext::default(),
            )
            .unwrap()
            .data;
        let pg_2a = engine
            .fetch_pages_sync(
                &source,
                SourceRuntimeState::default(),
                &format!("{base_c}/ch/302-post-entry,{{\"method\":\"POST\",\"body\":\"p=1\"}}"),
                &ComicJsContext::default(),
                &ComicChapterJsContext::default(),
            )
            .unwrap()
            .data;
        let ch_2b = engine
            .fetch_chapters_sync(
                &source,
                SourceRuntimeState::default(),
                &format!("{base_c}/toc/mixed-endpoint"),
                &ComicJsContext::default(),
            )
            .unwrap()
            .data;
        let ch_2c = engine
            .fetch_chapters_sync(
                &source,
                SourceRuntimeState::default(),
                &format!(
                    "{base_c}/toc/post-200-then-307-alias,{{\"method\":\"POST\",\"body\":\"p=1\"}}"
                ),
                &ComicJsContext::default(),
            )
            .unwrap()
            .data;
        let ch_307 = engine
            .fetch_chapters_sync(
                &source,
                SourceRuntimeState::default(),
                &format!("{base_c}/toc/307-entry,{{\"method\":\"POST\",\"body\":\"p=1\"}}"),
                &ComicJsContext::default(),
            )
            .unwrap()
            .data;
        (ch_2a, pg_2a, ch_2b, ch_2c, ch_307)
    })
    .await
    .unwrap();

    let t_2a: Vec<&str> = ch_2a.iter().map(|c| c.title.as_str()).collect();
    assert_eq!(t_2a, vec!["302-Ch1", "302-Ch2"]);
    assert_eq!(p1_302_hits.load(Ordering::SeqCst), 1);

    assert_eq!(
        pg_2a,
        vec![format!("{base}/302_1.jpg"), format!("{base}/302_2.jpg")]
    );
    assert_eq!(ch_302_hits.load(Ordering::SeqCst), 1);

    let t_2b: Vec<&str> = ch_2b.iter().map(|c| c.title.as_str()).collect();
    assert_eq!(t_2b, vec!["MixedGet1", "MixedPost2(p=2)"]);

    let t_2c: Vec<&str> = ch_2c.iter().map(|c| c.title.as_str()).collect();
    assert_eq!(t_2c, vec!["Ch1"]);

    let t_307: Vec<&str> = ch_307.iter().map(|c| c.title.as_str()).collect();
    assert_eq!(t_307, vec!["307-Ch1-POST", "307-Ch2-GET"]);

    server.abort();
}

#[tokio::test]
async fn pagination_cycle_guard_handles_302_alias_to_post_target_and_clamped_307_shared_url() {
    use axum::extract::Query;
    use axum::routing::post;
    use std::sync::atomic::{AtomicUsize, Ordering};

    #[derive(serde::Deserialize)]
    struct PQuery {
        p: Option<u32>,
    }

    let p1_2d_hits = Arc::new(AtomicUsize::new(0));
    let p1_2d_hits_c = p1_2d_hits.clone();
    let shared_2e_hits = Arc::new(AtomicUsize::new(0));
    let shared_2e_hits_c = shared_2e_hits.clone();

    let app = Router::new()
        // 2d: POST -> 302 -> GET /2d-real?p=1 -> GET /2d-real?p=2 -> GET /2d-alias -> 302 -> GET /2d-real?p=1
        .route(
            "/toc/2d-start",
            post(|| async {
                Response::builder()
                    .status(StatusCode::FOUND)
                    .header("location", "/toc/2d-real?p=1")
                    .body(Body::empty())
                    .unwrap()
            }),
        )
        .route(
            "/toc/2d-real",
            get(move |Query(q): Query<PQuery>| {
                let hits = p1_2d_hits_c.clone();
                async move {
                    if q.p.unwrap_or(1) == 1 {
                        hits.fetch_add(1, Ordering::SeqCst);
                        Response::builder()
                            .status(StatusCode::OK)
                            .header("content-type", "application/json")
                            .body(Body::from(
                                r#"{"items":[{"t":"2d-Ch1","u":"/ch/2d1"}],"next":"/toc/2d-real?p=2"}"#,
                            ))
                            .unwrap()
                    } else {
                        Response::builder()
                            .status(StatusCode::OK)
                            .header("content-type", "application/json")
                            .body(Body::from(
                                r#"{"items":[{"t":"2d-Ch2","u":"/ch/2d2"}],"next":"/toc/2d-alias"}"#,
                            ))
                            .unwrap()
                    }
                }
            }),
        )
        .route(
            "/toc/2d-alias",
            get(|| async {
                Response::builder()
                    .status(StatusCode::FOUND)
                    .header("location", "/toc/2d-real?p=1")
                    .body(Body::empty())
                    .unwrap()
            }),
        )
        // 2e: POST /toc/2e-hop -> 307 /toc/2e-shared (p=1 -> Ch1, p=2 -> Ch2, p=3/p=4 clamped to identical p=2 body)
        .route(
            "/toc/2e-hop",
            post(|| async {
                Response::builder()
                    .status(StatusCode::TEMPORARY_REDIRECT)
                    .header("location", "/toc/2e-shared")
                    .body(Body::empty())
                    .unwrap()
            }),
        )
        .route(
            "/toc/2e-shared",
            post(move |body: String| {
                let hits = shared_2e_hits_c.clone();
                async move {
                    hits.fetch_add(1, Ordering::SeqCst);
                    let resp_body = if body.trim() == "p=1" {
                        r#"{"items":[{"t":"2e-Ch1","u":"/ch/2e1"}]}"#
                    } else {
                        r#"{"items":[{"t":"2e-Ch2","u":"/ch/2e2"}]}"#
                    };
                    Response::builder()
                        .status(StatusCode::OK)
                        .header("content-type", "application/json")
                        .body(Body::from(resp_body))
                        .unwrap()
                }
            }),
        );

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let server = tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });
    let base = format!("http://{addr}");

    let source_2d: ComicSourceSpec = serde_json::from_value(serde_json::json!({
        "sourceName": "Guard2d",
        "sourceUrl": base,
        "ruleToc": {
            "chapterList": "$.items[*]",
            "chapterName": "$.t",
            "chapterUrl": "$.u",
            "nextTocUrl": "$.next"
        }
    }))
    .unwrap();

    let source_2e: ComicSourceSpec = serde_json::from_value(serde_json::json!({
        "sourceName": "Guard2e",
        "sourceUrl": base,
        "ruleToc": {
            "chapterList": "$.items[*]",
            "chapterName": "$.t",
            "chapterUrl": "$.u",
            "nextTocUrl": "@js: var c = Number(java.get('p2e') || 1); var n = c + 1; java.put('p2e', String(n)); n <= 4 ? baseUrl.replace('/toc/2e-shared', '/toc/2e-hop') + ',{\"method\":\"POST\",\"body\":\"p=' + n + '\"}' : ''"
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
    let (ch_2d, ch_2e) = tokio::task::spawn_blocking(move || {
        let ch_2d = engine
            .fetch_chapters_sync(
                &source_2d,
                SourceRuntimeState::default(),
                &format!("{base_c}/toc/2d-start,{{\"method\":\"POST\",\"body\":\"p=1\"}}"),
                &ComicJsContext::default(),
            )
            .unwrap()
            .data;
        let ch_2e = engine
            .fetch_chapters_sync(
                &source_2e,
                SourceRuntimeState::default(),
                &format!("{base_c}/toc/2e-hop,{{\"method\":\"POST\",\"body\":\"p=1\"}}"),
                &ComicJsContext::default(),
            )
            .unwrap()
            .data;
        (ch_2d, ch_2e)
    })
    .await
    .unwrap();

    let t_2d: Vec<&str> = ch_2d.iter().map(|c| c.title.as_str()).collect();
    assert_eq!(t_2d, vec!["2d-Ch1", "2d-Ch2"]);
    // `GET /toc/2d-alias` redirects to `/toc/2d-real?p=1` server-side before `record_fetched`
    // can inspect `final_url`, so p=1 sees 2 HTTP requests (initial POST->302 + alias GET->302)
    // and terminates immediately without duplicating 2d-Ch1 or re-entering p=2.
    assert_eq!(p1_2d_hits.load(Ordering::SeqCst), 2);

    let t_2e: Vec<&str> = ch_2e.iter().map(|c| c.title.as_str()).collect();
    assert_eq!(t_2e, vec!["2e-Ch1", "2e-Ch2"]);
    // p=1 (2e-Ch1), p=2 (2e-Ch2), p=3 (clamped identical 2e-Ch2 -> stopped by guard; p=4 never fetched).
    assert_eq!(shared_2e_hits.load(Ordering::SeqCst), 3);

    server.abort();
}

#[tokio::test]
async fn pagination_cycle_guard_stops_clamped_direct_post_200_pages() {
    use axum::routing::post;
    use std::sync::atomic::{AtomicUsize, Ordering};

    let direct_2e_hits = Arc::new(AtomicUsize::new(0));
    let direct_2e_hits_c = direct_2e_hits.clone();

    let app = Router::new()
        .route(
            "/toc/2e-direct",
            post(move |body: String| {
                let hits = direct_2e_hits_c.clone();
                async move {
                    hits.fetch_add(1, Ordering::SeqCst);
                    let resp_body = if body.trim() == "p=1" {
                        r#"{"items":[{"t":"2eDir-Ch1","u":"/ch/2ed1"}]}"#
                    } else {
                        r#"{"items":[{"t":"2eDir-Ch2","u":"/ch/2ed2"}]}"#
                    };
                    Response::builder()
                        .status(StatusCode::OK)
                        .header("content-type", "application/json")
                        .body(Body::from(resp_body))
                        .unwrap()
                }
            }),
        )
        .route(
            "/toc/post-then-get-distinct",
            post(|| async {
                Response::builder()
                    .status(StatusCode::OK)
                    .header("content-type", "application/json")
                    .body(Body::from(
                        r#"{"items":[{"t":"PostCh1","u":"/ch/p1"}],"next":"/toc/post-then-get-distinct"}"#,
                    ))
                    .unwrap()
            })
            .get(|| async {
                Response::builder()
                    .status(StatusCode::OK)
                    .header("content-type", "application/json")
                    .body(Body::from(
                        r#"{"items":[{"t":"GetCh2","u":"/ch/g2"}]}"#,
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

    let source_direct: ComicSourceSpec = serde_json::from_value(serde_json::json!({
        "sourceName": "Guard2eDirect",
        "sourceUrl": base,
        "ruleToc": {
            "chapterList": "$.items[*]",
            "chapterName": "$.t",
            "chapterUrl": "$.u",
            "nextTocUrl": "@js: var c = Number(java.get('p2ed') || 1); var n = c + 1; java.put('p2ed', String(n)); n <= 4 ? baseUrl + ',{\"method\":\"POST\",\"body\":\"p=' + n + '\"}' : ''"
        }
    }))
    .unwrap();

    let source_post_get: ComicSourceSpec = serde_json::from_value(serde_json::json!({
        "sourceName": "GuardPostThenGet",
        "sourceUrl": base,
        "ruleToc": {
            "chapterList": "$.items[*]",
            "chapterName": "$.t",
            "chapterUrl": "$.u",
            "nextTocUrl": "$.next"
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
    let (ch_direct, ch_post_get) = tokio::task::spawn_blocking(move || {
        let ch_direct = engine
            .fetch_chapters_sync(
                &source_direct,
                SourceRuntimeState::default(),
                &format!("{base_c}/toc/2e-direct,{{\"method\":\"POST\",\"body\":\"p=1\"}}"),
                &ComicJsContext::default(),
            )
            .unwrap()
            .data;
        let ch_post_get = engine
            .fetch_chapters_sync(
                &source_post_get,
                SourceRuntimeState::default(),
                &format!(
                    "{base_c}/toc/post-then-get-distinct,{{\"method\":\"POST\",\"body\":\"p=1\"}}"
                ),
                &ComicJsContext::default(),
            )
            .unwrap()
            .data;
        (ch_direct, ch_post_get)
    })
    .await
    .unwrap();

    let t_direct: Vec<&str> = ch_direct.iter().map(|c| c.title.as_str()).collect();
    assert_eq!(t_direct, vec!["2eDir-Ch1", "2eDir-Ch2"]);
    // p=1 (2eDir-Ch1), p=2 (2eDir-Ch2), p=3 (clamped identical 2eDir-Ch2 -> stopped by guard; p=4 never fetched).
    assert_eq!(direct_2e_hits.load(Ordering::SeqCst), 3);

    let t_post_get: Vec<&str> = ch_post_get.iter().map(|c| c.title.as_str()).collect();
    assert_eq!(t_post_get, vec!["PostCh1", "GetCh2"]);

    server.abort();
}

#[tokio::test]
async fn extract_page_urls_self_closing_svg_and_math_do_not_swallow_trailing_urls() {
    let app = Router::new()
        .route(
            "/ch/svg-self-close",
            get(|| async {
                Response::builder()
                    .status(StatusCode::OK)
                    .header("content-type", "text/html; charset=utf-8")
                    .body(Body::from(
                        "<svg width=\"10\" height=\"10\" />\nhttps://cdn.example/after_svg.jpg\n<svg/>\nhttps://cdn.example/after_svg_compact.jpg\n",
                    ))
                    .unwrap()
            }),
        )
        .route(
            "/ch/math-self-close",
            get(|| async {
                Response::builder()
                    .status(StatusCode::OK)
                    .header("content-type", "text/html; charset=utf-8")
                    .body(Body::from(
                        "<math xmlns=\"http://www.w3.org/1998/Math/MathML\" />\nhttps://cdn.example/after_math.jpg\n<svg><path d=\"M0 0\" /></svg>\nhttps://cdn.example/after_nested_svg.jpg\n<math><mi>x</mi></math>\nhttps://cdn.example/after_nested_math.jpg\n",
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

    let engine = ComicSourceEngine::new(
        HttpClientConfig {
            allow_private_ips: true,
            ..Default::default()
        },
        Duration::from_secs(10),
    );
    let raw_src: ComicSourceSpec = serde_json::from_value(serde_json::json!({
        "sourceName": "SelfCloseForeign",
        "sourceUrl": base,
        "ruleContent": { "content": "@js:result" }
    }))
    .unwrap();
    let rule_src: ComicSourceSpec = serde_json::from_value(serde_json::json!({
        "sourceName": "SelfCloseForeignRule",
        "sourceUrl": base,
        "ruleContent": {
            "content": "@js:result",
            "imageUrl": "@js:result + '?v=1'"
        }
    }))
    .unwrap();

    let base_c = base.clone();
    let (svg_pages, math_pages, svg_rule_pages) = tokio::task::spawn_blocking(move || {
        let p1 = engine
            .fetch_pages_sync(
                &raw_src,
                SourceRuntimeState::default(),
                &format!("{base_c}/ch/svg-self-close"),
                &ComicJsContext::default(),
                &ComicChapterJsContext::default(),
            )
            .unwrap()
            .data;
        let p2 = engine
            .fetch_pages_sync(
                &raw_src,
                SourceRuntimeState::default(),
                &format!("{base_c}/ch/math-self-close"),
                &ComicJsContext::default(),
                &ComicChapterJsContext::default(),
            )
            .unwrap()
            .data;
        let p3 = engine
            .fetch_pages_sync(
                &rule_src,
                SourceRuntimeState::default(),
                &format!("{base_c}/ch/svg-self-close"),
                &ComicJsContext::default(),
                &ComicChapterJsContext::default(),
            )
            .unwrap()
            .data;
        (p1, p2, p3)
    })
    .await
    .unwrap();

    assert_eq!(
        svg_pages,
        vec![
            "https://cdn.example/after_svg.jpg",
            "https://cdn.example/after_svg_compact.jpg",
        ]
    );
    assert_eq!(
        math_pages,
        vec![
            "https://cdn.example/after_math.jpg",
            "https://cdn.example/after_nested_svg.jpg",
            "https://cdn.example/after_nested_math.jpg",
        ]
    );
    assert_eq!(
        svg_rule_pages,
        vec![
            "https://cdn.example/after_svg.jpg?v=1",
            "https://cdn.example/after_svg_compact.jpg?v=1",
        ]
    );

    server.abort();
}

#[tokio::test]
async fn search_url_percent_encodes_key_without_corrupting_post_json_options() {
    use axum::extract::Query;
    use axum::routing::post;
    use serde::Deserialize;
    use std::sync::Mutex;

    #[derive(Deserialize)]
    struct Q {
        q: String,
    }

    let seen_get = Arc::new(Mutex::new(Vec::<String>::new()));
    let seen_post = Arc::new(Mutex::new(Vec::<String>::new()));
    let sg = seen_get.clone();
    let sp = seen_post.clone();

    let app = Router::new()
        .route(
            "/search-get",
            get(move |Query(q): Query<Q>| {
                let sg = sg.clone();
                async move {
                    sg.lock().unwrap().push(q.q);
                    Response::builder()
                        .status(StatusCode::OK)
                        .header("content-type", "application/json")
                        .body(Body::from(r#"{"items":[{"t":"Hit","u":"/c/1"}]}"#))
                        .unwrap()
                }
            }),
        )
        .route(
            "/search-post",
            post(move |body: axum::body::Bytes| {
                let sp = sp.clone();
                async move {
                    sp.lock()
                        .unwrap()
                        .push(String::from_utf8_lossy(&body).into_owned());
                    Response::builder()
                        .status(StatusCode::OK)
                        .header("content-type", "application/json")
                        .body(Body::from(r#"{"items":[{"t":"HitPost","u":"/c/2"}]}"#))
                        .unwrap()
                }
            }),
        );

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let server = tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });
    let base = format!("http://{addr}");

    let get_src: ComicSourceSpec = serde_json::from_value(serde_json::json!({
        "sourceName": "EncodeGet",
        "sourceUrl": base,
        "searchUrl": format!("{base}/search-get?q={{{{key}}}}"),
        "ruleSearch": { "bookList": "$.items[*]", "name": "$.t", "bookUrl": "$.u" }
    }))
    .unwrap();
    let post_src: ComicSourceSpec = serde_json::from_value(serde_json::json!({
        "sourceName": "JsonPost",
        "sourceUrl": base,
        "searchUrl": format!("{base}/search-post,{{\"method\":\"POST\",\"body\":{{\"kw\":\"{{{{key}}}}\"}}}}"),
        "ruleSearch": { "bookList": "$.items[*]", "name": "$.t", "bookUrl": "$.u" }
    }))
    .unwrap();

    let engine = ComicSourceEngine::new(
        HttpClientConfig {
            allow_private_ips: true,
            ..Default::default()
        },
        Duration::from_secs(10),
    );
    let (r_get, r_post) = tokio::task::spawn_blocking(move || {
        let a = engine
            .search_sync(&get_src, SourceRuntimeState::default(), "神之塔 & 100%", 1)
            .unwrap()
            .data;
        let b = engine
            .search_sync(&post_src, SourceRuntimeState::default(), "神之塔 & 100%", 1)
            .unwrap()
            .data;
        (a, b)
    })
    .await
    .unwrap();

    assert_eq!(r_get[0].title, "Hit");
    assert_eq!(r_post[0].title, "HitPost");
    assert_eq!(seen_get.lock().unwrap().as_slice(), &["神之塔 & 100%"]);
    let post_body: serde_json::Value = serde_json::from_str(&seen_post.lock().unwrap()[0]).unwrap();
    assert_eq!(post_body["kw"], "神之塔 & 100%");

    server.abort();
}

#[tokio::test]
async fn detail_literal_reading_direction_malformed_json_and_http_login_url_fallback() {
    use crate::domain::series::ReadingDirection;
    use axum::routing::post;

    let app = Router::new()
        .route(
            "/detail",
            get(|| async {
                Response::builder()
                    .status(StatusCode::OK)
                    .header("content-type", "application/json")
                    .body(Body::from(r#"{"title":"Webtoon Title"}"#))
                    .unwrap()
            }),
        )
        .route(
            "/bad-json",
            get(|| async {
                Response::builder()
                    .status(StatusCode::OK)
                    .header("content-type", "application/json")
                    .body(Body::from(r#"{ "broken": [1, 2,"#))
                    .unwrap()
            }),
        )
        .route(
            "/bad-502",
            get(|| async {
                Response::builder()
                    .status(StatusCode::BAD_GATEWAY)
                    .header("content-type", "application/json")
                    .body(Body::from(r#"{"error":"upstream_down"}"#))
                    .unwrap()
            }),
        )
        .route(
            "/auth/login",
            post(|| async {
                Response::builder()
                    .status(StatusCode::OK)
                    .header("set-cookie", "sess=ok42; Path=/")
                    .body(Body::from("ok"))
                    .unwrap()
            }),
        );

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let server = tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });
    let base = format!("http://{addr}");

    let detail_src: ComicSourceSpec = serde_json::from_value(serde_json::json!({
        "sourceName": "DirOverride",
        "sourceUrl": base,
        "readingDirection": "ltr",
        "ruleDetail": { "name": "$.title", "readingDirection": "webtoon" }
    }))
    .unwrap();
    let bad_json_src: ComicSourceSpec = serde_json::from_value(serde_json::json!({
        "sourceName": "BadJson",
        "sourceUrl": base,
        "searchUrl": format!("{base}/bad-json?q={{{{key}}}}"),
        "ruleSearch": { "bookList": "$.items[*]", "name": "$.t", "bookUrl": "$.u" }
    }))
    .unwrap();
    let bad_502_src: ComicSourceSpec = serde_json::from_value(serde_json::json!({
        "sourceName": "Bad502",
        "sourceUrl": base,
        "searchUrl": format!("{base}/bad-502?q={{{{key}}}}"),
        "ruleSearch": { "bookList": "$.items[*]", "name": "$.t", "bookUrl": "$.u" }
    }))
    .unwrap();
    let login_src: ComicSourceSpec = serde_json::from_value(serde_json::json!({
        "sourceName": "HttpLoginUrl",
        "sourceUrl": base,
        "loginUrl": format!("{base}/auth/login"),
        "loginCheckJs": format!("<js>java.post('{base}/auth/login', '{{}}', {{}}); 'checked';</js>"),
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
    let (detail_out, bad_err, bad_502_err, login_out) = tokio::task::spawn_blocking(move || {
        let d = engine
            .fetch_detail_sync(
                &detail_src,
                SourceRuntimeState::default(),
                &format!("{base_c}/detail"),
                None,
            )
            .unwrap()
            .data;
        let b = engine
            .search_sync(&bad_json_src, SourceRuntimeState::default(), "x", 1)
            .unwrap_err();
        let b502 = engine
            .search_sync(&bad_502_src, SourceRuntimeState::default(), "x", 1)
            .unwrap_err();
        let l = engine
            .run_login_action_sync(
                &login_src,
                SourceRuntimeState::default(),
                HashMap::new(),
                None,
            )
            .unwrap();
        (d, b, b502, l)
    })
    .await
    .unwrap();

    assert_eq!(
        detail_out.reading_direction,
        Some(ReadingDirection::Webtoon)
    );
    assert!(
        bad_err.contains("JSON"),
        "expected JSON error, got: {bad_err}"
    );
    assert!(
        bad_502_err.contains("502"),
        "expected HTTP 502 error, got: {bad_502_err}"
    );
    assert_eq!(login_out.data, "checked");
    assert!(
        login_out
            .state
            .cookies
            .values()
            .any(|v| v.contains("sess=ok42")),
        "expected loginCheckJs cookie to be persisted: {:?}",
        login_out.state.cookies
    );

    server.abort();
}

#[tokio::test]
async fn json_scalar_roots_return_empty_results_without_invalid_json_error() {
    let app = Router::new()
        .route(
            "/scalar-str",
            get(|| async {
                Response::builder()
                    .status(StatusCode::OK)
                    .header("content-type", "application/json")
                    .body(Body::from(r#""just-a-string""#))
                    .unwrap()
            }),
        )
        .route(
            "/scalar-num",
            get(|| async {
                Response::builder()
                    .status(StatusCode::OK)
                    .header("content-type", "application/json")
                    .body(Body::from("123"))
                    .unwrap()
            }),
        )
        .route(
            "/scalar-bool",
            get(|| async {
                Response::builder()
                    .status(StatusCode::OK)
                    .header("content-type", "application/json")
                    .body(Body::from("true"))
                    .unwrap()
            }),
        )
        .route(
            "/scalar-null",
            get(|| async {
                Response::builder()
                    .status(StatusCode::OK)
                    .header("content-type", "application/json")
                    .body(Body::from("null"))
                    .unwrap()
            }),
        )
        .route(
            "/broken",
            get(|| async {
                Response::builder()
                    .status(StatusCode::OK)
                    .header("content-type", "application/json")
                    .body(Body::from(r#"{ "broken": [1, 2,"#))
                    .unwrap()
            }),
        );

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let server = tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });
    let base = format!("http://{addr}");

    let make_src = |path: &str| -> ComicSourceSpec {
        serde_json::from_value(serde_json::json!({
            "sourceName": format!("Scalar-{path}"),
            "sourceUrl": base,
            "searchUrl": format!("{base}{path}?q={{{{key}}}}"),
            "ruleSearch": { "bookList": "$.items[*]", "name": "$.t", "bookUrl": "$.u" }
        }))
        .unwrap()
    };

    let s_str = make_src("/scalar-str");
    let s_num = make_src("/scalar-num");
    let s_bool = make_src("/scalar-bool");
    let s_null = make_src("/scalar-null");
    let s_broken = make_src("/broken");

    let engine = ComicSourceEngine::new(
        HttpClientConfig {
            allow_private_ips: true,
            ..Default::default()
        },
        Duration::from_secs(10),
    );

    let (r_str, r_num, r_bool, r_null, r_broken) = tokio::task::spawn_blocking(move || {
        (
            engine.search_sync(&s_str, SourceRuntimeState::default(), "q", 1),
            engine.search_sync(&s_num, SourceRuntimeState::default(), "q", 1),
            engine.search_sync(&s_bool, SourceRuntimeState::default(), "q", 1),
            engine.search_sync(&s_null, SourceRuntimeState::default(), "q", 1),
            engine.search_sync(&s_broken, SourceRuntimeState::default(), "q", 1),
        )
    })
    .await
    .unwrap();

    assert!(r_str.unwrap().data.is_empty());
    assert!(r_num.unwrap().data.is_empty());
    assert!(r_bool.unwrap().data.is_empty());
    assert!(r_null.unwrap().data.is_empty());
    let err = r_broken.unwrap_err();
    assert!(err.contains("上游返回无效 JSON"), "got: {err}");

    server.abort();
}

#[tokio::test]
async fn mixed_js_search_url_encodes_static_key_and_form_body_and_blocks_option_injection() {
    let seen_requests: Arc<std::sync::Mutex<Vec<(String, String)>>> =
        Arc::new(std::sync::Mutex::new(Vec::new()));
    let seen_c = seen_requests.clone();

    let app = Router::new().route(
        "/search",
        axum::routing::any(move |req: axum::extract::Request| {
            let seen = seen_c.clone();
            async move {
                let uri = req.uri().to_string();
                let method = req.method().to_string();
                let body_bytes = axum::body::to_bytes(req.into_body(), 64 * 1024)
                    .await
                    .unwrap_or_default();
                let body_str = String::from_utf8_lossy(&body_bytes).to_string();
                seen.lock()
                    .unwrap()
                    .push((format!("{method} {uri}"), body_str));
                Response::builder()
                    .status(StatusCode::OK)
                    .header("content-type", "application/json")
                    .body(Body::from(r#"{"items":[{"t":"Hit","u":"/c/1"}]}"#))
                    .unwrap()
            }
        }),
    );

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let server = tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });
    let base = format!("http://{addr}");

    let mixed_get: ComicSourceSpec = serde_json::from_value(serde_json::json!({
        "sourceName": "MixedGet",
        "sourceUrl": base,
        "searchUrl": format!("{base}/search?q={{{{key}}}}&sig={{{{1+1}}}}"),
        "ruleSearch": { "bookList": "$.items[*]", "name": "$.t", "bookUrl": "$.u" }
    }))
    .unwrap();

    let mixed_opts: ComicSourceSpec = serde_json::from_value(serde_json::json!({
        "sourceName": "MixedOpts",
        "sourceUrl": base,
        "searchUrl": format!("{base}/search?q={{{{key}}}}&sig={{{{1+1}}}},{{\"method\":\"POST\",\"body\":\"k={{{{key}}}}&s={{{{1+1}}}}\"}}"),
        "ruleSearch": { "bookList": "$.items[*]", "name": "$.t", "bookUrl": "$.u" }
    }))
    .unwrap();

    let engine = ComicSourceEngine::new(
        HttpClientConfig {
            allow_private_ips: true,
            ..Default::default()
        },
        Duration::from_secs(10),
    );

    let (res_get, res_lit_opts, res_post) = tokio::task::spawn_blocking(move || {
        (
            engine.search_sync(
                &mixed_get,
                SourceRuntimeState::default(),
                "a&admin=1#frag",
                1,
            ),
            engine.search_sync(
                &mixed_get,
                SourceRuntimeState::default(),
                r#"foo,{"method":"DELETE"}"#,
                1,
            ),
            engine.search_sync(&mixed_opts, SourceRuntimeState::default(), "x&evil=1", 1),
        )
    })
    .await
    .unwrap();

    assert!(res_get.is_ok(), "unexpected error: {:?}", res_get.err());
    assert!(
        res_lit_opts.is_ok(),
        "literal options-like keyword should be encoded cleanly: {:?}",
        res_lit_opts.err()
    );
    assert!(res_post.is_ok(), "unexpected error: {:?}", res_post.err());

    let logged = seen_requests.lock().unwrap();
    assert_eq!(logged.len(), 3);
    assert_eq!(logged[0].0, "GET /search?q=a%26admin%3D1%23frag&sig=2");
    assert!(
        logged[1]
            .0
            .starts_with("GET /search?q=foo%2C%7B%22method%22"),
        "unexpected uri: {}",
        logged[1].0
    );
    assert_eq!(logged[2].0, "POST /search?q=x%26evil%3D1&sig=2");
    assert_eq!(logged[2].1, "k=x%26evil%3D1&s=2");

    server.abort();
}

#[tokio::test]
async fn comic_js_pipeline_guard_blocks_comma_quote_semicolon_option_injection() {
    use std::sync::atomic::{AtomicUsize, Ordering};

    let post_hits = Arc::new(AtomicUsize::new(0));
    let post_hits_c = post_hits.clone();

    let app = Router::new().route(
        "/search",
        axum::routing::any(move |req: axum::extract::Request| {
            let hits = post_hits_c.clone();
            async move {
                if req.method() == axum::http::Method::POST {
                    hits.fetch_add(1, Ordering::SeqCst);
                }
                Response::builder()
                    .status(StatusCode::OK)
                    .header("content-type", "application/json")
                    .body(Body::from(r#"{"items":[{"t":"Ok","u":"/c/1"}]}"#))
                    .unwrap()
            }
        }),
    );

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let server = tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });
    let base = format!("http://{addr}");

    let at_js_comma_src: ComicSourceSpec = serde_json::from_value(serde_json::json!({
        "sourceName": "AtJsCommaInject",
        "sourceUrl": base,
        "searchUrl": format!("@js:'{base}/search?q=' + key + '{{\"method\":\"POST\",\"body\":\"pwn=1\"}}'"),
        "ruleSearch": { "bookList": "$.items[*]", "name": "$.t", "bookUrl": "$.u" }
    }))
    .unwrap();
    let tag_js_comma_src: ComicSourceSpec = serde_json::from_value(serde_json::json!({
        "sourceName": "TagJsCommaInject",
        "sourceUrl": base,
        "searchUrl": format!("<js>'{base}/search?q=' + key + '{{\"method\":\"POST\",\"body\":\"pwn=1\"}}'</js>"),
        "ruleSearch": { "bookList": "$.items[*]", "name": "$.t", "bookUrl": "$.u" }
    }))
    .unwrap();
    let at_js_quote_breakout_src: ComicSourceSpec = serde_json::from_value(serde_json::json!({
        "sourceName": "AtJsSingleQuoteInject",
        "sourceUrl": base,
        "searchUrl": format!("@js:JSON.stringify({{method:'POST',body:'q=' + key}}) === '' ? '' : '{base}/search,' + '{{\"method\":\"POST\",\"body\":\"' + key + '\"}}'"),
        "ruleSearch": { "bookList": "$.items[*]", "name": "$.t", "bookUrl": "$.u" }
    }))
    .unwrap();
    let safe_benign_punct_src: ComicSourceSpec = serde_json::from_value(serde_json::json!({
        "sourceName": "SafePunctuation",
        "sourceUrl": base,
        "searchUrl": format!("@js:'{base}/search?q=' + encodeURIComponent(key) + ',' + JSON.stringify({{method:'POST',body:JSON.stringify({{q:key}})}})"),
        "ruleSearch": { "bookList": "$.items[*]", "name": "$.t", "bookUrl": "$.u" }
    }))
    .unwrap();

    let engine = ComicSourceEngine::new(
        HttpClientConfig {
            allow_private_ips: true,
            ..Default::default()
        },
        Duration::from_secs(10),
    );

    tokio::task::spawn_blocking(move || {
        for src in [&at_js_comma_src, &tag_js_comma_src] {
            let err = engine
                .search_sync(src, SourceRuntimeState::default(), "hello,", 1)
                .expect_err("comma-only option injection must be rejected");
            assert!(
                err.contains("选项") || err.contains("结构"),
                "unexpected error: {err}"
            );
        }

        // Single quote and semicolon in keyword trigger probe check without crashing
        // and safe JSON.stringify construction succeeds.
        for kw in ["hello,", "O'Reilly;part2"] {
            let ok = engine
                .search_sync(&safe_benign_punct_src, SourceRuntimeState::default(), kw, 1)
                .unwrap()
                .data;
            assert_eq!(ok[0].title, "Ok");
        }

        // Unconditional restore check: even when key contains ' or ; and triggers probe,
        // non-injecting quote breakout check still rejects structural mutation cleanly.
        let mut_err = engine
            .search_sync(
                &at_js_quote_breakout_src,
                SourceRuntimeState::default(),
                r#"1","headers":{"X-Evil":"1"},"z":""#,
                1,
            )
            .expect_err("structural mutation with quotes must be rejected");
        assert!(mut_err.contains("结构"), "unexpected error: {mut_err}");
    })
    .await
    .unwrap();

    // Only the 2 safe_benign_punct_src POST requests should have hit the server.
    assert_eq!(post_hits.load(Ordering::SeqCst), 2);
    server.abort();
}

#[tokio::test]
async fn comic_split_tag_js_url_and_options_evaluates_both_and_blocks_injection() {
    use axum::body::to_bytes;
    use std::sync::Mutex;

    let captured: Arc<Mutex<Vec<(String, String)>>> = Arc::new(Mutex::new(Vec::new()));
    let cap_c = captured.clone();

    let app = Router::new().route(
        "/search",
        axum::routing::any(move |req: axum::extract::Request| {
            let cap = cap_c.clone();
            async move {
                let method = req.method().to_string();
                let uri = req.uri().to_string();
                let body_bytes = to_bytes(req.into_body(), 64 * 1024)
                    .await
                    .unwrap_or_default();
                let body = String::from_utf8_lossy(&body_bytes).to_string();
                cap.lock().unwrap().push((format!("{method} {uri}"), body));
                Response::builder()
                    .status(StatusCode::OK)
                    .header("content-type", "application/json")
                    .body(Body::from(r#"{"items":[{"t":"SplitHit","u":"/c/1"}]}"#))
                    .unwrap()
            }
        }),
    );

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let server = tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });
    let base = format!("http://{addr}");

    let split_ok_src: ComicSourceSpec = serde_json::from_value(serde_json::json!({
        "sourceName": "SplitTagJsComic",
        "sourceUrl": base,
        "searchUrl": "<js>'/search?q=' + encodeURIComponent(key)</js>,{\"method\":\"POST\",\"body\":\"k={{key}}\"}",
        "ruleSearch": { "bookList": "$.items[*]", "name": "$.t", "bookUrl": "$.u" }
    }))
    .unwrap();

    let split_unsafe_url_src: ComicSourceSpec = serde_json::from_value(serde_json::json!({
        "sourceName": "SplitUnsafeUrlComic",
        "sourceUrl": base,
        "searchUrl": "<js>'/search?q=' + key</js>,{\"method\":\"POST\",\"body\":\"k={{key}}\"}",
        "ruleSearch": { "bookList": "$.items[*]", "name": "$.t", "bookUrl": "$.u" }
    }))
    .unwrap();

    let split_js_opts_src: ComicSourceSpec = serde_json::from_value(serde_json::json!({
        "sourceName": "SplitJsOptsComic",
        "sourceUrl": base,
        "searchUrl": "<js>'/search?q=' + encodeURIComponent(key)</js>,{\"method\":\"POST\",\"body\":{\"kw\":\"{{String(key)}}\"}}",
        "ruleSearch": { "bookList": "$.items[*]", "name": "$.t", "bookUrl": "$.u" }
    }))
    .unwrap();

    let engine = ComicSourceEngine::new(
        HttpClientConfig {
            allow_private_ips: true,
            ..Default::default()
        },
        Duration::from_secs(10),
    );

    tokio::task::spawn_blocking(move || {
        let ok = engine
            .search_sync(
                &split_ok_src,
                SourceRuntimeState::default(),
                "hello world",
                1,
            )
            .unwrap()
            .data;
        assert_eq!(ok[0].title, "SplitHit");

        // Option injection in url_tpl side must be rejected even with trailing ,{options}.
        let evil = r#"hello,{"method":"DELETE","headers":{"X-Evil":"1"}}"#;
        let url_err = engine
            .search_sync(
                &split_unsafe_url_src,
                SourceRuntimeState::default(),
                evil,
                1,
            )
            .expect_err("option injection in <js> url_tpl side must be rejected");
        assert!(url_err.contains("选项"), "unexpected error: {url_err}");

        // Option breakout in options side with {{String(key)}} is JSON-escaped cleanly without mutating JSON structure.
        let breakout = r#"a","method":"DELETE"#;
        let ok2 = engine
            .search_sync(
                &split_js_opts_src,
                SourceRuntimeState::default(),
                breakout,
                1,
            )
            .unwrap()
            .data;
        assert_eq!(ok2[0].title, "SplitHit");
    })
    .await
    .unwrap();

    let reqs = captured.lock().unwrap();
    assert_eq!(reqs.len(), 2);
    assert_eq!(reqs[0].0, "POST /search?q=hello%20world");
    assert_eq!(reqs[0].1, "k=hello%20world");
    assert!(reqs[1].0.starts_with("POST /search?q=a%22"));
    let parsed_body: serde_json::Value = serde_json::from_str(&reqs[1].1).unwrap();
    assert_eq!(parsed_body["kw"], r#"a","method":"DELETE"#);
    server.abort();
}

#[test]
fn comic_run_login_action_strips_case_insensitive_js_wrappers_and_preserves_http_login_url() {
    let engine = ComicSourceEngine::default();
    let cases = [
        (
            "@JS: function login() { return 'upper-at:' + (result.user || ''); }",
            None,
            Some("login()"),
            "upper-at:alice",
        ),
        (
            "<js>function check() { return 'lower-tag:' + (result.user || ''); }</js>",
            Some("@JS: check()"),
            None,
            "lower-tag:alice",
        ),
        (
            "<JS>function check() { return 'upper-tag:' + (result.user || ''); }</JS>",
            Some("<JS>check()</JS>"),
            None,
            "upper-tag:alice",
        ),
        (
            "https://comic.example.test/login",
            Some("<JS>'http-check:' + (result.user || '')</JS>"),
            None,
            "http-check:alice",
        ),
    ];

    for (login_url, login_check_js, action, expected) in cases {
        let spec: ComicSourceSpec = serde_json::from_value(serde_json::json!({
            "sourceName": "LoginWrapperComic",
            "sourceUrl": "https://comic.example.test",
            "loginUrl": login_url,
            "loginCheckJs": login_check_js,
        }))
        .unwrap();
        let out = engine
            .run_login_action_sync(
                &spec,
                SourceRuntimeState::default(),
                HashMap::from([("user".to_string(), "alice".to_string())]),
                action,
            )
            .unwrap();
        assert_eq!(out.data, expected);
    }
}

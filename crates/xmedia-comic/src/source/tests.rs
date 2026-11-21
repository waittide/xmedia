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

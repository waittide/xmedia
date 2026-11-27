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

use crate::domain::online::{
    ComicChapterJsContext, ComicJsContext, ComicSourceSpec, SourceRuntimeState,
};
use crate::domain::series::ReadingDirection;
use crate::infra::source::ComicSourceEngine;
use crate::infra::source::session::extract_img_srcs_from_html;
use xmedia_source::crud::{SourceRecord, SourceSpecAdapter};

#[test]
fn parse_comic_source_spec_uses_canonical_keys_and_object_headers() {
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
        "ruleToc": {"chapterList": "li.ep", "name": "a@text", "url": "a@href"},
        "rulePages": {
          "content": "#_imageList img@data-url",
          "imageHeader": "{\"X-Img-Token\":\"abc\"}"
        }
      }
    ]"##;

    let batch =
        <ComicSourceSpec as xmedia_source::crud::SourceSpecAdapter>::parse_batch(raw).unwrap();
    assert_eq!(batch.len(), 1);

    let (native, _) = &batch[0];
    assert_eq!(native.source_name, "Comic Native");
    assert_eq!(native.source_type, 2);
    assert_eq!(
        native.effective_reading_direction(),
        ReadingDirection::Webtoon
    );
    let native_headers = native.image_headers(None);
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
}

#[test]
fn parse_comic_source_spec_rejects_legacy_field_names() {
    let raw = r#"{"bookSourceName":"Legacy","bookSourceUrl":"https://legacy.example.com"}"#;
    assert!(ComicSourceSpec::parse_batch(raw).is_err());
}

#[test]
fn seeds_comic_json_parses_cleanly_with_all_five_verified_sources() {
    let raw = include_str!("../../../../testdata/online_sources.json");
    let batch = <ComicSourceSpec as xmedia_source::crud::SourceSpecAdapter>::parse_batch(raw)
        .expect("testdata/online_sources.json must parse");
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
            .image_headers(None)
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
        let h = batch[idx].0.image_headers(None);
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
fn sync_raw_json_fields_updates_comic_group_key() {
    let comic_raw = r#"{"sourceName":"B","sourceUrl":"https://b","sourceGroup":"Old"}"#;
    let updated_comic = SourceRecord::sync_raw_json_fields(
        comic_raw,
        Some(false),
        Some(50),
        Some("Manga"),
        ComicSourceSpec::JSON_GROUP_KEY,
    );
    let v2: serde_json::Value = serde_json::from_str(&updated_comic).unwrap();
    assert_eq!(v2["sourceGroup"], "Manga");
    assert_eq!(v2["enabled"], false);
    assert_eq!(v2["weight"], 50);
}

#[test]
fn parse_reading_direction_and_html_img_extraction() {
    assert_eq!(
        "RTL".parse::<ReadingDirection>().ok(),
        Some(ReadingDirection::Rtl)
    );
    assert_eq!(
        "ltr".parse::<ReadingDirection>().ok(),
        Some(ReadingDirection::Ltr)
    );
    assert_eq!(
        "webtoon".parse::<ReadingDirection>().ok(),
        Some(ReadingDirection::Webtoon)
    );
    assert_eq!("unknown".parse::<ReadingDirection>().ok(), None);

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
        "rulePages": {
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
fn sync_raw_json_fields_updates_source_group_after_clear_and_restore() {
    let raw = r#"{"sourceName":"BugRepro","sourceUrl":"https://ex.test","sourceGroup":"Original"}"#;
    let (_, normalized) = ComicSourceSpec::parse_batch(raw).unwrap().remove(0);
    let cleared = SourceRecord::sync_raw_json_fields(
        &normalized,
        None,
        None,
        Some(""),
        ComicSourceSpec::JSON_GROUP_KEY,
    );
    let v_cleared: serde_json::Value = serde_json::from_str(&cleared).unwrap();
    assert!(v_cleared.get("sourceGroup").is_none());

    let restored = SourceRecord::sync_raw_json_fields(
        &cleared,
        None,
        None,
        Some("Restored"),
        ComicSourceSpec::JSON_GROUP_KEY,
    );
    let v_restored: serde_json::Value = serde_json::from_str(&restored).unwrap();
    assert_eq!(v_restored["sourceGroup"], "Restored");
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
        "rulePages": { "content": "#imgs@html" }
    }))
    .unwrap();
    let multiline_src: ComicSourceSpec = serde_json::from_value(serde_json::json!({
        "sourceName": "MultilineFrag",
        "sourceUrl": base,
        "rulePages": { "content": "@js:result" }
    }))
    .unwrap();
    let json_rule_src: ComicSourceSpec = serde_json::from_value(serde_json::json!({
        "sourceName": "JsonStrRule",
        "sourceUrl": base,
        "rulePages": {
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
        "rulePages": {
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
        "rulePages": { "content": "@js:result" }
    }))
    .unwrap();
    let mixed_rule_src: ComicSourceSpec = serde_json::from_value(serde_json::json!({
        "sourceName": "MixedRule",
        "sourceUrl": base,
        "rulePages": {
            "content": "@js:result",
            "imageUrl": "@js:result + '?tok=1'"
        }
    }))
    .unwrap();
    let wrapper_rule_src: ComicSourceSpec = serde_json::from_value(serde_json::json!({
        "sourceName": "WrapperRule",
        "sourceUrl": base,
        "rulePages": {
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
        "rulePages": {
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

#![allow(unused_imports)]
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

#[tokio::test]
async fn comic_search_allows_non_json_braces_and_blocks_json_options_smuggling() {
    use axum::{Router, routing::get};

    let app = Router::new().route(
        "/search",
        get(
            |axum::extract::Query(q): axum::extract::Query<HashMap<String, String>>| async move {
                let kw = q.get("q").cloned().unwrap_or_default();
                serde_json::json!({
                    "items": [{ "title": format!("Hit:{kw}"), "url": "/comic/1" }]
                })
                .to_string()
            },
        ),
    );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let base = format!("http://{addr}");
    let srv = tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });

    let base_c = base.clone();
    tokio::task::spawn_blocking(move || {
        let engine = ComicSourceEngine::new(
            HttpClientConfig {
                allow_private_ips: true,
                ..Default::default()
            },
            Duration::from_secs(5),
        );
        let spec: ComicSourceSpec = serde_json::from_value(serde_json::json!({
            "sourceName": "NonJsonBracesComic",
            "sourceUrl": base_c,
            "searchUrl": format!("{base_c}/search?q={{{{key}}}}"),
            "ruleSearch": {
                "bookList": "$.items[*]",
                "name": "$.title",
                "bookUrl": "$.url"
            }
        }))
        .unwrap();

        let ok = engine
            .search_sync(&spec, SourceRuntimeState::default(), "foo,{bar}", 1)
            .expect("non-JSON ,{bar} search keyword must succeed");
        assert_eq!(ok.data.len(), 1);
        assert_eq!(ok.data[0].title, "Hit:foo,{bar}");

        let err = engine
            .search_sync(
                &spec,
                SourceRuntimeState::default(),
                r#"foo,{"method":"POST"}"#,
                1,
            )
            .expect_err("valid JSON option smuggling must still be rejected");
        assert!(err.contains("请求选项"), "unexpected error: {err}");
    })
    .await
    .unwrap();

    srv.abort();
}

#[test]
fn merged_image_headers_overrides_json_accept_unless_image_header_specifies_accept() {
    let base_only: ComicSourceSpec = serde_json::from_value(serde_json::json!({
        "sourceName": "MdLike",
        "sourceUrl": "https://api.mangadex.org",
        "header": r#"{"User-Agent":"xmedia/0.1.0","accept":"application/json"}"#,
        "imageHeader": r#"{"User-Agent":"xmedia/0.1.0"}"#
    }))
    .unwrap();
    let h1 = base_only.image_headers(None);
    assert_eq!(
        h1.get("Accept").map(String::as_str),
        Some("image/avif,image/webp,image/apng,image/*,*/*;q=0.8")
    );
    assert!(!h1.contains_key("accept"));

    let with_img_accept: ComicSourceSpec = serde_json::from_value(serde_json::json!({
        "sourceName": "ExplicitAccept",
        "sourceUrl": "https://api.mangadex.org",
        "header": r#"{"Accept":"application/json"}"#,
        "imageHeader": r#"{"Accept":"image/webp"}"#
    }))
    .unwrap();
    assert_eq!(
        with_img_accept
            .image_headers(None)
            .get("Accept")
            .map(String::as_str),
        Some("image/webp")
    );

    let with_pages_accept: ComicSourceSpec = serde_json::from_value(serde_json::json!({
        "sourceName": "PagesAccept",
        "sourceUrl": "https://api.mangadex.org",
        "header": r#"{"Accept":"application/json"}"#,
        "ruleContent": { "imageHeader": r#"{"accept":"image/png"}"# }
    }))
    .unwrap();
    assert_eq!(
        with_pages_accept
            .image_headers(None)
            .get("accept")
            .map(String::as_str),
        Some("image/png")
    );
}

#[tokio::test]
async fn comic_session_http_fetch_retries_at_exact_8s_and_skips_below_8s() {
    use std::sync::atomic::{AtomicUsize, Ordering};

    let hits_8s = Arc::new(AtomicUsize::new(0));
    let hits_7s = Arc::new(AtomicUsize::new(0));
    let h8 = hits_8s.clone();
    let h7 = hits_7s.clone();

    let app = Router::new()
        .route(
            "/search-8s",
            get(move || {
                let h = h8.clone();
                async move {
                    let n = h.fetch_add(1, Ordering::SeqCst) + 1;
                    if n == 1 {
                        Response::builder()
                            .status(StatusCode::SERVICE_UNAVAILABLE)
                            .body(Body::from("busy"))
                            .unwrap()
                    } else {
                        Response::builder()
                            .status(StatusCode::OK)
                            .header("content-type", "application/json")
                            .body(Body::from(r#"{"items":[{"t":"Ok8s","u":"/c/8"}]}"#))
                            .unwrap()
                    }
                }
            }),
        )
        .route(
            "/search-7s",
            get(move || {
                let h = h7.clone();
                async move {
                    let n = h.fetch_add(1, Ordering::SeqCst) + 1;
                    if n == 1 {
                        Response::builder()
                            .status(StatusCode::SERVICE_UNAVAILABLE)
                            .body(Body::from("busy"))
                            .unwrap()
                    } else {
                        Response::builder()
                            .status(StatusCode::OK)
                            .header("content-type", "application/json")
                            .body(Body::from(r#"{"items":[{"t":"Unexpected","u":"/c/7"}]}"#))
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

    let src_8s: ComicSourceSpec = serde_json::from_value(serde_json::json!({
        "sourceName": "Boundary8s",
        "sourceUrl": base,
        "searchUrl": format!("{base}/search-8s?q={{{{key}}}}"),
        "ruleSearch": { "bookList": "$.items[*]", "name": "$.t", "bookUrl": "$.u" }
    }))
    .unwrap();
    let src_7s: ComicSourceSpec = serde_json::from_value(serde_json::json!({
        "sourceName": "Boundary7s",
        "sourceUrl": base,
        "searchUrl": format!("{base}/search-7s?q={{{{key}}}}"),
        "ruleSearch": { "bookList": "$.items[*]", "name": "$.t", "bookUrl": "$.u" }
    }))
    .unwrap();

    let (res_8s, res_7s) = tokio::task::spawn_blocking(move || {
        let engine_8s = ComicSourceEngine::new(
            HttpClientConfig {
                allow_private_ips: true,
                ..Default::default()
            },
            Duration::from_secs(8),
        );
        let engine_7s = ComicSourceEngine::new(
            HttpClientConfig {
                allow_private_ips: true,
                ..Default::default()
            },
            Duration::from_secs(7),
        );
        let ok = engine_8s
            .search_sync(&src_8s, SourceRuntimeState::default(), "q", 1)
            .unwrap()
            .data;
        let err = engine_7s
            .search_sync(&src_7s, SourceRuntimeState::default(), "q", 1)
            .unwrap_err();
        (ok, err)
    })
    .await
    .unwrap();

    assert_eq!(res_8s[0].title, "Ok8s");
    assert!(res_7s.contains("503"), "expected 503 error, got: {res_7s}");
    assert_eq!(hits_8s.load(Ordering::SeqCst), 2);
    assert_eq!(hits_7s.load(Ordering::SeqCst), 1);

    server.abort();
}

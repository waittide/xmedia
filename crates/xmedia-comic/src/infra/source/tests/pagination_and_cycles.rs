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

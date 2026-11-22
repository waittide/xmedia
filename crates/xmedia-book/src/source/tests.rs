use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::time::{Duration, Instant};

use axum::Router;
use axum::routing::{get, post};

use super::*;
use crate::source::http::{
    HttpClientConfig, parse_legado_url, try_resolve_data_url, validate_target_url,
};
use crate::source::js_runtime::{
    MAX_JS_HTTP_REQUESTS, MAX_PERSISTENT_CACHE_BYTES, MAX_VARIABLE_BYTES,
};
use crate::source::model::{BookJsContext, ChapterJsContext, LegadoBookSource, SourceRuntimeState};

#[test]
fn data_url_with_type_option_encodes_bytes_as_hex_matching_legado() {
    let raw = r#"data:;base64,eyJrZXkiOiLmtYvor5UiLCJwYWdlIjoxfQ==,{"type":"gysearch"}"#;
    let parsed = parse_legado_url(raw);
    assert_eq!(parsed.options.content_type.as_deref(), Some("gysearch"));
    let res = try_resolve_data_url(&parsed).unwrap().unwrap();
    let decoded = String::from_utf8(hex::decode(&res.body).unwrap()).unwrap();
    assert_eq!(decoded, r#"{"key":"测试","page":1}"#);
}

#[test]
fn ssrf_guard_blocks_loopback_and_private_ips() {
    assert!(validate_target_url("http://127.0.0.1:8080/api", false).is_err());
    assert!(validate_target_url("http://localhost/admin", false).is_err());
    assert!(validate_target_url("http://169.254.169.254/latest/meta-data", false).is_err());
    assert!(validate_target_url("http://10.0.0.1/secret", false).is_err());
    assert!(validate_target_url("http://192.168.1.1/", false).is_err());
    assert!(validate_target_url("file:///etc/passwd", false).is_err());
    // Public IP literal allowed
    assert!(validate_target_url("https://1.1.1.1/", false).is_ok());
}

#[test]
fn js_sandbox_interrupts_infinite_loop_within_timeout() {
    let src = LegadoBookSource {
        book_source_name: "LoopTest".into(),
        book_source_url: "https://example.com".into(),
        search_url: Some("<js>while(true){}</js>".into()),
        rule_search: Some(Default::default()),
        ..Default::default()
    };
    let engine = BookSourceEngine::new(
        HttpClientConfig {
            allow_private_ips: true,
            ..Default::default()
        },
        Duration::from_millis(250),
    );
    let err = engine
        .search_sync(&src, SourceRuntimeState::default(), "test", 1)
        .unwrap_err();
    assert!(
        err.contains("interrupted") || err.contains("失败"),
        "unexpected error: {err}"
    );

    let mustache_src = LegadoBookSource {
        book_source_name: "MustacheLoopTest".into(),
        book_source_url: "https://example.com".into(),
        search_url: Some("https://example.com/{{ while(true){} }}/search".into()),
        rule_search: Some(Default::default()),
        ..Default::default()
    };
    let mustache_err = engine
        .search_sync(&mustache_src, SourceRuntimeState::default(), "test", 1)
        .unwrap_err();
    assert!(
        mustache_err.contains("interrupted") || mustache_err.contains("失败"),
        "mustache loop must fail with sandbox error, got: {mustache_err}"
    );
}

#[tokio::test]
async fn end_to_end_classic_html_css_legado_source() {
    let app = Router::new()
        .route(
            "/search",
            get(|| async {
                r#"<html><body>
                  <div class="result-list">
                    <div class="book-item">
                      <a class="title" href="/book/101">诡秘之主（精校版）</a>
                      <span class="author">爱潜水的乌贼</span>
                      <img class="cover" src="/img/101.jpg"/>
                      <p class="desc">蒸汽与机械的浪潮中</p>
                    </div>
                  </div>
                </body></html>"#
            }),
        )
        .route(
            "/book/101",
            get(|| async {
                r#"<html><body>
                  <h1 id="name">诡秘之主</h1>
                  <div class="meta"><span class="writer">爱潜水的乌贼</span></div>
                  <div id="intro">谁能触及非凡？</div>
                  <a id="toc-link" href="/book/101/toc">目录</a>
                </body></html>"#
            }),
        )
        .route(
            "/book/101/toc",
            get(|| async {
                r#"<html><body>
                  <ul id="chapters">
                    <li><a href="/book/101/ch1">第一章 绯红</a></li>
                    <li><a href="/book/101/ch2">第二章 情况</a></li>
                  </ul>
                </body></html>"#
            }),
        )
        .route(
            "/book/101/ch1",
            get(|| async {
                r#"<html><body>
                  <h2 class="ch-title">第一章 绯红</h2>
                  <div id="content"><p>痛！好痛！</p><p>头好痛！</p></div>
                </body></html>"#
            }),
        );

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let server_handle = tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });

    let base = format!("http://{addr}");
    let raw_json = serde_json::json!({
        "bookSourceName": "经典HTML书源",
        "bookSourceUrl": base,
        "searchUrl": "/search?q={{key}}&p={{page}}",
        "ruleSearch": {
            "bookList": "class.book-item",
            "name": "class.title@text##（精校版）",
            "author": "class.author@text",
            "bookUrl": "class.title@href",
            "coverUrl": "class.cover@src",
            "intro": "class.desc@text"
        },
        "ruleBookInfo": {
            "name": "id.name@text",
            "author": "class.writer@text",
            "intro": "id.intro@text",
            "tocUrl": "id.toc-link@href"
        },
        "ruleToc": {
            "chapterList": "#chapters li a",
            "chapterName": "text",
            "chapterUrl": "href"
        },
        "ruleContent": {
            "title": "class.ch-title@text",
            "content": "id.content@html"
        }
    })
    .to_string();

    let (src, _) = LegadoBookSource::parse_json_batch(&raw_json)
        .unwrap()
        .pop()
        .unwrap();
    let engine = BookSourceEngine::new(
        HttpClientConfig {
            allow_private_ips: true,
            ..Default::default()
        },
        Duration::from_secs(10),
    );

    let search_out = tokio::task::spawn_blocking({
        let engine = engine.clone();
        let src = src.clone();
        move || engine.search_sync(&src, SourceRuntimeState::default(), "诡秘", 1)
    })
    .await
    .unwrap()
    .unwrap();

    assert_eq!(search_out.data.len(), 1);
    assert_eq!(search_out.data[0].name, "诡秘之主");
    assert_eq!(search_out.data[0].author, "爱潜水的乌贼");
    assert!(search_out.data[0].book_url.ends_with("/book/101"));

    let book_url = search_out.data[0].book_url.clone();
    let detail_out = tokio::task::spawn_blocking({
        let engine = engine.clone();
        let src = src.clone();
        let state = search_out.state.clone();
        move || engine.fetch_book_info_sync(&src, state, &book_url, None)
    })
    .await
    .unwrap()
    .unwrap();

    assert_eq!(detail_out.data.name, "诡秘之主");
    assert!(detail_out.data.toc_url.ends_with("/book/101/toc"));

    let toc_url = detail_out.data.toc_url.clone();
    let toc_out = tokio::task::spawn_blocking({
        let engine = engine.clone();
        let src = src.clone();
        let state = detail_out.state.clone();
        let book_ctx = BookJsContext {
            name: detail_out.data.name.clone(),
            author: detail_out.data.author.clone(),
            ..Default::default()
        };
        move || engine.fetch_toc_sync(&src, state, &toc_url, &book_ctx)
    })
    .await
    .unwrap()
    .unwrap();

    assert_eq!(toc_out.data.len(), 2);
    assert_eq!(toc_out.data[0].title, "第一章 绯红");

    let ch0 = toc_out.data[0].clone();
    let content_out = tokio::task::spawn_blocking({
        let engine = engine.clone();
        let src = src.clone();
        let state = toc_out.state.clone();
        move || {
            engine.fetch_chapter_content_sync(
                &src,
                state,
                &ch0.url,
                &BookJsContext::default(),
                &ChapterJsContext {
                    index: ch0.index,
                    title: ch0.title.clone(),
                    url: ch0.url.clone(),
                },
            )
        }
    })
    .await
    .unwrap()
    .unwrap();

    assert_eq!(content_out.data.title, "第一章 绯红");
    assert_eq!(content_out.data.content, "痛！好痛！\n\n头好痛！");

    server_handle.abort();
}

#[tokio::test]
async fn end_to_end_jslib_data_url_json_aggregator_source() {
    let app = Router::new()
        .route(
            "/search",
            get(|| async {
                serde_json::json!({
                    "code": 0,
                    "data": [
                        {
                            "book_id": "bk99",
                            "book_name": "凡人修仙传（别名：凡人）",
                            "author": "忘语",
                            "source": "番茄",
                            "tab": "小说",
                            "thumb_url": "https://img.example.com/99.jpg",
                            "abstract": "一个普通山村穷小子",
                            "status": "完结",
                            "score": "9.6",
                            "tags": "仙侠",
                            "last_chapter_title": "大结局",
                            "word_number": "770万"
                        }
                    ]
                })
                .to_string()
            }),
        )
        .route(
            "/detail",
            post(|| async {
                serde_json::json!({
                    "code": 0,
                    "data": {
                        "book_id": "bk99",
                        "book_name": "凡人修仙传",
                        "author": "忘语",
                        "source": "番茄",
                        "tab": "小说",
                        "abstract": "山村小子踏入修仙路"
                    }
                })
                .to_string()
            }),
        )
        .route(
            "/catalog",
            post(|| async {
                serde_json::json!({
                    "code": 0,
                    "data": [
                        { "item_id": "c1", "title": "第一章 山边小村", "source": "番茄", "tab": "小说" },
                        { "item_id": "c2", "title": "第二章 青牛镇", "source": "番茄", "tab": "小说" }
                    ]
                })
                .to_string()
            }),
        )
        .route(
            "/content",
            post(|| async {
                serde_json::json!({
                    "code": 0,
                    "content": "<p>二愣子睁大着双眼。</p><p>直直望着茅草屋顶。</p><img src=\"data:image/svg+xml;base64,PHN2Zz48L3N2Zz4=\">"
                })
                .to_string()
            }),
        );

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let server_handle = tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });

    let api_base = format!("http://{addr}");
    let raw_json = serde_json::json!({
        "bookSourceName": "模拟聚合JS源",
        "bookSourceUrl": "聚合源",
        "jsLib": format!(r#"
            function BaseUrl() {{ return "{api_base}"; }}
            function req(path, method, body) {{
                var opts = {{ method: method || 'GET', headers: {{ 'Content-Type': 'application/json' }}, body: body ? JSON.stringify(body) : '' }};
                return this.java.ajax(this.BaseUrl() + path + ',' + JSON.stringify(opts));
            }}
        "#),
        "searchUrl": r#"<js>
            let payload = java.base64Encode(JSON.stringify({ key: key, page: page }));
            `data:;base64,${payload},{"type":"gysearch"}`;
        </js>"#,
        "ruleSearch": {
            "bookList": "<js>\nconst r = JSON.parse(java.hexDecodeToString(result));\nreq(`/search?q=${r.key}&p=${r.page}`);\n</js>\n$.data",
            "name": "$.book_name##（别名：.*?）",
            "author": "$.author",
            "bookUrl": "<js>\nlet p = java.base64Encode(JSON.stringify({ book_id: result.book_id, source: result.source }));\n`data:;base64,${p},{\"type\":\"gydetail\"}`;\n</js>",
            "coverUrl": "$.thumb_url",
            "intro": "$.abstract",
            "kind": "{{$.status}},{{$.score}},{{$.tags}}",
            "lastChapter": "{{$.source}} {{$.last_chapter_title}}",
            "wordCount": "$.word_number"
        },
        "ruleBookInfo": {
            "init": "<js>\nconst r = JSON.parse(java.hexDecodeToString(result));\nreq('/detail', 'POST', r);\n</js>$.data",
            "name": "$.book_name",
            "author": "$.author",
            "intro": "$.abstract",
            "tocUrl": "<js>\nlet p = java.base64Encode(JSON.stringify({ book_id: result.book_id }));\n`data:;base64,${p},{\"type\":\"gycatalog\"}`;\n</js>"
        },
        "ruleToc": {
            "chapterList": "<js>\nconst r = JSON.parse(java.hexDecodeToString(result));\njava.put('book_id', r.book_id);\nreq('/catalog', 'POST', r);\n</js>$.data",
            "chapterName": "$.title",
            "chapterUrl": "<js>\nlet p = java.base64Encode(JSON.stringify({ book_id: java.get('book_id'), item_id: result.item_id, title: result.title, sources: result.source }));\n`data:;base64,${p},{\"type\":\"gycontent\"}`;\n</js>"
        },
        "ruleContent": {
            "title": "@js:\nresult = String(java.hexDecodeToString(result));\nJSON.parse(result)['title']",
            "content": "<js>\nconst r = JSON.parse(java.hexDecodeToString(result));\nreq('/content', 'POST', r);\n</js>$.content"
        }
    })
    .to_string();

    let (src, _) = LegadoBookSource::parse_json_batch(&raw_json)
        .unwrap()
        .pop()
        .unwrap();
    let engine = BookSourceEngine::new(
        HttpClientConfig {
            allow_private_ips: true,
            ..Default::default()
        },
        Duration::from_secs(10),
    );

    let search_out = tokio::task::spawn_blocking({
        let engine = engine.clone();
        let src = src.clone();
        move || engine.search_sync(&src, SourceRuntimeState::default(), "凡人", 1)
    })
    .await
    .unwrap()
    .unwrap();

    assert_eq!(search_out.data.len(), 1);
    assert_eq!(search_out.data[0].name, "凡人修仙传");
    assert_eq!(search_out.data[0].kind.as_deref(), Some("完结,9.6,仙侠"));
    assert_eq!(
        search_out.data[0].last_chapter.as_deref(),
        Some("番茄 大结局")
    );

    let book_url = search_out.data[0].book_url.clone();
    let detail_out = tokio::task::spawn_blocking({
        let engine = engine.clone();
        let src = src.clone();
        let state = search_out.state.clone();
        move || engine.fetch_book_info_sync(&src, state, &book_url, None)
    })
    .await
    .unwrap()
    .unwrap();
    assert_eq!(detail_out.data.name, "凡人修仙传");

    let toc_url = detail_out.data.toc_url.clone();
    let toc_out = tokio::task::spawn_blocking({
        let engine = engine.clone();
        let src = src.clone();
        let state = detail_out.state.clone();
        move || engine.fetch_toc_sync(&src, state, &toc_url, &BookJsContext::default())
    })
    .await
    .unwrap()
    .unwrap();
    assert_eq!(toc_out.data.len(), 2);
    assert_eq!(toc_out.data[0].title, "第一章 山边小村");

    let ch0 = toc_out.data[0].clone();
    let content_out = tokio::task::spawn_blocking({
        let engine = engine.clone();
        let src = src.clone();
        let state = toc_out.state.clone();
        move || {
            engine.fetch_chapter_content_sync(
                &src,
                state,
                &ch0.url,
                &BookJsContext::default(),
                &ChapterJsContext {
                    index: 0,
                    title: ch0.title.clone(),
                    url: ch0.url.clone(),
                },
            )
        }
    })
    .await
    .unwrap()
    .unwrap();

    assert_eq!(content_out.data.title, "第一章 山边小村");
    assert_eq!(
        content_out.data.content,
        "二愣子睁大着双眼。\n\n直直望着茅草屋顶。"
    );

    server_handle.abort();
}

#[test]
fn js_bridge_enforces_container_quotas_and_preserves_diagnostics() {
    let src = LegadoBookSource {
        book_source_name: "QuotaTest".into(),
        book_source_url: "https://example.com".into(),
        login_url: Some(
            r#"
            function testQuotas() {
                var bigVar = 'v'.repeat(300 * 1024);
                source.setVariable(bigVar);
                var chunk = 'c'.repeat(200 * 1024);
                for (var i = 0; i < 10; i++) {
                    cache.put('k' + i, chunk);
                }
                var tempBig = 't'.repeat(5 * 1024 * 1024);
                java.put('huge', tempBig);
                return java.get('huge').length + ':' + source.getVariable().length;
            }
            "#
            .into(),
        ),
        ..Default::default()
    };
    let engine = BookSourceEngine::new(
        HttpClientConfig {
            allow_private_ips: true,
            ..Default::default()
        },
        Duration::from_secs(5),
    );
    let out = engine
        .run_login_action_sync(
            &src,
            SourceRuntimeState::default(),
            HashMap::new(),
            Some("testQuotas()"),
        )
        .unwrap();
    assert_eq!(out.data, "0:0");
    assert!(out.state.variable.len() <= MAX_VARIABLE_BYTES);
    let cache_bytes: usize = out.state.cache.iter().map(|(k, v)| k.len() + v.len()).sum();
    assert!(
        cache_bytes <= MAX_PERSISTENT_CACHE_BYTES,
        "persistent cache exceeded quota: {cache_bytes}"
    );
    assert!(
        out.diagnostics
            .logs
            .iter()
            .any(|l| l.contains("配额") || l.contains("上限")),
        "expected quota rejection logs, got {:?}",
        out.diagnostics.logs
    );
}

#[test]
fn js_sandbox_external_cancel_flag_interrupts_promptly_and_preserves_diagnostics() {
    let src = LegadoBookSource {
        book_source_name: "CancelTest".into(),
        book_source_url: "https://example.com".into(),
        search_url: Some("<js>java.log('before-loop'); while(true){}</js>".into()),
        rule_search: Some(Default::default()),
        ..Default::default()
    };
    let engine = BookSourceEngine::new(
        HttpClientConfig {
            allow_private_ips: true,
            ..Default::default()
        },
        Duration::from_secs(30),
    );
    let cancel = Arc::new(AtomicBool::new(false));
    let cancel_bg = cancel.clone();
    std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(80));
        cancel_bg.store(true, Ordering::Relaxed);
    });
    let started = Instant::now();
    let err = engine
        .search_sync_with_control(
            &src,
            SourceRuntimeState::default(),
            "test",
            1,
            Duration::from_secs(30),
            cancel,
        )
        .unwrap_err();
    assert!(
        started.elapsed() < Duration::from_secs(2),
        "expected prompt cancellation, took {:?}",
        started.elapsed()
    );
    assert!(
        err.diagnostics.logs.iter().any(|l| l == "before-loop"),
        "expected diagnostics preserved on error: {:?}",
        err.diagnostics
    );
}

#[tokio::test]
async fn js_sandbox_caps_http_requests_per_operation() {
    let hits = Arc::new(AtomicUsize::new(0));
    let hits_c = hits.clone();
    let app = Router::new().route(
        "/ping",
        get(move || {
            let hits = hits_c.clone();
            async move {
                hits.fetch_add(1, Ordering::SeqCst);
                "ok"
            }
        }),
    );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let server = tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });

    let src = LegadoBookSource {
        book_source_name: "HttpCapTest".into(),
        book_source_url: format!("http://{addr}"),
        login_url: Some(format!(
            r#"
            function spam() {{
                var okCount = 0;
                for (var i = 0; i < 40; i++) {{
                    if (java.ajax("http://{addr}/ping") === "ok") okCount++;
                }}
                return String(okCount);
            }}
            "#
        )),
        ..Default::default()
    };
    let engine = BookSourceEngine::new(
        HttpClientConfig {
            allow_private_ips: true,
            ..Default::default()
        },
        Duration::from_secs(10),
    );
    let err = tokio::task::spawn_blocking(move || {
        engine.run_login_action_sync(
            &src,
            SourceRuntimeState::default(),
            HashMap::new(),
            Some("spam()"),
        )
    })
    .await
    .unwrap()
    .expect_err("HTTP flood above MAX_JS_HTTP_REQUESTS must throw JS exception");

    assert!(
        err.contains("上限"),
        "expected HTTP cap error message, got: {err}"
    );
    assert_eq!(hits.load(Ordering::SeqCst), MAX_JS_HTTP_REQUESTS);
    server.abort();
}

#[test]
fn run_login_action_merges_login_info_without_wiping_existing_fields() {
    let src = LegadoBookSource {
        book_source_name: "LoginMergeTest".into(),
        book_source_url: "https://example.com".into(),
        login_url: Some(
            "function check(){ return JSON.stringify(source.getLoginInfoMap()); }".into(),
        ),
        ..Default::default()
    };
    let engine = BookSourceEngine::default();
    let mut initial = SourceRuntimeState::default();
    initial.login_info.insert("username".into(), "alice".into());
    initial
        .login_info
        .insert("password".into(), "s3cr3t".into());

    let out = engine
        .run_login_action_sync(
            &src,
            initial,
            HashMap::from([("region".into(), "cn".into())]),
            Some("check()"),
        )
        .unwrap();

    assert_eq!(
        out.state.login_info.get("username").map(String::as_str),
        Some("alice")
    );
    assert_eq!(
        out.state.login_info.get("password").map(String::as_str),
        Some("s3cr3t")
    );
    assert_eq!(
        out.state.login_info.get("region").map(String::as_str),
        Some("cn")
    );
    assert!(out.data.contains("alice") && out.data.contains("s3cr3t") && out.data.contains("cn"));
}

#[test]
fn search_url_template_encodes_keyword_and_blocks_legado_options_json_injection() {
    use std::io::{Read, Write};
    use std::net::TcpListener;
    use std::sync::{Arc, Mutex};

    #[derive(Default)]
    struct Captured {
        targets: Vec<String>,
        bodies: Vec<String>,
        has_injected_header: bool,
    }

    let captured = Arc::new(Mutex::new(Captured::default()));
    let cap_clone = captured.clone();
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();

    let server = std::thread::spawn(move || {
        for _ in 0..2 {
            let (mut stream, _) = listener.accept().unwrap();
            let mut buf = [0u8; 8192];
            let n = stream.read(&mut buf).unwrap();
            let req_str = String::from_utf8_lossy(&buf[..n]).to_string();
            let (head, body) = req_str.split_once("\r\n\r\n").unwrap_or((&req_str, ""));
            let first_line = head.lines().next().unwrap_or("");
            let target = first_line
                .split_whitespace()
                .nth(1)
                .unwrap_or("")
                .to_string();
            let has_injected = head
                .lines()
                .any(|l| l.to_ascii_lowercase().starts_with("x-injected:"));
            {
                let mut g = cap_clone.lock().unwrap();
                g.targets.push(target);
                g.bodies.push(body.to_string());
                if has_injected {
                    g.has_injected_header = true;
                }
            }
            let payload = r#"{"items":[]}"#;
            let resp = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                payload.len(),
                payload
            );
            stream.write_all(resp.as_bytes()).unwrap();
        }
    });

    let cfg = HttpClientConfig {
        allow_private_ips: true,
        ..Default::default()
    };
    let engine = BookSourceEngine::new(cfg, Duration::from_secs(2));
    let src = LegadoBookSource {
        book_source_name: "InjectTestBook".into(),
        book_source_url: format!("http://{addr}"),
        search_url: Some(
            "/search?q={{key}}&p={{page}},{\"method\":\"POST\",\"body\":\"kw={{key}}&p={{page}}\"}"
                .into(),
        ),
        rule_search: Some(crate::source::model::LegadoSearchRule {
            book_list: Some("$.items[*]".into()),
            name: Some("$.name".into()),
            ..Default::default()
        }),
        ..Default::default()
    };

    engine
        .search_sync(
            &src,
            SourceRuntimeState::default(),
            "hello world&injected=1",
            1,
        )
        .unwrap();
    let poison = "x\",\"headers\":{\"X-Injected\":\"pwned\"},\"z\":\"";
    engine
        .search_sync(&src, SourceRuntimeState::default(), poison, 2)
        .unwrap();

    server.join().unwrap();
    let cap = captured.lock().unwrap();
    assert!(!cap.has_injected_header, "X-Injected header leaked!");
    assert_eq!(cap.targets[0], "/search?q=hello%20world%26injected%3D1&p=1");
    assert_eq!(cap.bodies[0], "kw=hello%20world%26injected%3D1&p=1");
    assert!(cap.targets[1].contains("q=x%22"));
    assert!(!cap.targets[1].contains("X-Injected\":\"pwned"));
}

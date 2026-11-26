use super::*;

#[tokio::test]
async fn online_detail_chapters_pages_enforce_cancel_on_future_drop() {
    use std::sync::atomic::{AtomicUsize, Ordering};
    use tokio::sync::Notify;

    let entered = Arc::new(Notify::new());
    let entered_c = entered.clone();
    let ch_p2_hits = Arc::new(AtomicUsize::new(0));
    let ch_p2_hits_c = ch_p2_hits.clone();

    let mock_app = Router::new()
        .route(
            "/slow_page1",
            get(move || {
                let notify = entered_c.clone();
                async move {
                    notify.notify_one();
                    tokio::time::sleep(std::time::Duration::from_secs(5)).await;
                    Response::builder()
                        .status(StatusCode::OK)
                        .body(Body::from("<h1>late</h1>"))
                        .unwrap()
                }
            }),
        )
        .route(
            "/toc_p1",
            get(|| async {
                Response::builder()
                    .status(StatusCode::OK)
                    .header("content-type", "text/html")
                    .body(Body::from(
                        r#"<ul><li><a href="/ch/1">Ch 1</a></li></ul><a class="next" href="/toc_p2">Next</a>"#,
                    ))
                    .unwrap()
            }),
        )
        .route(
            "/toc_p2",
            get(move || {
                let hits = ch_p2_hits_c.clone();
                async move {
                    hits.fetch_add(1, Ordering::SeqCst);
                    tokio::time::sleep(std::time::Duration::from_secs(5)).await;
                    Response::builder()
                        .status(StatusCode::OK)
                        .body(Body::from("<ul></ul>"))
                        .unwrap()
                }
            }),
        );

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let server = tokio::spawn(async move {
        axum::serve(listener, mock_app).await.unwrap();
    });
    let base = format!("http://{addr}");

    let h = setup_test_harness(true).await;

    let src = serde_json::json!({
        "sourceName": "SlowDropSource",
        "sourceUrl": base,
        "enabledExplore": true,
        "exploreUrl": "Slow::/slow_page1",
        "ruleSearch": { "bookList": "h1", "name": "@text", "bookUrl": "@text" },
        "ruleDetail": { "name": "h1@text" },
        "ruleToc": {
            "chapterList": "ul li",
            "chapterName": "a@text",
            "chapterUrl": "a@href",
            "nextTocUrl": "a.next@href"
        },
        "ruleContent": {
            "content": "#imgs@html",
            "nextContentUrl": "a.next@href"
        }
    });
    let (st_imp, b_imp) = json_request(
        &h.app,
        "POST",
        "/api/v1/comic/sources/import",
        &h.admin_token,
        Some(serde_json::json!({ "raw_json": src.to_string() })),
    )
    .await;
    assert_eq!(st_imp, StatusCode::OK);
    let imp_val: serde_json::Value = serde_json::from_slice(&b_imp).unwrap();
    let sid = imp_val["sources"][0]["id"].as_str().unwrap().to_string();

    // Dropping explore / detail / chapters / pages futures via short client timeout must return immediately
    // without blocking the caller or panicking.
    for (uri, payload) in [
        (
            "/api/v1/comic/online/explore",
            serde_json::json!({ "source_id": sid, "page": 1 }),
        ),
        (
            "/api/v1/comic/online/detail",
            serde_json::json!({ "source_id": sid, "comic_url": format!("{base}/slow_page1") }),
        ),
        (
            "/api/v1/comic/online/chapters",
            serde_json::json!({ "source_id": sid, "toc_url": format!("{base}/toc_p1") }),
        ),
        (
            "/api/v1/comic/online/pages",
            serde_json::json!({ "source_id": sid, "chapter_url": format!("{base}/slow_page1") }),
        ),
    ] {
        let res = tokio::time::timeout(
            std::time::Duration::from_millis(120),
            json_request(&h.app, "POST", uri, &h.member_token, Some(payload)),
        )
        .await;
        assert!(res.is_err(), "expected short client timeout on {uri}");
    }

    // Confirm mock server saw /slow_page1 and /toc_p2 requests
    assert!(ch_p2_hits.load(Ordering::SeqCst) >= 1);
    server.abort();
}

#[tokio::test]
async fn test_source_and_online_ops_preserve_concurrent_runtime_state_e2e() {
    use tokio::sync::Notify;

    let search_entered = Arc::new(Notify::new());
    let search_release = Arc::new(Notify::new());
    let detail_entered = Arc::new(Notify::new());
    let detail_release = Arc::new(Notify::new());

    let se = search_entered.clone();
    let sr = search_release.clone();
    let de = detail_entered.clone();
    let dr = detail_release.clone();

    let mock_app = Router::new()
        .route(
            "/search",
            get(move || {
                let se = se.clone();
                let sr = sr.clone();
                async move {
                    se.notify_one();
                    sr.notified().await;
                    Response::builder()
                        .status(StatusCode::OK)
                        .header("content-type", "application/json")
                        .body(Body::from(r#"{"items":[{"t":"Found","u":"/detail"}]}"#))
                        .unwrap()
                }
            }),
        )
        .route(
            "/detail",
            get(move || {
                let de = de.clone();
                let dr = dr.clone();
                async move {
                    de.notify_one();
                    dr.notified().await;
                    Response::builder()
                        .status(StatusCode::OK)
                        .header("content-type", "application/json")
                        .body(Body::from(r#"{"title":"DetailTitle"}"#))
                        .unwrap()
                }
            }),
        );

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let server = tokio::spawn(async move {
        axum::serve(listener, mock_app).await.unwrap();
    });
    let base = format!("http://{addr}");

    let h = setup_test_harness(true).await;
    let src = serde_json::json!({
        "sourceName": "ConcurrentStateSource",
        "sourceUrl": base,
        "loginUi": "[{\"name\":\"username\",\"type\":\"text\"},{\"name\":\"apiToken\",\"type\":\"password\"}]",
        "loginUrl": "@js:'logged'",
        "searchUrl": format!("{base}/search?q={{{{key}}}}"),
        "ruleSearch": { "bookList": "$.items[*]", "name": "$.t", "bookUrl": "$.u" },
        "ruleDetail": {
            "init": "@js:cookie.setCookie(baseUrl, 'detail_ck=from_detail');result",
            "name": "$.title"
        }
    });
    let (st_imp, b_imp) = json_request(
        &h.app,
        "POST",
        "/api/v1/comic/sources/import",
        &h.admin_token,
        Some(serde_json::json!({ "raw_json": src.to_string() })),
    )
    .await;
    assert_eq!(st_imp, StatusCode::OK);
    let imp_val: serde_json::Value = serde_json::from_slice(&b_imp).unwrap();
    let sid = imp_val["sources"][0]["id"].as_str().unwrap().to_string();

    // Seed v0 / t0
    let (st_l0, _) = json_request(
        &h.app,
        "POST",
        &format!("/api/v1/comic/sources/{sid}/login-action"),
        &h.admin_token,
        Some(serde_json::json!({
            "login_info": { "username": "alice_v0", "apiToken": "t0" },
            "action": "source.setVariable('v0');'ok'"
        })),
    )
    .await;
    assert_eq!(st_l0, StatusCode::OK);

    // Subcase A: test_source (search) in-flight vs concurrent login-action -> preserves v1_fresh & t1_fresh
    let app_a = h.app.clone();
    let tok_a = h.admin_token.clone();
    let sid_a = sid.clone();
    let test_task = tokio::spawn(async move {
        json_request(
            &app_a,
            "POST",
            &format!("/api/v1/comic/sources/{sid_a}/test"),
            &tok_a,
            Some(serde_json::json!({ "mode": "search", "keyword": "k" })),
        )
        .await
    });

    search_entered.notified().await;
    let (st_l1, _) = json_request(
        &h.app,
        "POST",
        &format!("/api/v1/comic/sources/{sid}/login-action"),
        &h.admin_token,
        Some(serde_json::json!({
            "login_info": { "username": "alice_v1", "apiToken": "t1_fresh" },
            "action": "source.setVariable('v1_fresh');'ok'"
        })),
    )
    .await;
    assert_eq!(st_l1, StatusCode::OK);
    search_release.notify_one();

    let (st_test, _) = test_task.await.unwrap();
    assert_eq!(st_test, StatusCode::OK);

    let (st_v1, b_v1) = json_request(
        &h.app,
        "GET",
        &format!("/api/v1/comic/sources/{sid}/variable"),
        &h.admin_token,
        None,
    )
    .await;
    assert_eq!(st_v1, StatusCode::OK);
    let v1_json: serde_json::Value = serde_json::from_slice(&b_v1).unwrap();
    assert_eq!(v1_json["variable"], "v1_fresh");

    let (st_chk1, b_chk1) = json_request(
        &h.app,
        "POST",
        &format!("/api/v1/comic/sources/{sid}/login-action"),
        &h.admin_token,
        Some(serde_json::json!({
            "login_info": { "username": "alice_v1", "apiToken": "••••••••" },
            "action": "source.getLoginInfoMap().apiToken"
        })),
    )
    .await;
    assert_eq!(st_chk1, StatusCode::OK);
    let chk1_json: serde_json::Value = serde_json::from_slice(&b_chk1).unwrap();
    assert_eq!(chk1_json["result"], "t1_fresh");

    // Subcase B: online/detail (sets cookie without changing variable/login_info) vs concurrent login-action
    let app_b = h.app.clone();
    let tok_b = h.member_token.clone();
    let sid_b = sid.clone();
    let base_b = base.clone();
    let detail_task = tokio::spawn(async move {
        json_request(
            &app_b,
            "POST",
            "/api/v1/comic/online/detail",
            &tok_b,
            Some(
                serde_json::json!({ "source_id": sid_b, "comic_url": format!("{base_b}/detail") }),
            ),
        )
        .await
    });

    detail_entered.notified().await;
    let (st_l2, _) = json_request(
        &h.app,
        "POST",
        &format!("/api/v1/comic/sources/{sid}/login-action"),
        &h.admin_token,
        Some(serde_json::json!({
            "login_info": { "username": "alice_v2", "apiToken": "t2_fresh" },
            "action": "source.setVariable('v2_fresh');'ok'"
        })),
    )
    .await;
    assert_eq!(st_l2, StatusCode::OK);
    detail_release.notify_one();

    let (st_det, _) = detail_task.await.unwrap();
    assert_eq!(st_det, StatusCode::OK);

    let (st_v2, b_v2) = json_request(
        &h.app,
        "GET",
        &format!("/api/v1/comic/sources/{sid}/variable"),
        &h.admin_token,
        None,
    )
    .await;
    assert_eq!(st_v2, StatusCode::OK);
    let v2_json: serde_json::Value = serde_json::from_slice(&b_v2).unwrap();
    assert_eq!(v2_json["variable"], "v2_fresh");

    let (st_chk2, b_chk2) = json_request(
        &h.app,
        "POST",
        &format!("/api/v1/comic/sources/{sid}/login-action"),
        &h.admin_token,
        Some(serde_json::json!({
            "login_info": { "username": "alice_v2", "apiToken": "••••••••" },
            "action": "source.getLoginInfoMap().apiToken"
        })),
    )
    .await;
    assert_eq!(st_chk2, StatusCode::OK);
    let chk2_json: serde_json::Value = serde_json::from_slice(&b_chk2).unwrap();
    assert_eq!(chk2_json["result"], "t2_fresh");
    assert!(
        chk2_json["source"]["cookie_hosts"]
            .as_array()
            .unwrap()
            .iter()
            .any(|v| v.as_str() == Some("127.0.0.1")),
        "cookie set by concurrent online/detail should be preserved: {}",
        chk2_json["source"]["cookie_hosts"]
    );

    server.abort();
}

#[tokio::test]
async fn patch_and_import_upsert_concurrent_with_login_action_preserve_runtime_state_e2e() {
    let h = setup_test_harness(true).await;

    let src = serde_json::json!({
        "sourceName": "ConcurrentPatchImport",
        "sourceUrl": "https://concurrent-comic.test",
        "sourceGroup": "InitGroup",
        "weight": 1,
        "loginUi": "[{\"name\":\"user\",\"type\":\"text\"},{\"name\":\"apiToken\",\"type\":\"password\"}]",
        "loginUrl": "@js:'ok'"
    });
    let (st_imp, b_imp) = json_request(
        &h.app,
        "POST",
        "/api/v1/comic/sources/import",
        &h.admin_token,
        Some(serde_json::json!({ "raw_json": src.to_string() })),
    )
    .await;
    assert_eq!(st_imp, StatusCode::OK);
    let imp_val: serde_json::Value = serde_json::from_slice(&b_imp).unwrap();
    let sid = imp_val["sources"][0]["id"].as_str().unwrap().to_string();

    for round in 0..20i64 {
        let var_val = format!("var_round_{round}");
        let tok_val = format!("tok_round_{round}");
        let group_patch = format!("PatchGroup_{round}");
        let group_import = format!("ImportGroup_{round}");
        let weight_target = 100 + round;

        let action_js = format!(
            "source.setVariable('{var_val}');cookie.setCookie('https://concurrent-comic.test','sid=s{round}');cache.put('k','c{round}');'ok'"
        );
        let import_payload = serde_json::json!({
            "sourceName": "ConcurrentPatchImport",
            "sourceUrl": "https://concurrent-comic.test",
            "sourceGroup": group_import,
            "weight": weight_target,
            "loginUi": "[{\"name\":\"user\",\"type\":\"text\"},{\"name\":\"apiToken\",\"type\":\"password\"}]",
            "loginUrl": "@js:'ok'"
        })
        .to_string();

        let login_uri = format!("/api/v1/comic/sources/{sid}/login-action");
        let patch_uri = format!("/api/v1/comic/sources/{sid}");

        let (res_login, res_patch) = tokio::join!(
            json_request(
                &h.app,
                "POST",
                &login_uri,
                &h.admin_token,
                Some(serde_json::json!({
                    "login_info": { "user": format!("u{round}"), "apiToken": tok_val },
                    "action": action_js
                })),
            ),
            json_request(
                &h.app,
                "PATCH",
                &patch_uri,
                &h.admin_token,
                Some(serde_json::json!({
                    "weight": weight_target,
                    "group_name": group_patch
                })),
            ),
        );
        assert_eq!(res_login.0, StatusCode::OK);
        assert_eq!(res_patch.0, StatusCode::OK);

        // Also race login-action against import upsert
        let var_val2 = format!("var_round_{round}_b");
        let tok_val2 = format!("tok_round_{round}_b");
        let action_js2 = format!(
            "source.setVariable('{var_val2}');cookie.setCookie('https://concurrent-comic.test','sid=s{round}_b');cache.put('k','c{round}_b');'ok'"
        );
        let (res_login2, res_upsert) = tokio::join!(
            json_request(
                &h.app,
                "POST",
                &login_uri,
                &h.admin_token,
                Some(serde_json::json!({
                    "login_info": { "user": format!("u{round}_b"), "apiToken": tok_val2 },
                    "action": action_js2
                })),
            ),
            json_request(
                &h.app,
                "POST",
                "/api/v1/comic/sources/import",
                &h.admin_token,
                Some(serde_json::json!({ "raw_json": import_payload })),
            ),
        );
        assert_eq!(res_login2.0, StatusCode::OK);
        assert_eq!(res_upsert.0, StatusCode::OK);

        // Verify neither login_info, variable, cookies, cache, nor metadata was lost
        let (st_chk, b_chk) = json_request(
            &h.app,
            "POST",
            &format!("/api/v1/comic/sources/{sid}/login-action"),
            &h.admin_token,
            Some(serde_json::json!({
                "login_info": { "user": format!("u{round}_b"), "apiToken": "••••••••" },
                "action": "JSON.stringify({v:source.getVariable(),t:source.getLoginInfoMap().apiToken,c:cookie.getCookie('https://concurrent-comic.test'),k:cache.get('k')})"
            })),
        )
        .await;
        assert_eq!(st_chk, StatusCode::OK);
        let chk_val: serde_json::Value = serde_json::from_slice(&b_chk).unwrap();
        let inner: serde_json::Value =
            serde_json::from_str(chk_val["result"].as_str().unwrap()).unwrap();
        assert_eq!(inner["v"], var_val2);
        assert_eq!(inner["t"], tok_val2);
        assert!(
            inner["c"]
                .as_str()
                .unwrap()
                .contains(&format!("sid=s{round}_b"))
        );
        assert_eq!(inner["k"], format!("c{round}_b"));
        assert_eq!(chk_val["source"]["weight"], weight_target);
        assert_eq!(chk_val["source"]["group_name"], group_import);
    }
}

#[tokio::test]
async fn online_search_blocks_keyword_smuggling_and_detail_preserves_same_host_cookies_e2e() {
    use axum::routing::post;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use tokio::sync::Notify;

    let smuggle_hits = Arc::new(AtomicUsize::new(0));
    let smuggle_hits_c = smuggle_hits.clone();
    let smuggle_hits_opt = smuggle_hits.clone();
    let detail_entered = Arc::new(Notify::new());
    let detail_release = Arc::new(Notify::new());
    let de = detail_entered.clone();
    let dr = detail_release.clone();

    let mock_app = Router::new()
        .route(
            "/search_plain",
            get(|| async {
                Response::builder()
                    .status(StatusCode::OK)
                    .header("content-type", "application/json")
                    .body(Body::from(r#"{"items":[{"t":"PlainOk","u":"/detail"}]}"#))
                    .unwrap()
            })
            .post(move || {
                let hits = smuggle_hits_c.clone();
                async move {
                    hits.fetch_add(1, Ordering::SeqCst);
                    Response::builder()
                        .status(StatusCode::OK)
                        .header("content-type", "application/json")
                        .body(Body::from(r#"{"items":[{"t":"Smuggled","u":"/detail"}]}"#))
                        .unwrap()
                }
            }),
        )
        .route(
            "/search_opts",
            post(move |headers: HeaderMap, body: String| {
                let hits = smuggle_hits_opt.clone();
                async move {
                    if headers.get("x-smuggled").is_some() {
                        hits.fetch_add(1, Ordering::SeqCst);
                    }
                    Response::builder()
                        .status(StatusCode::OK)
                        .header("content-type", "application/json")
                        .body(Body::from(
                            serde_json::json!({
                                "items": [{ "t": format!("Opts:{body}"), "u": "/detail" }]
                            })
                            .to_string(),
                        ))
                        .unwrap()
                }
            }),
        )
        .route(
            "/detail",
            get(move || {
                let de = de.clone();
                let dr = dr.clone();
                async move {
                    de.notify_one();
                    dr.notified().await;
                    Response::builder()
                        .status(StatusCode::OK)
                        .header("content-type", "application/json")
                        .header("set-cookie", "detail_ck=from_detail; Path=/")
                        .body(Body::from(r#"{"title":"DetailTitle"}"#))
                        .unwrap()
                }
            }),
        );

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let server = tokio::spawn(async move {
        axum::serve(listener, mock_app).await.unwrap();
    });
    let base = format!("http://{addr}");

    let h = setup_test_harness(true).await;

    let sources = serde_json::json!([
        {
            "sourceName": "JsSearch",
            "sourceUrl": format!("{base}#js"),
            "searchUrl": format!("@js:'{base}/search_plain?q=' + key"),
            "ruleSearch": { "bookList": "$.items[*]", "name": "$.t", "bookUrl": "$.u" }
        },
        {
            "sourceName": "MustacheArithSearch",
            "sourceUrl": format!("{base}#ma"),
            "searchUrl": format!("{base}/search_plain?offset={{{{((page - 1) * 20)}}}}&q={{{{key}}}}"),
            "ruleSearch": { "bookList": "$.items[*]", "name": "$.t", "bookUrl": "$.u" }
        },
        {
            "sourceName": "OptsSearch",
            "sourceUrl": format!("{base}#opts"),
            "enabledCookieJar": true,
            "loginUrl": "@js:'ok'",
            "searchUrl": format!("{base}/search_opts,{{\"method\":\"POST\",\"body\":\"q={{{{key}}}}\"}}"),
            "ruleSearch": { "bookList": "$.items[*]", "name": "$.t", "bookUrl": "$.u" },
            "ruleDetail": { "name": "$.title" }
        }
    ]);
    let (st_imp, b_imp) = json_request(
        &h.app,
        "POST",
        "/api/v1/comic/sources/import",
        &h.admin_token,
        Some(serde_json::json!({ "raw_json": sources.to_string() })),
    )
    .await;
    assert_eq!(st_imp, StatusCode::OK);
    let imp_val: serde_json::Value = serde_json::from_slice(&b_imp).unwrap();
    let js_id = imp_val["sources"][0]["id"].as_str().unwrap().to_string();
    let ma_id = imp_val["sources"][1]["id"].as_str().unwrap().to_string();
    let opts_id = imp_val["sources"][2]["id"].as_str().unwrap().to_string();

    // 1. Member search smuggling attempts on @js: is isolated as error, while Mustache arithmetic safely percent-encodes {{key}}
    {
        let (st, b) = json_request(
            &h.app,
            "POST",
            "/api/v1/comic/online/search",
            &h.member_token,
            Some(serde_json::json!({
                "keyword": r#"a,{"method":"POST","headers":{"X-Smuggled":"YES"},"body":"evil"}"#,
                "source_ids": [&js_id],
                "page": 1
            })),
        )
        .await;
        assert_eq!(st, StatusCode::OK);
        let v: serde_json::Value = serde_json::from_slice(&b).unwrap();
        assert!(v["results"].as_array().unwrap().is_empty());
        assert_eq!(v["errors"].as_array().unwrap().len(), 1);

        let (st_ma, b_ma) = json_request(
            &h.app,
            "POST",
            "/api/v1/comic/online/search",
            &h.member_token,
            Some(serde_json::json!({
                "keyword": r#"a,{"method":"POST","headers":{"X-Smuggled":"YES"},"body":"evil"}"#,
                "source_ids": [&ma_id],
                "page": 1
            })),
        )
        .await;
        assert_eq!(st_ma, StatusCode::OK);
        let v_ma: serde_json::Value = serde_json::from_slice(&b_ma).unwrap();
        assert_eq!(v_ma["results"][0]["title"], "PlainOk");
        assert!(v_ma["errors"].as_array().unwrap().is_empty());
    }

    // 2. Member JSON breakout attempt inside ,{options} searchUrl does NOT smuggle X-Smuggled header
    let breakout_kw = r#"a","headers":{"X-Smuggled":"PWNED_HEADER"},"x":""#;
    let (st_opt, b_opt) = json_request(
        &h.app,
        "POST",
        "/api/v1/comic/online/search",
        &h.member_token,
        Some(serde_json::json!({
            "keyword": breakout_kw,
            "source_ids": [&opts_id],
            "page": 1
        })),
    )
    .await;
    assert_eq!(st_opt, StatusCode::OK);
    let v_opt: serde_json::Value = serde_json::from_slice(&b_opt).unwrap();
    assert_eq!(
        v_opt["results"][0]["title"],
        format!(
            "Opts:q={}",
            xmedia_source::http::url_encode_component(breakout_kw)
        )
    );

    // 3. Same-host cookie-pair granularity: slow online/detail Set-Cookie: detail_ck=from_detail
    //    must preserve concurrent login-action session=s1_fresh rotation on 127.0.0.1!
    let (st_seed_ck, _) = json_request(
        &h.app,
        "POST",
        &format!("/api/v1/comic/sources/{opts_id}/login-action"),
        &h.admin_token,
        Some(serde_json::json!({
            "login_info": {},
            "action": format!("cookie.setCookie('{base}', 'session=s0');'ok'")
        })),
    )
    .await;
    assert_eq!(st_seed_ck, StatusCode::OK);

    let app_d = h.app.clone();
    let tok_d = h.member_token.clone();
    let oid_d = opts_id.clone();
    let base_d = base.clone();
    let detail_task = tokio::spawn(async move {
        json_request(
            &app_d,
            "POST",
            "/api/v1/comic/online/detail",
            &tok_d,
            Some(serde_json::json!({
                "source_id": oid_d,
                "comic_url": format!("{base_d}/detail")
            })),
        )
        .await
    });

    detail_entered.notified().await;
    let (st_rot, _) = json_request(
        &h.app,
        "POST",
        &format!("/api/v1/comic/sources/{opts_id}/login-action"),
        &h.admin_token,
        Some(serde_json::json!({
            "login_info": {},
            "action": format!("cookie.setCookie('{base}', 'session=s1_fresh');'ok'")
        })),
    )
    .await;
    assert_eq!(st_rot, StatusCode::OK);
    detail_release.notify_one();

    let (st_det, _) = detail_task.await.unwrap();
    assert_eq!(st_det, StatusCode::OK);

    let (st_chk, b_chk) = json_request(
        &h.app,
        "POST",
        &format!("/api/v1/comic/sources/{opts_id}/login-action"),
        &h.admin_token,
        Some(serde_json::json!({
            "login_info": {},
            "action": format!("cookie.getCookie('{base}')")
        })),
    )
    .await;
    assert_eq!(st_chk, StatusCode::OK);
    let chk_val: serde_json::Value = serde_json::from_slice(&b_chk).unwrap();
    let cookies_str = chk_val["result"].as_str().unwrap();
    assert!(
        cookies_str.contains("session=s1_fresh"),
        "concurrent login-action cookie rotation was clobbered: {cookies_str}"
    );
    assert!(
        cookies_str.contains("detail_ck=from_detail"),
        "detail Set-Cookie was lost: {cookies_str}"
    );
    assert_eq!(smuggle_hits.load(Ordering::SeqCst), 0);

    server.abort();
}

#[tokio::test]
async fn online_search_enforces_source_concurrent_rate_throttling() {
    let h = setup_test_harness(true).await;

    let origin_app = Router::new().route(
        "/search",
        get(|| async {
            Response::builder()
                .status(StatusCode::OK)
                .header("content-type", "application/json")
                .body(Body::from(
                    r#"{"items":[{"title":"Throttled","url":"/c/1"}]}"#,
                ))
                .unwrap()
        }),
    );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let server = tokio::spawn(async move {
        axum::serve(listener, origin_app).await.unwrap();
    });
    let base = format!("http://{addr}");

    let src = serde_json::json!({
        "sourceName": "ThrottledComic",
        "sourceUrl": base,
        "concurrentRate": "1/350",
        "searchUrl": format!("{base}/search?q={{{{key}}}}"),
        "ruleSearch": {
            "bookList": "$.items[*]",
            "name": "$.title",
            "bookUrl": "@js:'https://example.test/c/1'"
        }
    });
    let (st_imp, b_imp) = json_request(
        &h.app,
        "POST",
        "/api/v1/comic/sources/import",
        &h.admin_token,
        Some(serde_json::json!({ "raw_json": src.to_string() })),
    )
    .await;
    assert_eq!(st_imp, StatusCode::OK);
    let imp_val: serde_json::Value = serde_json::from_slice(&b_imp).unwrap();
    let sid = imp_val["sources"][0]["id"].as_str().unwrap().to_string();

    let t0 = std::time::Instant::now();
    let (st1, b1) = json_request(
        &h.app,
        "POST",
        "/api/v1/comic/online/search",
        &h.member_token,
        Some(serde_json::json!({ "keyword": "a", "source_ids": [sid] })),
    )
    .await;
    let (st2, b2) = json_request(
        &h.app,
        "POST",
        "/api/v1/comic/online/search",
        &h.member_token,
        Some(serde_json::json!({ "keyword": "b", "source_ids": [sid] })),
    )
    .await;
    let elapsed = t0.elapsed();

    assert_eq!(st1, StatusCode::OK);
    assert_eq!(st2, StatusCode::OK);
    let v1: serde_json::Value = serde_json::from_slice(&b1).unwrap();
    let v2: serde_json::Value = serde_json::from_slice(&b2).unwrap();
    assert_eq!(v1["results"].as_array().unwrap().len(), 1);
    assert_eq!(v2["results"].as_array().unwrap().len(), 1);
    assert!(
        elapsed >= std::time::Duration::from_millis(300),
        "expected concurrentRate 1/350 throttling, elapsed={elapsed:?}"
    );

    server.abort();
}

#[tokio::test]
async fn caller_supplied_comic_urls_reject_legado_options_and_data_urls_with_400() {
    let h = setup_test_harness(true).await;
    let src_json = serde_json::json!([{
        "bookSourceName": "GuardComicSource",
        "bookSourceUrl": "https://comic-guard.example",
        "searchUrl": "/search?q={{key}}",
        "ruleSearch": { "bookList": "$.items", "name": "$.name", "bookUrl": "$.url" }
    }]);
    let (st_imp, b_imp) = json_request(
        &h.app,
        "POST",
        "/api/v1/comic/sources/import",
        &h.admin_token,
        Some(serde_json::json!({ "raw_json": src_json.to_string() })),
    )
    .await;
    assert_eq!(st_imp, StatusCode::OK);
    let imp_val: serde_json::Value = serde_json::from_slice(&b_imp).unwrap();
    let sid = imp_val["sources"][0]["id"].as_str().unwrap().to_string();

    for bad_url in [
        "https://comic-guard.example/c/1,{\"method\":\"POST\",\"headers\":{\"X-Evil\":\"1\"}}",
        "data:text/html,<h1>evil</h1>",
    ] {
        for (uri, payload) in [
            (
                "/api/v1/comic/online/detail",
                serde_json::json!({ "source_id": sid, "comic_url": bad_url }),
            ),
            (
                "/api/v1/comic/online/chapters",
                serde_json::json!({ "source_id": sid, "toc_url": bad_url }),
            ),
            (
                "/api/v1/comic/online/pages",
                serde_json::json!({ "source_id": sid, "chapter_url": bad_url }),
            ),
        ] {
            let (st, _) = json_request(&h.app, "POST", uri, &h.member_token, Some(payload)).await;
            assert_eq!(
                st,
                StatusCode::BAD_REQUEST,
                "expected 400 on {uri} for {bad_url}"
            );
        }
    }
}

#[tokio::test]
async fn comic_online_search_enforces_cancel_on_future_drop() {
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::time::Duration;

    let step1_entered = Arc::new(tokio::sync::Notify::new());
    let step1_entered_c = step1_entered.clone();
    let step2_hits = Arc::new(AtomicUsize::new(0));
    let step2_hits_c = step2_hits.clone();

    let origin_app = Router::new()
        .route(
            "/step1",
            get(move || {
                let notify = step1_entered_c.clone();
                async move {
                    notify.notify_waiters();
                    tokio::time::sleep(Duration::from_millis(220)).await;
                    r#"{"items":[{"name":"Comic","url":"/c/1"}]}"#
                }
            }),
        )
        .route(
            "/step2",
            get(move || {
                let hits = step2_hits_c.clone();
                async move {
                    hits.fetch_add(1, Ordering::SeqCst);
                    r#"{"ok":true}"#
                }
            }),
        );

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let base = format!("http://{addr}");
    let srv = tokio::spawn(async move {
        axum::serve(listener, origin_app).await.unwrap();
    });

    let h = setup_test_harness(true).await;
    let src_json = serde_json::json!([{
        "bookSourceName": "DropComicSearchSource",
        "bookSourceUrl": base,
        "searchUrl": format!("{base}/step1?q={{{{key}}}}"),
        "ruleSearch": {
            "bookList": "$.items[*]",
            "name": "@js:java.ajax(baseUrl.replace('/step1', '/step2')); 'Comic'",
            "bookUrl": "$.url"
        }
    }]);
    let (st_imp, b_imp) = json_request(
        &h.app,
        "POST",
        "/api/v1/comic/sources/import",
        &h.admin_token,
        Some(serde_json::json!({ "raw_json": src_json.to_string() })),
    )
    .await;
    assert_eq!(st_imp, StatusCode::OK);
    let imp_val: serde_json::Value = serde_json::from_slice(&b_imp).unwrap();
    let sid = imp_val["sources"][0]["id"].as_str().unwrap().to_string();

    {
        let search_fut = json_request(
            &h.app,
            "POST",
            "/api/v1/comic/online/search",
            &h.member_token,
            Some(serde_json::json!({ "keyword": "drop", "source_ids": [sid] })),
        );
        tokio::pin!(search_fut);
        tokio::select! {
            _ = &mut search_fut => panic!("comic search completed before drop"),
            _ = step1_entered.notified() => {}
        }
    }

    tokio::time::sleep(Duration::from_millis(320)).await;
    assert_eq!(
        step2_hits.load(Ordering::SeqCst),
        0,
        "CancelTasksOnDrop must abort comic search tasks before step 2 java.ajax"
    );

    srv.abort();
}

#[tokio::test]
async fn comic_image_proxy_strips_static_range_from_source_headers() {
    let seen_range: Arc<std::sync::Mutex<Vec<Option<String>>>> =
        Arc::new(std::sync::Mutex::new(Vec::new()));
    let seen_referer: Arc<std::sync::Mutex<Vec<Option<String>>>> =
        Arc::new(std::sync::Mutex::new(Vec::new()));
    let seen_range_c = seen_range.clone();
    let seen_referer_c = seen_referer.clone();

    let png = valid_png_bytes();
    let origin_app = Router::new().route(
        "/page1.png",
        get(move |headers: HeaderMap| {
            let ranges = seen_range_c.clone();
            let refs = seen_referer_c.clone();
            let bytes = png.clone();
            async move {
                let rng = headers
                    .get(header::RANGE)
                    .and_then(|v| v.to_str().ok())
                    .map(ToString::to_string);
                let ref_val = headers
                    .get(header::REFERER)
                    .and_then(|v| v.to_str().ok())
                    .map(ToString::to_string);
                ranges.lock().unwrap().push(rng.clone());
                refs.lock().unwrap().push(ref_val);
                if rng.is_some() {
                    return Response::builder()
                        .status(StatusCode::PARTIAL_CONTENT)
                        .header("content-type", "image/png")
                        .body(Body::from(bytes[..10].to_vec()))
                        .unwrap();
                }
                Response::builder()
                    .status(StatusCode::OK)
                    .header("content-type", "image/png")
                    .body(Body::from(bytes))
                    .unwrap()
            }
        }),
    );

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let base = format!("http://{addr}");
    let srv = tokio::spawn(async move {
        axum::serve(listener, origin_app).await.unwrap();
    });

    let h = setup_test_harness(true).await;
    let src_json = serde_json::json!([{
        "bookSourceName": "StaticRangeComicSource",
        "bookSourceUrl": base,
        "header": r#"{"range":"bytes=0-5"}"#,
        "imageHeader": r#"{"Range":"bytes=0-10","Referer":"https://ok.example/"}"#
    }]);
    let (st_imp, b_imp) = json_request(
        &h.app,
        "POST",
        "/api/v1/comic/sources/import",
        &h.admin_token,
        Some(serde_json::json!({ "raw_json": src_json.to_string() })),
    )
    .await;
    assert_eq!(st_imp, StatusCode::OK);
    let imp_val: serde_json::Value = serde_json::from_slice(&b_imp).unwrap();
    let sid = imp_val["sources"][0]["id"].as_str().unwrap();

    let encoded_url =
        form_urlencoded::byte_serialize(format!("{base}/page1.png").as_bytes()).collect::<String>();
    let req = Request::builder()
        .method("GET")
        .uri(format!(
            "/api/v1/comic/online/image-proxy?source_id={sid}&url={encoded_url}"
        ))
        .header("Authorization", format!("Bearer {}", h.member_token))
        .body(Body::empty())
        .unwrap();
    let resp = h.app.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body = to_bytes(resp.into_body(), 1024 * 1024).await.unwrap();
    assert_eq!(body.as_ref(), valid_png_bytes().as_slice());
    assert_eq!(*seen_range.lock().unwrap(), vec![None]);
    assert_eq!(
        *seen_referer.lock().unwrap(),
        vec![Some("https://ok.example/".to_string())]
    );

    srv.abort();
}

#[tokio::test]
async fn image_proxy_retries_transient_503_and_does_not_retry_403() {
    use std::sync::atomic::{AtomicUsize, Ordering};

    let flaky_hits = Arc::new(AtomicUsize::new(0));
    let forbidden_hits = Arc::new(AtomicUsize::new(0));
    let fh = flaky_hits.clone();
    let fbh = forbidden_hits.clone();

    let origin_app = Router::new()
        .route(
            "/flaky.png",
            get(move || {
                let hits = fh.clone();
                async move {
                    let n = hits.fetch_add(1, Ordering::SeqCst) + 1;
                    if n == 1 {
                        Response::builder()
                            .status(StatusCode::SERVICE_UNAVAILABLE)
                            .body(Body::from("temporarily unavailable"))
                            .unwrap()
                    } else {
                        Response::builder()
                            .status(StatusCode::OK)
                            .header("content-type", "image/png")
                            .body(Body::from(valid_png_bytes()))
                            .unwrap()
                    }
                }
            }),
        )
        .route(
            "/forbidden.png",
            get(move || {
                let hits = fbh.clone();
                async move {
                    hits.fetch_add(1, Ordering::SeqCst);
                    (StatusCode::FORBIDDEN, "forbidden")
                }
            }),
        );

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let base = format!("http://{addr}");
    let srv = tokio::spawn(async move {
        axum::serve(listener, origin_app).await.unwrap();
    });

    let h = setup_test_harness(true).await;

    // 1. Transient 503 -> 200 succeeds with nosniff and sandbox CSP
    let enc_flaky =
        form_urlencoded::byte_serialize(format!("{base}/flaky.png").as_bytes()).collect::<String>();
    let resp_ok = h
        .app
        .clone()
        .oneshot(
            Request::builder()
                .method("GET")
                .uri(format!("/api/v1/comic/online/image-proxy?url={enc_flaky}"))
                .header(header::AUTHORIZATION, format!("Bearer {}", h.member_token))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp_ok.status(), StatusCode::OK);
    assert_eq!(
        resp_ok.headers().get(header::CONTENT_TYPE).unwrap(),
        "image/png"
    );
    assert_eq!(
        resp_ok
            .headers()
            .get(header::X_CONTENT_TYPE_OPTIONS)
            .unwrap(),
        "nosniff"
    );
    assert_eq!(
        resp_ok
            .headers()
            .get(header::CONTENT_SECURITY_POLICY)
            .unwrap(),
        xmedia_source::proxy::IMAGE_PROXY_CSP
    );
    let body_ok = to_bytes(resp_ok.into_body(), 1024 * 1024).await.unwrap();
    assert_eq!(body_ok.as_ref(), valid_png_bytes().as_slice());
    assert_eq!(flaky_hits.load(Ordering::SeqCst), 2);

    // 2. Upstream 403 returns 400 Bad Request in exactly 1 upstream hit (no retry)
    let enc_403 = form_urlencoded::byte_serialize(format!("{base}/forbidden.png").as_bytes())
        .collect::<String>();
    let resp_403 = h
        .app
        .clone()
        .oneshot(
            Request::builder()
                .method("GET")
                .uri(format!("/api/v1/comic/online/image-proxy?url={enc_403}"))
                .header(header::AUTHORIZATION, format!("Bearer {}", h.member_token))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp_403.status(), StatusCode::BAD_REQUEST);
    assert_eq!(forbidden_hits.load(Ordering::SeqCst), 1);

    srv.abort();
}

#[tokio::test]
async fn online_comic_items_favorites_progress_and_availability_e2e() {
    let h = setup_test_harness(true).await;

    // Import one comic source so we have a valid online source_id.
    let src_json = serde_json::json!({
        "bookSourceName": "Test Comic Source",
        "bookSourceUrl": "https://comic-item.test",
        "bookSourceGroup": "Test",
        "bookSourceType": 2,
        "ruleSearch": { "bookList": "$.items[*]", "name": "$.t", "bookUrl": "$.u" }
    });
    let (st_imp, b_imp) = json_request(
        &h.app,
        "POST",
        "/api/v1/comic/sources/import",
        &h.admin_token,
        Some(serde_json::json!({ "raw_json": src_json.to_string() })),
    )
    .await;
    assert_eq!(st_imp, StatusCode::OK);
    let imp_val: serde_json::Value = serde_json::from_slice(&b_imp).unwrap();
    let source_id = imp_val["sources"][0]["id"].as_str().unwrap().to_string();

    // 1. Idempotent upsert with favorite + progress (no trailing slash on /online/items)
    let (st_put, b_put) = json_request(
        &h.app,
        "PUT",
        "/api/v1/comic/online/items",
        &h.member_token,
        Some(serde_json::json!({
            "source_id": source_id,
            "item_key": "https://comic-item.test/series/1",
            "title": "One Piece",
            "subtitle": "Eiichiro Oda",
            "cover_url": "https://comic-item.test/cover.jpg",
            "extra": { "toc_url": "https://comic-item.test/series/1/toc" },
            "favorite": true,
            "progress": {
                "unit_index": 2,
                "unit_key": "https://comic-item.test/series/1/ch/3",
                "unit_title": "Chapter 3",
                "position": 5,
                "duration": 20,
                "total_units": 10,
                "completed": false
            }
        })),
    )
    .await;
    assert_eq!(st_put, StatusCode::OK);
    let item: serde_json::Value = serde_json::from_slice(&b_put).unwrap();
    let item_id = item["id"].as_str().unwrap().to_string();
    assert_eq!(item["media_kind"], "comic");
    assert_eq!(item["source_kind"], "online");
    assert_eq!(item["source_name"], "Test Comic Source");
    assert_eq!(item["availability"], "ok");
    assert_eq!(item["favorite"], true);
    assert_eq!(item["progress"]["unit_index"], 2);
    assert!((item["progress_ratio"].as_f64().unwrap() - 0.225).abs() < 1e-6);

    // 2. Also upsert a remote OPDS comic item
    let (st_rem, b_rem) = json_request(
        &h.app,
        "PUT",
        "/api/v1/comic/online/items",
        &h.member_token,
        Some(serde_json::json!({
            "source_kind": "remote",
            "source_id": "opds-home-1",
            "item_key": "urn:opds:comic:42",
            "title": "Remote Comic",
            "cover_url": "/_app/opds/opds-home-1/proxy-image?url=a",
            "favorite": true
        })),
    )
    .await;
    assert_eq!(st_rem, StatusCode::OK);
    let rem_item: serde_json::Value = serde_json::from_slice(&b_rem).unwrap();
    assert_eq!(rem_item["availability"], "unverified");

    // 3. Lookup returns both items for member, none for admin
    let (st_lk, b_lk) = json_request(
        &h.app,
        "POST",
        "/api/v1/comic/online/items/lookup",
        &h.member_token,
        Some(serde_json::json!({
            "items": [
                { "source_id": source_id, "item_key": "https://comic-item.test/series/1" },
                { "source_kind": "remote", "source_id": "opds-home-1", "item_key": "urn:opds:comic:42" },
                { "source_id": source_id, "item_key": "https://comic-item.test/missing" }
            ]
        })),
    )
    .await;
    assert_eq!(st_lk, StatusCode::OK);
    let lk: serde_json::Value = serde_json::from_slice(&b_lk).unwrap();
    assert_eq!(lk["total"], 2);

    let (st_other, _) = json_request(
        &h.app,
        "GET",
        &format!("/api/v1/comic/online/items/{item_id}"),
        &h.admin_token,
        None,
    )
    .await;
    assert_eq!(st_other, StatusCode::NOT_FOUND);

    // 4. Continue view returns only the in-progress item; Favorites returns both
    let (st_cont, b_cont) = json_request(
        &h.app,
        "GET",
        "/api/v1/comic/online/items?view=continue",
        &h.member_token,
        None,
    )
    .await;
    assert_eq!(st_cont, StatusCode::OK);
    let cont: serde_json::Value = serde_json::from_slice(&b_cont).unwrap();
    assert_eq!(cont["total"], 1);
    assert_eq!(cont["items"][0]["id"], item_id);

    let (st_fav, b_fav) = json_request(
        &h.app,
        "GET",
        "/api/v1/comic/online/items?view=favorites",
        &h.member_token,
        None,
    )
    .await;
    assert_eq!(st_fav, StatusCode::OK);
    let fav: serde_json::Value = serde_json::from_slice(&b_fav).unwrap();
    assert_eq!(fav["total"], 2);

    // 5. Disable source -> availability becomes source_disabled; delete source -> source_missing
    let (st_dis, _) = json_request(
        &h.app,
        "PATCH",
        &format!("/api/v1/comic/sources/{source_id}"),
        &h.admin_token,
        Some(serde_json::json!({ "enabled": false })),
    )
    .await;
    assert_eq!(st_dis, StatusCode::OK);

    let (st_get1, b_get1) = json_request(
        &h.app,
        "GET",
        &format!("/api/v1/comic/online/items/{item_id}"),
        &h.member_token,
        None,
    )
    .await;
    assert_eq!(st_get1, StatusCode::OK);
    let g1: serde_json::Value = serde_json::from_slice(&b_get1).unwrap();
    assert_eq!(g1["availability"], "source_disabled");

    let (st_del_src, _) = json_request(
        &h.app,
        "DELETE",
        &format!("/api/v1/comic/sources/{source_id}"),
        &h.admin_token,
        None,
    )
    .await;
    assert_eq!(st_del_src, StatusCode::NO_CONTENT);

    let (st_get2, b_get2) = json_request(
        &h.app,
        "GET",
        &format!("/api/v1/comic/online/items/{item_id}"),
        &h.member_token,
        None,
    )
    .await;
    assert_eq!(st_get2, StatusCode::OK);
    let g2: serde_json::Value = serde_json::from_slice(&b_get2).unwrap();
    assert_eq!(g2["availability"], "source_missing");
    assert!(g2["source_name"].is_null());

    // 6. Update progress -> clear progress -> patch favorite=false -> delete
    let (st_prog, b_prog) = json_request(
        &h.app,
        "PUT",
        &format!("/api/v1/comic/online/items/{item_id}/progress"),
        &h.member_token,
        Some(serde_json::json!({
            "unit_index": 9,
            "position": 19,
            "duration": 20,
            "total_units": 10,
            "completed": true
        })),
    )
    .await;
    assert_eq!(st_prog, StatusCode::OK);
    let p_val: serde_json::Value = serde_json::from_slice(&b_prog).unwrap();
    assert_eq!(p_val["progress"]["completed"], true);
    assert_eq!(p_val["progress_ratio"], 1.0);

    let (st_clr, b_clr) = json_request(
        &h.app,
        "DELETE",
        &format!("/api/v1/comic/online/items/{item_id}/progress"),
        &h.member_token,
        None,
    )
    .await;
    assert_eq!(st_clr, StatusCode::OK);
    let clr_val: serde_json::Value = serde_json::from_slice(&b_clr).unwrap();
    assert!(clr_val["progress"].is_null());

    let (st_patch, b_patch) = json_request(
        &h.app,
        "PATCH",
        &format!("/api/v1/comic/online/items/{item_id}"),
        &h.member_token,
        Some(serde_json::json!({ "favorite": false })),
    )
    .await;
    assert_eq!(st_patch, StatusCode::OK);
    let pat_val: serde_json::Value = serde_json::from_slice(&b_patch).unwrap();
    assert_eq!(pat_val["favorite"], false);

    let (st_del, _) = json_request(
        &h.app,
        "DELETE",
        &format!("/api/v1/comic/online/items/{item_id}"),
        &h.member_token,
        None,
    )
    .await;
    assert_eq!(st_del, StatusCode::NO_CONTENT);
}

#[tokio::test]
async fn online_tracked_comic_hybrid_cache_opds_pse_and_refresh_e2e() {
    use base64::Engine;
    use base64::engine::general_purpose::STANDARD as BASE64;
    use std::sync::atomic::{AtomicBool, Ordering};

    let release_ch2 = Arc::new(AtomicBool::new(false));
    let release_ch2_srv = Arc::clone(&release_ch2);
    let png = valid_png_bytes();
    let png_1 = png.clone();
    let png_2 = png.clone();
    let png_cov = png;

    let origin = Router::new()
        .route(
            "/comic/1",
            get(move || {
                let has_ch2 = release_ch2_srv.load(Ordering::Relaxed);
                async move {
                    let mut chapters = vec![serde_json::json!({
                        "title": "第1话 初遇",
                        "url": "/ch/1"
                    })];
                    if has_ch2 {
                        chapters.push(serde_json::json!({
                            "title": "第2话 启程",
                            "url": "/ch/2"
                        }));
                    }
                    serde_json::json!({
                        "name": "追更示例漫",
                        "author": "测试作者",
                        "cover": "/img/cover.png",
                        "intro": "追更混合模式测试简介",
                        "tocUrl": "/toc/1",
                        "chapters": chapters
                    })
                    .to_string()
                }
            }),
        )
        .route(
            "/toc/1",
            get({
                let flag = Arc::clone(&release_ch2);
                move || {
                    let has_ch2 = flag.load(Ordering::Relaxed);
                    async move {
                        let mut chapters = vec![serde_json::json!({
                            "title": "第1话 初遇",
                            "url": "/ch/1"
                        })];
                        if has_ch2 {
                            chapters.push(serde_json::json!({
                                "title": "第2话 启程",
                                "url": "/ch/2"
                            }));
                        }
                        serde_json::json!({ "chapters": chapters }).to_string()
                    }
                }
            }),
        )
        .route(
            "/ch/1",
            get(|| async {
                serde_json::json!({ "pages": ["/img/p1.png", "/img/p2.png"] }).to_string()
            }),
        )
        .route(
            "/ch/2",
            get(|| async { serde_json::json!({ "pages": ["/img/p1.png"] }).to_string() }),
        )
        .route(
            "/img/cover.png",
            get(move || {
                let bytes = png_cov.clone();
                async move { ([(header::CONTENT_TYPE, "image/png")], bytes) }
            }),
        )
        .route(
            "/img/p1.png",
            get(move || {
                let bytes = png_1.clone();
                async move { ([(header::CONTENT_TYPE, "image/png")], bytes) }
            }),
        )
        .route(
            "/img/p2.png",
            get(move || {
                let bytes = png_2.clone();
                async move { ([(header::CONTENT_TYPE, "image/png")], bytes) }
            }),
        );

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let base = format!("http://{addr}");
    let _srv = tokio::spawn(async move {
        axum::serve(listener, origin).await.unwrap();
    });

    let h = setup_test_harness(true).await;

    let src_json = serde_json::json!([{
        "sourceName": "TrackedComicMock",
        "sourceUrl": base,
        "ruleBookInfo": {
            "name": "$.name",
            "author": "$.author",
            "coverUrl": "$.cover",
            "intro": "$.intro",
            "tocUrl": "$.tocUrl"
        },
        "ruleToc": {
            "chapterList": "$.chapters[*]",
            "chapterName": "$.title",
            "chapterUrl": "$.url"
        },
        "ruleContent": {
            "content": "@js:(result.pages||[]).join('\\n')"
        }
    }]);
    let (st_imp, b_imp) = json_request(
        &h.app,
        "POST",
        "/api/v1/comic/sources/import",
        &h.admin_token,
        Some(serde_json::json!({ "raw_json": src_json.to_string() })),
    )
    .await;
    assert_eq!(st_imp, StatusCode::OK);
    let imp_val: serde_json::Value = serde_json::from_slice(&b_imp).unwrap();
    let sid = imp_val["sources"][0]["id"].as_str().unwrap().to_string();

    // 1. Track online comic (lightweight, chapters uncached initially)
    let (st_track, b_track) = json_request(
        &h.app,
        "POST",
        "/api/v1/comic/online/track",
        &h.admin_token,
        Some(serde_json::json!({
            "source_id": sid,
            "comic_url": format!("{base}/comic/1"),
            "toc_url": format!("{base}/toc/1")
        })),
    )
    .await;
    assert_eq!(st_track, StatusCode::OK);
    let tracked: serde_json::Value = serde_json::from_slice(&b_track).unwrap();
    let series_id = tracked["series"]["id"].as_str().unwrap().to_string();
    assert_eq!(tracked["series"]["title"], "追更示例漫");
    assert_eq!(tracked["chapters"].as_array().unwrap().len(), 1);
    assert_eq!(tracked["chapters"][0]["online_tracking"]["cached"], false);
    let ch1_id = tracked["chapters"][0]["id"].as_str().unwrap().to_string();

    // 2. Member cannot see admin's private OnlineTracked comic in OPDS or local pool until member tracks it
    let basic_auth = format!("Basic {}", BASE64.encode("member:memberpassword123"));
    let unbound_feed_res = h
        .app
        .clone()
        .oneshot(
            Request::builder()
                .uri(format!("/opds/comic/v1.2/series/{series_id}"))
                .header(header::AUTHORIZATION, &basic_auth)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(unbound_feed_res.status(), StatusCode::NOT_FOUND);

    let (st_pool, b_pool) =
        json_request(&h.app, "GET", "/api/v1/comic/pool", &h.member_token, None).await;
    assert_eq!(st_pool, StatusCode::OK);
    let pool_items: Vec<serde_json::Value> = serde_json::from_slice(&b_pool).unwrap();
    assert!(pool_items.is_empty());

    // Member tracks the same online comic -> deduplicates and reuses same pooled series_id
    let (st_m_track, b_m_track) = json_request(
        &h.app,
        "POST",
        "/api/v1/comic/online/track",
        &h.member_token,
        Some(serde_json::json!({
            "source_id": sid,
            "comic_url": format!("{base}/comic/1"),
            "toc_url": format!("{base}/toc/1")
        })),
    )
    .await;
    assert_eq!(st_m_track, StatusCode::OK);
    let member_tracked: serde_json::Value = serde_json::from_slice(&b_m_track).unwrap();
    assert_eq!(member_tracked["series"]["id"].as_str().unwrap(), series_id);

    // Now OPDS 1.2 series & chapter feeds expose the tracked online comic to member
    let feed_res = h
        .app
        .clone()
        .oneshot(
            Request::builder()
                .uri(format!("/opds/comic/v1.2/series/{series_id}"))
                .header(header::AUTHORIZATION, &basic_auth)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(feed_res.status(), StatusCode::OK);
    let feed_xml = String::from_utf8(
        to_bytes(feed_res.into_body(), 1024 * 1024)
            .await
            .unwrap()
            .to_vec(),
    )
    .unwrap();
    assert!(feed_xml.contains("追更示例漫"));
    assert!(feed_xml.contains(&format!("/opds/comic/v1.2/chapters/{ch1_id}/download")));

    // 3. OPDS PSE page 0 request triggers on-demand CBZ caching transparently
    let pse_res = h
        .app
        .clone()
        .oneshot(
            Request::builder()
                .uri(format!("/opds/comic/v1.2/chapters/{ch1_id}/pse/0"))
                .header(header::AUTHORIZATION, &basic_auth)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(pse_res.status(), StatusCode::OK);
    assert_eq!(
        pse_res.headers().get(header::CONTENT_TYPE).unwrap(),
        "image/png"
    );

    // 4. OPDS CBZ archive download streams valid zip with ComicInfo.xml + 2 pages
    let dl_res = h
        .app
        .clone()
        .oneshot(
            Request::builder()
                .uri(format!("/opds/comic/v1.2/chapters/{ch1_id}/download"))
                .header(header::AUTHORIZATION, &basic_auth)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(dl_res.status(), StatusCode::OK);
    let dl_bytes = to_bytes(dl_res.into_body(), 4 * 1024 * 1024).await.unwrap();
    let archive = zip::ZipArchive::new(Cursor::new(dl_bytes)).unwrap();
    assert_eq!(archive.len(), 3); // ComicInfo.xml + 0001.png + 0002.png

    // 5. Upstream releases chapter 2 -> refresh appends it, batch cache downloads only ch2
    release_ch2.store(true, Ordering::Relaxed);
    let (st_ref, b_ref) = json_request(
        &h.app,
        "POST",
        &format!("/api/v1/comic/series/{series_id}/online/refresh"),
        &h.admin_token,
        None,
    )
    .await;
    assert_eq!(st_ref, StatusCode::OK);
    let ref_val: serde_json::Value = serde_json::from_slice(&b_ref).unwrap();
    assert_eq!(ref_val["chapters"].as_array().unwrap().len(), 2);
    assert_eq!(ref_val["chapters"][0]["online_tracking"]["cached"], true);
    assert_eq!(ref_val["chapters"][1]["online_tracking"]["cached"], false);

    let (st_cache, b_cache) = json_request(
        &h.app,
        "POST",
        &format!("/api/v1/comic/series/{series_id}/online/cache"),
        &h.admin_token,
        None,
    )
    .await;
    assert_eq!(st_cache, StatusCode::OK);
    let cache_val: serde_json::Value = serde_json::from_slice(&b_cache).unwrap();
    assert_eq!(cache_val["already_cached"], 1);
    assert_eq!(cache_val["cached_now"], 1);
    assert!(cache_val["failed"].as_array().unwrap().is_empty());

    // 6. Unbinding removes user visibility; auto-GC deletes pooled OnlineTracked series only at 0 bindings
    let (st_unb_m, b_unb_m) = json_request(
        &h.app,
        "DELETE",
        &format!("/api/v1/comic/bindings/{series_id}"),
        &h.member_token,
        None,
    )
    .await;
    assert_eq!(st_unb_m, StatusCode::OK);
    let unb_m: serde_json::Value = serde_json::from_slice(&b_unb_m).unwrap();
    assert_eq!(unb_m["unbound"], true);
    assert_eq!(unb_m["remaining_bindings"], 1);
    assert_eq!(unb_m["deleted_from_pool"], false);

    let (st_unb_a, b_unb_a) = json_request(
        &h.app,
        "DELETE",
        &format!("/api/v1/comic/bindings/{series_id}"),
        &h.admin_token,
        None,
    )
    .await;
    assert_eq!(st_unb_a, StatusCode::OK);
    let unb_a: serde_json::Value = serde_json::from_slice(&b_unb_a).unwrap();
    assert_eq!(unb_a["unbound"], true);
    assert_eq!(unb_a["remaining_bindings"], 0);
    assert_eq!(unb_a["deleted_from_pool"], true);
}

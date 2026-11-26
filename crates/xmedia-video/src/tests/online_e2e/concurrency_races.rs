use std::sync::Arc;

use axum::Router;
use axum::body::Body;
use axum::extract::Query;
use axum::http::{HeaderMap, StatusCode, header};
use axum::response::Response;
use axum::routing::get;
use serde::Deserialize;

use super::{get_bytes_with_auth, patch_json, post_json, setup_test_app};

#[derive(Debug, Default, Deserialize)]
struct DelayedSearchQuery {
    wd: Option<String>,
}

#[tokio::test]
async fn test_source_concurrent_login_action_preserves_updated_state_case_b_e2e() {
    let (_tmp, app, admin_tok, _member_tok) = setup_test_app(true).await;

    let entered = Arc::new(tokio::sync::Notify::new());
    let release = Arc::new(tokio::sync::Notify::new());
    let entered_srv = entered.clone();
    let release_srv = release.clone();

    let origin_app = Router::new().route(
        "/delayed/api.php/provide/vod/",
        get(move |Query(q): Query<DelayedSearchQuery>| {
            let entered_inner = entered_srv.clone();
            let release_inner = release_srv.clone();
            async move {
                if q.wd.as_deref() == Some("Delayed") {
                    entered_inner.notify_one();
                    let _ = tokio::time::timeout(
                        std::time::Duration::from_secs(5),
                        release_inner.notified(),
                    )
                    .await;
                }
                Response::builder()
                    .status(StatusCode::OK)
                    .header("content-type", "application/json")
                    .body(Body::from(
                        serde_json::json!({
                            "code": 1,
                            "page": 1,
                            "pagecount": 1,
                            "total": 1,
                            "list": [{
                                "vod_id": 9901,
                                "vod_name": "Delayed Movie",
                                "type_name": "电影",
                                "vod_remarks": "HD"
                            }]
                        })
                        .to_string(),
                    ))
                    .unwrap()
            }
        }),
    );

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let origin = format!("http://{addr}");
    let srv = tokio::spawn(async move {
        axum::serve(listener, origin_app).await.unwrap();
    });

    let src_json = serde_json::json!({
        "sourceName": "Delayed Mock AppleCMS",
        "sourceUrl": format!("{origin}/delayed/api.php/provide/vod/"),
        "sourceFormat": "applecms_v10",
        "apiUrl": format!("{origin}/delayed/api.php/provide/vod/"),
        "enabled": true,
        "searchable": true,
        "loginUi": "[{\"name\":\"username\",\"type\":\"text\"},{\"name\":\"apiToken\",\"type\":\"password\"},{\"name\":\"varVal\",\"type\":\"text\"}]",
        "loginUrl": "function doLogin(){ source.setVariable(String(source.getLoginInfoMap()['varVal'] || '')); return 'ok'; }"
    });

    let (st, import_res) = post_json(
        &app,
        "/api/v1/video/sources/import",
        &admin_tok,
        serde_json::json!({ "raw_json": src_json.to_string() }),
    )
    .await;
    assert_eq!(st, StatusCode::OK);
    let sid = import_res["sources"][0]["id"].as_str().unwrap().to_string();

    let (st, login1) = post_json(
        &app,
        &format!("/api/v1/video/sources/{sid}/login-action"),
        &admin_tok,
        serde_json::json!({
            "login_info": {
                "username": "alice_1",
                "apiToken": "secret_1",
                "varVal": "var_1"
            },
            "action": "doLogin()"
        }),
    )
    .await;
    assert_eq!(st, StatusCode::OK);
    assert_eq!(login1["result"], "ok");
    assert_eq!(login1["source"]["login_info"]["username"], "alice_1");

    let app_for_test = app.clone();
    let tok_for_test = admin_tok.clone();
    let sid_for_test = sid.clone();
    let test_task = tokio::spawn(async move {
        post_json(
            &app_for_test,
            &format!("/api/v1/video/sources/{sid_for_test}/test"),
            &tok_for_test,
            serde_json::json!({ "mode": "search", "keyword": "Delayed" }),
        )
        .await
    });

    tokio::time::timeout(std::time::Duration::from_secs(5), entered.notified())
        .await
        .expect("delayed mock search request should reach upstream");

    let (st, login2) = post_json(
        &app,
        &format!("/api/v1/video/sources/{sid}/login-action"),
        &admin_tok,
        serde_json::json!({
            "login_info": {
                "username": "bob_2",
                "apiToken": "secret_2",
                "varVal": "var_2"
            },
            "action": "doLogin()"
        }),
    )
    .await;
    assert_eq!(st, StatusCode::OK);
    assert_eq!(login2["result"], "ok");

    release.notify_one();
    let (test_st, test_res) = test_task.await.unwrap();
    assert_eq!(test_st, StatusCode::OK);
    assert_eq!(test_res["ok"], true);

    let (st, _, var_bytes) = get_bytes_with_auth(
        &app,
        &format!("/api/v1/video/sources/{sid}/variable"),
        Some(&admin_tok),
        None,
    )
    .await;
    assert_eq!(st, StatusCode::OK);
    let var_json: serde_json::Value = serde_json::from_slice(&var_bytes).unwrap();
    assert_eq!(var_json["variable"], "var_2");

    let (st, _, list_bytes) =
        get_bytes_with_auth(&app, "/api/v1/video/sources", Some(&admin_tok), None).await;
    assert_eq!(st, StatusCode::OK);
    let sources: Vec<serde_json::Value> = serde_json::from_slice(&list_bytes).unwrap();
    let rec = sources
        .iter()
        .find(|s| s["id"].as_str() == Some(sid.as_str()))
        .unwrap();
    assert_eq!(rec["login_info"]["username"], "bob_2");
    assert_eq!(rec["login_info"]["varVal"], "var_2");
    assert_eq!(rec["login_info"]["apiToken"], "••••••••");

    let app_for_search = app.clone();
    let tok_for_search = admin_tok.clone();
    let sid_for_search = sid.clone();
    let search_task = tokio::spawn(async move {
        post_json(
            &app_for_search,
            "/api/v1/video/online/search",
            &tok_for_search,
            serde_json::json!({ "keyword": "Delayed", "source_ids": [sid_for_search] }),
        )
        .await
    });

    tokio::time::timeout(std::time::Duration::from_secs(5), entered.notified())
        .await
        .expect("delayed mock search request should reach upstream");

    let (st, _) = post_json(
        &app,
        &format!("/api/v1/video/sources/{sid}/login-action"),
        &admin_tok,
        serde_json::json!({
            "login_info": {
                "username": "carol_3",
                "apiToken": "secret_3",
                "varVal": "var_3"
            },
            "action": "doLogin()"
        }),
    )
    .await;
    assert_eq!(st, StatusCode::OK);

    release.notify_one();
    let (search_st, search_res) = search_task.await.unwrap();
    assert_eq!(search_st, StatusCode::OK);
    assert_eq!(search_res["results"].as_array().unwrap().len(), 1);

    let (st, _, var_bytes2) = get_bytes_with_auth(
        &app,
        &format!("/api/v1/video/sources/{sid}/variable"),
        Some(&admin_tok),
        None,
    )
    .await;
    assert_eq!(st, StatusCode::OK);
    let var_json2: serde_json::Value = serde_json::from_slice(&var_bytes2).unwrap();
    assert_eq!(var_json2["variable"], "var_3");

    let (st, _, list_bytes2) =
        get_bytes_with_auth(&app, "/api/v1/video/sources", Some(&admin_tok), None).await;
    assert_eq!(st, StatusCode::OK);
    let sources2: Vec<serde_json::Value> = serde_json::from_slice(&list_bytes2).unwrap();
    let rec2 = sources2
        .iter()
        .find(|s| s["id"].as_str() == Some(sid.as_str()))
        .unwrap();
    assert_eq!(rec2["login_info"]["username"], "carol_3");
    assert_eq!(rec2["login_info"]["varVal"], "var_3");

    srv.abort();
}

fn race_login_source_json(origin: &str, weight: i64, group_name: &str) -> serde_json::Value {
    serde_json::json!({
        "sourceName": "Concurrent Race Source",
        "sourceUrl": format!("{origin}/race/api.php/provide/vod/"),
        "sourceGroup": group_name,
        "weight": weight,
        "sourceFormat": "applecms_v10",
        "apiUrl": format!("{origin}/race/api.php/provide/vod/"),
        "enabled": true,
        "searchable": true,
        "loginUi": "[{\"name\":\"username\",\"type\":\"text\"},{\"name\":\"apiToken\",\"type\":\"password\"},{\"name\":\"varVal\",\"type\":\"text\"}]",
        "loginUrl": "function doLogin(){ var info = source.getLoginInfoMap(); var u = String(info['username'] || ''); var v = String(info['varVal'] || ''); source.setVariable(v); cookie.setCookie('https://login-race.example', 'tok=' + u); cache.put('k_user', u); return u; } function readCache(){ return String(cache.get('k_user') || '') + '|' + String(cookie.getCookie('https://login-race.example', 'tok') || ''); }"
    })
}

#[tokio::test]
async fn patch_weight_concurrent_with_login_action_has_zero_lost_updates_e2e() {
    let (_tmp, app, admin_tok, _member_tok) = setup_test_app(true).await;

    let src_json = race_login_source_json("https://race-weight.example", 0, "race");
    let (st, import_res) = post_json(
        &app,
        "/api/v1/video/sources/import",
        &admin_tok,
        serde_json::json!({ "raw_json": src_json.to_string() }),
    )
    .await;
    assert_eq!(st, StatusCode::OK);
    let sid = import_res["sources"][0]["id"].as_str().unwrap().to_string();

    for round in 1..=60i64 {
        let user_val = format!("user_{round}");
        let var_val = format!("var_{round}");
        let secret_val = format!("secret_{round}");
        let delay_us = ((round as u64) % 20) * 100;

        let app_login = app.clone();
        let tok_login = admin_tok.clone();
        let sid_login = sid.clone();
        let u_clone = user_val.clone();
        let v_clone = var_val.clone();
        let s_clone = secret_val.clone();
        let login_task = tokio::spawn(async move {
            post_json(
                &app_login,
                &format!("/api/v1/video/sources/{sid_login}/login-action"),
                &tok_login,
                serde_json::json!({
                    "login_info": {
                        "username": u_clone,
                        "apiToken": s_clone,
                        "varVal": v_clone
                    },
                    "action": "doLogin()"
                }),
            )
            .await
        });

        let app_patch = app.clone();
        let tok_patch = admin_tok.clone();
        let sid_patch = sid.clone();
        let patch_task = tokio::spawn(async move {
            if delay_us > 0 {
                tokio::time::sleep(std::time::Duration::from_micros(delay_us)).await;
            }
            patch_json(
                &app_patch,
                &format!("/api/v1/video/sources/{sid_patch}"),
                &tok_patch,
                serde_json::json!({ "weight": round }),
            )
            .await
        });

        let (login_res, patch_res) = tokio::join!(login_task, patch_task);
        let (login_st, _) = login_res.unwrap();
        let (patch_st, _) = patch_res.unwrap();
        assert_eq!(login_st, StatusCode::OK);
        assert_eq!(patch_st, StatusCode::OK);

        let (st, _, var_bytes) = get_bytes_with_auth(
            &app,
            &format!("/api/v1/video/sources/{sid}/variable"),
            Some(&admin_tok),
            None,
        )
        .await;
        assert_eq!(st, StatusCode::OK);
        let var_json: serde_json::Value = serde_json::from_slice(&var_bytes).unwrap();
        assert_eq!(
            var_json["variable"].as_str(),
            Some(var_val.as_str()),
            "round {round} (delay={delay_us}us) lost variable update"
        );

        let (st, _, list_bytes) =
            get_bytes_with_auth(&app, "/api/v1/video/sources", Some(&admin_tok), None).await;
        assert_eq!(st, StatusCode::OK);
        let sources: Vec<serde_json::Value> = serde_json::from_slice(&list_bytes).unwrap();
        let rec = sources
            .iter()
            .find(|s| s["id"].as_str() == Some(sid.as_str()))
            .unwrap();
        assert_eq!(
            rec["weight"].as_i64(),
            Some(round),
            "round {round} lost weight update"
        );
        assert_eq!(
            rec["login_info"]["username"].as_str(),
            Some(user_val.as_str()),
            "round {round} lost login_info username"
        );
        assert_eq!(
            rec["login_info"]["varVal"].as_str(),
            Some(var_val.as_str()),
            "round {round} lost login_info varVal"
        );
        assert_eq!(rec["login_info"]["apiToken"], "••••••••");

        let (st, cache_res) = post_json(
            &app,
            &format!("/api/v1/video/sources/{sid}/login-action"),
            &admin_tok,
            serde_json::json!({ "login_info": {}, "action": "readCache()" }),
        )
        .await;
        assert_eq!(st, StatusCode::OK);
        let expected_cache_cookie = format!("{user_val}|{user_val}");
        assert_eq!(
            cache_res["result"].as_str(),
            Some(expected_cache_cookie.as_str()),
            "round {round} lost cache/cookie update"
        );
    }
}

#[tokio::test]
async fn patch_variable_concurrent_with_login_action_preserves_login_cookies_cache_e2e() {
    let (_tmp, app, admin_tok, _member_tok) = setup_test_app(true).await;

    let src_json = race_login_source_json("https://race-var.example", 10, "race");
    let (st, import_res) = post_json(
        &app,
        "/api/v1/video/sources/import",
        &admin_tok,
        serde_json::json!({ "raw_json": src_json.to_string() }),
    )
    .await;
    assert_eq!(st, StatusCode::OK);
    let sid = import_res["sources"][0]["id"].as_str().unwrap().to_string();

    for round in 1..=60i64 {
        let user_val = format!("user_{round}");
        let var_val = format!("var_{round}");
        let patch_var = format!("v_patch_{round}");
        let secret_val = format!("secret_{round}");
        let delay_us = ((round as u64) % 20) * 100;

        let app_login = app.clone();
        let tok_login = admin_tok.clone();
        let sid_login = sid.clone();
        let u_clone = user_val.clone();
        let v_clone = var_val.clone();
        let s_clone = secret_val.clone();
        let login_task = tokio::spawn(async move {
            post_json(
                &app_login,
                &format!("/api/v1/video/sources/{sid_login}/login-action"),
                &tok_login,
                serde_json::json!({
                    "login_info": {
                        "username": u_clone,
                        "apiToken": s_clone,
                        "varVal": v_clone
                    },
                    "action": "doLogin()"
                }),
            )
            .await
        });

        let app_patch = app.clone();
        let tok_patch = admin_tok.clone();
        let sid_patch = sid.clone();
        let pv_clone = patch_var.clone();
        let patch_task = tokio::spawn(async move {
            if delay_us > 0 {
                tokio::time::sleep(std::time::Duration::from_micros(delay_us)).await;
            }
            patch_json(
                &app_patch,
                &format!("/api/v1/video/sources/{sid_patch}"),
                &tok_patch,
                serde_json::json!({ "variable": pv_clone }),
            )
            .await
        });

        let (login_res, patch_res) = tokio::join!(login_task, patch_task);
        let (login_st, _) = login_res.unwrap();
        let (patch_st, _) = patch_res.unwrap();
        assert_eq!(login_st, StatusCode::OK);
        assert_eq!(patch_st, StatusCode::OK);

        let (st, _, var_bytes) = get_bytes_with_auth(
            &app,
            &format!("/api/v1/video/sources/{sid}/variable"),
            Some(&admin_tok),
            None,
        )
        .await;
        assert_eq!(st, StatusCode::OK);
        let var_json: serde_json::Value = serde_json::from_slice(&var_bytes).unwrap();
        let got_var = var_json["variable"].as_str().unwrap_or_default();
        assert!(
            got_var == var_val || got_var == patch_var,
            "round {round} (delay={delay_us}us) reverted variable to stale value: {got_var}"
        );

        let (st, _, list_bytes) =
            get_bytes_with_auth(&app, "/api/v1/video/sources", Some(&admin_tok), None).await;
        assert_eq!(st, StatusCode::OK);
        let sources: Vec<serde_json::Value> = serde_json::from_slice(&list_bytes).unwrap();
        let rec = sources
            .iter()
            .find(|s| s["id"].as_str() == Some(sid.as_str()))
            .unwrap();
        assert_eq!(
            rec["login_info"]["username"].as_str(),
            Some(user_val.as_str()),
            "round {round} lost login_info username"
        );
        assert_eq!(
            rec["login_info"]["varVal"].as_str(),
            Some(var_val.as_str()),
            "round {round} lost login_info varVal"
        );

        let (st, cache_res) = post_json(
            &app,
            &format!("/api/v1/video/sources/{sid}/login-action"),
            &admin_tok,
            serde_json::json!({ "login_info": {}, "action": "readCache()" }),
        )
        .await;
        assert_eq!(st, StatusCode::OK);
        let expected_cache_cookie = format!("{user_val}|{user_val}");
        assert_eq!(
            cache_res["result"].as_str(),
            Some(expected_cache_cookie.as_str()),
            "round {round} lost cache/cookie update"
        );
    }
}

#[tokio::test]
async fn import_upsert_concurrent_with_login_action_preserves_runtime_state_e2e() {
    let (_tmp, app, admin_tok, _member_tok) = setup_test_app(true).await;

    let initial_json = race_login_source_json("https://race-import.example", 0, "g_0");
    let (st, import_res) = post_json(
        &app,
        "/api/v1/video/sources/import",
        &admin_tok,
        serde_json::json!({ "raw_json": initial_json.to_string() }),
    )
    .await;
    assert_eq!(st, StatusCode::OK);
    let sid = import_res["sources"][0]["id"].as_str().unwrap().to_string();

    for round in 1..=20i64 {
        let user_val = format!("user_{round}");
        let var_val = format!("var_{round}");
        let group_val = format!("g_{round}");
        let delay_us = ((round as u64) % 10) * 150;
        let upsert_json = race_login_source_json("https://race-import.example", round, &group_val);

        let app_login = app.clone();
        let tok_login = admin_tok.clone();
        let sid_login = sid.clone();
        let u_clone = user_val.clone();
        let v_clone = var_val.clone();
        let login_task = tokio::spawn(async move {
            post_json(
                &app_login,
                &format!("/api/v1/video/sources/{sid_login}/login-action"),
                &tok_login,
                serde_json::json!({
                    "login_info": {
                        "username": u_clone,
                        "apiToken": "secret",
                        "varVal": v_clone
                    },
                    "action": "doLogin()"
                }),
            )
            .await
        });

        let app_import = app.clone();
        let tok_import = admin_tok.clone();
        let import_task = tokio::spawn(async move {
            if delay_us > 0 {
                tokio::time::sleep(std::time::Duration::from_micros(delay_us)).await;
            }
            post_json(
                &app_import,
                "/api/v1/video/sources/import",
                &tok_import,
                serde_json::json!({ "raw_json": upsert_json.to_string() }),
            )
            .await
        });

        let (login_res, import_res) = tokio::join!(login_task, import_task);
        assert_eq!(login_res.unwrap().0, StatusCode::OK);
        assert_eq!(import_res.unwrap().0, StatusCode::OK);

        let (st, _, var_bytes) = get_bytes_with_auth(
            &app,
            &format!("/api/v1/video/sources/{sid}/variable"),
            Some(&admin_tok),
            None,
        )
        .await;
        assert_eq!(st, StatusCode::OK);
        let var_json: serde_json::Value = serde_json::from_slice(&var_bytes).unwrap();
        assert_eq!(var_json["variable"].as_str(), Some(var_val.as_str()));

        let (st, _, list_bytes) =
            get_bytes_with_auth(&app, "/api/v1/video/sources", Some(&admin_tok), None).await;
        assert_eq!(st, StatusCode::OK);
        let sources: Vec<serde_json::Value> = serde_json::from_slice(&list_bytes).unwrap();
        let rec = sources
            .iter()
            .find(|s| s["id"].as_str() == Some(sid.as_str()))
            .unwrap();
        assert_eq!(rec["weight"].as_i64(), Some(round));
        assert_eq!(rec["group_name"].as_str(), Some(group_val.as_str()));
        assert_eq!(
            rec["login_info"]["username"].as_str(),
            Some(user_val.as_str())
        );

        let (st, cache_res) = post_json(
            &app,
            &format!("/api/v1/video/sources/{sid}/login-action"),
            &admin_tok,
            serde_json::json!({ "login_info": {}, "action": "readCache()" }),
        )
        .await;
        assert_eq!(st, StatusCode::OK);
        let expected = format!("{user_val}|{user_val}");
        assert_eq!(cache_res["result"].as_str(), Some(expected.as_str()));
    }
}

#[tokio::test]
async fn same_host_browse_set_cookie_concurrent_with_login_action_preserves_both_e2e() {
    let (_tmp, app, admin_tok, _member_tok) = setup_test_app(true).await;

    let entered = Arc::new(tokio::sync::Notify::new());
    let release = Arc::new(tokio::sync::Notify::new());
    let entered_srv = entered.clone();
    let release_srv = release.clone();

    let origin_app = Router::new()
        .route(
            "/browse",
            get(move || {
                let entered_inner = entered_srv.clone();
                let release_inner = release_srv.clone();
                async move {
                    entered_inner.notify_one();
                    let _ = tokio::time::timeout(
                        std::time::Duration::from_secs(5),
                        release_inner.notified(),
                    )
                    .await;
                    Response::builder()
                        .status(StatusCode::OK)
                        .header("content-type", "application/json")
                        .header(
                            header::SET_COOKIE,
                            "cf_bm=browse_cookie_123; Path=/; HttpOnly",
                        )
                        .body(Body::from(
                            serde_json::json!({
                                "list": [{ "id": "vid-1", "title": "SameHost Movie" }]
                            })
                            .to_string(),
                        ))
                        .unwrap()
                }
            }),
        )
        .route(
            "/echo",
            get(|headers: HeaderMap| async move {
                let cookie_hdr = headers
                    .get(header::COOKIE)
                    .and_then(|v| v.to_str().ok())
                    .unwrap_or("")
                    .to_string();
                Response::builder()
                    .status(StatusCode::OK)
                    .header("content-type", "application/json")
                    .body(Body::from(
                        serde_json::json!({ "cookie": cookie_hdr }).to_string(),
                    ))
                    .unwrap()
            }),
        );

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let origin = format!("http://{addr}");
    let srv = tokio::spawn(async move {
        axum::serve(listener, origin_app).await.unwrap();
    });

    let login_script = format!(
        "function setSession() {{ cookie.setCookie('{origin}', 'session_id=' + String(result.sid || '')); return 'ok'; }} \
         function readCookies() {{ return String(cookie.getCookie('{origin}', 'session_id') || '') + '|' + String(cookie.getCookie('{origin}', 'cf_bm') || ''); }} \
         function probeEcho() {{ return String(java.ajax('{origin}/echo')); }}"
    );
    let src_json = serde_json::json!({
        "sourceName": "Same Host Cookie Source",
        "sourceUrl": origin,
        "sourceFormat": "rule",
        "enabled": true,
        "enabledExplore": true,
        "enabledCookieJar": true,
        "exploreUrl": "/browse",
        "ruleExplore": {
            "list": "$.list",
            "name": "$.title",
            "vodId": "$.id"
        },
        "loginUrl": login_script
    });

    let (st, import_res) = post_json(
        &app,
        "/api/v1/video/sources/import",
        &admin_tok,
        serde_json::json!({ "raw_json": src_json.to_string() }),
    )
    .await;
    assert_eq!(st, StatusCode::OK);
    let sid = import_res["sources"][0]["id"].as_str().unwrap().to_string();

    let (st, _) = post_json(
        &app,
        &format!("/api/v1/video/sources/{sid}/login-action"),
        &admin_tok,
        serde_json::json!({ "login_info": { "sid": "old_session" }, "action": "setSession()" }),
    )
    .await;
    assert_eq!(st, StatusCode::OK);

    let app_browse = app.clone();
    let tok_browse = admin_tok.clone();
    let sid_browse = sid.clone();
    let browse_task = tokio::spawn(async move {
        post_json(
            &app_browse,
            "/api/v1/video/online/browse",
            &tok_browse,
            serde_json::json!({ "source_id": sid_browse, "page": 1 }),
        )
        .await
    });

    tokio::time::timeout(std::time::Duration::from_secs(5), entered.notified())
        .await
        .expect("delayed browse request should reach upstream");

    let (st, _) = post_json(
        &app,
        &format!("/api/v1/video/sources/{sid}/login-action"),
        &admin_tok,
        serde_json::json!({ "login_info": { "sid": "new_session" }, "action": "setSession()" }),
    )
    .await;
    assert_eq!(st, StatusCode::OK);

    release.notify_one();
    let (browse_st, browse_res) = browse_task.await.unwrap();
    assert_eq!(browse_st, StatusCode::OK);
    assert_eq!(browse_res["results"][0]["title"], "SameHost Movie");

    let (st, read_res) = post_json(
        &app,
        &format!("/api/v1/video/sources/{sid}/login-action"),
        &admin_tok,
        serde_json::json!({ "login_info": {}, "action": "readCookies()" }),
    )
    .await;
    assert_eq!(st, StatusCode::OK);
    assert_eq!(read_res["result"], "new_session|browse_cookie_123");

    let (st, echo_res) = post_json(
        &app,
        &format!("/api/v1/video/sources/{sid}/login-action"),
        &admin_tok,
        serde_json::json!({ "login_info": {}, "action": "probeEcho()" }),
    )
    .await;
    assert_eq!(st, StatusCode::OK);
    let echo_json: serde_json::Value =
        serde_json::from_str(echo_res["result"].as_str().unwrap()).unwrap();
    let sent_cookie = echo_json["cookie"].as_str().unwrap();
    assert!(sent_cookie.contains("session_id=new_session"));
    assert!(sent_cookie.contains("cf_bm=browse_cookie_123"));
    assert!(!sent_cookie.contains("old_session"));

    srv.abort();
}

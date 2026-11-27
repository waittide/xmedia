use axum::Router;
use axum::body::Body;
use axum::http::{Request, StatusCode, header};
use axum::response::Response;
use axum::routing::get;
use tower::ServiceExt;
use uuid::Uuid;

use super::{
    PNG_1X1, delayed_chunked_response, get_bytes_with_auth, post_json, setup_test_app,
    start_mock_video_origin, urlencoding_param,
};

#[tokio::test]
async fn video_online_sources_crud_rbac_applecms_rule_and_proxies_e2e() {
    let (_tmp, app, admin_tok, member_tok) = setup_test_app(true).await;
    let (origin, captured_headers, mock_handle) = start_mock_video_origin().await;

    // 1. Import sources using testdata/online_sources.json templates rewritten to mock origin
    let seed_sources: Vec<serde_json::Value> =
        serde_json::from_str(include_str!("../../../testdata/online_sources.json")).unwrap();
    assert_eq!(seed_sources.len(), 6);

    let mut peertube_src = seed_sources[4].clone();
    peertube_src["sourceUrl"] = serde_json::json!(format!("{origin}/peertube"));
    peertube_src["categoriesUrl"] =
        serde_json::json!(format!("{origin}/peertube/api/v1/videos/categories"));
    peertube_src["searchUrl"] = serde_json::json!(format!(
        "<js>'{origin}/peertube/api/v1/search/videos?search=' + encodeURIComponent(key) + '&count=20&start=' + ((page - 1) * 20)</js>"
    ));
    peertube_src["exploreUrl"] = serde_json::json!(format!(
        "<js>'{origin}/peertube/api/v1/videos?count=20&start=' + ((page - 1) * 20) + (catId ? '&categoryOneOf=' + encodeURIComponent(catId) : '')</js>"
    ));
    peertube_src["ruleSearch"]["coverUrl"] =
        serde_json::json!(format!("{origin}{{{{$.thumbnailPath}}}}"));
    peertube_src["ruleSearch"]["detailUrl"] =
        serde_json::json!(format!("{origin}/peertube/api/v1/videos/{{{{$.uuid}}}}"));
    peertube_src["ruleDetail"]["coverUrl"] =
        serde_json::json!(format!("{origin}{{{{$.thumbnailPath}}}}"));

    let mut archive_src = seed_sources[5].clone();
    archive_src["sourceUrl"] = serde_json::json!(format!("{origin}/archive"));
    archive_src["searchUrl"] = serde_json::json!(format!(
        "<js>'{origin}/archive/advancedsearch.php?q=' + encodeURIComponent('mediatype:movies AND (' + key + ')') + '&output=json'</js>"
    ));
    archive_src["exploreUrl"] = serde_json::json!(format!(
        "<js>var q = 'mediatype:movies' + (catId ? ' AND collection:' + catId : ''); '{origin}/archive/advancedsearch.php?q=' + encodeURIComponent(q) + '&output=json';</js>"
    ));
    archive_src["ruleSearch"]["coverUrl"] =
        serde_json::json!(format!("{origin}/posters/earth2.png"));
    archive_src["ruleSearch"]["detailUrl"] =
        serde_json::json!(format!("{origin}/archive/metadata/{{{{$.identifier}}}}"));
    archive_src["ruleDetail"]["coverUrl"] =
        serde_json::json!(format!("{origin}/posters/earth2.png"));
    archive_src["ruleDetail"]["episodes"] = serde_json::json!(format!(
        "<js>(function(){{ var dir = result.dir || ''; var files = Array.isArray(result.files) ? result.files : []; var mp4s = files.filter(function(f){{ return f && f.name && /\\.mp4$/i.test(String(f.name)); }}); return mp4s.map(function(f){{ var cleanPath = String(f.name).split('/').map(encodeURIComponent).join('/'); return {{ name: String(f.name).split('/').pop(), url: '{origin}' + dir + '/' + cleanPath }}; }}); }})()</js>"
    ));

    let applecms_ok = serde_json::json!({
        "sourceName": "Mock AppleCMS",
        "sourceUrl": format!("{origin}/applecms/api.php/provide/vod/"),
        "sourceGroup": "测试分组",
        "sourceFormat": "applecms_v10",
        "apiUrl": format!("{origin}/applecms/api.php/provide/vod/"),
        "enabled": true,
        "searchable": true,
        "weight": 100,
        "checkKeyword": "流浪地球",
        "preferredPlayFrom": ["ikm3u8"],
        "mediaHeader": "{\"Referer\":\"https://player.example.com/\",\"X-Custom-Token\":\"secret-media-tok\"}",
        "loginUi": "[{\"name\":\"token\",\"type\":\"password\"}]",
        "loginUrl": "function login(){ var u = String(source.getLoginInfoMap()['token'] || ''); source.setVariable('auth=' + u); return 'ok'; }"
    });
    let applecms_nosearch = serde_json::json!({
        "sourceName": "Mock Suoni NoSearch",
        "apiUrl": format!("{origin}/suoni/api.php/provide/vod/"),
        "sourceGroup": "测试分组",
        "enabled": true,
        "searchable": false,
        "weight": 80,
        "preferredPlayFrom": ["snm3u8"]
    });
    let disabled_src = serde_json::json!({
        "sourceName": "Disabled Source",
        "apiUrl": format!("{origin}/suoni/api.php/provide/vod/"),
        "enabled": false,
        "weight": 10
    });

    let batch_json = serde_json::Value::Array(vec![
        applecms_ok,
        applecms_nosearch,
        peertube_src,
        archive_src,
        disabled_src,
    ]);
    let (st, import_res) = post_json(
        &app,
        "/api/v1/plugins/import?domain=video",
        &admin_tok,
        serde_json::json!({ "raw_json": batch_json.to_string() }),
    )
    .await;
    assert_eq!(st, StatusCode::OK);
    assert_eq!(import_res["imported_count"], 5);

    // Re-import idempotency check
    let (st, reimport_res) = post_json(
        &app,
        "/api/v1/plugins/import?domain=video",
        &admin_tok,
        serde_json::json!({ "raw_json": batch_json.to_string() }),
    )
    .await;
    assert_eq!(st, StatusCode::OK);
    assert_eq!(reimport_res["imported_count"], 0);
    assert_eq!(reimport_res["updated_count"], 5);

    // List sources & extract IDs
    let (st, _, list_bytes) =
        get_bytes_with_auth(&app, "/api/v1/plugins?domain=video", Some(&admin_tok), None).await;
    assert_eq!(st, StatusCode::OK);
    let admin_sources: Vec<serde_json::Value> = serde_json::from_slice(&list_bytes).unwrap();
    assert_eq!(admin_sources.len(), 5);

    let find_id = |sources: &[serde_json::Value], name: &str| -> Uuid {
        let s = sources
            .iter()
            .find(|v| v["name"] == name)
            .expect("source exists")["id"]
            .as_str()
            .unwrap();
        Uuid::parse_str(s).unwrap()
    };
    let apple_id = find_id(&admin_sources, "Mock AppleCMS");
    let suoni_id = find_id(&admin_sources, "Mock Suoni NoSearch");
    let peertube_id = find_id(&admin_sources, "PeerTube (Framatube)");
    let archive_id = find_id(&admin_sources, "Internet Archive Video");
    let disabled_id = find_id(&admin_sources, "Disabled Source");

    // 2. Per-user Plugin & Runtime State Isolation + Secret Masking
    // Before importing, member sees an empty plugin list and gets 404 on admin's source IDs
    let (st, _, member_list_bytes) = get_bytes_with_auth(
        &app,
        "/api/v1/plugins?domain=video",
        Some(&member_tok),
        None,
    )
    .await;
    assert_eq!(st, StatusCode::OK);
    let member_sources: Vec<serde_json::Value> =
        serde_json::from_slice(&member_list_bytes).unwrap();
    assert!(member_sources.is_empty());

    let (st, _, _) = get_bytes_with_auth(
        &app,
        &format!("/api/v1/plugins/{apple_id}"),
        Some(&member_tok),
        None,
    )
    .await;
    assert_eq!(st, StatusCode::NOT_FOUND);
    let (st, _, _) = get_bytes_with_auth(
        &app,
        &format!("/api/v1/plugins/{apple_id}/variable"),
        Some(&member_tok),
        None,
    )
    .await;
    assert_eq!(st, StatusCode::NOT_FOUND);
    let (st, _, _) = get_bytes_with_auth(
        &app,
        &format!("/api/v1/plugins/{apple_id}/export"),
        Some(&member_tok),
        None,
    )
    .await;
    assert_eq!(st, StatusCode::NOT_FOUND);
    let (st, _) = post_json(
        &app,
        &format!("/api/v1/plugins/{apple_id}/test"),
        &member_tok,
        serde_json::json!({"mode":"categories"}),
    )
    .await;
    assert_eq!(st, StatusCode::NOT_FOUND);
    let (st, _) = post_json(
        &app,
        &format!("/api/v1/plugins/{apple_id}/login-action"),
        &member_tok,
        serde_json::json!({"login_info": {"token": "hijack"}, "action": "login()"}),
    )
    .await;
    assert_eq!(st, StatusCode::NOT_FOUND);
    let (st, _) = post_json(
        &app,
        "/api/v1/plugins/export-batch?domain=video",
        &member_tok,
        serde_json::json!({ "ids": [apple_id] }),
    )
    .await;
    assert_eq!(st, StatusCode::NOT_FOUND);

    // Batch action by member on admin's IDs affects 0 items and leaves admin's plugins intact
    let (st, member_batch_on_admin) = post_json(
        &app,
        "/api/v1/plugins/batch",
        &member_tok,
        serde_json::json!({ "ids": [apple_id, suoni_id], "action": "delete" }),
    )
    .await;
    assert_eq!(st, StatusCode::OK);
    assert_eq!(member_batch_on_admin["affected_count"], 0);
    let (st, _, admin_check_bytes) =
        get_bytes_with_auth(&app, "/api/v1/plugins?domain=video", Some(&admin_tok), None).await;
    assert_eq!(st, StatusCode::OK);
    let admin_check: Vec<serde_json::Value> = serde_json::from_slice(&admin_check_bytes).unwrap();
    assert_eq!(admin_check.len(), 5);

    // Login action stores variable & masks sensitive password field in summary
    let (st, login_res) = post_json(
        &app,
        &format!("/api/v1/plugins/{apple_id}/login-action"),
        &admin_tok,
        serde_json::json!({
            "login_info": {"token": "super-secret-val"},
            "action": "login()"
        }),
    )
    .await;
    assert_eq!(st, StatusCode::OK);
    assert_eq!(login_res["result"], "ok");
    assert_eq!(login_res["source"]["login_info"]["token"], "••••••••");

    let (st, _, var_bytes) = get_bytes_with_auth(
        &app,
        &format!("/api/v1/plugins/{apple_id}/variable"),
        Some(&admin_tok),
        None,
    )
    .await;
    assert_eq!(st, StatusCode::OK);
    let var_json: serde_json::Value = serde_json::from_slice(&var_bytes).unwrap();
    assert_eq!(var_json["variable"], "auth=super-secret-val");

    // Member can import & manage their own isolated plugins without inheriting admin's runtime state
    let (st, m_imp_res) = post_json(
        &app,
        "/api/v1/plugins/import?domain=video",
        &member_tok,
        serde_json::json!({ "raw_json": batch_json.to_string() }),
    )
    .await;
    assert_eq!(st, StatusCode::OK);
    assert_eq!(m_imp_res["imported_count"], 5);

    let (st, _, member_list_bytes2) = get_bytes_with_auth(
        &app,
        "/api/v1/plugins?domain=video",
        Some(&member_tok),
        None,
    )
    .await;
    assert_eq!(st, StatusCode::OK);
    let member_sources2: Vec<serde_json::Value> =
        serde_json::from_slice(&member_list_bytes2).unwrap();
    assert_eq!(member_sources2.len(), 5);

    let m_apple_id = find_id(&member_sources2, "Mock AppleCMS");
    let m_suoni_id = find_id(&member_sources2, "Mock Suoni NoSearch");
    let m_peertube_id = find_id(&member_sources2, "PeerTube (Framatube)");
    let m_archive_id = find_id(&member_sources2, "Internet Archive Video");
    assert_ne!(m_apple_id, apple_id);

    let member_apple = member_sources2
        .iter()
        .find(|s| s["name"] == "Mock AppleCMS")
        .unwrap();
    assert!(member_apple["login_info"].as_object().unwrap().is_empty());
    assert_eq!(member_apple["login_ui"].as_array().unwrap().len(), 1);
    assert_eq!(member_apple["variable_preview"], "");

    let (st, m_login_res) = post_json(
        &app,
        &format!("/api/v1/plugins/{m_apple_id}/login-action"),
        &member_tok,
        serde_json::json!({
            "login_info": {"token": "member-own-secret"},
            "action": "login()"
        }),
    )
    .await;
    assert_eq!(st, StatusCode::OK);
    assert_eq!(m_login_res["result"], "ok");
    assert_eq!(m_login_res["source"]["login_info"]["token"], "••••••••");

    let (st, _, m_var_bytes) = get_bytes_with_auth(
        &app,
        &format!("/api/v1/plugins/{m_apple_id}/variable"),
        Some(&member_tok),
        None,
    )
    .await;
    assert_eq!(st, StatusCode::OK);
    let m_var_json: serde_json::Value = serde_json::from_slice(&m_var_bytes).unwrap();
    assert_eq!(m_var_json["variable"], "auth=member-own-secret");

    let (st, _, admin_var_after) = get_bytes_with_auth(
        &app,
        &format!("/api/v1/plugins/{apple_id}/variable"),
        Some(&admin_tok),
        None,
    )
    .await;
    assert_eq!(st, StatusCode::OK);
    let admin_var_json: serde_json::Value = serde_json::from_slice(&admin_var_after).unwrap();
    assert_eq!(admin_var_json["variable"], "auth=super-secret-val");

    // Reorder / batch / export-batch / patch / export
    let (st, _) = post_json(
        &app,
        "/api/v1/plugins/reorder?domain=video",
        &admin_tok,
        serde_json::json!({ "ordered_ids": [peertube_id, apple_id] }),
    )
    .await;
    assert_eq!(st, StatusCode::OK);

    let (st, export_batch_res) = post_json(
        &app,
        "/api/v1/plugins/export-batch?domain=video",
        &admin_tok,
        serde_json::json!({ "ids": [apple_id, peertube_id] }),
    )
    .await;
    assert_eq!(st, StatusCode::OK);
    assert_eq!(export_batch_res.as_array().unwrap().len(), 2);

    let patch_res = app
        .clone()
        .oneshot(
            Request::builder()
                .method("PATCH")
                .uri(format!("/api/v1/plugins/{archive_id}"))
                .header(header::AUTHORIZATION, format!("Bearer {admin_tok}"))
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(
                    serde_json::json!({ "weight": 89, "group_name": "归档精选" }).to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(patch_res.status(), StatusCode::OK);

    let (st, _, exp_bytes) = get_bytes_with_auth(
        &app,
        &format!("/api/v1/plugins/{archive_id}/export"),
        Some(&admin_tok),
        None,
    )
    .await;
    assert_eq!(st, StatusCode::OK);
    let exp_json: serde_json::Value = serde_json::from_slice(&exp_bytes).unwrap();
    assert_eq!(exp_json["weight"], 89);
    assert_eq!(exp_json["sourceGroup"], "归档精选");

    // 3. Multi-source Search (searches member's own enabled + searchable plugins)
    let (st, search_res) = post_json(
        &app,
        "/api/v1/video/online/search",
        &member_tok,
        serde_json::json!({ "keyword": "流浪", "page": 1 }),
    )
    .await;
    assert_eq!(st, StatusCode::OK);
    let results = search_res["results"].as_array().unwrap();
    assert!(results.len() >= 3);
    assert!(search_res["errors"].as_array().unwrap().is_empty());
    let pt_hit = results
        .iter()
        .find(|r| r["source_name"] == "PeerTube (Framatube)")
        .unwrap();
    assert_eq!(pt_hit["remarks"], "2:05");

    // Accessing another user's (admin's) source_id on online endpoints returns 404 Not Found
    let (st, _) = post_json(
        &app,
        "/api/v1/video/online/categories",
        &member_tok,
        serde_json::json!({ "source_id": apple_id }),
    )
    .await;
    assert_eq!(st, StatusCode::NOT_FOUND);
    let (st, _) = post_json(
        &app,
        "/api/v1/video/online/categories",
        &member_tok,
        serde_json::json!({ "source_id": disabled_id }),
    )
    .await;
    assert_eq!(st, StatusCode::NOT_FOUND);

    // 4. AppleCMS Categories -> Browse -> Detail (/share/ filtered out) -> Play
    let (st, cat_res) = post_json(
        &app,
        "/api/v1/video/online/categories",
        &member_tok,
        serde_json::json!({ "source_id": m_apple_id }),
    )
    .await;
    assert_eq!(st, StatusCode::OK);
    assert_eq!(cat_res["categories"].as_array().unwrap().len(), 2);

    let (st, browse_res) = post_json(
        &app,
        "/api/v1/video/online/browse",
        &member_tok,
        serde_json::json!({ "source_id": m_apple_id, "category_id": "33", "page": 1 }),
    )
    .await;
    assert_eq!(st, StatusCode::OK);
    assert_eq!(browse_res["results"][0]["title"], "科幻精选-33");

    let (st, detail_res) = post_json(
        &app,
        "/api/v1/video/online/detail",
        &member_tok,
        serde_json::json!({ "source_id": m_apple_id, "video_id": "71070" }),
    )
    .await;
    assert_eq!(st, StatusCode::OK);
    let groups = detail_res["play_groups"].as_array().unwrap();
    assert_eq!(groups.len(), 1);
    assert_eq!(groups[0]["name"], "ikm3u8");
    let play_url = groups[0]["episodes"][0]["play_url"].as_str().unwrap();
    assert!(play_url.ends_with("/applecms/api.php/provide/vod/redir/master.m3u8"));

    let (st, play_res) = post_json(
        &app,
        "/api/v1/video/online/play",
        &member_tok,
        serde_json::json!({ "source_id": m_apple_id, "play_url": play_url, "group_name": "ikm3u8" }),
    )
    .await;
    assert_eq!(st, StatusCode::OK);
    assert_eq!(play_res["stream_type"], "hls");
    assert_eq!(play_res["headers_applied"], true);
    let proxy_hls_url = play_res["proxy_stream_url"].as_str().unwrap();
    assert!(proxy_hls_url.starts_with("/api/v1/video/online/hls-proxy?"));

    // 5. Generic `rule` mode: PeerTube & Internet Archive
    let (st, pt_cats) = post_json(
        &app,
        "/api/v1/video/online/categories",
        &member_tok,
        serde_json::json!({ "source_id": m_peertube_id }),
    )
    .await;
    assert_eq!(st, StatusCode::OK);
    assert_eq!(pt_cats["categories"].as_array().unwrap().len(), 2);

    let (st, pt_browse) = post_json(
        &app,
        "/api/v1/video/online/browse",
        &member_tok,
        serde_json::json!({ "source_id": m_peertube_id, "page": 1 }),
    )
    .await;
    assert_eq!(st, StatusCode::OK);
    assert_eq!(pt_browse["results"][0]["title"], "PeerTube Browse Item");

    let (st, pt_detail) = post_json(
        &app,
        "/api/v1/video/online/detail",
        &member_tok,
        serde_json::json!({
            "source_id": m_peertube_id,
            "detail_url": format!("{origin}/peertube/api/v1/videos/pt-uuid-1")
        }),
    )
    .await;
    assert_eq!(st, StatusCode::OK);
    assert_eq!(
        pt_detail["play_groups"][0]["episodes"][0]["play_url"],
        format!("{origin}/cdn/v2/master.m3u8")
    );

    let (st, ia_cats) = post_json(
        &app,
        "/api/v1/video/online/categories",
        &member_tok,
        serde_json::json!({ "source_id": m_archive_id }),
    )
    .await;
    assert_eq!(st, StatusCode::OK);
    assert_eq!(ia_cats["categories"].as_array().unwrap().len(), 5);

    let (st, ia_detail) = post_json(
        &app,
        "/api/v1/video/online/detail",
        &member_tok,
        serde_json::json!({
            "source_id": m_archive_id,
            "detail_url": format!("{origin}/archive/metadata/BigBuckBunny_124")
        }),
    )
    .await;
    assert_eq!(st, StatusCode::OK);
    let ia_ep_url = ia_detail["play_groups"][0]["episodes"][0]["play_url"]
        .as_str()
        .unwrap();
    assert!(ia_ep_url.ends_with("/archive/files/BigBuckBunny_124/Content/big_buck_bunny_720p.mp4"));

    // 6. Test endpoint (`full`, `browse`) for both admin and member on their own plugins
    let (st, full_test) = post_json(
        &app,
        &format!("/api/v1/plugins/{apple_id}/test"),
        &admin_tok,
        serde_json::json!({ "mode": "full", "keyword": "流浪" }),
    )
    .await;
    assert_eq!(st, StatusCode::OK);
    assert_eq!(full_test["ok"], true);
    assert!(full_test["play_result"]["stream_type"] == "hls");

    let (st, suoni_test) = post_json(
        &app,
        &format!("/api/v1/plugins/{suoni_id}/test"),
        &admin_tok,
        serde_json::json!({ "mode": "browse" }),
    )
    .await;
    assert_eq!(st, StatusCode::OK);
    assert_eq!(suoni_test["ok"], true);

    let (st, m_suoni_test) = post_json(
        &app,
        &format!("/api/v1/plugins/{m_suoni_id}/test"),
        &member_tok,
        serde_json::json!({ "mode": "browse" }),
    )
    .await;
    assert_eq!(st, StatusCode::OK);
    assert_eq!(m_suoni_test["ok"], true);

    // 7. Poster proxy: raster OK + other user's source_id 404 + SVG/HTML rejected
    let (st, hdrs, png_body) = get_bytes_with_auth(
        &app,
        &format!(
            "/api/v1/video/online/poster-proxy?url={}&source_id={m_apple_id}",
            urlencoding_param(&format!("{origin}/posters/earth2.png"))
        ),
        Some(&member_tok),
        None,
    )
    .await;
    assert_eq!(st, StatusCode::OK);
    assert_eq!(png_body, PNG_1X1);
    assert_eq!(hdrs.get(header::X_CONTENT_TYPE_OPTIONS).unwrap(), "nosniff");

    let (st_foreign_poster, _, _) = get_bytes_with_auth(
        &app,
        &format!(
            "/api/v1/video/online/poster-proxy?url={}&source_id={apple_id}",
            urlencoding_param(&format!("{origin}/posters/earth2.png"))
        ),
        Some(&member_tok),
        None,
    )
    .await;
    assert_eq!(st_foreign_poster, StatusCode::NOT_FOUND);

    let (st, _, _) = get_bytes_with_auth(
        &app,
        &format!(
            "/api/v1/video/online/poster-proxy?url={}",
            urlencoding_param(&format!("{origin}/posters/evil.svg"))
        ),
        Some(&member_tok),
        None,
    )
    .await;
    assert_eq!(st, StatusCode::BAD_REQUEST);

    let (st, _, _) = get_bytes_with_auth(
        &app,
        &format!(
            "/api/v1/video/online/poster-proxy?url={}",
            urlencoding_param(&format!("{origin}/posters/html.jpg"))
        ),
        Some(&member_tok),
        None,
    )
    .await;
    assert_eq!(st, StatusCode::BAD_REQUEST);

    // 8. Stream proxy: 206 Range + server-side mediaHeader injection + no auth leak + 416 + HTML reject
    let (st, hdrs, part_bytes) = get_bytes_with_auth(
        &app,
        &format!(
            "/api/v1/video/online/stream-proxy?url={}&source_id={m_apple_id}",
            urlencoding_param(&format!("{origin}/media/movie.mp4"))
        ),
        Some(&member_tok),
        Some("bytes=0-15"),
    )
    .await;
    assert_eq!(st, StatusCode::PARTIAL_CONTENT);
    assert_eq!(part_bytes.len(), 16);
    assert_eq!(&part_bytes[4..12], b"ftypisom");
    assert_eq!(hdrs.get(header::CONTENT_RANGE).unwrap(), "bytes 0-15/4096");
    assert_eq!(hdrs.get(header::ACCEPT_RANGES).unwrap(), "bytes");
    assert_eq!(hdrs.get(header::X_CONTENT_TYPE_OPTIONS).unwrap(), "nosniff");

    {
        let cap = captured_headers.lock().unwrap();
        let last = cap.last().unwrap();
        assert_eq!(
            last.get("referer").map(String::as_str),
            Some("https://player.example.com/")
        );
        assert_eq!(
            last.get("x-custom-token").map(String::as_str),
            Some("secret-media-tok")
        );
        assert!(!last.contains_key("authorization"));
    }

    let (st, hdrs, _) = get_bytes_with_auth(
        &app,
        &format!(
            "/api/v1/video/online/stream-proxy?url={}&source_id={m_apple_id}",
            urlencoding_param(&format!("{origin}/media/movie.mp4"))
        ),
        Some(&member_tok),
        Some("bytes=999999-"),
    )
    .await;
    assert_eq!(st, StatusCode::RANGE_NOT_SATISFIABLE);
    assert_eq!(hdrs.get(header::CONTENT_RANGE).unwrap(), "bytes */4096");

    let (st, _, _) = get_bytes_with_auth(
        &app,
        &format!(
            "/api/v1/video/online/stream-proxy?url={}&source_id={m_apple_id}",
            urlencoding_param(&format!("{origin}/media/html-page.mp4"))
        ),
        Some(&member_tok),
        None,
    )
    .await;
    assert_eq!(st, StatusCode::BAD_REQUEST);

    // 9. HLS proxy
    let (st, hdrs, master_bytes) = get_bytes_with_auth(
        &app,
        &format!("{proxy_hls_url}&token={member_tok}"),
        None,
        None,
    )
    .await;
    assert_eq!(st, StatusCode::OK);
    assert_eq!(
        hdrs.get(header::CONTENT_TYPE).unwrap(),
        "application/vnd.apple.mpegurl; charset=utf-8"
    );
    let master_text = String::from_utf8(master_bytes).unwrap();
    let expected_sub = format!("{origin}/cdn/v2/5000kb/hls/index.m3u8");
    assert!(master_text.contains(&urlencoding_param(&expected_sub)));
    assert!(master_text.contains(&format!("&token={member_tok}")));
    assert!(
        master_text
            .contains("#EXT-X-SESSION-KEY:METHOD=AES-128,URI=\"/api/v1/video/online/hls-proxy?")
    );
    assert!(master_text.contains("#EXT-X-MEDIA:TYPE=SUBTITLES,GROUP-ID=\"subs\",NAME=\"zh\",URI=\"/api/v1/video/online/hls-proxy?"));
    assert!(master_text.contains(
        "#EXT-X-I-FRAME-STREAM-INF:BANDWIDTH=60000,URI=\"/api/v1/video/online/hls-proxy?"
    ));

    let sub_proxy_line = master_text
        .lines()
        .find(|l| {
            l.starts_with("/api/v1/video/online/hls-proxy?")
                && l.contains("5000kb%2Fhls%2Findex.m3u8")
        })
        .unwrap();
    let (st, _, media_bytes) = get_bytes_with_auth(&app, sub_proxy_line, None, None).await;
    assert_eq!(st, StatusCode::OK);
    let media_text = String::from_utf8(media_bytes).unwrap();
    assert!(media_text.contains("#EXT-X-KEY:METHOD=AES-128,URI=\"/api/v1/video/online/hls-proxy?"));
    assert!(media_text.contains("#EXT-X-MAP:URI=\"/api/v1/video/online/hls-proxy?"));
    assert!(media_text.contains("#EXT-X-BYTERANGE:376@0"));

    let seg_proxy_line = media_text
        .lines()
        .find(|l| l.starts_with("/api/v1/video/online/hls-proxy?") && l.contains("seg0.ts"))
        .unwrap();
    let (st, hdrs, seg_chunk) =
        get_bytes_with_auth(&app, seg_proxy_line, None, Some("bytes=0-187")).await;
    assert_eq!(st, StatusCode::PARTIAL_CONTENT);
    assert_eq!(seg_chunk.len(), 188);
    assert_eq!(seg_chunk[0], 0x47);
    assert_eq!(hdrs.get(header::CONTENT_TYPE).unwrap(), "video/mp2t");
    assert_eq!(hdrs.get(header::CONTENT_RANGE).unwrap(), "bytes 0-187/376");

    let missing_sid = Uuid::new_v4();
    for endpoint in ["stream-proxy", "hls-proxy", "subtitle-proxy"] {
        let target = urlencoding_param(&format!("{origin}/cdn/v2/master.m3u8"));
        let (st_no_sid, _, _) = get_bytes_with_auth(
            &app,
            &format!("/api/v1/video/online/{endpoint}?url={target}"),
            Some(&member_tok),
            None,
        )
        .await;
        assert_eq!(
            st_no_sid,
            StatusCode::BAD_REQUEST,
            "missing source_id must be rejected on {endpoint}"
        );

        let (st_unknown, _, _) = get_bytes_with_auth(
            &app,
            &format!("/api/v1/video/online/{endpoint}?url={target}&source_id={missing_sid}"),
            Some(&member_tok),
            None,
        )
        .await;
        assert_eq!(
            st_unknown,
            StatusCode::NOT_FOUND,
            "nonexistent source_id must be rejected on {endpoint}"
        );

        let (st_foreign, _, _) = get_bytes_with_auth(
            &app,
            &format!("/api/v1/video/online/{endpoint}?url={target}&source_id={apple_id}"),
            Some(&member_tok),
            None,
        )
        .await;
        assert_eq!(
            st_foreign,
            StatusCode::NOT_FOUND,
            "another user's source_id must return 404 on {endpoint}"
        );
    }

    let (st, _, _) = get_bytes_with_auth(
        &app,
        &format!(
            "/api/v1/video/online/hls-proxy?url={}&source_id={m_apple_id}",
            urlencoding_param(&format!("{origin}/cdn/oversized.m3u8"))
        ),
        Some(&member_tok),
        None,
    )
    .await;
    assert_eq!(st, StatusCode::BAD_REQUEST);

    let del_res = app
        .clone()
        .oneshot(
            Request::builder()
                .method("DELETE")
                .uri(format!("/api/v1/plugins/{suoni_id}"))
                .header(header::AUTHORIZATION, format!("Bearer {admin_tok}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(del_res.status(), StatusCode::NO_CONTENT);

    let (st, batch_del) = post_json(
        &app,
        "/api/v1/plugins/batch",
        &admin_tok,
        serde_json::json!({ "ids": [disabled_id], "action": "delete" }),
    )
    .await;
    assert_eq!(st, StatusCode::OK);
    assert_eq!(batch_del["affected_count"], 1);

    let (st, seed_imp) = post_json(
        &app,
        "/api/v1/plugins/import?domain=video",
        &admin_tok,
        serde_json::json!({ "raw_json": include_str!("../../../testdata/online_sources.json") }),
    )
    .await;
    assert_eq!(st, StatusCode::OK);
    assert_eq!(seed_imp["imported_count"], 6);

    mock_handle.abort();
}

#[tokio::test]
async fn ssrf_blocks_loopback_and_redirects_when_allow_private_ips_is_false() {
    let (_tmp, app, admin_tok, member_tok) = setup_test_app(false).await;
    let (origin, _, mock_handle) = start_mock_video_origin().await;

    let src_payload = serde_json::json!({
        "raw_json": serde_json::json!({
            "sourceName": "Loopback Source",
            "apiUrl": format!("{origin}/applecms/api.php/provide/vod/"),
            "enabled": true
        }).to_string()
    });
    let (st_adm, adm_imp) = post_json(
        &app,
        "/api/v1/plugins/import?domain=video",
        &admin_tok,
        src_payload.clone(),
    )
    .await;
    assert_eq!(st_adm, StatusCode::OK);
    let admin_sid = adm_imp["sources"][0]["id"].as_str().unwrap();

    // Member cannot use admin's source_id
    let (st_foreign, _, _) = get_bytes_with_auth(
        &app,
        &format!(
            "/api/v1/video/online/stream-proxy?url={}&source_id={admin_sid}",
            urlencoding_param(&format!("{origin}/media/movie.mp4"))
        ),
        Some(&member_tok),
        None,
    )
    .await;
    assert_eq!(st_foreign, StatusCode::NOT_FOUND);

    // Member imports their own plugin and hits SSRF guard
    let (st, imp) = post_json(
        &app,
        "/api/v1/plugins/import?domain=video",
        &member_tok,
        src_payload,
    )
    .await;
    assert_eq!(st, StatusCode::OK);
    let sid = imp["sources"][0]["id"].as_str().unwrap();

    let (st, _, _) = get_bytes_with_auth(
        &app,
        &format!(
            "/api/v1/video/online/stream-proxy?url={}&source_id={sid}",
            urlencoding_param(&format!("{origin}/media/movie.mp4"))
        ),
        Some(&member_tok),
        None,
    )
    .await;
    assert_eq!(st, StatusCode::BAD_REQUEST);

    let (st, _, _) = get_bytes_with_auth(
        &app,
        &format!(
            "/api/v1/video/online/hls-proxy?url={}&source_id={sid}",
            urlencoding_param(&format!("{origin}/cdn/v2/master.m3u8"))
        ),
        Some(&member_tok),
        None,
    )
    .await;
    assert_eq!(st, StatusCode::BAD_REQUEST);

    let (st, _, _) = get_bytes_with_auth(
        &app,
        &format!(
            "/api/v1/video/online/poster-proxy?url={}",
            urlencoding_param(&format!("{origin}/posters/earth2.png"))
        ),
        Some(&member_tok),
        None,
    )
    .await;
    assert_eq!(st, StatusCode::BAD_REQUEST);

    for blocked in [
        "http://localhost/video.mp4",
        "http://169.254.169.254/latest/meta-data/",
        "http://[::1]/video.mp4",
        "http://10.0.0.1/video.mp4",
        "file:///etc/passwd",
    ] {
        let (st, _, _) = get_bytes_with_auth(
            &app,
            &format!(
                "/api/v1/video/online/stream-proxy?url={}&source_id={sid}",
                urlencoding_param(blocked)
            ),
            Some(&member_tok),
            None,
        )
        .await;
        assert_eq!(
            st,
            StatusCode::BAD_REQUEST,
            "expected SSRF rejection for {blocked}"
        );
    }

    let (st, cat_err) = post_json(
        &app,
        "/api/v1/video/online/categories",
        &member_tok,
        serde_json::json!({ "source_id": sid }),
    )
    .await;
    assert_eq!(st, StatusCode::BAD_REQUEST);
    let _ = cat_err;

    mock_handle.abort();
}

#[tokio::test]
async fn proxy_prefix_sniffing_and_segment_ct_normalization_e2e() {
    let (_tmp, app, _admin_tok, member_tok) = setup_test_app(true).await;

    let origin_app = Router::new()
        .route(
            "/stream/chunked-bom",
            get(|| async {
                delayed_chunked_response(
                    "application/octet-stream",
                    vec![
                        (b"\xEF\xBB\xBF  \r\n".to_vec(), 0),
                        (
                            b"#EXTM3U\n#EXTINF:5.0,\nseg_chunked.ts\n#EXT-X-ENDLIST\n".to_vec(),
                            15,
                        ),
                    ],
                )
            }),
        )
        .route(
            "/stream/chunked-html",
            get(|| async {
                delayed_chunked_response(
                    "application/octet-stream",
                    vec![
                        (b"<ht".to_vec(), 0),
                        (b"ml><script>alert(1)</script></html>".to_vec(), 15),
                    ],
                )
            }),
        )
        .route(
            "/media/disguised-html.mp4",
            get(|| async {
                Response::builder()
                    .status(StatusCode::OK)
                    .header("content-type", "video/mp4")
                    .body(Body::from(
                        "<!DOCTYPE html><html><script>alert(1)</script></html>",
                    ))
                    .unwrap()
            }),
        )
        .route(
            "/media/disguised-svg.bin",
            get(|| async {
                Response::builder()
                    .status(StatusCode::OK)
                    .header("content-type", "application/octet-stream")
                    .body(Body::from(
                        "<svg xmlns=\"http://www.w3.org/2000/svg\"><script>alert(1)</script></svg>",
                    ))
                    .unwrap()
            }),
        )
        .route(
            "/media/chunked-mp4.mp4",
            get(|| async {
                delayed_chunked_response(
                    "Video/MP4",
                    vec![
                        (b"\x00\x00\x00\x18ftyp".to_vec(), 0),
                        (b"isom\x00\x00\x02\x00isomiso2".to_vec(), 10),
                    ],
                )
            }),
        )
        .route(
            "/stream/chunked-ts.ts",
            get(|| async {
                let mut rest = vec![0xAAu8; 184];
                rest[0] = 0x11;
                delayed_chunked_response(
                    "binary/octet-stream",
                    vec![(vec![0x47, 0x40, 0x00, 0x10], 0), (rest, 10)],
                )
            }),
        );

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let origin = format!("http://{addr}");
    let srv = tokio::spawn(async move {
        axum::serve(listener, origin_app).await.unwrap();
    });

    let (st_imp, imp_res) = post_json(
        &app,
        "/api/v1/plugins/import?domain=video",
        &member_tok,
        serde_json::json!({
            "raw_json": serde_json::json!({
                "sourceName": "SniffProxySource",
                "sourceUrl": origin,
                "apiUrl": format!("{origin}/api"),
                "enabled": true
            }).to_string()
        }),
    )
    .await;
    assert_eq!(st_imp, StatusCode::OK);
    let sid = imp_res["sources"][0]["id"].as_str().unwrap();

    let (st, hdrs, body) = get_bytes_with_auth(
        &app,
        &format!(
            "/api/v1/video/online/hls-proxy?url={}&source_id={sid}",
            urlencoding_param(&format!("{origin}/stream/chunked-bom"))
        ),
        Some(&member_tok),
        None,
    )
    .await;
    assert_eq!(st, StatusCode::OK);
    assert_eq!(
        hdrs.get(header::CONTENT_TYPE).unwrap(),
        "application/vnd.apple.mpegurl; charset=utf-8"
    );
    let playlist = String::from_utf8(body).unwrap();
    assert!(
        playlist.starts_with("#EXTM3U\n"),
        "expected rewritten playlist to start on line 1 with #EXTM3U, got: {playlist:?}"
    );
    assert!(playlist.contains(&format!(
        "/api/v1/video/online/hls-proxy?url={}",
        urlencoding_param(&format!("{origin}/stream/seg_chunked.ts"))
    )));

    let (st, _, _) = get_bytes_with_auth(
        &app,
        &format!(
            "/api/v1/video/online/hls-proxy?url={}&source_id={sid}",
            urlencoding_param(&format!("{origin}/stream/chunked-html"))
        ),
        Some(&member_tok),
        None,
    )
    .await;
    assert_eq!(st, StatusCode::BAD_REQUEST);

    let (st, _, _) = get_bytes_with_auth(
        &app,
        &format!(
            "/api/v1/video/online/stream-proxy?url={}&source_id={sid}",
            urlencoding_param(&format!("{origin}/stream/chunked-html"))
        ),
        Some(&member_tok),
        None,
    )
    .await;
    assert_eq!(st, StatusCode::BAD_REQUEST);

    let (st, _, _) = get_bytes_with_auth(
        &app,
        &format!(
            "/api/v1/video/online/stream-proxy?url={}&source_id={sid}",
            urlencoding_param(&format!("{origin}/media/disguised-html.mp4"))
        ),
        Some(&member_tok),
        None,
    )
    .await;
    assert_eq!(st, StatusCode::BAD_REQUEST);

    let (st, _, _) = get_bytes_with_auth(
        &app,
        &format!(
            "/api/v1/video/online/stream-proxy?url={}&source_id={sid}",
            urlencoding_param(&format!("{origin}/media/disguised-svg.bin"))
        ),
        Some(&member_tok),
        None,
    )
    .await;
    assert_eq!(st, StatusCode::BAD_REQUEST);

    let (st, hdrs, mp4_bytes) = get_bytes_with_auth(
        &app,
        &format!(
            "/api/v1/video/online/stream-proxy?url={}&source_id={sid}",
            urlencoding_param(&format!("{origin}/media/chunked-mp4.mp4"))
        ),
        Some(&member_tok),
        None,
    )
    .await;
    assert_eq!(st, StatusCode::OK);
    assert_eq!(hdrs.get(header::CONTENT_TYPE).unwrap(), "video/mp4");
    assert_eq!(
        mp4_bytes,
        b"\x00\x00\x00\x18ftypisom\x00\x00\x02\x00isomiso2"
    );

    let (st, hdrs, ts_bytes) = get_bytes_with_auth(
        &app,
        &format!(
            "/api/v1/video/online/hls-proxy?url={}&source_id={sid}",
            urlencoding_param(&format!("{origin}/stream/chunked-ts.ts"))
        ),
        Some(&member_tok),
        None,
    )
    .await;
    assert_eq!(st, StatusCode::OK);
    assert_eq!(hdrs.get(header::CONTENT_TYPE).unwrap(), "video/mp2t");
    assert_eq!(ts_bytes.len(), 188);
    assert_eq!(&ts_bytes[..5], &[0x47, 0x40, 0x00, 0x10, 0x11]);

    srv.abort();
}

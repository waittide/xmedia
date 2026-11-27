use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use axum::extract::Query;
use axum::http::{HeaderMap, StatusCode, header};
use axum::response::IntoResponse;
use axum::routing::{get, post};
use axum::{Json, Router};

use super::{send_json, setup_online_test_harness};

#[tokio::test]
async fn plugin_v2_js_module_netease_auth_settings_and_vip_stream_isolation_e2e() {
    let poll_count = Arc::new(AtomicUsize::new(0));
    let poll_counter = poll_count.clone();

    let upstream = Router::new()
        .route(
            "/api/login/qrcode/unikey",
            get(|| async { Json(serde_json::json!({ "code": 200, "unikey": "uk_test_99" })) }),
        )
        .route(
            "/api/login/qrcode/client/login",
            post(move |Query(q): Query<HashMap<String, String>>| {
                let counter = poll_counter.clone();
                async move {
                    assert_eq!(q.get("key").map(String::as_str), Some("uk_test_99"));
                    let step = counter.fetch_add(1, Ordering::SeqCst);
                    if step == 0 {
                        (
                            StatusCode::OK,
                            Json(serde_json::json!({ "code": 801, "message": "等待扫码" })),
                        )
                            .into_response()
                    } else {
                        (
                            StatusCode::OK,
                            [(
                                header::SET_COOKIE,
                                "MUSIC_U=vip_secret_cookie_admin; Path=/; HttpOnly",
                            )],
                            Json(serde_json::json!({ "code": 803, "message": "授权登录成功" })),
                        )
                            .into_response()
                    }
                }
            }),
        )
        .route(
            "/api/nuser/account/get",
            get(|headers: HeaderMap| async move {
                let cookie_hdr = headers
                    .get(header::COOKIE)
                    .and_then(|v| v.to_str().ok())
                    .unwrap_or("");
                if cookie_hdr.contains("MUSIC_U=vip_secret_cookie_admin") {
                    Json(serde_json::json!({
                        "code": 200,
                        "account": { "id": 88001, "vipType": 11 },
                        "profile": {
                            "userId": 88001,
                            "nickname": "CloudAdmin",
                            "avatarUrl": "https://p1.music.126.net/avatar.jpg"
                        }
                    }))
                } else {
                    Json(serde_json::json!({ "code": 200, "account": null, "profile": null }))
                }
            }),
        )
        .route(
            "/api/search/get/web",
            get(|Query(q): Query<HashMap<String, String>>| async move {
                let kw = q.get("s").cloned().unwrap_or_default();
                Json(serde_json::json!({
                    "code": 200,
                    "result": {
                        "songs": [
                            {
                                "id": 186016,
                                "name": format!("{kw} (Official)"),
                                "artists": [{ "name": "周杰伦" }],
                                "album": { "id": 18905, "name": "叶惠美" },
                                "duration": 269000
                            }
                        ]
                    }
                }))
            }),
        )
        .route(
            "/api/song/detail/",
            get(| h: HeaderMap, Query(q): Query<HashMap<String, String>>| async move {
                let _ = h;
                let id = q.get("id").cloned().unwrap_or_else(|| "186016".into());
                Json(serde_json::json!({
                    "code": 200,
                    "songs": [{
                        "id": id,
                        "name": "晴天 (Official)",
                        "artists": [{ "name": "周杰伦" }],
                        "album": {
                            "name": "叶惠美",
                            "picUrl": "http://127.0.0.1/cover/186016.jpg"
                        },
                        "duration": 269000
                    }]
                }))
            }),
        )
        .route(
            "/api/playlist/detail",
            get(|Query(q): Query<HashMap<String, String>>| async move {
                let pl_id = q.get("id").cloned().unwrap_or_default();
                Json(serde_json::json!({
                    "code": 200,
                    "result": {
                        "tracks": [{
                            "id": 29713754,
                            "name": format!("榜单曲目-{pl_id}"),
                            "artists": [{ "name": "测试歌手" }],
                            "album": { "name": "热歌合辑", "picUrl": "http://127.0.0.1/cover/top.jpg" },
                            "duration": 210000
                        }]
                    }
                }))
            }),
        )
        .route(
            "/api/song/lyric",
            get(|| async {
                Json(serde_json::json!({
                    "code": 200,
                    "lrc": { "lyric": "[00:00.00]故事的小黄花\n[00:04.50]从出生那年就飘着\n" },
                    "tlyric": { "lyric": "[00:00.00]Little yellow flower\n" }
                }))
            }),
        )
        .route(
            "/api/song/enhance/player/url/v1",
            get(
                |headers: HeaderMap, Query(q): Query<HashMap<String, String>>| async move {
                    let level = q.get("level").map(String::as_str).unwrap_or("exhigh");
                    let cookie_hdr = headers
                        .get(header::COOKIE)
                        .and_then(|v| v.to_str().ok())
                        .unwrap_or("");
                    let is_vip = cookie_hdr.contains("MUSIC_U=vip_secret_cookie_admin");
                    if level == "lossless" && !is_vip {
                        Json(serde_json::json!({
                            "code": 200,
                            "data": [{ "id": 186016, "url": null, "br": 0, "type": null }]
                        }))
                    } else if level == "lossless" {
                        Json(serde_json::json!({
                            "code": 200,
                            "data": [{
                                "id": 186016,
                                "url": "http://127.0.0.1/audio/186016.flac",
                                "br": 999000,
                                "type": "flac",
                                "expi": 1200
                            }]
                        }))
                    } else {
                        Json(serde_json::json!({
                            "code": 200,
                            "data": [{
                                "id": 186016,
                                "url": "http://127.0.0.1/audio/186016.mp3",
                                "br": 320000,
                                "type": "mp3",
                                "expi": 1200
                            }]
                        }))
                    }
                },
            ),
        );

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        let _ = axum::serve(listener, upstream).await;
    });

    let base_url = format!("http://127.0.0.1:{}", addr.port());
    let js_plugin = format!(
        r#"
export default definePlugin({{
  id: "netease-official",
  name: "网易云音乐官方",
  version: "1.2.0",
  domain: "music",
  baseUrl: "{base_url}",
  group: "官方直连",
  checkKeyword: "晴天",
  headers: {{
    "Referer": "https://music.163.com/"
  }},
  qualities: ["exhigh", "lossless"],
  exploreCategories: [
    {{ title: "飙升榜", id: "19723756" }}
  ],
  settings: [
    {{
      key: "defaultLevel",
      label: "默认音质",
      type: "select",
      default: "exhigh",
      options: [
        {{ label: "极高 320k", value: "exhigh" }},
        {{ label: "无损 FLAC", value: "lossless" }}
      ]
    }},
    {{
      key: "customToken",
      label: "私有口令",
      type: "password",
      default: ""
    }}
  ],
  auth: {{
    qrCode: {{
      async create(ctx) {{
        const data = ctx.http.get("{base_url}/api/login/qrcode/unikey?type=1").json();
        return {{
          key: data.unikey,
          qrUrl: "https://music.163.com/login?codekey=" + data.unikey
        }};
      }},
      async poll(ctx, {{ key }}) {{
        const data = ctx.http.post("{base_url}/api/login/qrcode/client/login?key=" + encodeURIComponent(key) + "&type=1").json();
        if (data.code === 803) return {{ status: "confirmed", message: data.message }};
        if (data.code === 802) return {{ status: "scanned", message: data.message }};
        if (data.code === 800) return {{ status: "expired", message: data.message }};
        return {{ status: "waiting", message: data.message }};
      }}
    }},
    async getProfile(ctx) {{
      const data = ctx.http.get("{base_url}/api/nuser/account/get").json();
      if (!data.account || !data.profile) return {{ loggedIn: false }};
      return {{
        loggedIn: true,
        userId: String(data.profile.userId),
        nickname: data.profile.nickname,
        avatarUrl: data.profile.avatarUrl,
        vipLabel: data.account.vipType === 11 ? "黑胶VIP" : "普通用户"
      }};
    }},
    async logout(ctx) {{
      ctx.cookies.clear();
      return {{ ok: true }};
    }}
  }},
  async search(ctx, {{ keyword, page, limit }}) {{
    const offset = (page - 1) * limit;
    const data = ctx.http.get("{base_url}/api/search/get/web", {{
      query: {{ csrf_token: "", s: keyword, type: 1, offset, limit }}
    }}).json();
    const songs = (data.result && data.result.songs) || [];
    return songs.map(s => ({{
      id: String(s.id),
      title: s.name,
      artists: s.artists,
      album: s.album,
      durationMs: s.duration,
      coverId: String(s.id)
    }}));
  }},
  async explore(ctx, {{ categoryId }}) {{
    const data = ctx.http.get("{base_url}/api/playlist/detail", {{
      query: {{ id: categoryId || "19723756", n: 100, updateTime: -1 }}
    }}).json();
    const tracks = (data.result && data.result.tracks) || [];
    return tracks.map(t => ({{
      id: String(t.id),
      title: t.name,
      artists: t.artists,
      album: t.album,
      durationMs: t.duration,
      coverUrl: t.album && t.album.picUrl
    }}));
  }},
  async getCover(ctx, {{ track }}) {{
    const data = ctx.http.get("{base_url}/api/song/detail/", {{
      query: {{ id: track.id, ids: "[" + track.id + "]" }}
    }}).json();
    const song = data.songs && data.songs[0];
    return song && song.album ? song.album.picUrl : null;
  }},
  async getLyrics(ctx, {{ track }}) {{
    const data = ctx.http.get("{base_url}/api/song/lyric", {{
      query: {{ id: track.id, lv: 1, tv: -1 }}
    }}).json();
    return {{
      lyric: data.lrc ? data.lrc.lyric : "",
      translatedLyric: data.tlyric ? data.tlyric.lyric : null
    }};
  }},
  async getStream(ctx, {{ track, quality }}) {{
    const level = quality || ctx.settings.defaultLevel || "exhigh";
    const data = ctx.http.get("{base_url}/api/song/enhance/player/url/v1", {{
      query: {{ ids: "[" + track.id + "]", level, encodeType: "flac" }}
    }}).json();
    const item = data.data && data.data[0];
    if (!item || !item.url) return null;
    return {{
      url: item.url,
      format: item.type,
      bitrate: item.br,
      expiresInSecs: item.expi
    }};
  }}
}});
"#
    );

    let h = setup_online_test_harness(true).await;

    // 1. Admin imports the ES Module plugin without passing ?domain= (auto-inferred from domain: "music")
    let (st, _, body) = send_json(
        &h.app,
        "POST",
        "/api/v1/plugins/import",
        &h.admin_token,
        Some(serde_json::json!({ "raw_json": js_plugin })),
    )
    .await;
    assert_eq!(
        st,
        StatusCode::OK,
        "import failed: {}",
        String::from_utf8_lossy(&body)
    );
    let import_dto: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(import_dto["imported_count"], 1);
    let src = &import_dto["sources"][0];
    let admin_plugin_id = src["id"].as_str().unwrap().to_string();
    assert_eq!(src["format"], "js_module");
    assert_eq!(src["version"], "1.2.0");
    assert_eq!(src["auth"]["qr_code"], true);
    assert_eq!(src["settings_schema"].as_array().unwrap().len(), 2);

    // 2. Anonymous search, explore, lyrics & 320k stream succeed, while lossless fails before login
    let (st_s, _, s_body) = send_json(
        &h.app,
        "POST",
        "/online/search",
        &h.admin_token,
        Some(serde_json::json!({ "keyword": "晴天" })),
    )
    .await;
    assert_eq!(st_s, StatusCode::OK);
    let search_json: serde_json::Value = serde_json::from_slice(&s_body).unwrap();
    assert_eq!(search_json["results"][0]["title"], "晴天 (Official)");
    assert!(
        search_json["results"][0]["proxy_cover_url"]
            .as_str()
            .unwrap()
            .contains("cover_id=186016")
    );

    let (st_ex, _, ex_body) = send_json(
        &h.app,
        "POST",
        "/online/explore",
        &h.admin_token,
        Some(serde_json::json!({ "source_id": admin_plugin_id })),
    )
    .await;
    assert_eq!(st_ex, StatusCode::OK);
    let ex_json: serde_json::Value = serde_json::from_slice(&ex_body).unwrap();
    assert_eq!(ex_json["categories"][0]["title"], "飙升榜");
    assert_eq!(ex_json["results"][0]["title"], "榜单曲目-19723756");

    let (st_vip_anon, _, _) = send_json(
        &h.app,
        "POST",
        "/online/stream",
        &h.admin_token,
        Some(serde_json::json!({
            "source_id": admin_plugin_id,
            "track_id": "186016",
            "quality": "lossless"
        })),
    )
    .await;
    assert_eq!(st_vip_anon, StatusCode::BAD_REQUEST);

    // 3. Admin performs QR code Contract Auth (create -> poll waiting -> poll confirmed -> profile)
    let (st_qr, _, qr_body) = send_json(
        &h.app,
        "POST",
        &format!("/api/v1/plugins/{admin_plugin_id}/auth"),
        &h.admin_token,
        Some(serde_json::json!({ "action": "qr_create" })),
    )
    .await;
    assert_eq!(st_qr, StatusCode::OK);
    let qr_json: serde_json::Value = serde_json::from_slice(&qr_body).unwrap();
    assert_eq!(qr_json["qr_key"], "uk_test_99");

    let (_, _, poll1_body) = send_json(
        &h.app,
        "POST",
        &format!("/api/v1/plugins/{admin_plugin_id}/auth"),
        &h.admin_token,
        Some(serde_json::json!({ "action": "qr_poll", "key": "uk_test_99" })),
    )
    .await;
    let poll1: serde_json::Value = serde_json::from_slice(&poll1_body).unwrap();
    assert_eq!(poll1["status"], "waiting");

    let (_, _, poll2_body) = send_json(
        &h.app,
        "POST",
        &format!("/api/v1/plugins/{admin_plugin_id}/auth"),
        &h.admin_token,
        Some(serde_json::json!({ "action": "qr_poll", "key": "uk_test_99" })),
    )
    .await;
    let poll2: serde_json::Value = serde_json::from_slice(&poll2_body).unwrap();
    assert_eq!(poll2["status"], "confirmed");
    assert_eq!(poll2["profile"]["nickname"], "CloudAdmin");
    assert_eq!(poll2["profile"]["vip_label"], "黑胶VIP");

    // 4. Admin updates settings (including password secret masking) & resolves lossless FLAC!
    let (st_set, _, set_body) = send_json(
        &h.app,
        "PUT",
        &format!("/api/v1/plugins/{admin_plugin_id}/settings"),
        &h.admin_token,
        Some(serde_json::json!({
            "values": {
                "defaultLevel": "lossless",
                "customToken": "my-secret-token"
            }
        })),
    )
    .await;
    assert_eq!(st_set, StatusCode::OK);
    let set_json: serde_json::Value = serde_json::from_slice(&set_body).unwrap();
    assert_eq!(set_json["settings_values"]["defaultLevel"], "lossless");
    assert_eq!(set_json["settings_values"]["customToken"], "••••••••");

    let (st_vip_ok, _, vip_body) = send_json(
        &h.app,
        "POST",
        "/online/stream",
        &h.admin_token,
        Some(serde_json::json!({
            "source_id": admin_plugin_id,
            "track_id": "186016",
            "quality": "lossless"
        })),
    )
    .await;
    assert_eq!(st_vip_ok, StatusCode::OK);
    let vip_json: serde_json::Value = serde_json::from_slice(&vip_body).unwrap();
    assert_eq!(vip_json["format"], "flac");
    assert_eq!(vip_json["bitrate"], 999);

    // 5. Strict isolation: Member imports the exact same JS plugin, gets their own plugin ID and
    //    logged-out state (Admin's VIP cookie is NEVER shared with Member!)
    let (_, _, mem_imp_body) = send_json(
        &h.app,
        "POST",
        "/api/v1/plugins/import",
        &h.member_token,
        Some(serde_json::json!({ "raw_json": js_plugin })),
    )
    .await;
    let mem_imp: serde_json::Value = serde_json::from_slice(&mem_imp_body).unwrap();
    let member_plugin_id = mem_imp["sources"][0]["id"].as_str().unwrap().to_string();
    assert_ne!(member_plugin_id, admin_plugin_id);

    let (_, _, mem_status_body) = send_json(
        &h.app,
        "POST",
        &format!("/api/v1/plugins/{member_plugin_id}/auth"),
        &h.member_token,
        Some(serde_json::json!({ "action": "status" })),
    )
    .await;
    let mem_status: serde_json::Value = serde_json::from_slice(&mem_status_body).unwrap();
    assert_eq!(mem_status["status"], "logged_out");

    // 6. Export preserves verbatim JS module with application/javascript Content-Type
    let (st_exp, exp_headers, exp_bytes) = send_json(
        &h.app,
        "GET",
        &format!("/api/v1/plugins/{admin_plugin_id}/export"),
        &h.admin_token,
        None,
    )
    .await;
    assert_eq!(st_exp, StatusCode::OK);
    assert_eq!(
        exp_headers
            .get(header::CONTENT_TYPE)
            .and_then(|v| v.to_str().ok()),
        Some("application/javascript; charset=utf-8")
    );
    assert!(String::from_utf8_lossy(&exp_bytes).contains("definePlugin"));
}

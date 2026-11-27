mod concurrency_races;
mod crud_and_ssrf;
mod proxy_and_guards;
mod tracked_and_jellyfin;

use std::collections::HashMap;
use std::sync::{Arc, Mutex, RwLock};

use axum::Router;
use axum::body::{Body, to_bytes};
use axum::extract::Query;
use axum::http::{HeaderMap, Request, StatusCode, header};
use axum::response::{IntoResponse, Redirect, Response};
use axum::routing::get;
use serde::Deserialize;
use tempfile::TempDir;
use toasty::Db;
use toasty::schema::ModelSet;
use tower::ServiceExt;
use xmedia_core::application::auth::AuthService;
use xmedia_core::application::dto::{CreateUserInput, SetupAdminInput};
use xmedia_core::application::library::LibraryService;
use xmedia_core::domain::user::UserRole;
use xmedia_core::infra::events::BroadcastEventHub;
use xmedia_core::infra::ffmpeg::FfmpegToolkit;
use xmedia_core::infra::persistence::repositories::{
    ToastyLibraryRepository, ToastyUserRepository,
};
use xmedia_core::infra::security::{JwtTokenService, SecurityManager};
use xmedia_core::{ensure_core_schema, register_core_models};

use crate::http::rest_router;
use crate::http::state::VideoHttpState;
use crate::{ensure_video_schema, init_video_module_with_allow_private_ips, register_video_models};

const PNG_1X1: &[u8] = &[
    0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A, 0x00, 0x00, 0x00, 0x0D, b'I', b'H', b'D', b'R',
    0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01, 0x08, 0x06, 0x00, 0x00, 0x00, 0x1F, 0x15, 0xC4,
    0x89, 0x00, 0x00, 0x00, 0x0A, b'I', b'D', b'A', b'T', 0x78, 0x9C, 0x63, 0x00, 0x01, 0x00, 0x00,
    0x05, 0x00, 0x01, 0x0D, 0x0A, 0x2D, 0xB4, 0x00, 0x00, 0x00, 0x00, b'I', b'E', b'N', b'D', 0xAE,
    0x42, 0x60, 0x82,
];

fn fake_mp4_payload() -> Vec<u8> {
    let mut data = Vec::with_capacity(4096);
    data.extend_from_slice(b"\x00\x00\x00\x18ftypisom\x00\x00\x02\x00isomiso2");
    for i in 0..(4096 - 24) {
        data.push((i % 251) as u8);
    }
    data
}

fn fake_ts_segment() -> Vec<u8> {
    let mut seg = vec![0u8; 188 * 2];
    seg[0] = 0x47;
    seg[188] = 0x47;
    seg[1] = 0x40;
    seg
}

#[derive(Debug, Default, Deserialize)]
struct AppleCmsQuery {
    ac: Option<String>,
    wd: Option<String>,
    t: Option<String>,
    ids: Option<String>,
    pg: Option<String>,
}

#[derive(Debug, Default, Deserialize)]
struct PeerTubeSearchQuery {
    search: Option<String>,
    #[serde(rename = "categoryOneOf")]
    category_one_of: Option<String>,
}

#[derive(Debug, Default, Deserialize)]
struct ArchiveSearchQuery {
    q: Option<String>,
}

async fn setup_test_app(allow_private_ips: bool) -> (TempDir, Router, String, String) {
    let tmp = TempDir::new().unwrap();
    let db_path = tmp.path().join("xmedia_video_online_e2e.db");
    let db_url = format!("sqlite:{}", db_path.display());

    let mut models = ModelSet::new();
    register_core_models(&mut models);
    register_video_models(&mut models);
    let mut db = Db::builder().models(models).connect(&db_url).await.unwrap();
    ensure_core_schema(&mut db, &db_url).await.unwrap();
    ensure_video_schema(&mut db, &db_url).await.unwrap();

    let users_repo = Arc::new(ToastyUserRepository::new(db.clone()));
    let libraries_repo = Arc::new(ToastyLibraryRepository::new(db.clone()));
    let lifecycles = Arc::new(RwLock::new(HashMap::new()));
    let events = Arc::new(BroadcastEventHub::default());
    let security = Arc::new(SecurityManager::new("video-online-e2e-secret"));
    let tokens = Arc::new(JwtTokenService::new("video-online-e2e-secret", 3600));

    let auth_svc = Arc::new(AuthService::new(
        users_repo,
        security,
        tokens,
        lifecycles.clone(),
    ));
    let library_svc = Arc::new(LibraryService::new(
        libraries_repo.clone(),
        events.clone(),
        lifecycles,
    ));
    let video_services = init_video_module_with_allow_private_ips(
        db,
        tmp.path(),
        FfmpegToolkit::new("__missing_ffmpeg__", "__missing_ffprobe__"),
        libraries_repo,
        &library_svc,
        events,
        allow_private_ips,
    )
    .unwrap();

    let admin_session = auth_svc
        .setup_initial_admin(SetupAdminInput {
            username: "admin".into(),
            display_name: Some("Admin".into()),
            password: "adminpassword123".into(),
        })
        .await
        .unwrap();

    let admin_user = auth_svc
        .authenticate(
            xmedia_core::application::dto::AuthCredential::Jwt(&admin_session.token),
            None,
        )
        .await
        .unwrap();
    auth_svc
        .create_user(
            &admin_user,
            CreateUserInput {
                username: "member".into(),
                display_name: Some("Member".into()),
                password: "memberpassword123".into(),
                role: UserRole::Member,
            },
        )
        .await
        .unwrap();
    let member_session = auth_svc
        .login("member", "memberpassword123", None)
        .await
        .unwrap();

    let state = VideoHttpState::new(auth_svc, library_svc, video_services);
    let app = Router::new()
        .nest("/api/v1/plugins", crate::http::plugins_router(&state))
        .nest("/api/v1/video", rest_router(state));

    (tmp, app, admin_session.token, member_session.token)
}

async fn start_mock_video_origin() -> (
    String,
    Arc<Mutex<Vec<HashMap<String, String>>>>,
    tokio::task::JoinHandle<()>,
) {
    let captured_headers: Arc<Mutex<Vec<HashMap<String, String>>>> =
        Arc::new(Mutex::new(Vec::new()));
    let cap_stream = captured_headers.clone();

    let mp4_bytes = Arc::new(fake_mp4_payload());
    let mp4_for_route = mp4_bytes.clone();

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let origin = format!("http://{addr}");
    let origin_detail = origin.clone();

    let app = Router::new()
        .route(
            "/applecms/api.php/provide/vod/",
            get(move |Query(q): Query<AppleCmsQuery>| {
                let origin_inner = origin_detail.clone();
                async move {
                    match q.ac.as_deref() {
                        Some("list") => Response::builder()
                            .status(StatusCode::OK)
                            .header("content-type", "application/json")
                            .body(Body::from(
                                serde_json::json!({
                                    "code": 1,
                                    "class": [
                                        {"type_id": 1, "type_pid": 0, "type_name": "电影"},
                                        {"type_id": 33, "type_pid": 1, "type_name": "科幻片"}
                                    ],
                                    "list": []
                                })
                                .to_string(),
                            ))
                            .unwrap(),
                        Some("detail") if q.ids.as_deref() == Some("71070") => Response::builder()
                            .status(StatusCode::OK)
                            .header("content-type", "application/json")
                            .body(Body::from(
                                serde_json::json!({
                                    "code": 1,
                                    "list": [{
                                        "vod_id": 71070,
                                        "vod_name": "流浪地球2",
                                        "type_name": "科幻片",
                                        "vod_pic": "/posters/earth2.png",
                                        "vod_remarks": "HD",
                                        "vod_year": "2023",
                                        "vod_area": "中国大陆",
                                        "vod_actor": "吴京,刘德华",
                                        "vod_director": "郭帆",
                                        "vod_content": "太阳危机即将来临",
                                        "vod_play_from": "htmlshare$$$ikm3u8",
                                        "vod_play_url": format!("在线$https://share.example.invalid/share/71070$$$正片${origin_inner}/applecms/api.php/provide/vod/redir/master.m3u8")
                                    }]
                                })
                                .to_string(),
                            ))
                            .unwrap(),
                        Some("detail") if q.wd.as_deref().is_some() => {
                            let kw = q.wd.unwrap_or_default();
                            if kw.contains("流浪") {
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
                                                "vod_id": 71070,
                                                "vod_name": "流浪地球2",
                                                "type_name": "科幻片",
                                                "vod_pic": "/posters/earth2.png",
                                                "vod_remarks": "HD",
                                                "vod_year": "2023",
                                                "vod_area": "中国大陆"
                                            }]
                                        })
                                        .to_string(),
                                    ))
                                    .unwrap()
                            } else {
                                Response::builder()
                                    .status(StatusCode::OK)
                                    .header("content-type", "application/json")
                                    .body(Body::from(r#"{"code":1,"list":[]}"#))
                                    .unwrap()
                            }
                        }
                        Some("detail") => {
                            let page =
                                q.pg.unwrap_or_else(|| "1".into()).parse::<u32>().unwrap_or(1);
                            let cat = q.t.as_deref().unwrap_or("");
                            Response::builder()
                                .status(StatusCode::OK)
                                .header("content-type", "application/json")
                                .body(Body::from(
                                    serde_json::json!({
                                        "code": 1,
                                        "page": page,
                                        "pagecount": 2,
                                        "total": 2,
                                        "list": [{
                                            "vod_id": 71070,
                                            "vod_name": format!("科幻精选-{cat}"),
                                            "type_name": "科幻片",
                                            "vod_pic": "/posters/earth2.png",
                                            "vod_remarks": "HD"
                                        }]
                                    })
                                    .to_string(),
                                ))
                                .unwrap()
                        }
                        _ => (StatusCode::BAD_REQUEST, "bad ac").into_response(),
                    }
                }
            }),
        )
        .route(
            "/suoni/api.php/provide/vod/",
            get(|Query(q): Query<AppleCmsQuery>| async move {
                if q.wd.is_some() {
                    return Response::builder()
                        .status(StatusCode::OK)
                        .header("content-type", "text/html; charset=utf-8")
                        .body(Body::from("暂不支持搜索"))
                        .unwrap();
                }
                match q.ac.as_deref() {
                    Some("list") => Response::builder()
                        .status(StatusCode::OK)
                        .header("content-type", "application/json")
                        .body(Body::from(
                            r#"{"code":1,"class":[{"type_id":69,"type_name":"动作片"}]}"#,
                        ))
                        .unwrap(),
                    _ => Response::builder()
                        .status(StatusCode::OK)
                        .header("content-type", "application/json")
                        .body(Body::from(
                            r#"{"code":1,"total":1,"list":[{"vod_id":160073,"vod_name":"索尼电影","vod_pic":"/posters/earth2.png"}]}"#,
                        ))
                        .unwrap(),
                }
            }),
        )
        .route(
            "/peertube/api/v1/videos/categories",
            get(|| async {
                Response::builder()
                    .status(StatusCode::OK)
                    .header("content-type", "application/json")
                    .body(Body::from(r#"{"1":"Music","2":"Films"}"#))
                    .unwrap()
            }),
        )
        .route(
            "/peertube/api/v1/search/videos",
            get(|Query(q): Query<PeerTubeSearchQuery>| async move {
                let kw = q.search.unwrap_or_default();
                Response::builder()
                    .status(StatusCode::OK)
                    .header("content-type", "application/json")
                    .body(Body::from(
                        serde_json::json!({
                            "total": 1,
                            "data": [{
                                "uuid": "pt-uuid-1",
                                "name": format!("Blender {kw}"),
                                "thumbnailPath": "/posters/earth2.png",
                                "duration": 125,
                                "publishedAt": "2024-03-10T00:00:00.000Z",
                                "category": {"label": "Films"}
                            }]
                        })
                        .to_string(),
                    ))
                    .unwrap()
            }),
        )
        .route(
            "/peertube/api/v1/videos",
            get(|Query(q): Query<PeerTubeSearchQuery>| async move {
                if q.category_one_of.as_deref() == Some("") {
                    return (StatusCode::INTERNAL_SERVER_ERROR, "empty categoryOneOf")
                        .into_response();
                }
                Response::builder()
                    .status(StatusCode::OK)
                    .header("content-type", "application/json")
                    .body(Body::from(
                        serde_json::json!({
                            "total": 1,
                            "data": [{
                                "uuid": "pt-uuid-1",
                                "name": "PeerTube Browse Item",
                                "thumbnailPath": "/posters/earth2.png",
                                "duration": 90,
                                "publishedAt": "2024-03-10T00:00:00.000Z",
                                "category": {"label": "Films"}
                            }]
                        })
                        .to_string(),
                    ))
                    .unwrap()
            }),
        )
        .route(
            "/peertube/api/v1/videos/pt-uuid-1",
            get(|| async {
                Response::builder()
                    .status(StatusCode::OK)
                    .header("content-type", "application/json")
                    .body(Body::from(
                        serde_json::json!({
                            "uuid": "pt-uuid-1",
                            "name": "Blender Open Movie",
                            "thumbnailPath": "/posters/earth2.png",
                            "description": "CC-BY film",
                            "account": {"displayName": "Blender Foundation"},
                            "publishedAt": "2024-03-10T00:00:00.000Z",
                            "category": {"label": "Films"},
                            "files": [],
                            "streamingPlaylists": [{
                                "playlistUrl": "/cdn/v2/master.m3u8"
                            }]
                        })
                        .to_string(),
                    ))
                    .unwrap()
            }),
        )
        .route(
            "/archive/advancedsearch.php",
            get(|Query(q): Query<ArchiveSearchQuery>| async move {
                let query_str = q.q.unwrap_or_default();
                Response::builder()
                    .status(StatusCode::OK)
                    .header("content-type", "application/json")
                    .body(Body::from(
                        serde_json::json!({
                            "response": {
                                "docs": [{
                                    "identifier": "BigBuckBunny_124",
                                    "title": format!("Big Buck Bunny ({query_str})"),
                                    "year": "2008"
                                }]
                            }
                        })
                        .to_string(),
                    ))
                    .unwrap()
            }),
        )
        .route(
            "/archive/metadata/BigBuckBunny_124",
            get(|| async {
                Response::builder()
                    .status(StatusCode::OK)
                    .header("content-type", "application/json")
                    .body(Body::from(
                        serde_json::json!({
                            "dir": "/archive/files/BigBuckBunny_124",
                            "metadata": {
                                "identifier": "BigBuckBunny_124",
                                "title": "Big Buck Bunny",
                                "description": "Public domain animation",
                                "creator": "Blender",
                                "year": "2008"
                            },
                            "files": [
                                {"name": "Content/big_buck_bunny_720p.mp4", "source": "derivative"},
                                {"name": "readme.txt", "source": "original"}
                            ]
                        })
                        .to_string(),
                    ))
                    .unwrap()
            }),
        )
        .route(
            "/posters/earth2.png",
            get(|| async {
                Response::builder()
                    .status(StatusCode::OK)
                    .header("content-type", "image/png")
                    .body(Body::from(PNG_1X1.to_vec()))
                    .unwrap()
            }),
        )
        .route(
            "/posters/evil.svg",
            get(|| async {
                Response::builder()
                    .status(StatusCode::OK)
                    .header("content-type", "image/svg+xml")
                    .body(Body::from("<svg><script>alert(1)</script></svg>"))
                    .unwrap()
            }),
        )
        .route(
            "/posters/html.jpg",
            get(|| async {
                Response::builder()
                    .status(StatusCode::OK)
                    .header("content-type", "text/html")
                    .body(Body::from("<html><body>bad</body></html>"))
                    .unwrap()
            }),
        )
        .route(
            "/media/movie.mp4",
            get(move |headers: HeaderMap| {
                let cap = cap_stream.clone();
                let data = mp4_for_route.clone();
                async move {
                    let mut map = HashMap::new();
                    for (k, v) in &headers {
                        if let Ok(s) = v.to_str() {
                            map.insert(k.as_str().to_ascii_lowercase(), s.to_string());
                        }
                    }
                    cap.lock().unwrap().push(map);

                    let total = data.len();
                    if let Some(rng) = headers.get(header::RANGE).and_then(|v| v.to_str().ok())
                        && let Some(spec) = rng.strip_prefix("bytes=")
                    {
                        let (s_str, e_str) = spec.split_once('-').unwrap_or((spec, ""));
                        let start = s_str.parse::<usize>().unwrap_or(0);
                        let end = if e_str.is_empty() {
                            total - 1
                        } else {
                            e_str.parse::<usize>().unwrap_or(total - 1).min(total - 1)
                        };
                        if start >= total || end < start {
                            return Response::builder()
                                .status(StatusCode::RANGE_NOT_SATISFIABLE)
                                .header("content-range", format!("bytes */{total}"))
                                .body(Body::empty())
                                .unwrap();
                        }
                        let slice = data[start..=end].to_vec();
                        return Response::builder()
                            .status(StatusCode::PARTIAL_CONTENT)
                            .header("content-type", "video/mp4")
                            .header("accept-ranges", "bytes")
                            .header("content-range", format!("bytes {start}-{end}/{total}"))
                            .header("content-length", slice.len().to_string())
                            .body(Body::from(slice))
                            .unwrap();
                    }

                    Response::builder()
                        .status(StatusCode::OK)
                        .header("content-type", "video/mp4")
                        .header("accept-ranges", "bytes")
                        .header("content-length", total.to_string())
                        .body(Body::from(data.as_ref().clone()))
                        .unwrap()
                }
            }),
        )
        .route(
            "/media/html-page.mp4",
            get(|| async {
                Response::builder()
                    .status(StatusCode::OK)
                    .header("content-type", "text/html; charset=utf-8")
                    .body(Body::from("<html>error</html>"))
                    .unwrap()
            }),
        )
        .route(
            "/applecms/api.php/provide/vod/redir/master.m3u8",
            get(|| async { Redirect::temporary("/cdn/v2/master.m3u8") }),
        )
        .route(
            "/cdn/v2/master.m3u8",
            get(|| async {
                let body = "#EXTM3U\n\
                    #EXT-X-SESSION-KEY:METHOD=AES-128,URI=\"keys/session.key\"\n\
                    #EXT-X-MEDIA:TYPE=SUBTITLES,GROUP-ID=\"subs\",NAME=\"zh\",URI=\"subs/zh.m3u8\"\n\
                    #EXT-X-I-FRAME-STREAM-INF:BANDWIDTH=60000,URI=\"5000kb/hls/iframe.m3u8\"\n\
                    #EXT-X-STREAM-INF:BANDWIDTH=1500000\n\
                    5000kb/hls/index.m3u8\n";
                Response::builder()
                    .status(StatusCode::OK)
                    .header("content-type", "application/vnd.apple.mpegurl")
                    .body(Body::from(body))
                    .unwrap()
            }),
        )
        .route(
            "/cdn/v2/5000kb/hls/index.m3u8",
            get(|| async {
                let body = "#EXTM3U\n\
                    #EXT-X-TARGETDURATION:4\n\
                    #EXT-X-KEY:METHOD=AES-128,URI=\"../../keys/ep.key\"\n\
                    #EXT-X-MAP:URI=\"init.mp4\",BYTERANGE=\"16@0\"\n\
                    #EXT-X-BYTERANGE:376@0\n\
                    seg0.ts\n";
                Response::builder()
                    .status(StatusCode::OK)
                    .header("content-type", "text/plain")
                    .body(Body::from(body))
                    .unwrap()
            }),
        )
        .route(
            "/cdn/v2/keys/ep.key",
            get(|| async {
                Response::builder()
                    .status(StatusCode::OK)
                    .header("content-type", "application/octet-stream")
                    .body(Body::from(vec![0xABu8; 16]))
                    .unwrap()
            }),
        )
        .route(
            "/cdn/v2/5000kb/hls/seg0.ts",
            get(|headers: HeaderMap| async move {
                let seg = fake_ts_segment();
                let total = seg.len();
                if let Some(rng) = headers.get(header::RANGE).and_then(|v| v.to_str().ok())
                    && rng == "bytes=0-187"
                {
                    return Response::builder()
                        .status(StatusCode::PARTIAL_CONTENT)
                        .header("content-type", "video/mp2t")
                        .header("accept-ranges", "bytes")
                        .header("content-range", format!("bytes 0-187/{total}"))
                        .header("content-length", "188")
                        .body(Body::from(seg[..188].to_vec()))
                        .unwrap();
                }
                Response::builder()
                    .status(StatusCode::OK)
                    .header("content-type", "video/mp2t")
                    .header("accept-ranges", "bytes")
                    .header("content-length", total.to_string())
                    .body(Body::from(seg))
                    .unwrap()
            }),
        )
        .route(
            "/cdn/oversized.m3u8",
            get(|| async {
                let mut big = String::from("#EXTM3U\n");
                big.push_str(&"#EXTINF:4.0,\nseg.ts\n".repeat(600_000));
                Response::builder()
                    .status(StatusCode::OK)
                    .header("content-type", "application/vnd.apple.mpegurl")
                    .body(Body::from(big))
                    .unwrap()
            }),
        )
        .route(
            "/edge/redirect-to-loopback",
            get(|| async { Redirect::temporary("http://127.0.0.1:9/secret.mp4") }),
        );

    let handle = tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });
    (origin, captured_headers, handle)
}

async fn post_json(
    app: &Router,
    uri: &str,
    token: &str,
    body: serde_json::Value,
) -> (StatusCode, serde_json::Value) {
    let res = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(uri)
                .header(header::AUTHORIZATION, format!("Bearer {token}"))
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(body.to_string()))
                .unwrap(),
        )
        .await
        .unwrap();
    let status = res.status();
    let bytes = to_bytes(res.into_body(), usize::MAX).await.unwrap();
    let val = serde_json::from_slice(&bytes).unwrap_or(serde_json::Value::Null);
    (status, val)
}

async fn patch_json(
    app: &Router,
    uri: &str,
    token: &str,
    body: serde_json::Value,
) -> (StatusCode, serde_json::Value) {
    let res = app
        .clone()
        .oneshot(
            Request::builder()
                .method("PATCH")
                .uri(uri)
                .header(header::AUTHORIZATION, format!("Bearer {token}"))
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(body.to_string()))
                .unwrap(),
        )
        .await
        .unwrap();
    let status = res.status();
    let bytes = to_bytes(res.into_body(), usize::MAX).await.unwrap();
    let val = serde_json::from_slice(&bytes).unwrap_or(serde_json::Value::Null);
    (status, val)
}

async fn send_json_with_method(
    app: &Router,
    method: &str,
    uri: &str,
    token: &str,
    body: Option<serde_json::Value>,
) -> (StatusCode, serde_json::Value) {
    let mut builder = Request::builder()
        .method(method)
        .uri(uri)
        .header(header::AUTHORIZATION, format!("Bearer {token}"));
    let req = if let Some(b) = body {
        builder = builder.header(header::CONTENT_TYPE, "application/json");
        builder.body(Body::from(b.to_string())).unwrap()
    } else {
        builder.body(Body::empty()).unwrap()
    };
    let res = app.clone().oneshot(req).await.unwrap();
    let status = res.status();
    let bytes = to_bytes(res.into_body(), usize::MAX).await.unwrap();
    let val = serde_json::from_slice(&bytes).unwrap_or(serde_json::Value::Null);
    (status, val)
}

async fn get_bytes_with_auth(
    app: &Router,
    uri: &str,
    token: Option<&str>,
    range: Option<&str>,
) -> (StatusCode, HeaderMap, Vec<u8>) {
    let mut builder = Request::builder().method("GET").uri(uri);
    if let Some(tok) = token {
        builder = builder.header(header::AUTHORIZATION, format!("Bearer {tok}"));
    }
    if let Some(rng) = range {
        builder = builder.header(header::RANGE, rng);
    }
    let res = app
        .clone()
        .oneshot(builder.body(Body::empty()).unwrap())
        .await
        .unwrap();
    let status = res.status();
    let headers = res.headers().clone();
    let bytes = to_bytes(res.into_body(), usize::MAX)
        .await
        .unwrap()
        .to_vec();
    (status, headers, bytes)
}

fn urlencoding_param(s: &str) -> String {
    form_urlencoded::byte_serialize(s.as_bytes()).collect()
}

fn delayed_chunked_response(content_type: &'static str, chunks: Vec<(Vec<u8>, u64)>) -> Response {
    let (tx, rx) =
        tokio::sync::mpsc::channel::<xmedia_core::domain::error::DomainResult<bytes::Bytes>>(8);
    tokio::spawn(async move {
        for (data, delay_ms) in chunks {
            if delay_ms > 0 {
                tokio::time::sleep(std::time::Duration::from_millis(delay_ms)).await;
            }
            if tx.send(Ok(bytes::Bytes::from(data))).await.is_err() {
                break;
            }
        }
    });
    Response::builder()
        .status(StatusCode::OK)
        .header("content-type", content_type)
        .body(Body::from_stream(
            xmedia_core::infra::stream::MpscByteStream::into_boxed(rx),
        ))
        .unwrap()
}

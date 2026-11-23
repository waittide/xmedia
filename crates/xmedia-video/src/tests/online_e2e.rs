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
use uuid::Uuid;
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
        .authenticate_jwt(&admin_session.token)
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
    let member_session = auth_svc.login("member", "memberpassword123").await.unwrap();

    let state = VideoHttpState::new(auth_svc, library_svc, video_services);
    let app = Router::new().nest("/api/v1/video", rest_router(state));

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
        // AppleCMS v10 searchable mock
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
                        let page = q.pg.unwrap_or_else(|| "1".into()).parse::<u32>().unwrap_or(1);
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
        // AppleCMS v10 non-searchable mock (returns plain-text error on wd)
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
                        .body(Body::from(r#"{"code":1,"class":[{"type_id":69,"type_name":"动作片"}]}"#))
                        .unwrap(),
                    _ => Response::builder()
                        .status(StatusCode::OK)
                        .header("content-type", "application/json")
                        .body(Body::from(r#"{"code":1,"total":1,"list":[{"vod_id":160073,"vod_name":"索尼电影","vod_pic":"/posters/earth2.png"}]}"#))
                        .unwrap(),
                }
            }),
        )
        // PeerTube mock (identical response shape to framatube.org)
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
                    return (StatusCode::INTERNAL_SERVER_ERROR, "empty categoryOneOf").into_response();
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
        // Internet Archive mock (identical shape to archive.org)
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
        // Posters
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
        // MP4 streaming with Range 206 + header inspection
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
        // HLS redirect -> post-redirect base URL + master/media/key/map/segment
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
                    // Intentionally omit mpegurl CT to test #EXTM3U first-chunk sniff
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
                // 9 MiB (> 8 MiB limit)
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

#[tokio::test]
async fn video_online_sources_crud_rbac_applecms_rule_and_proxies_e2e() {
    let (_tmp, app, admin_tok, member_tok) = setup_test_app(true).await;
    let (origin, captured_headers, mock_handle) = start_mock_video_origin().await;

    // 1. Import 4 sources (AppleCMS searchable, AppleCMS non-searchable, PeerTube rule, Internet Archive rule)
    //    Wait: use testdata/online_sources.json templates rewritten to mock origin for PeerTube & Archive.
    let seed_sources: Vec<serde_json::Value> =
        serde_json::from_str(include_str!("../../testdata/online_sources.json")).unwrap();
    assert_eq!(seed_sources.len(), 6);

    // Rewrite PeerTube (index 4) & Archive (index 5) URLs to mock origin
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
    // Keep same derivative/path encoding JS logic, targeting mock origin
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
        "/api/v1/video/sources/import",
        &admin_tok,
        serde_json::json!({ "raw_json": batch_json.to_string() }),
    )
    .await;
    assert_eq!(st, StatusCode::OK);
    assert_eq!(import_res["imported_count"], 5);

    // Re-import idempotency check
    let (st, reimport_res) = post_json(
        &app,
        "/api/v1/video/sources/import",
        &admin_tok,
        serde_json::json!({ "raw_json": batch_json.to_string() }),
    )
    .await;
    assert_eq!(st, StatusCode::OK);
    assert_eq!(reimport_res["imported_count"], 0);
    assert_eq!(reimport_res["updated_count"], 5);

    // List sources & extract IDs
    let (st, _, list_bytes) =
        get_bytes_with_auth(&app, "/api/v1/video/sources", Some(&admin_tok), None).await;
    assert_eq!(st, StatusCode::OK);
    let admin_sources: Vec<serde_json::Value> = serde_json::from_slice(&list_bytes).unwrap();
    assert_eq!(admin_sources.len(), 5);

    let find_id = |name: &str| -> Uuid {
        let s = admin_sources
            .iter()
            .find(|v| v["name"] == name)
            .expect("source exists")["id"]
            .as_str()
            .unwrap();
        Uuid::parse_str(s).unwrap()
    };
    let apple_id = find_id("Mock AppleCMS");
    let suoni_id = find_id("Mock Suoni NoSearch");
    let peertube_id = find_id("PeerTube (Framatube)");
    let archive_id = find_id("Internet Archive Video");
    let disabled_id = find_id("Disabled Source");

    // 2. RBAC & Secret Masking
    let (st, _, member_list_bytes) =
        get_bytes_with_auth(&app, "/api/v1/video/sources", Some(&member_tok), None).await;
    assert_eq!(st, StatusCode::OK);
    let member_sources: Vec<serde_json::Value> =
        serde_json::from_slice(&member_list_bytes).unwrap();
    assert_eq!(member_sources.len(), 4); // disabled source hidden

    // Member forbidden on admin mutations / export / variable / test / login-action
    let (st, _) = post_json(
        &app,
        "/api/v1/video/sources/import",
        &member_tok,
        serde_json::json!({"raw_json":"{}"}),
    )
    .await;
    assert_eq!(st, StatusCode::FORBIDDEN);
    let (st, _, _) = get_bytes_with_auth(
        &app,
        &format!("/api/v1/video/sources/{apple_id}/variable"),
        Some(&member_tok),
        None,
    )
    .await;
    assert_eq!(st, StatusCode::FORBIDDEN);
    let (st, _) = post_json(
        &app,
        &format!("/api/v1/video/sources/{apple_id}/test"),
        &member_tok,
        serde_json::json!({"mode":"categories"}),
    )
    .await;
    assert_eq!(st, StatusCode::FORBIDDEN);

    // Login action stores variable & masks sensitive password field in summary
    let (st, login_res) = post_json(
        &app,
        &format!("/api/v1/video/sources/{apple_id}/login-action"),
        &admin_tok,
        serde_json::json!({
            "form_data": {"token": "super-secret-val"},
            "action": "login()"
        }),
    )
    .await;
    assert_eq!(st, StatusCode::OK);
    assert_eq!(login_res["result"], "ok");
    assert_eq!(login_res["source"]["login_info"]["token"], "••••••••");

    let (st, _, var_bytes) = get_bytes_with_auth(
        &app,
        &format!("/api/v1/video/sources/{apple_id}/variable"),
        Some(&admin_tok),
        None,
    )
    .await;
    assert_eq!(st, StatusCode::OK);
    let var_json: serde_json::Value = serde_json::from_slice(&var_bytes).unwrap();
    assert_eq!(var_json["variable"], "auth=super-secret-val");

    // Non-admin member sees stripped login_ui / login_info / variable_preview
    let (st, _, member_list_bytes2) =
        get_bytes_with_auth(&app, "/api/v1/video/sources", Some(&member_tok), None).await;
    assert_eq!(st, StatusCode::OK);
    let member_sources2: Vec<serde_json::Value> =
        serde_json::from_slice(&member_list_bytes2).unwrap();
    let member_apple = member_sources2
        .iter()
        .find(|s| s["name"] == "Mock AppleCMS")
        .unwrap();
    assert!(member_apple["login_info"].as_object().unwrap().is_empty());
    assert!(member_apple["login_ui"].as_array().unwrap().is_empty());
    assert_eq!(member_apple["variable_preview"], "");

    // Reorder / batch / export-batch / patch / export
    let (st, _) = post_json(
        &app,
        "/api/v1/video/sources/reorder",
        &admin_tok,
        serde_json::json!({ "ordered_ids": [peertube_id, apple_id] }),
    )
    .await;
    assert_eq!(st, StatusCode::OK);

    let (st, export_batch_res) = post_json(
        &app,
        "/api/v1/video/sources/export-batch",
        &admin_tok,
        serde_json::json!({ "ids": [apple_id, peertube_id] }),
    )
    .await;
    assert_eq!(st, StatusCode::OK);
    assert_eq!(export_batch_res.as_array().unwrap().len(), 2);

    // PATCH source weight/enabled/group and export single source
    let patch_res = app
        .clone()
        .oneshot(
            Request::builder()
                .method("PATCH")
                .uri(format!("/api/v1/video/sources/{archive_id}"))
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
        &format!("/api/v1/video/sources/{archive_id}/export"),
        Some(&admin_tok),
        None,
    )
    .await;
    assert_eq!(st, StatusCode::OK);
    let exp_json: serde_json::Value = serde_json::from_slice(&exp_bytes).unwrap();
    assert_eq!(exp_json["weight"], 89);
    assert_eq!(exp_json["sourceGroup"], "归档精选");

    // 3. Multi-source Search: skips `searchable: false` (suoni_id) cleanly, returns AppleCMS + PeerTube + Archive
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

    // Disabled source rejected for non-admin
    let (st, _) = post_json(
        &app,
        "/api/v1/video/online/categories",
        &member_tok,
        serde_json::json!({ "source_id": disabled_id }),
    )
    .await;
    assert_eq!(st, StatusCode::FORBIDDEN);

    // 4. AppleCMS Categories -> Browse -> Detail (/share/ filtered out) -> Play
    let (st, cat_res) = post_json(
        &app,
        "/api/v1/video/online/categories",
        &member_tok,
        serde_json::json!({ "source_id": apple_id }),
    )
    .await;
    assert_eq!(st, StatusCode::OK);
    assert_eq!(cat_res["categories"].as_array().unwrap().len(), 2);

    let (st, browse_res) = post_json(
        &app,
        "/api/v1/video/online/browse",
        &member_tok,
        serde_json::json!({ "source_id": apple_id, "category_id": "33", "page": 1 }),
    )
    .await;
    assert_eq!(st, StatusCode::OK);
    assert_eq!(browse_res["results"][0]["title"], "科幻精选-33");

    let (st, detail_res) = post_json(
        &app,
        "/api/v1/video/online/detail",
        &member_tok,
        serde_json::json!({ "source_id": apple_id, "video_id": "71070" }),
    )
    .await;
    assert_eq!(st, StatusCode::OK);
    let groups = detail_res["play_groups"].as_array().unwrap();
    assert_eq!(groups.len(), 1); // htmlshare filtered out!
    assert_eq!(groups[0]["name"], "ikm3u8");
    let play_url = groups[0]["episodes"][0]["play_url"].as_str().unwrap();
    assert!(play_url.ends_with("/applecms/api.php/provide/vod/redir/master.m3u8"));

    let (st, play_res) = post_json(
        &app,
        "/api/v1/video/online/play",
        &member_tok,
        serde_json::json!({ "source_id": apple_id, "play_url": play_url, "group_name": "ikm3u8" }),
    )
    .await;
    assert_eq!(st, StatusCode::OK);
    assert_eq!(play_res["stream_type"], "hls");
    assert_eq!(play_res["headers_applied"], true);
    let proxy_hls_url = play_res["proxy_stream_url"].as_str().unwrap();
    assert!(proxy_hls_url.starts_with("/api/v1/video/online/hls-proxy?"));

    // 5. Generic `rule` mode: PeerTube categories (JSON object -> JS), browse (empty catId avoids 500!), detail -> HLS
    let (st, pt_cats) = post_json(
        &app,
        "/api/v1/video/online/categories",
        &member_tok,
        serde_json::json!({ "source_id": peertube_id }),
    )
    .await;
    assert_eq!(st, StatusCode::OK);
    assert_eq!(pt_cats["categories"].as_array().unwrap().len(), 2);

    let (st, pt_browse) = post_json(
        &app,
        "/api/v1/video/online/browse",
        &member_tok,
        serde_json::json!({ "source_id": peertube_id, "page": 1 }),
    )
    .await;
    assert_eq!(st, StatusCode::OK);
    assert_eq!(pt_browse["results"][0]["title"], "PeerTube Browse Item");

    let (st, pt_detail) = post_json(
        &app,
        "/api/v1/video/online/detail",
        &member_tok,
        serde_json::json!({
            "source_id": peertube_id,
            "detail_url": format!("{origin}/peertube/api/v1/videos/pt-uuid-1")
        }),
    )
    .await;
    assert_eq!(st, StatusCode::OK);
    assert_eq!(
        pt_detail["play_groups"][0]["episodes"][0]["play_url"],
        format!("{origin}/cdn/v2/master.m3u8")
    );

    // Internet Archive `data:application/json` categories & MP4 episodes
    let (st, ia_cats) = post_json(
        &app,
        "/api/v1/video/online/categories",
        &member_tok,
        serde_json::json!({ "source_id": archive_id }),
    )
    .await;
    assert_eq!(st, StatusCode::OK);
    assert_eq!(ia_cats["categories"].as_array().unwrap().len(), 5);

    let (st, ia_detail) = post_json(
        &app,
        "/api/v1/video/online/detail",
        &member_tok,
        serde_json::json!({
            "source_id": archive_id,
            "detail_url": format!("{origin}/archive/metadata/BigBuckBunny_124")
        }),
    )
    .await;
    assert_eq!(st, StatusCode::OK);
    let ia_ep_url = ia_detail["play_groups"][0]["episodes"][0]["play_url"]
        .as_str()
        .unwrap();
    assert!(ia_ep_url.ends_with("/archive/files/BigBuckBunny_124/Content/big_buck_bunny_720p.mp4"));

    // 6. Test endpoint (`full`, `browse`, `explore` alias)
    let (st, full_test) = post_json(
        &app,
        &format!("/api/v1/video/sources/{apple_id}/test"),
        &admin_tok,
        serde_json::json!({ "mode": "full", "keyword": "流浪" }),
    )
    .await;
    assert_eq!(st, StatusCode::OK);
    assert_eq!(full_test["ok"], true);
    assert!(full_test["play_result"]["stream_type"] == "hls");

    let (st, suoni_test) = post_json(
        &app,
        &format!("/api/v1/video/sources/{suoni_id}/test"),
        &admin_tok,
        serde_json::json!({ "mode": "explore" }),
    )
    .await;
    assert_eq!(st, StatusCode::OK);
    assert_eq!(suoni_test["ok"], true);

    // 7. Poster proxy: raster OK + SVG/HTML rejected
    let (st, hdrs, png_body) = get_bytes_with_auth(
        &app,
        &format!(
            "/api/v1/video/online/poster-proxy?url={}&source_id={apple_id}",
            urlencoding_param(&format!("{origin}/posters/earth2.png"))
        ),
        Some(&member_tok),
        None,
    )
    .await;
    assert_eq!(st, StatusCode::OK);
    assert_eq!(png_body, PNG_1X1);
    assert_eq!(hdrs.get(header::X_CONTENT_TYPE_OPTIONS).unwrap(), "nosniff");

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
            "/api/v1/video/online/stream-proxy?url={}&source_id={apple_id}",
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
        // Client Authorization bearer token MUST NOT be forwarded upstream
        assert!(!last.contains_key("authorization"));
    }

    let (st, hdrs, _) = get_bytes_with_auth(
        &app,
        &format!(
            "/api/v1/video/online/stream-proxy?url={}",
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
            "/api/v1/video/online/stream-proxy?url={}",
            urlencoding_param(&format!("{origin}/media/html-page.mp4"))
        ),
        Some(&member_tok),
        None,
    )
    .await;
    assert_eq!(st, StatusCode::BAD_REQUEST);

    // 9. HLS proxy: post-redirect base URL + master/media rewrite + all directives + ?token= + binary segment Range 206 + >8MiB cap
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
    // Redirected to /cdn/v2/master.m3u8 so relative 5000kb/hls/index.m3u8 resolves under /cdn/v2/!
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

    // Follow rewritten sub-playlist line (which had text/plain CT -> sniffed via #EXTM3U!)
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

    // Binary .ts segment with Range: bytes=0-187 -> 206 Partial Content
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

    // Disabled source rejected on hls-proxy for non-admin
    let (st, _, _) = get_bytes_with_auth(
        &app,
        &format!(
            "/api/v1/video/online/hls-proxy?url={}&source_id={disabled_id}",
            urlencoding_param(&format!("{origin}/cdn/v2/master.m3u8"))
        ),
        Some(&member_tok),
        None,
    )
    .await;
    assert_eq!(st, StatusCode::FORBIDDEN);

    // > 8 MiB playlist rejected
    let (st, _, _) = get_bytes_with_auth(
        &app,
        &format!(
            "/api/v1/video/online/hls-proxy?url={}",
            urlencoding_param(&format!("{origin}/cdn/oversized.m3u8"))
        ),
        Some(&member_tok),
        None,
    )
    .await;
    assert_eq!(st, StatusCode::BAD_REQUEST);

    // Delete & Batch delete
    let del_res = app
        .clone()
        .oneshot(
            Request::builder()
                .method("DELETE")
                .uri(format!("/api/v1/video/sources/{suoni_id}"))
                .header(header::AUTHORIZATION, format!("Bearer {admin_tok}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(del_res.status(), StatusCode::NO_CONTENT);

    let (st, batch_del) = post_json(
        &app,
        "/api/v1/video/sources/batch",
        &admin_tok,
        serde_json::json!({ "ids": [disabled_id], "action": "delete" }),
    )
    .await;
    assert_eq!(st, StatusCode::OK);
    assert_eq!(batch_del["affected_count"], 1);

    // Importing testdata/online_sources.json directly through HTTP import endpoint succeeds
    let (st, seed_imp) = post_json(
        &app,
        "/api/v1/video/sources/import",
        &admin_tok,
        serde_json::json!({ "raw_json": include_str!("../../testdata/online_sources.json") }),
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

    // 1. Direct loopback blocked on stream-proxy, hls-proxy, poster-proxy
    let (st, _, _) = get_bytes_with_auth(
        &app,
        &format!(
            "/api/v1/video/online/stream-proxy?url={}",
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
            "/api/v1/video/online/hls-proxy?url={}",
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

    // 2. Metadata / private / localhost URLs blocked
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
                "/api/v1/video/online/stream-proxy?url={}",
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

    // 3. AppleCMS source pointing to 127.0.0.1 is blocked during search/categories/test
    let (st, imp) = post_json(
        &app,
        "/api/v1/video/sources/import",
        &admin_tok,
        serde_json::json!({
            "raw_json": serde_json::json!({
                "sourceName": "Loopback Source",
                "apiUrl": format!("{origin}/applecms/api.php/provide/vod/"),
                "enabled": true
            }).to_string()
        }),
    )
    .await;
    assert_eq!(st, StatusCode::OK);
    let sid = imp["sources"][0]["id"].as_str().unwrap();

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

    // 1. hls-proxy multi-chunk split BOM+whitespace before #EXTM3U on extensionless URL
    let (st, hdrs, body) = get_bytes_with_auth(
        &app,
        &format!(
            "/api/v1/video/online/hls-proxy?url={}",
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

    // 2. hls-proxy & stream-proxy multi-chunk split `<ht` + `ml>...` HTML -> 400 Bad Request
    let (st, _, _) = get_bytes_with_auth(
        &app,
        &format!(
            "/api/v1/video/online/hls-proxy?url={}",
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
            "/api/v1/video/online/stream-proxy?url={}",
            urlencoding_param(&format!("{origin}/stream/chunked-html"))
        ),
        Some(&member_tok),
        None,
    )
    .await;
    assert_eq!(st, StatusCode::BAD_REQUEST);

    // 3. stream-proxy disguised HTML (video/mp4) & SVG (application/octet-stream) -> 400 Bad Request
    let (st, _, _) = get_bytes_with_auth(
        &app,
        &format!(
            "/api/v1/video/online/stream-proxy?url={}",
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
            "/api/v1/video/online/stream-proxy?url={}",
            urlencoding_param(&format!("{origin}/media/disguised-svg.bin"))
        ),
        Some(&member_tok),
        None,
    )
    .await;
    assert_eq!(st, StatusCode::BAD_REQUEST);

    // 4. Multi-chunk binary streams preserve all bytes & normalize Content-Type for CompressionLayer
    let (st, hdrs, mp4_bytes) = get_bytes_with_auth(
        &app,
        &format!(
            "/api/v1/video/online/stream-proxy?url={}",
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
            "/api/v1/video/online/hls-proxy?url={}",
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

#[tokio::test]
async fn test_source_concurrent_login_action_preserves_updated_state_case_b_e2e() {
    let (_tmp, app, admin_tok, _member_tok) = setup_test_app(true).await;

    let entered = Arc::new(tokio::sync::Notify::new());
    let release = Arc::new(tokio::sync::Notify::new());
    let entered_srv = entered.clone();
    let release_srv = release.clone();

    let origin_app = Router::new().route(
        "/delayed/api.php/provide/vod/",
        get(move |Query(q): Query<AppleCmsQuery>| {
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

    // 1. Establish prior runtime_state (Case B pre-existing var_1 / alice_1 / secret_1).
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

    // 2. Start delayed test_source (captures prev_state with var_1 / alice_1) and pause at upstream.
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

    // 3. Concurrently run login-action updating to var_2 / bob_2 / secret_2 while test_source is in flight.
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

    // 4. Release upstream response and wait for test_source to finish and persist test result.
    release.notify_one();
    let (test_st, test_res) = test_task.await.unwrap();
    assert_eq!(test_st, StatusCode::OK);
    assert_eq!(test_res["ok"], true);

    // 5. Verify newer login-action variable & login_info survived test_source completion.
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

    // 6. Also verify concurrent online search preserves newer login-action state (var_3 / carol_3).
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

    // 1. Seed initial session_id=old_session on origin host.
    let (st, _) = post_json(
        &app,
        &format!("/api/v1/video/sources/{sid}/login-action"),
        &admin_tok,
        serde_json::json!({ "login_info": { "sid": "old_session" }, "action": "setSession()" }),
    )
    .await;
    assert_eq!(st, StatusCode::OK);

    // 2. Start delayed browse on the same origin host and wait until it hits upstream.
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

    // 3. Concurrently rotate session_id=new_session on the SAME origin host via login-action.
    let (st, _) = post_json(
        &app,
        &format!("/api/v1/video/sources/{sid}/login-action"),
        &admin_tok,
        serde_json::json!({ "login_info": { "sid": "new_session" }, "action": "setSession()" }),
    )
    .await;
    assert_eq!(st, StatusCode::OK);

    // 4. Release browse (returns Set-Cookie: cf_bm=browse_cookie_123 on the same origin).
    release.notify_one();
    let (browse_st, browse_res) = browse_task.await.unwrap();
    assert_eq!(browse_st, StatusCode::OK);
    assert_eq!(browse_res["results"][0]["title"], "SameHost Movie");

    // 5. Both new_session and browse_cookie_123 must survive on the same host.
    let (st, read_res) = post_json(
        &app,
        &format!("/api/v1/video/sources/{sid}/login-action"),
        &admin_tok,
        serde_json::json!({ "login_info": {}, "action": "readCookies()" }),
    )
    .await;
    assert_eq!(st, StatusCode::OK);
    assert_eq!(read_res["result"], "new_session|browse_cookie_123");

    // 6. Subsequent HTTP fetch to origin carries both cookies in Cookie header.
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

#[tokio::test]
async fn rule_play_subtitles_jsonpath_and_js_e2e() {
    let (_tmp, app, admin_tok, member_tok) = setup_test_app(true).await;

    let origin_app = Router::new().route(
        "/api/play",
        get(|| async {
            Response::builder()
                .status(StatusCode::OK)
                .header("content-type", "application/json")
                .body(Body::from(
                    serde_json::json!({
                        "stream": "/media/ep1.mp4",
                        "subs": [
                            { "label": "中文", "lang": "zh", "url": "/subs/zh.vtt" },
                            { "label": "English", "lang": "en", "url": "/subs/en.srt" }
                        ]
                    })
                    .to_string(),
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

    let src_json = serde_json::json!({
        "sourceName": "Rule Subtitles JSONPath",
        "sourceUrl": origin,
        "sourceFormat": "rule",
        "enabled": true,
        "rulePlay": {
            "requestUrl": "{{playUrl}}",
            "streamUrl": "$.stream",
            "subtitles": "$.subs"
        }
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

    let (st, play_res) = post_json(
        &app,
        "/api/v1/video/online/play",
        &member_tok,
        serde_json::json!({
            "source_id": sid,
            "play_url": format!("{origin}/api/play")
        }),
    )
    .await;
    assert_eq!(st, StatusCode::OK);
    assert_eq!(play_res["stream_url"], format!("{origin}/media/ep1.mp4"));
    let subs = play_res["subtitles"].as_array().unwrap();
    assert_eq!(subs.len(), 2);
    assert_eq!(subs[0]["label"], "中文");
    assert_eq!(subs[0]["lang"], "zh");
    assert_eq!(subs[0]["url"], format!("{origin}/subs/zh.vtt"));
    assert_eq!(subs[1]["label"], "English");
    assert_eq!(subs[1]["lang"], "en");
    assert_eq!(subs[1]["url"], format!("{origin}/subs/en.srt"));

    srv.abort();
}

#[tokio::test]
async fn hls_proxy_rejects_malformed_playlist_missing_extm3u_header() {
    let (_tmp, app, _admin_tok, member_tok) = setup_test_app(true).await;
    let origin_app = Router::new()
        .route(
            "/bad.m3u8",
            get(|| async {
                Response::builder()
                    .status(StatusCode::OK)
                    .header("content-type", "application/vnd.apple.mpegurl")
                    .body(Body::from("not an m3u8 playlist"))
                    .unwrap()
            }),
        )
        .route(
            "/fake-tag.m3u8",
            get(|| async {
                Response::builder()
                    .status(StatusCode::OK)
                    .header("content-type", "application/vnd.apple.mpegurl")
                    .body(Body::from(
                        "#EXTM3U_GARBAGE_NOT_TAG\n#EXTINF:5.0,\nseg0.ts\n",
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

    let encoded_url: String =
        form_urlencoded::byte_serialize(format!("{origin}/bad.m3u8").as_bytes()).collect();
    let (st, _, _) = get_bytes_with_auth(
        &app,
        &format!("/api/v1/video/online/hls-proxy?url={encoded_url}"),
        Some(&member_tok),
        None,
    )
    .await;
    assert_eq!(st, StatusCode::BAD_REQUEST);

    let encoded_fake: String =
        form_urlencoded::byte_serialize(format!("{origin}/fake-tag.m3u8").as_bytes()).collect();
    let (st_fake, _, _) = get_bytes_with_auth(
        &app,
        &format!("/api/v1/video/online/hls-proxy?url={encoded_fake}"),
        Some(&member_tok),
        None,
    )
    .await;
    assert_eq!(st_fake, StatusCode::BAD_REQUEST);

    srv.abort();
}

#[tokio::test]
async fn hls_proxy_refetches_full_playlist_when_caller_sends_range_on_large_m3u8() {
    let (_tmp, app, _admin_tok, member_tok) = setup_test_app(true).await;

    let mut playlist = String::from("#EXTM3U\n#EXT-X-VERSION:3\n#EXT-X-TARGETDURATION:6\n");
    for i in 0..120 {
        playlist.push_str(&format!("#EXTINF:6.0,\nseg_{i:03}.ts\n"));
    }
    playlist.push_str("#EXT-X-ENDLIST\n");
    assert!(playlist.len() > 2048);

    let full_bytes = Arc::new(playlist.into_bytes());
    let seen_ranges: Arc<Mutex<Vec<Option<String>>>> = Arc::new(Mutex::new(Vec::new()));
    let seen_ranges_srv = seen_ranges.clone();
    let full_bytes_srv = full_bytes.clone();

    let origin_app = Router::new().route(
        "/large.m3u8",
        get(move |headers: HeaderMap| {
            let full = full_bytes_srv.clone();
            let log = seen_ranges_srv.clone();
            async move {
                let rng = headers
                    .get(header::RANGE)
                    .and_then(|v| v.to_str().ok())
                    .map(ToString::to_string);
                log.lock().unwrap().push(rng.clone());

                if let Some(r) = rng {
                    let spec = r.trim().strip_prefix("bytes=").unwrap_or("0-511");
                    let (s, e) = spec.split_once('-').unwrap_or(("0", "511"));
                    let start: usize = s.parse().unwrap_or(0);
                    let end: usize = e.parse().unwrap_or(511).min(full.len().saturating_sub(1));
                    let slice = full[start..=end].to_vec();
                    return Response::builder()
                        .status(StatusCode::PARTIAL_CONTENT)
                        .header("content-type", "application/vnd.apple.mpegurl")
                        .header(
                            "content-range",
                            format!("bytes {start}-{end}/{}", full.len()),
                        )
                        .body(Body::from(slice))
                        .unwrap();
                }

                Response::builder()
                    .status(StatusCode::OK)
                    .header("content-type", "application/vnd.apple.mpegurl")
                    .body(Body::from(full.as_ref().clone()))
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

    let encoded_url: String =
        form_urlencoded::byte_serialize(format!("{origin}/large.m3u8").as_bytes()).collect();

    // Case 1: Range starting at byte 0 that truncates the playlist before seg_119.ts
    let (st1, _, body1) = get_bytes_with_auth(
        &app,
        &format!("/api/v1/video/online/hls-proxy?url={encoded_url}"),
        Some(&member_tok),
        Some("bytes=0-511"),
    )
    .await;
    assert_eq!(st1, StatusCode::OK);
    let text1 = String::from_utf8(body1).unwrap();
    assert!(text1.starts_with("#EXTM3U\n"));
    assert!(text1.contains("seg_000.ts"));
    assert!(text1.contains("seg_119.ts"));
    assert!(text1.contains("#EXT-X-ENDLIST"));
    assert!(text1.contains("/api/v1/video/online/hls-proxy?url="));

    // Case 2: Mid-playlist Range where #EXTM3U header is omitted in the 206 slice
    let (st2, _, body2) = get_bytes_with_auth(
        &app,
        &format!("/api/v1/video/online/hls-proxy?url={encoded_url}"),
        Some(&member_tok),
        Some("bytes=200-711"),
    )
    .await;
    assert_eq!(st2, StatusCode::OK);
    let text2 = String::from_utf8(body2).unwrap();
    assert!(text2.starts_with("#EXTM3U\n"));
    assert!(text2.contains("seg_119.ts"));
    assert!(text2.contains("#EXT-X-ENDLIST"));

    let recorded = seen_ranges.lock().unwrap().clone();
    assert_eq!(
        recorded,
        vec![
            Some("bytes=0-511".to_string()),
            None,
            Some("bytes=200-711".to_string()),
            None,
        ]
    );

    srv.abort();
}

#[tokio::test]
async fn proxy_rejects_xml_declared_svg_and_refetches_extensionless_octet_stream_range_hls_e2e() {
    let (_tmp, app, _admin_tok, member_tok) = setup_test_app(true).await;

    let mut manifest = String::from("#EXTM3U\n#EXT-X-VERSION:3\n#EXT-X-TARGETDURATION:6\n");
    for i in 0..80 {
        manifest.push_str(&format!("#EXTINF:6.0,\nseg_{i:03}.ts\n"));
    }
    manifest.push_str("#EXT-X-ENDLIST\n");
    let manifest_bytes = Arc::new(manifest.into_bytes());
    let manifest_srv = manifest_bytes.clone();

    let origin_app = Router::new()
        .route(
            "/xml-svg.bin",
            get(|| async {
                Response::builder()
                    .status(StatusCode::OK)
                    .header("content-type", "application/octet-stream")
                    .body(Body::from(
                        "<?xml version=\"1.0\" encoding=\"UTF-8\"?><svg xmlns=\"http://www.w3.org/2000/svg\"><script>alert(1)</script></svg>",
                    ))
                    .unwrap()
            }),
        )
        .route(
            "/xml-svg-split.bin",
            get(|| async {
                delayed_chunked_response(
                    "application/octet-stream",
                    vec![
                        (b"<?xml version=\"1.0\" encoding=\"UTF-8\"?>".to_vec(), 0),
                        (b"<svg xmlns=\"http://www.w3.org/2000/svg\"><script>alert(1)</script></svg>".to_vec(), 10),
                    ],
                )
            }),
        )
        .route(
            "/manifest",
            get(move |headers: HeaderMap| {
                let full = manifest_srv.clone();
                async move {
                    if let Some(r) = headers.get(header::RANGE).and_then(|v| v.to_str().ok()) {
                        let spec = r.trim().strip_prefix("bytes=").unwrap_or("10-500");
                        let (s, e) = spec.split_once('-').unwrap_or(("10", "500"));
                        let start: usize = s.parse().unwrap_or(10);
                        let end: usize = e.parse().unwrap_or(500).min(full.len().saturating_sub(1));
                        let slice = full[start..=end].to_vec();
                        return Response::builder()
                            .status(StatusCode::PARTIAL_CONTENT)
                            .header("content-type", "application/octet-stream")
                            .header(
                                "content-range",
                                format!("bytes {start}-{end}/{}", full.len()),
                            )
                            .body(Body::from(slice))
                            .unwrap();
                    }
                    Response::builder()
                        .status(StatusCode::OK)
                        .header("content-type", "application/octet-stream")
                        .body(Body::from(full.as_ref().clone()))
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

    // GAP-04: single-chunk and split-chunk <?xml ...?><svg> rejected on both stream-proxy and hls-proxy
    for endpoint in ["stream-proxy", "hls-proxy"] {
        for path in ["/xml-svg.bin", "/xml-svg-split.bin"] {
            let encoded = urlencoding_param(&format!("{origin}{path}"));
            let (st, _, _) = get_bytes_with_auth(
                &app,
                &format!("/api/v1/video/online/{endpoint}?url={encoded}"),
                Some(&member_tok),
                None,
            )
            .await;
            assert_eq!(
                st,
                StatusCode::BAD_REQUEST,
                "expected 400 on {endpoint} for {path}"
            );
        }
    }

    // GAP-05: mid-file Range on extensionless /manifest with application/octet-stream refetches and rewrites
    let encoded_manifest = urlencoding_param(&format!("{origin}/manifest"));
    let (st_hls, hdrs_hls, body_hls) = get_bytes_with_auth(
        &app,
        &format!("/api/v1/video/online/hls-proxy?url={encoded_manifest}"),
        Some(&member_tok),
        Some("bytes=10-500"),
    )
    .await;
    assert_eq!(st_hls, StatusCode::OK);
    let ct = hdrs_hls
        .get(header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");
    assert!(
        ct.contains("mpegurl"),
        "expected mpegurl content-type, got {ct}"
    );
    let rewritten = String::from_utf8(body_hls).unwrap();
    assert!(rewritten.starts_with("#EXTM3U\n"));
    assert!(rewritten.contains("seg_079.ts"));
    assert!(rewritten.contains("/api/v1/video/online/hls-proxy?url="));

    srv.abort();
}

#[tokio::test]
async fn video_caller_url_guards_chunked_partial_hls_and_utf16_comment_proxy_e2e() {
    let (_tmp, app, admin_tok, member_tok) = setup_test_app(true).await;

    let src_json = serde_json::json!([{
        "sourceName": "GuardVideoSource",
        "sourceUrl": "https://video-guard.example",
        "sourceFormat": "rule",
        "searchUrl": "/search?q={{key}}",
        "ruleSearch": { "list": "$.items", "title": "$.title", "detailUrl": "$.url" },
        "ruleDetail": { "title": "$.title" },
        "rulePlay": { "requestUrl": "{{playUrl}}", "streamUrl": "$.url" }
    }]);
    let (st_imp, imp_res) = post_json(
        &app,
        "/api/v1/video/sources/import",
        &admin_tok,
        serde_json::json!({ "raw_json": src_json.to_string() }),
    )
    .await;
    assert_eq!(st_imp, StatusCode::OK);
    let sid = imp_res["sources"][0]["id"].as_str().unwrap().to_string();

    // I2-GAP-02: detail and play reject Legado options and data: URLs with 400 Bad Request
    for bad_url in [
        "https://video-guard.example/v/1,{\"method\":\"POST\",\"headers\":{\"X-Evil\":\"1\"}}",
        "data:text/html,<h1>evil</h1>",
    ] {
        let (st_d, _) = post_json(
            &app,
            "/api/v1/video/online/detail",
            &member_tok,
            serde_json::json!({ "source_id": sid, "detail_url": bad_url }),
        )
        .await;
        assert_eq!(
            st_d,
            StatusCode::BAD_REQUEST,
            "expected 400 on detail for {bad_url}"
        );

        let (st_p, _) = post_json(
            &app,
            "/api/v1/video/online/play",
            &member_tok,
            serde_json::json!({ "source_id": sid, "play_url": bad_url }),
        )
        .await;
        assert_eq!(
            st_p,
            StatusCode::BAD_REQUEST,
            "expected 400 on play for {bad_url}"
        );
    }

    // I2-GAP-03 & I2-GAP-05: mock origin serving chunked 206 partial HLS (>=16 bytes before #EXTINF:)
    // plus UTF-16LE/BE SVG and >4KB HTML comment + SVG payloads.
    let full_playlist = "#EXTM3U\n#EXT-X-VERSION:3\n#EXT-X-TARGETDURATION:5\n#EXTINF:5.0,\nsegment_0001_very_long_url.ts\n#EXTINF:5.0,\nsegment_0002.ts\n#EXT-X-ENDLIST\n";
    let origin_app = Router::new()
        .route(
            "/chunked-manifest",
            get(move |headers: HeaderMap| async move {
                if let Some(r) = headers.get(header::RANGE).and_then(|v| v.to_str().ok()) {
                    let _ = r;
                    let mut resp = delayed_chunked_response(
                        "application/octet-stream",
                        vec![
                            (b"ment_0001_very_long_url.ts\n".to_vec(), 0),
                            (
                                b"#EXTINF:5.0,\nsegment_0002.ts\n#EXT-X-ENDLIST\n".to_vec(),
                                5,
                            ),
                        ],
                    );
                    *resp.status_mut() = StatusCode::PARTIAL_CONTENT;
                    resp.headers_mut().insert(
                        header::CONTENT_RANGE,
                        header::HeaderValue::from_static("bytes 24-120/240"),
                    );
                    return resp;
                }
                Response::builder()
                    .status(StatusCode::OK)
                    .header("content-type", "application/octet-stream")
                    .body(Body::from(full_playlist))
                    .unwrap()
            }),
        )
        .route(
            "/utf16le-svg.bin",
            get(|| async {
                let mut payload = vec![0xFFu8, 0xFE];
                for u in "<svg xmlns=\"http://www.w3.org/2000/svg\"><script>alert(1)</script></svg>"
                    .encode_utf16()
                {
                    payload.extend_from_slice(&u.to_le_bytes());
                }
                delayed_chunked_response(
                    "application/octet-stream",
                    vec![(payload[..4].to_vec(), 0), (payload[4..].to_vec(), 5)],
                )
            }),
        )
        .route(
            "/utf16le-1byte-svg.bin",
            get(|| async {
                let mut payload = vec![0xFFu8, 0xFE];
                for u in "<svg xmlns=\"http://www.w3.org/2000/svg\"><script>alert(1)</script></svg>"
                    .encode_utf16()
                {
                    payload.extend_from_slice(&u.to_le_bytes());
                }
                delayed_chunked_response(
                    "application/octet-stream",
                    vec![
                        (payload[..1].to_vec(), 0),
                        (payload[1..4].to_vec(), 5),
                        (payload[4..].to_vec(), 5),
                    ],
                )
            }),
        )
        .route(
            "/utf16be-svg.bin",
            get(|| async {
                let mut payload = vec![0xFEu8, 0xFF];
                for u in "<svg xmlns=\"http://www.w3.org/2000/svg\"></svg>".encode_utf16() {
                    payload.extend_from_slice(&u.to_be_bytes());
                }
                delayed_chunked_response(
                    "application/octet-stream",
                    vec![(payload[..4].to_vec(), 0), (payload[4..].to_vec(), 5)],
                )
            }),
        )
        .route(
            "/large-comment-svg.bin",
            get(|| async {
                let mut chunk1 = b"<!-- ".to_vec();
                chunk1.extend_from_slice(&vec![b'X'; 4600]);
                let chunk2 =
                    b" --><svg xmlns=\"http://www.w3.org/2000/svg\"><script>alert(1)</script></svg>"
                        .to_vec();
                delayed_chunked_response("application/octet-stream", vec![(chunk1, 0), (chunk2, 5)])
            }),
        )
        .route(
            "/large-ws-svg.bin",
            get(|| async {
                let chunk1 = vec![b' '; 4600];
                let chunk2 =
                    b"<svg xmlns=\"http://www.w3.org/2000/svg\"><script>alert(1)</script></svg>"
                        .to_vec();
                delayed_chunked_response("application/octet-stream", vec![(chunk1, 0), (chunk2, 5)])
            }),
        )
        .route(
            "/utf16le-ws-svg.bin",
            get(|| async {
                let mut payload = vec![0xFFu8, 0xFE];
                for _ in 0..2400 {
                    payload.extend_from_slice(&u16::from(b' ').to_le_bytes());
                }
                for u in "<svg xmlns=\"http://www.w3.org/2000/svg\"></svg>".encode_utf16() {
                    payload.extend_from_slice(&u.to_le_bytes());
                }
                delayed_chunked_response(
                    "application/octet-stream",
                    vec![(payload[..4200].to_vec(), 0), (payload[4200..].to_vec(), 5)],
                )
            }),
        )
        .route(
            "/live",
            get(|| async {
                delayed_chunked_response(
                    "text/plain",
                    vec![
                        (b"#EXT".to_vec(), 0),
                        (b"M3U\n#EXTINF:6.0,\nseg0.ts\n#EXT-X-ENDLIST\n".to_vec(), 5),
                    ],
                )
            }),
        );

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let origin = format!("http://{addr}");
    let srv = tokio::spawn(async move {
        axum::serve(listener, origin_app).await.unwrap();
    });

    // I2-GAP-03 (hls-proxy): 206 chunk 1 >= 16 bytes + chunk 2 #EXTINF: -> refetches 200 & rewrites
    let encoded_manifest = urlencoding_param(&format!("{origin}/chunked-manifest"));
    let (st_hls, hdrs_hls, body_hls) = get_bytes_with_auth(
        &app,
        &format!("/api/v1/video/online/hls-proxy?url={encoded_manifest}"),
        Some(&member_tok),
        Some("bytes=24-120"),
    )
    .await;
    assert_eq!(st_hls, StatusCode::OK);
    assert!(
        hdrs_hls
            .get(header::CONTENT_TYPE)
            .and_then(|v| v.to_str().ok())
            .unwrap_or("")
            .contains("mpegurl")
    );
    let rewritten = String::from_utf8(body_hls).unwrap();
    assert!(rewritten.starts_with("#EXTM3U\n"));
    assert!(rewritten.contains("segment_0001_very_long_url.ts"));

    // I2-GAP-03 (stream-proxy): same chunked 206 partial HLS is rejected with 400 Bad Request
    let (st_stream_hls, _, _) = get_bytes_with_auth(
        &app,
        &format!("/api/v1/video/online/stream-proxy?url={encoded_manifest}"),
        Some(&member_tok),
        Some("bytes=24-120"),
    )
    .await;
    assert_eq!(st_stream_hls, StatusCode::BAD_REQUEST);

    // I2-GAP-05 + GAP-05: UTF-16LE/BE SVG, >4KB HTML comment + SVG, and >4KB pure whitespace + SVG rejected on stream-proxy & hls-proxy
    for endpoint in ["stream-proxy", "hls-proxy"] {
        for path in [
            "/utf16le-svg.bin",
            "/utf16le-1byte-svg.bin",
            "/utf16be-svg.bin",
            "/large-comment-svg.bin",
            "/large-ws-svg.bin",
            "/utf16le-ws-svg.bin",
        ] {
            let encoded = urlencoding_param(&format!("{origin}{path}"));
            let (st, _, _) = get_bytes_with_auth(
                &app,
                &format!("/api/v1/video/online/{endpoint}?url={encoded}"),
                Some(&member_tok),
                None,
            )
            .await;
            assert_eq!(
                st,
                StatusCode::BAD_REQUEST,
                "expected 400 on {endpoint} for {path}"
            );
        }
    }

    // GAP-02: chunked b"#EXT" + b"M3U\n..." (200 OK, text/plain) must rewrite in hls-proxy and reject in stream-proxy
    let encoded_live = urlencoding_param(&format!("{origin}/live"));
    let (st_live_hls, hdrs_live_hls, body_live_hls) = get_bytes_with_auth(
        &app,
        &format!("/api/v1/video/online/hls-proxy?url={encoded_live}"),
        Some(&member_tok),
        None,
    )
    .await;
    assert_eq!(st_live_hls, StatusCode::OK);
    assert!(
        hdrs_live_hls
            .get(header::CONTENT_TYPE)
            .and_then(|v| v.to_str().ok())
            .unwrap_or("")
            .contains("mpegurl")
    );
    let rewritten_live = String::from_utf8(body_live_hls).unwrap();
    assert!(rewritten_live.starts_with("#EXTM3U\n"));
    assert!(rewritten_live.contains("/api/v1/video/online/hls-proxy?url="));

    let (st_live_stream, _, _) = get_bytes_with_auth(
        &app,
        &format!("/api/v1/video/online/stream-proxy?url={encoded_live}"),
        Some(&member_tok),
        None,
    )
    .await;
    assert_eq!(st_live_stream, StatusCode::BAD_REQUEST);

    srv.abort();
}

#[tokio::test]
async fn video_online_search_enforces_source_concurrent_rate_throttling() {
    let (_tmp, app, admin_tok, member_tok) = setup_test_app(true).await;

    let origin_app = Router::new().route(
        "/api/search",
        get(|| async {
            Response::builder()
                .status(StatusCode::OK)
                .header("content-type", "application/json")
                .body(Body::from(
                    r#"{"items":[{"id":"v1","title":"RatedVideo","url":"/v/1"}]}"#,
                ))
                .unwrap()
        }),
    );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let base = format!("http://{addr}");
    let srv = tokio::spawn(async move {
        axum::serve(listener, origin_app).await.unwrap();
    });

    let src = serde_json::json!({
        "sourceName": "RateLimitedVideo",
        "sourceUrl": base,
        "sourceFormat": "rule",
        "concurrentRate": "1/200",
        "searchUrl": format!("{base}/api/search?q={{{{key}}}}"),
        "ruleSearch": {
            "list": "$.items[*]",
            "vodId": "$.id",
            "name": "$.title",
            "detailUrl": "$.url"
        }
    });
    let (st_imp, imp_val) = post_json(
        &app,
        "/api/v1/video/sources/import",
        &admin_tok,
        serde_json::json!({ "raw_json": src.to_string() }),
    )
    .await;
    assert_eq!(st_imp, StatusCode::OK);
    let sid = imp_val["sources"][0]["id"].as_str().unwrap().to_string();

    let start = std::time::Instant::now();
    let (r1, r2) = tokio::join!(
        post_json(
            &app,
            "/api/v1/video/online/search",
            &member_tok,
            serde_json::json!({ "keyword": "a", "source_ids": [&sid] }),
        ),
        post_json(
            &app,
            "/api/v1/video/online/search",
            &member_tok,
            serde_json::json!({ "keyword": "b", "source_ids": [&sid] }),
        ),
    );
    let elapsed = start.elapsed();
    assert_eq!(r1.0, StatusCode::OK);
    assert_eq!(r2.0, StatusCode::OK);
    assert!(
        elapsed >= std::time::Duration::from_millis(160),
        "expected concurrentRate 1/200 to throttle 2 concurrent calls, took {:?}",
        elapsed
    );

    srv.abort();
}

#[tokio::test]
async fn subtitle_proxy_serves_vtt_converts_srt_injects_headers_and_enforces_security_e2e() {
    let (_tmp, app, admin_tok, member_tok) = setup_test_app(true).await;

    let origin_app = Router::new()
        .route(
            "/api/play",
            get(|| async {
                Response::builder()
                    .status(StatusCode::OK)
                    .header("content-type", "application/json")
                    .body(Body::from(
                        serde_json::json!({
                            "stream": "/media/ep1.mp4",
                            "subs": [
                                { "label": "English", "lang": "en", "url": "/subs/en.srt" },
                                { "label": "中文", "lang": "zh", "url": "/subs/zh.vtt" }
                            ]
                        })
                        .to_string(),
                    ))
                    .unwrap()
            }),
        )
        .route(
            "/subs/en.srt",
            get(|headers: HeaderMap| async move {
                if headers.get(header::REFERER).and_then(|v| v.to_str().ok())
                    != Some("https://sub.origin.test/")
                {
                    return Response::builder()
                        .status(StatusCode::FORBIDDEN)
                        .body(Body::from("missing referer"))
                        .unwrap();
                }
                Response::builder()
                    .status(StatusCode::OK)
                    .header("content-type", "text/plain; charset=utf-8")
                    .body(Body::from(
                        "1\n00:00:01,000 --> 00:00:03,500\nHello, Subtitle!\n",
                    ))
                    .unwrap()
            }),
        )
        .route(
            "/subs/zh.vtt",
            get(|| async {
                Response::builder()
                    .status(StatusCode::OK)
                    .header("content-type", "text/vtt; charset=utf-8")
                    .body(Body::from(
                        "WEBVTT\n\n00:00:01.000 --> 00:00:03.500\n你好字幕\n",
                    ))
                    .unwrap()
            }),
        )
        .route(
            "/subs/evil.vtt",
            get(|| async {
                Response::builder()
                    .status(StatusCode::OK)
                    .header("content-type", "text/plain")
                    .body(Body::from(
                        "WEBVTT\n\n00:00:01.000 --> 00:00:02.000\n<svg onload=alert(1)>\n",
                    ))
                    .unwrap()
            }),
        )
        .route(
            "/subs/huge.vtt",
            get(|| async {
                let mut body = b"WEBVTT\n\n00:00:01.000 --> 00:00:02.000\n".to_vec();
                body.extend(vec![b'x'; 4 * 1024 * 1024 + 128]);
                Response::builder()
                    .status(StatusCode::OK)
                    .header("content-type", "text/vtt")
                    .body(Body::from(body))
                    .unwrap()
            }),
        );

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let origin = format!("http://{addr}");
    let srv = tokio::spawn(async move {
        axum::serve(listener, origin_app).await.unwrap();
    });

    let src_json = serde_json::json!({
        "sourceName": "Subtitle Proxy Source",
        "sourceUrl": origin,
        "sourceFormat": "rule",
        "mediaHeader": { "Referer": "https://sub.origin.test/" },
        "rulePlay": {
            "requestUrl": "{{playUrl}}",
            "streamUrl": "$.stream",
            "subtitles": "$.subs"
        }
    });
    let (st_imp, imp_res) = post_json(
        &app,
        "/api/v1/video/sources/import",
        &admin_tok,
        serde_json::json!({ "raw_json": src_json.to_string() }),
    )
    .await;
    assert_eq!(st_imp, StatusCode::OK);
    let sid = imp_res["sources"][0]["id"].as_str().unwrap().to_string();

    let (st_play, play_res) = post_json(
        &app,
        "/api/v1/video/online/play",
        &member_tok,
        serde_json::json!({
            "source_id": sid,
            "play_url": format!("{origin}/api/play")
        }),
    )
    .await;
    assert_eq!(st_play, StatusCode::OK);
    let subs = play_res["subtitles"].as_array().unwrap();
    assert_eq!(subs.len(), 2);
    let srt_proxy = subs[0]["proxy_url"].as_str().unwrap();
    assert!(srt_proxy.starts_with("/api/v1/video/online/subtitle-proxy?url="));

    // 1. Authenticated member fetches SRT subtitle via proxy -> converted to WebVTT with security headers
    let (st_srt, hdrs_srt, body_srt) =
        get_bytes_with_auth(&app, srt_proxy, Some(&member_tok), None).await;
    assert_eq!(st_srt, StatusCode::OK);
    assert_eq!(
        hdrs_srt
            .get(header::CONTENT_TYPE)
            .and_then(|v| v.to_str().ok()),
        Some("text/vtt; charset=utf-8")
    );
    assert_eq!(
        hdrs_srt
            .get(header::X_CONTENT_TYPE_OPTIONS)
            .and_then(|v| v.to_str().ok()),
        Some("nosniff")
    );
    assert_eq!(
        hdrs_srt
            .get(header::CONTENT_SECURITY_POLICY)
            .and_then(|v| v.to_str().ok()),
        Some("sandbox; default-src 'none'")
    );
    let srt_text = String::from_utf8(body_srt).unwrap();
    assert!(srt_text.starts_with("WEBVTT\n\n"));
    assert!(srt_text.contains("00:00:01.000 --> 00:00:03.500\nHello, Subtitle!"));

    // 2. VTT passthrough works cleanly
    let vtt_proxy = subs[1]["proxy_url"].as_str().unwrap();
    let (st_vtt, _, body_vtt) = get_bytes_with_auth(&app, vtt_proxy, Some(&member_tok), None).await;
    assert_eq!(st_vtt, StatusCode::OK);
    assert!(String::from_utf8(body_vtt).unwrap().contains("你好字幕"));

    // 3. Unauthenticated request rejected with 401
    let (st_unauth, _, _) = get_bytes_with_auth(&app, srt_proxy, None, None).await;
    assert_eq!(st_unauth, StatusCode::UNAUTHORIZED);

    // 4. SVG/script injection and >4 MiB oversized subtitle rejected with 400
    for bad_path in ["/subs/evil.vtt", "/subs/huge.vtt"] {
        let encoded = urlencoding_param(&format!("{origin}{bad_path}"));
        let (st_bad, _, _) = get_bytes_with_auth(
            &app,
            &format!("/api/v1/video/online/subtitle-proxy?url={encoded}&source_id={sid}"),
            Some(&member_tok),
            None,
        )
        .await;
        assert_eq!(
            st_bad,
            StatusCode::BAD_REQUEST,
            "expected 400 for {bad_path}"
        );
    }

    // 5. Hardened default app blocks loopback SSRF on subtitle-proxy
    let (_tmp_h, app_hardened, _adm_h, mem_h) = setup_test_app(false).await;
    let encoded_loop = urlencoding_param(&format!("{origin}/subs/zh.vtt"));
    let (st_ssrf, _, _) = get_bytes_with_auth(
        &app_hardened,
        &format!("/api/v1/video/online/subtitle-proxy?url={encoded_loop}"),
        Some(&mem_h),
        None,
    )
    .await;
    assert_eq!(st_ssrf, StatusCode::BAD_REQUEST);

    srv.abort();
}

#[tokio::test]
async fn video_online_endpoints_enforce_cancel_on_future_drop() {
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
                    r#"{"items":[{"id":"v1","title":"Item"}],"cats":[{"id":"c1","name":"Cat"}],"title":"Video","stream":"https://cdn.example/v.mp4"}"#
                }
            }),
        )
        .route(
            "/step2",
            get(move || {
                let hits = step2_hits_c.clone();
                async move {
                    hits.fetch_add(1, Ordering::SeqCst);
                    r#"{"ok":true,"items":[{"id":"v1","title":"Step2"}],"cats":[{"id":"c1","name":"Cat2"}],"title":"Step2","stream":"https://cdn.example/v2.mp4"}"#
                }
            }),
        );

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let base = format!("http://{addr}");
    let srv = tokio::spawn(async move {
        axum::serve(listener, origin_app).await.unwrap();
    });

    let (_tmp, app, admin_tok, member_tok) = setup_test_app(true).await;

    let src_json = serde_json::json!({
        "sourceName": "DropVideoSource",
        "sourceUrl": base,
        "sourceFormat": "rule",
        "searchable": true,
        "enabledExplore": true,
        "categoriesUrl": format!("{base}/step1"),
        "ruleCategories": {
            "list": "$.cats[*]",
            "id": "$.id",
            "name": "@js:java.ajax(baseUrl.replace('/step1', '/step2')); 'Cat'"
        },
        "searchUrl": format!("{base}/step1?q={{{{key}}}}"),
        "ruleSearch": {
            "list": "$.items[*]",
            "title": "@js:java.ajax(baseUrl.replace('/step1', '/step2')); 'Title'",
            "videoId": "$.id"
        },
        "exploreUrl": format!("{base}/step1?p={{{{page}}}}"),
        "ruleExplore": {
            "list": "$.items[*]",
            "title": "@js:java.ajax(baseUrl.replace('/step1', '/step2')); 'Browse'",
            "videoId": "$.id"
        },
        "ruleDetail": {
            "requestUrl": format!("{base}/step1"),
            "name": "@js:java.ajax(baseUrl.replace('/step1', '/step2')); 'Detail'",
            "playUrl": "https://cdn.example/ep1.mp4"
        },
        "rulePlay": {
            "requestUrl": format!("{base}/step1"),
            "streamUrl": "@js:java.ajax(baseUrl.replace('/step1', '/step2')); 'https://cdn.example/v.mp4'"
        }
    });

    let (st_imp, imp_res) = post_json(
        &app,
        "/api/v1/video/sources/import",
        &admin_tok,
        serde_json::json!({ "raw_json": src_json.to_string() }),
    )
    .await;
    assert_eq!(st_imp, StatusCode::OK);
    let sid = imp_res["sources"][0]["id"].as_str().unwrap().to_string();

    for (uri, payload) in [
        (
            "/api/v1/video/online/categories",
            serde_json::json!({ "source_id": sid }),
        ),
        (
            "/api/v1/video/online/browse",
            serde_json::json!({ "source_id": sid, "page": 1 }),
        ),
        (
            "/api/v1/video/online/detail",
            serde_json::json!({ "source_id": sid, "video_id": "v1" }),
        ),
        (
            "/api/v1/video/online/play",
            serde_json::json!({ "source_id": sid, "play_url": format!("{base}/ep1") }),
        ),
        (
            "/api/v1/video/online/search",
            serde_json::json!({ "keyword": "drop", "source_ids": [sid] }),
        ),
    ] {
        let before_hits = step2_hits.load(Ordering::SeqCst);
        {
            let fut = post_json(&app, uri, &member_tok, payload);
            tokio::pin!(fut);
            tokio::select! {
                _ = &mut fut => panic!("{uri} completed before drop"),
                _ = step1_entered.notified() => {
                    // Drop the outer handler future immediately once step 1 starts.
                }
            }
        }
        tokio::time::sleep(Duration::from_millis(320)).await;
        assert_eq!(
            step2_hits.load(Ordering::SeqCst),
            before_hits,
            "dropping {uri} mid-flight must cancel/abort before step 2 java.ajax"
        );
    }

    srv.abort();
}

#[tokio::test]
async fn video_online_endpoints_enforce_timeout_on_stalled_upstream() {
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::time::Duration;
    use xmedia_core::domain::error::DomainError;
    use xmedia_source::http::HttpClientConfig;

    use crate::application::online::VideoOnlineService;
    use crate::infra::persistence::repositories::ToastyVideoSourceRepository;
    use crate::source::VideoSourceEngine;

    let after_hits = Arc::new(AtomicUsize::new(0));
    let after_hits_c = after_hits.clone();

    let origin_app = Router::new()
        .route(
            "/hop1",
            get(|| async {
                tokio::time::sleep(Duration::from_millis(90)).await;
                r#"{"next":"/stall"}"#
            }),
        )
        .route(
            "/stall",
            get(|| async {
                tokio::time::sleep(Duration::from_secs(2)).await;
                r#"{"cats":[{"id":"1","name":"Cat"}],"items":[{"id":"1","title":"Late"}],"title":"Late","stream":"https://cdn.example/late.mp4"}"#
            }),
        )
        .route(
            "/after",
            get(move || {
                let hits = after_hits_c.clone();
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

    let tmp = TempDir::new().unwrap();
    let db_path = tmp.path().join("xmedia_video_timeout.db");
    let db_url = format!("sqlite:{}", db_path.display());
    let mut models = ModelSet::new();
    register_core_models(&mut models);
    register_video_models(&mut models);
    let mut db = Db::builder().models(models).connect(&db_url).await.unwrap();
    ensure_core_schema(&mut db, &db_url).await.unwrap();
    ensure_video_schema(&mut db, &db_url).await.unwrap();

    let users_repo = Arc::new(ToastyUserRepository::new(db.clone()));
    let lifecycles = Arc::new(RwLock::new(HashMap::new()));
    let security = Arc::new(SecurityManager::new("video-timeout-secret"));
    let tokens = Arc::new(JwtTokenService::new("video-timeout-secret", 3600));
    let auth_svc = Arc::new(AuthService::new(users_repo, security, tokens, lifecycles));
    let admin_session = auth_svc
        .setup_initial_admin(SetupAdminInput {
            username: "admin".into(),
            display_name: Some("Admin".into()),
            password: "adminpassword123".into(),
        })
        .await
        .unwrap();
    let admin = auth_svc
        .authenticate_jwt(&admin_session.token)
        .await
        .unwrap();

    let sources_repo = Arc::new(ToastyVideoSourceRepository::new(db));
    let engine = VideoSourceEngine::new(
        HttpClientConfig {
            allow_private_ips: true,
            ..Default::default()
        },
        Duration::from_secs(5),
    );
    let svc = VideoOnlineService::with_engine(sources_repo, engine, true)
        .with_timeouts(Duration::from_millis(120), Duration::from_millis(160));

    let raw_src = serde_json::json!({
        "sourceName": "StalledMultiHopVideo",
        "sourceUrl": base,
        "sourceFormat": "rule",
        "enabledExplore": true,
        "categoriesUrl": format!("<js>java.ajax('{base}/hop1'); '{base}/stall'</js>"),
        "ruleCategories": {
            "list": "$.cats[*]",
            "id": "$.id",
            "name": "@js:java.ajax(baseUrl.replace('/stall', '/after')); 'Cat'"
        },
        "exploreUrl": format!("<js>java.ajax('{base}/hop1'); '{base}/stall'</js>"),
        "ruleExplore": {
            "list": "$.items[*]",
            "title": "@js:java.ajax(baseUrl.replace('/stall', '/after')); 'Item'",
            "videoId": "$.id"
        },
        "ruleDetail": {
            "requestUrl": format!("<js>java.ajax('{base}/hop1'); '{base}/stall'</js>"),
            "name": "@js:java.ajax(baseUrl.replace('/stall', '/after')); 'Detail'",
            "playUrl": "https://cdn.example/ep1.mp4"
        },
        "rulePlay": {
            "requestUrl": format!("<js>java.ajax('{base}/hop1'); '{base}/stall'</js>"),
            "streamUrl": "@js:java.ajax(baseUrl.replace('/stall', '/after')); 'https://cdn.example/v.mp4'"
        }
    });
    let imported = svc
        .import_sources(&admin, &raw_src.to_string())
        .await
        .unwrap();
    let sid = imported.sources[0].id;

    let t_start = std::time::Instant::now();
    let cat_err = svc.categories(&admin, sid).await.unwrap_err();
    assert!(matches!(cat_err, DomainError::Validation(ref m) if m.contains("超时")));

    let browse_err = svc.browse(&admin, sid, None, 1).await.unwrap_err();
    assert!(matches!(browse_err, DomainError::Validation(ref m) if m.contains("超时")));

    let detail_err = svc.detail(&admin, sid, Some("1"), None).await.unwrap_err();
    assert!(matches!(detail_err, DomainError::Validation(ref m) if m.contains("超时")));

    let play_err = svc
        .play(&admin, sid, &format!("{base}/ep1"), None)
        .await
        .unwrap_err();
    assert!(matches!(play_err, DomainError::Validation(ref m) if m.contains("超时")));

    assert!(
        t_start.elapsed() < Duration::from_secs(1),
        "all 4 timed-out video calls should finish well under 2s stall, took {:?}",
        t_start.elapsed()
    );
    assert_eq!(after_hits.load(Ordering::SeqCst), 0);

    srv.abort();
}

#[tokio::test]
async fn video_play_headers_applied_matches_build_sanitized_headers() {
    let (_tmp, app, admin_tok, member_tok) = setup_test_app(true).await;

    let origin_app = Router::new()
        .route(
            "/api/play",
            get(|| async { r#"{"stream":"/media/video.mp4"}"# }),
        )
        .route(
            "/media/video.mp4",
            get(|| async {
                Response::builder()
                    .status(StatusCode::OK)
                    .header("content-type", "video/mp4")
                    .body(Body::from(fake_mp4_payload()))
                    .unwrap()
            }),
        );

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let origin = format!("http://{addr}");
    let srv = tokio::spawn(async move {
        axum::serve(listener, origin_app).await.unwrap();
    });

    // 1. Forbidden / invalid headers only (applecms_v10 + rule) -> headers_applied == false
    let forbidden_applecms = serde_json::json!({
        "sourceName": "ForbiddenHeadersApplecms",
        "apiUrl": format!("{origin}/applecms/api.php/provide/vod/"),
        "header": r#"{"Host":"evil.com","Connection":"close"}"#,
        "mediaHeader": r#"{"Transfer-Encoding":"chunked","Bad\nName":"v","X-Newline":"a\nb"}"#
    });
    let forbidden_rule = serde_json::json!({
        "sourceName": "ForbiddenHeadersRule",
        "sourceUrl": origin,
        "sourceFormat": "rule",
        "header": r#"{"Host":"evil.com"}"#,
        "mediaHeader": r#"{"Connection":"close"}"#,
        "rulePlay": {
            "requestUrl": "{{playUrl}}",
            "streamUrl": "$.stream"
        }
    });
    // 2. Valid custom header alongside forbidden header -> headers_applied == true
    let valid_rule = serde_json::json!({
        "sourceName": "ValidCustomHeaderRule",
        "sourceUrl": origin,
        "sourceFormat": "rule",
        "header": r#"{"Host":"evil.com"}"#,
        "mediaHeader": r#"{"Referer":"https://player.example.com/"}"#,
        "rulePlay": {
            "requestUrl": "{{playUrl}}",
            "streamUrl": "$.stream"
        }
    });

    let (st_imp, imp_res) = post_json(
        &app,
        "/api/v1/video/sources/import",
        &admin_tok,
        serde_json::json!({
            "raw_json": serde_json::Value::Array(vec![forbidden_applecms, forbidden_rule, valid_rule]).to_string()
        }),
    )
    .await;
    assert_eq!(st_imp, StatusCode::OK);
    let sources = imp_res["sources"].as_array().unwrap();
    let id_forbidden_applecms = sources[0]["id"].as_str().unwrap();
    let id_forbidden_rule = sources[1]["id"].as_str().unwrap();
    let id_valid_rule = sources[2]["id"].as_str().unwrap();

    let (st1, play1) = post_json(
        &app,
        "/api/v1/video/online/play",
        &member_tok,
        serde_json::json!({
            "source_id": id_forbidden_applecms,
            "play_url": format!("{origin}/media/video.mp4")
        }),
    )
    .await;
    assert_eq!(st1, StatusCode::OK);
    assert_eq!(play1["headers_applied"], false);

    let (st2, play2) = post_json(
        &app,
        "/api/v1/video/online/play",
        &member_tok,
        serde_json::json!({
            "source_id": id_forbidden_rule,
            "play_url": format!("{origin}/api/play")
        }),
    )
    .await;
    assert_eq!(st2, StatusCode::OK);
    assert_eq!(play2["headers_applied"], false);

    let (st3, play3) = post_json(
        &app,
        "/api/v1/video/online/play",
        &member_tok,
        serde_json::json!({
            "source_id": id_valid_rule,
            "play_url": format!("{origin}/api/play")
        }),
    )
    .await;
    assert_eq!(st3, StatusCode::OK);
    assert_eq!(play3["headers_applied"], true);

    srv.abort();
}

#[tokio::test]
async fn video_proxies_strip_static_range_from_source_media_headers() {
    let (_tmp, app, admin_tok, member_tok) = setup_test_app(true).await;

    let seen_hls_ranges: Arc<Mutex<Vec<Option<String>>>> = Arc::new(Mutex::new(Vec::new()));
    let seen_hls_referers: Arc<Mutex<Vec<Option<String>>>> = Arc::new(Mutex::new(Vec::new()));
    let seen_mp4_ranges: Arc<Mutex<Vec<Option<String>>>> = Arc::new(Mutex::new(Vec::new()));
    let seen_mp4_referers: Arc<Mutex<Vec<Option<String>>>> = Arc::new(Mutex::new(Vec::new()));

    let hls_ranges_c = seen_hls_ranges.clone();
    let hls_refs_c = seen_hls_referers.clone();
    let mp4_ranges_c = seen_mp4_ranges.clone();
    let mp4_refs_c = seen_mp4_referers.clone();

    let playlist_body = "#EXTM3U\n#EXT-X-VERSION:3\n#EXT-X-TARGETDURATION:10\n#EXTINF:10.0,\nseg0.ts\n#EXTINF:10.0,\nseg1.ts\n#EXT-X-ENDLIST\n";
    let mp4_bytes = fake_mp4_payload();

    let origin_app = Router::new()
        .route(
            "/hls/index.m3u8",
            get(move |headers: HeaderMap| {
                let ranges = hls_ranges_c.clone();
                let refs = hls_refs_c.clone();
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
                        // Simulate upstream returning truncated 206 if Range was leaked!
                        return Response::builder()
                            .status(StatusCode::PARTIAL_CONTENT)
                            .header("content-type", "application/vnd.apple.mpegurl")
                            .header(
                                "content-range",
                                format!("bytes 0-10/{}", playlist_body.len()),
                            )
                            .body(Body::from(&playlist_body[..11]))
                            .unwrap();
                    }
                    Response::builder()
                        .status(StatusCode::OK)
                        .header("content-type", "application/vnd.apple.mpegurl")
                        .body(Body::from(playlist_body))
                        .unwrap()
                }
            }),
        )
        .route(
            "/media/video.mp4",
            get(move |headers: HeaderMap| {
                let ranges = mp4_ranges_c.clone();
                let refs = mp4_refs_c.clone();
                let full = mp4_bytes.clone();
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
                    if rng.as_deref() == Some("bytes=0-15") {
                        return Response::builder()
                            .status(StatusCode::PARTIAL_CONTENT)
                            .header("content-type", "video/mp4")
                            .header("content-range", format!("bytes 0-15/{}", full.len()))
                            .body(Body::from(full[..16].to_vec()))
                            .unwrap();
                    }
                    if rng.is_some() {
                        return Response::builder()
                            .status(StatusCode::PARTIAL_CONTENT)
                            .header("content-type", "video/mp4")
                            .header("content-range", format!("bytes 0-10/{}", full.len()))
                            .body(Body::from(full[..11].to_vec()))
                            .unwrap();
                    }
                    Response::builder()
                        .status(StatusCode::OK)
                        .header("content-type", "video/mp4")
                        .body(Body::from(full))
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
        "sourceName": "StaticRangeSource",
        "sourceUrl": origin,
        "sourceFormat": "rule",
        "header": r#"{"range":"bytes=0-5"}"#,
        "mediaHeader": {
            "Range": "bytes=0-10",
            "Referer": "https://ok.example/"
        },
        "rulePlay": {
            "requestUrl": "{{playUrl}}",
            "streamUrl": "$.stream"
        }
    });
    let (st_imp, imp_res) = post_json(
        &app,
        "/api/v1/video/sources/import",
        &admin_tok,
        serde_json::json!({ "raw_json": src_json.to_string() }),
    )
    .await;
    assert_eq!(st_imp, StatusCode::OK);
    let sid = imp_res["sources"][0]["id"].as_str().unwrap();

    // 1. open_hls_proxy with range_header = None must NOT send static Range and must return full rewritten playlist
    let hls_url = urlencoding_param(&format!("{origin}/hls/index.m3u8"));
    let (st_hls, _, body_hls) = get_bytes_with_auth(
        &app,
        &format!("/api/v1/video/online/hls-proxy?url={hls_url}&source_id={sid}"),
        Some(&member_tok),
        None,
    )
    .await;
    assert_eq!(st_hls, StatusCode::OK);
    let hls_text = String::from_utf8(body_hls).unwrap();
    assert!(hls_text.contains("seg0.ts"), "missing seg0.ts: {hls_text}");
    assert!(hls_text.contains("seg1.ts"), "missing seg1.ts: {hls_text}");
    assert_eq!(*seen_hls_ranges.lock().unwrap(), vec![None]);
    assert_eq!(
        *seen_hls_referers.lock().unwrap(),
        vec![Some("https://ok.example/".to_string())]
    );

    // 2. open_stream_proxy with range_header = None must NOT send static Range and must return 200 OK full payload
    let mp4_url = urlencoding_param(&format!("{origin}/media/video.mp4"));
    let (st_mp4_full, _, body_mp4_full) = get_bytes_with_auth(
        &app,
        &format!("/api/v1/video/online/stream-proxy?url={mp4_url}&source_id={sid}"),
        Some(&member_tok),
        None,
    )
    .await;
    assert_eq!(st_mp4_full, StatusCode::OK);
    assert_eq!(body_mp4_full.len(), fake_mp4_payload().len());

    // 3. open_stream_proxy with explicit client Range MUST still forward client Range and return 206
    let (st_mp4_rng, hdrs_rng, body_mp4_rng) = get_bytes_with_auth(
        &app,
        &format!("/api/v1/video/online/stream-proxy?url={mp4_url}&source_id={sid}"),
        Some(&member_tok),
        Some("bytes=0-15"),
    )
    .await;
    assert_eq!(st_mp4_rng, StatusCode::PARTIAL_CONTENT);
    assert_eq!(body_mp4_rng.len(), 16);
    assert_eq!(
        hdrs_rng
            .get(header::CONTENT_RANGE)
            .and_then(|v| v.to_str().ok()),
        Some("bytes 0-15/4096")
    );
    assert_eq!(
        *seen_mp4_ranges.lock().unwrap(),
        vec![None, Some("bytes=0-15".to_string())]
    );
    assert_eq!(
        *seen_mp4_referers.lock().unwrap(),
        vec![
            Some("https://ok.example/".to_string()),
            Some("https://ok.example/".to_string()),
        ]
    );

    srv.abort();
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

#[tokio::test]
async fn online_video_items_favorites_episode_progress_and_availability_e2e() {
    let (_tmp, app, admin_tok, member_tok) = setup_test_app(true).await;

    let src_json = serde_json::json!({
        "sourceName": "Test Video Source",
        "sourceUrl": "https://video-item.test",
        "sourceGroup": "Test",
        "sourceType": 1,
        "apiUrl": "https://video-item.test/api.php/provide/vod"
    });
    let (st_imp, imp_val) = post_json(
        &app,
        "/api/v1/video/sources/import",
        &admin_tok,
        serde_json::json!({ "raw_json": src_json.to_string() }),
    )
    .await;
    assert_eq!(st_imp, StatusCode::OK);
    let source_id = imp_val["sources"][0]["id"].as_str().unwrap().to_string();

    // 1. Upsert online video with episode + play-line (unit_group) + watch position in seconds
    let (st_put, item) = send_json_with_method(
        &app,
        "PUT",
        "/api/v1/video/online/items",
        &member_tok,
        Some(serde_json::json!({
            "source_id": source_id,
            "item_key": "https://video-item.test/vod/101",
            "title": "Spirited Away Series",
            "subtitle": "2024 · Anime",
            "cover_url": "https://video-item.test/poster.jpg",
            "kind": "anime",
            "extra": { "vod_id": "101", "remarks": "12 Episodes" },
            "favorite": true,
            "progress": {
                "unit_index": 1,
                "unit_key": "https://video-item.test/play/101-2.m3u8",
                "unit_title": "EP 02",
                "unit_group": "HD Line 1",
                "position": 600.0,
                "duration": 1200.0,
                "total_units": 12,
                "completed": false
            }
        })),
    )
    .await;
    assert_eq!(st_put, StatusCode::OK);
    let item_id = item["id"].as_str().unwrap().to_string();
    assert_eq!(item["media_kind"], "video");
    assert_eq!(item["source_name"], "Test Video Source");
    assert_eq!(item["availability"], "ok");
    assert_eq!(item["progress"]["unit_group"], "HD Line 1");
    assert!((item["progress_ratio"].as_f64().unwrap() - (1.5 / 12.0)).abs() < 1e-6);

    // 2. Lookup & Continue/Favorites queries
    let (st_lk, lk) = post_json(
        &app,
        "/api/v1/video/online/items/lookup",
        &member_tok,
        serde_json::json!({
            "items": [
                { "source_id": source_id, "item_key": "https://video-item.test/vod/101" }
            ]
        }),
    )
    .await;
    assert_eq!(st_lk, StatusCode::OK);
    assert_eq!(lk["total"], 1);

    let (st_cont, cont) = send_json_with_method(
        &app,
        "GET",
        "/api/v1/video/online/items?view=continue",
        &member_tok,
        None,
    )
    .await;
    assert_eq!(st_cont, StatusCode::OK);
    assert_eq!(cont["total"], 1);
    assert_eq!(cont["items"][0]["id"], item_id);

    // 3. Switch play line / episode via PUT /{id}/progress
    let (st_prog, prog) = send_json_with_method(
        &app,
        "PUT",
        &format!("/api/v1/video/online/items/{item_id}/progress"),
        &member_tok,
        Some(serde_json::json!({
            "unit_index": 2,
            "unit_key": "https://video-item.test/play/101-3.m3u8",
            "unit_title": "EP 03",
            "unit_group": "Backup Line",
            "position": 300.0,
            "duration": 1200.0,
            "total_units": 12,
            "completed": false
        })),
    )
    .await;
    assert_eq!(st_prog, StatusCode::OK);
    assert_eq!(prog["progress"]["unit_group"], "Backup Line");
    assert_eq!(prog["progress"]["unit_index"], 2);

    // 4. Disable source -> source_disabled
    let (st_dis, _) = patch_json(
        &app,
        &format!("/api/v1/video/sources/{source_id}"),
        &admin_tok,
        serde_json::json!({ "enabled": false }),
    )
    .await;
    assert_eq!(st_dis, StatusCode::OK);

    let (st_get, got) = send_json_with_method(
        &app,
        "GET",
        &format!("/api/v1/video/online/items/{item_id}"),
        &member_tok,
        None,
    )
    .await;
    assert_eq!(st_get, StatusCode::OK);
    assert_eq!(got["availability"], "source_disabled");

    // 5. Clear progress removes from continue view; delete returns 204
    let (st_clr, _) = send_json_with_method(
        &app,
        "DELETE",
        &format!("/api/v1/video/online/items/{item_id}/progress"),
        &member_tok,
        None,
    )
    .await;
    assert_eq!(st_clr, StatusCode::OK);

    let (st_cont2, cont2) = send_json_with_method(
        &app,
        "GET",
        "/api/v1/video/online/items?view=continue",
        &member_tok,
        None,
    )
    .await;
    assert_eq!(st_cont2, StatusCode::OK);
    assert_eq!(cont2["total"], 0);

    let (st_del, _) = send_json_with_method(
        &app,
        "DELETE",
        &format!("/api/v1/video/online/items/{item_id}"),
        &member_tok,
        None,
    )
    .await;
    assert_eq!(st_del, StatusCode::NO_CONTENT);
}

#[tokio::test]
async fn online_tracked_video_movie_show_play_cache_and_refresh_e2e() {
    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

    let (_tmp, app, admin_tok, member_tok) = setup_test_app(true).await;

    let include_ep3 = Arc::new(AtomicBool::new(false));
    let include_ep3_srv = Arc::clone(&include_ep3);
    let mp4_hits = Arc::new(AtomicUsize::new(0));
    let mp4_hits_srv = Arc::clone(&mp4_hits);
    let mp4_bytes = Arc::new(fake_mp4_payload());
    let mp4_for_srv = Arc::clone(&mp4_bytes);

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let base_url = format!("http://{addr}");
    let base_srv = base_url.clone();

    let mock_router = Router::new()
        .route(
            "/api.php/provide/vod/",
            get(move |Query(q): Query<AppleCmsQuery>| {
                let b = base_srv.clone();
                let ep3 = Arc::clone(&include_ep3_srv);
                async move {
                    let ids = q.ids.unwrap_or_default();
                    let list = if ids == "101" {
                        serde_json::json!([{
                            "vod_id": 101,
                            "vod_name": "星际迷航：测试电影",
                            "vod_pic": format!("{b}/poster.png"),
                            "vod_year": "2025",
                            "vod_area": "科幻",
                            "vod_content": "一部用于测试在线追更和离线缓存的电影。",
                            "vod_play_from": "mp4line",
                            "vod_play_url": format!("正片${b}/media/movie.mp4")
                        }])
                    } else if ids == "202" {
                        let mut eps = format!("第01集${b}/media/ep1.mp4#第02集${b}/media/ep2.mp4");
                        if ep3.load(Ordering::SeqCst) {
                            eps.push_str(&format!("#第03集${b}/media/ep3.mp4"));
                        }
                        serde_json::json!([{
                            "vod_id": 202,
                            "vod_name": "星海纪元",
                            "vod_pic": format!("{b}/poster.png"),
                            "vod_year": "2026",
                            "vod_area": "动画",
                            "vod_content": "一部连载中的在线追更剧集。",
                            "vod_play_from": "mp4line",
                            "vod_play_url": eps
                        }])
                    } else {
                        serde_json::json!([])
                    };
                    let body = serde_json::json!({
                        "code": 1,
                        "page": 1,
                        "pagecount": 1,
                        "total": list.as_array().map(|a| a.len()).unwrap_or(0),
                        "list": list
                    });
                    (
                        StatusCode::OK,
                        [(header::CONTENT_TYPE, "application/json")],
                        body.to_string(),
                    )
                }
            }),
        )
        .route(
            "/media/{name}",
            get(move || {
                let hits = Arc::clone(&mp4_hits_srv);
                let bytes = Arc::clone(&mp4_for_srv);
                async move {
                    hits.fetch_add(1, Ordering::SeqCst);
                    (
                        StatusCode::OK,
                        [(header::CONTENT_TYPE, "video/mp4")],
                        (*bytes).clone(),
                    )
                }
            }),
        )
        .route(
            "/poster.png",
            get(|| async {
                (
                    StatusCode::OK,
                    [(header::CONTENT_TYPE, "image/png")],
                    PNG_1X1.to_vec(),
                )
            }),
        );

    let srv_handle = tokio::spawn(async move {
        axum::serve(listener, mock_router).await.unwrap();
    });

    // 1. Import mock AppleCMS v10 source
    let source_spec = serde_json::json!({
        "sourceName": "Mock Tracked Video Source",
        "sourceUrl": base_url,
        "apiUrl": format!("{base_url}/api.php/provide/vod/"),
        "sourceFormat": "applecms_v10",
        "enabled": true
    });
    let (st_imp, imp_val) = send_json_with_method(
        &app,
        "POST",
        "/api/v1/video/sources/import",
        &admin_tok,
        Some(serde_json::json!({ "raw_json": source_spec.to_string() })),
    )
    .await;
    assert_eq!(st_imp, StatusCode::OK, "{imp_val}");
    let source_id = imp_val["sources"][0]["id"].as_str().unwrap().to_string();

    // 2. Admin tracks single-episode movie into the deduplicated pool and binds it
    let (st_mv, mv_val) = send_json_with_method(
        &app,
        "POST",
        "/api/v1/video/online/track",
        &admin_tok,
        Some(serde_json::json!({
            "source_id": source_id,
            "video_id": "101"
        })),
    )
    .await;
    assert_eq!(st_mv, StatusCode::OK, "{mv_val}");
    assert_eq!(mv_val["kind"], "movie");
    assert_eq!(mv_val["added_episodes"], 1);
    let movie_id = mv_val["video"]["id"].as_str().unwrap().to_string();
    assert_eq!(mv_val["video"]["title"], "星际迷航：测试电影");
    assert_eq!(mv_val["video"]["online_tracking"]["cached"], false);
    assert!(mv_val["video"]["poster_key"].as_str().is_some());
    assert_eq!(mp4_hits.load(Ordering::SeqCst), 0);

    // 3. Before binding, member cannot see admin's private online movie in pool or play/cache it
    let (st_pool, pool_val) =
        send_json_with_method(&app, "GET", "/api/v1/video/pool", &member_tok, None).await;
    assert_eq!(st_pool, StatusCode::OK, "{pool_val}");
    assert!(pool_val["movies"].as_array().unwrap().is_empty());

    let (st_iso, _) = send_json_with_method(
        &app,
        "GET",
        &format!("/api/v1/video/videos/{movie_id}/online/play?token={member_tok}"),
        &member_tok,
        None,
    )
    .await;
    assert_eq!(st_iso, StatusCode::NOT_FOUND);

    // Non-admin member tracks the same online movie -> deduplicates onto same movie_id
    let (st_member_mv, member_mv_val) = send_json_with_method(
        &app,
        "POST",
        "/api/v1/video/online/track",
        &member_tok,
        Some(serde_json::json!({
            "source_id": source_id,
            "video_id": "101"
        })),
    )
    .await;
    assert_eq!(st_member_mv, StatusCode::OK, "{member_mv_val}");
    assert_eq!(member_mv_val["video"]["id"].as_str().unwrap(), movie_id);
    assert_eq!(member_mv_val["added_episodes"], 0);

    // 4. Resolve on-demand playback stream for uncached movie (now bound to member)
    let (st_play, play_val) = send_json_with_method(
        &app,
        "GET",
        &format!("/api/v1/video/videos/{movie_id}/online/play?token={member_tok}"),
        &member_tok,
        None,
    )
    .await;
    assert_eq!(st_play, StatusCode::OK, "{play_val}");
    assert_eq!(play_val["cached"], false);
    assert_eq!(play_val["stream_type"], "mp4");
    assert!(
        play_val["proxy_stream_url"]
            .as_str()
            .unwrap()
            .contains("/api/v1/video/online/stream-proxy")
    );

    // 5. Explicitly cache movie to local managed storage -> HTTP 206 Range stream succeeds
    let (st_cache, cached_mv) = send_json_with_method(
        &app,
        "POST",
        &format!("/api/v1/video/videos/{movie_id}/online/cache"),
        &admin_tok,
        None,
    )
    .await;
    assert_eq!(st_cache, StatusCode::OK, "{cached_mv}");
    assert_eq!(cached_mv["online_tracking"]["cached"], true);
    assert_eq!(cached_mv["file_size"], mp4_bytes.len() as u64);
    assert_eq!(mp4_hits.load(Ordering::SeqCst), 1);

    let range_resp = app
        .clone()
        .oneshot(
            Request::builder()
                .uri(format!(
                    "/api/v1/video/videos/{movie_id}/stream?quality=direct&token={member_tok}"
                ))
                .header(header::RANGE, "bytes=0-15")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(range_resp.status(), StatusCode::PARTIAL_CONTENT);
    let range_bytes = to_bytes(range_resp.into_body(), 1024 * 1024).await.unwrap();
    assert_eq!(&range_bytes[..16], &mp4_bytes[..16]);

    // 6. Track multi-episode TV show -> refresh picks up newly added Episode 3
    let (st_show, show_val) = send_json_with_method(
        &app,
        "POST",
        "/api/v1/video/online/track",
        &admin_tok,
        Some(serde_json::json!({
            "source_id": source_id,
            "video_id": "202"
        })),
    )
    .await;
    assert_eq!(st_show, StatusCode::OK, "{show_val}");
    assert_eq!(show_val["kind"], "show");
    assert_eq!(show_val["added_episodes"], 2);
    let show_id = show_val["show"]["show"]["id"].as_str().unwrap().to_string();
    assert_eq!(show_val["show"]["show"]["episode_count"], 2);
    assert_eq!(
        show_val["show"]["show"]["online_tracking"]["source_id"],
        source_id
    );

    include_ep3.store(true, Ordering::SeqCst);
    let (st_ref, ref_val) = send_json_with_method(
        &app,
        "POST",
        &format!("/api/v1/video/shows/{show_id}/online/refresh"),
        &admin_tok,
        None,
    )
    .await;
    assert_eq!(st_ref, StatusCode::OK, "{ref_val}");
    assert_eq!(ref_val["show"]["episode_count"], 3);
    assert_eq!(
        ref_val["seasons"][0]["episodes"].as_array().unwrap().len(),
        3
    );

    // 7. Cache entire show -> 3 cached, second pass skips all 3
    let (st_sc, sc_val) = send_json_with_method(
        &app,
        "POST",
        &format!("/api/v1/video/shows/{show_id}/online/cache"),
        &admin_tok,
        Some(serde_json::json!({})),
    )
    .await;
    assert_eq!(st_sc, StatusCode::OK, "{sc_val}");
    assert_eq!(sc_val["cached_count"], 3);
    assert_eq!(sc_val["failed_count"], 0);
    assert_eq!(mp4_hits.load(Ordering::SeqCst), 4);

    let (st_sc2, sc_val2) = send_json_with_method(
        &app,
        "POST",
        &format!("/api/v1/video/shows/{show_id}/online/cache"),
        &admin_tok,
        Some(serde_json::json!({})),
    )
    .await;
    assert_eq!(st_sc2, StatusCode::OK);
    assert_eq!(sc_val2["cached_count"], 0);
    assert_eq!(sc_val2["skipped_count"], 3);
    assert_eq!(mp4_hits.load(Ordering::SeqCst), 4);

    // 8. Unbind movie: admin unbinds first (1 remaining -> NOT deleted), member unbinds second (0 -> GC)
    let (st_ub1, ub1_val) = send_json_with_method(
        &app,
        "DELETE",
        &format!("/api/v1/video/bindings/{movie_id}"),
        &admin_tok,
        None,
    )
    .await;
    assert_eq!(st_ub1, StatusCode::OK, "{ub1_val}");
    assert_eq!(ub1_val["unbound"], true);
    assert_eq!(ub1_val["remaining_bindings"], 1);
    assert_eq!(ub1_val["deleted_from_pool"], false);

    let (st_ub2, ub2_val) = send_json_with_method(
        &app,
        "DELETE",
        &format!("/api/v1/video/bindings/{movie_id}"),
        &member_tok,
        None,
    )
    .await;
    assert_eq!(st_ub2, StatusCode::OK, "{ub2_val}");
    assert_eq!(ub2_val["unbound"], true);
    assert_eq!(ub2_val["remaining_bindings"], 0);
    assert_eq!(ub2_val["deleted_from_pool"], true);

    // Unbind show by sole owner -> auto-GC show & episodes
    let (st_ubs, ubs_val) = send_json_with_method(
        &app,
        "DELETE",
        &format!("/api/v1/video/bindings/{show_id}"),
        &admin_tok,
        None,
    )
    .await;
    assert_eq!(st_ubs, StatusCode::OK, "{ubs_val}");
    assert_eq!(ubs_val["unbound"], true);
    assert_eq!(ubs_val["remaining_bindings"], 0);
    assert_eq!(ubs_val["deleted_from_pool"], true);

    srv_handle.abort();
}

#[tokio::test]
async fn jellyfin_api_local_and_online_tracked_video_e2e() {
    use bytes::Bytes;

    let tmp = TempDir::new().unwrap();
    let db_path = tmp.path().join("xmedia_video_jellyfin.db");
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
    let security = Arc::new(SecurityManager::new("video-jf-secret"));
    let tokens = Arc::new(JwtTokenService::new("video-jf-secret", 3600));

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
        true,
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
    let admin = auth_svc
        .authenticate_jwt(&admin_session.token)
        .await
        .unwrap();

    // 1. Upload one local movie into managed library
    let local_mp4 = fake_mp4_payload();
    let uploaded = video_services
        .ingest
        .upload_files(
            &admin,
            vec![crate::application::dto::VideoUploadItem {
                filename: "Local Classic (2020).mp4".into(),
                bytes: Bytes::from(local_mp4.clone()),
            }],
        )
        .await
        .unwrap();
    let local_movie_id = uploaded.uploaded_videos[0].id;

    let state = VideoHttpState::new(auth_svc, library_svc, video_services);
    let app = Router::new()
        .nest("/api/v1/video", rest_router(state.clone()))
        .nest("/jellyfin", crate::http::jellyfin_router(state));

    // 2. Start mock upstream video server (1 MP4 movie + 1 HLS multi-episode show)
    let ts_seg = Arc::new(fake_ts_segment());
    let ts_for_srv = Arc::clone(&ts_seg);
    let online_mp4 = Arc::new(fake_mp4_payload());
    let online_mp4_srv = Arc::clone(&online_mp4);

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let base_url = format!("http://{addr}");
    let base_srv = base_url.clone();

    let mock_router = Router::new()
        .route(
            "/api.php/provide/vod/",
            get(move |Query(q): Query<AppleCmsQuery>| {
                let b = base_srv.clone();
                async move {
                    let ids = q.ids.unwrap_or_default();
                    let list = if ids == "m1" {
                        serde_json::json!([{
                            "vod_id": "m1",
                            "vod_name": "在线电影MP4",
                            "vod_pic": format!("{b}/poster.png"),
                            "vod_year": "2025",
                            "vod_area": "动作",
                            "vod_play_from": "mp4",
                            "vod_play_url": format!("正片${b}/direct.mp4")
                        }])
                    } else if ids == "s1" {
                        serde_json::json!([{
                            "vod_id": "s1",
                            "vod_name": "在线追更番剧",
                            "vod_pic": format!("{b}/poster.png"),
                            "vod_year": "2026",
                            "vod_area": "动画",
                            "vod_play_from": "hls",
                            "vod_play_url": format!("第01集${b}/hls/ep1.m3u8#第02集${b}/hls/ep2.m3u8")
                        }])
                    } else {
                        serde_json::json!([])
                    };
                    (
                        StatusCode::OK,
                        [(header::CONTENT_TYPE, "application/json")],
                        serde_json::json!({ "code": 1, "list": list }).to_string(),
                    )
                }
            }),
        )
        .route(
            "/direct.mp4",
            get(move || {
                let d = Arc::clone(&online_mp4_srv);
                async move {
                    (
                        StatusCode::OK,
                        [(header::CONTENT_TYPE, "video/mp4")],
                        (*d).clone(),
                    )
                }
            }),
        )
        .route(
            "/hls/ep1.m3u8",
            get(|| async {
                let m3u8 = "#EXTM3U\n#EXT-X-VERSION:3\n#EXT-X-TARGETDURATION:6\n#EXTINF:6.0,\nseg0.ts\n#EXT-X-ENDLIST\n";
                (
                    StatusCode::OK,
                    [(header::CONTENT_TYPE, "application/vnd.apple.mpegurl")],
                    m3u8,
                )
            }),
        )
        .route(
            "/hls/seg0.ts",
            get(move || {
                let s = Arc::clone(&ts_for_srv);
                async move {
                    (
                        StatusCode::OK,
                        [(header::CONTENT_TYPE, "video/mp2t")],
                        (*s).clone(),
                    )
                }
            }),
        )
        .route(
            "/poster.png",
            get(|| async {
                (
                    StatusCode::OK,
                    [(header::CONTENT_TYPE, "image/png")],
                    PNG_1X1.to_vec(),
                )
            }),
        );
    let srv_handle = tokio::spawn(async move {
        axum::serve(listener, mock_router).await.unwrap();
    });

    // Import source & track both online movie (MP4) and online show (HLS) WITHOUT caching locally
    let src_spec = serde_json::json!({
        "sourceName": "JF Mock Source",
        "sourceUrl": base_url,
        "apiUrl": format!("{base_url}/api.php/provide/vod/"),
        "sourceFormat": "applecms_v10",
        "enabled": true
    });
    let (st_imp, imp_val) = send_json_with_method(
        &app,
        "POST",
        "/api/v1/video/sources/import",
        &admin_session.token,
        Some(serde_json::json!({ "raw_json": src_spec.to_string() })),
    )
    .await;
    assert_eq!(st_imp, StatusCode::OK);
    let source_id = imp_val["sources"][0]["id"].as_str().unwrap().to_string();

    let (st_m1, m1_val) = send_json_with_method(
        &app,
        "POST",
        "/api/v1/video/online/track",
        &admin_session.token,
        Some(serde_json::json!({ "source_id": source_id, "video_id": "m1" })),
    )
    .await;
    assert_eq!(st_m1, StatusCode::OK);
    let tracked_movie_id = m1_val["video"]["id"].as_str().unwrap().to_string();
    let tracked_movie_poster = m1_val["video"]["poster_key"].as_str().unwrap().to_string();

    let (st_s1, s1_val) = send_json_with_method(
        &app,
        "POST",
        "/api/v1/video/online/track",
        &admin_session.token,
        Some(serde_json::json!({ "source_id": source_id, "video_id": "s1" })),
    )
    .await;
    assert_eq!(st_s1, StatusCode::OK);
    let tracked_show_id = s1_val["show"]["show"]["id"].as_str().unwrap().to_string();
    let tracked_ep1_id = s1_val["show"]["seasons"][0]["episodes"][0]["id"]
        .as_str()
        .unwrap()
        .to_string();

    // 3. Jellyfin public discovery & AuthenticateByName
    let pub_resp = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/jellyfin/System/Info/Public")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(pub_resp.status(), StatusCode::OK);
    let pub_json: serde_json::Value =
        serde_json::from_slice(&to_bytes(pub_resp.into_body(), 1024 * 1024).await.unwrap())
            .unwrap();
    assert_eq!(pub_json["ServerName"], "XMedia");
    assert_eq!(pub_json["StartupWizardCompleted"], true);

    let auth_resp = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/jellyfin/Users/AuthenticateByName")
                .header(header::CONTENT_TYPE, "application/json")
                .header(
                    "X-Emby-Authorization",
                    r#"MediaBrowser Client="Infuse", Device="AppleTV", DeviceId="tv-1", Version="8.0""#,
                )
                .body(Body::from(
                    serde_json::json!({
                        "Username": "admin",
                        "Pw": "adminpassword123"
                    })
                    .to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(auth_resp.status(), StatusCode::OK);
    let auth_json: serde_json::Value =
        serde_json::from_slice(&to_bytes(auth_resp.into_body(), 1024 * 1024).await.unwrap())
            .unwrap();
    let jf_token = auth_json["AccessToken"].as_str().unwrap().to_string();
    let jf_user_id = auth_json["User"]["Id"].as_str().unwrap().to_string();
    assert!(!jf_token.is_empty());

    // 4. Views -> Items exposes BOTH local movie AND uncached online-tracked movie & show
    let views_resp = app
        .clone()
        .oneshot(
            Request::builder()
                .uri(format!("/jellyfin/Users/{jf_user_id}/Views"))
                .header("X-Emby-Token", &jf_token)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(views_resp.status(), StatusCode::OK);
    let views_json: serde_json::Value =
        serde_json::from_slice(&to_bytes(views_resp.into_body(), 1024 * 1024).await.unwrap())
            .unwrap();
    let lib_id = views_json["Items"][0]["Id"].as_str().unwrap().to_string();

    let items_resp = app
        .clone()
        .oneshot(
            Request::builder()
                .uri(format!(
                    "/jellyfin/Users/{jf_user_id}/Items?ParentId={lib_id}"
                ))
                .header(
                    header::AUTHORIZATION,
                    format!(r#"MediaBrowser Client="Swiftfin", Token="{jf_token}""#),
                )
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(items_resp.status(), StatusCode::OK);
    let items_json: serde_json::Value =
        serde_json::from_slice(&to_bytes(items_resp.into_body(), 1024 * 1024).await.unwrap())
            .unwrap();
    assert_eq!(items_json["TotalRecordCount"], 3);

    // 5. Series -> Seasons -> Episodes navigation works for online-tracked show
    let seasons_resp = app
        .clone()
        .oneshot(
            Request::builder()
                .uri(format!("/jellyfin/Shows/{tracked_show_id}/Seasons"))
                .header("X-Emby-Token", &jf_token)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(seasons_resp.status(), StatusCode::OK);
    let seasons_json: serde_json::Value = serde_json::from_slice(
        &to_bytes(seasons_resp.into_body(), 1024 * 1024)
            .await
            .unwrap(),
    )
    .unwrap();
    assert_eq!(seasons_json["TotalRecordCount"], 1);
    let season1_id = seasons_json["Items"][0]["Id"].as_str().unwrap().to_string();

    let eps_resp = app
        .clone()
        .oneshot(
            Request::builder()
                .uri(format!(
                    "/jellyfin/Shows/{tracked_show_id}/Episodes?SeasonId={season1_id}"
                ))
                .header("X-Emby-Token", &jf_token)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(eps_resp.status(), StatusCode::OK);
    let eps_json: serde_json::Value =
        serde_json::from_slice(&to_bytes(eps_resp.into_body(), 1024 * 1024).await.unwrap())
            .unwrap();
    assert_eq!(eps_json["TotalRecordCount"], 2);

    // 6. Local movie PlaybackInfo + Range stream
    let local_pb_resp = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/jellyfin/Items/{local_movie_id}/PlaybackInfo"))
                .header("X-Emby-Token", &jf_token)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(local_pb_resp.status(), StatusCode::OK);
    let local_pb: serde_json::Value = serde_json::from_slice(
        &to_bytes(local_pb_resp.into_body(), 1024 * 1024)
            .await
            .unwrap(),
    )
    .unwrap();
    let local_stream_url = local_pb["MediaSources"][0]["DirectStreamUrl"]
        .as_str()
        .unwrap();
    let local_stream_resp = app
        .clone()
        .oneshot(
            Request::builder()
                .uri(format!("/jellyfin{local_stream_url}"))
                .header(header::RANGE, "bytes=0-15")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(local_stream_resp.status(), StatusCode::PARTIAL_CONTENT);
    let local_chunk = to_bytes(local_stream_resp.into_body(), 1024 * 1024)
        .await
        .unwrap();
    assert_eq!(&local_chunk[..16], &local_mp4[..16]);

    // 7. Uncached tracked MP4 movie PlaybackInfo + transparent proxy stream
    let mp4_pb_resp = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/jellyfin/Items/{tracked_movie_id}/PlaybackInfo"))
                .header("X-Emby-Token", &jf_token)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(mp4_pb_resp.status(), StatusCode::OK);
    let mp4_pb: serde_json::Value = serde_json::from_slice(
        &to_bytes(mp4_pb_resp.into_body(), 1024 * 1024)
            .await
            .unwrap(),
    )
    .unwrap();
    let mp4_stream_url = mp4_pb["MediaSources"][0]["DirectStreamUrl"]
        .as_str()
        .unwrap();
    let mp4_stream_resp = app
        .clone()
        .oneshot(
            Request::builder()
                .uri(format!("/jellyfin{mp4_stream_url}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(mp4_stream_resp.status(), StatusCode::OK);
    let mp4_chunk = to_bytes(mp4_stream_resp.into_body(), 1024 * 1024)
        .await
        .unwrap();
    assert_eq!(mp4_chunk.as_ref(), online_mp4.as_slice());

    // 8. Uncached tracked HLS episode PlaybackInfo -> rewritten master.m3u8 -> proxied TS segment
    let hls_pb_resp = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/jellyfin/Items/{tracked_ep1_id}/PlaybackInfo"))
                .header("X-Emby-Token", &jf_token)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(hls_pb_resp.status(), StatusCode::OK);
    let hls_pb: serde_json::Value = serde_json::from_slice(
        &to_bytes(hls_pb_resp.into_body(), 1024 * 1024)
            .await
            .unwrap(),
    )
    .unwrap();
    let hls_url = hls_pb["MediaSources"][0]["TranscodingUrl"]
        .as_str()
        .unwrap();
    assert!(hls_url.contains("/hls/master.m3u8"));

    let m3u8_resp = app
        .clone()
        .oneshot(
            Request::builder()
                .uri(format!("/jellyfin{hls_url}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(m3u8_resp.status(), StatusCode::OK);
    let m3u8_text = String::from_utf8(
        to_bytes(m3u8_resp.into_body(), 1024 * 1024)
            .await
            .unwrap()
            .to_vec(),
    )
    .unwrap();
    let seg_rel_line = m3u8_text
        .lines()
        .find(|l| l.starts_with("proxy?url="))
        .expect("rewritten relative proxy segment URL");

    let seg_resp = app
        .clone()
        .oneshot(
            Request::builder()
                .uri(format!(
                    "/jellyfin/Videos/{}/hls/{seg_rel_line}",
                    tracked_ep1_id.replace('-', "")
                ))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(seg_resp.status(), StatusCode::OK);
    let seg_bytes = to_bytes(seg_resp.into_body(), 1024 * 1024).await.unwrap();
    assert_eq!(seg_bytes.as_ref(), ts_seg.as_slice());

    // 9. Poster image allows matching ?tag= without auth, rejects anonymous without tag
    let unauth_img = app
        .clone()
        .oneshot(
            Request::builder()
                .uri(format!("/jellyfin/Items/{tracked_movie_id}/Images/Primary"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(unauth_img.status(), StatusCode::UNAUTHORIZED);

    let tagged_img = app
        .clone()
        .oneshot(
            Request::builder()
                .uri(format!(
                    "/jellyfin/Items/{tracked_movie_id}/Images/Primary?tag={tracked_movie_poster}"
                ))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(tagged_img.status(), StatusCode::OK);

    // 10. Two-way progress & favorite sync between Jellyfin and XMedia REST
    let prog_resp = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/jellyfin/Sessions/Playing/Progress")
                .header("X-Emby-Token", &jf_token)
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(
                    serde_json::json!({
                        "ItemId": tracked_ep1_id,
                        "PositionTicks": 1_250_000_000i64
                    })
                    .to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(prog_resp.status(), StatusCode::NO_CONTENT);

    let fav_resp = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!(
                    "/jellyfin/Users/{jf_user_id}/FavoriteItems/{tracked_show_id}"
                ))
                .header("X-Emby-Token", &jf_token)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(fav_resp.status(), StatusCode::OK);

    // Verify synced state in Jellyfin /Items/Resume and XMedia REST API
    let resume_resp = app
        .clone()
        .oneshot(
            Request::builder()
                .uri(format!("/jellyfin/Users/{jf_user_id}/Items/Resume"))
                .header("X-Emby-Token", &jf_token)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resume_resp.status(), StatusCode::OK);
    let resume_json: serde_json::Value = serde_json::from_slice(
        &to_bytes(resume_resp.into_body(), 1024 * 1024)
            .await
            .unwrap(),
    )
    .unwrap();
    assert_eq!(resume_json["TotalRecordCount"], 1);
    assert_eq!(
        resume_json["Items"][0]["UserData"]["PlaybackPositionTicks"],
        1_250_000_000i64
    );

    let (st_native, native_detail) = send_json_with_method(
        &app,
        "GET",
        &format!("/api/v1/video/videos/{tracked_ep1_id}"),
        &admin_session.token,
        None,
    )
    .await;
    assert_eq!(st_native, StatusCode::OK);
    assert_eq!(native_detail["video"]["progress"]["position_secs"], 125.0);

    let (st_native_show, native_show) = send_json_with_method(
        &app,
        "GET",
        &format!("/api/v1/video/shows/{tracked_show_id}"),
        &admin_session.token,
        None,
    )
    .await;
    assert_eq!(st_native_show, StatusCode::OK);
    assert_eq!(native_show["show"]["is_favorite"], true);

    srv_handle.abort();
}

use std::io::{Cursor, Write};
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use axum::Router;
use axum::body::Body;
use axum::http::StatusCode;
use axum::http::header::{
    ACCEPT_RANGES, ACCESS_CONTROL_ALLOW_ORIGIN, CONTENT_LENGTH, CONTENT_RANGE,
    CONTENT_SECURITY_POLICY, CONTENT_TYPE, HOST, LOCATION, ORIGIN, RANGE, X_CONTENT_TYPE_OPTIONS,
};
use axum::response::IntoResponse;
use axum::routing::get;
use serde_json::{Value, json};
use tempfile::TempDir;
use tokio::net::TcpListener;
use tokio::sync::oneshot;
use xmedia::{XMediaApp, XMediaConfig};
use xmedia_core::application::dto::SetupAdminInput;
use zip::ZipWriter;
use zip::write::SimpleFileOptions;

use xmedia_app_lib::gateway::{
    APP_HEADER_NAME, APP_HEADER_VALUE, AppHealthResponse, MutatedRemoteSourceResponse,
    RemoteSourcesListResponse, SubsonicAlbumsResponse, TestRemoteSourceResponse, start_gateway,
};
use xmedia_app_lib::opds_client::{OpdsFeedDto, RemoteImportResultDto};
use xmedia_app_lib::remote_sources::RemoteProtocol;
use xmedia_app_lib::subsonic_client::{SubsonicAlbumDetailDto, SubsonicSearchResultDto};

struct RemoteXMediaServer {
    url: String,
    _tmp: TempDir,
    shutdown_tx: Option<oneshot::Sender<()>>,
}

impl Drop for RemoteXMediaServer {
    fn drop(&mut self) {
        if let Some(tx) = self.shutdown_tx.take() {
            let _ = tx.send(());
        }
    }
}

fn sample_epub_bytes() -> Vec<u8> {
    let mut cursor = Cursor::new(Vec::new());
    {
        let mut zw = ZipWriter::new(&mut cursor);
        let opts = SimpleFileOptions::default();

        zw.start_file("META-INF/container.xml", opts).unwrap();
        zw.write_all(
            br#"<?xml version="1.0" encoding="UTF-8"?>
<container version="1.0" xmlns="urn:oasis:names:tc:opendocument:xmlns:container">
  <rootfiles>
    <rootfile full-path="OEBPS/content.opf" media-type="application/oebps-package+xml"/>
  </rootfiles>
</container>"#,
        )
        .unwrap();

        zw.start_file("OEBPS/content.opf", opts).unwrap();
        zw.write_all(
            br#"<?xml version="1.0" encoding="UTF-8"?>
<package version="3.0" xmlns="http://www.idpf.org/2007/opf" unique-identifier="bookid">
  <metadata xmlns:dc="http://purl.org/dc/elements/1.1/">
    <dc:title>Foundation</dc:title>
    <dc:creator>Isaac Asimov</dc:creator>
    <dc:language>en</dc:language>
    <dc:subject>Sci-Fi</dc:subject>
  </metadata>
  <manifest>
    <item id="nav" href="nav.xhtml" media-type="application/xhtml+xml" properties="nav"/>
    <item id="ch1" href="ch1.xhtml" media-type="application/xhtml+xml"/>
  </manifest>
  <spine>
    <itemref idref="ch1"/>
  </spine>
</package>"#,
        )
        .unwrap();

        zw.start_file("OEBPS/nav.xhtml", opts).unwrap();
        zw.write_all(
            br#"<?xml version="1.0" encoding="UTF-8"?>
<html xmlns="http://www.w3.org/1999/xhtml" xmlns:epub="http://www.idpf.org/2007/ops">
<body>
  <nav epub:type="toc">
    <ol>
      <li><a href="ch1.xhtml">The Psychohistorians</a></li>
    </ol>
  </nav>
</body>
</html>"#,
        )
        .unwrap();

        zw.start_file("OEBPS/ch1.xhtml", opts).unwrap();
        zw.write_all(
            br#"<html xmlns="http://www.w3.org/1999/xhtml"><body><p>Hari Seldon</p></body></html>"#,
        )
        .unwrap();

        zw.finish().unwrap();
    }
    cursor.into_inner()
}

fn sample_wav_bytes(samples: u32) -> Vec<u8> {
    let sample_rate: u32 = 8000;
    let channels: u16 = 1;
    let bits: u16 = 16;
    let byte_rate = sample_rate * 2;
    let block_align: u16 = 2;
    let data_size = samples * 2;
    let riff_size = 36 + data_size;

    let mut buf = Vec::with_capacity((riff_size + 8) as usize);
    buf.extend_from_slice(b"RIFF");
    buf.extend_from_slice(&riff_size.to_le_bytes());
    buf.extend_from_slice(b"WAVEfmt ");
    buf.extend_from_slice(&16u32.to_le_bytes());
    buf.extend_from_slice(&1u16.to_le_bytes());
    buf.extend_from_slice(&channels.to_le_bytes());
    buf.extend_from_slice(&sample_rate.to_le_bytes());
    buf.extend_from_slice(&byte_rate.to_le_bytes());
    buf.extend_from_slice(&block_align.to_le_bytes());
    buf.extend_from_slice(&bits.to_le_bytes());
    buf.extend_from_slice(b"data");
    buf.extend_from_slice(&data_size.to_le_bytes());
    buf.resize((riff_size + 8) as usize, 0);
    buf
}

async fn spawn_remote_xmedia_server(username: &str, password: &str) -> RemoteXMediaServer {
    let tmp = TempDir::new().unwrap();
    let db_path = tmp.path().join("remote_xmedia.db");
    let app = XMediaApp::init(XMediaConfig {
        data_dir: tmp.path().to_path_buf(),
        database_url: format!("sqlite:{}", db_path.display()),
        secret: "remote-server-secret-key-for-e2e".to_string(),
        jwt_ttl_secs: 3600,
        ffmpeg_bin: "__missing_ffmpeg__".to_string(),
        external_web_dir: None,
    })
    .await
    .expect("remote XMediaApp should initialize");

    let session = app
        .auth
        .setup_initial_admin(SetupAdminInput {
            username: username.to_string(),
            display_name: Some("Remote Admin".into()),
            password: password.to_string(),
        })
        .await
        .expect("remote admin setup should succeed");

    let listener = TcpListener::bind(SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 0))
        .await
        .unwrap();
    let addr = listener.local_addr().unwrap();
    let url = format!("http://127.0.0.1:{}", addr.port());

    let (shutdown_tx, shutdown_rx) = oneshot::channel::<()>();
    let (_, router, _) = app.into_embedded_host();
    tokio::spawn(async move {
        let _ = axum::serve(listener, router)
            .with_graceful_shutdown(async move {
                let _ = shutdown_rx.await;
            })
            .await;
    });

    // Seed remote server with a valid EPUB book and a valid WAV audio track.
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(15))
        .build()
        .unwrap();

    let epub_form = reqwest::multipart::Form::new().part(
        "file",
        reqwest::multipart::Part::bytes(sample_epub_bytes())
            .file_name("foundation.epub")
            .mime_str("application/epub+zip")
            .unwrap(),
    );
    let epub_res = client
        .post(format!("{url}/api/v1/book/books/upload"))
        .bearer_auth(&session.token)
        .multipart(epub_form)
        .send()
        .await
        .unwrap();
    assert_eq!(epub_res.status(), StatusCode::CREATED);

    let wav_form = reqwest::multipart::Form::new().part(
        "file",
        reqwest::multipart::Part::bytes(sample_wav_bytes(1600))
            .file_name("galactic_theme.wav")
            .mime_str("audio/wav")
            .unwrap(),
    );
    let wav_res = client
        .post(format!("{url}/api/v1/music/tracks/upload"))
        .bearer_auth(&session.token)
        .multipart(wav_form)
        .send()
        .await
        .unwrap();
    assert_eq!(wav_res.status(), StatusCode::CREATED);

    RemoteXMediaServer {
        url,
        _tmp: tmp,
        shutdown_tx: Some(shutdown_tx),
    }
}

#[tokio::test]
async fn e2e_standalone_local_engine_opds_and_subsonic_roundtrip() {
    let remote_user = "remoteadmin";
    let remote_pass = "RemotePass123!";
    let remote = spawn_remote_xmedia_server(remote_user, remote_pass).await;

    let local_tmp = TempDir::new().unwrap();
    let gateway = start_gateway(
        SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 0),
        local_tmp.path().to_path_buf(),
        None,
    )
    .await
    .expect("standalone loopback gateway should start");

    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(15))
        .build()
        .unwrap();

    // 1. /_app/health reports standalone mode + 4 modules + X-XMedia-App: standalone
    let health_resp = client
        .get(format!("{}/_app/health", gateway.url))
        .send()
        .await
        .unwrap();
    assert_eq!(health_resp.status(), StatusCode::OK);
    assert_eq!(
        health_resp
            .headers()
            .get(APP_HEADER_NAME)
            .and_then(|v| v.to_str().ok()),
        Some(APP_HEADER_VALUE)
    );
    let health: AppHealthResponse = health_resp.json().await.unwrap();
    assert_eq!(health.mode, "standalone");
    assert_eq!(health.remote_source_count, 0);
    assert_eq!(health.enabled_modules.len(), 4);

    // Appearance endpoint requires X-XMedia-App header and returns synced dark state
    let appearance_missing_hdr = client
        .put(format!("{}/_app/appearance", gateway.url))
        .json(&json!({ "dark": true }))
        .send()
        .await
        .unwrap();
    assert_eq!(appearance_missing_hdr.status(), StatusCode::FORBIDDEN);

    let appearance_ok = client
        .put(format!("{}/_app/appearance", gateway.url))
        .header(APP_HEADER_NAME, APP_HEADER_VALUE)
        .json(&json!({ "dark": true }))
        .send()
        .await
        .unwrap();
    assert_eq!(appearance_ok.status(), StatusCode::OK);
    let appearance_json: Value = appearance_ok.json().await.unwrap();
    assert_eq!(appearance_json["dark"], json!(true));

    let missing_sys_font = client
        .get(format!("{}/_app/system-font/0", gateway.url))
        .send()
        .await
        .unwrap();
    assert_eq!(missing_sys_font.status(), StatusCode::NOT_FOUND);

    // 2. Zero-login auto-session works via POST (and rejects GET)
    let get_rejected = client
        .get(format!("{}/_app/auto-session", gateway.url))
        .send()
        .await
        .unwrap();
    assert_eq!(get_rejected.status(), StatusCode::METHOD_NOT_ALLOWED);

    // Unauthenticated /_app/remote-sources* must be rejected with 401 before session cookie is issued
    let unauthed_sources = client
        .get(format!("{}/_app/remote-sources", gateway.url))
        .send()
        .await
        .unwrap();
    assert_eq!(unauthed_sources.status(), StatusCode::UNAUTHORIZED);

    let session_resp = client
        .post(format!("{}/_app/auto-session", gateway.url))
        .header(APP_HEADER_NAME, APP_HEADER_VALUE)
        .send()
        .await
        .unwrap();
    assert_eq!(session_resp.status(), StatusCode::OK);
    let set_cookie = session_resp
        .headers()
        .get(reqwest::header::SET_COOKIE)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .to_string();
    assert!(
        set_cookie.contains("xmedia_token=") && set_cookie.contains("HttpOnly"),
        "expected HttpOnly session cookie, got: {set_cookie}"
    );
    let cookie_pair = set_cookie
        .split(';')
        .next()
        .expect("cookie pair")
        .trim()
        .to_string();
    let mut cookie_headers = reqwest::header::HeaderMap::new();
    cookie_headers.insert(reqwest::header::COOKIE, cookie_pair.parse().unwrap());
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(15))
        .default_headers(cookie_headers)
        .build()
        .unwrap();

    let session_json: Value = session_resp.json().await.unwrap();
    let local_token = session_json["token"].as_str().unwrap().to_string();
    assert_eq!(session_json["user"]["username"], json!("local"));

    // Authenticate against embedded /api/v1/auth/me (both Bearer and Cookie) & verify empty local book library
    let me_resp: Value = client
        .get(format!("{}/api/v1/auth/me", gateway.url))
        .bearer_auth(&local_token)
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(me_resp["username"], json!("local"));

    let me_cookie: Value = client
        .get(format!("{}/api/v1/auth/me", gateway.url))
        .header(
            reqwest::header::COOKIE,
            format!("xmedia_token={local_token}"),
        )
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(me_cookie["username"], json!("local"));

    let initial_books: Value = client
        .get(format!("{}/api/v1/book/books", gateway.url))
        .bearer_auth(&local_token)
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(initial_books["total"], json!(0));

    // 3. Test & add remote OPDS 1.2 source, browse catalog, search via OpenSearch, and import EPUB to local library
    let opds12_url = format!("{}/opds/book/v1.2", remote.url);
    let opds12_probe: TestRemoteSourceResponse = client
        .post(format!("{}/_app/remote-sources/test", gateway.url))
        .json(&json!({
            "protocol": "opds",
            "url": opds12_url,
            "username": remote_user,
            "password": remote_pass,
            "allow_insecure_tls": false
        }))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert!(opds12_probe.ok, "OPDS 1.2 probe failed: {:?}", opds12_probe);
    assert_eq!(opds12_probe.protocol, RemoteProtocol::Opds);
    assert!(
        opds12_probe
            .detail
            .as_deref()
            .is_some_and(|d| d.contains("1.2")),
        "expected OPDS 1.2 detail, got: {:?}",
        opds12_probe.detail
    );

    let created_opds12_resp = client
        .post(format!("{}/_app/remote-sources", gateway.url))
        .json(&json!({
            "name": "Remote OPDS 1.2",
            "protocol": "opds",
            "url": opds12_url,
            "username": remote_user,
            "password": remote_pass,
            "allow_insecure_tls": false
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(created_opds12_resp.status(), StatusCode::CREATED);
    let created_opds12_raw = created_opds12_resp.text().await.unwrap();
    assert!(
        !created_opds12_raw.contains(remote_pass),
        "remote source API must never leak plaintext password"
    );
    let created_opds12: MutatedRemoteSourceResponse =
        serde_json::from_str(&created_opds12_raw).unwrap();
    assert!(created_opds12.source.has_password);
    let opds12_id = created_opds12.source.id.clone();

    let persisted_sources_disk =
        std::fs::read_to_string(local_tmp.path().join("remote_sources.json")).unwrap();
    assert!(
        !persisted_sources_disk.contains(remote_pass),
        "remote_sources.json on disk must never contain plaintext passwords"
    );
    assert!(
        persisted_sources_disk.contains("enc:v1:"),
        "remote_sources.json on disk must store AES-GCM encrypted password"
    );

    // Update name with empty password -> existing password must be retained
    let updated_opds12: MutatedRemoteSourceResponse = client
        .put(format!("{}/_app/remote-sources/{opds12_id}", gateway.url))
        .json(&json!({
            "name": "Home OPDS 1.2",
            "password": ""
        }))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(updated_opds12.source.name, "Home OPDS 1.2");
    assert!(updated_opds12.source.has_password);

    // Browse OPDS 1.2 root navigation feed
    let root_feed: OpdsFeedDto = client
        .get(format!("{}/_app/opds/{opds12_id}/browse", gateway.url))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(root_feed.opds_version, "1.2");
    assert!(!root_feed.nav_links.is_empty());

    let all_books_nav = root_feed
        .nav_links
        .iter()
        .find(|n| n.href.ends_with("/all"))
        .expect("root OPDS 1.2 feed should link to /all");

    // Browse acquisition feed
    let acq_feed: OpdsFeedDto = client
        .get(format!("{}/_app/opds/{opds12_id}/browse", gateway.url))
        .query(&[("url", all_books_nav.href.as_str())])
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(acq_feed.entries.len(), 1);
    assert_eq!(acq_feed.entries[0].title, "Foundation");
    assert_eq!(acq_feed.entries[0].authors, vec!["Isaac Asimov"]);
    assert_eq!(acq_feed.entries[0].downloads[0].format, "epub");

    // Search OPDS 1.2 via OpenSearch XML template
    let search_feed: OpdsFeedDto = client
        .get(format!("{}/_app/opds/{opds12_id}/browse", gateway.url))
        .query(&[("q", "Foundation")])
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(search_feed.entries.len(), 1);
    assert_eq!(search_feed.entries[0].title, "Foundation");

    // Also verify OPDS 2.0 probe + browse + search
    let opds2_url = format!("{}/opds/book/v2", remote.url);
    let opds2_probe: TestRemoteSourceResponse = client
        .post(format!("{}/_app/remote-sources/test", gateway.url))
        .json(&json!({
            "protocol": "opds",
            "url": opds2_url,
            "username": remote_user,
            "password": remote_pass,
            "allow_insecure_tls": false
        }))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert!(opds2_probe.ok, "OPDS 2.0 probe failed: {:?}", opds2_probe);
    assert!(
        opds2_probe
            .detail
            .as_deref()
            .is_some_and(|d| d.contains("2.0")),
        "expected OPDS 2.0 detail, got: {:?}",
        opds2_probe.detail
    );

    let created_opds2: MutatedRemoteSourceResponse = client
        .post(format!("{}/_app/remote-sources", gateway.url))
        .json(&json!({
            "name": "Remote OPDS 2.0",
            "protocol": "opds",
            "url": opds2_url,
            "username": remote_user,
            "password": remote_pass,
            "allow_insecure_tls": false
        }))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let opds2_id = created_opds2.source.id.clone();

    let opds2_search: OpdsFeedDto = client
        .get(format!("{}/_app/opds/{opds2_id}/browse", gateway.url))
        .query(&[("q", "Foundation")])
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(opds2_search.opds_version, "2.0");
    assert_eq!(opds2_search.entries.len(), 1);
    assert_eq!(opds2_search.entries[0].title, "Foundation");

    // One-click import remote EPUB into standalone client's local book library!
    let dl_href = acq_feed.entries[0].downloads[0].href.clone();
    let import_resp = client
        .post(format!("{}/_app/opds/{opds12_id}/import", gateway.url))
        .json(&json!({
            "url": dl_href,
            "filename": "Foundation.epub"
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(import_resp.status(), StatusCode::CREATED);
    let imported_book: RemoteImportResultDto = import_resp.json().await.unwrap();
    assert_eq!(imported_book.media_kind, "book");
    assert_eq!(imported_book.title, "Foundation");
    assert!(!imported_book.imported_id.is_empty());

    // Confirm imported book exists in standalone local /api/v1/book/books!
    let local_books: Value = client
        .get(format!("{}/api/v1/book/books", gateway.url))
        .bearer_auth(&local_token)
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(local_books["total"], json!(1));
    assert_eq!(
        local_books["items"][0]["metadata"]["title"],
        json!("Foundation")
    );

    // 4. Remote OpenSubsonic source: ping test, list albums, album detail, search, Range stream, and import track
    let sub_probe: TestRemoteSourceResponse = client
        .post(format!("{}/_app/remote-sources/test", gateway.url))
        .json(&json!({
            "protocol": "subsonic",
            "url": remote.url,
            "username": remote_user,
            "password": remote_pass,
            "allow_insecure_tls": false
        }))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert!(sub_probe.ok, "Subsonic probe failed: {:?}", sub_probe);
    assert_eq!(sub_probe.protocol, RemoteProtocol::Subsonic);
    assert!(
        sub_probe
            .detail
            .as_deref()
            .is_some_and(|d| d.contains("OpenSubsonic")),
        "expected OpenSubsonic detail, got: {:?}",
        sub_probe.detail
    );

    let created_sub: MutatedRemoteSourceResponse = client
        .post(format!("{}/_app/remote-sources", gateway.url))
        .json(&json!({
            "name": "Remote Music",
            "protocol": "subsonic",
            "url": remote.url,
            "username": remote_user,
            "password": remote_pass,
            "allow_insecure_tls": false
        }))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let sub_id = created_sub.source.id.clone();

    let albums_resp: SubsonicAlbumsResponse = client
        .get(format!(
            "{}/_app/subsonic/{sub_id}/albums?type=newest&size=30&offset=0",
            gateway.url
        ))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(albums_resp.albums.len(), 1);
    let album_id = albums_resp.albums[0].id.clone();

    let album_detail: SubsonicAlbumDetailDto = client
        .get(format!(
            "{}/_app/subsonic/{sub_id}/albums/{album_id}",
            gateway.url
        ))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(album_detail.songs.len(), 1);
    let song = &album_detail.songs[0];
    assert!(
        song.stream_url
            .starts_with(&format!("/_app/subsonic/{sub_id}/stream/"))
    );

    let sub_search: SubsonicSearchResultDto = client
        .get(format!("{}/_app/subsonic/{sub_id}/search", gateway.url))
        .query(&[("q", "galactic")])
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(sub_search.songs.len(), 1);

    // Verify Range 206 Partial Content audio stream proxy
    let stream_resp = client
        .get(format!("{}{}", gateway.url, song.stream_url))
        .header(RANGE, "bytes=0-43")
        .send()
        .await
        .unwrap();
    assert_eq!(stream_resp.status(), StatusCode::PARTIAL_CONTENT);
    assert_eq!(
        stream_resp
            .headers()
            .get(ACCEPT_RANGES)
            .and_then(|v| v.to_str().ok()),
        Some("bytes")
    );
    assert!(
        stream_resp
            .headers()
            .get(CONTENT_RANGE)
            .and_then(|v| v.to_str().ok())
            .is_some_and(|cr| cr.starts_with("bytes 0-43/"))
    );
    let header_bytes = stream_resp.bytes().await.unwrap();
    assert_eq!(header_bytes.len(), 44);
    assert_eq!(&header_bytes[0..4], b"RIFF");

    // One-click import remote Subsonic song into local music library
    let import_song_resp = client
        .post(format!(
            "{}/_app/subsonic/{sub_id}/import/{}",
            gateway.url, song.id
        ))
        .json(&json!({}))
        .send()
        .await
        .unwrap();
    assert_eq!(import_song_resp.status(), StatusCode::CREATED);
    let imported_song: RemoteImportResultDto = import_song_resp.json().await.unwrap();
    assert_eq!(imported_song.media_kind, "music");
    assert!(!imported_song.imported_id.is_empty());

    let local_tracks: Value = client
        .get(format!("{}/api/v1/music/tracks", gateway.url))
        .bearer_auth(&local_token)
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(local_tracks["total"], json!(1));

    // 5. List, delete remote sources, and verify unknown /_app route returns 404
    let listed: RemoteSourcesListResponse = client
        .get(format!("{}/_app/remote-sources", gateway.url))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(listed.sources.len(), 3);

    let after_delete: RemoteSourcesListResponse = client
        .delete(format!("{}/_app/remote-sources/{opds2_id}", gateway.url))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(after_delete.sources.len(), 2);

    let not_found = client
        .get(format!("{}/_app/does-not-exist", gateway.url))
        .send()
        .await
        .unwrap();
    assert_eq!(not_found.status(), StatusCode::NOT_FOUND);

    for bare in ["/_app", "/_app/"] {
        let resp = client
            .get(format!("{}{bare}", gateway.url))
            .send()
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    }

    // Protocol mismatch (calling Subsonic endpoint with OPDS source ID) returns 400
    let mismatch = client
        .get(format!("{}/_app/subsonic/{opds12_id}/albums", gateway.url))
        .send()
        .await
        .unwrap();
    assert_eq!(mismatch.status(), StatusCode::BAD_REQUEST);

    // clear_password removes stored password
    let cleared: MutatedRemoteSourceResponse = client
        .put(format!("{}/_app/remote-sources/{opds12_id}", gateway.url))
        .json(&json!({ "clear_password": true }))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert!(!cleared.source.has_password);

    gateway.shutdown().await;
}

fn sample_cbz_bytes() -> Vec<u8> {
    // Minimal 1x1 PNG inside a CBZ zip archive
    let png_1x1: &[u8] = &[
        0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A, 0x00, 0x00, 0x00, 0x0D, 0x49, 0x48, 0x44,
        0x52, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01, 0x08, 0x02, 0x00, 0x00, 0x00, 0x90,
        0x77, 0x53, 0xDE, 0x00, 0x00, 0x00, 0x0C, 0x49, 0x44, 0x41, 0x54, 0x08, 0xD7, 0x63, 0xF8,
        0xCF, 0xC0, 0x00, 0x00, 0x03, 0x01, 0x01, 0x00, 0x18, 0xDD, 0x8D, 0xB0, 0x00, 0x00, 0x00,
        0x00, 0x49, 0x45, 0x4E, 0x44, 0xAE, 0x42, 0x60, 0x82,
    ];
    let mut cursor = Cursor::new(Vec::new());
    {
        let mut zw = ZipWriter::new(&mut cursor);
        let opts = SimpleFileOptions::default();
        zw.start_file("001.png", opts).unwrap();
        zw.write_all(png_1x1).unwrap();
        zw.finish().unwrap();
    }
    cursor.into_inner()
}

async fn spawn_mock_server(router: Router) -> (String, oneshot::Sender<()>) {
    let listener = TcpListener::bind(SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 0))
        .await
        .unwrap();
    let addr = listener.local_addr().unwrap();
    let (tx, rx) = oneshot::channel::<()>();
    tokio::spawn(async move {
        let _ = axum::serve(listener, router)
            .with_graceful_shutdown(async move {
                let _ = rx.await;
            })
            .await;
    });
    (format!("http://127.0.0.1:{}", addr.port()), tx)
}

#[tokio::test]
async fn e2e_security_guards_ssrf_proxy_xss_redirect_and_error_redaction() {
    // Attacker server counts every request it receives (should remain 0!)
    let attacker_hits = Arc::new(AtomicUsize::new(0));
    let hits_clone = attacker_hits.clone();
    let attacker_router = Router::new().fallback(get(move || {
        let hits = hits_clone.clone();
        async move {
            hits.fetch_add(1, Ordering::SeqCst);
            (StatusCode::OK, "STOLEN")
        }
    }));
    let (attacker_url, _attacker_tx) = spawn_mock_server(attacker_router).await;

    // Mock upstream server serving OPDS + Subsonic edge cases
    let attacker_redirect_target = format!("{attacker_url}/steal");
    let png_bytes: Vec<u8> = vec![
        0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A, 0x00, 0x00, 0x00, 0x0D, 0x49, 0x48, 0x44,
        0x52, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01, 0x08, 0x02, 0x00, 0x00, 0x00, 0x90,
        0x77, 0x53, 0xDE, 0x00, 0x00, 0x00, 0x0C, 0x49, 0x44, 0x41, 0x54, 0x08, 0xD7, 0x63, 0xF8,
        0xCF, 0xC0, 0x00, 0x00, 0x03, 0x01, 0x01, 0x00, 0x18, 0xDD, 0x8D, 0xB0, 0x00, 0x00, 0x00,
        0x00, 0x49, 0x45, 0x4E, 0x44, 0xAE, 0x42, 0x60, 0x82,
    ];
    let cbz_bytes = sample_cbz_bytes();
    let wav_bytes = sample_wav_bytes(800);

    let png_for_opds = png_bytes.clone();
    let png_for_pse = png_bytes.clone();
    let png_for_sub = png_bytes.clone();
    let cbz_for_dl = cbz_bytes.clone();
    let wav_for_stream = wav_bytes.clone();

    let upstream_router = Router::new()
        .route(
            "/opds",
            get(|| async {
                (
                    [(CONTENT_TYPE, "application/atom+xml")],
                    r#"<?xml version="1.0" encoding="UTF-8"?>
<feed xmlns="http://www.w3.org/2005/Atom" xmlns:pse="http://vaemendis.net/ns/opds-pse/ns">
  <title>Mock Comic OPDS</title>
  <entry>
    <id>comic-1</id>
    <title>Space Adventure</title>
    <link rel="http://opds-spec.org/acquisition" href="/download/space.cbz" type="application/vnd.comicbook+zip"/>
    <link rel="http://vaemendis.net/opds-pse/stream" href="/pse/{pageNumber}" type="image/jpeg" pse:count="10"/>
  </entry>
</feed>"#,
                )
            }),
        )
        .route(
            "/pse/{page}",
            get(move |headers: axum::http::HeaderMap| {
                let png = png_for_pse.clone();
                async move {
                    let has_basic_auth = headers
                        .get(axum::http::header::AUTHORIZATION)
                        .and_then(|v| v.to_str().ok())
                        .is_some_and(|v| v.starts_with("Basic "));
                    if !has_basic_auth {
                        return (StatusCode::UNAUTHORIZED, "missing OPDS basic auth")
                            .into_response();
                    }
                    ([(CONTENT_TYPE, "image/png")], png).into_response()
                }
            }),
        )
        .route(
            "/redirect-cross-origin",
            get(move || {
                let target = attacker_redirect_target.clone();
                async move { (StatusCode::FOUND, [(LOCATION, target)]) }
            }),
        )
        .route(
            "/img/good.png",
            get(move || {
                let body = png_for_opds.clone();
                async move { ([(CONTENT_TYPE, "image/png")], body) }
            }),
        )
        .route(
            "/img/spoofed-svg-as-png",
            get(|| async {
                (
                    [(CONTENT_TYPE, "image/png")],
                    "<svg xmlns='http://www.w3.org/2000/svg'><script>alert(1)</script></svg>",
                )
            }),
        )
        .route(
            "/img/xss.svg",
            get(|| async {
                (
                    [(CONTENT_TYPE, "image/svg+xml")],
                    "<svg xmlns='http://www.w3.org/2000/svg'><script>alert(1)</script></svg>",
                )
            }),
        )
        .route(
            "/img/xss.html",
            get(|| async { ([(CONTENT_TYPE, "text/html")], "<html><script>alert(1)</script></html>") }),
        )
        .route(
            "/img/huge.png",
            get(|| async {
                let chunk = axum::body::Bytes::from(vec![0u8; 1024 * 1024]);
                let stream = futures_util::stream::iter(
                    (0..13).map(move |_| Ok::<axum::body::Bytes, std::io::Error>(chunk.clone())),
                );
                let mut res = (StatusCode::OK, Body::from_stream(stream)).into_response();
                res.headers_mut()
                    .insert(CONTENT_TYPE, "image/png".parse().unwrap());
                res.headers_mut()
                    .insert(CONTENT_LENGTH, (13 * 1024 * 1024).to_string().parse().unwrap());
                res
            }),
        )
        .route(
            "/download/space.cbz",
            get(move || {
                let body = cbz_for_dl.clone();
                async move { ([(CONTENT_TYPE, "application/vnd.comicbook+zip")], body) }
            }),
        )
        .route(
            "/download/aborted.epub",
            get(|| async {
                let stream = futures_util::stream::iter(vec![
                    Ok::<axum::body::Bytes, std::io::Error>(axum::body::Bytes::from_static(
                        b"partial-data",
                    )),
                    Err(std::io::Error::other("simulated upstream disconnect")),
                ]);
                let mut res = (StatusCode::OK, Body::from_stream(stream)).into_response();
                res.headers_mut()
                    .insert(CONTENT_TYPE, "application/epub+zip".parse().unwrap());
                res
            }),
        )
        .route(
            "/rest/getCoverArt.view",
            get(move |axum::extract::Query(q): axum::extract::Query<std::collections::HashMap<String, String>>| {
                let png = png_for_sub.clone();
                async move {
                    match q.get("id").map(String::as_str) {
                        Some("svg") => (
                            [(CONTENT_TYPE, "image/svg+xml")],
                            b"<svg/>".to_vec(),
                        )
                            .into_response(),
                        Some("html") => (
                            [(CONTENT_TYPE, "text/html")],
                            b"<html>bad</html>".to_vec(),
                        )
                            .into_response(),
                        Some("spoofed") => (
                            [(CONTENT_TYPE, "image/png")],
                            b"<svg><script>alert(1)</script></svg>".to_vec(),
                        )
                            .into_response(),
                        _ => ([(CONTENT_TYPE, "image/png")], png).into_response(),
                    }
                }
            }),
        )
        .route(
            "/rest/download.view",
            get(|| async { (StatusCode::NOT_FOUND, "download disabled") }),
        )
        .route(
            "/rest/getSong.view",
            get(|| async {
                (
                    [(CONTENT_TYPE, "application/json")],
                    r#"{"subsonic-response":{"status":"ok","song":{"id":"song 1+2","title":"Live + Studio","suffix":"wav"}}}"#,
                )
            }),
        )
        .route(
            "/rest/stream.view",
            get(move |axum::extract::Query(q): axum::extract::Query<std::collections::HashMap<String, String>>| {
                let wav = wav_for_stream.clone();
                async move {
                    match q.get("id").map(String::as_str) {
                        Some("html") => (
                            [(CONTENT_TYPE, "text/html")],
                            "<html><body>not audio</body></html>",
                        )
                            .into_response(),
                        Some("json-err") => (
                            [(CONTENT_TYPE, "application/json")],
                            r#"{"subsonic-response":{"status":"failed","error":{"code":70,"message":"Track missing"}}}"#,
                        )
                            .into_response(),
                        Some("live") => {
                            let stream = futures_util::stream::iter(vec![Ok::<_, std::io::Error>(
                                axum::body::Bytes::from_static(b"RIFF0000WAVE"),
                            )]);
                            let mut res = (StatusCode::OK, Body::from_stream(stream)).into_response();
                            res.headers_mut()
                                .insert(CONTENT_TYPE, "audio/mpeg".parse().unwrap());
                            res
                        }
                        _ => {
                            let mut res = (StatusCode::OK, wav).into_response();
                            res.headers_mut()
                                .insert(CONTENT_TYPE, "audio/wav".parse().unwrap());
                            res
                        }
                    }
                }
            }),
        );

    let (upstream_url, _upstream_tx) = spawn_mock_server(upstream_router).await;

    let local_tmp = TempDir::new().unwrap();
    let gateway = start_gateway(
        SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 0),
        local_tmp.path().to_path_buf(),
        None,
    )
    .await
    .unwrap();
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(15))
        .build()
        .unwrap();

    // 1. Standalone gateway disables permissive CORS and rejects cross-origin requests on /api/v1/*
    let cors_probe = client
        .get(format!("{}/api/v1/system/capabilities", gateway.url))
        .header(ORIGIN, "https://evil.example")
        .send()
        .await
        .unwrap();
    assert_eq!(cors_probe.status(), StatusCode::FORBIDDEN);
    assert!(
        cors_probe
            .headers()
            .get(ACCESS_CONTROL_ALLOW_ORIGIN)
            .is_none()
    );

    // 1b. POST /_app/auto-session without X-XMedia-App: standalone header is rejected
    let missing_header_session = client
        .post(format!("{}/_app/auto-session", gateway.url))
        .send()
        .await
        .unwrap();
    assert_eq!(missing_header_session.status(), StatusCode::FORBIDDEN);

    // 2. /_app/* enforces Host, Origin, and Sec-Fetch-Site guards
    let bad_host = client
        .get(format!("{}/_app/health", gateway.url))
        .header(HOST, format!("evil.example:{}", gateway.addr.port()))
        .send()
        .await
        .unwrap();
    assert_eq!(bad_host.status(), StatusCode::FORBIDDEN);

    let bad_origin = client
        .post(format!("{}/_app/auto-session", gateway.url))
        .header(ORIGIN, "https://evil.example")
        .send()
        .await
        .unwrap();
    assert_eq!(bad_origin.status(), StatusCode::FORBIDDEN);

    let bad_port_origin = client
        .post(format!("{}/_app/auto-session", gateway.url))
        .header(ORIGIN, "http://127.0.0.1:1")
        .send()
        .await
        .unwrap();
    assert_eq!(bad_port_origin.status(), StatusCode::FORBIDDEN);

    let cross_site = client
        .post(format!("{}/_app/auto-session", gateway.url))
        .header("sec-fetch-site", "cross-site")
        .send()
        .await
        .unwrap();
    assert_eq!(cross_site.status(), StatusCode::FORBIDDEN);

    let same_site_cross_port = client
        .post(format!("{}/_app/auto-session", gateway.url))
        .header("sec-fetch-site", "same-site")
        .send()
        .await
        .unwrap();
    assert_eq!(same_site_cross_port.status(), StatusCode::FORBIDDEN);

    let same_site_api = client
        .get(format!("{}/api/v1/system/capabilities", gateway.url))
        .header("sec-fetch-site", "same-site")
        .send()
        .await
        .unwrap();
    assert_eq!(same_site_api.status(), StatusCode::FORBIDDEN);

    let same_origin_ok = client
        .get(format!("{}/_app/health", gateway.url))
        .header(ORIGIN, &gateway.url)
        .header("sec-fetch-site", "same-origin")
        .send()
        .await
        .unwrap();
    assert_eq!(same_origin_ok.status(), StatusCode::OK);

    // 2b. Mutating & proxy /_app/* routes require a valid local session cookie (401 without cookie)
    for unauthed_req in [
        client.get(format!("{}/_app/remote-sources", gateway.url)),
        client
            .post(format!("{}/_app/remote-sources", gateway.url))
            .json(&json!({})),
        client
            .put(format!("{}/_app/icon", gateway.url))
            .json(&json!({ "icon": "obsidian" })),
        client.get(format!("{}/_app/opds/any/browse", gateway.url)),
        client.get(format!("{}/_app/subsonic/any/albums", gateway.url)),
    ] {
        let resp = unauthed_req.send().await.unwrap();
        assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
    }

    let auto_session = client
        .post(format!("{}/_app/auto-session", gateway.url))
        .header(APP_HEADER_NAME, APP_HEADER_VALUE)
        .send()
        .await
        .unwrap();
    assert_eq!(auto_session.status(), StatusCode::OK);
    let cookie_pair = auto_session
        .headers()
        .get(reqwest::header::SET_COOKIE)
        .and_then(|v| v.to_str().ok())
        .and_then(|raw| raw.split(';').next())
        .expect("auto-session Set-Cookie")
        .trim()
        .to_string();
    let mut authed_headers = reqwest::header::HeaderMap::new();
    authed_headers.insert(reqwest::header::COOKIE, cookie_pair.parse().unwrap());
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(15))
        .default_headers(authed_headers)
        .build()
        .unwrap();

    assert_eq!(
        gateway.mobile_ui().cached_desired_icon(),
        Some(xmedia_app_lib::app_icon::AppIconVariant::Cobalt)
    );
    let set_icon_res = client
        .put(format!("{}/_app/icon", gateway.url))
        .json(&json!({ "icon": "porcelain" }))
        .send()
        .await
        .unwrap();
    assert_eq!(set_icon_res.status(), StatusCode::OK);
    assert_eq!(
        gateway.mobile_ui().cached_desired_icon(),
        Some(xmedia_app_lib::app_icon::AppIconVariant::Porcelain)
    );

    // 3. Register OPDS & Subsonic mock sources with secrets
    let opds_created: MutatedRemoteSourceResponse = client
        .post(format!("{}/_app/remote-sources", gateway.url))
        .json(&json!({
            "name": "Mock OPDS",
            "protocol": "opds",
            "url": format!("{upstream_url}/opds"),
            "username": "alice",
            "password": "OPDS_TOP_SECRET_PASSWORD",
            "allow_insecure_tls": false
        }))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let opds_id = opds_created.source.id;

    let sub_created: MutatedRemoteSourceResponse = client
        .post(format!("{}/_app/remote-sources", gateway.url))
        .json(&json!({
            "name": "Mock Subsonic",
            "protocol": "subsonic",
            "url": upstream_url,
            "username": "",
            "password": "SUBSONIC_API_KEY_SECRET",
            "allow_insecure_tls": false
        }))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let sub_id = sub_created.source.id;

    // 4. P0 #1: test_remote_source rejects cross-origin password reuse via source_id with 400
    let steal_attempt = client
        .post(format!("{}/_app/remote-sources/test", gateway.url))
        .json(&json!({
            "protocol": "opds",
            "url": format!("{attacker_url}/steal"),
            "username": "alice",
            "password": "",
            "allow_insecure_tls": false,
            "source_id": opds_id
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(steal_attempt.status(), StatusCode::BAD_REQUEST);
    assert_eq!(
        attacker_hits.load(Ordering::SeqCst),
        0,
        "attacker must never receive stored credentials"
    );

    // Same-origin test_remote_source with source_id succeeds
    let same_origin_test: TestRemoteSourceResponse = client
        .post(format!("{}/_app/remote-sources/test", gateway.url))
        .json(&json!({
            "protocol": "opds",
            "url": format!("{upstream_url}/opds"),
            "username": "alice",
            "password": "",
            "allow_insecure_tls": false,
            "source_id": opds_id
        }))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert!(same_origin_test.ok);

    // 5. P0 #2: OPDS SSRF, protocol-relative URL, and cross-origin redirect blocking
    for bad_url in [
        format!("{attacker_url}/steal"),
        "//evil.example/steal".to_string(),
        "file:///etc/passwd".to_string(),
    ] {
        let browse_res = client
            .get(format!("{}/_app/opds/{opds_id}/browse", gateway.url))
            .query(&[("url", bad_url.as_str())])
            .send()
            .await
            .unwrap();
        assert_eq!(browse_res.status(), StatusCode::BAD_REQUEST);

        let img_res = client
            .get(format!("{}/_app/opds/{opds_id}/proxy-image", gateway.url))
            .query(&[("url", bad_url.as_str())])
            .send()
            .await
            .unwrap();
        assert_eq!(img_res.status(), StatusCode::BAD_REQUEST);
    }

    let redirect_res = client
        .get(format!("{}/_app/opds/{opds_id}/browse", gateway.url))
        .query(&[("url", format!("{upstream_url}/redirect-cross-origin"))])
        .send()
        .await
        .unwrap();
    assert_eq!(redirect_res.status(), StatusCode::BAD_GATEWAY);
    assert_eq!(
        attacker_hits.load(Ordering::SeqCst),
        0,
        "cross-origin redirect must be blocked before contacting attacker"
    );

    // 6. P0 #3: Proxy image / cover / stream Content-Type whitelist & security headers
    let good_img = client
        .get(format!("{}/_app/opds/{opds_id}/proxy-image", gateway.url))
        .query(&[("url", format!("{upstream_url}/img/good.png"))])
        .send()
        .await
        .unwrap();
    assert_eq!(good_img.status(), StatusCode::OK);
    assert_eq!(
        good_img
            .headers()
            .get(X_CONTENT_TYPE_OPTIONS)
            .and_then(|v| v.to_str().ok()),
        Some("nosniff")
    );
    assert_eq!(
        good_img
            .headers()
            .get(CONTENT_SECURITY_POLICY)
            .and_then(|v| v.to_str().ok()),
        Some("sandbox")
    );

    for bad_img_path in ["/img/xss.svg", "/img/xss.html", "/img/spoofed-svg-as-png"] {
        let resp = client
            .get(format!("{}/_app/opds/{opds_id}/proxy-image", gateway.url))
            .query(&[("url", format!("{upstream_url}{bad_img_path}"))])
            .send()
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::UNSUPPORTED_MEDIA_TYPE);
    }

    let huge_img = client
        .get(format!("{}/_app/opds/{opds_id}/proxy-image", gateway.url))
        .query(&[("url", format!("{upstream_url}/img/huge.png"))])
        .send()
        .await
        .unwrap();
    assert_eq!(huge_img.status(), StatusCode::PAYLOAD_TOO_LARGE);

    let sub_cover_ok = client
        .get(format!("{}/_app/subsonic/{sub_id}/cover/ok", gateway.url))
        .send()
        .await
        .unwrap();
    assert_eq!(sub_cover_ok.status(), StatusCode::OK);
    assert_eq!(
        sub_cover_ok
            .headers()
            .get(X_CONTENT_TYPE_OPTIONS)
            .and_then(|v| v.to_str().ok()),
        Some("nosniff")
    );
    assert_eq!(
        sub_cover_ok
            .headers()
            .get(CONTENT_SECURITY_POLICY)
            .and_then(|v| v.to_str().ok()),
        Some("sandbox")
    );

    for bad_cover in ["svg", "html", "spoofed"] {
        let resp = client
            .get(format!(
                "{}/_app/subsonic/{sub_id}/cover/{bad_cover}",
                gateway.url
            ))
            .send()
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::UNSUPPORTED_MEDIA_TYPE);
    }

    // Subsonic proxy_stream: rejects HTML, intercepts HTTP 200 JSON error, supports HEAD, no false Accept-Ranges
    let html_stream = client
        .get(format!(
            "{}/_app/subsonic/{sub_id}/stream/html",
            gateway.url
        ))
        .send()
        .await
        .unwrap();
    assert_eq!(html_stream.status(), StatusCode::UNSUPPORTED_MEDIA_TYPE);

    let json_err_stream = client
        .get(format!(
            "{}/_app/subsonic/{sub_id}/stream/json-err",
            gateway.url
        ))
        .send()
        .await
        .unwrap();
    assert_eq!(json_err_stream.status(), StatusCode::BAD_GATEWAY);
    let err_text = json_err_stream.text().await.unwrap();
    assert!(err_text.contains("Track missing"));

    let live_stream = client
        .get(format!(
            "{}/_app/subsonic/{sub_id}/stream/live",
            gateway.url
        ))
        .send()
        .await
        .unwrap();
    assert_eq!(live_stream.status(), StatusCode::OK);
    assert!(
        live_stream.headers().get(ACCEPT_RANGES).is_none(),
        "chunked stream without Content-Length must not fabricate Accept-Ranges: bytes"
    );
    assert_eq!(
        live_stream
            .headers()
            .get(X_CONTENT_TYPE_OPTIONS)
            .and_then(|v| v.to_str().ok()),
        Some("nosniff")
    );
    assert_eq!(
        live_stream
            .headers()
            .get(CONTENT_SECURITY_POLICY)
            .and_then(|v| v.to_str().ok()),
        Some("sandbox")
    );

    let head_stream = client
        .head(format!(
            "{}/_app/subsonic/{sub_id}/stream/song%201%2B2",
            gateway.url
        ))
        .send()
        .await
        .unwrap();
    assert_eq!(head_stream.status(), StatusCode::OK);
    assert!(head_stream.bytes().await.unwrap().is_empty());

    // 7. P1 #6: Subsonic connection error redacts apiKey via without_url()
    let dead_listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let dead_port = dead_listener.local_addr().unwrap().port();
    drop(dead_listener);
    let redacted_probe: TestRemoteSourceResponse = client
        .post(format!("{}/_app/remote-sources/test", gateway.url))
        .json(&json!({
            "protocol": "subsonic",
            "url": format!("http://127.0.0.1:{dead_port}"),
            "username": "",
            "password": "SUPER_SECRET_API_KEY_999",
            "allow_insecure_tls": false
        }))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert!(!redacted_probe.ok);
    let probe_err = redacted_probe.error.unwrap_or_default();
    assert!(
        !probe_err.contains("SUPER_SECRET_API_KEY_999") && !probe_err.contains("apiKey="),
        "reqwest error leaked secret apiKey: {probe_err}"
    );

    // 8. OPDS Comic PSE browse + authenticated page stream proxy + CBZ import + aborted download temp file RAII cleanup
    let comic_feed: OpdsFeedDto = client
        .get(format!("{}/_app/opds/{opds_id}/browse", gateway.url))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(comic_feed.entries.len(), 1);
    assert_eq!(comic_feed.entries[0].page_count, Some(10));
    let pse_template = comic_feed.entries[0]
        .pse_stream_url
        .as_deref()
        .expect("PSE stream URL must be present");
    assert!(
        pse_template.starts_with(&format!("/_app/opds/{opds_id}/proxy-image?url=")),
        "PSE stream URL must route through /_app/opds/{opds_id}/..., got: {pse_template}"
    );
    let pse_page_path = pse_template.replace("{pageNumber}", "0");
    let pse_page_resp = client
        .get(format!("{}{pse_page_path}", gateway.url))
        .send()
        .await
        .unwrap();
    assert_eq!(pse_page_resp.status(), StatusCode::OK);
    assert_eq!(
        pse_page_resp
            .headers()
            .get(CONTENT_TYPE)
            .and_then(|v| v.to_str().ok()),
        Some("image/png")
    );
    let pse_page_bytes = pse_page_resp.bytes().await.unwrap();
    assert!(pse_page_bytes.starts_with(b"\x89PNG\r\n\x1a\n"));

    let comic_import = client
        .post(format!("{}/_app/opds/{opds_id}/import", gateway.url))
        .json(&json!({
            "url": format!("{upstream_url}/download/space.cbz"),
            "filename": "space.cbz"
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(comic_import.status(), StatusCode::CREATED);
    let imported_comic: RemoteImportResultDto = comic_import.json().await.unwrap();
    assert_eq!(imported_comic.media_kind, "comic");

    // Aborted stream download must fail cleanly and leave ZERO .tmp files in data_dir/tmp
    let aborted_import = client
        .post(format!("{}/_app/opds/{opds_id}/import", gateway.url))
        .json(&json!({
            "url": format!("{upstream_url}/download/aborted.epub"),
            "filename": "aborted.epub"
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(aborted_import.status(), StatusCode::BAD_GATEWAY);
    let leftover_tmps: Vec<_> = std::fs::read_dir(local_tmp.path().join("tmp"))
        .unwrap()
        .flatten()
        .collect();
    assert!(
        leftover_tmps.is_empty(),
        "expected RAII guard to clean up aborted download temp file, found: {:?}",
        leftover_tmps
    );

    // 9. Subsonic download.view -> stream.view fallback with space/+ song ID
    let fallback_import = client
        .post(format!(
            "{}/_app/subsonic/{sub_id}/import/song%201%2B2",
            gateway.url
        ))
        .json(&json!({}))
        .send()
        .await
        .unwrap();
    assert_eq!(fallback_import.status(), StatusCode::CREATED);
    let imported_track: RemoteImportResultDto = fallback_import.json().await.unwrap();
    assert_eq!(imported_track.media_kind, "music");

    gateway.shutdown().await;
}

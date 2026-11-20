use std::io::{Cursor, Write};
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::time::Duration;

use axum::http::StatusCode;
use axum::http::header::{ACCEPT_RANGES, CONTENT_RANGE, RANGE};
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
    let app = XMediaApp::connect_and_init(XMediaConfig {
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
    let router = app.router;
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

    // 2. Zero-login auto-session works immediately with no remote sources configured
    let session_resp = client
        .get(format!("{}/_app/auto-session", gateway.url))
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

    gateway.shutdown().await;
}

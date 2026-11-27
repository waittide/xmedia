use std::collections::HashMap;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, RwLock};

use async_trait::async_trait;
use axum::Router;
use axum::body::{Body, to_bytes};
use axum::http::{Request, StatusCode, header};
use bytes::Bytes;
use jiff::Timestamp;
use tempfile::TempDir;
use toasty::Db;
use toasty::schema::ModelSet;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tower::ServiceExt;
use uuid::Uuid;
use xmedia_core::application::auth::AuthService;
use xmedia_core::application::dto::{CreateUserInput, SetupAdminInput};
use xmedia_core::application::library::LibraryService;
use xmedia_core::application::ports::BinaryPayload;
use xmedia_core::domain::error::{DomainError, DomainResult};
use xmedia_core::domain::library::MediaKind;
use xmedia_core::domain::user::UserRole;
use xmedia_core::infra::events::BroadcastEventHub;
use xmedia_core::infra::ffmpeg::FfmpegToolkit;
use xmedia_core::infra::persistence::repositories::{
    ToastyLibraryRepository, ToastyUserRepository,
};
use xmedia_core::infra::security::{JwtTokenService, SecurityManager};
use xmedia_core::infra::stream::MpscByteStream;
use xmedia_core::{ensure_core_schema, register_core_models};
use xmedia_source::crud::{SourceCrudService, SourceRepository};

use super::setup_test_env;
use crate::application::dto::{BindMusicItemsInput, UploadFilePathItem};
use crate::application::online::{MAX_AUDIO_PROXY_BYTES, MusicOnlineService};
use crate::application::ports::{
    AudioMetadataProbePort, AudioProxyResponse, DownloadedTempAudio, MusicMediaProxyPort,
    MusicScannerPort, MusicStoragePort, TranscodeFormat, UpstreamAudioProxyRequest,
};
use crate::domain::catalog::{
    Album, AlbumListQuery, AlbumListType, AlbumSpec, Artist, ArtistQuery, ArtistRoleFilter,
    AudioFormat, ReplayGain, SearchQuery, SortOrder, Track, TrackId, TrackQuery, TrackSortField,
    TrackSource,
};
use crate::domain::online::{MusicSourceSpec, infer_audio_format, url_encode_component};
use crate::domain::repository::{AlbumScope, CatalogMutation, MusicCatalogRepository};
use crate::http::state::MusicHttpState;
use crate::http::{rest_router, subsonic_router};
use crate::infra::audio_probe::LoftyAudioProbe;
use crate::infra::persistence::repositories::{
    ToastyMusicCatalogRepository, ToastyMusicSourceRepository,
};
use crate::infra::source::MusicSourceEngine;
use crate::infra::storage::{LocalMusicScanner, LocalMusicStorage};
use crate::{ensure_music_schema, init_music_module_with_allow_private_ips, register_music_models};

const TINY_PNG: &[u8] = &[
    0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A, 0x00, 0x00, 0x00, 0x0D, b'I', b'H', b'D', b'R',
    0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01, 0x08, 0x06, 0x00, 0x00, 0x00, 0x1F, 0x15, 0xC4,
    0x89, 0x00, 0x00, 0x00, 0x0A, b'I', b'D', b'A', b'T', 0x78, 0x9C, 0x63, 0x00, 0x01, 0x00, 0x00,
    0x05, 0x00, 0x01, 0x0D, 0x0A, 0x2D, 0xB4, 0x00, 0x00, 0x00, 0x00, b'I', b'E', b'N', b'D', 0xAE,
    0x42, 0x60, 0x82,
];

/// Synthesizes a valid PCM 16-bit mono 8000 Hz AIFF container.
fn make_aiff(samples: u32) -> Vec<u8> {
    let data_size = samples * 2;
    let ssnd_chunk_data_size = 8 + data_size;
    let form_size = 4 + (8 + 18) + (8 + ssnd_chunk_data_size);

    let mut buf = Vec::with_capacity((8 + form_size) as usize);
    buf.extend_from_slice(b"FORM");
    buf.extend_from_slice(&form_size.to_be_bytes());
    buf.extend_from_slice(b"AIFF");

    // COMM chunk (18 bytes): 1 channel, `samples` frames, 16-bit, 8000.0 Hz (80-bit IEEE 754)
    buf.extend_from_slice(b"COMM");
    buf.extend_from_slice(&18u32.to_be_bytes());
    buf.extend_from_slice(&1u16.to_be_bytes());
    buf.extend_from_slice(&samples.to_be_bytes());
    buf.extend_from_slice(&16u16.to_be_bytes());
    buf.extend_from_slice(&[0x40, 0x0B, 0xFA, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00]);

    // SSND chunk
    buf.extend_from_slice(b"SSND");
    buf.extend_from_slice(&ssnd_chunk_data_size.to_be_bytes());
    buf.extend_from_slice(&0u32.to_be_bytes()); // offset
    buf.extend_from_slice(&0u32.to_be_bytes()); // block_size
    buf.resize(buf.len() + data_size as usize, 0);
    buf
}

#[tokio::test]
async fn expanded_audio_formats_probe_scan_stream_and_subsonic_prefixes() {
    for (raw, expected, ext, mime, lossless) in [
        ("aac", AudioFormat::Aac, "aac", "audio/aac", false),
        ("aiff", AudioFormat::Aiff, "aiff", "audio/aiff", true),
        (".aif", AudioFormat::Aiff, "aiff", "audio/aiff", true),
        ("AIFC", AudioFormat::Aiff, "aiff", "audio/aiff", true),
        ("ape", AudioFormat::Ape, "ape", "audio/x-ape", true),
        ("wv", AudioFormat::Wv, "wv", "audio/x-wavpack", true),
        ("wavpack", AudioFormat::Wv, "wv", "audio/x-wavpack", true),
    ] {
        let fmt = raw.parse::<AudioFormat>().unwrap();
        assert_eq!(fmt, expected);
        assert_eq!(fmt.extension(), ext);
        assert_eq!(fmt.mime_type(), mime);
        assert_eq!(fmt.is_lossless(), lossless);
    }

    assert_eq!(TranscodeFormat::parse("aac").unwrap(), TranscodeFormat::Aac);
    assert_eq!(
        TranscodeFormat::parse("ape").unwrap(),
        TranscodeFormat::Flac
    );
    assert_eq!(TranscodeFormat::parse("wv").unwrap(), TranscodeFormat::Flac);
    assert_eq!(TranscodeFormat::parse("aif").unwrap(), TranscodeFormat::Wav);
    assert_eq!(
        infer_audio_format("https://cdn.example.com/lossless/track.aifc?sig=1").as_deref(),
        Some("aiff")
    );
    assert_eq!(
        infer_audio_format("https://cdn.example.com/lossless/track.wv").as_deref(),
        Some("wv")
    );
    assert_eq!(
        infer_audio_format("https://cdn.example.com/lossless/track.ape").as_deref(),
        Some("ape")
    );
    assert_eq!(
        infer_audio_format("https://cdn.example.com/stream/track.aac").as_deref(),
        Some("aac")
    );

    let env = setup_test_env("formats_v2.db").await;
    let session = env
        .auth_svc
        .setup_initial_admin(SetupAdminInput {
            username: "admin".into(),
            display_name: None,
            password: "password123".into(),
        })
        .await
        .unwrap();
    let admin = env
        .auth_svc
        .authenticate(
            xmedia_core::application::dto::AuthCredential::Jwt(&session.token),
            None,
        )
        .await
        .unwrap();

    // Probe & scan AIFF (.aif) + cover + sidecar LRC
    let scan_root = env.tmp.path().join("aiff_mount");
    std::fs::create_dir_all(&scan_root).unwrap();
    let aif_file = scan_root.join("StudioMaster.aif");
    let lrc_file = scan_root.join("StudioMaster.lrc");
    let cover_file = scan_root.join("cover.png");
    std::fs::write(&aif_file, make_aiff(16_000)).unwrap();
    std::fs::write(&lrc_file, "[00:01.00]AIFF lossless line\n").unwrap();
    std::fs::write(&cover_file, TINY_PNG).unwrap();

    let probe = LoftyAudioProbe;
    let probed = probe
        .probe_audio_file(&aif_file.to_string_lossy(), "aif", "StudioMaster")
        .unwrap();
    assert_eq!(probed.format, AudioFormat::Aiff);
    assert_eq!(probed.duration_secs, 2);
    assert_eq!(probed.sample_rate_hz, Some(8000));
    assert_eq!(probed.channels, Some(1));

    let scanner = LocalMusicScanner;
    let entries = scanner
        .scan_audio_entries(&scan_root.to_string_lossy())
        .await
        .unwrap();
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].ext_hint, "aif");
    assert_eq!(
        entries[0].sidecar_lrc_relative_path.as_deref(),
        Some("StudioMaster.lrc")
    );
    assert!(entries[0].directory_cover_path.is_some());

    // Upload .aif via upload_files_from_paths and verify raw stream + Subsonic prefix stripping
    let uploaded = env
        .music_services
        .ingest
        .upload_files_from_paths(
            &admin,
            vec![
                UploadFilePathItem {
                    filename: "StudioMaster.aif".into(),
                    temp_path: aif_file,
                },
                UploadFilePathItem {
                    filename: "StudioMaster.lrc".into(),
                    temp_path: lrc_file,
                },
            ],
        )
        .await
        .unwrap();
    assert_eq!(uploaded.uploaded_tracks.len(), 1);
    let dto = &uploaded.uploaded_tracks[0];
    assert_eq!(dto.format, AudioFormat::Aiff);
    assert!(dto.is_lossless);
    assert_eq!(dto.mime_type, "audio/aiff");

    // Save an album cover so we can test Subsonic `al-` prefix stripping on getCoverArt
    let local_storage = LocalMusicStorage::new(env.tmp.path()).unwrap();
    let cover_key = local_storage
        .save_cover(
            &dto.album_id.0.to_string(),
            &BinaryPayload {
                bytes: TINY_PNG.to_vec(),
                mime_type: "image/png".into(),
            },
        )
        .await
        .unwrap();
    let repo = ToastyMusicCatalogRepository::new(env.db.clone());
    let mut alb = repo
        .query_albums(AlbumScope::Id(dto.album_id))
        .await
        .unwrap()
        .items
        .into_iter()
        .next()
        .unwrap();
    alb.set_cover_if_absent(&cover_key, Timestamp::now());
    repo.save(CatalogMutation::Album(&alb)).await.unwrap();

    let state = MusicHttpState::new(
        env.auth_svc.clone(),
        env.library_svc.clone(),
        env.music_services.clone(),
    );
    let app = Router::new()
        .nest("/api/v1/music", rest_router(state.clone()))
        .nest("/rest", subsonic_router(state));

    let stream_res = app
        .clone()
        .oneshot(
            Request::builder()
                .uri(format!(
                    "/api/v1/music/tracks/{}/stream?token={}",
                    dto.id, session.token
                ))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(stream_res.status(), StatusCode::OK);
    assert_eq!(
        stream_res
            .headers()
            .get(header::CONTENT_TYPE)
            .and_then(|v| v.to_str().ok()),
        Some("audio/aiff")
    );
    let body = to_bytes(stream_res.into_body(), usize::MAX).await.unwrap();
    assert_eq!(&body[0..4], b"FORM");
    assert_eq!(&body[8..12], b"AIFF");

    let sub_song_res = app
        .clone()
        .oneshot(
            Request::builder()
                .uri(format!(
                    "/rest/getSong?apiKey={}&f=json&id=tr-{}",
                    session.user.api_key, dto.id
                ))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(sub_song_res.status(), StatusCode::OK);
    let song_json: serde_json::Value = serde_json::from_slice(
        &to_bytes(sub_song_res.into_body(), usize::MAX)
            .await
            .unwrap(),
    )
    .unwrap();
    assert_eq!(song_json["subsonic-response"]["song"]["suffix"], "aiff");
    assert_eq!(
        song_json["subsonic-response"]["song"]["contentType"],
        "audio/aiff"
    );

    let sub_cover_res = app
        .oneshot(
            Request::builder()
                .uri(format!(
                    "/rest/getCoverArt?apiKey={}&id=al-{}",
                    session.user.api_key, dto.album_id
                ))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(sub_cover_res.status(), StatusCode::OK);
}

#[tokio::test]
async fn sql_catalog_pushdown_scales_beyond_500_bound_tracks_and_isolates_users() {
    let env = setup_test_env("sql_pushdown_540.db").await;
    let session = env
        .auth_svc
        .setup_initial_admin(SetupAdminInput {
            username: "admin".into(),
            display_name: None,
            password: "password123".into(),
        })
        .await
        .unwrap();
    let admin = env
        .auth_svc
        .authenticate(
            xmedia_core::application::dto::AuthCredential::Jwt(&session.token),
            None,
        )
        .await
        .unwrap();
    let bob_dto = env
        .auth_svc
        .create_user(
            &admin,
            CreateUserInput {
                username: "bob".into(),
                display_name: None,
                password: "password123".into(),
                role: UserRole::Member,
            },
        )
        .await
        .unwrap();

    let lib = env
        .library_svc
        .ensure_managed_library(MediaKind::Music)
        .await
        .unwrap();
    let repo = ToastyMusicCatalogRepository::new(env.db.clone());
    let now = Timestamp::now();

    let artist_main = Artist::reconcile(None, " Main Artist ", true, true, None, now).0;
    let artist_guest = Artist::reconcile(None, " Guest Vocalist ", false, true, None, now).0;
    repo.save(CatalogMutation::Artist(&artist_main))
        .await
        .unwrap();
    repo.save(CatalogMutation::Artist(&artist_guest))
        .await
        .unwrap();

    let album_rock = Album::reconcile(
        None,
        AlbumSpec {
            library_id: lib.id,
            raw_title: "Symphonic Rock",
            album_artist_id: artist_main.id,
            album_artist_name: &artist_main.name,
            year: Some(2022),
            genre: Some("Rock"),
            is_compilation: false,
            cover_hint: None,
        },
        now,
    )
    .0;
    let album_jazz = Album::reconcile(
        None,
        AlbumSpec {
            library_id: lib.id,
            raw_title: "Midnight Jazz",
            album_artist_id: artist_main.id,
            album_artist_name: &artist_main.name,
            year: Some(2024),
            genre: Some("Jazz"),
            is_compilation: false,
            cover_hint: None,
        },
        now,
    )
    .0;
    repo.save(CatalogMutation::Album(&album_rock))
        .await
        .unwrap();
    repo.save(CatalogMutation::Album(&album_jazz))
        .await
        .unwrap();

    // Insert 540 active bound tracks (> 500 UUID threshold -> exercises json_each(unhex(...))),
    // plus 15 unbound tracks and 5 missing tracks.
    let mut admin_bound_ids = Vec::with_capacity(545);
    let mut bob_bound_ids = Vec::new();
    for idx in 0..560u32 {
        let is_jazz = idx % 2 == 1;
        let album = if is_jazz { &album_jazz } else { &album_rock };
        let track_artist = if idx % 10 == 0 {
            &artist_guest
        } else {
            &artist_main
        };
        let is_missing = idx >= 555;
        let tid = TrackId::new();
        let tr = Track {
            id: tid,
            library_id: lib.id,
            album_id: album.id,
            artist_id: track_artist.id,
            album_artist_id: artist_main.id,
            title: format!("Track-{idx:04}"),
            artist_name: track_artist.name.clone(),
            album_artist_name: artist_main.name.clone(),
            album_title: album.title.clone(),
            track_number: Some((idx % 30) + 1),
            disc_number: Some(1),
            year: album.year,
            genre: album.genre.clone(),
            duration_secs: 180,
            bit_rate_kbps: Some(900),
            sample_rate_hz: Some(44_100),
            bit_depth: Some(16),
            channels: Some(2),
            format: AudioFormat::Flac,
            file_size: 1024,
            file_hash: format!("hash-{idx}"),
            replay_gain: ReplayGain::default(),
            has_embedded_lyrics: false,
            has_sidecar_lyrics: false,
            is_compilation: false,
            cover_key: None,
            source: TrackSource::Managed {
                storage_key: format!("audio/{tid}.flac"),
                sidecar_lrc_key: None,
            },
            is_missing,
            created_at: now,
            updated_at: now,
        };
        repo.save(CatalogMutation::Track(&tr)).await.unwrap();
        if idx < 540 || is_missing {
            admin_bound_ids.push(tid.0);
        }
        if idx < 3 {
            bob_bound_ids.push(tid.0);
        }
        if idx < 5 {
            repo.save(CatalogMutation::TrackTags {
                track_id: tid,
                tags: vec!["Favorites".into(), "Lossless".into()],
            })
            .await
            .unwrap();
        }
    }
    repo.save(CatalogMutation::RecomputeAggregates)
        .await
        .unwrap();

    env.music_services
        .ingest
        .bind_tracks(
            &admin,
            BindMusicItemsInput {
                item_ids: admin_bound_ids,
                ..Default::default()
            },
        )
        .await
        .unwrap();
    env.music_services
        .ingest
        .bind_tracks(
            &admin,
            BindMusicItemsInput {
                item_ids: bob_bound_ids,
                user_id: Some(bob_dto.id.0),
                ..Default::default()
            },
        )
        .await
        .unwrap();

    // Verify >500 bound tracks SQL queries for admin vs 3 bound tracks for bob
    assert_eq!(
        env.music_services
            .browse
            .count_bound_tracks_by_library(admin.id)
            .await
            .unwrap()
            .values()
            .sum::<u64>(),
        540
    );
    assert_eq!(
        env.music_services
            .browse
            .count_bound_tracks_by_library(bob_dto.id)
            .await
            .unwrap()
            .values()
            .sum::<u64>(),
        3
    );

    let overview = env
        .music_services
        .catalog
        .get_home_overview(admin.id)
        .await
        .unwrap();
    assert_eq!(overview.stats.total_tracks, 540);
    assert_eq!(overview.stats.total_albums, 2);
    assert_eq!(overview.stats.total_artists, 2);
    assert_eq!(overview.stats.genres_count, 2);
    assert_eq!(overview.stats.tags_count, 2);
    assert_eq!(overview.stats.libraries_count, 1);

    let page_tracks = env
        .music_services
        .tracks
        .query_tracks(
            admin.id,
            TrackQuery {
                genre: Some("jazz".into()),
                sort_by: TrackSortField::Title,
                sort_order: SortOrder::Asc,
                page: 2,
                page_size: 25,
                ..Default::default()
            },
        )
        .await
        .unwrap();
    assert_eq!(page_tracks.total, 270);
    assert_eq!(page_tracks.items.len(), 25);

    let tagged = env
        .music_services
        .tracks
        .query_tracks(
            admin.id,
            TrackQuery {
                tag: Some("favorites".into()),
                ..Default::default()
            },
        )
        .await
        .unwrap();
    assert_eq!(tagged.total, 5);

    let bob_albums = env
        .music_services
        .browse
        .list_albums(
            bob_dto.id,
            AlbumListQuery {
                list_type: AlbumListType::AlphabeticalByName,
                ..Default::default()
            },
        )
        .await
        .unwrap();
    assert_eq!(bob_albums.total, 2);
    // Bob has tracks 0 (Rock), 1 (Jazz), 2 (Rock) -> projected counts must be 1 and 2, NOT 270!
    assert_eq!(bob_albums.items[0].title, "Midnight Jazz");
    assert_eq!(bob_albums.items[0].track_count, 1);
    assert_eq!(bob_albums.items[1].title, "Symphonic Rock");
    assert_eq!(bob_albums.items[1].track_count, 2);

    let guest_artists = env
        .music_services
        .browse
        .list_artists(
            admin.id,
            ArtistQuery {
                role_filter: ArtistRoleFilter::TrackArtist,
                search: Some("guest".into()),
                ..Default::default()
            },
        )
        .await
        .unwrap();
    assert_eq!(guest_artists.total, 1);
    assert_eq!(guest_artists.items[0].track_count, 54);
    assert!(!guest_artists.items[0].is_album_artist);

    let genres = env
        .music_services
        .catalog
        .list_genres(admin.id)
        .await
        .unwrap();
    assert_eq!(genres.len(), 2);
    assert_eq!(genres[0].song_count, 270);

    let tags = env
        .music_services
        .catalog
        .list_tags(bob_dto.id)
        .await
        .unwrap();
    assert_eq!(tags.len(), 2);
    assert_eq!(tags[0].song_count, 3);

    let search_res = env
        .music_services
        .catalog
        .search(
            admin.id,
            SearchQuery {
                query: "Track-0539".into(),
                ..Default::default()
            },
        )
        .await
        .unwrap();
    assert_eq!(search_res.tracks.len(), 1);

    // Unbound track Track-0545 must not appear in admin's search
    let unbound_search = env
        .music_services
        .catalog
        .search(
            admin.id,
            SearchQuery {
                query: "Track-0545".into(),
                ..Default::default()
            },
        )
        .await
        .unwrap();
    assert!(unbound_search.tracks.is_empty());
}

struct OversizedMockMediaProxy {
    chunk_size: usize,
    chunk_count: usize,
}

#[async_trait]
impl MusicMediaProxyPort for OversizedMockMediaProxy {
    fn has_effective_proxy_headers(&self, _spec: &MusicSourceSpec) -> bool {
        false
    }

    async fn fetch_cover_image(
        &self,
        _url: &str,
        _headers: Option<&HashMap<String, String>>,
    ) -> DomainResult<(Vec<u8>, &'static str)> {
        Err(DomainError::NotFound("unused".into()))
    }

    async fn open_audio_proxy_stream(
        &self,
        _req: UpstreamAudioProxyRequest<'_>,
    ) -> DomainResult<AudioProxyResponse> {
        // Single refcounted buffer cloned `chunk_count` times -> O(1) memory, > 2 GiB logical stream!
        let unit = Bytes::from(vec![0x55u8; self.chunk_size]);
        let count = self.chunk_count;
        let (tx, rx) = tokio::sync::mpsc::channel::<DomainResult<Bytes>>(4);
        tokio::spawn(async move {
            for _ in 0..count {
                if tx.send(Ok(unit.clone())).await.is_err() {
                    break;
                }
            }
        });
        Ok(AudioProxyResponse {
            is_partial: false,
            content_type: "audio/mpeg".into(),
            content_length: None,
            content_range: None,
            accept_ranges: None,
            stream: MpscByteStream::into_boxed(rx),
        })
    }

    async fn download_audio_to_temp_file(
        &self,
        _url: &str,
        _headers: &HashMap<String, String>,
        _ext_hint: &str,
        _max_bytes: u64,
    ) -> DomainResult<DownloadedTempAudio> {
        Err(DomainError::NotFound("unused".into()))
    }
}

#[tokio::test]
async fn audio_proxy_requires_enabled_source_and_enforces_2gib_content_length_and_stream_cap() {
    let upstream_hits = Arc::new(AtomicUsize::new(0));
    let hits_srv = upstream_hits.clone();

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let base = format!("http://{addr}");
    let srv = tokio::spawn(async move {
        while let Ok((mut sock, _)) = listener.accept().await {
            let h = hits_srv.clone();
            tokio::spawn(async move {
                let mut req_buf = vec![0u8; 4096];
                let n = sock.read(&mut req_buf).await.unwrap_or(0);
                if n == 0 {
                    return;
                }
                h.fetch_add(1, Ordering::SeqCst);
                let req_str = String::from_utf8_lossy(&req_buf[..n]);
                if req_str.contains("/audio/too-large.flac") {
                    let head = format!(
                        "HTTP/1.1 200 OK\r\nContent-Type: audio/flac\r\nContent-Length: {}\r\nConnection: close\r\n\r\nfLaC0000",
                        MAX_AUDIO_PROXY_BYTES + 1
                    );
                    let _ = sock.write_all(head.as_bytes()).await;
                    let _ = sock.flush().await;
                } else if req_str.contains("/audio/exact-limit.flac") {
                    let head = format!(
                        "HTTP/1.1 200 OK\r\nContent-Type: audio/flac\r\nContent-Length: {}\r\n\r\n",
                        MAX_AUDIO_PROXY_BYTES
                    );
                    let _ = sock.write_all(head.as_bytes()).await;
                    let _ = sock.write_all(&[b'f'; 64]).await;
                    let _ = sock.flush().await;
                    tokio::time::sleep(std::time::Duration::from_secs(5)).await;
                } else {
                    let head = "HTTP/1.1 200 OK\r\nContent-Type: audio/mpeg\r\nContent-Length: 64\r\nConnection: close\r\n\r\n";
                    let _ = sock.write_all(head.as_bytes()).await;
                    let _ = sock.write_all(&[0xFFu8; 64]).await;
                    let _ = sock.flush().await;
                }
            });
        }
    });

    let tmp = TempDir::new().unwrap();
    let db_path = tmp.path().join("proxy_bounds.db");
    let db_url = format!("sqlite:{}", db_path.display());
    let mut models = ModelSet::new();
    register_core_models(&mut models);
    register_music_models(&mut models);
    let mut db = Db::builder().models(models).connect(&db_url).await.unwrap();
    ensure_core_schema(&mut db, &db_url).await.unwrap();
    ensure_music_schema(&mut db, &db_url).await.unwrap();

    let users_repo = Arc::new(ToastyUserRepository::new(db.clone()));
    let libraries_repo = Arc::new(ToastyLibraryRepository::new(db.clone()));
    let lifecycles = Arc::new(RwLock::new(HashMap::new()));
    let events = Arc::new(BroadcastEventHub::default());
    let security = Arc::new(SecurityManager::new("proxy-bounds-secret"));
    let tokens = Arc::new(JwtTokenService::new("proxy-bounds-secret", 3600));
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
    let music_services = init_music_module_with_allow_private_ips(
        db.clone(),
        tmp.path(),
        FfmpegToolkit::default(),
        libraries_repo,
        &library_svc,
        events,
        true,
    )
    .unwrap();

    let admin_sess = auth_svc
        .setup_initial_admin(SetupAdminInput {
            username: "admin".into(),
            display_name: None,
            password: "password123".into(),
        })
        .await
        .unwrap();
    let admin = auth_svc
        .authenticate(
            xmedia_core::application::dto::AuthCredential::Jwt(&admin_sess.token),
            None,
        )
        .await
        .unwrap();
    let _member = auth_svc
        .create_user(
            &admin,
            CreateUserInput {
                username: "member".into(),
                display_name: None,
                password: "password123".into(),
                role: UserRole::Member,
            },
        )
        .await
        .unwrap();
    let member_sess = auth_svc.login("member", "password123", None).await.unwrap();
    let member = auth_svc
        .authenticate(
            xmedia_core::application::dto::AuthCredential::Jwt(&member_sess.token),
            None,
        )
        .await
        .unwrap();

    let admin_imported = music_services
        .online
        .sources_crud()
        .import_sources(
            &admin,
            &serde_json::json!([
                { "sourceName": "EnabledSrc", "sourceUrl": base, "enabled": true },
                { "sourceName": "DisabledSrc", "sourceUrl": base, "enabled": false }
            ])
            .to_string(),
        )
        .await
        .unwrap();
    let admin_enabled_id = admin_imported.sources[0].id;
    let admin_disabled_id = admin_imported.sources[1].id;

    let member_imported = music_services
        .online
        .sources_crud()
        .import_sources(
            &member,
            &serde_json::json!([
                { "sourceName": "EnabledSrc", "sourceUrl": base, "enabled": true }
            ])
            .to_string(),
        )
        .await
        .unwrap();
    let enabled_id = member_imported.sources[0].id;

    let state = MusicHttpState::new(auth_svc, library_svc, music_services);
    let app = rest_router(state);

    let valid_target = url_encode_component(&format!("{base}/audio/valid.mp3"));

    // 1. Missing source_id -> 400 Bad Request (0 upstream hits!)
    let missing_resp = app
        .clone()
        .oneshot(
            Request::builder()
                .uri(format!("/online/audio-proxy?url={valid_target}"))
                .header(
                    header::AUTHORIZATION,
                    format!("Bearer {}", member_sess.token),
                )
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(missing_resp.status(), StatusCode::BAD_REQUEST);
    assert_eq!(upstream_hits.load(Ordering::SeqCst), 0);

    // 2. Nonexistent source_id -> 404 Not Found (0 upstream hits!)
    let unknown_resp = app
        .clone()
        .oneshot(
            Request::builder()
                .uri(format!(
                    "/online/audio-proxy?url={valid_target}&source_id={}",
                    Uuid::now_v7()
                ))
                .header(
                    header::AUTHORIZATION,
                    format!("Bearer {}", member_sess.token),
                )
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(unknown_resp.status(), StatusCode::NOT_FOUND);
    assert_eq!(upstream_hits.load(Ordering::SeqCst), 0);

    // 3. Cross-user source_id (owned by admin) -> 404 Not Found (0 upstream hits!)
    for cross_id in [admin_enabled_id, admin_disabled_id] {
        let cross_resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri(format!(
                        "/online/audio-proxy?url={valid_target}&source_id={cross_id}"
                    ))
                    .header(
                        header::AUTHORIZATION,
                        format!("Bearer {}", member_sess.token),
                    )
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(cross_resp.status(), StatusCode::NOT_FOUND);
        assert_eq!(upstream_hits.load(Ordering::SeqCst), 0);
    }

    // 4. Content-Length = 2 GiB + 1 -> 400 Bad Request
    let over_target = url_encode_component(&format!("{base}/audio/too-large.flac"));
    let over_resp = app
        .clone()
        .oneshot(
            Request::builder()
                .uri(format!(
                    "/online/audio-proxy?url={over_target}&source_id={enabled_id}"
                ))
                .header(
                    header::AUTHORIZATION,
                    format!("Bearer {}", member_sess.token),
                )
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(over_resp.status(), StatusCode::BAD_REQUEST);

    // 5. Content-Length = exact 2 GiB -> 200 OK
    let exact_target = url_encode_component(&format!("{base}/audio/exact-limit.flac"));
    let exact_resp = app
        .oneshot(
            Request::builder()
                .uri(format!(
                    "/online/audio-proxy?url={exact_target}&source_id={enabled_id}"
                ))
                .header(
                    header::AUTHORIZATION,
                    format!("Bearer {}", member_sess.token),
                )
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(exact_resp.status(), StatusCode::OK);
    assert_eq!(
        exact_resp
            .headers()
            .get(header::CONTENT_LENGTH)
            .and_then(|v| v.to_str().ok()),
        Some(MAX_AUDIO_PROXY_BYTES.to_string().as_str())
    );

    // 6. Unbounded stream without Content-Length exceeding 2 GiB (129 * 16 MiB = 2064 MiB)
    //    is terminated at chunk #129 by StreamByteLimiter inside stream_audio_proxy!
    let sources_repo: Arc<dyn SourceRepository> = Arc::new(ToastyMusicSourceRepository::new(db));
    let sources_crud = Arc::new(SourceCrudService::<MusicSourceSpec>::new(sources_repo));
    let mock_proxy = Arc::new(OversizedMockMediaProxy {
        chunk_size: 16 * 1024 * 1024,
        chunk_count: 130,
    });
    let bounded_svc = MusicOnlineService::with_ports(
        sources_crud,
        Arc::new(MusicSourceEngine::default()),
        mock_proxy,
    );
    let mut proxy_out = bounded_svc
        .stream_audio_proxy(
            &member,
            "https://audio.example.com/infinite.mp3",
            enabled_id,
            None,
            None,
        )
        .await
        .unwrap();

    let mut emitted_bytes: u64 = 0;
    let mut hit_cap_err = false;
    while let Some(item) = std::future::poll_fn(|cx| proxy_out.stream.as_mut().poll_next(cx)).await
    {
        match item {
            Ok(chunk) => {
                emitted_bytes += chunk.len() as u64;
                assert!(emitted_bytes <= MAX_AUDIO_PROXY_BYTES);
            }
            Err(DomainError::Validation(msg)) => {
                assert!(msg.contains("最大字节限制"));
                hit_cap_err = true;
                break;
            }
            Err(other) => panic!("unexpected stream error: {other:?}"),
        }
    }
    assert!(
        hit_cap_err,
        "stream_audio_proxy must abort once bytes exceed 2 GiB"
    );
    assert_eq!(emitted_bytes, MAX_AUDIO_PROXY_BYTES);

    srv.abort();
}

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use axum::Router;
use axum::extract::Query;
use axum::http::StatusCode;
use axum::routing::get;
use serde::Deserialize;
use xmedia_source::crud::{MASKED_SECRET_PLACEHOLDER, SourceSpecAdapter};
use xmedia_source::http::HttpClientConfig;
use xmedia_source::model::SourceRuntimeState;

use super::{SEEDS_MUSIC_JSON, send_json, setup_online_test_harness};
use crate::domain::catalog::StructuredLyrics;
use crate::domain::online::{
    MusicSourceSpec, MusicTrackContext, infer_audio_format, normalize_artist, parse_duration_secs,
    url_encode_component,
};
use crate::infra::source::MusicSourceEngine;

#[test]
fn seeds_music_json_parses_cleanly_and_helpers_behave_correctly() {
    let parsed = <MusicSourceSpec as SourceSpecAdapter>::parse_batch(SEEDS_MUSIC_JSON).unwrap();
    assert_eq!(parsed.len(), 4);
    assert_eq!(parsed[0].0.source_name, "GDStudio Music API (网易云)");
    assert_eq!(
        parsed[0].0.supported_qualities,
        vec!["128", "192", "320", "740", "999"]
    );
    assert_eq!(parsed[1].0.source_name, "Audius Public API");
    assert_eq!(parsed[2].0.source_name, "Internet Archive Audio");
    assert_eq!(parsed[3].0.source_name, "iTunes Search + LRCLIB");

    let custom_raw = r#"{
        "name": "Custom",
        "url": "https://example.test",
        "enabled": "false",
        "weight": "42",
        "Header": {"User-Agent": "CustomUA"},
        "mediaHeader": {"Referer": "https://media.test/"},
        "supportedQualities": "128, 320 | flac"
    }"#;
    let custom_batch = <MusicSourceSpec as SourceSpecAdapter>::parse_batch(custom_raw).unwrap();
    let spec = &custom_batch[0].0;
    assert!(!spec.enabled);
    assert_eq!(spec.weight, 42);
    assert_eq!(spec.supported_qualities, vec!["128", "320", "flac"]);
    let merged_hdr = spec.parse_media_headers();
    assert_eq!(
        merged_hdr.get("User-Agent").map(String::as_str),
        Some("CustomUA")
    );
    assert_eq!(
        merged_hdr.get("Referer").map(String::as_str),
        Some("https://media.test/")
    );

    assert_eq!(
        url_encode_component("R&B #1 中文"),
        "R%26B%20%231%20%E4%B8%AD%E6%96%87"
    );
    assert_eq!(
        normalize_artist("  Artist A \n\n Artist B  "),
        "Artist A / Artist B"
    );
    assert_eq!(parse_duration_secs("35:57", None), Some(2157));
    assert_eq!(parse_duration_secs("1:02:03", None), Some(3723));
    assert_eq!(parse_duration_secs("225868", Some("ms")), Some(226));
    assert_eq!(parse_duration_secs("212", Some("s")), Some(212));
    assert_eq!(parse_duration_secs("3433.45", Some("auto")), Some(3433));
    assert_eq!(parse_duration_secs("180000", Some("auto")), Some(180));

    assert_eq!(
        infer_audio_format("https://cdn.example.com/track/song.MP3?token=abc#v1"),
        Some("mp3".into())
    );
    assert_eq!(
        infer_audio_format("https://cdn.example.com/track/song.m4a"),
        Some("m4a".into())
    );
    assert_eq!(
        infer_audio_format("https://cdn.example.com/track/noext"),
        None
    );

    let lrc =
        StructuredLyrics::from("[ti:Mojito]\n[ar:周杰伦]\n[00:01.20]麻烦给我的爱人来一杯Mojito\n");
    assert!(lrc.synced);
    assert_eq!(lrc.display_title.as_deref(), Some("Mojito"));
    assert_eq!(lrc.display_artist.as_deref(), Some("周杰伦"));
    assert_eq!(lrc.lines.len(), 1);
    assert_eq!(lrc.lines[0].start_ms, 1200);
}

#[tokio::test]
async fn engine_replays_all_four_seed_protocols_login_and_error_handling() {
    #[derive(Deserialize)]
    struct GdQuery {
        types: Option<String>,
        id: Option<String>,
        br: Option<String>,
    }

    let app = Router::new()
        .route(
            "/gd/api.php",
            get(|Query(q): Query<GdQuery>| async move {
                match q.types.as_deref() {
                    Some("search") => axum::Json(serde_json::json!([
                        {
                            "id": "2736327941",
                            "name": "Mojito (Live)",
                            "artist": ["孙焱彬", "周杰伦"],
                            "album": "Live Collection",
                            "pic_id": "109951171867660113",
                            "url_id": "2736327941",
                            "lyric_id": "2736327941"
                        }
                    ])),
                    Some("url") => axum::Json(serde_json::json!({
                        "url": format!("http://127.0.0.1:9/audio/{}.mp3?br={}", q.id.unwrap_or_default(), q.br.as_deref().unwrap_or("320")),
                        "br": 320,
                        "size": 21222966
                    })),
                    Some("pic") => axum::Json(serde_json::json!({
                        "url": "https://p2.music.126.net/demo.jpg?param=500y500"
                    })),
                    Some("lyric") => axum::Json(serde_json::json!({
                        "lyric": "[00:00.00]Mojito\n[00:12.50]麻烦给我的爱人来一杯Mojito",
                        "tlyric": "[00:12.50]Please get my lover a Mojito"
                    })),
                    _ => axum::Json(serde_json::json!({})),
                }
            }),
        )
        .route(
            "/v1/tracks/search",
            get(|| async {
                axum::Json(serde_json::json!({
                    "data": [{
                        "id": "V66vyPP",
                        "title": "Xsetra - Fragments",
                        "duration": 212,
                        "genre": "Drum & Bass",
                        "user": { "name": "Xsetra" },
                        "artwork": { "480x480": "https://cdn.audius.test/480.jpg" }
                    }]
                }))
            }),
        )
        .route(
            "/v1/tracks/trending",
            get(|| async {
                axum::Json(serde_json::json!({
                    "data": [{
                        "id": "TREND1",
                        "title": "Trending Beat",
                        "duration": 180,
                        "genre": "Electronic",
                        "user": { "name": "DJ Demo" },
                        "artwork": { "150x150": "https://cdn.audius.test/150.jpg" }
                    }]
                }))
            }),
        )
        .route(
            "/v1/tracks/{id}",
            get(|| async {
                axum::Json(serde_json::json!({
                    "data": {
                        "id": "V66vyPP",
                        "title": "Xsetra - Fragments (Full)",
                        "duration": 212,
                        "genre": "Drum & Bass",
                        "user": { "name": "Xsetra" },
                        "artwork": { "480x480": "https://cdn.audius.test/480.jpg" }
                    }
                }))
            }),
        )
        .route(
            "/advancedsearch.php",
            get(|| async {
                axum::Json(serde_json::json!({
                    "response": {
                        "docs": [
                            {
                                "identifier": "bndf_20250623",
                                "title": "BEETHOVEN Klavierkonzert Nr. 3"
                            },
                            {
                                "identifier": "danutz-22",
                                "title": "BEETHOVEN IN 22 CENTURY",
                                "creator": "DANUTZ"
                            }
                        ]
                    }
                }))
            }),
        )
        .route(
            "/metadata/bndf_20250623",
            get(|| async {
                axum::Json(serde_json::json!({
                    "metadata": {
                        "identifier": "bndf_20250623",
                        "title": "BEETHOVEN Klavierkonzert Nr. 3",
                        "creator": ["Ludwig van Beethoven", "Orchestra"]
                    },
                    "files": [
                        { "name": "secret.mp3", "private": "true", "length": "01:00", "bitrate": "128" },
                        { "name": "subdir/track 8.mp3", "format": "VBR MP3", "bitrate": "136", "length": "35:57" }
                    ]
                }))
            }),
        )
        .route(
            "/search",
            get(|| async {
                axum::Json(serde_json::json!({
                    "results": [{
                        "trackId": 6814997425i64,
                        "trackName": "Shake It Off",
                        "artistName": "Taylor Swift",
                        "collectionName": "1989",
                        "previewUrl": "https://audio-ssl.itunes.apple.com/preview.m4a",
                        "artworkUrl100": "https://is1-ssl.mzstatic.com/image/100x100bb.jpg",
                        "trackTimeMillis": 225868
                    }]
                }))
            }),
        )
        .route(
            "/lookup",
            get(|| async {
                axum::Json(serde_json::json!({
                    "results": [{
                        "trackId": 6814997425i64,
                        "trackName": "Shake It Off",
                        "artistName": "Taylor Swift",
                        "collectionName": "1989",
                        "previewUrl": "https://audio-ssl.itunes.apple.com/lookup-preview.m4a",
                        "artworkUrl100": "https://is1-ssl.mzstatic.com/image/100x100bb.jpg",
                        "trackTimeMillis": 225868
                    }]
                }))
            }),
        )
        .route(
            "/api/search",
            get(|| async {
                axum::Json(serde_json::json!([
                    {
                        "id": 1,
                        "syncedLyrics": "[00:00.98] I stay out too late\n[00:04.20] Got nothing in my brain",
                        "plainLyrics": "I stay out too late"
                    }
                ]))
            }),
        )
        .route(
            "/error-500",
            get(|| async { (StatusCode::INTERNAL_SERVER_ERROR, "upstream error") }),
        );

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let server = tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });
    let base = format!("http://{addr}");

    let engine = MusicSourceEngine::new(
        HttpClientConfig {
            timeout_secs: 5,
            allow_private_ips: true,
            proxy_url: None,
        },
        Duration::from_secs(10),
    );

    let mut specs: Vec<MusicSourceSpec> =
        <MusicSourceSpec as SourceSpecAdapter>::parse_batch(SEEDS_MUSIC_JSON)
            .unwrap()
            .into_iter()
            .map(|(s, _)| s)
            .collect();

    tokio::task::spawn_blocking(move || {
        {
            let gd = &mut specs[0];
            gd.source_url = base.clone();
            gd.search_url = Some(format!(
                "{base}/gd/api.php?types=search&source=netease&name={{{{key}}}}&count={{{{limit}}}}&pages={{{{page}}}}"
            ));
            gd.explore_url = Some(format!(
                "热歌榜::{base}/gd/api.php?types=search&source=netease&name=hot&count={{{{limit}}}}&pages={{{{page}}}}"
            ));
            gd.rule_stream.as_mut().unwrap().request_url = Some(format!(
                "{base}/gd/api.php?types=url&source=netease&id={{{{urlId}}}}&br={{{{quality}}}}"
            ));
            gd.rule_cover.as_mut().unwrap().request_url = Some(format!(
                "{base}/gd/api.php?types=pic&source=netease&id={{{{coverId}}}}&size=500"
            ));
            gd.rule_lyric.as_mut().unwrap().request_url = Some(format!(
                "{base}/gd/api.php?types=lyric&source=netease&id={{{{lyricId}}}}"
            ));

            let s_out = engine
                .search_sync(gd, SourceRuntimeState::default(), "Mojito", 1, 10)
                .unwrap();
            assert_eq!(s_out.data.len(), 1);
            assert_eq!(s_out.data[0].artist, "孙焱彬 / 周杰伦");
            let ctx = s_out.data[0].to_context(Some("320".into()));

            let stream_out = engine
                .resolve_stream_sync(gd, SourceRuntimeState::default(), &ctx, Some("320"))
                .unwrap();
            assert!(stream_out.data.stream_url.contains("br=320"));
            assert_eq!(stream_out.data.format, "mp3");
            assert_eq!(stream_out.data.bitrate, Some(320));
            assert_eq!(stream_out.data.expires_in_secs, Some(1200));

            let cover_out = engine
                .resolve_cover_sync(gd, SourceRuntimeState::default(), &ctx)
                .unwrap();
            assert_eq!(
                cover_out.data.as_deref(),
                Some("https://p2.music.126.net/demo.jpg?param=500y500")
            );

            let lrc_out = engine
                .resolve_lyrics_sync(gd, SourceRuntimeState::default(), &ctx)
                .unwrap()
                .data
                .unwrap();
            assert!(lrc_out.lyrics.synced);
            assert_eq!(lrc_out.lyrics.lines.len(), 2);
            assert!(lrc_out.translated_lyrics.is_some());
        }

        {
            let audius = &mut specs[1];
            audius.source_url = base.clone();
            audius.search_url = Some(format!(
                "{base}/v1/tracks/search?query={{{{key}}}}&limit={{{{limit}}}}"
            ));
            audius.explore_url = Some(format!(
                "热门::{base}/v1/tracks/trending?limit={{{{limit}}}}"
            ));
            audius.rule_track_detail.as_mut().unwrap().request_url =
                Some(format!("{base}/v1/tracks/{{{{trackId}}}}"));

            let s_out = engine
                .search_sync(audius, SourceRuntimeState::default(), "fragments", 1, 10)
                .unwrap();
            assert_eq!(s_out.data[0].duration_secs, Some(212));
            let exp_out = engine
                .explore_sync(audius, SourceRuntimeState::default(), None, 1, 10)
                .unwrap();
            assert_eq!(exp_out.data.tracks[0].track_id, "TREND1");

            let detail_out = engine
                .fetch_track_detail_sync(
                    audius,
                    SourceRuntimeState::default(),
                    &s_out.data[0].to_context(None),
                )
                .unwrap();
            assert_eq!(detail_out.data.title, "Xsetra - Fragments (Full)");

            let stream_out = engine
                .resolve_stream_sync(
                    audius,
                    SourceRuntimeState::default(),
                    &detail_out.data.to_context(None),
                    None,
                )
                .unwrap();
            assert_eq!(
                stream_out.data.stream_url,
                "https://api.audius.co/v1/tracks/V66vyPP/stream?app_name=xmedia"
            );
            assert_eq!(stream_out.data.format, "mp3");
            assert_eq!(stream_out.data.bitrate, Some(320));
        }

        {
            let ia = &mut specs[2];
            ia.source_url = base.clone();
            ia.search_url = Some(format!("{base}/advancedsearch.php?q={{{{key}}}}"));
            ia.rule_track_detail.as_mut().unwrap().request_url =
                Some(format!("{base}/metadata/{{{{trackId}}}}"));
            ia.rule_stream.as_mut().unwrap().request_url =
                Some(format!("{base}/metadata/{{{{urlId}}}}"));

            let s_out = engine
                .search_sync(ia, SourceRuntimeState::default(), "beethoven", 1, 10)
                .unwrap();
            assert_eq!(s_out.data.len(), 2);
            assert_eq!(s_out.data[0].artist, "");
            assert_eq!(s_out.data[1].artist, "DANUTZ");

            let detail_out = engine
                .fetch_track_detail_sync(
                    ia,
                    SourceRuntimeState::default(),
                    &s_out.data[0].to_context(None),
                )
                .unwrap();
            assert_eq!(detail_out.data.duration_secs, Some(2157));
            assert_eq!(detail_out.data.artist, "Ludwig van Beethoven / Orchestra");

            let stream_out = engine
                .resolve_stream_sync(
                    ia,
                    SourceRuntimeState::default(),
                    &detail_out.data.to_context(None),
                    None,
                )
                .unwrap();
            assert_eq!(
                stream_out.data.stream_url,
                "https://archive.org/download/bndf_20250623/subdir/track%208.mp3"
            );
            assert_eq!(stream_out.data.bitrate, Some(136));
        }

        {
            let itunes = &mut specs[3];
            itunes.source_url = base.clone();
            itunes.search_url = Some(format!("{base}/search?term={{{{key}}}}"));
            itunes.rule_track_detail.as_mut().unwrap().request_url =
                Some(format!("{base}/lookup?id={{{{trackId}}}}"));
            itunes.rule_stream.as_mut().unwrap().request_url =
                Some(format!("{base}/lookup?id={{{{trackId}}}}"));
            itunes.rule_lyric.as_mut().unwrap().request_url =
                Some(format!("{base}/api/search?q={{{{title}}}} {{{{artist}}}}"));

            let s_out = engine
                .search_sync(itunes, SourceRuntimeState::default(), "Taylor Swift", 1, 10)
                .unwrap();
            assert_eq!(s_out.data[0].duration_secs, Some(226));
            assert_eq!(
                s_out.data[0].cover_url.as_deref(),
                Some("https://is1-ssl.mzstatic.com/image/600x600bb.jpg")
            );

            let mut ctx_no_play = s_out.data[0].to_context(None);
            ctx_no_play.play_url = None;
            let stream_out = engine
                .resolve_stream_sync(itunes, SourceRuntimeState::default(), &ctx_no_play, None)
                .unwrap();
            assert_eq!(
                stream_out.data.stream_url,
                "https://audio-ssl.itunes.apple.com/lookup-preview.m4a"
            );
            assert_eq!(stream_out.data.format, "m4a");

            let lrc_out = engine
                .resolve_lyrics_sync(
                    itunes,
                    SourceRuntimeState::default(),
                    &s_out.data[0].to_context(None),
                )
                .unwrap()
                .data
                .unwrap();
            assert!(lrc_out.lyrics.synced);
            assert_eq!(lrc_out.lyrics.lines.len(), 2);
        }

        {
            let mut login_spec = MusicSourceSpec {
                source_name: "Login Test".into(),
                source_url: base.clone(),
                search_url: Some(format!("{base}/error-500")),
                login_url: Some(
                    "function login() { source.setVariable('vip_' + result.user); java.toast('ok'); return 'signed_in'; }"
                        .into(),
                ),
                ..Default::default()
            };
            login_spec.rule_search = specs[0].rule_search.clone();
            let login_res = engine
                .run_login_action_sync(
                    &login_spec,
                    SourceRuntimeState::default(),
                    HashMap::from([("user".into(), "alice".into())]),
                    Some("login()"),
                )
                .unwrap();
            assert_eq!(login_res.data, "signed_in");
            assert_eq!(login_res.state.variable, "vip_alice");
            assert_eq!(login_res.diagnostics.toasts, vec!["ok"]);

            let err_res = engine.search_sync(&login_spec, SourceRuntimeState::default(), "test", 1, 5);
            assert!(err_res.is_err());

            let empty_ctx = MusicTrackContext::default();
            let missing_stream = engine.resolve_stream_sync(
                &login_spec,
                SourceRuntimeState::default(),
                &empty_ctx,
                None,
            );
            assert!(missing_stream.is_err());
        }
    })
    .await
    .unwrap();

    server.abort();
}

#[tokio::test]
async fn engine_blocks_template_js_injection_and_fixes_short_circuit_headers_ia_login_cover() {
    use std::sync::atomic::{AtomicUsize, Ordering};

    use crate::domain::online::{MusicCoverRule, MusicSearchRule, MusicStreamRule};

    let lookup_hits = Arc::new(AtomicUsize::new(0));
    let lookup_hits_srv = lookup_hits.clone();
    let seen_queries: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));
    let seen_queries_srv = seen_queries.clone();

    #[derive(Deserialize)]
    struct LookupQuery {
        id: Option<String>,
        q: Option<String>,
    }

    let app = Router::new()
        .route(
            "/echo-search",
            get(move |axum::extract::RawQuery(raw_q): axum::extract::RawQuery| {
                let sq = seen_queries_srv.clone();
                async move {
                    if let Some(q) = raw_q {
                        sq.lock().unwrap().push(q);
                    }
                    axum::Json(serde_json::json!([
                        { "id": "t1", "name": "Safe Song", "artist": "Safe Artist" }
                    ]))
                }
            }),
        )
        .route(
            "/stream-lookup",
            get(move |Query(q): Query<LookupQuery>| {
                let hits = lookup_hits_srv.clone();
                async move {
                    hits.fetch_add(1, Ordering::SeqCst);
                    axum::Json(serde_json::json!({
                        "url": format!(
                            "https://cdn.example.test/audio/{}/{}.m4a",
                            q.id.as_deref().unwrap_or("none"),
                            q.q.as_deref().unwrap_or("default")
                        ),
                        "fmt": "flac",
                        "br": 999,
                        "ttl": 600,
                        "dynHeaders": { "X-Dyn": "from-jsonpath", "Referer": "https://dyn.test/" }
                    }))
                }
            }),
        )
        .route(
            "/metadata/ia-bool-priv",
            get(|| async {
                axum::Json(serde_json::json!({
                    "metadata": {
                        "identifier": "ia-bool-priv",
                        "title": "Public OGG Item",
                        "creator": "Archivist"
                    },
                    "files": [
                        { "name": "secret_bool.mp3", "private": true, "length": "01:00", "bitrate": "320" },
                        { "name": "secret_str.mp3", "private": "true", "length": "02:00", "bitrate": "256" },
                        { "name": "public_track.ogg", "length": "03:30", "bitrate": "192.4" }
                    ]
                }))
            }),
        );

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let server = tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });
    let base = format!("http://{addr}");

    let engine = MusicSourceEngine::new(
        HttpClientConfig {
            timeout_secs: 5,
            allow_private_ips: true,
            proxy_url: None,
        },
        Duration::from_secs(10),
    );

    let seeds: Vec<MusicSourceSpec> =
        <MusicSourceSpec as SourceSpecAdapter>::parse_batch(SEEDS_MUSIC_JSON)
            .unwrap()
            .into_iter()
            .map(|(s, _)| s)
            .collect();

    tokio::task::spawn_blocking(move || {
        let inj_spec = MusicSourceSpec {
            source_name: "Injection Guard".into(),
            source_url: base.clone(),
            search_url: Some(format!(
                "{base}/echo-search?k={{{{key}}}}&extra={{{{1+1}}}}"
            )),
            rule_search: Some(MusicSearchRule {
                track_list: Some("$".into()),
                track_id: Some("$.id".into()),
                title: Some("$.name".into()),
                artist: Some("$.artist".into()),
                ..Default::default()
            }),
            rule_stream: Some(MusicStreamRule {
                stream_url: Some(
                    "{{playUrl}}||https://cdn.example.test/tracks/{{urlId}}/{{trackId}}.mp3?q={{1+1}}"
                        .into(),
                ),
                format: Some("mp3".into()),
                ..Default::default()
            }),
            ..Default::default()
        };

        let search_out = engine
            .search_sync(
                &inj_spec,
                SourceRuntimeState::default(),
                "{{java.toast('INJECTED_KEY')}}",
                1,
                10,
            )
            .unwrap();
        assert!(search_out.diagnostics.toasts.is_empty());
        {
            let qs = seen_queries.lock().unwrap();
            assert!(qs[0].contains("extra=2"));
            assert!(!qs[0].contains("INJECTED_KEY") || qs[0].contains("%7B%7B"));
        }

        let mut ctx_play = MusicTrackContext {
            track_id: "{{1+1}}".into(),
            url_id: Some("{{java.toast('INJECTED_URLID')}}".into()),
            play_url: Some("https://cdn.example.test/{{java.toast('INJECTED_PLAY')}}.mp3".into()),
            ..Default::default()
        };
        let play_out = engine
            .resolve_stream_sync(&inj_spec, SourceRuntimeState::default(), &ctx_play, None)
            .unwrap();
        assert!(play_out.diagnostics.toasts.is_empty());
        assert_eq!(
            play_out.data.stream_url,
            "https://cdn.example.test/%7B%7Bjava.toast('INJECTED_PLAY')%7D%7D.mp3"
        );

        ctx_play.play_url = None;
        let stream_out = engine
            .resolve_stream_sync(&inj_spec, SourceRuntimeState::default(), &ctx_play, None)
            .unwrap();
        assert!(stream_out.diagnostics.toasts.is_empty());
        assert_eq!(
            stream_out.data.stream_url,
            format!(
                "https://cdn.example.test/tracks/{}/{}.mp3?q=2",
                url_encode_component("{{java.toast('INJECTED_URLID')}}"),
                url_encode_component("{{1+1}}"),
            )
        );

        let js_spec = MusicSourceSpec {
            source_name: "JS Global Guard".into(),
            source_url: base.clone(),
            rule_stream: Some(MusicStreamRule {
                stream_url: Some(
                    "@js:'https://cdn.example.test/' + encodeURIComponent(track.urlId) + '?q=' + quality"
                        .into(),
                ),
                ..Default::default()
            }),
            ..Default::default()
        };
        let js_ctx = MusicTrackContext {
            track_id: "1".into(),
            url_id: Some("';java.toast('PWN');'".into()),
            ..Default::default()
        };
        let js_out = engine
            .resolve_stream_sync(&js_spec, SourceRuntimeState::default(), &js_ctx, Some("320"))
            .unwrap();
        assert!(js_out.diagnostics.toasts.is_empty());
        assert_eq!(
            js_out.data.stream_url,
            "https://cdn.example.test/'%3Bjava.toast('PWN')%3B'?q=320"
        );

        let mut sc_spec = MusicSourceSpec {
            source_name: "ShortCircuit Spec".into(),
            source_url: base.clone(),
            supported_qualities: vec!["256".into()],
            rule_stream: Some(MusicStreamRule {
                request_url: Some(format!(
                    "{base}/stream-lookup?id={{{{trackId}}}}&q={{{{ quality }}}}"
                )),
                stream_url: Some("$.url".into()),
                format: Some("m4a".into()),
                bitrate: Some("256".into()),
                ..Default::default()
            }),
            ..Default::default()
        };
        let sc_ctx = MusicTrackContext {
            track_id: "trk1".into(),
            play_url: Some("https://cdn.example.test/preview.m4a".into()),
            ..Default::default()
        };
        let out_q = engine
            .resolve_stream_sync(&sc_spec, SourceRuntimeState::default(), &sc_ctx, Some("256"))
            .unwrap();
        assert_eq!(lookup_hits.load(Ordering::SeqCst), 1);
        assert!(out_q.data.stream_url.ends_with("/trk1/256.m4a"));

        sc_spec.rule_stream.as_mut().unwrap().request_url =
            Some(format!("{base}/stream-lookup?id={{{{trackId}}}}"));
        sc_spec.rule_stream.as_mut().unwrap().headers = Some(r#"{"X-Static":"1"}"#.into());
        let out_static_hdr = engine
            .resolve_stream_sync(&sc_spec, SourceRuntimeState::default(), &sc_ctx, None)
            .unwrap();
        assert_eq!(lookup_hits.load(Ordering::SeqCst), 2);
        assert_eq!(
            out_static_hdr.data.headers.get("X-Static").map(String::as_str),
            Some("1")
        );

        sc_spec.rule_stream.as_mut().unwrap().headers = Some("$.dynHeaders".into());
        sc_spec.rule_stream.as_mut().unwrap().format = Some("$.fmt".into());
        sc_spec.rule_stream.as_mut().unwrap().bitrate = Some("$.br".into());
        sc_spec.rule_stream.as_mut().unwrap().expires_in_secs = Some("$.ttl".into());
        let out_dyn = engine
            .resolve_stream_sync(&sc_spec, SourceRuntimeState::default(), &sc_ctx, None)
            .unwrap();
        assert_eq!(lookup_hits.load(Ordering::SeqCst), 3);
        assert_eq!(out_dyn.data.format, "flac");
        assert_eq!(out_dyn.data.bitrate, Some(999));
        assert_eq!(out_dyn.data.expires_in_secs, Some(600));
        assert_eq!(
            out_dyn.data.headers.get("X-Dyn").map(String::as_str),
            Some("from-jsonpath")
        );

        sc_spec.rule_stream.as_mut().unwrap().headers = None;
        sc_spec.rule_stream.as_mut().unwrap().format = Some("m4a".into());
        sc_spec.rule_stream.as_mut().unwrap().bitrate = Some("256".into());
        sc_spec.rule_stream.as_mut().unwrap().expires_in_secs = None;
        let out_fast = engine
            .resolve_stream_sync(&sc_spec, SourceRuntimeState::default(), &sc_ctx, Some("256"))
            .unwrap();
        assert_eq!(lookup_hits.load(Ordering::SeqCst), 3);
        assert_eq!(out_fast.data.stream_url, "https://cdn.example.test/preview.m4a");

        let mut ia = seeds[2].clone();
        ia.source_url = base.clone();
        ia.rule_track_detail.as_mut().unwrap().request_url =
            Some(format!("{base}/metadata/{{{{trackId}}}}"));
        ia.rule_stream.as_mut().unwrap().request_url =
            Some(format!("{base}/metadata/{{{{urlId}}}}"));
        let ia_ctx = MusicTrackContext {
            track_id: "ia-bool-priv".into(),
            url_id: Some("ia-bool-priv".into()),
            ..Default::default()
        };
        let ia_detail = engine
            .fetch_track_detail_sync(&ia, SourceRuntimeState::default(), &ia_ctx)
            .unwrap();
        assert_eq!(ia_detail.data.duration_secs, Some(210));
        let ia_stream = engine
            .resolve_stream_sync(&ia, SourceRuntimeState::default(), &ia_ctx, None)
            .unwrap();
        assert_eq!(
            ia_stream.data.stream_url,
            "https://archive.org/download/ia-bool-priv/public_track.ogg"
        );
        assert_eq!(ia_stream.data.format, "ogg");
        assert_eq!(ia_stream.data.bitrate, Some(192));

        let login_spec = MusicSourceSpec {
            source_name: "Case-insensitive Login".into(),
            source_url: base.clone(),
            login_url: Some("@JS: function login() { return result.password; }".into()),
            ..Default::default()
        };
        let mut init_rt = SourceRuntimeState::default();
        init_rt
            .login_info
            .insert("password".into(), "real_secret_pw".into());
        let login_out = engine
            .run_login_action_sync(
                &login_spec,
                init_rt,
                HashMap::from([(
                    "password".into(),
                    MASKED_SECRET_PLACEHOLDER.to_string(),
                )]),
                Some("login()"),
            )
            .unwrap();
        assert_eq!(login_out.data, "real_secret_pw");
        assert_eq!(
            login_out.state.login_info.get("password").map(String::as_str),
            Some("real_secret_pw")
        );

        let cover_spec = MusicSourceSpec {
            source_name: "Whitespace Cover Req".into(),
            source_url: base,
            rule_cover: Some(MusicCoverRule {
                request_url: Some("   ".into()),
                cover_url: Some("https://override.test/wrong.jpg".into()),
            }),
            ..Default::default()
        };
        let cover_ctx = MusicTrackContext {
            track_id: "c1".into(),
            cover_url: Some("https://cdn.example.test/existing.jpg".into()),
            ..Default::default()
        };
        let cover_out = engine
            .resolve_cover_sync(&cover_spec, SourceRuntimeState::default(), &cover_ctx)
            .unwrap();
        assert_eq!(
            cover_out.data.as_deref(),
            Some("https://cdn.example.test/existing.jpg")
        );
    })
    .await
    .unwrap();

    server.abort();
}

#[tokio::test]
async fn regression_explore_detail_url_js_injection_and_uppercase_quality() {
    use std::sync::atomic::{AtomicUsize, Ordering};

    use crate::domain::online::{MusicStreamRule, MusicTrackDetailRule};

    #[derive(Deserialize)]
    struct QueryCapture {
        q: Option<String>,
        p: Option<String>,
        l: Option<String>,
        calc: Option<String>,
        leak: Option<String>,
        x: Option<String>,
    }

    let seen_stream_q: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));
    let seen_stream_q_srv = seen_stream_q.clone();
    let seen_detail_x: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));
    let seen_detail_x_srv = seen_detail_x.clone();
    let seen_explore: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));
    let seen_explore_srv = seen_explore.clone();
    let seen_explore_srv2 = seen_explore.clone();
    let stream_hits = Arc::new(AtomicUsize::new(0));
    let stream_hits_srv = stream_hits.clone();

    let mock_app = Router::new()
        .route(
            "/cat-js",
            get(move |Query(q): Query<QueryCapture>| {
                let se = seen_explore_srv.clone();
                async move {
                    se.lock().unwrap().push(format!(
                        "cat:p={}:l={}:calc={}",
                        q.p.as_deref().unwrap_or(""),
                        q.l.as_deref().unwrap_or(""),
                        q.calc.as_deref().unwrap_or("")
                    ));
                    axum::Json(serde_json::json!([
                        { "id": "c1", "name": "Cat Track", "artist": "Cat Artist" }
                    ]))
                }
            }),
        )
        .route(
            "/custom-explore",
            get(move |Query(q): Query<QueryCapture>| {
                let se = seen_explore_srv2.clone();
                async move {
                    se.lock().unwrap().push(format!(
                        "custom:p={}:l={}:leak={}",
                        q.p.as_deref().unwrap_or(""),
                        q.l.as_deref().unwrap_or(""),
                        q.leak.as_deref().unwrap_or("")
                    ));
                    axum::Json(serde_json::json!([
                        { "id": "c2", "name": "Custom Track", "artist": "Custom Artist" }
                    ]))
                }
            }),
        )
        .route(
            "/detail-ok",
            get(move |Query(q): Query<QueryCapture>| {
                let sd = seen_detail_x_srv.clone();
                async move {
                    sd.lock().unwrap().push(q.x.unwrap_or_default());
                    axum::Json(serde_json::json!({
                        "id": "d1",
                        "name": "Detail Track",
                        "artist": "Detail Artist",
                        "lrc": "[00:01.00]Safe Lyric Line"
                    }))
                }
            }),
        )
        .route(
            "/stream-q",
            get(move |Query(q): Query<QueryCapture>| {
                let hits = stream_hits_srv.clone();
                let sq = seen_stream_q_srv.clone();
                async move {
                    hits.fetch_add(1, Ordering::SeqCst);
                    sq.lock().unwrap().push(q.q.unwrap_or_default());
                    axum::Json(serde_json::json!({
                        "url": "https://cdn.example.test/hq.mp3"
                    }))
                }
            }),
        );

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let server = tokio::spawn(async move {
        axum::serve(listener, mock_app).await.unwrap();
    });
    let base = format!("http://{addr}");

    let engine = MusicSourceEngine::new(
        HttpClientConfig {
            timeout_secs: 5,
            allow_private_ips: true,
            proxy_url: None,
        },
        Duration::from_secs(10),
    );

    let base_clone = base.clone();
    let seen_detail_x_check = seen_detail_x.clone();
    let seen_stream_q_check = seen_stream_q.clone();
    tokio::task::spawn_blocking(move || {
        let q_spec = MusicSourceSpec {
            source_name: "Uppercase Quality".into(),
            source_url: base_clone.clone(),
            supported_qualities: vec!["256".into()],
            rule_stream: Some(MusicStreamRule {
                request_url: Some(format!("{base_clone}/stream-q?q={{{{ QUALITY }}}}")),
                stream_url: Some("$.url".into()),
                format: Some("mp3".into()),
                bitrate: Some("256".into()),
                ..Default::default()
            }),
            ..Default::default()
        };
        let q_ctx = MusicTrackContext {
            track_id: "t1".into(),
            play_url: Some("https://cdn.example.test/low.mp3".into()),
            ..Default::default()
        };
        let q_out = engine
            .resolve_stream_sync(&q_spec, SourceRuntimeState::default(), &q_ctx, Some("256"))
            .unwrap();
        assert_eq!(stream_hits.load(Ordering::SeqCst), 1);
        assert_eq!(q_out.data.stream_url, "https://cdn.example.test/hq.mp3");
        assert_eq!(seen_stream_q_check.lock().unwrap().as_slice(), &["256"]);

        let detail_spec = MusicSourceSpec {
            source_name: "Detail Fallback Guard".into(),
            source_url: base_clone.clone(),
            rule_track_detail: Some(MusicTrackDetailRule {
                request_url: None,
                track_id: Some("$.id".into()),
                title: Some("$.name".into()),
                artist: Some("$.artist".into()),
                lyric: Some("$.lrc".into()),
                ..Default::default()
            }),
            ..Default::default()
        };
        let mut rt_secret = SourceRuntimeState {
            variable: "ADMIN_VAR".into(),
            ..Default::default()
        };
        rt_secret
            .login_info
            .insert("api_secret".into(), "TOP_SECRET".into());

        let js_ctx = MusicTrackContext {
            track_id: "t1".into(),
            detail_url: Some(format!(
                "@js:java.toast(source.getLoginInfo()); source.setVariable('PWN'); '{base_clone}/detail-ok'"
            )),
            ..Default::default()
        };
        let detail_js_err = engine
            .fetch_track_detail_sync(&detail_spec, rt_secret.clone(), &js_ctx)
            .unwrap_err();
        assert!(detail_js_err.diagnostics.toasts.is_empty());

        let lyric_js_err = engine
            .resolve_lyrics_sync(&detail_spec, rt_secret.clone(), &js_ctx)
            .unwrap_err();
        assert!(lyric_js_err.diagnostics.toasts.is_empty());

        let mustache_ctx = MusicTrackContext {
            track_id: "t1".into(),
            detail_url: Some(format!(
                "{base_clone}/detail-ok?x={{{{java.toast(source.getLoginInfo())}}}}"
            )),
            ..Default::default()
        };
        let detail_ok = engine
            .fetch_track_detail_sync(&detail_spec, rt_secret.clone(), &mustache_ctx)
            .unwrap();
        assert!(detail_ok.diagnostics.toasts.is_empty());
        assert_eq!(detail_ok.state.variable, "ADMIN_VAR");
        assert_eq!(detail_ok.data.title, "Detail Track");

        let lyric_ok = engine
            .resolve_lyrics_sync(&detail_spec, rt_secret, &mustache_ctx)
            .unwrap();
        assert!(lyric_ok.diagnostics.toasts.is_empty());
        assert_eq!(lyric_ok.state.variable, "ADMIN_VAR");
        assert_eq!(
            lyric_ok.data.unwrap().lyrics.lines[0].text,
            "Safe Lyric Line"
        );
        {
            let xs = seen_detail_x_check.lock().unwrap();
            assert_eq!(xs.len(), 2);
            assert_eq!(xs[0], "{{java.toast(source.getLoginInfo())}}");
        }
    })
    .await
    .unwrap();

    let h = setup_online_test_harness(true).await;
    let source_json = serde_json::json!({
        "sourceName": "Explore Guard Source",
        "sourceUrl": base,
        "enabled": true,
        "exploreUrl": format!("分类JS::{base}/cat-js?p={{{{page}}}}&l={{{{limit}}}}&calc={{{{1+1}}}}"),
        "loginUrl": "function login() { source.setVariable('ADMIN_SECRET_VAR'); return 'ok'; }",
        "ruleSearch": {
            "trackList": "$",
            "trackId": "$.id",
            "title": "$.name",
            "artist": "$.artist"
        },
        "ruleTrackDetail": {
            "trackId": "$.id",
            "title": "$.name",
            "artist": "$.artist",
            "lyric": "$.lrc"
        }
    });
    let (st, _, body) = send_json(
        &h.app,
        "POST",
        "/sources/import",
        &h.admin_token,
        Some(serde_json::json!({ "raw_json": source_json.to_string() })),
    )
    .await;
    assert_eq!(st, StatusCode::OK);
    let imp: serde_json::Value = serde_json::from_slice(&body).unwrap();
    let sid = imp["sources"][0]["id"].as_str().unwrap().to_string();

    let (st, _, _) = send_json(
        &h.app,
        "POST",
        &format!("/sources/{sid}/login-action"),
        &h.admin_token,
        Some(serde_json::json!({
            "login_info": { "api_secret": "TOP_SECRET_999" },
            "action": "login()"
        })),
    )
    .await;
    assert_eq!(st, StatusCode::OK);

    let (st, _, body) = send_json(
        &h.app,
        "POST",
        "/online/explore",
        &h.member_token,
        Some(serde_json::json!({
            "source_id": sid,
            "explore_url": format!("{base}/cat-js?p={{{{page}}}}&l={{{{limit}}}}&calc={{{{1+1}}}}"),
            "page": 2,
            "limit": 5
        })),
    )
    .await;
    assert_eq!(st, StatusCode::OK);
    let cat_resp: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(cat_resp["results"][0]["title"], "Cat Track");

    let (st, _, _) = send_json(
        &h.app,
        "POST",
        "/online/explore",
        &h.member_token,
        Some(serde_json::json!({
            "source_id": sid,
            "explore_url": format!("@js:java.toast(source.getLoginInfo()); source.setVariable('PWNED'); '{base}/custom-explore'")
        })),
    )
    .await;
    assert_eq!(st, StatusCode::BAD_REQUEST);

    let (st, _, body) = send_json(
        &h.app,
        "POST",
        "/online/explore",
        &h.member_token,
        Some(serde_json::json!({
            "source_id": sid,
            "explore_url": format!("{base}/custom-explore?p={{{{page}}}}&l={{{{limit}}}}&leak={{{{java.toast(source.getLoginInfo())}}}}"),
            "page": 3,
            "limit": 7
        })),
    )
    .await;
    assert_eq!(st, StatusCode::OK);
    let custom_resp: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(custom_resp["results"][0]["title"], "Custom Track");
    assert!(
        custom_resp["diagnostics"]["toasts"]
            .as_array()
            .unwrap()
            .is_empty()
    );

    let (st, _, body) = send_json(
        &h.app,
        "POST",
        "/online/track-info",
        &h.member_token,
        Some(serde_json::json!({
            "source_id": sid,
            "track_id": "d1",
            "detail_url": format!("{base}/detail-ok?x={{{{java.toast(source.getLoginInfo())}}}}")
        })),
    )
    .await;
    assert_eq!(st, StatusCode::OK);
    let ti_resp: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(ti_resp["track"]["title"], "Detail Track");
    assert!(
        ti_resp["diagnostics"]["toasts"]
            .as_array()
            .unwrap()
            .is_empty()
    );

    let (st, _, _) = send_json(
        &h.app,
        "POST",
        "/online/lyrics",
        &h.member_token,
        Some(serde_json::json!({
            "source_id": sid,
            "track_id": "d1",
            "detail_url": format!("@js:java.toast('PWN'); '{base}/detail-ok'")
        })),
    )
    .await;
    assert_eq!(st, StatusCode::BAD_REQUEST);

    let (st, _, body) = send_json(
        &h.app,
        "POST",
        "/online/lyrics",
        &h.member_token,
        Some(serde_json::json!({
            "source_id": sid,
            "track_id": "d1",
            "detail_url": format!("{base}/detail-ok?x={{{{java.toast('PWN')}}}}")
        })),
    )
    .await;
    assert_eq!(st, StatusCode::OK);
    let lrc_resp: StructuredLyrics = serde_json::from_slice(&body).unwrap();
    assert_eq!(lrc_resp.lines[0].text, "Safe Lyric Line");

    let (st, _, body) = send_json(
        &h.app,
        "GET",
        &format!("/sources/{sid}/variable"),
        &h.admin_token,
        None,
    )
    .await;
    assert_eq!(st, StatusCode::OK);
    let var_resp: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(var_resp["variable"], "ADMIN_SECRET_VAR");

    {
        let ex = seen_explore.lock().unwrap();
        assert_eq!(ex[0], "cat:p=2:l=5:calc=2");
        assert_eq!(
            ex[1],
            "custom:p=3:l=7:leak={{java.toast(source.getLoginInfo())}}"
        );
    }

    server.abort();
}

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use url::Url;
use uuid::Uuid;
use xmedia_source::http::{HttpClientConfig, LegadoHttpClient};

use crate::source::applecms::{
    infer_stream_type, parse_vod_play_groups, search as applecms_search,
};
use crate::source::hls::{
    infer_segment_content_type, is_hls_playlist_response, looks_like_html_bytes,
    rewrite_m3u8_playlist,
};
use crate::source::model::{VideoSourceFormat, VideoSourceSpec};

#[test]
fn parse_seeds_video_json_succeeds_with_all_six_verified_sources() {
    let raw = include_str!("../../../../seeds/sources/video.json");
    let batch = VideoSourceSpec::parse_json_batch(raw).expect("video.json must parse");
    assert_eq!(batch.len(), 6);

    assert_eq!(batch[0].0.source_name, "爱坤资源 (ikunzy)");
    assert_eq!(
        batch[0].0.effective_format(),
        VideoSourceFormat::ApplecmsV10
    );
    assert!(batch[0].0.searchable);
    assert_eq!(batch[0].0.preferred_play_from, vec!["ikm3u8"]);

    assert_eq!(batch[1].0.source_name, "魔都资源 (moduzy)");
    assert_eq!(batch[1].0.preferred_play_from, vec!["modum3u8"]);

    assert_eq!(batch[2].0.source_name, "索尼资源 (suoniapi)");
    assert!(!batch[2].0.searchable);

    assert_eq!(batch[3].0.source_name, "樱花资源 (apiyhzy)");
    assert!(!batch[3].0.searchable);

    assert_eq!(batch[4].0.source_name, "PeerTube (Framatube)");
    assert_eq!(batch[4].0.effective_format(), VideoSourceFormat::Rule);

    assert_eq!(batch[5].0.source_name, "Internet Archive Video");
    assert_eq!(batch[5].0.effective_format(), VideoSourceFormat::Rule);
}

#[test]
fn video_source_spec_normalizes_empty_source_url_from_api_url_and_auto_detects_format() {
    let raw = r#"{
        "sourceName": "NoSourceUrlCMS",
        "apiUrl": "https://cms.example.com/api.php/provide/vod/",
        "preferredPlayFrom": "ikm3u8, snm3u8"
    }"#;
    let batch = VideoSourceSpec::parse_json_batch(raw).unwrap();
    assert_eq!(batch.len(), 1);
    let (spec, clean_raw) = &batch[0];
    assert_eq!(
        spec.effective_source_url(),
        "https://cms.example.com/api.php/provide/vod/"
    );
    assert_eq!(spec.effective_format(), VideoSourceFormat::ApplecmsV10);
    assert_eq!(spec.preferred_play_from, vec!["ikm3u8", "snm3u8"]);
    let reparsed: serde_json::Value = serde_json::from_str(clean_raw).unwrap();
    assert_eq!(
        reparsed["sourceUrl"],
        "https://cms.example.com/api.php/provide/vod/"
    );
}

#[test]
fn applecms_parses_groups_filters_share_urls_and_sorts_preferred_play_from() {
    let groups = parse_vod_play_groups(
        Some("kuaikan$$$ikm3u8$$$other"),
        Some(
            "第1集$https://share.example.com/share/abc#第2集$https://share.example.com/share/def$$$第1集$https://cdn.example.com/1/index.m3u8#https://cdn.example.com/2/index.m3u8$$$HD$https://other.example.com/ep1.mp4?token=1",
        ),
        &["ikm3u8".into()],
        VideoSourceSpec::DEFAULT_PLAY_FILTER_REGEX,
    );

    assert_eq!(groups.len(), 2);
    assert_eq!(groups[0].name, "ikm3u8");
    assert_eq!(groups[0].episodes.len(), 2);
    assert_eq!(groups[0].episodes[0].name, "第1集");
    assert_eq!(
        groups[0].episodes[0].play_url,
        "https://cdn.example.com/1/index.m3u8"
    );
    assert_eq!(groups[0].episodes[1].name, "第2集");
    assert_eq!(
        groups[0].episodes[1].play_url,
        "https://cdn.example.com/2/index.m3u8"
    );

    assert_eq!(groups[1].name, "other");
    assert_eq!(groups[1].episodes.len(), 1);
    assert_eq!(groups[1].episodes[0].name, "HD");
}

#[test]
fn applecms_falls_back_to_all_http_episodes_when_filter_regex_matches_none() {
    let groups = parse_vod_play_groups(
        Some("stream"),
        Some("播放$https://cdn.example.com/live/channel1"),
        &[],
        VideoSourceSpec::DEFAULT_PLAY_FILTER_REGEX,
    );
    assert_eq!(groups.len(), 1);
    assert_eq!(groups[0].episodes.len(), 1);
    assert_eq!(
        groups[0].episodes[0].play_url,
        "https://cdn.example.com/live/channel1"
    );

    let share_only = parse_vod_play_groups(
        Some("shareonly"),
        Some("正片$http://127.0.0.1:9/share/2001"),
        &[],
        VideoSourceSpec::DEFAULT_PLAY_FILTER_REGEX,
    );
    assert!(
        share_only.is_empty(),
        "obvious /share/ HTML page must be excluded"
    );
}

#[tokio::test]
async fn applecms_detail_strips_html_tags_and_decodes_entities_in_intro() {
    use axum::Router;
    use axum::body::Body;
    use axum::http::StatusCode;
    use axum::response::Response;
    use axum::routing::get;

    let origin_app = Router::new().route(
        "/api.php/provide/vod/",
        get(|| async {
            Response::builder()
                .status(StatusCode::OK)
                .header("content-type", "application/json")
                .body(Body::from(
                    serde_json::json!({
                        "code": 1,
                        "list": [{
                            "vod_id": 42,
                            "vod_name": "Test Video",
                            "vod_content": "<p>Sci-Fi &amp; Action</p><br/><p>Line&nbsp;2 &lt;HD&gt;</p>",
                            "vod_play_from": "ikm3u8",
                            "vod_play_url": "正片$https://cdn.example.com/42/index.m3u8"
                        }]
                    })
                    .to_string(),
                ))
                .unwrap()
        }),
    );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let srv = tokio::spawn(async move {
        axum::serve(listener, origin_app).await.unwrap();
    });
    let origin = format!("http://{addr}");

    let spec: VideoSourceSpec = serde_json::from_value(serde_json::json!({
        "sourceName": "IntroHtmlStrip",
        "sourceUrl": format!("{origin}/api.php/provide/vod/"),
        "sourceFormat": "applecms_v10",
        "apiUrl": format!("{origin}/api.php/provide/vod/")
    }))
    .unwrap();
    let detail = tokio::task::spawn_blocking(move || {
        let cookies = Arc::new(Mutex::new(HashMap::new()));
        let http = LegadoHttpClient::new(
            HttpClientConfig {
                allow_private_ips: true,
                ..Default::default()
            },
            cookies,
            None,
            false,
        )
        .unwrap();
        crate::source::applecms::fetch_detail(&spec, &http, Some("42"), None)
    })
    .await
    .unwrap()
    .unwrap();

    assert_eq!(
        detail.intro.as_deref(),
        Some("Sci-Fi & Action\nLine 2 <HD>")
    );
    srv.abort();
}

#[test]
fn rewrite_m3u8_playlist_rewrites_all_five_directives_and_segment_lines() {
    let sid = Uuid::from_u128(42);
    let base_url = Url::parse("https://cdn.example.com/hls/v2/master.m3u8").unwrap();
    let playlist = "\u{feff}#EXTM3U\n\
        #EXT-X-VERSION:7\n\
        #EXT-X-SESSION-KEY:METHOD=AES-128,URI=\"keys/session.key\"\n\
        #EXT-X-MEDIA:TYPE=SUBTITLES,GROUP-ID=\"subs\",NAME=\"Chinese\",URI=\"subs/zh.m3u8\"\n\
        #EXT-X-I-FRAME-STREAM-INF:BANDWIDTH=80000,URI=\"iframe/720.m3u8\"\n\
        #EXT-X-STREAM-INF:BANDWIDTH=2000000,SUBTITLES=\"subs\"\n\
        720p/media.m3u8\n\
        #EXT-X-KEY:METHOD=AES-128,URI=\"https://keys.example.net/ep1.key\",IV=0x01\n\
        #EXT-X-KEY:METHOD=SAMPLE-AES,URI=\"skd://fairplay-key-id\"\n\
        #EXT-X-MAP:URI=\"720p/init.mp4\",BYTERANGE=\"1334@0\"\n\
        #EXT-X-PART:DURATION=0.5,URI=\"720p/part1.m4s\"\n\
        #EXT-X-BYTERANGE:108238@1334\n\
        720p/seg0.m4s\n\
        https://alt.cdn.example.com:8443/seg1.ts?auth=ok\n";

    let out_no_tok = rewrite_m3u8_playlist(playlist, &base_url, Some(sid), None);
    assert!(!out_no_tok.contains("token="));
    assert!(out_no_tok.contains(
        "#EXT-X-SESSION-KEY:METHOD=AES-128,URI=\"/api/v1/video/online/hls-proxy?url=https%3A%2F%2Fcdn.example.com%2Fhls%2Fv2%2Fkeys%2Fsession.key&source_id="
    ));
    assert!(out_no_tok.contains(
        "#EXT-X-MEDIA:TYPE=SUBTITLES,GROUP-ID=\"subs\",NAME=\"Chinese\",URI=\"/api/v1/video/online/hls-proxy?url=https%3A%2F%2Fcdn.example.com%2Fhls%2Fv2%2Fsubs%2Fzh.m3u8&source_id="
    ));
    assert!(out_no_tok.contains(
        "#EXT-X-I-FRAME-STREAM-INF:BANDWIDTH=80000,URI=\"/api/v1/video/online/hls-proxy?url=https%3A%2F%2Fcdn.example.com%2Fhls%2Fv2%2Fiframe%2F720.m3u8&source_id="
    ));
    assert!(out_no_tok.contains(
        "#EXT-X-KEY:METHOD=AES-128,URI=\"/api/v1/video/online/hls-proxy?url=https%3A%2F%2Fkeys.example.net%2Fep1.key&source_id="
    ));
    assert!(out_no_tok.contains("URI=\"skd://fairplay-key-id\""));
    assert!(out_no_tok.contains(
        "#EXT-X-MAP:URI=\"/api/v1/video/online/hls-proxy?url=https%3A%2F%2Fcdn.example.com%2Fhls%2Fv2%2F720p%2Finit.mp4&source_id="
    ));
    assert!(out_no_tok.contains("BYTERANGE=\"1334@0\""));
    assert!(out_no_tok.contains(
        "#EXT-X-PART:DURATION=0.5,URI=\"/api/v1/video/online/hls-proxy?url=https%3A%2F%2Fcdn.example.com%2Fhls%2Fv2%2F720p%2Fpart1.m4s&source_id="
    ));
    assert!(out_no_tok.contains("#EXT-X-BYTERANGE:108238@1334\n"));
    assert!(out_no_tok.contains(
        "/api/v1/video/online/hls-proxy?url=https%3A%2F%2Fcdn.example.com%2Fhls%2Fv2%2F720p%2Fmedia.m3u8&source_id="
    ));
    assert!(out_no_tok.contains(
        "/api/v1/video/online/hls-proxy?url=https%3A%2F%2Fcdn.example.com%2Fhls%2Fv2%2F720p%2Fseg0.m4s&source_id="
    ));
    assert!(out_no_tok.contains(
        "/api/v1/video/online/hls-proxy?url=https%3A%2F%2Falt.cdn.example.com%3A8443%2Fseg1.ts%3Fauth%3Dok&source_id="
    ));

    let out_with_tok = rewrite_m3u8_playlist(playlist, &base_url, Some(sid), Some("jwt+123"));
    assert!(out_with_tok.contains("&token=jwt%2B123"));
}

#[test]
fn hls_detection_and_segment_mime_inference_avoid_text_and_detect_bom_extm3u() {
    let url_no_ext = Url::parse("https://cdn.example.com/stream/playlist").unwrap();
    assert!(is_hls_playlist_response(
        "text/plain",
        &url_no_ext,
        b"\xEF\xBB\xBF  #EXTM3U\n#EXT-X-TARGETDURATION:4\n"
    ));
    assert!(!is_hls_playlist_response(
        "application/vnd.apple.mpegurl",
        &url_no_ext,
        b"<!DOCTYPE html><html>blocked</html>"
    ));
    assert!(looks_like_html_bytes(b"  <HTML><body>err</body></html>"));
    assert!(!looks_like_html_bytes(&[0x47, 0x40, 0x00, 0x10, 0x00]));

    assert_eq!(
        infer_segment_content_type(
            Some("text/plain"),
            &Url::parse("https://cdn.example.com/seg0.ts").unwrap()
        ),
        "video/mp2t"
    );
    assert_eq!(
        infer_segment_content_type(
            None,
            &Url::parse("https://cdn.example.com/init.m4s").unwrap()
        ),
        "video/mp4"
    );
    assert_eq!(
        infer_segment_content_type(
            None,
            &Url::parse("https://cdn.example.com/sub.vtt").unwrap()
        ),
        "text/vtt; charset=utf-8"
    );
    assert_eq!(
        infer_segment_content_type(
            None,
            &Url::parse("https://cdn.example.com/enc.key").unwrap()
        ),
        "application/octet-stream"
    );
    assert_eq!(
        infer_stream_type("https://cdn.example.com/live/index.m3u8?a=1"),
        "hls"
    );
    assert_eq!(
        infer_stream_type("https://cdn.example.com/movie/play.mp4?t=2"),
        "mp4"
    );
}

#[tokio::test]
async fn applecms_guards_against_plain_text_and_403_search_errors() {
    use axum::Router;
    use axum::http::StatusCode;
    use axum::routing::get;

    let app = Router::new()
        .route(
            "/text/api.php/provide/vod/",
            get(|| async { (StatusCode::OK, "暂不支持搜索") }),
        )
        .route(
            "/forbidden/api.php/provide/vod/",
            get(|| async { (StatusCode::FORBIDDEN, "403 Forbidden") }),
        );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let srv = tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });

    let spec_txt = VideoSourceSpec {
        source_name: "TextErr".into(),
        source_url: format!("http://{addr}/text/api.php/provide/vod/"),
        api_url: Some(format!("http://{addr}/text/api.php/provide/vod/")),
        searchable: true,
        ..Default::default()
    };
    let spec_403 = VideoSourceSpec {
        source_name: "403Err".into(),
        source_url: format!("http://{addr}/forbidden/api.php/provide/vod/"),
        api_url: Some(format!("http://{addr}/forbidden/api.php/provide/vod/")),
        searchable: true,
        ..Default::default()
    };

    let res_txt = tokio::task::spawn_blocking(move || {
        let cfg = HttpClientConfig {
            allow_private_ips: true,
            ..Default::default()
        };
        let cookies = Arc::new(Mutex::new(HashMap::new()));
        let http = LegadoHttpClient::new(cfg, cookies, None, false).unwrap();
        let e1 = applecms_search(&spec_txt, &http, "test", 1).unwrap_err();
        let e2 = applecms_search(&spec_403, &http, "test", 1).unwrap_err();
        (e1, e2)
    })
    .await
    .unwrap();

    assert!(res_txt.0.contains("非 JSON 响应"));
    assert!(res_txt.1.contains("403"));
    srv.abort();
}

#[test]
fn rewrite_m3u8_playlist_strips_leading_bom_and_whitespace_before_extm3u() {
    let base_url = Url::parse("https://cdn.example.com/hls/master.m3u8").unwrap();
    let playlist = "\u{feff}  \r\n\t#EXTM3U\n#EXTINF:5.0,\nseg0.ts\n#EXT-X-ENDLIST\n";
    let rewritten = rewrite_m3u8_playlist(playlist, &base_url, None, None);
    assert!(
        rewritten.starts_with("#EXTM3U\n"),
        "expected line 1 to start with #EXTM3U, got: {rewritten:?}"
    );
    assert!(rewritten.contains(
        "/api/v1/video/online/hls-proxy?url=https%3A%2F%2Fcdn.example.com%2Fhls%2Fseg0.ts"
    ));
}

#[test]
fn infer_segment_content_type_normalizes_case_and_generic_binary_for_compression_safety() {
    assert_eq!(
        infer_segment_content_type(
            Some("Video/MP4"),
            &Url::parse("https://cdn.example.com/media/stream").unwrap()
        ),
        "video/mp4"
    );
    assert_eq!(
        infer_segment_content_type(
            Some("  VIDEO/MP2T ; codecs=\"mp2t\" "),
            &Url::parse("https://cdn.example.com/seg").unwrap()
        ),
        "video/mp2t; codecs=\"mp2t\""
    );
    assert_eq!(
        infer_segment_content_type(
            Some("Audio/AAC"),
            &Url::parse("https://cdn.example.com/audio").unwrap()
        ),
        "audio/aac"
    );
    assert_eq!(
        infer_segment_content_type(
            Some("binary/octet-stream"),
            &Url::parse("https://cdn.example.com/seg0.ts").unwrap()
        ),
        "video/mp2t"
    );
    assert_eq!(
        infer_segment_content_type(
            Some("binary/octet-stream"),
            &Url::parse("https://cdn.example.com/blob").unwrap()
        ),
        "application/octet-stream"
    );
    assert_eq!(
        infer_segment_content_type(
            Some("application/force-download"),
            &Url::parse("https://cdn.example.com/movie.mp4").unwrap()
        ),
        "video/mp4"
    );
    assert_eq!(
        infer_segment_content_type(
            Some("application/x-binary"),
            &Url::parse("https://cdn.example.com/init.m4s").unwrap()
        ),
        "video/mp4"
    );
    assert_eq!(
        infer_segment_content_type(
            Some("application/octet-stream"),
            &Url::parse("https://cdn.example.com/seg0.ts").unwrap()
        ),
        "video/mp2t"
    );
    assert_eq!(
        infer_segment_content_type(
            Some("application/octet-stream"),
            &Url::parse("https://cdn.example.com/init.m4s").unwrap()
        ),
        "video/mp4"
    );
    assert_eq!(
        infer_segment_content_type(
            Some("application/octet-stream"),
            &Url::parse("https://cdn.example.com/movie.mp4").unwrap()
        ),
        "video/mp4"
    );
    assert_eq!(
        infer_segment_content_type(
            Some("application/octet-stream"),
            &Url::parse("https://cdn.example.com/audio.aac").unwrap()
        ),
        "audio/aac"
    );
    assert_eq!(
        infer_segment_content_type(
            Some("application/octet-stream"),
            &Url::parse("https://cdn.example.com/enc.key").unwrap()
        ),
        "application/octet-stream"
    );
    assert_eq!(
        infer_segment_content_type(
            Some("application/mp4"),
            &Url::parse("https://cdn.example.com/video").unwrap()
        ),
        "video/mp4"
    );
}

#[tokio::test]
async fn rule_play_subtitles_evaluates_jsonpath_js_and_edge_cases() {
    use std::sync::atomic::AtomicBool;
    use std::time::Duration;

    use axum::Router;
    use axum::http::StatusCode;
    use axum::routing::get;

    use crate::source::engine::VideoSourceEngine;
    use crate::source::model::SourceRuntimeState;

    let app = Router::new()
        .route(
            "/play/one",
            get(|| async {
                (
                    StatusCode::OK,
                    r#"{"stream":"/cdn/one.mp4","subs":[{"label":"中文","lang":"zh","url":"/subs/zh.vtt"}]}"#,
                )
            }),
        )
        .route(
            "/play/multi",
            get(|| async {
                (
                    StatusCode::OK,
                    r#"{"stream":"/cdn/multi.mp4","subs":[{"label":"中文","lang":"zh","url":"/subs/zh.vtt"},{"url":"https://sub.example.com/en.srt"}]}"#,
                )
            }),
        )
        .route(
            "/play/single-obj",
            get(|| async {
                (
                    StatusCode::OK,
                    r#"{"stream":"/cdn/obj.mp4","sub":{"label":"日语","lang":"ja","url":"/subs/ja.ass"}}"#,
                )
            }),
        )
        .route(
            "/play/url-list",
            get(|| async {
                (
                    StatusCode::OK,
                    r#"{"stream":"/cdn/urls.mp4","subs":["/subs/a.vtt","https://sub.example.com/b.srt"]}"#,
                )
            }),
        )
        .route(
            "/play/empty",
            get(|| async { (StatusCode::OK, r#"{"stream":"/cdn/empty.mp4","subs":[]}"#) }),
        )
        .route(
            "/play/null",
            get(|| async { (StatusCode::OK, r#"{"stream":"/cdn/null.mp4","subs":null}"#) }),
        );

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let origin = format!("http://{addr}");
    let srv = tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });

    let origin_clone = origin.clone();
    tokio::task::spawn_blocking(move || {
        let engine = VideoSourceEngine::new(
            HttpClientConfig {
                allow_private_ips: true,
                ..Default::default()
            },
            Duration::from_secs(10),
        );

        let make_spec = |sub_rule: &str| -> VideoSourceSpec {
            let raw = serde_json::json!({
                "sourceName": "SubTest",
                "sourceUrl": origin_clone,
                "sourceFormat": "rule",
                "rulePlay": {
                    "requestUrl": "{{playUrl}}",
                    "streamUrl": "$.stream",
                    "subtitles": sub_rule
                }
            });
            VideoSourceSpec::parse_json_batch(&raw.to_string())
                .unwrap()
                .remove(0)
                .0
        };

        let resolve = |sub_rule: &str, path: &str| {
            let spec = make_spec(sub_rule);
            engine
                .resolve_play_sync_with_control(
                    &spec,
                    SourceRuntimeState::default(),
                    path,
                    None,
                    Duration::from_secs(10),
                    Arc::new(AtomicBool::new(false)),
                )
                .unwrap()
                .data
        };

        // 1. JSONPath `$.subs` with 1 item
        let r1 = resolve("$.subs", "/play/one");
        assert_eq!(r1.subtitles.len(), 1);
        assert_eq!(r1.subtitles[0].label, "中文");
        assert_eq!(r1.subtitles[0].lang.as_deref(), Some("zh"));
        assert_eq!(r1.subtitles[0].url, format!("{origin_clone}/subs/zh.vtt"));

        // 2. JSONPath `$.subs` with 2 items
        let r2 = resolve("$.subs", "/play/multi");
        assert_eq!(r2.subtitles.len(), 2);
        assert_eq!(r2.subtitles[0].label, "中文");
        assert_eq!(r2.subtitles[1].label, "字幕 2");
        assert_eq!(r2.subtitles[1].lang, None);
        assert_eq!(r2.subtitles[1].url, "https://sub.example.com/en.srt");

        // 3. JSONPath wildcard `$.subs[*]`
        let r3 = resolve("$.subs[*]", "/play/multi");
        assert_eq!(r3.subtitles.len(), 2);
        assert_eq!(r3.subtitles[0].url, format!("{origin_clone}/subs/zh.vtt"));
        assert_eq!(r3.subtitles[1].url, "https://sub.example.com/en.srt");

        // 4. Single JSON object `$.sub`
        let r4 = resolve("$.sub", "/play/single-obj");
        assert_eq!(r4.subtitles.len(), 1);
        assert_eq!(r4.subtitles[0].label, "日语");
        assert_eq!(r4.subtitles[0].lang.as_deref(), Some("ja"));
        assert_eq!(r4.subtitles[0].url, format!("{origin_clone}/subs/ja.ass"));

        // 5. Array of URL strings
        let r5 = resolve("$.subs", "/play/url-list");
        assert_eq!(r5.subtitles.len(), 2);
        assert_eq!(r5.subtitles[0].label, "字幕 1");
        assert_eq!(r5.subtitles[0].url, format!("{origin_clone}/subs/a.vtt"));
        assert_eq!(r5.subtitles[1].url, "https://sub.example.com/b.srt");

        // 6. `<js>JSON.stringify(result.subs)</js>` and `<js>result.subs</js>`
        let r6 = resolve("<js>JSON.stringify(result.subs)</js>", "/play/multi");
        assert_eq!(r6.subtitles.len(), 2);
        let r7 = resolve("<js>result.subs</js>", "/play/multi");
        assert_eq!(r7.subtitles.len(), 2);

        // 7. Empty / null / missing
        assert!(resolve("$.subs", "/play/empty").subtitles.is_empty());
        assert!(resolve("$.subs", "/play/null").subtitles.is_empty());
        assert!(resolve("$.missing", "/play/one").subtitles.is_empty());
        assert!(resolve("<js>null</js>", "/play/one").subtitles.is_empty());
    })
    .await
    .unwrap();

    srv.abort();
}

#[test]
fn video_search_url_template_encodes_keyword_and_isolates_legado_options_json_injection() {
    use std::io::{Read, Write};
    use std::net::TcpListener;
    use std::sync::{Arc, Mutex};
    use std::time::Duration;

    use crate::source::engine::VideoSourceEngine;
    use crate::source::model::SourceRuntimeState;

    #[derive(Default)]
    struct Captured {
        targets: Vec<String>,
        bodies: Vec<String>,
        has_injected_header: bool,
    }

    let captured = Arc::new(Mutex::new(Captured::default()));
    let cap_clone = captured.clone();
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();

    let server = std::thread::spawn(move || {
        for _ in 0..2 {
            let (mut stream, _) = listener.accept().unwrap();
            let mut buf = [0u8; 8192];
            let n = stream.read(&mut buf).unwrap();
            let req_str = String::from_utf8_lossy(&buf[..n]).to_string();
            let (head, body) = req_str.split_once("\r\n\r\n").unwrap_or((&req_str, ""));
            let first_line = head.lines().next().unwrap_or("");
            let target = first_line
                .split_whitespace()
                .nth(1)
                .unwrap_or("")
                .to_string();
            let has_injected = head
                .lines()
                .any(|l| l.to_ascii_lowercase().starts_with("x-injected:"));
            {
                let mut g = cap_clone.lock().unwrap();
                g.targets.push(target);
                g.bodies.push(body.to_string());
                if has_injected {
                    g.has_injected_header = true;
                }
            }
            let payload = r#"{"items":[]}"#;
            let resp = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                payload.len(),
                payload
            );
            stream.write_all(resp.as_bytes()).unwrap();
        }
    });

    let cfg = HttpClientConfig {
        allow_private_ips: true,
        ..Default::default()
    };
    let engine = VideoSourceEngine::new(cfg, Duration::from_secs(2));
    let src: VideoSourceSpec = serde_json::from_value(serde_json::json!({
        "sourceName": "InjectVideoTest",
        "sourceUrl": format!("http://{addr}"),
        "sourceFormat": "rule",
        "searchUrl": "/search?q={{key}}&pg={{page}},{\"method\":\"POST\",\"body\":\"wd={{key}}&pg={{page}}\"}",
        "ruleSearch": {
            "list": "$.items[*]",
            "title": "$.title",
            "videoId": "$.id"
        }
    }))
    .unwrap();

    engine
        .search_sync_with_control(
            &src,
            SourceRuntimeState::default(),
            "hello world&injected=1",
            1,
            Duration::from_secs(5),
            Arc::new(std::sync::atomic::AtomicBool::new(false)),
        )
        .unwrap();

    let poison = "x\",\"headers\":{\"X-Injected\":\"pwned\"},\"z\":\"";
    engine
        .search_sync_with_control(
            &src,
            SourceRuntimeState::default(),
            poison,
            2,
            Duration::from_secs(5),
            Arc::new(std::sync::atomic::AtomicBool::new(false)),
        )
        .unwrap();

    server.join().unwrap();
    let cap = captured.lock().unwrap();
    assert!(!cap.has_injected_header, "X-Injected header leaked!");
    assert_eq!(
        cap.targets[0],
        "/search?q=hello%20world%26injected%3D1&pg=1"
    );
    assert_eq!(cap.bodies[0], "wd=hello%20world%26injected%3D1&pg=1");
    assert!(cap.targets[1].contains("q=x%22"));
    assert!(!cap.targets[1].contains("X-Injected\":\"pwned"));
}

#[test]
fn video_search_url_js_options_block_duplicate_key_and_support_unquoted_and_mixed_js_urls() {
    use std::io::{Read, Write};
    use std::net::TcpListener;
    use std::sync::{Arc, Mutex};
    use std::time::Duration;

    use crate::source::engine::VideoSourceEngine;
    use crate::source::model::SourceRuntimeState;

    #[derive(Default)]
    struct Captured {
        methods: Vec<String>,
        targets: Vec<String>,
        bodies: Vec<String>,
        signs: Vec<String>,
    }

    let captured = Arc::new(Mutex::new(Captured::default()));
    let cap_clone = captured.clone();
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();

    let server = std::thread::spawn(move || {
        for _ in 0..4 {
            let (mut stream, _) = listener.accept().unwrap();
            let mut buf = [0u8; 8192];
            let n = stream.read(&mut buf).unwrap();
            let req_str = String::from_utf8_lossy(&buf[..n]).to_string();
            let (head, body) = req_str.split_once("\r\n\r\n").unwrap_or((&req_str, ""));
            let first_line = head.lines().next().unwrap_or("");
            let mut parts = first_line.split_whitespace();
            let method = parts.next().unwrap_or("").to_string();
            let target = parts.next().unwrap_or("").to_string();
            let sign = head
                .lines()
                .find_map(|l| {
                    let (k, v) = l.split_once(':')?;
                    k.trim()
                        .eq_ignore_ascii_case("x-sign")
                        .then(|| v.trim().to_string())
                })
                .unwrap_or_default();
            {
                let mut g = cap_clone.lock().unwrap();
                g.methods.push(method);
                g.targets.push(target);
                g.bodies.push(body.to_string());
                g.signs.push(sign);
            }
            let payload = r#"{"items":[]}"#;
            let resp = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                payload.len(),
                payload
            );
            stream.write_all(resp.as_bytes()).unwrap();
        }
    });

    let cfg = HttpClientConfig {
        allow_private_ips: true,
        ..Default::default()
    };
    let engine = VideoSourceEngine::new(cfg, Duration::from_secs(2));
    let dup_key_payload = "x\",\"method\":\"DELETE\",\"charset\":\"";

    let make_src = |search_url: &str| -> VideoSourceSpec {
        serde_json::from_value(serde_json::json!({
            "sourceName": "JsOptsVideoTest",
            "sourceUrl": format!("http://{addr}"),
            "sourceFormat": "rule",
            "jsLib": "function sign(k){ return 'sig:' + k.length; } function flag(){ return true; }",
            "searchUrl": search_url,
            "ruleSearch": {
                "list": "$.items[*]",
                "title": "$.title",
                "videoId": "$.id"
            }
        }))
        .unwrap()
    };

    // 1. JS-enabled LegadoUrlOptions + {{key}} + trailing key + mixed JS URL part
    let src_js_opts = make_src(
        "/s?wd={{key}}&t={{100 + 23}},{\"method\":\"POST\",\"headers\":{\"X-Sign\":\"{{sign(key)}}\"},\"body\":\"q={{key}}\",\"charset\":\"UTF-8\"}",
    );
    engine
        .search_sync_with_control(
            &src_js_opts,
            SourceRuntimeState::default(),
            dup_key_payload,
            1,
            Duration::from_secs(5),
            Arc::new(std::sync::atomic::AtomicBool::new(false)),
        )
        .unwrap();

    // 2. Unquoted {{...}} placeholders inside JS-enabled LegadoUrlOptions
    let src_unquoted_opts = make_src(
        "/unq,{\"method\":\"POST\",\"webView\":{{flag()}},\"body\":{\"page\":{{page}},\"kw\":\"{{key}}\"}}",
    );
    engine
        .search_sync_with_control(
            &src_unquoted_opts,
            SourceRuntimeState::default(),
            dup_key_payload,
            3,
            Duration::from_secs(5),
            Arc::new(std::sync::atomic::AtomicBool::new(false)),
        )
        .unwrap();

    // 3. Mixed {{key}} + JS {{...}} without options percent-encodes {{key}}
    let src_mixed_url = make_src("/mixed?wd={{key}}&t={{40 + 2}}");
    engine
        .search_sync_with_control(
            &src_mixed_url,
            SourceRuntimeState::default(),
            "a&admin=1",
            1,
            Duration::from_secs(5),
            Arc::new(std::sync::atomic::AtomicBool::new(false)),
        )
        .unwrap();

    // 4. Case-insensitive <JS> pipeline still permitted to build options
    let src_ci_js = make_src("<JS>'/ci,' + JSON.stringify({method:'POST',body:'ok'})</JS>");
    engine
        .search_sync_with_control(
            &src_ci_js,
            SourceRuntimeState::default(),
            "q",
            1,
            Duration::from_secs(5),
            Arc::new(std::sync::atomic::AtomicBool::new(false)),
        )
        .unwrap();

    // 5. Options smuggling via {{...}} when no @js:/<js> is present must be rejected
    let src_smuggle_mustache =
        make_src("/bad?q={{key}}&x={{\",{\\\"method\\\":\\\"DELETE\\\"}\"}}");
    assert!(
        engine
            .search_sync_with_control(
                &src_smuggle_mustache,
                SourceRuntimeState::default(),
                "q",
                1,
                Duration::from_secs(5),
                Arc::new(std::sync::atomic::AtomicBool::new(false)),
            )
            .is_err()
    );

    server.join().unwrap();
    let cap = captured.lock().unwrap();
    assert_eq!(cap.methods, vec!["POST", "POST", "GET", "POST"]);
    assert!(cap.targets[0].starts_with("/s?wd=x%22") && cap.targets[0].ends_with("&t=123"));
    assert_eq!(
        cap.bodies[0],
        format!(
            "q={}",
            xmedia_source::http::url_encode_component(dup_key_payload)
        )
    );
    assert_eq!(cap.signs[0], format!("sig:{}", dup_key_payload.len()));

    let unq_body: serde_json::Value = serde_json::from_str(&cap.bodies[1]).unwrap();
    assert_eq!(unq_body["page"], 3);
    assert_eq!(unq_body["kw"].as_str(), Some(dup_key_payload));

    assert_eq!(cap.targets[2], "/mixed?wd=a%26admin%3D1&t=42");
    assert_eq!(cap.targets[3], "/ci");
    assert_eq!(cap.bodies[3], "ok");
}

#[test]
fn video_run_login_action_strips_at_js_and_tag_js_wrappers() {
    let engine = crate::source::VideoSourceEngine::default();
    let src_at: VideoSourceSpec = serde_json::from_value(serde_json::json!({
        "sourceName": "AtJsLogin",
        "sourceUrl": "https://video.example.test",
        "loginUrl": "@js:function login() { return 'at:' + (source.getLoginInfoMap().user || ''); }"
    }))
    .unwrap();
    let out_at = engine
        .run_login_action_sync(
            &src_at,
            crate::source::model::SourceRuntimeState::default(),
            HashMap::from([("user".to_string(), "alice".to_string())]),
            Some("login()"),
        )
        .unwrap();
    assert_eq!(out_at.data, "at:alice");

    let src_tag: VideoSourceSpec = serde_json::from_value(serde_json::json!({
        "sourceName": "TagJsLogin",
        "sourceUrl": "https://video.example.test",
        "loginUrl": "<js>function check() { return 'tag:' + (source.getLoginInfoMap().user || ''); }</js>",
        "loginCheckJs": "@js:check()"
    }))
    .unwrap();
    let out_tag = engine
        .run_login_action_sync(
            &src_tag,
            crate::source::model::SourceRuntimeState::default(),
            HashMap::from([("user".to_string(), "bob".to_string())]),
            None,
        )
        .unwrap();
    assert_eq!(out_tag.data, "tag:bob");
}

#[test]
fn subtitle_proxy_normalizer_passes_vtt_converts_srt_and_rejects_xss_or_oversized() {
    use crate::source::hls::{MAX_SUBTITLE_BYTES, validate_and_normalize_subtitle_to_vtt};

    let vtt = b"\xEF\xBB\xBFWEBVTT\r\n\r\n00:00:01.000 --> 00:00:03.000\r\nHello, world!\r\n";
    let vtt_out = validate_and_normalize_subtitle_to_vtt(vtt, Some("text/vtt")).unwrap();
    assert_eq!(
        vtt_out,
        "WEBVTT\n\n00:00:01.000 --> 00:00:03.000\nHello, world!\n"
    );

    let srt = b"1\r\n00:00:01,250 --> 00:00:04,500 align:start\r\nHello, SRT!\r\n\r\n2\r\n00:05,000 --> 00:08,100\r\nSecond\r\n";
    let srt_out =
        validate_and_normalize_subtitle_to_vtt(srt, Some("application/x-subrip")).unwrap();
    assert!(srt_out.starts_with("WEBVTT\n\n"));
    assert!(srt_out.contains("00:00:01.250 --> 00:00:04.500 align:start\nHello, SRT!"));
    assert!(srt_out.contains("00:05.000 --> 00:08.100\nSecond"));

    // Reject HTML/SVG/XML/script, invalid UTF-8, non-subtitle text, and >4 MiB payloads
    assert!(
        validate_and_normalize_subtitle_to_vtt(b"<html><body>xss</body></html>", None).is_err()
    );
    assert!(
        validate_and_normalize_subtitle_to_vtt(
            b"WEBVTT\n\n00:00:01.000 --> 00:00:02.000\n<script>alert(1)</script>",
            None
        )
        .is_err()
    );
    assert!(validate_and_normalize_subtitle_to_vtt(b"WEBVTT", Some("text/html")).is_err());
    assert!(validate_and_normalize_subtitle_to_vtt(b"WEBVTT", Some("application/xml")).is_err());
    assert!(validate_and_normalize_subtitle_to_vtt(b"plain text without cues", None).is_err());
    assert!(validate_and_normalize_subtitle_to_vtt(&[0xFF, 0xFE, b'W', 0x00], None).is_err());
    let oversized = vec![b'a'; MAX_SUBTITLE_BYTES + 1];
    assert!(validate_and_normalize_subtitle_to_vtt(&oversized, None).is_err());
}

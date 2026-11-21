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

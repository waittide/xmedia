use md5::{Digest, Md5};
use url::Url;

use super::parser::{build_subsonic_url, encode_segment, resolve_audio_filename};

#[test]
fn subsonic_url_builder_generates_valid_md5_token_and_salt() {
    let built = build_subsonic_url(
        "http://127.0.0.1:4533",
        "ping.view",
        "alice",
        "secret123",
        &[("size", "20".into())],
    )
    .unwrap();

    let parsed = Url::parse(&built).unwrap();
    assert_eq!(parsed.path(), "/rest/ping.view");
    let params: std::collections::HashMap<String, String> =
        parsed.query_pairs().into_owned().collect();
    assert_eq!(params.get("u").map(String::as_str), Some("alice"));
    assert_eq!(params.get("v").map(String::as_str), Some("1.16.1"));
    assert_eq!(params.get("c").map(String::as_str), Some("XMedia"));
    assert_eq!(params.get("f").map(String::as_str), Some("json"));
    assert_eq!(params.get("size").map(String::as_str), Some("20"));

    let salt = params.get("s").unwrap();
    let token = params.get("t").unwrap();
    let mut md5 = Md5::new();
    md5.update(b"secret123");
    md5.update(salt.as_bytes());
    assert_eq!(token, &hex::encode(md5.finalize()));
}

#[test]
fn subsonic_url_builder_handles_existing_query_rest_suffix_and_api_key() {
    let built = build_subsonic_url(
        "http://127.0.0.1:4533/music/rest/?foo=bar",
        "stream.view",
        "",
        "xmk_secret_key",
        &[("id", "track 1+2".into())],
    )
    .unwrap();

    let parsed = Url::parse(&built).unwrap();
    assert_eq!(parsed.path(), "/music/rest/stream.view");
    let params: std::collections::HashMap<String, String> =
        parsed.query_pairs().into_owned().collect();
    assert_eq!(params.get("foo").map(String::as_str), Some("bar"));
    assert_eq!(
        params.get("apiKey").map(String::as_str),
        Some("xmk_secret_key")
    );
    assert_eq!(params.get("id").map(String::as_str), Some("track 1+2"));
    assert!(!params.contains_key("u"));
}

#[test]
fn encode_segment_uses_rfc3986_percent20_and_preserves_special_chars() {
    assert_eq!(encode_segment("song 1+2/test"), "song%201%2B2%2Ftest");
    assert_eq!(encode_segment("abc-123_4.5~"), "abc-123_4.5~");
}

#[test]
fn resolve_audio_filename_decodes_rfc5987_with_plus_intact() {
    let name = resolve_audio_filename(
        None,
        Some(
            r#"attachment; filename="fallback.flac"; filename*=UTF-8''Rock%20%2B%20Roll+Live.flac"#,
        ),
        "audio/flac",
        None,
        None,
        "song-1",
    );
    assert_eq!(name, "Rock + Roll+Live.flac");
}

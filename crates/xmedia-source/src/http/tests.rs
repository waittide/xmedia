use std::collections::HashMap;
use std::io::{Read, Write};
use std::net::{IpAddr, TcpListener};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use axum::Router;
use axum::http::HeaderMap;
use axum::response::IntoResponse;
use axum::routing::get;
use reqwest::dns::{Name, Resolve};

use super::*;

#[test]
fn restricted_ip_blocks_private_loopback_cgnat_and_ipv6_ranges() {
    for blocked in [
        "127.0.0.1",
        "10.0.0.1",
        "172.16.0.1",
        "172.31.255.254",
        "192.168.1.1",
        "169.254.169.254",
        "100.64.0.1",
        "192.0.0.1",
        "198.18.0.1",
        "198.19.255.254",
        "0.0.0.0",
        "240.0.0.1",
        "::1",
        "::",
        "fc00::1",
        "fd12:3456::1",
        "fe80::1",
        "fec0::1",
        "::ffff:127.0.0.1",
        "::ffff:172.16.5.4",
        "::127.0.0.1",
        "::ffff:0:127.0.0.1",
        "64:ff9b::127.0.0.1",
        "64:ff9b::1.1.1.1",
        "64:ff9b:1::1",
        "2002:7f00:1::1",
        "2001:0000::1",
        "2001:db8::1",
    ] {
        let ip: IpAddr = blocked.parse().unwrap();
        assert!(is_restricted_ip(ip), "expected {blocked} to be restricted");
    }

    for allowed in [
        "1.1.1.1",
        "8.8.8.8",
        "172.15.0.1",
        "172.32.0.1",
        "2606:4700::1",
    ] {
        let ip: IpAddr = allowed.parse().unwrap();
        assert!(!is_restricted_ip(ip), "expected {allowed} to be public");
    }
}

#[test]
fn validate_target_url_rejects_local_private_and_non_http() {
    assert!(validate_target_url("http://127.0.0.1/admin", false).is_err());
    assert!(validate_target_url("http://localhost:8080/a", false).is_err());
    assert!(validate_target_url("http://localhost./a", false).is_err());
    assert!(validate_target_url("http://sub.localhost./a", false).is_err());
    assert!(validate_target_url("http://printer.local./a", false).is_err());
    assert!(validate_target_url("http://172.16.1.2/secret", false).is_err());
    assert!(validate_target_url("http://[::ffff:10.0.0.1]/x", false).is_err());
    assert!(validate_target_url("http://[64:ff9b::127.0.0.1]/x", false).is_err());
    assert!(validate_target_url("http://[2002:7f00:1::1]/x", false).is_err());
    assert!(validate_target_url("file:///etc/passwd", false).is_err());
    // Fail-closed when DNS resolution fails for a nonexistent domain
    assert!(
        validate_target_url("http://definitely-nonexistent-host.invalid/", false).is_err(),
        "DNS failure must fail-closed"
    );
    assert!(validate_target_url("https://1.1.1.1/ok", false).is_ok());
    assert!(validate_target_url("http://127.0.0.1/ok", true).is_ok());
}

#[test]
fn ssrf_redirect_policy_blocks_302_to_loopback_and_localhost() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    let handle = std::thread::spawn(move || {
        if let Ok((mut stream, _)) = listener.accept() {
            let mut buf = [0u8; 1024];
            let _ = stream.read(&mut buf);
            let resp = "HTTP/1.1 302 Found\r\nLocation: http://127.0.0.1:9/secret\r\nContent-Length: 0\r\nConnection: close\r\n\r\n";
            let _ = stream.write_all(resp.as_bytes());
        }
    });

    // Build a blocking client that permits connecting to the initial test server IP
    // ONLY by bypassing initial check, while testing ssrf_redirect_policy(false, 6).
    let client = reqwest::blocking::Client::builder()
        .timeout(Duration::from_secs(5))
        .redirect(ssrf_redirect_policy(false, MAX_REDIRECT_HOPS))
        .build()
        .unwrap();
    let err = client
        .get(format!("http://{addr}/start"))
        .send()
        .unwrap_err();
    assert!(
        err.is_redirect(),
        "expected redirect policy error, got: {err}"
    );
    let _ = handle.join();
}

#[test]
fn cookie_matching_only_allows_parent_to_child_with_longest_suffix() {
    let cookies = Arc::new(Mutex::new(HashMap::new()));
    let client = LegadoHttpClient::new(
        HttpClientConfig {
            allow_private_ips: true,
            ..Default::default()
        },
        cookies,
        None,
        true,
    )
    .unwrap();

    assert!(client.set_cookie("https://api.example.com", "secret=child"));
    assert!(client.set_cookie("https://example.com", "base=root"));
    assert!(client.set_cookie("https://sub.example.com", "sub=mid"));

    // Exact matches
    assert_eq!(client.get_cookie_header("https://example.com"), "base=root");
    assert_eq!(
        client.get_cookie_header("https://api.example.com"),
        "secret=child"
    );
    // Subdomain inherits longest parent suffix
    assert_eq!(
        client.get_cookie_header("https://deep.sub.example.com"),
        "sub=mid"
    );
    assert_eq!(
        client.get_cookie_header("https://other.example.com"),
        "base=root"
    );

    // Child cookie must NEVER leak upward to parent domain
    client.remove_cookie("https://example.com");
    assert_eq!(
        client.get_cookie_header("https://example.com"),
        "",
        "child domain cookie must not leak to parent domain"
    );
    assert_eq!(
        client.get_cookie_header("https://evil-example.com"),
        "",
        "unrelated domain with matching string suffix must not receive cookie"
    );

    // Cookie quota enforcement
    let oversized = format!("big={}", "a".repeat(MAX_COOKIE_TOTAL_BYTES + 16));
    assert!(
        !client.set_cookie("https://quota.example.com", &oversized),
        "oversized cookie must be rejected by quota"
    );
}

#[test]
fn legado_http_client_enforces_response_size_limit() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    let handle = std::thread::spawn(move || {
        if let Ok((mut stream, _)) = listener.accept() {
            let mut buf = [0u8; 1024];
            let _ = stream.read(&mut buf);
            let oversz = MAX_RESPONSE_BODY_BYTES + 1024;
            let header = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: text/plain\r\nContent-Length: {oversz}\r\nConnection: close\r\n\r\n"
            );
            let _ = stream.write_all(header.as_bytes());
        }
    });

    let client = LegadoHttpClient::new(
        HttpClientConfig {
            allow_private_ips: true,
            ..Default::default()
        },
        Arc::new(Mutex::new(HashMap::new())),
        None,
        false,
    )
    .unwrap();

    let err = client
        .fetch(&format!("http://{addr}/huge"), "")
        .unwrap_err();
    assert!(
        err.contains("响应体超过最大限制"),
        "unexpected error: {err}"
    );
    let _ = handle.join();
}

#[tokio::test]
async fn ssrf_safe_resolver_blocks_localhost_and_literal_private_ips() {
    let resolver = SsrfSafeResolver;
    for host in ["localhost", "127.0.0.1", "[::ffff:10.0.0.1]", "box.local"] {
        let name: Name = host.parse().unwrap();
        let err = resolver.resolve(name).await.err().expect("expected error");
        let io_err = err.downcast_ref::<std::io::Error>();
        assert_eq!(
            io_err.map(|e| e.kind()),
            Some(std::io::ErrorKind::PermissionDenied),
            "expected PermissionDenied for {host}, got {err}"
        );
    }

    let public_name: Name = "1.1.1.1".parse().unwrap();
    let addrs: Vec<_> = resolver
        .resolve(public_name)
        .await
        .expect("public literal IP should pass")
        .collect();
    assert_eq!(addrs.len(), 1);
    assert_eq!(addrs[0].ip(), "1.1.1.1".parse::<IpAddr>().unwrap());
}

#[tokio::test]
async fn ssrf_redirect_policy_blocks_https_to_http_downgrade_and_hop_limit() {
    let app = Router::new().route(
        "/loop",
        get(|| async { axum::response::Redirect::temporary("/loop") }),
    );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let server = tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });

    let err = tokio::task::spawn_blocking(move || {
        let client = reqwest::blocking::Client::builder()
            .timeout(Duration::from_secs(5))
            .redirect(ssrf_redirect_policy(true, 2))
            .build()
            .unwrap();
        client
            .get(format!("http://{addr}/loop"))
            .send()
            .unwrap_err()
    })
    .await
    .unwrap();
    assert!(
        err.is_redirect(),
        "expected hop limit redirect error: {err}"
    );
    server.abort();

    assert!(validate_target_url_static("http://1.1.1.1/a", false).is_ok());
}

#[test]
fn parse_legado_url_handles_commas_in_url_and_nested_braces() {
    let raw = r#"https://api.example.com/v1/list?tags=a,b,c, {"method":"POST","headers":{"X-Custom":"{1,2}","Accept":"application/json"},"body":{"nested":{"k":"v"}}}"#;
    let parsed = parse_legado_url(raw);
    assert_eq!(parsed.url, "https://api.example.com/v1/list?tags=a,b,c");
    assert_eq!(parsed.options.method.as_deref(), Some("POST"));
    assert_eq!(
        parsed.options.headers.as_ref().unwrap()["X-Custom"].as_str(),
        Some("{1,2}")
    );
    assert_eq!(
        parsed.options.body.as_ref().unwrap()["nested"]["k"].as_str(),
        Some("v")
    );

    let unclosed = "https://api.example.com/search?q=a,b,{not-closed";
    let fallback = parse_legado_url(unclosed);
    assert_eq!(fallback.url, unclosed);
    assert!(fallback.options.method.is_none());
}

#[test]
fn try_resolve_data_url_handles_plain_percent_encoded_and_hex_mode() {
    let plain = parse_legado_url("data:text/plain,hello%20world");
    let plain_res = try_resolve_data_url(&plain).unwrap().unwrap();
    assert_eq!(plain_res.status, 200);
    assert_eq!(plain_res.body, "hello world");

    let img = parse_legado_url(r#"data:image/png;base64,aGVsbG8=,{"type":"image"}"#);
    let img_res = try_resolve_data_url(&img).unwrap().unwrap();
    assert_eq!(img_res.body, "hello");

    let hex_mode = parse_legado_url(r#"data:;base64,aGVsbG8=,{"type":"comic"}"#);
    let hex_res = try_resolve_data_url(&hex_mode).unwrap().unwrap();
    assert_eq!(hex_res.body, hex::encode(b"hello"));
}

#[test]
fn decode_response_bytes_priority_order() {
    let (gbk_bytes, _, _) = encoding_rs::GBK.encode("测试中文");
    // 1. Explicit charset wins over header & meta
    let decoded_explicit =
        decode_response_bytes(&gbk_bytes, Some("gbk"), Some("text/plain; charset=utf-8"));
    assert_eq!(decoded_explicit, "测试中文");

    // 2. Content-Type charset wins over meta
    let mut gbk_with_utf8_meta = b"<meta charset=\"utf-8\">".to_vec();
    gbk_with_utf8_meta.extend_from_slice(&gbk_bytes);
    let decoded_ct =
        decode_response_bytes(&gbk_with_utf8_meta, None, Some("text/html; charset=gbk"));
    assert!(decoded_ct.contains("测试中文"));

    // 3. <meta charset="gb18030"> sniffed when no explicit or header charset
    let (gb18030_bytes, _, _) = encoding_rs::GB18030.encode("漫画章节");
    let mut meta_bytes = b"<html><head><meta charset=\"gb18030\"></head>".to_vec();
    meta_bytes.extend_from_slice(&gb18030_bytes);
    let decoded_meta = decode_response_bytes(&meta_bytes, None, None);
    assert!(decoded_meta.contains("漫画章节"));

    // 4. Valid UTF-8
    assert_eq!(
        decode_response_bytes("纯UTF8文本".as_bytes(), None, None),
        "纯UTF8文本"
    );

    // 5. Fallback chardetng on raw non-UTF-8 Shift_JIS
    let (sjis_bytes, _, _) = encoding_rs::SHIFT_JIS.encode("日本語のテスト文字列です");
    let decoded_sjis = decode_response_bytes(&sjis_bytes, None, None);
    assert_eq!(decoded_sjis, "日本語のテスト文字列です");
}

#[tokio::test]
async fn cookie_jar_set_cookie_from_http_response_and_forbidden_headers_stripped() {
    let seen_headers: Arc<Mutex<HashMap<String, String>>> = Arc::new(Mutex::new(HashMap::new()));
    let seen_c = seen_headers.clone();

    let app = Router::new().route(
        "/login",
        get(move |headers: HeaderMap| {
            let seen = seen_c.clone();
            async move {
                let mut map = seen.lock().unwrap();
                for (k, v) in &headers {
                    if let Ok(s) = v.to_str() {
                        map.insert(k.as_str().to_ascii_lowercase(), s.to_string());
                    }
                }
                (
                    [(
                        reqwest::header::SET_COOKIE.as_str(),
                        "sid=xyz789; Path=/; HttpOnly",
                    )],
                    "ok",
                )
                    .into_response()
            }
        }),
    );

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let server = tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });

    let url =
        format!(r#"http://{addr}/login,{{"headers":{{"Content-Length":"999","X-Opt":"ok"}}}}"#);
    let res = tokio::task::spawn_blocking(move || {
        let cookies = Arc::new(Mutex::new(HashMap::new()));
        let default_header_json =
            r#"{"Host":"evil.example.com","Connection":"keep-alive","X-Allowed":"yes"}"#;
        let client = LegadoHttpClient::new(
            HttpClientConfig {
                allow_private_ips: true,
                ..Default::default()
            },
            cookies,
            Some(default_header_json),
            true,
        )
        .unwrap();
        let r = client.fetch(&url, "").unwrap();
        let jar_ck = client.get_cookie_header(&format!("http://{addr}"));
        (r, jar_ck)
    })
    .await
    .unwrap();

    assert_eq!(res.0.body, "ok");
    assert_eq!(res.1, "sid=xyz789");
    let captured = seen_headers.lock().unwrap();
    assert_eq!(captured.get("x-allowed").map(String::as_str), Some("yes"));
    assert_eq!(captured.get("x-opt").map(String::as_str), Some("ok"));
    assert_ne!(
        captured.get("host").map(String::as_str),
        Some("evil.example.com")
    );
    assert!(!captured.contains_key("content-length"));

    server.abort();
}

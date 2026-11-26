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

    // Unbalanced `{` or `"` in URL before `,{...}` must not poison backwards scanner
    let unbalanced_url = r#"https://api.example.com/search?q={"bad,{"method":"POST"}"#;
    let parsed_unbalanced = parse_legado_url(unbalanced_url);
    assert_eq!(
        parsed_unbalanced.url,
        r#"https://api.example.com/search?q={"bad"#
    );
    assert_eq!(parsed_unbalanced.options.method.as_deref(), Some("POST"));

    // Braces in URL path + commas in query + `}` inside JSON string value
    let brace_in_str = r#"https://api.example.com/v1/{id}/items?a=1,2,{"method":"POST","headers":{"X-Test":"a}b"}}"#;
    let parsed_brace_str = parse_legado_url(brace_in_str);
    assert_eq!(
        parsed_brace_str.url,
        "https://api.example.com/v1/{id}/items?a=1,2"
    );
    assert_eq!(parsed_brace_str.options.method.as_deref(), Some("POST"));
    assert_eq!(
        parsed_brace_str.options.headers.as_ref().unwrap()["X-Test"].as_str(),
        Some("a}b")
    );

    // Escaped quotes & braces inside JSON string value
    let escaped_quote = r#"https://api.example.com/x,{"method":"POST","body":"a\"b}c\\"}"#;
    let parsed_escaped = parse_legado_url(escaped_quote);
    assert_eq!(parsed_escaped.url, "https://api.example.com/x");
    assert_eq!(parsed_escaped.options.method.as_deref(), Some("POST"));
    assert_eq!(
        parsed_escaped.options.body.as_ref().unwrap().as_str(),
        Some(r#"a"b}c\"#)
    );

    // Malformed trailing `,{not_json}` falls back to raw URL
    let malformed = "https://api.example.com/search?q=1,{not_json}";
    let parsed_malformed = parse_legado_url(malformed);
    assert_eq!(parsed_malformed.url, malformed);
    assert!(parsed_malformed.options.method.is_none());
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

#[test]
fn decode_response_bytes_preserves_utf8_json_with_charset_literal_and_decodes_gbk_meta() {
    let expected = r#"{"note":"legacy page had charset=gbk","title":"中文标题"}"#;
    let utf8_json = expected.as_bytes();

    for ct in [
        Some("application/json"),
        Some("application/javascript"),
        Some("text/javascript"),
        Some("text/plain"),
        None,
    ] {
        assert_eq!(
            decode_response_bytes(utf8_json, None, ct),
            expected,
            "UTF-8 JSON corrupted for Content-Type={ct:?}"
        );
    }

    // Even if JSON embeds an HTML `<meta charset="gbk">` string, non-HTML Content-Type skips sniffing
    let expected_with_meta = r#"{"html":"<meta charset=\"gbk\">","title":"中文标题"}"#;
    assert_eq!(
        decode_response_bytes(
            expected_with_meta.as_bytes(),
            None,
            Some("application/json")
        ),
        expected_with_meta
    );

    // Actual GBK HTML with `<meta charset="gbk">` still decodes
    let (gbk_body, _, _) = encoding_rs::GBK.encode("第一章 诡秘");
    let mut gbk_html = br#"<html><head><meta charset="gbk"></head><body>"#.to_vec();
    gbk_html.extend_from_slice(&gbk_body);
    let decoded_gbk = decode_response_bytes(&gbk_html, None, Some("text/html"));
    assert!(decoded_gbk.contains("第一章 诡秘"), "got: {decoded_gbk}");

    // Legacy `<meta http-equiv="Content-Type" content="text/html; charset=gb18030">` decodes too
    let (gb18030_body, _, _) = encoding_rs::GB18030.encode("第二章 非凡");
    let mut gb18030_html =
        br#"<html><head><meta http-equiv="Content-Type" content="text/html; charset=gb18030"></head>"#
            .to_vec();
    gb18030_html.extend_from_slice(&gb18030_body);
    let decoded_gb18030 = decode_response_bytes(&gb18030_html, None, None);
    assert!(
        decoded_gb18030.contains("第二章 非凡"),
        "got: {decoded_gb18030}"
    );
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

#[test]
fn literal_http_url_rejects_options_and_data_urls() {
    assert_eq!(
        resolve_literal_http_url("https://example.com/v/1?a=b", None).unwrap(),
        "https://example.com/v/1?a=b"
    );
    assert_eq!(
        resolve_literal_http_url("/v/1", Some("https://example.com/root/")).unwrap(),
        "https://example.com/v/1"
    );
    assert!(
        resolve_literal_http_url(
            "https://example.com/v/1,{\"method\":\"POST\",\"headers\":{\"X-Evil\":\"1\"}}",
            None,
        )
        .is_err()
    );
    assert!(resolve_literal_http_url("data:text/html,<h1>x</h1>", None).is_err());
    assert!(resolve_literal_http_url("javascript:alert(1)", Some("https://example.com")).is_err());
}

#[test]
fn legado_url_template_encodes_url_vars_and_isolates_json_options() {
    let rendered = substitute_legado_url_template_vars(
        "https://example.com/search?q={{key}}&p={{page}}",
        &[("key", "hello world&injected=1"), ("page", "2")],
        None,
    )
    .unwrap();
    assert_eq!(
        rendered,
        "https://example.com/search?q=hello%20world%26injected%3D1&p=2"
    );

    let poison = "x\",\"headers\":{\"X-Injected\":\"pwned\"},\"z\":\"";
    let rendered_post = substitute_legado_url_template_vars(
        "https://example.com/api?k={{key}},{\"method\":\"POST\",\"body\":\"q={{key}}&p={{page}}\"}",
        &[("key", poison), ("page", "1")],
        None,
    )
    .unwrap();
    let (url_part, opt_part) = split_legado_url_and_raw_options(&rendered_post);
    assert!(url_part.contains("?k=x%22"));
    let parsed_opts: LegadoUrlOptions = serde_json::from_str(opt_part.unwrap()).unwrap();
    assert_eq!(parsed_opts.method.as_deref(), Some("POST"));
    assert!(parsed_opts.headers.is_none());
    assert!(
        parsed_opts
            .body
            .as_ref()
            .and_then(|v| v.as_str())
            .unwrap()
            .starts_with("q=x%22%2C%22headers%22")
    );

    let json_body_tpl = "https://example.com/api,{\"method\":\"POST\",\"headers\":{\"Content-Type\":\"application/json\"},\"body\":\"{\\\"kw\\\":\\\"{{key}}\\\"}\"}";
    let rendered_json =
        substitute_legado_url_template_vars(json_body_tpl, &[("key", poison)], None).unwrap();
    let (_, json_opts_str) = split_legado_url_and_raw_options(&rendered_json);
    let json_opts: LegadoUrlOptions = serde_json::from_str(json_opts_str.unwrap()).unwrap();
    assert!(json_opts.headers.unwrap().get("X-Injected").is_none());
    let inner_body: serde_json::Value =
        serde_json::from_str(json_opts.body.as_ref().and_then(|v| v.as_str()).unwrap()).unwrap();
    assert_eq!(inner_body["kw"].as_str(), Some(poison));
}

#[test]
fn eval_json_options_template_prevents_duplicate_keys_and_handles_unquoted_placeholders() {
    let poison = "x\",\"method\":\"DELETE\",\"charset\":\"";
    let tpl = r#"{"method":"POST","headers":{"X-Sign":"{{sign(key)}}"},"body":"q={{key}}","charset":"UTF-8"}"#;
    let rendered = eval_json_options_template(
        tpl,
        &[("key", poison), ("page", "1")],
        |expr| match expr {
            "{{sign(key)}}" => Ok("sig\"123".to_string()),
            other => Err(format!("unexpected expr: {other}")),
        },
        "searchUrl",
    )
    .unwrap();

    // Raw structure signature must have exact keys without duplicate "method" or "charset"
    let sig = json_raw_structure_signature(&rendered).unwrap();
    let method_count = sig
        .iter()
        .filter(|t| matches!(t, JsonSigToken::Key(k) if k == "method"))
        .count();
    assert_eq!(method_count, 1);
    let parsed: LegadoUrlOptions = serde_json::from_str(&rendered).unwrap();
    assert_eq!(parsed.method.as_deref(), Some("POST"));
    assert_eq!(parsed.charset.as_deref(), Some("UTF-8"));
    assert_eq!(
        parsed.headers.as_ref().unwrap()["X-Sign"].as_str(),
        Some("sig\"123")
    );
    assert_eq!(
        parsed.body.as_ref().unwrap().as_str(),
        Some(format!("q={}", url_encode_component(poison)).as_str())
    );

    // Mixed JS + static form-urlencoded body must percent-encode static vars while preserving JS output
    let form_js_tpl = r#"{"method":"POST","headers":{"Content-Type":"application/x-www-form-urlencoded"},"body":"q={{key}}&sig={{sign(key)}}"}"#;
    let rendered_form_js = eval_json_options_template(
        form_js_tpl,
        &[("key", "foo&admin=1")],
        |expr| match expr {
            "{{sign(key)}}" => Ok("deadbeef".to_string()),
            other => Err(format!("unexpected expr: {other}")),
        },
        "searchUrl",
    )
    .unwrap();
    let parsed_form_js: LegadoUrlOptions = serde_json::from_str(&rendered_form_js).unwrap();
    assert_eq!(
        parsed_form_js.body.as_ref().unwrap().as_str(),
        Some("q=foo%26admin%3D1&sig=deadbeef")
    );

    // Explicit application/json Content-Type must JSON-escape rather than URL-encode
    let json_js_tpl = r#"{"method":"POST","headers":{"Content-Type":"application/json"},"body":"{\"q\":\"{{key}}\",\"sig\":\"{{sign(key)}}\"}"}"#;
    let rendered_json_js = eval_json_options_template(
        json_js_tpl,
        &[("key", "foo&\"bar\"")],
        |_| Ok("sig1".to_string()),
        "searchUrl",
    )
    .unwrap();
    let parsed_json_js: LegadoUrlOptions = serde_json::from_str(&rendered_json_js).unwrap();
    let inner_json: serde_json::Value =
        serde_json::from_str(parsed_json_js.body.as_ref().unwrap().as_str().unwrap()).unwrap();
    assert_eq!(inner_json["q"], "foo&\"bar\"");
    assert_eq!(inner_json["sig"], "sig1");

    // Unquoted placeholders (`"webView":{{flag}}` and `"page":{{page}}`) still get a valid expected_sig
    let unquoted_tpl =
        r#"{"method":"POST","webView":{{flag}},"body":{"page":{{page}},"kw":"{{key}}"}}"#;
    let rendered_unquoted = eval_json_options_template(
        unquoted_tpl,
        &[("key", poison), ("page", "3")],
        |expr| match expr {
            "{{flag}}" => Ok("true".to_string()),
            other => Err(format!("unexpected expr: {other}")),
        },
        "searchUrl",
    )
    .unwrap();
    let parsed_unquoted: LegadoUrlOptions = serde_json::from_str(&rendered_unquoted).unwrap();
    assert_eq!(parsed_unquoted.method.as_deref(), Some("POST"));
    assert_eq!(parsed_unquoted.web_view, Some(serde_json::json!(true)));
    assert_eq!(parsed_unquoted.body.as_ref().unwrap()["page"], 3);
    assert_eq!(
        parsed_unquoted.body.as_ref().unwrap()["kw"].as_str(),
        Some(poison)
    );

    // Unquoted JS returning an object or breakout string cannot alter JSON structure
    let rendered_obj_as_str = eval_json_options_template(
        r#"{"method":"POST","headers":{"X-A":{{dynVal}}},"body":"q={{key}}"}"#,
        &[("key", "ok")],
        |_| Ok(r#"{"evil":1}"#.to_string()),
        "searchUrl",
    )
    .unwrap();
    let parsed_obj: LegadoUrlOptions = serde_json::from_str(&rendered_obj_as_str).unwrap();
    assert!(
        parsed_obj.headers.as_ref().unwrap()["X-A"].is_string(),
        "object value must be stringified, not injected as nested object"
    );

    // Malformed template JSON fails instead of falling back to expected_sig == None
    assert!(
        eval_json_options_template(
            r#"{"method":"POST","webView":{{flag}},"#,
            &[("key", "ok")],
            |_| Ok("true".to_string()),
            "searchUrl",
        )
        .is_err()
    );
}

#[test]
fn legado_http_client_fetch_with_timeout_reports_body_timeout_on_chunked_stall() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    let (done_tx, done_rx) = std::sync::mpsc::channel::<()>();

    let server_handle = std::thread::spawn(move || {
        // First connection: send 200 OK headers + first chunk immediately, then stall > op_timeout.
        if let Ok((mut stream, _)) = listener.accept() {
            let mut buf = [0u8; 1024];
            let _ = stream.read(&mut buf);
            let head_and_chunk1 = b"HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nTransfer-Encoding: chunked\r\nConnection: close\r\n\r\n13\r\n{\"url\":\"https://cdn\r\n";
            let _ = stream.write_all(head_and_chunk1);
            let _ = stream.flush();
            let _ = done_rx.recv_timeout(Duration::from_millis(1200));
        }
        // Second connection: send incomplete chunked body and close immediately (non-timeout failure).
        if let Ok((mut stream, _)) = listener.accept() {
            let mut buf = [0u8; 1024];
            let _ = stream.read(&mut buf);
            let broken = b"HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nTransfer-Encoding: chunked\r\nConnection: close\r\n\r\n7\r\n{\"a\":1}\r\n";
            let _ = stream.write_all(broken);
            let _ = stream.flush();
        }
    });

    let client = LegadoHttpClient::new(
        HttpClientConfig {
            allow_private_ips: true,
            timeout_secs: 5,
            ..Default::default()
        },
        Arc::new(Mutex::new(HashMap::new())),
        None,
        false,
    )
    .unwrap();

    let t0 = std::time::Instant::now();
    let err = client
        .fetch_with_timeout(
            &format!("http://{addr}/slow-body"),
            "",
            Some(Duration::from_millis(200)),
        )
        .unwrap_err();
    let elapsed = t0.elapsed();
    let _ = done_tx.send(());

    assert!(
        elapsed < Duration::from_millis(500),
        "expected fast abort under 500ms, took {:?}",
        elapsed
    );
    assert!(
        err.contains("读取响应体超时"),
        "expected body-read timeout message containing '读取响应体超时', got: {err}"
    );

    let eof_err = client
        .fetch_with_timeout(
            &format!("http://{addr}/broken-body"),
            "",
            Some(Duration::from_millis(500)),
        )
        .unwrap_err();
    assert!(
        eof_err.contains("读取响应体失败"),
        "expected non-timeout EOF error containing '读取响应体失败', got: {eof_err}"
    );

    let _ = server_handle.join();
}

#[tokio::test]
async fn fetch_with_timeout_resilient_skips_retry_when_post_attempt_budget_under_4s() {
    use axum::http::StatusCode;
    use std::sync::atomic::{AtomicUsize, Ordering};

    let hits = Arc::new(AtomicUsize::new(0));
    let hits_c = hits.clone();

    let app = Router::new().route(
        "/slow-503",
        get(move || {
            let h = hits_c.clone();
            async move {
                h.fetch_add(1, Ordering::SeqCst);
                tokio::time::sleep(Duration::from_millis(4800)).await;
                (StatusCode::SERVICE_UNAVAILABLE, "busy")
            }
        }),
    );

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let server = tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });

    let url = format!("http://{addr}/slow-503");
    let started = std::time::Instant::now();
    let res = tokio::task::spawn_blocking(move || {
        let client = LegadoHttpClient::new(
            HttpClientConfig {
                allow_private_ips: true,
                timeout_secs: 10,
                ..Default::default()
            },
            Arc::new(Mutex::new(HashMap::new())),
            None,
            false,
        )
        .unwrap();
        client.fetch_with_timeout_resilient(&url, "", Some(Duration::from_secs(5)), 1, None)
    })
    .await
    .unwrap()
    .expect("should return 503 response instead of sleeping past deadline");

    let elapsed = started.elapsed();
    assert_eq!(res.status, 503);
    assert_eq!(res.body, "busy");
    assert_eq!(hits.load(Ordering::SeqCst), 1);
    assert!(
        elapsed < Duration::from_millis(5050),
        "must return immediately without 300ms backoff sleep, took {elapsed:?}"
    );

    server.abort();
}

#[tokio::test]
async fn fetch_with_timeout_resilient_retries_503_to_200_and_never_retries_post_or_ssrf() {
    use axum::http::StatusCode;
    use axum::routing::post;
    use std::sync::atomic::{AtomicUsize, Ordering};

    let flaky_hits = Arc::new(AtomicUsize::new(0));
    let post_hits = Arc::new(AtomicUsize::new(0));
    let redir_hits = Arc::new(AtomicUsize::new(0));
    let secret_hits = Arc::new(AtomicUsize::new(0));

    let fh = flaky_hits.clone();
    let ph = post_hits.clone();
    let rh = redir_hits.clone();
    let sh = secret_hits.clone();

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let secret_url = format!("http://{addr}/secret");

    let app = Router::new()
        .route(
            "/flaky",
            get(move || {
                let h = fh.clone();
                async move {
                    let n = h.fetch_add(1, Ordering::SeqCst) + 1;
                    if n == 1 {
                        (StatusCode::SERVICE_UNAVAILABLE, "try again")
                    } else {
                        (StatusCode::OK, "recovered")
                    }
                }
            }),
        )
        .route(
            "/submit",
            post(move || {
                let h = ph.clone();
                async move {
                    h.fetch_add(1, Ordering::SeqCst);
                    (StatusCode::SERVICE_UNAVAILABLE, "post busy")
                }
            }),
        )
        .route(
            "/redir",
            get(move || {
                let h = rh.clone();
                let loc = secret_url.clone();
                async move {
                    h.fetch_add(1, Ordering::SeqCst);
                    (
                        StatusCode::FOUND,
                        [(reqwest::header::LOCATION.as_str(), loc)],
                        "",
                    )
                }
            }),
        )
        .route(
            "/secret",
            get(move || {
                let h = sh.clone();
                async move {
                    h.fetch_add(1, Ordering::SeqCst);
                    (StatusCode::OK, "secret")
                }
            }),
        );

    let server = tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });

    let base = format!("http://{addr}");
    tokio::task::spawn_blocking(move || {
        let client = LegadoHttpClient::new(
            HttpClientConfig {
                allow_private_ips: true,
                timeout_secs: 10,
                ..Default::default()
            },
            Arc::new(Mutex::new(HashMap::new())),
            None,
            false,
        )
        .unwrap();

        // 1. 503 -> 200 succeeds when >= 4s budget remains
        let ok_res = client
            .fetch_with_timeout_resilient(
                &format!("{base}/flaky"),
                "",
                Some(Duration::from_secs(6)),
                1,
                None,
            )
            .unwrap();
        assert_eq!(ok_res.status, 200);
        assert_eq!(ok_res.body, "recovered");

        // 2. Non-idempotent POST never retries
        let post_res = client
            .fetch_with_timeout_resilient(
                &format!(r#"{base}/submit,{{"method":"POST","body":"x=1"}}"#),
                "",
                Some(Duration::from_secs(6)),
                2,
                None,
            )
            .unwrap();
        assert_eq!(post_res.status, 503);

        // 3. Direct SSRF blocked upfront (0 target hits, no retry)
        let ssrf_client = LegadoHttpClient::new(
            HttpClientConfig {
                allow_private_ips: false,
                timeout_secs: 10,
                ..Default::default()
            },
            Arc::new(Mutex::new(HashMap::new())),
            None,
            false,
        )
        .unwrap();
        assert!(
            ssrf_client
                .fetch_with_timeout_resilient(
                    &format!("{base}/secret"),
                    "",
                    Some(Duration::from_secs(6)),
                    2,
                    None,
                )
                .is_err()
        );

        // 4. 302 -> 127.0.0.1 blocked by ssrf_redirect_policy(false) and never retried
        let redir_blocking = reqwest::blocking::Client::builder()
            .timeout(Duration::from_secs(5))
            .redirect(ssrf_redirect_policy(false, MAX_REDIRECT_HOPS))
            .build()
            .unwrap();
        let redir_client = LegadoHttpClient::with_blocking_client_for_test(
            redir_blocking,
            HttpClientConfig {
                allow_private_ips: true,
                timeout_secs: 5,
                ..Default::default()
            },
        );
        assert!(
            redir_client
                .fetch_with_timeout_resilient(
                    &format!("{base}/redir"),
                    "",
                    Some(Duration::from_secs(6)),
                    2,
                    None,
                )
                .is_err()
        );
    })
    .await
    .unwrap();

    assert_eq!(flaky_hits.load(Ordering::SeqCst), 2);
    assert_eq!(post_hits.load(Ordering::SeqCst), 1);
    assert_eq!(redir_hits.load(Ordering::SeqCst), 1);
    assert_eq!(secret_hits.load(Ordering::SeqCst), 0);

    server.abort();
}

#[tokio::test]
async fn proxied_requests_enforce_dns_ssrf_validation_before_contacting_proxy() {
    use std::sync::atomic::{AtomicUsize, Ordering};

    let proxy_hits = Arc::new(AtomicUsize::new(0));
    let proxy_hits_c = proxy_hits.clone();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let proxy_addr = listener.local_addr().unwrap();
    let proxy_server = tokio::spawn(async move {
        while let Ok((mut stream, _)) = listener.accept().await {
            proxy_hits_c.fetch_add(1, Ordering::SeqCst);
            let _ = tokio::io::AsyncWriteExt::shutdown(&mut stream).await;
        }
    });

    assert!(
        validate_target_url_async("http://127.0.0.1/admin", false)
            .await
            .is_err()
    );
    assert!(
        validate_target_url_async("http://definitely-nonexistent-ssrf-host.invalid/", false)
            .await
            .is_err()
    );
    assert!(
        validate_target_url_async("https://1.1.1.1/ok", false)
            .await
            .is_ok()
    );

    let proxy_url = format!("http://{proxy_addr}");
    let blocking_err = tokio::task::spawn_blocking({
        let proxy_url = proxy_url.clone();
        move || {
            let client = LegadoHttpClient::new(
                HttpClientConfig {
                    timeout_secs: 5,
                    allow_private_ips: false,
                    proxy_url: Some(proxy_url),
                },
                Arc::new(Mutex::new(HashMap::new())),
                None,
                false,
            )
            .unwrap();
            client
                .fetch("http://definitely-nonexistent-ssrf-host.invalid/secret", "")
                .unwrap_err()
        }
    })
    .await
    .unwrap();
    assert!(
        blocking_err.contains("安全策略"),
        "expected SSRF DNS failure before proxy send, got: {blocking_err}"
    );
    assert_eq!(
        proxy_hits.load(Ordering::SeqCst),
        0,
        "proxied blocking client must reject SSRF target before opening proxy connection"
    );

    let async_err = crate::proxy::fetch_image(
        "http://definitely-nonexistent-ssrf-host.invalid/cover.png",
        None,
        1024,
        &HttpClientConfig {
            timeout_secs: 5,
            allow_private_ips: false,
            proxy_url: Some(proxy_url),
        },
    )
    .await
    .unwrap_err();
    assert!(
        async_err.to_string().contains("安全策略"),
        "expected async SSRF DNS failure before proxy send, got: {async_err}"
    );
    assert_eq!(
        proxy_hits.load(Ordering::SeqCst),
        0,
        "proxied async image fetcher must reject SSRF target before opening proxy connection"
    );

    proxy_server.abort();
}

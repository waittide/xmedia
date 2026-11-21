use std::collections::HashMap;
use std::io::{Read, Write};
use std::net::{IpAddr, TcpListener};
use std::sync::{Arc, Mutex};
use std::time::Duration;

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

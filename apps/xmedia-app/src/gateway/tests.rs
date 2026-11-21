use super::format_gateway_url;
use super::security::{is_allowed_same_origin_header, parse_loopback_host_header};

#[test]
fn formats_ipv4_and_ipv6_gateway_urls_from_actual_socket_addr() {
    assert_eq!(
        format_gateway_url("127.0.0.1:41234".parse().unwrap()),
        "http://127.0.0.1:41234"
    );
    assert_eq!(
        format_gateway_url("0.0.0.0:41234".parse().unwrap()),
        "http://127.0.0.1:41234"
    );
    assert_eq!(
        format_gateway_url("[::1]:41234".parse().unwrap()),
        "http://[::1]:41234"
    );
    assert_eq!(
        format_gateway_url("[::]:41234".parse().unwrap()),
        "http://[::1]:41234"
    );
}

#[test]
fn validates_loopback_host_and_same_origin_headers() {
    assert_eq!(
        parse_loopback_host_header("127.0.0.1:8099", 8099),
        Some(("127.0.0.1", 8099))
    );
    assert_eq!(
        parse_loopback_host_header("LOCALHOST:8099", 8099),
        Some(("localhost", 8099))
    );
    assert_eq!(
        parse_loopback_host_header("[::1]:8099", 8099),
        Some(("[::1]", 8099))
    );
    assert_eq!(parse_loopback_host_header("127.0.0.1:9999", 8099), None);
    assert_eq!(parse_loopback_host_header("evil.example:8099", 8099), None);
    assert_eq!(parse_loopback_host_header("192.168.1.5:8099", 8099), None);

    assert!(is_allowed_same_origin_header(
        "http://127.0.0.1:8099",
        "127.0.0.1",
        8099
    ));
    assert!(is_allowed_same_origin_header(
        "http://[::1]:8099",
        "[::1]",
        8099
    ));
    assert!(!is_allowed_same_origin_header(
        "https://127.0.0.1:8099",
        "127.0.0.1",
        8099
    ));
    assert!(!is_allowed_same_origin_header(
        "http://127.0.0.1:9999",
        "127.0.0.1",
        8099
    ));
    assert!(!is_allowed_same_origin_header(
        "http://evil.example:8099",
        "127.0.0.1",
        8099
    ));
    assert!(!is_allowed_same_origin_header("null", "127.0.0.1", 8099));
}

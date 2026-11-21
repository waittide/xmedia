use std::net::{IpAddr, SocketAddr};
use std::sync::Arc;
use std::time::Duration;

use axum::extract::{Request, State};
use axum::http::StatusCode;
use axum::http::header::{HOST, ORIGIN};
use axum::middleware::Next;
use axum::response::Response;
use reqwest::dns::{Addrs, Name, Resolve, Resolving};
use url::Url;

use super::StandaloneGatewayState;
use super::dto::app_error;
use crate::opds_client::same_origin_redirect_policy;

const CONNECT_TIMEOUT: Duration = Duration::from_secs(15);
const READ_TIMEOUT: Duration = Duration::from_secs(45);
const LOCAL_UPLOAD_READ_TIMEOUT: Duration = Duration::from_secs(600);

fn is_blocked_metadata_or_link_local(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(v4) => {
            v4.is_link_local() // 169.254.0.0/16 (cloud metadata)
                || v4.is_unspecified()
                || v4.octets()[0] == 0
                || v4.is_multicast()
                || v4.is_broadcast()
        }
        IpAddr::V6(v6) => {
            if let Some(mapped) = v6.to_ipv4_mapped() {
                return is_blocked_metadata_or_link_local(IpAddr::V4(mapped));
            }
            let s = v6.segments();
            v6.is_unspecified() || v6.is_multicast() || (s[0] & 0xffc0) == 0xfe80 // fe80::/10 link-local
        }
    }
}

/// DNS resolver for remote OPDS/Subsonic sources: allows LAN & loopback self-hosted servers while
/// pinning DNS results and blocking link-local / cloud metadata endpoints against DNS rebinding.
#[derive(Debug, Default)]
struct NoMetadataResolver;

impl Resolve for NoMetadataResolver {
    fn resolve(&self, name: Name) -> Resolving {
        let host = name.as_str().to_string();
        Box::pin(async move {
            let addrs: Vec<SocketAddr> = tokio::net::lookup_host((host.as_str(), 0))
                .await
                .map_err(|e| {
                    Box::new(std::io::Error::new(
                        std::io::ErrorKind::AddrNotAvailable,
                        format!("DNS resolution failed for '{host}': {e}"),
                    )) as Box<dyn std::error::Error + Send + Sync>
                })?
                .collect();
            if addrs.is_empty() {
                return Err(Box::new(std::io::Error::new(
                    std::io::ErrorKind::AddrNotAvailable,
                    format!("DNS resolution returned no addresses for '{host}'"),
                ))
                    as Box<dyn std::error::Error + Send + Sync>);
            }
            for addr in &addrs {
                if is_blocked_metadata_or_link_local(addr.ip()) {
                    return Err(Box::new(std::io::Error::new(
                        std::io::ErrorKind::PermissionDenied,
                        format!(
                            "blocked link-local or metadata address '{}' for host '{host}'",
                            addr.ip()
                        ),
                    ))
                        as Box<dyn std::error::Error + Send + Sync>);
                }
            }
            let boxed: Addrs = Box::new(addrs.into_iter());
            Ok(boxed)
        })
    }
}

pub(super) fn build_remote_http_client(
    allow_insecure_tls: bool,
) -> anyhow::Result<reqwest::Client> {
    let client = reqwest::Client::builder()
        .use_rustls_tls()
        .connect_timeout(CONNECT_TIMEOUT)
        .read_timeout(READ_TIMEOUT)
        .dns_resolver(Arc::new(NoMetadataResolver))
        .redirect(same_origin_redirect_policy())
        .danger_accept_invalid_certs(allow_insecure_tls)
        .build()?;
    Ok(client)
}

pub(super) fn build_local_upload_http_client() -> anyhow::Result<reqwest::Client> {
    let client = reqwest::Client::builder()
        .no_proxy()
        .connect_timeout(CONNECT_TIMEOUT)
        .read_timeout(LOCAL_UPLOAD_READ_TIMEOUT)
        .redirect(reqwest::redirect::Policy::none())
        .build()?;
    Ok(client)
}

pub(super) fn parse_loopback_host_header(
    raw_host: &str,
    expected_port: u16,
) -> Option<(&'static str, u16)> {
    let trimmed = raw_host.trim().to_ascii_lowercase();
    if expected_port == 80 {
        match trimmed.as_str() {
            "127.0.0.1" => return Some(("127.0.0.1", 80)),
            "localhost" => return Some(("localhost", 80)),
            "[::1]" => return Some(("[::1]", 80)),
            _ => {}
        }
    }
    let expected_suffix = format!(":{expected_port}");
    let host_part = trimmed.strip_suffix(&expected_suffix)?;
    match host_part {
        "127.0.0.1" => Some(("127.0.0.1", expected_port)),
        "localhost" => Some(("localhost", expected_port)),
        "[::1]" => Some(("[::1]", expected_port)),
        _ => None,
    }
}

pub(super) fn is_allowed_same_origin_header(
    raw_origin: &str,
    canonical_host: &str,
    expected_port: u16,
) -> bool {
    let Ok(parsed) = Url::parse(raw_origin.trim()) else {
        return false;
    };
    if parsed.scheme() != "http" {
        return false;
    }
    if !parsed.username().is_empty()
        || parsed.password().is_some()
        || parsed.query().is_some()
        || parsed.fragment().is_some()
    {
        return false;
    }
    if parsed.port_or_known_default() != Some(expected_port) {
        return false;
    }
    parsed
        .host_str()
        .is_some_and(|h| h.eq_ignore_ascii_case(canonical_host))
}

pub(super) async fn guard_app_loopback_request(
    State(state): State<StandaloneGatewayState>,
    req: Request,
    next: Next,
) -> Response {
    let Some(raw_host) = req.headers().get(HOST).and_then(|v| v.to_str().ok()) else {
        return app_error(
            StatusCode::FORBIDDEN,
            "invalid_host",
            "Missing or invalid Host header on local gateway endpoint",
        );
    };
    let Some((canonical_host, port)) = parse_loopback_host_header(raw_host, state.expected_port)
    else {
        return app_error(
            StatusCode::FORBIDDEN,
            "invalid_host",
            "Local gateway endpoints only accept loopback Host headers",
        );
    };

    if let Some(sec_fetch_site) = req
        .headers()
        .get("sec-fetch-site")
        .and_then(|v| v.to_str().ok())
    {
        let sfs = sec_fetch_site.trim();
        // Block both `cross-site` AND `same-site` (cross-port localhost CSRF).
        if !sfs.eq_ignore_ascii_case("same-origin") && !sfs.eq_ignore_ascii_case("none") {
            return app_error(
                StatusCode::FORBIDDEN,
                "cross_site_blocked",
                "Cross-site or cross-port requests to local gateway endpoints are forbidden",
            );
        }
    }

    if let Some(origin_val) = req.headers().get(ORIGIN) {
        let valid = origin_val
            .to_str()
            .ok()
            .is_some_and(|raw| is_allowed_same_origin_header(raw, canonical_host, port));
        if !valid {
            return app_error(
                StatusCode::FORBIDDEN,
                "cross_origin_blocked",
                "Cross-origin requests to local gateway endpoints are forbidden",
            );
        }
    }

    next.run(req).await
}

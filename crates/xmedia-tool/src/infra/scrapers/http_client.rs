use std::net::{IpAddr, Ipv4Addr, ToSocketAddrs};
use std::time::Duration;

use reqwest::Client;
use xmedia_core::domain::error::{DomainError, DomainResult};

use crate::domain::config::ProviderCredentials;

pub const DEFAULT_TOOL_USER_AGENT: &str =
    "xmedia-tool/0.1.0 (https://github.com/xmedia-org/xmedia)";
const MAX_REDIRECT_HOPS: usize = 6;

fn is_restricted_ipv4(v4: Ipv4Addr) -> bool {
    let [a, b, _, _] = v4.octets();
    v4.is_loopback()
        || v4.is_private()
        || v4.is_link_local()
        || v4.is_broadcast()
        || v4.is_documentation()
        || v4.is_multicast()
        || v4.is_unspecified()
        || (a == 100 && (64..=127).contains(&b)) // CGNAT 100.64.0.0/10
        || a == 0
        || a >= 240
}

pub fn is_restricted_ip(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(v4) => is_restricted_ipv4(v4),
        IpAddr::V6(v6) => {
            if let Some(mapped) = v6.to_ipv4_mapped() {
                return is_restricted_ipv4(mapped);
            }
            v6.is_loopback()
                || v6.is_unspecified()
                || v6.is_multicast()
                || (v6.segments()[0] & 0xfe00) == 0xfc00 // fc00::/7 ULA
                || (v6.segments()[0] & 0xffc0) == 0xfe80 // fe80::/10 link-local
        }
    }
}

pub fn validate_public_http_url(raw_url: &str) -> DomainResult<reqwest::Url> {
    validate_artwork_http_url(raw_url, false)
}

pub fn validate_artwork_http_url(
    raw_url: &str,
    allow_private_ips: bool,
) -> DomainResult<reqwest::Url> {
    let parsed = reqwest::Url::parse(raw_url.trim())
        .map_err(|_| DomainError::Validation("invalid artwork URL".into()))?;
    if !matches!(parsed.scheme(), "http" | "https")
        || !parsed.username().is_empty()
        || parsed.password().is_some()
    {
        return Err(DomainError::Validation(
            "artwork URL must use http or https without embedded credentials".into(),
        ));
    }
    let host = parsed
        .host_str()
        .ok_or_else(|| DomainError::Validation("artwork URL missing host".into()))?;
    if allow_private_ips {
        return Ok(parsed);
    }
    let lower = host.to_ascii_lowercase();
    if lower.is_empty()
        || lower == "localhost"
        || lower.ends_with(".localhost")
        || lower.ends_with(".local")
    {
        return Err(DomainError::Validation(
            "disallowed local/private artwork host".into(),
        ));
    }

    if let Ok(ip) = host
        .trim_matches(|c| c == '[' || c == ']')
        .parse::<IpAddr>()
    {
        if is_restricted_ip(ip) {
            return Err(DomainError::Validation(format!(
                "disallowed private/reserved IP address '{ip}'"
            )));
        }
        return Ok(parsed);
    }

    let port = parsed.port_or_known_default().unwrap_or(80);
    if let Ok(addrs) = (host, port).to_socket_addrs() {
        for addr in addrs {
            if is_restricted_ip(addr.ip()) {
                return Err(DomainError::Validation(format!(
                    "host '{host}' resolved to private/reserved IP '{}'",
                    addr.ip()
                )));
            }
        }
    }

    Ok(parsed)
}

fn build_http_client_builder(creds: &ProviderCredentials) -> DomainResult<reqwest::ClientBuilder> {
    let ua = creds
        .user_agent
        .as_deref()
        .filter(|s| !s.trim().is_empty())
        .unwrap_or(DEFAULT_TOOL_USER_AGENT);

    let timeout_secs = creds.timeout_secs.map(|s| s.clamp(5, 120)).unwrap_or(20);
    let mut builder = Client::builder()
        .user_agent(ua)
        .http1_ignore_invalid_headers_in_responses(true)
        .timeout(Duration::from_secs(timeout_secs))
        .connect_timeout(Duration::from_secs(timeout_secs.min(10)));

    if let Some(proxy_url) = creds
        .http_proxy
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
    {
        let proxy = reqwest::Proxy::all(proxy_url).map_err(|e| {
            DomainError::Validation(format!("invalid proxy URL '{proxy_url}': {e}"))
        })?;
        builder = builder.proxy(proxy);
    }

    Ok(builder)
}

pub fn build_http_client(creds: &ProviderCredentials) -> DomainResult<Client> {
    build_http_client_builder(creds)?
        .build()
        .map_err(|e| DomainError::Storage(format!("failed building HTTP client: {e}")))
}

pub fn build_ssrf_safe_http_client(
    creds: &ProviderCredentials,
    allow_private_ips: bool,
) -> DomainResult<Client> {
    let policy = reqwest::redirect::Policy::custom(move |attempt| {
        if attempt.previous().len() >= MAX_REDIRECT_HOPS {
            return attempt.error(std::io::Error::other(format!(
                "redirect limit ({MAX_REDIRECT_HOPS}) exceeded"
            )));
        }
        if let Err(err) = validate_artwork_http_url(attempt.url().as_str(), allow_private_ips) {
            return attempt.error(std::io::Error::new(
                std::io::ErrorKind::PermissionDenied,
                err.to_string(),
            ));
        }
        attempt.follow()
    });
    build_http_client_builder(creds)?
        .redirect(policy)
        .build()
        .map_err(|e| DomainError::Storage(format!("failed building HTTP client: {e}")))
}

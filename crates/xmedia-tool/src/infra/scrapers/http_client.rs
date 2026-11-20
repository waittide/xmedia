use std::collections::HashMap;
use std::net::{IpAddr, Ipv4Addr, ToSocketAddrs};
use std::sync::{LazyLock, Mutex};
use std::time::Duration;

use reqwest::Client;
use xmedia_core::domain::error::{DomainError, DomainResult};

use crate::domain::config::ProviderCredentials;

pub const DEFAULT_TOOL_USER_AGENT: &str =
    "xmedia-tool/0.1.0 (https://github.com/xmedia-org/xmedia)";
const MAX_REDIRECT_HOPS: usize = 6;
const MAX_CACHED_HTTP_CLIENTS: usize = 64;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SsrfRedirectGuard {
    None,
    PublicOnly,
    AllowPrivate,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct HttpClientCacheKey {
    proxy_url: Option<String>,
    user_agent: String,
    timeout_secs: u64,
    ssrf_guard: SsrfRedirectGuard,
}

static HTTP_CLIENT_CACHE: LazyLock<Mutex<HashMap<HttpClientCacheKey, Client>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

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

pub fn get_or_build_http_client(
    proxy_url: Option<&str>,
    user_agent: &str,
    timeout_secs: u64,
    ssrf_guard: SsrfRedirectGuard,
) -> DomainResult<Client> {
    let normalized_proxy = proxy_url
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string);
    let normalized_ua = if user_agent.trim().is_empty() {
        DEFAULT_TOOL_USER_AGENT.to_string()
    } else {
        user_agent.trim().to_string()
    };
    let clamped_timeout = timeout_secs.clamp(5, 120);
    let key = HttpClientCacheKey {
        proxy_url: normalized_proxy,
        user_agent: normalized_ua,
        timeout_secs: clamped_timeout,
        ssrf_guard,
    };

    {
        let guard = HTTP_CLIENT_CACHE.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(client) = guard.get(&key) {
            return Ok(client.clone());
        }
    }

    let mut builder = Client::builder()
        .user_agent(key.user_agent.as_str())
        .http1_ignore_invalid_headers_in_responses(true)
        .timeout(Duration::from_secs(key.timeout_secs))
        .connect_timeout(Duration::from_secs(key.timeout_secs.min(10)))
        .pool_max_idle_per_host(16)
        .pool_idle_timeout(Duration::from_secs(90));

    if let Some(ref proxy_str) = key.proxy_url {
        let proxy = reqwest::Proxy::all(proxy_str.as_str()).map_err(|e| {
            DomainError::Validation(format!("invalid proxy URL '{proxy_str}': {e}"))
        })?;
        builder = builder.proxy(proxy);
    }

    match key.ssrf_guard {
        SsrfRedirectGuard::None => {}
        SsrfRedirectGuard::PublicOnly | SsrfRedirectGuard::AllowPrivate => {
            let allow_private_ips = key.ssrf_guard == SsrfRedirectGuard::AllowPrivate;
            let policy = reqwest::redirect::Policy::custom(move |attempt| {
                if attempt.previous().len() >= MAX_REDIRECT_HOPS {
                    return attempt.error(std::io::Error::other(format!(
                        "redirect limit ({MAX_REDIRECT_HOPS}) exceeded"
                    )));
                }
                if let Err(err) =
                    validate_artwork_http_url(attempt.url().as_str(), allow_private_ips)
                {
                    return attempt.error(std::io::Error::new(
                        std::io::ErrorKind::PermissionDenied,
                        err.to_string(),
                    ));
                }
                attempt.follow()
            });
            builder = builder.redirect(policy);
        }
    }

    let client = builder
        .build()
        .map_err(|e| DomainError::Storage(format!("failed building HTTP client: {e}")))?;

    let mut guard = HTTP_CLIENT_CACHE.lock().unwrap_or_else(|e| e.into_inner());
    if guard.len() >= MAX_CACHED_HTTP_CLIENTS && !guard.contains_key(&key) {
        guard.clear();
    }
    Ok(guard.entry(key).or_insert(client).clone())
}

pub fn build_http_client(creds: &ProviderCredentials) -> DomainResult<Client> {
    let ua = creds
        .user_agent
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .unwrap_or(DEFAULT_TOOL_USER_AGENT);
    let timeout_secs = creds.timeout_secs.map(|s| s.clamp(5, 120)).unwrap_or(20);
    get_or_build_http_client(
        creds.http_proxy.as_deref(),
        ua,
        timeout_secs,
        SsrfRedirectGuard::None,
    )
}

pub fn build_ssrf_safe_http_client(
    creds: &ProviderCredentials,
    allow_private_ips: bool,
) -> DomainResult<Client> {
    let ua = creds
        .user_agent
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .unwrap_or(DEFAULT_TOOL_USER_AGENT);
    let timeout_secs = creds.timeout_secs.map(|s| s.clamp(5, 120)).unwrap_or(20);
    let guard = if allow_private_ips {
        SsrfRedirectGuard::AllowPrivate
    } else {
        SsrfRedirectGuard::PublicOnly
    };
    get_or_build_http_client(creds.http_proxy.as_deref(), ua, timeout_secs, guard)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn http_client_cache_reuses_entries_by_tuple_key() {
        let c1 = get_or_build_http_client(None, "test-ua-1", 15, SsrfRedirectGuard::None).unwrap();
        let c2 = get_or_build_http_client(None, "test-ua-1", 15, SsrfRedirectGuard::None).unwrap();
        let _c3 =
            get_or_build_http_client(None, "test-ua-1", 15, SsrfRedirectGuard::PublicOnly).unwrap();
        let _ = (c1, c2);
        let guard = HTTP_CLIENT_CACHE.lock().unwrap();
        assert!(guard.contains_key(&HttpClientCacheKey {
            proxy_url: None,
            user_agent: "test-ua-1".into(),
            timeout_secs: 15,
            ssrf_guard: SsrfRedirectGuard::None,
        }));
        assert!(guard.contains_key(&HttpClientCacheKey {
            proxy_url: None,
            user_agent: "test-ua-1".into(),
            timeout_secs: 15,
            ssrf_guard: SsrfRedirectGuard::PublicOnly,
        }));
    }
}

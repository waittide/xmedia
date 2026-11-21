use std::net::{IpAddr, Ipv4Addr, SocketAddr, ToSocketAddrs};

use reqwest::dns::{Addrs, Name, Resolve, Resolving};
use url::Url;

/// Checks whether an IP address is private, loopback, link-local, transition, or otherwise non-public.
pub fn is_restricted_ip(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(v4) => is_restricted_ipv4(v4),
        IpAddr::V6(v6) => {
            if let Some(mapped) = v6.to_ipv4_mapped() {
                return is_restricted_ipv4(mapped);
            }
            let s = v6.segments();
            let is_ipv4_compatible =
                s[0] == 0 && s[1] == 0 && s[2] == 0 && s[3] == 0 && s[4] == 0 && s[5] == 0;
            let is_ipv4_translated =
                s[0] == 0 && s[1] == 0 && s[2] == 0 && s[3] == 0 && s[4] == 0xffff && s[5] == 0;
            let is_nat64_well_known = s[0] == 0x0064
                && s[1] == 0xff9b
                && s[2] == 0
                && s[3] == 0
                && s[4] == 0
                && s[5] == 0;
            let is_nat64_local = s[0] == 0x0064 && s[1] == 0xff9b && s[2] == 0x0001;
            let is_discard = s[0] == 0x0100 && s[1] == 0 && s[2] == 0 && s[3] == 0; // 100::/64
            let is_benchmarking = s[0] == 0x2001 && s[1] == 0x0002 && s[2] == 0; // 2001:2::/48
            let is_orchid = s[0] == 0x2001 && (0x0010..=0x002f).contains(&s[1]); // 2001:10::/28 & 2001:20::/28
            let is_6to4 = s[0] == 0x2002;
            let is_teredo = s[0] == 0x2001 && s[1] == 0x0000;
            let is_doc = s[0] == 0x2001 && s[1] == 0x0db8;
            let is_site_local = (s[0] & 0xffc0) == 0xfec0;
            let is_ula = (s[0] & 0xfe00) == 0xfc00;
            let is_link_local = (s[0] & 0xffc0) == 0xfe80;

            v6.is_loopback()
                || v6.is_unspecified()
                || v6.is_multicast()
                || is_ipv4_compatible
                || is_ipv4_translated
                || is_nat64_well_known
                || is_nat64_local
                || is_discard
                || is_benchmarking
                || is_orchid
                || is_6to4
                || is_teredo
                || is_doc
                || is_site_local
                || is_ula
                || is_link_local
        }
    }
}

fn is_restricted_ipv4(v4: Ipv4Addr) -> bool {
    let [a, b, c, _] = v4.octets();
    v4.is_loopback()
        || v4.is_private()
        || v4.is_link_local()
        || v4.is_broadcast()
        || v4.is_documentation()
        || v4.is_multicast()
        || v4.is_unspecified()
        || (a == 100 && (64..=127).contains(&b)) // CGNAT 100.64.0.0/10
        || (a == 192 && b == 0 && c == 0) // IETF protocol assignments 192.0.0.0/24
        || (a == 192 && b == 88 && c == 99) // 6to4 relay anycast 192.88.99.0/24
        || (a == 198 && (b == 18 || b == 19)) // Benchmarking 198.18.0.0/15
        || (a == 0)
        || (a >= 240)
}

pub fn is_restricted_hostname(host: &str) -> bool {
    let normalized = host.trim_end_matches('.').to_ascii_lowercase();
    normalized.is_empty()
        || normalized == "localhost"
        || normalized.ends_with(".localhost")
        || normalized.ends_with(".local")
}

/// Validates URL scheme, hostname, and literal IP without performing DNS resolution.
/// Used by async clients & redirect policies where DNS is resolved once via `SsrfSafeResolver`.
pub fn validate_target_url_static(raw_url: &str, allow_private_ips: bool) -> Result<Url, String> {
    let parsed = Url::parse(raw_url).map_err(|e| format!("无效的 URL '{raw_url}': {e}"))?;
    if !matches!(parsed.scheme(), "http" | "https") {
        return Err(format!("不支持的 URL 协议: {}", parsed.scheme()));
    }
    let host = parsed
        .host_str()
        .ok_or_else(|| format!("URL 缺少主机名: {raw_url}"))?;

    if allow_private_ips {
        return Ok(parsed);
    }

    if is_restricted_hostname(host) {
        return Err(format!("安全策略拦截了对本地主机 '{host}' 的请求"));
    }

    let ip_candidate = host
        .trim_matches(|c| c == '[' || c == ']')
        .trim_end_matches('.');
    if let Ok(ip) = ip_candidate.parse::<IpAddr>() {
        if is_restricted_ip(ip) {
            return Err(format!("安全策略拦截了对内网/保留地址 '{ip}' 的请求"));
        }
        return Ok(parsed);
    }

    Ok(parsed)
}

/// Validates URL scheme, hostname, literal IP, and (when `!allow_private_ips`) DNS records fail-closed.
pub fn validate_target_url(raw_url: &str, allow_private_ips: bool) -> Result<Url, String> {
    let parsed = validate_target_url_static(raw_url, allow_private_ips)?;
    if allow_private_ips {
        return Ok(parsed);
    }

    let host = parsed
        .host_str()
        .ok_or_else(|| format!("URL 缺少主机名: {raw_url}"))?;
    let ip_candidate = host
        .trim_matches(|c| c == '[' || c == ']')
        .trim_end_matches('.');
    if ip_candidate.parse::<IpAddr>().is_ok() {
        return Ok(parsed);
    }

    let port = parsed.port_or_known_default().unwrap_or(80);
    let addrs: Vec<SocketAddr> = (host, port)
        .to_socket_addrs()
        .map_err(|e| format!("安全策略拒绝未解析目标 '{host}': {e}"))?
        .collect();
    if addrs.is_empty() {
        return Err(format!("安全策略拒绝无 DNS 解析结果的目标 '{host}'"));
    }
    for addr in addrs {
        if is_restricted_ip(addr.ip()) {
            return Err(format!(
                "安全策略拦截了目标 '{host}' 解析到的内网/保留地址 '{}'",
                addr.ip()
            ));
        }
    }
    Ok(parsed)
}

/// Async custom DNS resolver injected into `reqwest` clients when `allow_private_ips == false`
/// and no proxy is configured. Validates every resolved IP and returns the validated addresses
/// directly to `reqwest`, eliminating DNS Rebinding TOCTOU and blocking DNS on Tokio workers.
#[derive(Debug, Default)]
pub struct SsrfSafeResolver;

impl Resolve for SsrfSafeResolver {
    fn resolve(&self, name: Name) -> Resolving {
        let host = name.as_str().to_string();
        Box::pin(async move {
            if is_restricted_hostname(&host) {
                return Err(Box::new(std::io::Error::new(
                    std::io::ErrorKind::PermissionDenied,
                    format!("安全策略拦截了对本地主机 '{host}' 的请求"),
                ))
                    as Box<dyn std::error::Error + Send + Sync>);
            }

            let ip_candidate = host
                .trim_matches(|c| c == '[' || c == ']')
                .trim_end_matches('.');
            if let Ok(ip) = ip_candidate.parse::<IpAddr>() {
                if is_restricted_ip(ip) {
                    return Err(Box::new(std::io::Error::new(
                        std::io::ErrorKind::PermissionDenied,
                        format!("安全策略拦截了对内网/保留地址 '{ip}' 的请求"),
                    ))
                        as Box<dyn std::error::Error + Send + Sync>);
                }
                let addrs: Addrs = Box::new(std::iter::once(SocketAddr::new(ip, 0)));
                return Ok(addrs);
            }

            let resolved: Vec<SocketAddr> = tokio::net::lookup_host((host.as_str(), 0))
                .await
                .map_err(|e| {
                    Box::new(std::io::Error::new(
                        std::io::ErrorKind::AddrNotAvailable,
                        format!("安全策略拒绝未解析目标 '{host}': {e}"),
                    )) as Box<dyn std::error::Error + Send + Sync>
                })?
                .collect();

            if resolved.is_empty() {
                return Err(Box::new(std::io::Error::new(
                    std::io::ErrorKind::AddrNotAvailable,
                    format!("安全策略拒绝无 DNS 解析结果的目标 '{host}'"),
                ))
                    as Box<dyn std::error::Error + Send + Sync>);
            }

            for addr in &resolved {
                if is_restricted_ip(addr.ip()) {
                    return Err(Box::new(std::io::Error::new(
                        std::io::ErrorKind::PermissionDenied,
                        format!(
                            "安全策略拦截了目标 '{host}' 解析到的内网/保留地址 '{}'",
                            addr.ip()
                        ),
                    ))
                        as Box<dyn std::error::Error + Send + Sync>);
                }
            }

            let addrs: Addrs = Box::new(resolved.into_iter());
            Ok(addrs)
        })
    }
}

/// Builds a redirect policy that enforces hop limit and static SSRF validation on every redirect target.
/// Actual DNS resolution and IP enforcement run asynchronously inside `SsrfSafeResolver` (no blocking DNS).
pub fn ssrf_redirect_policy(allow_private_ips: bool, max_hops: usize) -> reqwest::redirect::Policy {
    reqwest::redirect::Policy::custom(move |attempt| {
        if attempt.previous().len() >= max_hops {
            return attempt.error(std::io::Error::other(format!(
                "重定向次数超过上限 ({max_hops})"
            )));
        }
        if let Some(prev) = attempt.previous().last()
            && prev.scheme().eq_ignore_ascii_case("https")
            && attempt.url().scheme().eq_ignore_ascii_case("http")
        {
            return attempt.error(std::io::Error::new(
                std::io::ErrorKind::PermissionDenied,
                "禁止从 HTTPS 降级重定向至 HTTP",
            ));
        }
        if let Err(reason) = validate_target_url_static(attempt.url().as_str(), allow_private_ips) {
            return attempt.error(std::io::Error::new(
                std::io::ErrorKind::PermissionDenied,
                reason,
            ));
        }
        attempt.follow()
    })
}

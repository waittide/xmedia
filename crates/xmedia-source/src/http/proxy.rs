use std::collections::HashMap;
use std::net::IpAddr;

use reqwest::header::HeaderMap;
use url::Url;

/// Multi-tenant hosting / CDN public suffixes where two distinct tenants (`a.<suffix>` vs
/// `b.<suffix>`) must never be treated as sharing registrable-domain affinity.
const MULTI_TENANT_PUBLIC_SUFFIXES: &[&str] = &[
    "amazonaws.com",
    "aliyuncs.com",
    "appspot.com",
    "azurewebsites.net",
    "cloudfront.net",
    "fastly.net",
    "firebaseapp.com",
    "fly.dev",
    "github.io",
    "gitlab.io",
    "herokuapp.com",
    "myqcloud.com",
    "netlify.app",
    "onrender.com",
    "pages.dev",
    "r2.dev",
    "vercel.app",
    "web.app",
    "workers.dev",
];

fn is_common_cctld_second_level(label: &str) -> bool {
    matches!(
        label,
        "ac" | "ad"
            | "asso"
            | "biz"
            | "co"
            | "com"
            | "ed"
            | "edu"
            | "firm"
            | "gen"
            | "go"
            | "gov"
            | "gr"
            | "idv"
            | "ind"
            | "info"
            | "law"
            | "lg"
            | "med"
            | "mil"
            | "ne"
            | "net"
            | "nom"
            | "or"
            | "org"
            | "per"
            | "pp"
            | "pro"
            | "prv"
    )
}

fn parse_normalized_host(raw_url: &str, allow_bare_host: bool) -> Option<String> {
    let trimmed = raw_url.trim();
    if trimmed.is_empty() {
        return None;
    }

    let parsed = match Url::parse(trimmed) {
        Ok(u) => {
            if !matches!(u.scheme(), "http" | "https") {
                return None;
            }
            u
        }
        Err(_) if allow_bare_host && !trimmed.contains("://") => {
            Url::parse(&format!("https://{trimmed}")).ok()?
        }
        Err(_) => return None,
    };

    let host = parsed
        .host_str()?
        .trim_end_matches('.')
        .to_ascii_lowercase();
    if host.is_empty() {
        return None;
    }
    Some(host)
}

fn parse_ip_host(host: &str) -> Option<IpAddr> {
    host.trim_matches(|c| c == '[' || c == ']')
        .parse::<IpAddr>()
        .ok()
}

/// Computes the registrable domain (eTLD+1) of an already-lowercased, trailing-dot-stripped
/// DNS hostname. Returns `None` for IP literals, `localhost`/`.local`, single-label hosts,
/// or bare public suffixes themselves.
pub fn registrable_domain(host: &str) -> Option<&str> {
    if host.is_empty() || parse_ip_host(host).is_some() {
        return None;
    }
    if host == "localhost" || host.ends_with(".localhost") || host.ends_with(".local") {
        return None;
    }

    let labels: Vec<&str> = host.split('.').collect();
    if labels.len() < 2 || labels.iter().any(|l| l.is_empty()) {
        return None;
    }

    // Check multi-tenant hosting suffixes (e.g. `foo.github.io` -> `foo.github.io`, not `github.io`).
    for &suffix in MULTI_TENANT_PUBLIC_SUFFIXES {
        if host == suffix {
            return None;
        }
        if let Some(prefix) = host.strip_suffix(suffix)
            && prefix.ends_with('.')
        {
            let suffix_label_count = suffix.split('.').count();
            let needed = suffix_label_count + 1;
            if labels.len() < needed {
                return None;
            }
            let keep_from = labels.len() - needed;
            let byte_offset = host_tail_offset(host, &labels[keep_from..]);
            return Some(&host[byte_offset..]);
        }
    }

    let tld = labels[labels.len() - 1];
    let sld = labels[labels.len() - 2];
    let suffix_labels = if tld.len() == 2
        && tld.bytes().all(|b| b.is_ascii_alphabetic())
        && is_common_cctld_second_level(sld)
    {
        2
    } else {
        1
    };

    let needed = suffix_labels + 1;
    if labels.len() < needed {
        return None;
    }
    let keep_from = labels.len() - needed;
    let byte_offset = host_tail_offset(host, &labels[keep_from..]);
    Some(&host[byte_offset..])
}

fn host_tail_offset(host: &str, tail_labels: &[&str]) -> usize {
    let tail_len: usize =
        tail_labels.iter().map(|l| l.len()).sum::<usize>() + tail_labels.len().saturating_sub(1);
    host.len().saturating_sub(tail_len)
}

/// Returns `true` when `target_url` shares the exact host or registrable domain (eTLD+1) of
/// `source_url`. Fails closed (`false`) if either URL is invalid or non-HTTP(S).
pub fn has_origin_affinity(source_url: &str, target_url: &str) -> bool {
    let Some(source_host) = parse_normalized_host(source_url, true) else {
        return false;
    };
    let Some(target_host) = parse_normalized_host(target_url, false) else {
        return false;
    };

    match (parse_ip_host(&source_host), parse_ip_host(&target_host)) {
        (Some(src_ip), Some(dst_ip)) => return src_ip == dst_ip,
        (Some(_), None) | (None, Some(_)) => return false,
        (None, None) => {}
    }

    if source_host == target_host {
        return true;
    }

    match (
        registrable_domain(&source_host),
        registrable_domain(&target_host),
    ) {
        (Some(src_reg), Some(dst_reg)) => src_reg == dst_reg,
        _ => false,
    }
}

/// Returns `true` for credential-bearing static headers that must never be forwarded to a
/// third-party origin unrelated to the source's `source_url`.
pub fn is_sensitive_origin_header(name: &str) -> bool {
    matches!(
        name.trim().to_ascii_lowercase().as_str(),
        "authorization" | "cookie" | "cookie2" | "x-api-key" | "proxy-authorization"
    )
}

/// Strips sensitive authentication headers (`authorization`, `cookie`, `cookie2`, `x-api-key`,
/// `proxy-authorization`) whenever `target_url` does not match the host or registrable domain
/// of `source_url`. `proxy-authorization` is stripped unconditionally.
pub fn filter_headers_for_target_origin(
    source_url: &str,
    target_url: &str,
    headers: &HeaderMap,
) -> HeaderMap {
    let same_affinity = has_origin_affinity(source_url, target_url);
    let mut filtered = HeaderMap::with_capacity(headers.len());
    for (name, value) in headers {
        let name_str = name.as_str();
        if name_str.eq_ignore_ascii_case("proxy-authorization") {
            continue;
        }
        if !same_affinity && is_sensitive_origin_header(name_str) {
            continue;
        }
        filtered.append(name.clone(), value.clone());
    }
    filtered
}

/// In-place `HashMap<String, String>` counterpart of [`filter_headers_for_target_origin`].
pub fn strip_cross_origin_headers(
    headers: &mut HashMap<String, String>,
    source_url: &str,
    target_url: &str,
) {
    let same_affinity = has_origin_affinity(source_url, target_url);
    headers.retain(|k, _| {
        let trimmed = k.trim();
        !trimmed.eq_ignore_ascii_case("proxy-authorization")
            && (same_affinity || !is_sensitive_origin_header(trimmed))
    });
}

/// `HashMap<String, String>` counterpart of [`filter_headers_for_target_origin`].
pub fn filter_header_map_for_target_origin(
    source_url: &str,
    target_url: &str,
    headers: &HashMap<String, String>,
) -> HashMap<String, String> {
    let mut cloned = headers.clone();
    strip_cross_origin_headers(&mut cloned, source_url, target_url);
    cloned
}

#[cfg(test)]
mod tests {
    use reqwest::header::{AUTHORIZATION, COOKIE, HeaderValue, REFERER, USER_AGENT};

    use super::*;

    #[test]
    fn origin_affinity_matches_exact_host_and_registrable_domain_safely() {
        // Exact host & subdomain affinity
        assert!(has_origin_affinity(
            "https://manga.example.com",
            "https://img.cdn.example.com/1.jpg"
        ));
        assert!(has_origin_affinity(
            "manga.example.com",
            "https://example.com/cover.png"
        ));
        assert!(!has_origin_affinity(
            "https://example.com",
            "https://evil-example.com/cover.png"
        ));

        // Two-part ccTLD SLDs (.co.uk, .com.cn, .co.jp)
        assert!(has_origin_affinity(
            "https://www.site.co.uk",
            "https://img.site.co.uk/a.webp"
        ));
        assert!(!has_origin_affinity(
            "https://victim.co.uk",
            "https://attacker.co.uk/a.webp"
        ));
        assert!(has_origin_affinity(
            "https://api.music.com.cn",
            "https://cdn.music.com.cn/track.mp3"
        ));
        assert!(!has_origin_affinity(
            "https://victim.com.cn",
            "https://attacker.com.cn/track.mp3"
        ));
        // `co.com` (3-letter gTLD) remains a valid registrable domain
        assert!(has_origin_affinity(
            "https://a.co.com",
            "https://b.co.com/x.jpg"
        ));

        // Multi-tenant hosting suffixes isolate distinct tenants
        assert!(!has_origin_affinity(
            "https://alice.github.io",
            "https://bob.github.io/steal.png"
        ));
        assert!(has_origin_affinity(
            "https://alice.github.io/catalog",
            "https://sub.alice.github.io/img.png"
        ));
        assert!(!has_origin_affinity(
            "https://d111.cloudfront.net",
            "https://d222.cloudfront.net/v.m3u8"
        ));

        // IP literals require exact IP equality (never octet suffix matching)
        assert!(has_origin_affinity(
            "http://127.0.0.1:8080",
            "http://127.0.0.1:9090/cover.png"
        ));
        assert!(!has_origin_affinity(
            "http://10.0.1.5:8080",
            "http://192.168.1.5:8080/cover.png"
        ));
        assert!(has_origin_affinity(
            "http://[::1]:8080",
            "http://[::1]:3000/a.png"
        ));

        // Non-HTTP target schemes and empty URLs fail closed
        assert!(!has_origin_affinity(
            "https://example.com",
            "file:///etc/passwd"
        ));
        assert!(!has_origin_affinity("", "https://example.com/a.png"));
    }

    #[test]
    fn filter_headers_strips_credentials_cross_origin_and_preserves_same_domain() {
        let mut headers = HeaderMap::new();
        headers.insert(AUTHORIZATION, HeaderValue::from_static("Bearer top-secret"));
        headers.insert(COOKIE, HeaderValue::from_static("sid=secret-cookie"));
        headers.insert("x-api-key", HeaderValue::from_static("xmk_secret"));
        headers.insert(
            "proxy-authorization",
            HeaderValue::from_static("Basic cHJveHk6cGFzcw=="),
        );
        headers.insert(
            REFERER,
            HeaderValue::from_static("https://comic.example.com/"),
        );
        headers.insert(USER_AGENT, HeaderValue::from_static("CustomUA/1.0"));

        // Same registrable domain preserves Authorization, Cookie, X-Api-Key, Referer, UA
        let same = filter_headers_for_target_origin(
            "https://comic.example.com",
            "https://img.example.com/p1.jpg",
            &headers,
        );
        assert_eq!(same.get(AUTHORIZATION).unwrap(), "Bearer top-secret");
        assert_eq!(same.get(COOKIE).unwrap(), "sid=secret-cookie");
        assert_eq!(same.get("x-api-key").unwrap(), "xmk_secret");
        assert!(same.get("proxy-authorization").is_none());
        assert_eq!(same.get(REFERER).unwrap(), "https://comic.example.com/");
        assert_eq!(same.get(USER_AGENT).unwrap(), "CustomUA/1.0");

        // Cross-origin strips Authorization, Cookie, X-Api-Key, Proxy-Authorization, keeps Referer & UA
        let cross = filter_headers_for_target_origin(
            "https://comic.example.com",
            "https://attacker.other-cdn.org/p1.jpg",
            &headers,
        );
        assert!(cross.get(AUTHORIZATION).is_none());
        assert!(cross.get(COOKIE).is_none());
        assert!(cross.get("x-api-key").is_none());
        assert!(cross.get("proxy-authorization").is_none());
        assert_eq!(cross.get(REFERER).unwrap(), "https://comic.example.com/");
        assert_eq!(cross.get(USER_AGENT).unwrap(), "CustomUA/1.0");
    }
}

use std::collections::HashMap;
use std::net::{IpAddr, SocketAddr};
use std::sync::{Arc, LazyLock, Mutex};
use std::time::Duration;

use bytes::{Bytes, BytesMut};
use reqwest::header::{AUTHORIZATION, COOKIE, HeaderMap, LOCATION, PROXY_AUTHORIZATION};
use tokio::sync::mpsc;
use url::Url;
use xmedia_core::application::ports::ByteChunkStream;
use xmedia_core::domain::error::{DomainError, DomainResult};
use xmedia_core::infra::stream::MpscByteStream;
use xmedia_source::http::{
    DEFAULT_WEBVIEW_UA, MAX_REDIRECT_HOPS, SsrfSafeResolver, is_restricted_ip,
    ssrf_redirect_policy, validate_target_url, validate_target_url_static,
};
use xmedia_source::proxy::{
    MEDIA_PROLOG_SNIFF_MAX_BYTES, has_enough_media_sniff_bytes,
    has_unclosed_leading_xml_or_comment, is_only_bom_or_whitespace, strip_bom_and_ascii_ws,
};

use crate::infra::source::hls::{
    SNIFF_MIN_NON_WS_BYTES, has_binary_media_magic_or_null, is_generic_binary_ct,
    is_partial_hls_playlist_range_candidate, is_utf16_bom_prefix, looks_like_html_bytes,
};

pub const STREAM_CONNECT_TIMEOUT_SECS: u64 = 10;
pub const STREAM_READ_TIMEOUT_SECS: u64 = 30;
const MAX_CACHED_STREAMING_CLIENTS: usize = 16;

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct StreamingClientKey {
    allow_private_ips: bool,
    proxy_url: Option<String>,
    follow_redirects: bool,
}

static STREAMING_ASYNC_CLIENT_CACHE: LazyLock<Mutex<HashMap<StreamingClientKey, reqwest::Client>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

pub async fn validate_streaming_target_url(
    raw_url: &str,
    allow_private_ips: bool,
) -> DomainResult<Url> {
    validate_streaming_target_url_with_resolver(
        raw_url,
        allow_private_ips,
        |host, port| async move {
            let resolved: Vec<SocketAddr> = tokio::net::lookup_host((host.as_str(), port))
                .await
                .map_err(|e| format!("安全策略拒绝未解析目标 '{host}': {e}"))?
                .collect();
            Ok(resolved)
        },
    )
    .await
}

async fn validate_streaming_target_url_with_resolver<F, Fut>(
    raw_url: &str,
    allow_private_ips: bool,
    resolve_fn: F,
) -> DomainResult<Url>
where
    F: FnOnce(String, u16) -> Fut,
    Fut: std::future::Future<Output = Result<Vec<SocketAddr>, String>>,
{
    let parsed = validate_target_url_static(raw_url.trim(), allow_private_ips)
        .map_err(DomainError::Validation)?;
    if allow_private_ips {
        return Ok(parsed);
    }

    let host = parsed
        .host_str()
        .ok_or_else(|| DomainError::Validation(format!("URL 缺少主机名: {raw_url}")))?;
    let ip_candidate = host
        .trim_matches(|c| c == '[' || c == ']')
        .trim_end_matches('.');
    if ip_candidate.parse::<IpAddr>().is_ok() {
        return Ok(parsed);
    }

    let port = parsed.port_or_known_default().unwrap_or(80);
    let resolved = resolve_fn(host.to_string(), port)
        .await
        .map_err(DomainError::Validation)?;
    if resolved.is_empty() {
        return Err(DomainError::Validation(format!(
            "安全策略拒绝无 DNS 解析结果的目标 '{host}'"
        )));
    }
    for addr in &resolved {
        if is_restricted_ip(addr.ip()) {
            return Err(DomainError::Validation(format!(
                "安全策略拦截了目标 '{host}' 解析到的内网/保留地址 '{}'",
                addr.ip()
            )));
        }
    }
    Ok(parsed)
}

fn proxy_dns_ssrf_redirect_policy(
    allow_private_ips: bool,
    max_hops: usize,
) -> reqwest::redirect::Policy {
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
        if let Err(reason) = validate_target_url(attempt.url().as_str(), allow_private_ips) {
            return attempt.error(std::io::Error::new(
                std::io::ErrorKind::PermissionDenied,
                reason,
            ));
        }
        attempt.follow()
    })
}

fn get_or_build_streaming_async_client_inner(
    allow_private_ips: bool,
    proxy_url: Option<&str>,
    follow_redirects: bool,
) -> DomainResult<reqwest::Client> {
    let key = StreamingClientKey {
        allow_private_ips,
        proxy_url: proxy_url
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_string),
        follow_redirects,
    };
    {
        let guard = STREAMING_ASYNC_CLIENT_CACHE
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        if let Some(client) = guard.get(&key) {
            return Ok(client.clone());
        }
    }

    let redirect_policy = if !follow_redirects {
        reqwest::redirect::Policy::none()
    } else if key.proxy_url.is_some() && !key.allow_private_ips {
        proxy_dns_ssrf_redirect_policy(key.allow_private_ips, MAX_REDIRECT_HOPS)
    } else {
        ssrf_redirect_policy(key.allow_private_ips, MAX_REDIRECT_HOPS)
    };

    let mut builder = reqwest::Client::builder()
        .connect_timeout(Duration::from_secs(STREAM_CONNECT_TIMEOUT_SECS))
        .read_timeout(Duration::from_secs(STREAM_READ_TIMEOUT_SECS))
        .user_agent(DEFAULT_WEBVIEW_UA)
        .pool_max_idle_per_host(16)
        .pool_idle_timeout(Duration::from_secs(90))
        .redirect(redirect_policy);
    if let Some(ref proxy) = key.proxy_url {
        let p = reqwest::Proxy::all(proxy.as_str())
            .map_err(|e| DomainError::Storage(format!("代理配置无效: {e}")))?;
        builder = builder.proxy(p);
    } else {
        builder = builder.no_proxy();
        if !key.allow_private_ips {
            builder = builder.dns_resolver(Arc::new(SsrfSafeResolver));
        }
    }

    let client = builder
        .build()
        .map_err(|e| DomainError::Storage(format!("创建流媒体 HTTP 客户端失败: {e}")))?;

    let mut guard = STREAMING_ASYNC_CLIENT_CACHE
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    if guard.len() >= MAX_CACHED_STREAMING_CLIENTS && !guard.contains_key(&key) {
        guard.clear();
    }
    Ok(guard.entry(key).or_insert(client).clone())
}

pub async fn send_streaming_get(
    raw_url: &str,
    mut headers: HeaderMap,
    allow_private_ips: bool,
    proxy_url: Option<&str>,
    err_prefix: &str,
) -> DomainResult<reqwest::Response> {
    let has_proxy = proxy_url.map(str::trim).is_some_and(|s| !s.is_empty());
    let mut current_url = validate_streaming_target_url(raw_url, allow_private_ips).await?;

    if !has_proxy || allow_private_ips {
        let client = get_or_build_streaming_async_client_inner(allow_private_ips, proxy_url, true)?;
        return client
            .get(current_url)
            .headers(headers)
            .send()
            .await
            .map_err(|e| DomainError::Validation(format!("{err_prefix}: {e}")));
    }

    let client = get_or_build_streaming_async_client_inner(allow_private_ips, proxy_url, false)?;
    for hop in 0..=MAX_REDIRECT_HOPS {
        let resp = client
            .get(current_url.clone())
            .headers(headers.clone())
            .send()
            .await
            .map_err(|e| DomainError::Validation(format!("{err_prefix}: {e}")))?;

        if !resp.status().is_redirection() {
            return Ok(resp);
        }
        let Some(loc_val) = resp.headers().get(LOCATION) else {
            return Ok(resp);
        };
        if hop >= MAX_REDIRECT_HOPS {
            return Err(DomainError::Validation(format!(
                "{err_prefix}: 重定向次数超过上限 ({MAX_REDIRECT_HOPS})"
            )));
        }
        let loc_str = loc_val
            .to_str()
            .map_err(|_| DomainError::Validation(format!("{err_prefix}: 无效的重定向响应头")))?;
        let next_url = current_url
            .join(loc_str.trim())
            .map_err(|e| DomainError::Validation(format!("{err_prefix}: 无效的重定向目标: {e}")))?;
        if current_url.scheme().eq_ignore_ascii_case("https")
            && next_url.scheme().eq_ignore_ascii_case("http")
        {
            return Err(DomainError::Validation(
                "禁止从 HTTPS 降级重定向至 HTTP".into(),
            ));
        }
        let validated_next =
            validate_streaming_target_url(next_url.as_str(), allow_private_ips).await?;
        if (
            current_url.scheme(),
            current_url.host_str(),
            current_url.port_or_known_default(),
        ) != (
            validated_next.scheme(),
            validated_next.host_str(),
            validated_next.port_or_known_default(),
        ) {
            headers.remove(AUTHORIZATION);
            headers.remove(COOKIE);
            headers.remove(PROXY_AUTHORIZATION);
            headers.remove("x-api-key");
        }
        current_url = validated_next;
    }

    Err(DomainError::Validation(format!(
        "{err_prefix}: 重定向次数超过上限 ({MAX_REDIRECT_HOPS})"
    )))
}

pub async fn read_sniff_prefix_with_context(
    resp: &mut reqwest::Response,
    max_sniff_bytes: usize,
    is_partial: bool,
    ct_lower: &str,
    final_url: &Url,
) -> DomainResult<Bytes> {
    let mut buf = BytesMut::new();
    let might_be_text_or_partial_hls = is_partial
        || ct_lower.is_empty()
        || ct_lower == "text/plain"
        || is_generic_binary_ct(ct_lower);

    while let Some(chunk) = resp
        .chunk()
        .await
        .map_err(|e| DomainError::Validation(format!("读取上游媒体响应失败: {e}")))?
    {
        if chunk.is_empty() {
            continue;
        }
        buf.extend_from_slice(&chunk);
        if has_unclosed_leading_xml_or_comment(&buf) || is_only_bom_or_whitespace(&buf) {
            if buf.len() >= max_sniff_bytes.max(MEDIA_PROLOG_SNIFF_MAX_BYTES) {
                break;
            }
            continue;
        }
        if buf.len() >= max_sniff_bytes {
            break;
        }
        if might_be_text_or_partial_hls {
            let hls_ready = if is_partial {
                is_partial_hls_playlist_range_candidate(ct_lower, final_url, &buf)
            } else {
                let stripped = strip_bom_and_ascii_ws(&buf);
                let has_full_tag = stripped.starts_with(b"#EXTM3U")
                    || stripped.starts_with(b"#EXT-X-")
                    || stripped.starts_with(b"#EXTINF:");
                let ct_or_path_hls = ct_lower.contains("mpegurl")
                    || final_url.path().to_ascii_lowercase().ends_with(".m3u8")
                    || final_url.path().to_ascii_lowercase().ends_with(".m3u");
                has_full_tag
                    || (ct_or_path_hls
                        && has_enough_media_sniff_bytes(&buf, SNIFF_MIN_NON_WS_BYTES))
            };
            if has_binary_media_magic_or_null(&buf)
                || looks_like_html_bytes(&buf)
                || hls_ready
                || (is_utf16_bom_prefix(&buf)
                    && has_enough_media_sniff_bytes(&buf, SNIFF_MIN_NON_WS_BYTES))
            {
                break;
            }
        } else if has_enough_media_sniff_bytes(&buf, SNIFF_MIN_NON_WS_BYTES) {
            break;
        }
    }
    Ok(buf.freeze())
}

pub fn spawn_chunk_stream(
    mut resp: reqwest::Response,
    initial_chunk: Option<Bytes>,
) -> ByteChunkStream {
    let (tx, rx) = mpsc::channel::<DomainResult<Bytes>>(16);
    tokio::spawn(async move {
        if let Some(first) = initial_chunk
            && !first.is_empty()
            && tx.send(Ok(first)).await.is_err()
        {
            return;
        }
        loop {
            match resp.chunk().await {
                Ok(Some(chunk)) => {
                    if chunk.is_empty() {
                        continue;
                    }
                    if tx.send(Ok(chunk)).await.is_err() {
                        break;
                    }
                }
                Ok(None) => break,
                Err(err) => {
                    let _ = tx
                        .send(Err(DomainError::Storage(format!(
                            "upstream media stream error: {err}"
                        ))))
                        .await;
                    break;
                }
            }
        }
    });
    MpscByteStream::into_boxed(rx)
}

#[cfg(test)]
mod unit_tests {
    use std::net::{IpAddr, Ipv4Addr, SocketAddr};

    use super::*;

    #[tokio::test]
    async fn validate_streaming_target_url_blocks_dns_rebinding_to_private_ip() {
        let err = validate_streaming_target_url_with_resolver(
            "https://rebind.example.com/live/index.m3u8",
            false,
            |_host, port| async move {
                Ok(vec![SocketAddr::new(
                    IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)),
                    port,
                )])
            },
        )
        .await
        .expect_err("loopback DNS resolution must be blocked");
        assert!(err.to_string().contains("内网/保留地址"));

        let err_meta = validate_streaming_target_url_with_resolver(
            "http://metadata.example.com/latest/meta-data",
            false,
            |_host, port| async move {
                Ok(vec![SocketAddr::new(
                    IpAddr::V4(Ipv4Addr::new(169, 254, 169, 254)),
                    port,
                )])
            },
        )
        .await
        .expect_err("link-local metadata IP must be blocked");
        assert!(err_meta.to_string().contains("169.254.169.254"));
    }

    #[tokio::test]
    async fn send_streaming_get_enforces_dns_ssrf_even_when_proxy_is_configured() {
        let err = send_streaming_get(
            "http://127.0.0.1:8080/secret.m3u8",
            HeaderMap::new(),
            false,
            Some("http://127.0.0.1:7890"),
            "拉取 HLS 失败",
        )
        .await
        .expect_err("loopback literal IP must be blocked before proxy connect");
        assert!(err.to_string().contains("内网/保留地址"));

        let err_local_host = send_streaming_get(
            "http://internal.localhost/secret.m3u8",
            HeaderMap::new(),
            false,
            Some("http://127.0.0.1:7890"),
            "拉取 HLS 失败",
        )
        .await
        .expect_err("localhost subdomain must be blocked before proxy connect");
        assert!(err_local_host.to_string().contains("本地主机"));
    }
}

use std::collections::HashMap;
use std::sync::{Arc, LazyLock, Mutex};
use std::time::Duration;

use axum::http::header::{CONTENT_TYPE, IF_RANGE, RANGE, USER_AGENT};
use axum::http::{HeaderMap, HeaderValue, StatusCode};
use bytes::{Bytes, BytesMut};
use uuid::Uuid;
use xmedia_core::application::ports::{ByteChunkStream, ByteRangeSpec};
use xmedia_core::domain::error::{DomainError, DomainResult};
use xmedia_core::domain::user::User;
use xmedia_core::http::parse_http_range_spec;
use xmedia_core::infra::stream::MpscByteStream;
use xmedia_source::http::{
    DEFAULT_WEBVIEW_UA, HttpClientConfig, MAX_REDIRECT_HOPS, SsrfSafeResolver,
    build_sanitized_headers, normalize_cookie_key, ssrf_redirect_policy,
    validate_target_url_static,
};
use xmedia_source::proxy::{
    DEFAULT_IMAGE_PROXY_MAX_BYTES, DEFAULT_IMAGE_PROXY_TIMEOUT_SECS, MEDIA_PROLOG_SNIFF_MAX_BYTES,
    fetch_validated_image_bytes_with_config, has_enough_media_sniff_bytes,
    has_unclosed_leading_xml_or_comment, is_only_bom_or_whitespace, looks_like_html_or_svg_bytes,
};

use super::MusicOnlineService;
use crate::source::model::MusicSourceSpec;

const AUDIO_PROXY_CONNECT_TIMEOUT: Duration = Duration::from_secs(10);
const AUDIO_PROXY_READ_TIMEOUT: Duration = Duration::from_secs(30);
const AUDIO_SNIFF_PREFIX_MAX_BYTES: usize = 4096;
const AUDIO_SNIFF_MIN_NON_WS_BYTES: usize = 16;

static AUDIO_STREAMING_CLIENT_CACHE: LazyLock<Mutex<HashMap<bool, reqwest::Client>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

pub struct AudioProxyResponse {
    pub status: StatusCode,
    pub content_type: String,
    pub content_length: Option<String>,
    pub content_range: Option<String>,
    pub accept_ranges: Option<String>,
    pub stream: ByteChunkStream,
}

fn get_or_build_audio_streaming_client(allow_private_ips: bool) -> DomainResult<reqwest::Client> {
    {
        let guard = AUDIO_STREAMING_CLIENT_CACHE
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        if let Some(existing) = guard.get(&allow_private_ips) {
            return Ok(existing.clone());
        }
    }

    let mut builder = reqwest::Client::builder()
        .connect_timeout(AUDIO_PROXY_CONNECT_TIMEOUT)
        .read_timeout(AUDIO_PROXY_READ_TIMEOUT)
        // Intentionally NO `.timeout(...)` so long-lived audio streams never abort mid-song.
        .user_agent(DEFAULT_WEBVIEW_UA)
        .pool_max_idle_per_host(16)
        .pool_idle_timeout(Duration::from_secs(90))
        .redirect(ssrf_redirect_policy(allow_private_ips, MAX_REDIRECT_HOPS))
        .no_proxy();

    if !allow_private_ips {
        builder = builder.dns_resolver(Arc::new(SsrfSafeResolver));
    }

    let client = builder
        .build()
        .map_err(|e| DomainError::Storage(format!("创建音频流代理客户端失败: {e}")))?;

    let mut guard = AUDIO_STREAMING_CLIENT_CACHE
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    Ok(guard.entry(allow_private_ips).or_insert(client).clone())
}

fn lookup_cookie_for_url(cookies: &HashMap<String, String>, target_url: &str) -> Option<String> {
    let key = normalize_cookie_key(target_url);
    if key.is_empty() {
        return None;
    }
    if let Some(v) = cookies.get(&key)
        && !v.trim().is_empty()
    {
        return Some(v.clone());
    }
    let mut best: Option<(&str, &str)> = None;
    for (stored_host, val) in cookies {
        let clean_stored = stored_host.trim_start_matches('.');
        if clean_stored.is_empty() || val.trim().is_empty() {
            continue;
        }
        if key.len() > clean_stored.len()
            && key.ends_with(clean_stored)
            && key.as_bytes()[key.len() - clean_stored.len() - 1] == b'.'
        {
            let replace = match best {
                None => true,
                Some((prev_host, _)) => {
                    clean_stored.len() > prev_host.len()
                        || (clean_stored.len() == prev_host.len() && clean_stored < prev_host)
                }
            };
            if replace {
                best = Some((clean_stored, val.as_str()));
            }
        }
    }
    best.map(|(_, v)| v.to_string())
}

fn is_allowed_audio_content_type(ct_lower: &str) -> bool {
    ct_lower.is_empty()
        || ct_lower.starts_with("audio/")
        || matches!(
            ct_lower,
            "application/octet-stream"
                | "binary/octet-stream"
                | "application/ogg"
                | "video/mp4"
                | "application/mp4"
        )
}

async fn read_audio_sniff_prefix(
    resp: &mut reqwest::Response,
    max_sniff_bytes: usize,
) -> DomainResult<Bytes> {
    let mut buf = BytesMut::new();
    while let Some(chunk) = resp
        .chunk()
        .await
        .map_err(|e| DomainError::Validation(format!("拉取音频流失败: {e}")))?
    {
        if chunk.is_empty() {
            continue;
        }
        buf.extend_from_slice(&chunk);
        let sniff_limit =
            if has_unclosed_leading_xml_or_comment(&buf) || is_only_bom_or_whitespace(&buf) {
                max_sniff_bytes.max(MEDIA_PROLOG_SNIFF_MAX_BYTES)
            } else {
                max_sniff_bytes
            };
        if buf.len() >= sniff_limit
            || has_enough_media_sniff_bytes(&buf, AUDIO_SNIFF_MIN_NON_WS_BYTES)
        {
            break;
        }
    }
    Ok(buf.freeze())
}

fn parse_total_size_from_content_range(header: Option<&HeaderValue>) -> u64 {
    let Some(raw) = header.and_then(|v| v.to_str().ok()) else {
        return 0;
    };
    raw.trim()
        .rsplit_once('/')
        .and_then(|(_, total)| total.trim().parse::<u64>().ok())
        .unwrap_or(0)
}

impl MusicOnlineService {
    async fn build_source_media_headers(
        &self,
        actor: &User,
        source_id: Option<Uuid>,
        target_url: &str,
    ) -> DomainResult<HashMap<String, String>> {
        let Some(sid) = source_id else {
            return Ok(HashMap::new());
        };
        let rec = self.load_source_for_actor(actor, sid).await?;
        let Ok(spec) = serde_json::from_str::<MusicSourceSpec>(&rec.raw_json) else {
            return Ok(HashMap::new());
        };
        let mut merged = spec.parse_media_headers();
        // Never allow static source config to override client Range requests.
        merged.retain(|k, _| !k.eq_ignore_ascii_case("range"));

        if !merged.keys().any(|k| k.eq_ignore_ascii_case("cookie"))
            && spec.enabled_cookie_jar
            && let Some(cookie) = lookup_cookie_for_url(&rec.runtime_state.cookies, target_url)
        {
            merged.insert("Cookie".to_string(), cookie);
        }
        Ok(merged)
    }

    pub async fn fetch_cover_proxy(
        &self,
        actor: &User,
        raw_url: &str,
        source_id: Option<Uuid>,
    ) -> DomainResult<(Vec<u8>, &'static str)> {
        let trimmed = raw_url.trim();
        let headers = self
            .build_source_media_headers(actor, source_id, trimmed)
            .await?;
        let header_opt = (!headers.is_empty()).then_some(&headers);
        let cfg = HttpClientConfig {
            timeout_secs: DEFAULT_IMAGE_PROXY_TIMEOUT_SECS,
            allow_private_ips: self.allow_private_ips,
            proxy_url: None,
        };
        fetch_validated_image_bytes_with_config(
            trimmed,
            header_opt,
            DEFAULT_IMAGE_PROXY_MAX_BYTES,
            &cfg,
        )
        .await
    }

    pub async fn stream_audio_proxy(
        &self,
        actor: &User,
        raw_url: &str,
        source_id: Option<Uuid>,
        client_headers: &HeaderMap,
    ) -> DomainResult<AudioProxyResponse> {
        let trimmed = raw_url.trim();
        let validated_url = validate_target_url_static(trimmed, self.allow_private_ips)
            .map_err(DomainError::Validation)?;

        let source_headers = self
            .build_source_media_headers(actor, source_id, validated_url.as_str())
            .await?;
        let mut out_headers = build_sanitized_headers(&source_headers);
        if !out_headers.contains_key(USER_AGENT) {
            out_headers.insert(USER_AGENT, HeaderValue::from_static(DEFAULT_WEBVIEW_UA));
        }

        let range_spec = parse_http_range_spec(client_headers);
        if range_spec == ByteRangeSpec::Malformed {
            return Err(DomainError::Validation("无效的 Range 请求头".into()));
        }
        if let Some(range_val) = client_headers.get(RANGE)
            && range_spec != ByteRangeSpec::Full
            && let Ok(s) = range_val.to_str()
            && let Ok(hv) = HeaderValue::from_str(s.trim())
        {
            out_headers.insert(RANGE, hv);
            if let Some(if_range) = client_headers.get(IF_RANGE)
                && let Ok(ir_s) = if_range.to_str()
                && let Ok(ir_hv) = HeaderValue::from_str(ir_s.trim())
            {
                out_headers.insert(IF_RANGE, ir_hv);
            }
        }

        let client = get_or_build_audio_streaming_client(self.allow_private_ips)?;
        let send_res = tokio::time::timeout(
            AUDIO_PROXY_READ_TIMEOUT,
            client.get(validated_url).headers(out_headers).send(),
        )
        .await;

        let mut resp = match send_res {
            Ok(Ok(r)) => r,
            Ok(Err(e)) => {
                return Err(DomainError::Validation(format!("拉取音频流失败: {e}")));
            }
            Err(_) => {
                return Err(DomainError::Validation("上游音频请求超时".into()));
            }
        };

        let status = resp.status();
        match status {
            StatusCode::OK | StatusCode::PARTIAL_CONTENT => {}
            StatusCode::RANGE_NOT_SATISFIABLE => {
                let total_size =
                    parse_total_size_from_content_range(resp.headers().get("content-range"));
                return Err(DomainError::RangeNotSatisfiable { total_size });
            }
            StatusCode::NOT_FOUND | StatusCode::GONE => {
                return Err(DomainError::NotFound("上游音频资源不存在".into()));
            }
            StatusCode::TOO_MANY_REQUESTS => {
                return Err(DomainError::TooManyRequests(
                    "上游音频源请求过于频繁".into(),
                ));
            }
            other => {
                return Err(DomainError::Validation(format!(
                    "上游音频响应状态异常: {other}"
                )));
            }
        }

        let raw_ct = resp
            .headers()
            .get(CONTENT_TYPE)
            .and_then(|v| v.to_str().ok())
            .map(str::trim)
            .unwrap_or("");
        let base_ct = raw_ct
            .split(';')
            .next()
            .unwrap_or(raw_ct)
            .trim()
            .to_ascii_lowercase();
        if !is_allowed_audio_content_type(&base_ct) {
            return Err(DomainError::UnsupportedFormat(format!(
                "不支持的音频内容类型: {base_ct}"
            )));
        }

        let content_type = if raw_ct.is_empty() {
            "application/octet-stream".to_string()
        } else {
            raw_ct.to_string()
        };
        let content_length = resp
            .headers()
            .get("content-length")
            .and_then(|v| v.to_str().ok())
            .map(|s| s.trim().to_string());
        let content_range = resp
            .headers()
            .get("content-range")
            .and_then(|v| v.to_str().ok())
            .map(|s| s.trim().to_string());
        let accept_ranges = resp
            .headers()
            .get("accept-ranges")
            .and_then(|v| v.to_str().ok())
            .map(|s| s.trim().to_string())
            .or_else(|| (status == StatusCode::PARTIAL_CONTENT).then(|| "bytes".to_string()));

        let prefix = read_audio_sniff_prefix(&mut resp, AUDIO_SNIFF_PREFIX_MAX_BYTES).await?;
        if looks_like_html_or_svg_bytes(&prefix) {
            return Err(DomainError::Validation(
                "拒绝转发伪装为音频的 HTML/SVG 内容".into(),
            ));
        }

        let initial = (!prefix.is_empty()).then_some(prefix);
        let (tx, rx) = tokio::sync::mpsc::channel::<DomainResult<Bytes>>(16);
        tokio::spawn(async move {
            if let Some(first) = initial
                && tx.send(Ok(first)).await.is_err()
            {
                return;
            }
            loop {
                tokio::select! {
                    biased;
                    _ = tx.closed() => break,
                    chunk_res = resp.chunk() => match chunk_res {
                        Ok(Some(chunk)) => {
                            if chunk.is_empty() {
                                continue;
                            }
                            if tx.send(Ok(chunk)).await.is_err() {
                                break;
                            }
                        }
                        Ok(None) => break,
                        Err(e) => {
                            tracing::warn!(error = %e, "audio-proxy upstream stream read error");
                            let _ = tx
                                .send(Err(DomainError::Storage(format!("音频流读取失败: {e}"))))
                                .await;
                            break;
                        }
                    }
                }
            }
        });

        Ok(AudioProxyResponse {
            status,
            content_type,
            content_length,
            content_range,
            accept_ranges,
            stream: MpscByteStream::into_boxed(rx),
        })
    }
}

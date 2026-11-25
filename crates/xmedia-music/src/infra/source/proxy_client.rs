use std::collections::HashMap;
use std::path::Path;
use std::sync::{Arc, LazyLock, Mutex};
use std::time::Duration;

use async_trait::async_trait;
use axum::http::header::{CONTENT_TYPE, IF_RANGE, RANGE, USER_AGENT};
use axum::http::{HeaderMap, HeaderValue, StatusCode};
use bytes::{Bytes, BytesMut};
use tokio::io::AsyncWriteExt;
use uuid::Uuid;
use xmedia_core::application::ports::ByteRangeSpec;
use xmedia_core::domain::error::{DomainError, DomainResult};
use xmedia_core::http::parse_http_range_spec;
use xmedia_core::infra::stream::MpscByteStream;
use xmedia_source::http::{
    DEFAULT_WEBVIEW_UA, HttpClientConfig, MAX_REDIRECT_HOPS, SsrfSafeResolver,
    build_sanitized_headers, ssrf_redirect_policy, validate_target_url_static,
};
use xmedia_source::proxy::{
    DEFAULT_IMAGE_PROXY_MAX_BYTES, DEFAULT_IMAGE_PROXY_TIMEOUT_SECS, MEDIA_PROLOG_SNIFF_MAX_BYTES,
    fetch_validated_image_bytes_with_config, has_enough_media_sniff_bytes,
    has_unclosed_leading_xml_or_comment, is_only_bom_or_whitespace, limit_byte_stream,
    looks_like_html_or_svg_bytes, validate_proxy_content_length,
};

use crate::application::ports::{
    AudioProxyResponse, DownloadedTempAudio, MusicMediaProxyPort, TempAudioFile,
    UpstreamAudioProxyRequest,
};
use crate::domain::online::MusicSourceSpec;

const AUDIO_PROXY_CONNECT_TIMEOUT: Duration = Duration::from_secs(10);
const AUDIO_PROXY_READ_TIMEOUT: Duration = Duration::from_secs(30);
const AUDIO_SNIFF_PREFIX_MAX_BYTES: usize = 4096;
const AUDIO_SNIFF_MIN_NON_WS_BYTES: usize = 16;

static AUDIO_STREAMING_CLIENT_CACHE: LazyLock<Mutex<HashMap<bool, reqwest::Client>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

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

fn parse_content_length_header(header: Option<&HeaderValue>) -> DomainResult<Option<u64>> {
    let Some(hv) = header else {
        return Ok(None);
    };
    let raw = hv
        .to_str()
        .map_err(|_| DomainError::Validation("上游音频 Content-Length 头无效".into()))?
        .trim();
    if raw.is_empty() {
        return Ok(None);
    }
    let parsed = raw
        .parse::<u64>()
        .map_err(|_| DomainError::Validation(format!("上游音频 Content-Length 无效: {raw}")))?;
    Ok(Some(parsed))
}

async fn open_exclusive_temp_file(path: &Path) -> std::io::Result<tokio::fs::File> {
    let mut opts = tokio::fs::OpenOptions::new();
    opts.write(true).create_new(true);
    #[cfg(unix)]
    opts.mode(0o600);
    opts.open(path).await
}

async fn create_exclusive_temp_audio_file(
    ext: &str,
) -> DomainResult<(TempAudioFile, tokio::fs::File)> {
    let clean_ext: String = ext
        .chars()
        .filter(|c| c.is_ascii_alphanumeric())
        .take(8)
        .collect();
    let suffix = if clean_ext.is_empty() {
        "tmp".to_string()
    } else {
        clean_ext
    };
    for _ in 0..8 {
        let path = std::env::temp_dir().join(format!("xmedia-music-{}.{suffix}", Uuid::now_v7()));
        match open_exclusive_temp_file(&path).await {
            Ok(file) => return Ok((TempAudioFile::new(path), file)),
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(e) => {
                return Err(DomainError::Storage(format!(
                    "创建临时音频缓存文件失败: {e}"
                )));
            }
        }
    }
    Err(DomainError::Storage(
        "创建独占临时音频缓存文件失败: 重试次数耗尽".into(),
    ))
}

#[derive(Debug, Clone, Default)]
pub struct HttpMusicMediaProxy {
    allow_private_ips: bool,
}

impl HttpMusicMediaProxy {
    pub fn new(allow_private_ips: bool) -> Self {
        Self { allow_private_ips }
    }
}

#[async_trait]
impl MusicMediaProxyPort for HttpMusicMediaProxy {
    fn has_effective_proxy_headers(&self, spec: &MusicSourceSpec) -> bool {
        let mut merged = spec.parse_media_headers();
        merged.retain(|k, _| !k.eq_ignore_ascii_case("range"));
        !build_sanitized_headers(&merged).is_empty()
    }

    async fn fetch_cover_image(
        &self,
        url: &str,
        headers: Option<&HashMap<String, String>>,
    ) -> DomainResult<(Vec<u8>, &'static str)> {
        let cfg = HttpClientConfig {
            timeout_secs: DEFAULT_IMAGE_PROXY_TIMEOUT_SECS,
            allow_private_ips: self.allow_private_ips,
            proxy_url: None,
        };
        fetch_validated_image_bytes_with_config(url, headers, DEFAULT_IMAGE_PROXY_MAX_BYTES, &cfg)
            .await
    }

    async fn open_audio_proxy_stream(
        &self,
        req: UpstreamAudioProxyRequest<'_>,
    ) -> DomainResult<AudioProxyResponse> {
        let validated_url =
            validate_target_url_static(req.target_url.trim(), self.allow_private_ips)
                .map_err(DomainError::Validation)?;

        let mut out_headers = build_sanitized_headers(req.headers);
        if !out_headers.contains_key(USER_AGENT) {
            out_headers.insert(USER_AGENT, HeaderValue::from_static(DEFAULT_WEBVIEW_UA));
        }

        if let Some(raw_range) = req.range_header.map(str::trim).filter(|s| !s.is_empty()) {
            let mut probe_map = HeaderMap::new();
            let hv = HeaderValue::from_str(raw_range)
                .map_err(|_| DomainError::Validation("无效的 Range 请求头".into()))?;
            probe_map.insert(RANGE, hv.clone());
            let spec = parse_http_range_spec(&probe_map);
            if spec == ByteRangeSpec::Malformed {
                return Err(DomainError::Validation("无效的 Range 请求头".into()));
            }
            if spec != ByteRangeSpec::Full {
                out_headers.insert(RANGE, hv);
                if let Some(ir) = req.if_range_header.map(str::trim).filter(|s| !s.is_empty())
                    && let Ok(ir_hv) = HeaderValue::from_str(ir)
                {
                    out_headers.insert(IF_RANGE, ir_hv);
                }
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

        let content_length = parse_content_length_header(resp.headers().get("content-length"))?;
        validate_proxy_content_length(content_length, req.max_bytes, "音频流")?;

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
            is_partial: status == StatusCode::PARTIAL_CONTENT,
            content_type,
            content_length,
            content_range,
            accept_ranges,
            stream: limit_byte_stream(MpscByteStream::new(rx), req.max_bytes),
        })
    }

    async fn download_audio_to_temp_file(
        &self,
        url: &str,
        headers: &HashMap<String, String>,
        ext_hint: &str,
        max_bytes: u64,
    ) -> DomainResult<DownloadedTempAudio> {
        let validated_url = validate_target_url_static(url.trim(), self.allow_private_ips)
            .map_err(DomainError::Validation)?;
        let mut out_headers = build_sanitized_headers(headers);
        if !out_headers.contains_key(USER_AGENT) {
            out_headers.insert(USER_AGENT, HeaderValue::from_static(DEFAULT_WEBVIEW_UA));
        }

        let client = get_or_build_audio_streaming_client(self.allow_private_ips)?;
        let mut resp = tokio::time::timeout(
            AUDIO_PROXY_READ_TIMEOUT,
            client.get(validated_url).headers(out_headers).send(),
        )
        .await
        .map_err(|_| DomainError::Validation("上游音频下载连接超时".into()))?
        .map_err(|e| DomainError::Validation(format!("拉取在线音频失败: {e}")))?;

        let status = resp.status();
        if status != StatusCode::OK && status != StatusCode::PARTIAL_CONTENT {
            return Err(DomainError::Validation(format!(
                "上游音频响应状态异常: {status}"
            )));
        }

        let declared_len = parse_content_length_header(resp.headers().get("content-length"))?;
        validate_proxy_content_length(declared_len, max_bytes, "在线音频")?;

        let raw_ct = resp
            .headers()
            .get(CONTENT_TYPE)
            .and_then(|v| v.to_str().ok())
            .map(str::trim)
            .unwrap_or("")
            .to_string();
        let base_ct = raw_ct
            .split(';')
            .next()
            .unwrap_or(&raw_ct)
            .trim()
            .to_ascii_lowercase();
        if !is_allowed_audio_content_type(&base_ct) {
            return Err(DomainError::UnsupportedFormat(format!(
                "不支持的音频内容类型: {base_ct}"
            )));
        }

        let prefix = read_audio_sniff_prefix(&mut resp, AUDIO_SNIFF_PREFIX_MAX_BYTES).await?;
        if looks_like_html_or_svg_bytes(&prefix) {
            return Err(DomainError::Validation(
                "上游返回了伪装为音频的 HTML/SVG 内容".into(),
            ));
        }

        let (temp_file, mut file) = create_exclusive_temp_audio_file(ext_hint).await?;

        let mut total_bytes: u64 = 0;
        if !prefix.is_empty() {
            total_bytes = total_bytes.saturating_add(prefix.len() as u64);
            if total_bytes > max_bytes {
                return Err(DomainError::Validation(format!(
                    "在线音频体积超过上限 ({} MiB)",
                    max_bytes / (1024 * 1024)
                )));
            }
            file.write_all(&prefix)
                .await
                .map_err(|e| DomainError::Storage(format!("写入临时音频缓存失败: {e}")))?;
        }

        while let Some(chunk) = resp
            .chunk()
            .await
            .map_err(|e| DomainError::Validation(format!("读取上游音频流失败: {e}")))?
        {
            if chunk.is_empty() {
                continue;
            }
            total_bytes = total_bytes.saturating_add(chunk.len() as u64);
            if total_bytes > max_bytes {
                return Err(DomainError::Validation(format!(
                    "在线音频体积超过上限 ({} MiB)",
                    max_bytes / (1024 * 1024)
                )));
            }
            file.write_all(&chunk)
                .await
                .map_err(|e| DomainError::Storage(format!("写入临时音频缓存失败: {e}")))?;
        }

        file.flush()
            .await
            .map_err(|e| DomainError::Storage(format!("刷新临时音频缓存失败: {e}")))?;
        drop(file);

        if total_bytes == 0 {
            return Err(DomainError::Validation("上游返回的音频内容为空".into()));
        }

        let content_type = (!raw_ct.is_empty()).then_some(raw_ct);
        Ok(DownloadedTempAudio {
            temp_file,
            content_type,
            total_bytes,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn temp_file_guard_creates_exclusive_owner_only_temp_file_and_cleans_up() {
        let (guard, mut file) = create_exclusive_temp_audio_file("mp3").await.unwrap();
        let path = guard.path().to_path_buf();
        file.write_all(b"audio-bytes").await.unwrap();
        drop(file);
        assert!(path.is_file());

        let err = open_exclusive_temp_file(&path).await.unwrap_err();
        assert_eq!(err.kind(), std::io::ErrorKind::AlreadyExists);

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(&path).unwrap().permissions().mode() & 0o777;
            assert_eq!(mode, 0o600);
        }

        drop(guard);
        assert!(!path.exists());
    }
}

use std::collections::HashMap;
use std::path::Path;

use async_trait::async_trait;
use reqwest::header::{ACCEPT_ENCODING, CONTENT_TYPE, HeaderValue, RANGE, USER_AGENT};
use url::Url;
use xmedia_core::domain::error::{DomainError, DomainResult};
use xmedia_source::http::{
    DEFAULT_WEBVIEW_UA, HttpClientConfig, build_sanitized_headers, decode_response_bytes,
};
use xmedia_source::proxy::{
    DEFAULT_IMAGE_PROXY_MAX_BYTES, DEFAULT_SUBTITLE_PROXY_MAX_BYTES, DEFAULT_VIDEO_PROXY_MAX_BYTES,
    fetch_image, limit_byte_stream, validate_proxy_content_length,
};

use crate::application::ports::{
    HlsProxyOutcome, ProxyStreamOutcome, ProxyStreamPayload, VideoStreamTransportPort,
};
use crate::domain::video::VideoContainer;
use crate::infra::source::hls::{
    MAX_M3U8_BYTES, SNIFF_PREFIX_MAX_BYTES, infer_segment_content_type, is_forbidden_media_ct,
    is_forbidden_subtitle_ct, is_hls_playlist_response, is_partial_hls_playlist_range_candidate,
    looks_like_html_bytes, rewrite_m3u8_playlist_with, validate_and_normalize_subtitle_to_vtt,
};
use crate::infra::source::streaming_client::{
    read_sniff_prefix_with_context, send_streaming_get, spawn_chunk_stream,
};

fn validate_range_header(raw: Option<&str>) -> DomainResult<Option<String>> {
    let Some(val) = raw.map(str::trim).filter(|s| !s.is_empty()) else {
        return Ok(None);
    };
    if !val.to_ascii_lowercase().starts_with("bytes=") || val.bytes().any(|b| b.is_ascii_control())
    {
        return Err(DomainError::Validation("无效的 Range 请求头".into()));
    }
    Ok(Some(val.to_string()))
}

#[derive(Debug, Clone)]
pub struct HttpVideoStreamTransport {
    http_config: HttpClientConfig,
    video_proxy_max_bytes: u64,
    subtitle_proxy_max_bytes: u64,
}

impl Default for HttpVideoStreamTransport {
    fn default() -> Self {
        Self::new(HttpClientConfig::default())
    }
}

impl HttpVideoStreamTransport {
    pub fn new(http_config: HttpClientConfig) -> Self {
        Self {
            http_config,
            video_proxy_max_bytes: DEFAULT_VIDEO_PROXY_MAX_BYTES,
            subtitle_proxy_max_bytes: DEFAULT_SUBTITLE_PROXY_MAX_BYTES,
        }
    }

    #[cfg(test)]
    pub(crate) fn with_max_bytes(mut self, video_max: u64, subtitle_max: u64) -> Self {
        self.video_proxy_max_bytes = video_max;
        self.subtitle_proxy_max_bytes = subtitle_max;
        self
    }
}

#[async_trait]
impl VideoStreamTransportPort for HttpVideoStreamTransport {
    async fn fetch_poster_bytes(
        &self,
        raw_url: &str,
        source_headers: &HashMap<String, String>,
    ) -> DomainResult<(Vec<u8>, &'static str)> {
        fetch_image(
            raw_url,
            (!source_headers.is_empty()).then_some(source_headers),
            DEFAULT_IMAGE_PROXY_MAX_BYTES,
            &self.http_config,
        )
        .await
    }

    async fn fetch_subtitle_vtt(
        &self,
        raw_url: &str,
        source_headers: &HashMap<String, String>,
    ) -> DomainResult<String> {
        let mut header_map = build_sanitized_headers(source_headers);
        if !header_map.contains_key(USER_AGENT) {
            header_map.insert(USER_AGENT, HeaderValue::from_static(DEFAULT_WEBVIEW_UA));
        }

        let resp = send_streaming_get(
            raw_url.trim(),
            header_map,
            self.http_config.allow_private_ips,
            self.http_config.proxy_url.as_deref(),
            "拉取字幕失败",
        )
        .await?;

        if !resp.status().is_success() {
            return Err(DomainError::Validation(format!(
                "字幕响应状态异常: {}",
                resp.status()
            )));
        }

        let header_ct = resp
            .headers()
            .get(CONTENT_TYPE)
            .and_then(|v| v.to_str().ok())
            .map(ToString::to_string);
        if let Some(ref ct) = header_ct
            && is_forbidden_subtitle_ct(&ct.to_ascii_lowercase())
        {
            return Err(DomainError::Validation(format!(
                "拒绝转发不安全的字幕内容类型: {ct}"
            )));
        }

        validate_proxy_content_length(
            resp.content_length(),
            self.subtitle_proxy_max_bytes,
            "字幕文件",
        )?;

        let mut limited = limit_byte_stream(
            spawn_chunk_stream(resp, None),
            self.subtitle_proxy_max_bytes,
        );
        let mut buf = Vec::new();
        while let Some(item) = std::future::poll_fn(|cx| limited.as_mut().poll_next(cx)).await {
            let chunk = item?;
            buf.extend_from_slice(&chunk);
        }

        validate_and_normalize_subtitle_to_vtt(&buf, header_ct.as_deref())
    }

    async fn open_stream_proxy(
        &self,
        raw_url: &str,
        source_headers: HashMap<String, String>,
        range_header: Option<&str>,
    ) -> DomainResult<ProxyStreamOutcome> {
        let sanitized_range = validate_range_header(range_header)?;

        let mut header_map = build_sanitized_headers(&source_headers);
        if !header_map.contains_key(USER_AGENT) {
            header_map.insert(USER_AGENT, HeaderValue::from_static(DEFAULT_WEBVIEW_UA));
        }
        header_map.insert(ACCEPT_ENCODING, HeaderValue::from_static("identity"));
        if let Some(ref rng) = sanitized_range
            && let Ok(hv) = HeaderValue::from_str(rng)
        {
            header_map.insert(RANGE, hv);
        }

        let mut resp = send_streaming_get(
            raw_url.trim(),
            header_map,
            self.http_config.allow_private_ips,
            self.http_config.proxy_url.as_deref(),
            "拉取视频流失败",
        )
        .await?;

        let status = resp.status();
        if status == reqwest::StatusCode::RANGE_NOT_SATISFIABLE {
            let content_range = resp
                .headers()
                .get(reqwest::header::CONTENT_RANGE)
                .and_then(|v| v.to_str().ok())
                .map(ToString::to_string);
            return Ok(ProxyStreamOutcome::RangeNotSatisfiable { content_range });
        }
        if !status.is_success() {
            return Err(DomainError::Validation(format!(
                "视频流响应状态异常: {status}"
            )));
        }

        let header_ct = resp
            .headers()
            .get(CONTENT_TYPE)
            .and_then(|v| v.to_str().ok())
            .map(ToString::to_string);
        if let Some(ref ct) = header_ct
            && is_forbidden_media_ct(&ct.to_ascii_lowercase())
        {
            return Err(DomainError::Validation(format!(
                "拒绝转发不安全的媒体内容类型: {ct}"
            )));
        }

        let content_type = infer_segment_content_type(header_ct.as_deref(), resp.url());
        let content_length = resp.content_length();
        validate_proxy_content_length(content_length, self.video_proxy_max_bytes, "代理视频流")?;

        let content_range = resp
            .headers()
            .get(reqwest::header::CONTENT_RANGE)
            .and_then(|v| v.to_str().ok())
            .map(ToString::to_string);
        let accept_ranges = resp
            .headers()
            .get(reqwest::header::ACCEPT_RANGES)
            .and_then(|v| v.to_str().ok())
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .unwrap_or("bytes")
            .to_string();

        let final_url = resp.url().clone();
        let ct_lower = header_ct
            .as_deref()
            .map(|s| s.split(';').next().unwrap_or(s).trim().to_ascii_lowercase())
            .unwrap_or_default();
        let prefix = read_sniff_prefix_with_context(
            &mut resp,
            SNIFF_PREFIX_MAX_BYTES,
            status == reqwest::StatusCode::PARTIAL_CONTENT,
            &ct_lower,
            &final_url,
        )
        .await?;
        if looks_like_html_bytes(&prefix) {
            return Err(DomainError::Validation(
                "拒绝转发伪装为媒体的 HTML/SVG 内容".into(),
            ));
        }
        if is_hls_playlist_response(&ct_lower, &final_url, &prefix)
            || (status == reqwest::StatusCode::PARTIAL_CONTENT
                && is_partial_hls_playlist_range_candidate(&ct_lower, &final_url, &prefix))
        {
            return Err(DomainError::Validation(
                "拒绝通过直链媒体代理转发 HLS 播放列表，请改用 hls-proxy".into(),
            ));
        }
        let initial = (!prefix.is_empty()).then_some(prefix);
        let stream = limit_byte_stream(
            spawn_chunk_stream(resp, initial),
            self.video_proxy_max_bytes,
        );

        Ok(ProxyStreamOutcome::Stream(ProxyStreamPayload {
            status: status.as_u16(),
            content_type,
            content_length,
            content_range,
            accept_ranges,
            stream,
        }))
    }

    async fn open_hls_proxy(
        &self,
        raw_url: &str,
        source_headers: HashMap<String, String>,
        range_header: Option<&str>,
        rewrite_uri: &(dyn for<'a> Fn(&'a str) -> String + Send + Sync),
    ) -> DomainResult<HlsProxyOutcome> {
        let sanitized_range = validate_range_header(range_header)?;

        let mut header_map = build_sanitized_headers(&source_headers);
        if !header_map.contains_key(USER_AGENT) {
            header_map.insert(USER_AGENT, HeaderValue::from_static(DEFAULT_WEBVIEW_UA));
        }
        header_map.insert(ACCEPT_ENCODING, HeaderValue::from_static("identity"));

        let mut include_range = sanitized_range.is_some();
        let (
            resp,
            status,
            header_ct,
            ct_lower,
            final_url,
            content_length,
            content_range,
            accept_ranges,
            prefix,
        ) = loop {
            let mut req_headers = header_map.clone();
            if include_range
                && let Some(ref rng) = sanitized_range
                && let Ok(hv) = HeaderValue::from_str(rng)
            {
                req_headers.insert(RANGE, hv);
            }

            let mut resp = send_streaming_get(
                raw_url.trim(),
                req_headers,
                self.http_config.allow_private_ips,
                self.http_config.proxy_url.as_deref(),
                "拉取 HLS 资源失败",
            )
            .await?;

            let status = resp.status();
            if status == reqwest::StatusCode::RANGE_NOT_SATISFIABLE {
                let content_range = resp
                    .headers()
                    .get(reqwest::header::CONTENT_RANGE)
                    .and_then(|v| v.to_str().ok())
                    .map(ToString::to_string);
                return Ok(HlsProxyOutcome::RangeNotSatisfiable { content_range });
            }
            if !status.is_success() {
                return Err(DomainError::Validation(format!(
                    "HLS 上游响应状态异常: {status}"
                )));
            }

            let header_ct = resp
                .headers()
                .get(CONTENT_TYPE)
                .and_then(|v| v.to_str().ok())
                .map(ToString::to_string);
            let ct_lower = header_ct
                .as_deref()
                .map(|s| s.split(';').next().unwrap_or(s).trim().to_ascii_lowercase())
                .unwrap_or_default();
            if is_forbidden_media_ct(&ct_lower) {
                return Err(DomainError::Validation(format!(
                    "拒绝转发不安全的媒体内容类型: {ct_lower}"
                )));
            }

            let final_url = resp.url().clone();
            let content_length = resp.content_length();
            let content_range = resp
                .headers()
                .get(reqwest::header::CONTENT_RANGE)
                .and_then(|v| v.to_str().ok())
                .map(ToString::to_string);
            let accept_ranges = resp
                .headers()
                .get(reqwest::header::ACCEPT_RANGES)
                .and_then(|v| v.to_str().ok())
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .unwrap_or("bytes")
                .to_string();

            let prefix = read_sniff_prefix_with_context(
                &mut resp,
                SNIFF_PREFIX_MAX_BYTES,
                status == reqwest::StatusCode::PARTIAL_CONTENT,
                &ct_lower,
                &final_url,
            )
            .await?;

            if looks_like_html_bytes(&prefix) {
                return Err(DomainError::Validation(
                    "拒绝转发伪装为媒体的 HTML/SVG 内容".into(),
                ));
            }

            if include_range
                && status == reqwest::StatusCode::PARTIAL_CONTENT
                && is_partial_hls_playlist_range_candidate(&ct_lower, &final_url, &prefix)
            {
                include_range = false;
                continue;
            }

            break (
                resp,
                status,
                header_ct,
                ct_lower,
                final_url,
                content_length,
                content_range,
                accept_ranges,
                prefix,
            );
        };

        if is_hls_playlist_response(&ct_lower, &final_url, &prefix) {
            validate_proxy_content_length(content_length, MAX_M3U8_BYTES as u64, "HLS 播放列表")?;
            let mut limited = limit_byte_stream(
                spawn_chunk_stream(resp, (!prefix.is_empty()).then_some(prefix)),
                MAX_M3U8_BYTES as u64,
            );
            let mut buf = Vec::new();
            while let Some(item) = std::future::poll_fn(|cx| limited.as_mut().poll_next(cx)).await {
                let chunk = item?;
                buf.extend_from_slice(&chunk);
            }

            if looks_like_html_bytes(&buf) {
                return Err(DomainError::Validation(
                    "拒绝转发伪装为播放列表的 HTML/SVG 内容".into(),
                ));
            }

            let text = decode_response_bytes(&buf, None, header_ct.as_deref());
            let trimmed_m3u8 =
                text.trim_start_matches(|c: char| c == '\u{feff}' || c.is_ascii_whitespace());
            let valid_extm3u_header = trimmed_m3u8
                .strip_prefix("#EXTM3U")
                .is_some_and(|rest| rest.is_empty() || rest.starts_with([' ', '\t', '\r', '\n']));
            if !valid_extm3u_header {
                return Err(DomainError::Validation(
                    "无效的 HLS 播放列表：缺少 #EXTM3U 头".into(),
                ));
            }
            let rewritten = rewrite_m3u8_playlist_with(&text, &final_url, |u: &str| rewrite_uri(u));
            return Ok(HlsProxyOutcome::Playlist(rewritten));
        }

        validate_proxy_content_length(content_length, self.video_proxy_max_bytes, "HLS 媒体分片")?;
        let content_type = infer_segment_content_type(header_ct.as_deref(), &final_url);
        let initial = (!prefix.is_empty()).then_some(prefix);
        let stream = limit_byte_stream(
            spawn_chunk_stream(resp, initial),
            self.video_proxy_max_bytes,
        );

        Ok(HlsProxyOutcome::Segment(ProxyStreamPayload {
            status: status.as_u16(),
            content_type,
            content_length,
            content_range,
            accept_ranges,
            stream,
        }))
    }

    async fn download_direct_video_to_path(
        &self,
        raw_url: &str,
        source_headers: HashMap<String, String>,
        dest_path: &Path,
        max_bytes: u64,
    ) -> DomainResult<VideoContainer> {
        super::transport_download::download_direct_video(
            &self.http_config,
            raw_url,
            source_headers,
            dest_path,
            max_bytes,
        )
        .await
    }

    async fn fetch_hls_playlist_text(
        &self,
        raw_url: &str,
        source_headers: HashMap<String, String>,
    ) -> DomainResult<(String, Url)> {
        super::transport_download::fetch_hls_playlist_text(
            &self.http_config,
            raw_url,
            source_headers,
        )
        .await
    }

    async fn download_hls_asset_to_path(
        &self,
        raw_url: &str,
        source_headers: HashMap<String, String>,
        dest_path: &Path,
        total_bytes: u64,
        max_bytes: u64,
    ) -> DomainResult<u64> {
        super::transport_download::download_hls_asset(
            &self.http_config,
            raw_url,
            source_headers,
            dest_path,
            total_bytes,
            max_bytes,
        )
        .await
    }
}

use std::collections::HashMap;
use std::path::Path;

use reqwest::header::{ACCEPT_ENCODING, CONTENT_TYPE, HeaderValue, USER_AGENT};
use tokio::io::AsyncWriteExt;
use url::Url;
use xmedia_core::domain::error::{DomainError, DomainResult};
use xmedia_source::http::{DEFAULT_WEBVIEW_UA, build_sanitized_headers, decode_response_bytes};
use xmedia_source::proxy::validate_proxy_content_length;

use crate::domain::video::VideoContainer;
use crate::infra::source::hls::{
    MAX_M3U8_BYTES, SNIFF_PREFIX_MAX_BYTES, is_forbidden_media_ct, is_hls_playlist_response,
    looks_like_html_bytes,
};

use super::streaming_client::{
    read_sniff_prefix_with_context, send_streaming_get, validate_streaming_target_url,
};
use super::transport::HttpVideoStreamTransport;

fn infer_container_from_url_and_ct(url: &Url, content_type: Option<&str>) -> VideoContainer {
    if let Some(ct) = content_type {
        let base = ct
            .split(';')
            .next()
            .unwrap_or(ct)
            .trim()
            .to_ascii_lowercase();
        match base.as_str() {
            "video/mp4" | "application/mp4" => return VideoContainer::Mp4,
            "video/x-matroska" => return VideoContainer::Mkv,
            "video/webm" => return VideoContainer::Webm,
            "video/quicktime" => return VideoContainer::Mov,
            "video/x-msvideo" => return VideoContainer::Avi,
            "video/mp2t" => return VideoContainer::Ts,
            "video/x-flv" => return VideoContainer::Flv,
            _ => {}
        }
    }
    if let Some(ext) = Path::new(url.path()).extension().and_then(|s| s.to_str())
        && let Some(c) = VideoContainer::from_extension(ext)
    {
        return c;
    }
    VideoContainer::Mp4
}

async fn open_exclusive_file(path: &Path) -> std::io::Result<tokio::fs::File> {
    let mut opts = tokio::fs::OpenOptions::new();
    opts.write(true).create_new(true);
    #[cfg(unix)]
    opts.mode(0o600);
    opts.open(path).await
}

impl HttpVideoStreamTransport {
    pub(super) async fn download_direct_video_impl(
        &self,
        raw_url: &str,
        source_headers: HashMap<String, String>,
        dest_path: &Path,
        max_bytes: u64,
    ) -> DomainResult<VideoContainer> {
        let validated =
            validate_streaming_target_url(raw_url.trim(), self.http_config.allow_private_ips)
                .await?;
        let mut header_map = build_sanitized_headers(&source_headers);
        if !header_map.contains_key(USER_AGENT) {
            header_map.insert(USER_AGENT, HeaderValue::from_static(DEFAULT_WEBVIEW_UA));
        }
        header_map.insert(ACCEPT_ENCODING, HeaderValue::from_static("identity"));

        let mut resp = send_streaming_get(
            validated.as_str(),
            header_map,
            self.http_config.allow_private_ips,
            self.http_config.proxy_url.as_deref(),
            "拉取在线视频失败",
        )
        .await?;

        if !resp.status().is_success() {
            return Err(DomainError::Validation(format!(
                "在线视频响应状态异常: {}",
                resp.status()
            )));
        }

        validate_proxy_content_length(resp.content_length(), max_bytes, "在线视频")?;

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
                "拒绝缓存不安全的媒体类型: {ct_lower}"
            )));
        }

        let final_url = resp.url().clone();
        let prefix = read_sniff_prefix_with_context(
            &mut resp,
            SNIFF_PREFIX_MAX_BYTES,
            false,
            &ct_lower,
            &final_url,
        )
        .await?;
        if looks_like_html_bytes(&prefix) {
            return Err(DomainError::Validation(
                "上游返回了伪装为视频的 HTML/SVG 内容".into(),
            ));
        }
        if is_hls_playlist_response(&ct_lower, &final_url, &prefix) {
            return Err(DomainError::Validation(
                "上游直链实际返回了 HLS 播放列表，请按 HLS 模式缓存".into(),
            ));
        }

        let container = infer_container_from_url_and_ct(&final_url, header_ct.as_deref());
        let mut file = tokio::fs::OpenOptions::new()
            .write(true)
            .truncate(true)
            .open(dest_path)
            .await
            .map_err(|e| DomainError::Storage(format!("打开临时视频缓存失败: {e}")))?;

        let mut total_bytes: u64 = 0;
        if !prefix.is_empty() {
            total_bytes = total_bytes.saturating_add(prefix.len() as u64);
            if total_bytes > max_bytes {
                return Err(DomainError::Validation(
                    "在线视频体积超过 4 GiB 缓存上限".into(),
                ));
            }
            file.write_all(&prefix)
                .await
                .map_err(|e| DomainError::Storage(format!("写入临时视频缓存失败: {e}")))?;
        }
        while let Some(chunk) = resp
            .chunk()
            .await
            .map_err(|e| DomainError::Validation(format!("读取上游视频流失败: {e}")))?
        {
            if chunk.is_empty() {
                continue;
            }
            total_bytes = total_bytes.saturating_add(chunk.len() as u64);
            if total_bytes > max_bytes {
                return Err(DomainError::Validation(
                    "在线视频体积超过 4 GiB 缓存上限".into(),
                ));
            }
            file.write_all(&chunk)
                .await
                .map_err(|e| DomainError::Storage(format!("写入临时视频缓存失败: {e}")))?;
        }
        file.flush()
            .await
            .map_err(|e| DomainError::Storage(format!("刷新临时视频缓存失败: {e}")))?;
        drop(file);

        if total_bytes == 0 {
            return Err(DomainError::Validation("上游返回的在线视频内容为空".into()));
        }
        Ok(container)
    }

    pub(super) async fn fetch_hls_playlist_text_impl(
        &self,
        raw_url: &str,
        source_headers: HashMap<String, String>,
    ) -> DomainResult<(String, Url)> {
        let validated =
            validate_streaming_target_url(raw_url.trim(), self.http_config.allow_private_ips)
                .await?;
        let mut header_map = build_sanitized_headers(&source_headers);
        if !header_map.contains_key(USER_AGENT) {
            header_map.insert(USER_AGENT, HeaderValue::from_static(DEFAULT_WEBVIEW_UA));
        }
        header_map.insert(ACCEPT_ENCODING, HeaderValue::from_static("identity"));

        let mut resp = send_streaming_get(
            validated.as_str(),
            header_map,
            self.http_config.allow_private_ips,
            self.http_config.proxy_url.as_deref(),
            "拉取 HLS 播放列表失败",
        )
        .await?;
        if !resp.status().is_success() {
            return Err(DomainError::Validation(format!(
                "HLS 播放列表响应状态异常: {}",
                resp.status()
            )));
        }
        let header_ct = resp
            .headers()
            .get(CONTENT_TYPE)
            .and_then(|v| v.to_str().ok())
            .map(ToString::to_string);
        let final_url = resp.url().clone();
        let mut buf = Vec::new();
        while let Some(chunk) = resp
            .chunk()
            .await
            .map_err(|e| DomainError::Validation(format!("读取 HLS 播放列表失败: {e}")))?
        {
            if buf.len().saturating_add(chunk.len()) > MAX_M3U8_BYTES {
                return Err(DomainError::Validation("HLS 播放列表体积超限".into()));
            }
            buf.extend_from_slice(&chunk);
        }
        if looks_like_html_bytes(&buf) {
            return Err(DomainError::Validation(
                "拒绝缓存伪装为 HLS 播放列表的 HTML/SVG 内容".into(),
            ));
        }
        let text = decode_response_bytes(&buf, None, header_ct.as_deref());
        let trimmed = text.trim_start_matches(|c: char| c == '\u{feff}' || c.is_ascii_whitespace());
        if !trimmed.starts_with("#EXTM3U") {
            return Err(DomainError::Validation("无效的 HLS 播放列表".into()));
        }
        Ok((text, final_url))
    }

    pub(super) async fn download_hls_asset_impl(
        &self,
        raw_url: &str,
        source_headers: HashMap<String, String>,
        dest_path: &Path,
        mut total_bytes: u64,
        max_bytes: u64,
    ) -> DomainResult<u64> {
        let validated =
            validate_streaming_target_url(raw_url.trim(), self.http_config.allow_private_ips)
                .await?;
        let mut header_map = build_sanitized_headers(&source_headers);
        if !header_map.contains_key(USER_AGENT) {
            header_map.insert(USER_AGENT, HeaderValue::from_static(DEFAULT_WEBVIEW_UA));
        }
        header_map.insert(ACCEPT_ENCODING, HeaderValue::from_static("identity"));

        let mut resp = send_streaming_get(
            validated.as_str(),
            header_map,
            self.http_config.allow_private_ips,
            self.http_config.proxy_url.as_deref(),
            "拉取 HLS 分片失败",
        )
        .await?;
        if !resp.status().is_success() {
            return Err(DomainError::Validation(format!(
                "HLS 分片响应状态异常: {}",
                resp.status()
            )));
        }
        let mut file = open_exclusive_file(dest_path)
            .await
            .map_err(|e| DomainError::Storage(format!("创建 HLS 分片缓存失败: {e}")))?;
        while let Some(chunk) = resp
            .chunk()
            .await
            .map_err(|e| DomainError::Validation(format!("读取 HLS 分片失败: {e}")))?
        {
            if chunk.is_empty() {
                continue;
            }
            total_bytes = total_bytes.saturating_add(chunk.len() as u64);
            if total_bytes > max_bytes {
                return Err(DomainError::Validation(
                    "HLS 缓存总大小超过 4 GiB 上限".into(),
                ));
            }
            file.write_all(&chunk)
                .await
                .map_err(|e| DomainError::Storage(format!("写入 HLS 分片失败: {e}")))?;
        }
        file.flush()
            .await
            .map_err(|e| DomainError::Storage(format!("刷新 HLS 分片失败: {e}")))?;
        Ok(total_bytes)
    }
}

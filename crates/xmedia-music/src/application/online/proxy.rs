use std::collections::HashMap;
use std::hash::{DefaultHasher, Hash, Hasher};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use uuid::Uuid;
use xmedia_core::domain::error::{DomainError, DomainResult};
use xmedia_core::domain::user::User;
use xmedia_source::crud::SourceRecord;
use xmedia_source::proxy::{limit_byte_stream, validate_proxy_content_length};

use super::{COVER_URL_CACHE_MAX_ENTRIES, CancelOnDrop, MusicOnlineService};
use crate::application::ports::{AudioProxyResponse, UpstreamAudioProxyRequest};
use crate::domain::online::{
    MusicRuleKind, MusicSourceSpec, MusicTrackContext, is_http_url, parse_concurrent_rate,
};

/// Maximum byte ceiling enforced on proxied online audio streams (`2 GiB`).
pub const MAX_AUDIO_PROXY_BYTES: u64 = 2 * 1024 * 1024 * 1024;

fn hash_raw_json(raw: &str) -> u64 {
    let mut hasher = DefaultHasher::new();
    raw.hash(&mut hasher);
    hasher.finish()
}

fn extract_source_headers_from_record(
    rec: &SourceRecord,
    target_url: &str,
) -> HashMap<String, String> {
    let Ok(spec) = serde_json::from_str::<MusicSourceSpec>(&rec.raw_json) else {
        return HashMap::new();
    };
    spec.build_media_headers_with_cookies(&rec.runtime_state.cookies, target_url)
}

impl MusicOnlineService {
    async fn build_optional_source_media_headers(
        &self,
        actor: &User,
        source_id: Option<Uuid>,
        target_url: &str,
    ) -> DomainResult<HashMap<String, String>> {
        let Some(sid) = source_id else {
            return Ok(HashMap::new());
        };
        let rec = self.sources.load_source_for_actor(actor, sid).await?;
        Ok(extract_source_headers_from_record(&rec, target_url))
    }

    pub async fn fetch_cover_proxy(
        &self,
        actor: &User,
        raw_url: &str,
        source_id: Option<Uuid>,
    ) -> DomainResult<(Vec<u8>, &'static str)> {
        let trimmed = raw_url.trim();
        let headers = self
            .build_optional_source_media_headers(actor, source_id, trimmed)
            .await?;
        let header_opt = (!headers.is_empty()).then_some(&headers);
        self.media_proxy
            .fetch_cover_image(trimmed, header_opt)
            .await
    }

    pub async fn fetch_cover_proxy_by_id(
        &self,
        actor: &User,
        source_id: Uuid,
        cover_id: Option<&str>,
        track_id: Option<&str>,
    ) -> DomainResult<(Vec<u8>, &'static str)> {
        let cid = cover_id.map(str::trim).filter(|s| !s.is_empty());
        let tid = track_id.map(str::trim).filter(|s| !s.is_empty());
        if cid.is_none() && tid.is_none() {
            return Err(DomainError::Validation(
                "缺少封面 URL 或 cover_id 参数".into(),
            ));
        }

        let rec = self.sources.load_source_for_actor(actor, source_id).await?;
        let cache_key = (
            source_id,
            hash_raw_json(&rec.raw_json),
            format!("{}|{}", cid.unwrap_or(""), tid.unwrap_or("")),
        );

        let cached_url = {
            let guard = self
                .cover_url_cache
                .lock()
                .unwrap_or_else(|e| e.into_inner());
            guard.get(&cache_key).cloned()
        };
        if let Some(url) = cached_url {
            return self.fetch_cover_proxy(actor, &url, Some(source_id)).await;
        }

        let parsed: MusicSourceSpec = serde_json::from_str(&rec.raw_json)
            .map_err(|e| DomainError::Validation(format!("音源 JSON 无效: {e}")))?;
        if !parsed.has_rule(MusicRuleKind::Cover) {
            return Err(DomainError::NotFound("音源未配置封面解析规则".into()));
        }

        let _permit = self
            .cover_resolve_sem
            .acquire()
            .await
            .map_err(|_| DomainError::Storage("封面解析调度已关闭".into()))?;

        let cached_after_wait = {
            let guard = self
                .cover_url_cache
                .lock()
                .unwrap_or_else(|e| e.into_inner());
            guard.get(&cache_key).cloned()
        };
        if let Some(url) = cached_after_wait {
            return self.fetch_cover_proxy(actor, &url, Some(source_id)).await;
        }

        let cover_timeout = self.op_timeout.min(Duration::from_secs(15));
        self.acquire_rate_slot(
            source_id,
            parse_concurrent_rate(parsed.concurrent_rate.as_deref()),
            cover_timeout,
        )
        .await
        .map_err(DomainError::Validation)?;

        let ctx = MusicTrackContext {
            track_id: tid.unwrap_or_default().to_string(),
            cover_id: cid.map(ToString::to_string),
            ..Default::default()
        };
        let engine = self.engine.clone();
        let prev_state = rec.runtime_state.clone();
        let prev_state_cmp = prev_state.clone();
        let cancel_flag = Arc::new(AtomicBool::new(false));
        let cancel_worker = cancel_flag.clone();
        let mut cancel_guard = CancelOnDrop::new(cancel_flag.clone());

        let timed = tokio::time::timeout(
            cover_timeout,
            tokio::task::spawn_blocking(move || {
                engine
                    .resolve_cover_sync_with_control(
                        &parsed,
                        prev_state,
                        &ctx,
                        cover_timeout,
                        cancel_worker,
                    )
                    .map_err(|e| e.message)
            }),
        )
        .await;

        let out = match timed {
            Ok(Ok(Ok(res))) => {
                cancel_guard.disarm();
                res
            }
            Ok(Ok(Err(err))) => {
                cancel_guard.disarm();
                return Err(DomainError::NotFound(format!("解析封面地址失败: {err}")));
            }
            Ok(Err(e)) => {
                cancel_guard.disarm();
                return Err(DomainError::Storage(format!("封面解析线程异常: {e}")));
            }
            Err(_) => {
                cancel_flag.store(true, Ordering::Relaxed);
                return Err(DomainError::Validation("解析封面地址超时".into()));
            }
        };

        if out.state != prev_state_cmp {
            let _ = self
                .sources
                .persist_runtime_state_with_base(source_id, &prev_state_cmp, out.state, false)
                .await;
        }

        let resolved_url = out
            .data
            .map(|u| u.trim().to_string())
            .filter(|u| is_http_url(u))
            .ok_or_else(|| DomainError::NotFound("音源未返回有效封面地址".into()))?;

        {
            let mut guard = self
                .cover_url_cache
                .lock()
                .unwrap_or_else(|e| e.into_inner());
            if guard.len() >= COVER_URL_CACHE_MAX_ENTRIES {
                guard.clear();
            }
            guard.insert(cache_key, resolved_url.clone());
        }

        self.fetch_cover_proxy(actor, &resolved_url, Some(source_id))
            .await
    }

    pub async fn stream_audio_proxy(
        &self,
        actor: &User,
        raw_url: &str,
        source_id: Uuid,
        range_header: Option<&str>,
        if_range_header: Option<&str>,
    ) -> DomainResult<AudioProxyResponse> {
        let rec = self.sources.load_source_for_actor(actor, source_id).await?;
        let trimmed = raw_url.trim();
        let source_headers = extract_source_headers_from_record(&rec, trimmed);

        let mut resp = self
            .media_proxy
            .open_audio_proxy_stream(UpstreamAudioProxyRequest {
                target_url: trimmed,
                headers: &source_headers,
                range_header,
                if_range_header,
                max_bytes: MAX_AUDIO_PROXY_BYTES,
            })
            .await?;

        validate_proxy_content_length(resp.content_length, MAX_AUDIO_PROXY_BYTES, "音频流")?;
        resp.stream = limit_byte_stream(resp.stream, MAX_AUDIO_PROXY_BYTES);
        Ok(resp)
    }
}

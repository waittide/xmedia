use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use uuid::Uuid;
use xmedia_core::domain::error::{DomainError, DomainResult};
use xmedia_core::domain::user::User;

use crate::application::ports::VideoProxyUrlBuilder;
use crate::domain::online::VideoSourceSpec;

use super::dto::OnlineVideoPlayResponseDto;
use super::{CancelOnDrop, VideoOnlineService};

impl VideoOnlineService {
    pub async fn play(
        &self,
        actor: &User,
        source_id: Uuid,
        play_url: &str,
        group_name: Option<&str>,
        url_builder: &dyn VideoProxyUrlBuilder,
    ) -> DomainResult<OnlineVideoPlayResponseDto> {
        let rec = self.load_source_for_actor(actor, source_id).await?;
        let parsed = <VideoSourceSpec as xmedia_source::crud::SourceSpecAdapter>::parse_single(
            &rec.raw_json,
        )
        .map_err(DomainError::Validation)?;
        let op_timeout = self.op_timeout;
        self.acquire_rate_slot(source_id, parsed.parse_concurrent_rate(), op_timeout)
            .await
            .map_err(DomainError::Validation)?;
        let clean_play_url = xmedia_source::http::resolve_literal_http_url(
            play_url,
            Some(parsed.effective_source_url()),
        )?;

        let engine = self.engine.clone();
        let prev_state = rec.runtime_state.clone();
        let prev_state_cmp = prev_state.clone();
        let play_owned = clean_play_url;
        let group_owned = group_name.map(ToString::to_string);
        let cancel_flag = Arc::new(AtomicBool::new(false));
        let cancel_worker = cancel_flag.clone();
        let mut cancel_guard = CancelOnDrop::new(cancel_flag.clone());

        let timed = tokio::time::timeout(
            op_timeout,
            tokio::task::spawn_blocking(move || {
                engine
                    .resolve_play_sync_with_control(
                        &parsed,
                        prev_state,
                        &play_owned,
                        group_owned.as_deref(),
                        op_timeout,
                        cancel_worker,
                    )
                    .map_err(|e| e.message)
            }),
        )
        .await;

        let mut out = match timed {
            Ok(Ok(Ok(res))) => {
                cancel_guard.disarm();
                res
            }
            Ok(Ok(Err(err))) => {
                cancel_guard.disarm();
                return Err(DomainError::Validation(err));
            }
            Ok(Err(e)) => {
                cancel_guard.disarm();
                return Err(DomainError::Storage(format!("解析播放地址失败: {e}")));
            }
            Err(_) => {
                cancel_flag.store(true, Ordering::Relaxed);
                return Err(DomainError::Validation(format!(
                    "解析播放地址超时（超过 {} 秒）",
                    op_timeout.as_secs()
                )));
            }
        };

        if out.state != prev_state_cmp
            && let Err(err) = self
                .persist_runtime_state_with_base(source_id, &prev_state_cmp, out.state, false)
                .await
        {
            tracing::warn!(source_id = %source_id, error = %err, "failed to persist runtime state after play");
        }
        if !actor.role.is_admin() {
            out.diagnostics.logs.clear();
        }

        out.data.proxy_stream_url = if out.data.stream_type == "hls" {
            url_builder.hls_proxy_url(&out.data.stream_url, source_id, None)
        } else {
            url_builder.stream_proxy_url(&out.data.stream_url, source_id, None)
        };
        for sub in &mut out.data.subtitles {
            let clean = sub.url.trim();
            if clean.starts_with("http://") || clean.starts_with("https://") {
                sub.proxy_url = Some(url_builder.subtitle_proxy_url(clean, source_id, None));
            }
        }

        Ok(OnlineVideoPlayResponseDto {
            source_id,
            source_name: rec.name,
            play: out.data,
            diagnostics: out.diagnostics,
        })
    }
}

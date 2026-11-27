mod resolve;
mod search_explore;
mod template;
#[cfg(test)]
mod tests;
mod v2;

use std::collections::HashMap;
use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use xmedia_source::http::{HttpClientConfig, LegadoHttpClient};
use xmedia_source::model::{SourceCallError, SourceCallOutput, SourceRuntimeState};

use super::session::MusicSourceSession;
use crate::application::ports::MusicSourceEnginePort;
use crate::domain::online::{
    MusicSourceSpec, MusicTrackContext, OnlineMusicExploreResult, OnlineMusicLyricsResult,
    OnlineMusicStreamResult, OnlineMusicTrackItem, infer_audio_format, is_http_url,
    normalize_artist, parse_duration_secs,
};

#[derive(Debug, Clone)]
pub struct MusicSourceEngine {
    http_config: HttpClientConfig,
    js_timeout: Duration,
}

impl Default for MusicSourceEngine {
    fn default() -> Self {
        Self {
            http_config: HttpClientConfig::default(),
            js_timeout: Duration::from_secs(45),
        }
    }
}

impl MusicSourceEngine {
    pub fn new(http_config: HttpClientConfig, js_timeout: Duration) -> Self {
        Self {
            http_config,
            js_timeout,
        }
    }

    pub fn http_config(&self) -> &HttpClientConfig {
        &self.http_config
    }

    fn build_session(
        &self,
        source: &MusicSourceSpec,
        mut initial_state: SourceRuntimeState,
        op_timeout: Duration,
        cancel_flag: Arc<AtomicBool>,
    ) -> Result<MusicSourceSession, String> {
        initial_state.ensure_android_id(&source.source_url);
        let cookies_arc = Arc::new(Mutex::new(initial_state.cookies.clone()));
        let state_arc = Arc::new(Mutex::new(initial_state));
        let http = LegadoHttpClient::new(
            self.http_config.clone(),
            cookies_arc.clone(),
            source.header.as_deref(),
            source.enabled_cookie_jar,
        )?;
        Ok(MusicSourceSession::new(
            source,
            http,
            state_arc,
            cookies_arc,
            self.js_timeout,
            op_timeout,
            cancel_flag,
        ))
    }
}

impl MusicSourceEnginePort for MusicSourceEngine {
    fn search_sync_with_control(
        &self,
        source: &MusicSourceSpec,
        initial_state: SourceRuntimeState,
        keyword: &str,
        page: u32,
        limit: u32,
        op_timeout: Duration,
        cancel_flag: Arc<AtomicBool>,
    ) -> Result<SourceCallOutput<Vec<OnlineMusicTrackItem>>, SourceCallError> {
        MusicSourceEngine::search_sync_with_control(
            self,
            source,
            initial_state,
            keyword,
            page,
            limit,
            op_timeout,
            cancel_flag,
        )
    }

    fn explore_sync_with_control(
        &self,
        source: &MusicSourceSpec,
        initial_state: SourceRuntimeState,
        explore_url_override: Option<&str>,
        page: u32,
        limit: u32,
        op_timeout: Duration,
        cancel_flag: Arc<AtomicBool>,
    ) -> Result<SourceCallOutput<OnlineMusicExploreResult>, SourceCallError> {
        MusicSourceEngine::explore_sync_with_control(
            self,
            source,
            initial_state,
            explore_url_override,
            page,
            limit,
            op_timeout,
            cancel_flag,
        )
    }

    fn fetch_track_detail_sync_with_control(
        &self,
        source: &MusicSourceSpec,
        initial_state: SourceRuntimeState,
        ctx: &MusicTrackContext,
        op_timeout: Duration,
        cancel_flag: Arc<AtomicBool>,
    ) -> Result<SourceCallOutput<OnlineMusicTrackItem>, SourceCallError> {
        MusicSourceEngine::fetch_track_detail_sync_with_control(
            self,
            source,
            initial_state,
            ctx,
            op_timeout,
            cancel_flag,
        )
    }

    fn resolve_stream_sync_with_control(
        &self,
        source: &MusicSourceSpec,
        initial_state: SourceRuntimeState,
        ctx: &MusicTrackContext,
        quality: Option<&str>,
        op_timeout: Duration,
        cancel_flag: Arc<AtomicBool>,
    ) -> Result<SourceCallOutput<OnlineMusicStreamResult>, SourceCallError> {
        MusicSourceEngine::resolve_stream_sync_with_control(
            self,
            source,
            initial_state,
            ctx,
            quality,
            op_timeout,
            cancel_flag,
        )
    }

    fn resolve_cover_sync_with_control(
        &self,
        source: &MusicSourceSpec,
        initial_state: SourceRuntimeState,
        ctx: &MusicTrackContext,
        op_timeout: Duration,
        cancel_flag: Arc<AtomicBool>,
    ) -> Result<SourceCallOutput<Option<String>>, SourceCallError> {
        MusicSourceEngine::resolve_cover_sync_with_control(
            self,
            source,
            initial_state,
            ctx,
            op_timeout,
            cancel_flag,
        )
    }

    fn resolve_lyrics_sync_with_control(
        &self,
        source: &MusicSourceSpec,
        initial_state: SourceRuntimeState,
        ctx: &MusicTrackContext,
        op_timeout: Duration,
        cancel_flag: Arc<AtomicBool>,
    ) -> Result<SourceCallOutput<Option<OnlineMusicLyricsResult>>, SourceCallError> {
        MusicSourceEngine::resolve_lyrics_sync_with_control(
            self,
            source,
            initial_state,
            ctx,
            op_timeout,
            cancel_flag,
        )
    }

    fn run_login_action_sync(
        &self,
        source: &MusicSourceSpec,
        initial_state: SourceRuntimeState,
        login_info: HashMap<String, String>,
        action_js: Option<&str>,
    ) -> Result<SourceCallOutput<String>, SourceCallError> {
        MusicSourceEngine::run_login_action_sync(self, source, initial_state, login_info, action_js)
    }

    fn http_config(&self) -> HttpClientConfig {
        self.http_config.clone()
    }
}

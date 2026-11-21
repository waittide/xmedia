use std::collections::HashMap;
use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use xmedia_source::http::LegadoHttpClient;
use xmedia_source::js_runtime::{JsSandboxConfig, ReusableSourceSession};
use xmedia_source::model::{ExecutionDiagnostics, SourceMetadata, SourceRuntimeState};
use xmedia_source::rule::RuleValue;

use crate::source::model::{MusicSourceSpec, MusicTrackContext};

pub(super) const MUSIC_JS_BOOTSTRAP: &str = r#"
globalThis.track = {
    id: '',
    trackId: '',
    title: '',
    artist: '',
    album: '',
    duration: 0,
    coverUrl: '',
    coverId: '',
    urlId: '',
    lyricId: '',
    playUrl: '',
    detailUrl: '',
    quality: '',
    _vars: {},
    getVariable: function(k) { return this._vars[String(k)] || ''; },
    putVariable: function(k, v) { this._vars[String(k)] = v == null ? '' : String(v); }
};
"#;

pub struct MusicSourceSession {
    pub(super) inner: ReusableSourceSession,
}

impl MusicSourceSession {
    pub fn new(
        source: &MusicSourceSpec,
        http: LegadoHttpClient,
        state_arc: Arc<Mutex<SourceRuntimeState>>,
        cookies_arc: Arc<Mutex<HashMap<String, String>>>,
        js_timeout: Duration,
        op_timeout: Duration,
        cancel_flag: Arc<AtomicBool>,
    ) -> Self {
        let metadata = SourceMetadata::from(source);
        let mut inner = ReusableSourceSession::new(
            metadata,
            http,
            state_arc,
            cookies_arc,
            JsSandboxConfig::with_timeout(js_timeout),
            op_timeout,
            cancel_flag,
        );
        inner.add_bootstrap_snippet(MUSIC_JS_BOOTSTRAP);
        Self { inner }
    }

    pub fn http(&self) -> &LegadoHttpClient {
        self.inner.http()
    }

    pub fn check_active(&self) -> Result<(), String> {
        self.inner.check_active()
    }

    pub fn snapshot_diagnostics(&self) -> ExecutionDiagnostics {
        self.inner.snapshot_diagnostics()
    }

    pub fn finalize_state(&self) -> (SourceRuntimeState, ExecutionDiagnostics) {
        self.inner.finalize_state()
    }

    pub fn set_search_vars(&mut self, keyword: &str, page: u32, limit: u32) -> Result<(), String> {
        self.inner.set_search_vars(keyword, page)?;
        self.inner
            .set_domain_global("limit", serde_json::json!(limit), false)
    }

    pub fn set_track_context(&mut self, ctx: &MusicTrackContext) -> Result<(), String> {
        let payload = serde_json::json!({
            "id": ctx.track_id,
            "trackId": ctx.track_id,
            "title": ctx.title,
            "artist": ctx.artist,
            "album": ctx.album.as_deref().unwrap_or(""),
            "duration": ctx.duration_secs.unwrap_or(0),
            "coverUrl": ctx.cover_url.as_deref().unwrap_or(""),
            "coverId": ctx.cover_id.as_deref().unwrap_or(""),
            "urlId": ctx.url_id.as_deref().unwrap_or(""),
            "lyricId": ctx.lyric_id.as_deref().unwrap_or(""),
            "playUrl": ctx.play_url.as_deref().unwrap_or(""),
            "detailUrl": ctx.detail_url.as_deref().unwrap_or(""),
            "quality": ctx.quality.as_deref().unwrap_or(""),
        });
        self.inner.set_domain_global("track", payload, true)?;
        if let Some(ref q) = ctx.quality
            && !q.trim().is_empty()
        {
            self.inner
                .set_domain_global("quality", serde_json::json!(q), false)?;
        }
        Ok(())
    }

    pub fn eval_string(
        &mut self,
        input: &RuleValue,
        rule: &str,
        base_url: &str,
    ) -> Result<String, String> {
        self.inner.eval_string(input, rule, base_url)
    }

    pub fn eval_list(
        &mut self,
        input: &RuleValue,
        rule: &str,
        base_url: &str,
    ) -> Result<Vec<RuleValue>, String> {
        self.inner.eval_list(input, rule, base_url)
    }

    pub fn eval_opt(
        &mut self,
        input: &RuleValue,
        rule_opt: Option<&str>,
        base_url: &str,
    ) -> String {
        self.inner.eval_opt(input, rule_opt, base_url)
    }

    pub fn eval_login_script(
        &mut self,
        login_url_js: &str,
        action_js: Option<&str>,
        login_info: &HashMap<String, String>,
    ) -> Result<String, String> {
        self.inner
            .eval_login_script(login_url_js, action_js, login_info)
    }
}

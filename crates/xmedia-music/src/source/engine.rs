mod resolve;
mod search_explore;
mod template;
#[cfg(test)]
mod tests;

pub use search_explore::parse_explore_categories;

use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use xmedia_source::http::{HttpClientConfig, LegadoHttpClient};
use xmedia_source::model::SourceRuntimeState;

use super::model::MusicSourceSpec;
use super::session::MusicSourceSession;
use crate::domain::catalog::AudioFormat;

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

/// Percent-encodes a query/text component (UTF-8 bytes outside RFC 3986 unreserved set).
pub fn url_encode_component(raw: &str) -> String {
    let mut out = String::with_capacity(raw.len() * 3);
    for b in raw.as_bytes() {
        let ch = *b;
        if ch.is_ascii_alphanumeric() || matches!(ch, b'-' | b'_' | b'.' | b'~') {
            out.push(ch as char);
        } else {
            out.push('%');
            out.push(hex_upper(ch >> 4));
            out.push(hex_upper(ch & 0x0F));
        }
    }
    out
}

fn hex_upper(nibble: u8) -> char {
    match nibble {
        0..=9 => (b'0' + nibble) as char,
        10..=15 => (b'A' + (nibble - 10)) as char,
        _ => '0',
    }
}

/// Normalizes multi-value artist outputs (`"A\nB"`) into `"A / B"`.
pub fn normalize_artist(raw: &str) -> String {
    raw.lines()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .collect::<Vec<_>>()
        .join(" / ")
}

/// Parses duration values formatted as `hh:mm:ss`, `mm:ss`, seconds, or milliseconds.
pub fn parse_duration_secs(raw: &str, unit: Option<&str>) -> Option<u32> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return None;
    }
    if trimmed.contains(':') {
        let parts: Vec<&str> = trimmed.split(':').collect();
        match parts.len() {
            2 => {
                let mins = parts[0].trim().parse::<f64>().ok()?;
                let secs = parts[1].trim().parse::<f64>().ok()?;
                if mins < 0.0 || secs < 0.0 {
                    return None;
                }
                return Some((mins * 60.0 + secs).round() as u32);
            }
            3 => {
                let hrs = parts[0].trim().parse::<f64>().ok()?;
                let mins = parts[1].trim().parse::<f64>().ok()?;
                let secs = parts[2].trim().parse::<f64>().ok()?;
                if hrs < 0.0 || mins < 0.0 || secs < 0.0 {
                    return None;
                }
                return Some((hrs * 3600.0 + mins * 60.0 + secs).round() as u32);
            }
            _ => return None,
        }
    }
    let val = trimmed.parse::<f64>().ok()?;
    if !val.is_finite() || val < 0.0 {
        return None;
    }
    let unit_norm = unit
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_ascii_lowercase);
    let secs = match unit_norm.as_deref() {
        Some("ms") => val / 1000.0,
        Some("s") | Some("sec") | Some("secs") => val,
        _ => {
            if val >= 36_000.0 {
                val / 1000.0
            } else {
                val
            }
        }
    };
    Some(secs.round() as u32)
}

/// Infers audio format from URL path extension (`flac`, `mp3`, `m4a`, `alac`, `aac`, `ogg`, `opus`, `wav`, `aiff`, `ape`, `wv`).
pub fn infer_audio_format(url: &str) -> Option<String> {
    let without_frag = url.split('#').next().unwrap_or(url);
    let without_query = without_frag.split('?').next().unwrap_or(without_frag);
    let last_seg = without_query.rsplit('/').next().unwrap_or(without_query);
    let ext = last_seg.rsplit_once('.')?.1;
    AudioFormat::from_extension(ext).map(|fmt| fmt.as_str().to_string())
}

pub(crate) fn is_http_url(u: &str) -> bool {
    let trimmed = u.trim();
    trimmed
        .get(..7)
        .is_some_and(|p| p.eq_ignore_ascii_case("http://"))
        || trimmed
            .get(..8)
            .is_some_and(|p| p.eq_ignore_ascii_case("https://"))
}

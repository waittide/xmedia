use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use std::time::Duration;

use xmedia_source::http::resolve_relative_url;
use xmedia_source::model::{SourceCallError, SourceCallOutput, SourceRuntimeState};
use xmedia_source::plugin_v2::{PluginV2Manifest, PluginV2Runner};

use super::{
    MusicSourceEngine, infer_audio_format, is_http_url, normalize_artist, parse_duration_secs,
};
use crate::domain::catalog::StructuredLyrics;
use crate::domain::online::{
    MusicSourceSpec, MusicTrackContext, OnlineMusicExploreCategory, OnlineMusicExploreResult,
    OnlineMusicLyricsResult, OnlineMusicStreamResult, OnlineMusicTrackItem,
};

fn val_str(obj: &serde_json::Map<String, serde_json::Value>, keys: &[&str]) -> Option<String> {
    for k in keys {
        if let Some(v) = obj.get(*k) {
            match v {
                serde_json::Value::String(s) if !s.trim().is_empty() => {
                    return Some(s.trim().to_string());
                }
                serde_json::Value::Number(n) => return Some(n.to_string()),
                _ => {}
            }
        }
    }
    None
}

fn parse_artist_field(obj: &serde_json::Map<String, serde_json::Value>) -> String {
    for k in ["artist", "artists", "ar"] {
        if let Some(v) = obj.get(k) {
            match v {
                serde_json::Value::String(s) if !s.trim().is_empty() => {
                    return normalize_artist(s);
                }
                serde_json::Value::Array(arr) => {
                    let parts: Vec<String> = arr
                        .iter()
                        .filter_map(|item| match item {
                            serde_json::Value::String(s) if !s.trim().is_empty() => {
                                Some(s.trim().to_string())
                            }
                            serde_json::Value::Object(m) => val_str(m, &["name", "title"]),
                            _ => None,
                        })
                        .collect();
                    if !parts.is_empty() {
                        return parts.join(" / ");
                    }
                }
                _ => {}
            }
        }
    }
    String::new()
}

fn parse_album_field(obj: &serde_json::Map<String, serde_json::Value>) -> Option<String> {
    for k in ["album", "al"] {
        if let Some(v) = obj.get(k) {
            match v {
                serde_json::Value::String(s) if !s.trim().is_empty() => {
                    return Some(s.trim().to_string());
                }
                serde_json::Value::Object(m) => {
                    if let Some(name) = val_str(m, &["name", "title"]) {
                        return Some(name);
                    }
                }
                _ => {}
            }
        }
    }
    None
}

fn parse_duration_field(obj: &serde_json::Map<String, serde_json::Value>) -> Option<u32> {
    for k in ["durationSecs", "duration_secs"] {
        if let Some(n) = obj.get(k).and_then(|v| v.as_f64())
            && n.is_finite()
            && n >= 0.0
        {
            return Some(n.round() as u32);
        }
    }
    for k in ["durationMs", "duration_ms", "dt"] {
        if let Some(n) = obj.get(k).and_then(|v| v.as_f64())
            && n.is_finite()
            && n >= 0.0
        {
            return Some((n / 1000.0).round() as u32);
        }
    }
    if let Some(raw) = val_str(obj, &["duration"]) {
        return parse_duration_secs(&raw, None);
    }
    None
}

fn parse_v2_track_item(val: &serde_json::Value, base_url: &str) -> Option<OnlineMusicTrackItem> {
    let obj = val.as_object()?;
    let track_id = val_str(obj, &["id", "trackId", "track_id"]).unwrap_or_default();
    let title = val_str(obj, &["title", "name"]).unwrap_or_default();
    if track_id.is_empty() && title.is_empty() {
        return None;
    }
    let artist = parse_artist_field(obj);
    let album = parse_album_field(obj);
    let duration_secs = parse_duration_field(obj);
    let cover_url = val_str(obj, &["coverUrl", "cover_url", "picUrl"])
        .map(|u| resolve_relative_url(&u, base_url));
    let cover_id = val_str(obj, &["coverId", "cover_id"]);
    let url_id = val_str(obj, &["urlId", "url_id"]);
    let lyric_id = val_str(obj, &["lyricId", "lyric_id"]);
    let play_url =
        val_str(obj, &["playUrl", "play_url"]).map(|u| resolve_relative_url(&u, base_url));
    let detail_url =
        val_str(obj, &["detailUrl", "detail_url"]).map(|u| resolve_relative_url(&u, base_url));
    let format =
        val_str(obj, &["format"]).or_else(|| play_url.as_deref().and_then(infer_audio_format));

    Some(OnlineMusicTrackItem {
        track_id,
        title,
        artist,
        album,
        duration_secs,
        cover_url,
        cover_id,
        url_id,
        lyric_id,
        play_url,
        detail_url,
        format,
    })
}

fn parse_v2_track_list(val: &serde_json::Value, base_url: &str) -> Vec<OnlineMusicTrackItem> {
    let arr = match val {
        serde_json::Value::Array(a) => Some(a),
        serde_json::Value::Object(m) => m
            .get("tracks")
            .or_else(|| m.get("items"))
            .or_else(|| m.get("results"))
            .and_then(|v| v.as_array()),
        _ => None,
    };
    arr.map(|items| {
        items
            .iter()
            .filter_map(|v| parse_v2_track_item(v, base_url))
            .collect()
    })
    .unwrap_or_default()
}

fn track_ctx_to_js_arg(ctx: &MusicTrackContext) -> serde_json::Value {
    serde_json::json!({
        "id": ctx.track_id,
        "trackId": ctx.track_id,
        "title": ctx.title,
        "artist": ctx.artist,
        "album": ctx.album,
        "durationSecs": ctx.duration_secs,
        "coverUrl": ctx.cover_url,
        "coverId": ctx.cover_id,
        "urlId": ctx.url_id,
        "lyricId": ctx.lyric_id,
        "playUrl": ctx.play_url,
        "detailUrl": ctx.detail_url,
        "quality": ctx.quality,
    })
}

impl MusicSourceEngine {
    #[allow(clippy::too_many_arguments)]
    pub(super) fn search_v2_sync(
        &self,
        manifest: &PluginV2Manifest,
        js_code: &str,
        initial_state: SourceRuntimeState,
        keyword: &str,
        page: u32,
        limit: u32,
        op_timeout: Duration,
        cancel_flag: Arc<AtomicBool>,
    ) -> Result<SourceCallOutput<Vec<OnlineMusicTrackItem>>, SourceCallError> {
        let out = PluginV2Runner::call_method_sync(
            js_code,
            manifest,
            initial_state,
            &self.http_config,
            self.js_timeout,
            op_timeout,
            cancel_flag,
            &["search"],
            &[serde_json::json!({
                "keyword": keyword,
                "page": page.max(1),
                "limit": limit.clamp(1, 50),
            })],
        )?;
        let tracks = parse_v2_track_list(&out.data, &manifest.base_url);
        Ok(SourceCallOutput {
            data: tracks,
            state: out.state,
            diagnostics: out.diagnostics,
        })
    }

    #[allow(clippy::too_many_arguments)]
    pub(super) fn explore_v2_sync(
        &self,
        manifest: &PluginV2Manifest,
        js_code: &str,
        initial_state: SourceRuntimeState,
        explore_url_override: Option<&str>,
        page: u32,
        limit: u32,
        op_timeout: Duration,
        cancel_flag: Arc<AtomicBool>,
    ) -> Result<SourceCallOutput<OnlineMusicExploreResult>, SourceCallError> {
        let default_cats: Vec<OnlineMusicExploreCategory> = manifest
            .explore_categories
            .iter()
            .map(|c| OnlineMusicExploreCategory {
                title: c.title.clone(),
                url: c.id.clone(),
            })
            .collect();
        let cat_id = explore_url_override
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .or_else(|| default_cats.first().map(|c| c.url.as_str()))
            .unwrap_or("");

        let out = PluginV2Runner::call_any_method_sync(
            js_code,
            manifest,
            initial_state,
            &self.http_config,
            self.js_timeout,
            op_timeout,
            cancel_flag,
            &[&["explore"], &["browse"]],
            &[serde_json::json!({
                "categoryId": cat_id,
                "exploreUrl": cat_id,
                "page": page.max(1),
                "limit": limit.clamp(1, 50),
            })],
        )?;

        let tracks = parse_v2_track_list(&out.data, &manifest.base_url);
        let categories = out
            .data
            .as_object()
            .and_then(|m| m.get("categories"))
            .and_then(|v| v.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|item| {
                        let obj = item.as_object()?;
                        let title = val_str(obj, &["title", "name"])?;
                        let url = val_str(obj, &["id", "url"])?;
                        Some(OnlineMusicExploreCategory { title, url })
                    })
                    .collect::<Vec<_>>()
            })
            .filter(|v| !v.is_empty())
            .unwrap_or(default_cats);

        Ok(SourceCallOutput {
            data: OnlineMusicExploreResult { tracks, categories },
            state: out.state,
            diagnostics: out.diagnostics,
        })
    }

    pub(super) fn fetch_track_detail_v2_sync(
        &self,
        manifest: &PluginV2Manifest,
        js_code: &str,
        initial_state: SourceRuntimeState,
        ctx: &MusicTrackContext,
        op_timeout: Duration,
        cancel_flag: Arc<AtomicBool>,
    ) -> Result<SourceCallOutput<OnlineMusicTrackItem>, SourceCallError> {
        if !manifest.has_detail {
            return Ok(SourceCallOutput {
                data: OnlineMusicTrackItem {
                    track_id: ctx.track_id.clone(),
                    title: ctx.title.clone(),
                    artist: ctx.artist.clone(),
                    album: ctx.album.clone(),
                    duration_secs: ctx.duration_secs,
                    cover_url: ctx.cover_url.clone(),
                    cover_id: ctx.cover_id.clone(),
                    url_id: ctx.url_id.clone(),
                    lyric_id: ctx.lyric_id.clone(),
                    play_url: ctx.play_url.clone(),
                    detail_url: ctx.detail_url.clone(),
                    format: ctx.play_url.as_deref().and_then(infer_audio_format),
                },
                state: initial_state,
                diagnostics: Default::default(),
            });
        }

        let track_arg = track_ctx_to_js_arg(ctx);
        let out = PluginV2Runner::call_any_method_sync(
            js_code,
            manifest,
            initial_state,
            &self.http_config,
            self.js_timeout,
            op_timeout,
            cancel_flag,
            &[&["getTrackInfo"], &["getDetail"]],
            &[serde_json::json!({
                "id": ctx.track_id,
                "trackId": ctx.track_id,
                "detailUrl": ctx.detail_url,
                "track": track_arg,
            })],
        )?;

        let parsed = parse_v2_track_item(&out.data, &manifest.base_url);
        let merged = match parsed {
            Some(mut item) => {
                if item.track_id.is_empty() {
                    item.track_id = ctx.track_id.clone();
                }
                if item.title.is_empty() {
                    item.title = ctx.title.clone();
                }
                if item.artist.is_empty() {
                    item.artist = ctx.artist.clone();
                }
                if item.album.is_none() {
                    item.album = ctx.album.clone();
                }
                if item.duration_secs.is_none() {
                    item.duration_secs = ctx.duration_secs;
                }
                if item.cover_url.is_none() {
                    item.cover_url = ctx.cover_url.clone();
                }
                if item.cover_id.is_none() {
                    item.cover_id = ctx.cover_id.clone();
                }
                if item.url_id.is_none() {
                    item.url_id = ctx.url_id.clone();
                }
                if item.lyric_id.is_none() {
                    item.lyric_id = ctx.lyric_id.clone();
                }
                if item.play_url.is_none() {
                    item.play_url = ctx.play_url.clone();
                }
                if item.detail_url.is_none() {
                    item.detail_url = ctx.detail_url.clone();
                }
                item
            }
            None => OnlineMusicTrackItem {
                track_id: ctx.track_id.clone(),
                title: ctx.title.clone(),
                artist: ctx.artist.clone(),
                album: ctx.album.clone(),
                duration_secs: ctx.duration_secs,
                cover_url: ctx.cover_url.clone(),
                cover_id: ctx.cover_id.clone(),
                url_id: ctx.url_id.clone(),
                lyric_id: ctx.lyric_id.clone(),
                play_url: ctx.play_url.clone(),
                detail_url: ctx.detail_url.clone(),
                format: ctx.play_url.as_deref().and_then(infer_audio_format),
            },
        };

        Ok(SourceCallOutput {
            data: merged,
            state: out.state,
            diagnostics: out.diagnostics,
        })
    }

    #[allow(clippy::too_many_arguments)]
    pub(super) fn resolve_stream_v2_sync(
        &self,
        source: &MusicSourceSpec,
        manifest: &PluginV2Manifest,
        js_code: &str,
        initial_state: SourceRuntimeState,
        ctx: &MusicTrackContext,
        quality: Option<&str>,
        op_timeout: Duration,
        cancel_flag: Arc<AtomicBool>,
    ) -> Result<SourceCallOutput<OnlineMusicStreamResult>, SourceCallError> {
        let requested_quality = quality
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .or_else(|| {
                ctx.quality
                    .as_deref()
                    .map(str::trim)
                    .filter(|s| !s.is_empty())
            });
        let (effective_quality, _) = source.resolve_quality(requested_quality);
        let mut ctx_with_q = ctx.clone();
        ctx_with_q.quality = Some(effective_quality.to_string());
        let track_arg = track_ctx_to_js_arg(&ctx_with_q);

        let out = PluginV2Runner::call_method_sync(
            js_code,
            manifest,
            initial_state,
            &self.http_config,
            self.js_timeout,
            op_timeout,
            cancel_flag,
            &["getStream"],
            &[serde_json::json!({
                "id": ctx.track_id,
                "trackId": ctx.track_id,
                "urlId": ctx.url_id,
                "playUrl": ctx.play_url,
                "quality": effective_quality,
                "track": track_arg,
            })],
        )?;

        let mut headers = source.parse_media_headers();
        let (raw_url, format_opt, bitrate, expires_in_secs) = match &out.data {
            serde_json::Value::String(s) => (s.trim().to_string(), None, None, None),
            serde_json::Value::Object(obj) => {
                let u = val_str(obj, &["streamUrl", "stream_url", "url"]).unwrap_or_default();
                let fmt = val_str(obj, &["format", "type", "encodeType"]);
                let br = obj
                    .get("bitrate")
                    .or_else(|| obj.get("br"))
                    .and_then(|v| v.as_f64())
                    .filter(|v| v.is_finite() && *v > 0.0)
                    .map(|v| {
                        let rounded = v.round() as u32;
                        if rounded > 10_000 {
                            rounded / 1000
                        } else {
                            rounded
                        }
                    });
                let exp = obj
                    .get("expiresInSecs")
                    .or_else(|| obj.get("expires_in_secs"))
                    .or_else(|| obj.get("expi"))
                    .and_then(|v| v.as_u64());
                if let Some(hdrs) = obj.get("headers").and_then(|v| v.as_object()) {
                    for (k, v) in hdrs {
                        if let Some(sv) = v.as_str() {
                            let kt = k.trim();
                            if !kt.is_empty() && !kt.eq_ignore_ascii_case("range") {
                                headers.retain(|ek, _| !ek.eq_ignore_ascii_case(kt));
                                headers.insert(kt.to_string(), sv.to_string());
                            }
                        }
                    }
                }
                (u, fmt, br, exp)
            }
            _ => (String::new(), None, None, None),
        };

        if raw_url.is_empty() {
            return Err(SourceCallError::new(
                "未能解析出有效音频流地址（可能需要登录 VIP 账号或该歌曲暂无版权）",
                out.diagnostics,
            ));
        }
        let resolved_stream = resolve_relative_url(&raw_url, &manifest.base_url);
        if !is_http_url(&resolved_stream) {
            return Err(SourceCallError::new(
                format!("解析到的音频流地址不是有效的 HTTP(S) URL: {resolved_stream}"),
                out.diagnostics,
            ));
        }
        let format = format_opt
            .map(|f| f.to_ascii_lowercase())
            .or_else(|| infer_audio_format(&resolved_stream))
            .unwrap_or_else(|| "mp3".into());

        Ok(SourceCallOutput {
            data: OnlineMusicStreamResult {
                stream_url: resolved_stream,
                format,
                bitrate,
                headers,
                expires_in_secs,
            },
            state: out.state,
            diagnostics: out.diagnostics,
        })
    }

    pub(super) fn resolve_cover_v2_sync(
        &self,
        manifest: &PluginV2Manifest,
        js_code: &str,
        initial_state: SourceRuntimeState,
        ctx: &MusicTrackContext,
        op_timeout: Duration,
        cancel_flag: Arc<AtomicBool>,
    ) -> Result<SourceCallOutput<Option<String>>, SourceCallError> {
        if !manifest.has_cover_resolver {
            return Ok(SourceCallOutput {
                data: ctx
                    .cover_url
                    .as_deref()
                    .map(str::trim)
                    .filter(|s| !s.is_empty())
                    .map(|u| resolve_relative_url(u, &manifest.base_url)),
                state: initial_state,
                diagnostics: Default::default(),
            });
        }

        let track_arg = track_ctx_to_js_arg(ctx);
        let out = PluginV2Runner::call_method_sync(
            js_code,
            manifest,
            initial_state,
            &self.http_config,
            self.js_timeout,
            op_timeout,
            cancel_flag,
            &["getCover"],
            &[serde_json::json!({
                "id": ctx.track_id,
                "trackId": ctx.track_id,
                "coverId": ctx.cover_id,
                "track": track_arg,
            })],
        )?;

        let raw_cover = match &out.data {
            serde_json::Value::String(s) if !s.trim().is_empty() => Some(s.trim().to_string()),
            serde_json::Value::Object(obj) => val_str(obj, &["coverUrl", "cover_url", "url"]),
            _ => None,
        };
        let resolved = raw_cover
            .map(|u| resolve_relative_url(&u, &manifest.base_url))
            .or_else(|| ctx.cover_url.clone());

        Ok(SourceCallOutput {
            data: resolved,
            state: out.state,
            diagnostics: out.diagnostics,
        })
    }

    pub(super) fn resolve_lyrics_v2_sync(
        &self,
        manifest: &PluginV2Manifest,
        js_code: &str,
        initial_state: SourceRuntimeState,
        ctx: &MusicTrackContext,
        op_timeout: Duration,
        cancel_flag: Arc<AtomicBool>,
    ) -> Result<SourceCallOutput<Option<OnlineMusicLyricsResult>>, SourceCallError> {
        if !manifest.has_lyrics {
            return Ok(SourceCallOutput {
                data: None,
                state: initial_state,
                diagnostics: Default::default(),
            });
        }

        let track_arg = track_ctx_to_js_arg(ctx);
        let out = PluginV2Runner::call_method_sync(
            js_code,
            manifest,
            initial_state,
            &self.http_config,
            self.js_timeout,
            op_timeout,
            cancel_flag,
            &["getLyrics"],
            &[serde_json::json!({
                "id": ctx.track_id,
                "trackId": ctx.track_id,
                "lyricId": ctx.lyric_id,
                "track": track_arg,
            })],
        )?;

        let (raw_lyric, raw_tlyric) = match &out.data {
            serde_json::Value::String(s) => (s.clone(), None),
            serde_json::Value::Object(obj) => {
                let lrc = val_str(obj, &["lyric", "lrc", "plainLyric"]).unwrap_or_default();
                let tlrc = val_str(obj, &["translatedLyric", "translated_lyric", "tlyric"]);
                (lrc, tlrc)
            }
            _ => (String::new(), None),
        };

        if raw_lyric.trim().is_empty() {
            return Ok(SourceCallOutput {
                data: None,
                state: out.state,
                diagnostics: out.diagnostics,
            });
        }

        let mut parsed = StructuredLyrics::from(raw_lyric.as_str());
        if parsed.display_title.is_none() && !ctx.title.trim().is_empty() {
            parsed.display_title = Some(ctx.title.clone());
        }
        if parsed.display_artist.is_none() && !ctx.artist.trim().is_empty() {
            parsed.display_artist = Some(ctx.artist.clone());
        }
        if parsed.is_empty() {
            return Ok(SourceCallOutput {
                data: None,
                state: out.state,
                diagnostics: out.diagnostics,
            });
        }

        let translated_lyrics = raw_tlyric.as_deref().and_then(|raw_t| {
            let t_parsed = StructuredLyrics::from(raw_t);
            (!t_parsed.is_empty()).then_some(t_parsed)
        });

        Ok(SourceCallOutput {
            data: Some(OnlineMusicLyricsResult {
                lyrics: parsed,
                translated_lyrics,
                raw_lyric,
                raw_translated_lyric: raw_tlyric,
            }),
            state: out.state,
            diagnostics: out.diagnostics,
        })
    }
}

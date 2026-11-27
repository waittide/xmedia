use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicU64};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use xmedia_core::domain::library::MediaKind;

use super::schema::{
    PluginAuthActionRequest, PluginAuthCapsDto, PluginExploreCategoryDto, PluginProfileDto,
    PluginSettingFieldDto, PluginSettingOptionDto, PluginV2Manifest,
};
use crate::http::{HttpClientConfig, LegadoHttpClient};
use crate::js_runtime::{JsExecutionContext, JsSandboxConfig, now_mono_ms};
use crate::model::{
    ExecutionDiagnostics, SourceCallError, SourceCallOutput, SourceMetadata, SourceRuntimeState,
};

/// Raw outcome produced by [`PluginV2Runner::run_auth_action`] before wrapping `SourceSummaryDto`.
#[derive(Debug, Clone)]
pub struct PluginV2AuthExecResult {
    pub ok: bool,
    pub status: Option<String>,
    pub message: Option<String>,
    pub qr_key: Option<String>,
    pub qr_url: Option<String>,
    pub qr_image: Option<String>,
    pub cooldown_secs: Option<u32>,
    pub profile: Option<PluginProfileDto>,
}

/// Strips leading UTF-8 BOM, whitespace, `//` line comments, and `/* */` block comments.
fn strip_leading_ws_and_comments(raw: &str) -> &str {
    let mut s = raw.trim_start_matches('\u{feff}').trim_start();
    loop {
        if let Some(rest) = s.strip_prefix("//") {
            match rest.find('\n') {
                Some(nl) => s = rest[nl + 1..].trim_start(),
                None => return "",
            }
        } else if let Some(rest) = s.strip_prefix("/*") {
            match rest.find("*/") {
                Some(end) => s = rest[end + 2..].trim_start(),
                None => return "",
            }
        } else {
            return s;
        }
    }
}

/// Returns `true` when `raw` looks like a JavaScript / ES Module plugin rather than JSON rules.
pub fn is_js_plugin_source(raw: &str) -> bool {
    let stripped = strip_leading_ws_and_comments(raw);
    if stripped.is_empty() || stripped.starts_with('{') || stripped.starts_with('[') {
        return false;
    }
    stripped.contains("definePlugin")
        || stripped.starts_with("export ")
        || stripped.contains("\nexport ")
        || stripped.contains("module.exports")
}

/// Merges schema `default_value` entries with persisted user `runtime_state.settings`.
pub fn build_effective_settings(
    schema: &[PluginSettingFieldDto],
    persisted: &HashMap<String, serde_json::Value>,
) -> serde_json::Value {
    let mut map = serde_json::Map::new();
    for field in schema {
        let val = persisted
            .get(&field.key)
            .cloned()
            .unwrap_or_else(|| field.default_value.clone());
        map.insert(field.key.clone(), val);
    }
    for (k, v) in persisted {
        map.entry(k.clone()).or_insert_with(|| v.clone());
    }
    serde_json::Value::Object(map)
}

fn slugify_id(name: &str) -> String {
    let slug: String = name
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' || c == '_' {
                c.to_ascii_lowercase()
            } else {
                '-'
            }
        })
        .collect();
    let trimmed = slug.trim_matches('-');
    if trimmed.is_empty() {
        "custom-plugin".to_string()
    } else {
        trimmed.to_string()
    }
}

static MANIFEST_CACHE: std::sync::LazyLock<
    Mutex<HashMap<String, Result<PluginV2Manifest, String>>>,
> = std::sync::LazyLock::new(|| Mutex::new(HashMap::new()));

/// Evaluates a Plugin v2 JS file inside an isolated, zero-network QuickJS sandbox (3s timeout)
/// to extract and validate its [`PluginV2Manifest`].
pub fn inspect_js_plugin_manifest(js_code: &str) -> Result<PluginV2Manifest, String> {
    let key = js_code.trim().to_string();
    {
        let guard = MANIFEST_CACHE.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(cached) = guard.get(&key) {
            return cached.clone();
        }
    }
    let res = inspect_js_plugin_manifest_uncached(&key);
    let mut guard = MANIFEST_CACHE.lock().unwrap_or_else(|e| e.into_inner());
    if guard.len() >= 256 {
        guard.clear();
    }
    guard.insert(key, res.clone());
    res
}

fn inspect_js_plugin_manifest_uncached(js_code: &str) -> Result<PluginV2Manifest, String> {
    let cookies = Arc::new(Mutex::new(HashMap::new()));
    let http = LegadoHttpClient::new_offline(cookies);
    let state = Arc::new(Mutex::new(SourceRuntimeState::default()));
    let diag = Arc::new(Mutex::new(ExecutionDiagnostics::default()));
    let mut cfg = JsSandboxConfig::with_timeout(Duration::from_secs(3));
    cfg.max_http_requests = 0;

    let mut js = JsExecutionContext::new(&SourceMetadata::default(), http, state, diag, cfg)?;
    js.load_v2_plugin(js_code, &serde_json::json!({}))?;

    let raw = js.eval_v2_json(
        r#"
        (function() {
            var p = globalThis.__x_v2_plugin || {};
            var a = p.auth || null;
            var authCaps = {
                qr_code: Boolean(a && a.qrCode && typeof a.qrCode.create === 'function' && typeof a.qrCode.poll === 'function'),
                otp: Boolean(a && a.otp && typeof a.otp.send === 'function' && typeof a.otp.verify === 'function'),
                password: Boolean(a && (typeof a.password === 'function' || (a.password && typeof a.password.login === 'function'))),
                cookie: Boolean(a && a.cookie),
                has_profile: Boolean(a && typeof a.getProfile === 'function'),
                has_logout: Boolean(a && typeof a.logout === 'function'),
                description: (a && typeof a.description === 'string' && a.description.trim()) ? a.description.trim() : null,
                cookie_hint: (a && a.cookie && typeof a.cookie.hint === 'string') ? a.cookie.hint : ((a && typeof a.cookieHint === 'string') ? a.cookieHint : null)
            };
            var rawSettings = Array.isArray(p.settings) ? p.settings : [];
            var settings = rawSettings.map(function(s) {
                var opts = Array.isArray(s.options) ? s.options.map(function(o) {
                    return { label: String(o.label != null ? o.label : o.value), value: String(o.value != null ? o.value : '') };
                }) : [];
                return {
                    key: String(s.key || '').trim(),
                    label: String(s.label || s.key || '').trim(),
                    type: String(s.type || 'text').trim().toLowerCase(),
                    default_value: s.default !== undefined ? s.default : (s.defaultValue !== undefined ? s.defaultValue : null),
                    options: opts,
                    description: typeof s.description === 'string' ? s.description : null,
                    placeholder: typeof s.placeholder === 'string' ? s.placeholder : null
                };
            }).filter(function(s) { return s.key.length > 0; });
            var rawCats = Array.isArray(p.exploreCategories) ? p.exploreCategories : [];
            var cats = rawCats.map(function(c) {
                return {
                    title: String(c.title || c.name || '').trim(),
                    id: String(c.id != null ? c.id : (c.url != null ? c.url : '')).trim()
                };
            }).filter(function(c) { return c.title.length > 0 && c.id.length > 0; });
            return {
                id: String(p.id || '').trim(),
                name: String(p.name || p.sourceName || '').trim(),
                version: String(p.version || '1.0.0').trim(),
                domain: String(p.domain || '').trim().toLowerCase(),
                baseUrl: String(p.baseUrl || p.sourceUrl || '').trim(),
                group: String(p.group || p.sourceGroup || '').trim(),
                description: typeof p.description === 'string' ? p.description.trim() : (typeof p.comment === 'string' ? p.comment.trim() : null),
                checkKeyword: typeof p.checkKeyword === 'string' ? p.checkKeyword.trim() : null,
                rateLimit: typeof p.rateLimit === 'string' ? p.rateLimit.trim() : null,
                headers: (p.headers && typeof p.headers === 'object' && !Array.isArray(p.headers)) ? p.headers : {},
                mediaHeaders: (p.mediaHeaders && typeof p.mediaHeaders === 'object' && !Array.isArray(p.mediaHeaders)) ? p.mediaHeaders : {},
                qualities: Array.isArray(p.qualities) ? p.qualities.map(String) : [],
                exploreCategories: cats,
                hasSearch: typeof p.search === 'function',
                hasCategories: typeof p.getCategories === 'function' || cats.length > 0,
                hasExplore: typeof p.explore === 'function' || typeof p.browse === 'function',
                hasDetail: typeof p.getTrackInfo === 'function' || typeof p.getDetail === 'function' || typeof p.getBookInfo === 'function' || typeof p.getComicDetail === 'function',
                hasToc: typeof p.getToc === 'function' || typeof p.getChapters === 'function',
                hasContent: typeof p.getChapterContent === 'function' || typeof p.getContent === 'function' || typeof p.getPages === 'function',
                hasStream: typeof p.getStream === 'function' || typeof p.getPlayInfo === 'function',
                hasLyrics: typeof p.getLyrics === 'function',
                hasCoverResolver: typeof p.getCover === 'function',
                auth: authCaps,
                settings: settings,
                enabled: p.enabled !== undefined ? Boolean(p.enabled) : true,
                weight: typeof p.weight === 'number' && isFinite(p.weight) ? Math.trunc(p.weight) : 0
            };
        })()
        "#,
    )?;

    let obj = raw
        .as_object()
        .ok_or_else(|| "解析插件元数据对象失败".to_string())?;
    let domain_str = obj
        .get("domain")
        .and_then(|v| v.as_str())
        .unwrap_or_default();
    let domain = MediaKind::parse(domain_str).map_err(|_| {
        format!(
            "JS 插件缺少有效的 domain 字段（需为 book / comic / music / video，当前为 '{domain_str}'）"
        )
    })?;
    let name = obj
        .get("name")
        .and_then(|v| v.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .ok_or_else(|| "JS 插件缺少非空 name 字段".to_string())?
        .to_string();
    let base_url = obj
        .get("baseUrl")
        .and_then(|v| v.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .ok_or_else(|| "JS 插件缺少非空 baseUrl 字段".to_string())?
        .to_string();
    let parsed_base = url::Url::parse(&base_url)
        .map_err(|e| format!("JS 插件 baseUrl '{base_url}' 无效: {e}"))?;
    if !matches!(parsed_base.scheme(), "http" | "https") {
        return Err(format!("JS 插件 baseUrl 仅支持 http/https: '{base_url}'"));
    }

    let id = obj
        .get("id")
        .and_then(|v| v.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(ToString::to_string)
        .unwrap_or_else(|| slugify_id(&name));
    let version = obj
        .get("version")
        .and_then(|v| v.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .unwrap_or("1.0.0")
        .to_string();
    let group = obj
        .get("group")
        .and_then(|v| v.as_str())
        .unwrap_or_default()
        .trim()
        .to_string();
    let description = obj
        .get("description")
        .and_then(|v| v.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(ToString::to_string);
    let check_keyword = obj
        .get("checkKeyword")
        .and_then(|v| v.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(ToString::to_string);
    let rate_limit = obj
        .get("rateLimit")
        .and_then(|v| v.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(ToString::to_string);

    let mut headers = HashMap::new();
    if let Some(hdr_obj) = obj.get("headers").and_then(|v| v.as_object()) {
        for (k, v) in hdr_obj {
            if let Some(s) = v.as_str() {
                headers.insert(k.clone(), s.to_string());
            }
        }
    }

    let mut media_headers = HashMap::new();
    if let Some(hdr_obj) = obj.get("mediaHeaders").and_then(|v| v.as_object()) {
        for (k, v) in hdr_obj {
            if let Some(s) = v.as_str() {
                media_headers.insert(k.clone(), s.to_string());
            }
        }
    }

    let qualities: Vec<String> = obj
        .get("qualities")
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|v| v.as_str().map(|s| s.trim().to_string()))
                .filter(|s| !s.is_empty())
                .collect()
        })
        .unwrap_or_default();

    let explore_categories: Vec<PluginExploreCategoryDto> = obj
        .get("exploreCategories")
        .cloned()
        .and_then(|v| serde_json::from_value(v).ok())
        .unwrap_or_default();

    let auth: PluginAuthCapsDto = obj
        .get("auth")
        .cloned()
        .and_then(|v| serde_json::from_value(v).ok())
        .unwrap_or_default();

    let mut settings: Vec<PluginSettingFieldDto> = obj
        .get("settings")
        .cloned()
        .and_then(|v| serde_json::from_value(v).ok())
        .unwrap_or_default();
    for field in &mut settings {
        if !matches!(
            field.field_type.as_str(),
            "select" | "switch" | "text" | "password"
        ) {
            field.field_type = "text".to_string();
        }
        if field.field_type == "select" && field.options.is_empty() {
            field.options.push(PluginSettingOptionDto {
                label: field.label.clone(),
                value: field.default_value.as_str().unwrap_or_default().to_string(),
            });
        }
    }

    Ok(PluginV2Manifest {
        id,
        name,
        version,
        domain,
        base_url,
        group,
        description,
        check_keyword,
        rate_limit,
        headers,
        media_headers,
        qualities,
        explore_categories,
        has_search: obj
            .get("hasSearch")
            .and_then(|v| v.as_bool())
            .unwrap_or(false),
        has_categories: obj
            .get("hasCategories")
            .and_then(|v| v.as_bool())
            .unwrap_or(false),
        has_explore: obj
            .get("hasExplore")
            .and_then(|v| v.as_bool())
            .unwrap_or(false),
        has_detail: obj
            .get("hasDetail")
            .and_then(|v| v.as_bool())
            .unwrap_or(false),
        has_toc: obj.get("hasToc").and_then(|v| v.as_bool()).unwrap_or(false),
        has_content: obj
            .get("hasContent")
            .and_then(|v| v.as_bool())
            .unwrap_or(false),
        has_stream: obj
            .get("hasStream")
            .and_then(|v| v.as_bool())
            .unwrap_or(false),
        has_lyrics: obj
            .get("hasLyrics")
            .and_then(|v| v.as_bool())
            .unwrap_or(false),
        has_cover_resolver: obj
            .get("hasCoverResolver")
            .and_then(|v| v.as_bool())
            .unwrap_or(false),
        auth,
        settings,
        enabled: obj.get("enabled").and_then(|v| v.as_bool()).unwrap_or(true),
        weight: obj.get("weight").and_then(|v| v.as_i64()).unwrap_or(0),
    })
}

/// Stateless executor for Plugin v2 JS modules across all domains (`book`, `comic`, `music`, `video`).
pub struct PluginV2Runner;

impl PluginV2Runner {
    #[allow(clippy::too_many_arguments)]
    pub fn call_method_sync(
        js_code: &str,
        manifest: &PluginV2Manifest,
        initial_state: SourceRuntimeState,
        http_config: &HttpClientConfig,
        js_timeout: Duration,
        op_timeout: Duration,
        cancel_flag: Arc<AtomicBool>,
        method_path: &[&str],
        args: &[serde_json::Value],
    ) -> Result<SourceCallOutput<serde_json::Value>, SourceCallError> {
        Self::call_any_method_sync(
            js_code,
            manifest,
            initial_state,
            http_config,
            js_timeout,
            op_timeout,
            cancel_flag,
            &[method_path],
            args,
        )
    }

    #[allow(clippy::too_many_arguments)]
    pub fn call_any_method_sync(
        js_code: &str,
        manifest: &PluginV2Manifest,
        mut initial_state: SourceRuntimeState,
        http_config: &HttpClientConfig,
        js_timeout: Duration,
        op_timeout: Duration,
        cancel_flag: Arc<AtomicBool>,
        candidate_paths: &[&[&str]],
        args: &[serde_json::Value],
    ) -> Result<SourceCallOutput<serde_json::Value>, SourceCallError> {
        initial_state.ensure_android_id(&manifest.base_url);
        let effective_settings =
            build_effective_settings(&manifest.settings, &initial_state.settings);
        let cookies_arc = Arc::new(Mutex::new(initial_state.cookies.clone()));
        let state_arc = Arc::new(Mutex::new(initial_state));
        let diag_arc = Arc::new(Mutex::new(ExecutionDiagnostics::default()));

        let header_json = if manifest.headers.is_empty() {
            None
        } else {
            serde_json::to_string(&manifest.headers).ok()
        };
        let http = LegadoHttpClient::new(
            http_config.clone(),
            cookies_arc.clone(),
            header_json.as_deref(),
            true,
        )
        .map_err(|e| SourceCallError::new(e, ExecutionDiagnostics::default()))?;

        let meta = SourceMetadata {
            source_name: manifest.name.clone(),
            source_url: manifest.base_url.clone(),
            header: header_json,
            enabled_cookie_jar: true,
            ..Default::default()
        };
        let op_deadline_ms = Arc::new(AtomicU64::new(
            now_mono_ms().saturating_add(op_timeout.as_millis() as u64),
        ));

        let mut js = JsExecutionContext::new_with_control(
            &meta,
            http,
            state_arc.clone(),
            diag_arc.clone(),
            JsSandboxConfig::with_timeout(js_timeout),
            op_deadline_ms,
            cancel_flag,
        )
        .map_err(|e| {
            let d = diag_arc.lock().unwrap_or_else(|e| e.into_inner()).clone();
            SourceCallError::new(e, d)
        })?;

        let exec_res = (|| -> Result<serde_json::Value, String> {
            js.load_v2_plugin(js_code, &effective_settings)?;
            let candidates_json =
                serde_json::to_string(candidate_paths).map_err(|e| e.to_string())?;
            let match_expr = format!(
                "(function() {{ var candidates = {candidates_json}; for (var c = 0; c < candidates.length; c++) {{ var p = candidates[c]; var t = globalThis.__x_v2_plugin; for (var i = 0; i < p.length; i++) {{ if (t == null) break; t = t[p[i]]; }} if (typeof t === 'function') return c; }} return -1; }})()"
            );
            let matched_idx = js.eval_v2_json(&match_expr)?.as_i64().unwrap_or(-1);
            if matched_idx < 0 {
                let names = candidate_paths
                    .iter()
                    .map(|p| p.join("."))
                    .collect::<Vec<_>>()
                    .join(" / ");
                return Err(format!("插件「{}」未实现方法: {names}", manifest.name));
            }
            js.call_v2_method(candidate_paths[matched_idx as usize], args)
        })();

        let mut final_state = state_arc.lock().unwrap_or_else(|e| e.into_inner()).clone();
        final_state.cookies = cookies_arc
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone();
        let diagnostics = diag_arc.lock().unwrap_or_else(|e| e.into_inner()).clone();

        match exec_res {
            Ok(data) => Ok(SourceCallOutput {
                data,
                state: final_state,
                diagnostics,
            }),
            Err(err) => Err(SourceCallError::new(err, diagnostics)),
        }
    }

    pub fn has_method_sync(js_code: &str, method_path: &[&str]) -> bool {
        let cookies = Arc::new(Mutex::new(HashMap::new()));
        let http = LegadoHttpClient::new_offline(cookies);
        let state = Arc::new(Mutex::new(SourceRuntimeState::default()));
        let diag = Arc::new(Mutex::new(ExecutionDiagnostics::default()));
        let mut cfg = JsSandboxConfig::with_timeout(Duration::from_secs(2));
        cfg.max_http_requests = 0;
        let Ok(mut js) =
            JsExecutionContext::new(&SourceMetadata::default(), http, state, diag, cfg)
        else {
            return false;
        };
        if js.load_v2_plugin(js_code, &serde_json::json!({})).is_err() {
            return false;
        }
        let path_json = serde_json::to_string(method_path).unwrap_or_else(|_| "[]".into());
        let check_expr = format!(
            "(function() {{ var t = globalThis.__x_v2_plugin; var p = {path_json}; for (var i = 0; i < p.length; i++) {{ if (t == null) return false; t = t[p[i]]; }} return typeof t === 'function'; }})()"
        );
        js.eval_v2_json(&check_expr)
            .ok()
            .and_then(|v| v.as_bool())
            .unwrap_or(false)
    }

    /// Executes a structured Contract Auth action (`status`, `qr_create`, `qr_poll`, `otp_send`,
    /// `otp_verify`, `password_login`, `cookie_login`, `logout`) in a single QuickJS session.
    pub fn run_auth_action_sync(
        js_code: &str,
        manifest: &PluginV2Manifest,
        mut initial_state: SourceRuntimeState,
        req: PluginAuthActionRequest,
        http_config: &HttpClientConfig,
        op_timeout: Duration,
    ) -> Result<SourceCallOutput<PluginV2AuthExecResult>, SourceCallError> {
        initial_state.ensure_android_id(&manifest.base_url);
        let effective_settings =
            build_effective_settings(&manifest.settings, &initial_state.settings);
        let cookies_arc = Arc::new(Mutex::new(initial_state.cookies.clone()));
        let state_arc = Arc::new(Mutex::new(initial_state));
        let diag_arc = Arc::new(Mutex::new(ExecutionDiagnostics::default()));

        let header_json = if manifest.headers.is_empty() {
            None
        } else {
            serde_json::to_string(&manifest.headers).ok()
        };
        let http = LegadoHttpClient::new(
            http_config.clone(),
            cookies_arc.clone(),
            header_json.as_deref(),
            true,
        )
        .map_err(|e| SourceCallError::new(e, ExecutionDiagnostics::default()))?;

        let meta = SourceMetadata {
            source_name: manifest.name.clone(),
            source_url: manifest.base_url.clone(),
            header: header_json,
            enabled_cookie_jar: true,
            ..Default::default()
        };
        let op_deadline_ms = Arc::new(AtomicU64::new(
            now_mono_ms().saturating_add(op_timeout.as_millis() as u64),
        ));
        let cancel_flag = Arc::new(AtomicBool::new(false));

        let mut js = JsExecutionContext::new_with_control(
            &meta,
            http.clone(),
            state_arc.clone(),
            diag_arc.clone(),
            JsSandboxConfig::with_timeout(op_timeout),
            op_deadline_ms,
            cancel_flag,
        )
        .map_err(|e| {
            let d = diag_arc.lock().unwrap_or_else(|e| e.into_inner()).clone();
            SourceCallError::new(e, d)
        })?;

        let parse_profile = |val: &serde_json::Value| -> Option<PluginProfileDto> {
            let obj = val.as_object()?;
            let logged_in = obj
                .get("loggedIn")
                .or_else(|| obj.get("logged_in"))
                .and_then(|v| v.as_bool())
                .unwrap_or_else(|| {
                    obj.get("nickname")
                        .or_else(|| obj.get("userId"))
                        .or_else(|| obj.get("user_id"))
                        .and_then(|v| v.as_str())
                        .is_some_and(|s| !s.trim().is_empty())
                });
            let str_field = |k1: &str, k2: &str| -> Option<String> {
                obj.get(k1).or_else(|| obj.get(k2)).and_then(|v| match v {
                    serde_json::Value::String(s) if !s.trim().is_empty() => {
                        Some(s.trim().to_string())
                    }
                    serde_json::Value::Number(n) => Some(n.to_string()),
                    _ => None,
                })
            };
            Some(PluginProfileDto {
                logged_in,
                user_id: str_field("userId", "user_id"),
                nickname: str_field("nickname", "name"),
                avatar_url: str_field("avatarUrl", "avatar_url"),
                vip_label: str_field("vipLabel", "vip_label"),
                message: str_field("message", "msg"),
            })
        };

        let exec_res = (|| -> Result<PluginV2AuthExecResult, String> {
            js.load_v2_plugin(js_code, &effective_settings)?;

            match req {
                PluginAuthActionRequest::Status => {
                    let profile = if manifest.auth.has_profile {
                        let raw = js.call_v2_method(&["auth", "getProfile"], &[])?;
                        parse_profile(&raw).unwrap_or_default()
                    } else {
                        let has_cookies = !cookies_arc
                            .lock()
                            .unwrap_or_else(|e| e.into_inner())
                            .is_empty();
                        PluginProfileDto {
                            logged_in: has_cookies,
                            ..Default::default()
                        }
                    };
                    Ok(PluginV2AuthExecResult {
                        ok: true,
                        status: Some(if profile.logged_in {
                            "logged_in".into()
                        } else {
                            "logged_out".into()
                        }),
                        message: profile.message.clone(),
                        qr_key: None,
                        qr_url: None,
                        qr_image: None,
                        cooldown_secs: None,
                        profile: Some(profile),
                    })
                }
                PluginAuthActionRequest::QrCreate => {
                    if !manifest.auth.qr_code {
                        return Err("该插件不支持扫码登录".into());
                    }
                    let raw = js.call_v2_method(&["auth", "qrCode", "create"], &[])?;
                    let obj = raw
                        .as_object()
                        .ok_or_else(|| "auth.qrCode.create 返回格式无效".to_string())?;
                    let key = obj
                        .get("key")
                        .or_else(|| obj.get("qr_key"))
                        .and_then(|v| v.as_str())
                        .map(str::trim)
                        .filter(|s| !s.is_empty())
                        .ok_or_else(|| "auth.qrCode.create 未返回有效的二维码 key".to_string())?
                        .to_string();
                    let qr_url = obj
                        .get("qrUrl")
                        .or_else(|| obj.get("url"))
                        .or_else(|| obj.get("qr_url"))
                        .and_then(|v| v.as_str())
                        .map(str::trim)
                        .filter(|s| !s.is_empty())
                        .map(ToString::to_string);
                    let qr_image = obj
                        .get("qrImage")
                        .or_else(|| obj.get("image"))
                        .or_else(|| obj.get("qr_image"))
                        .and_then(|v| v.as_str())
                        .map(str::trim)
                        .filter(|s| !s.is_empty())
                        .map(ToString::to_string);
                    let message = obj
                        .get("message")
                        .and_then(|v| v.as_str())
                        .map(ToString::to_string);
                    Ok(PluginV2AuthExecResult {
                        ok: true,
                        status: Some("waiting".into()),
                        message,
                        qr_key: Some(key),
                        qr_url,
                        qr_image,
                        cooldown_secs: None,
                        profile: None,
                    })
                }
                PluginAuthActionRequest::QrPoll { key } => {
                    if !manifest.auth.qr_code {
                        return Err("该插件不支持扫码登录".into());
                    }
                    let raw = js.call_v2_method(
                        &["auth", "qrCode", "poll"],
                        &[serde_json::json!({ "key": key })],
                    )?;
                    let obj = raw
                        .as_object()
                        .ok_or_else(|| "auth.qrCode.poll 返回格式无效".to_string())?;
                    let status = obj
                        .get("status")
                        .and_then(|v| v.as_str())
                        .unwrap_or("waiting")
                        .trim()
                        .to_ascii_lowercase();
                    let message = obj
                        .get("message")
                        .and_then(|v| v.as_str())
                        .map(ToString::to_string);
                    let mut profile = obj.get("profile").and_then(parse_profile);
                    if status == "confirmed"
                        && profile.is_none()
                        && manifest.auth.has_profile
                        && let Ok(p_raw) = js.call_v2_method(&["auth", "getProfile"], &[])
                    {
                        profile = parse_profile(&p_raw);
                    }
                    Ok(PluginV2AuthExecResult {
                        ok: true,
                        status: Some(status),
                        message,
                        qr_key: Some(key),
                        qr_url: None,
                        qr_image: None,
                        cooldown_secs: None,
                        profile,
                    })
                }
                PluginAuthActionRequest::OtpSend {
                    country_code,
                    phone,
                } => {
                    if !manifest.auth.otp {
                        return Err("该插件不支持短信验证码登录".into());
                    }
                    let trimmed_phone = phone.trim();
                    if trimmed_phone.is_empty() {
                        return Err("请输入手机号".into());
                    }
                    let raw = js.call_v2_method(
                        &["auth", "otp", "send"],
                        &[serde_json::json!({
                            "countryCode": country_code.trim(),
                            "phone": trimmed_phone,
                        })],
                    )?;
                    let obj = raw.as_object();
                    let ok = obj
                        .and_then(|o| o.get("ok"))
                        .and_then(|v| v.as_bool())
                        .unwrap_or(true);
                    let message = obj
                        .and_then(|o| o.get("message"))
                        .and_then(|v| v.as_str())
                        .map(ToString::to_string);
                    let cooldown = obj
                        .and_then(|o| o.get("cooldownSecs").or_else(|| o.get("cooldown_secs")))
                        .and_then(|v| v.as_u64())
                        .map(|n| n.clamp(1, 600) as u32)
                        .unwrap_or(60);
                    Ok(PluginV2AuthExecResult {
                        ok,
                        status: Some(if ok { "sent".into() } else { "failed".into() }),
                        message,
                        qr_key: None,
                        qr_url: None,
                        qr_image: None,
                        cooldown_secs: Some(cooldown),
                        profile: None,
                    })
                }
                PluginAuthActionRequest::OtpVerify {
                    country_code,
                    phone,
                    code,
                } => {
                    if !manifest.auth.otp {
                        return Err("该插件不支持短信验证码登录".into());
                    }
                    if phone.trim().is_empty() || code.trim().is_empty() {
                        return Err("请输入手机号和验证码".into());
                    }
                    let raw = js.call_v2_method(
                        &["auth", "otp", "verify"],
                        &[serde_json::json!({
                            "countryCode": country_code.trim(),
                            "phone": phone.trim(),
                            "code": code.trim(),
                        })],
                    )?;
                    let obj = raw.as_object();
                    let ok = obj
                        .and_then(|o| o.get("ok"))
                        .and_then(|v| v.as_bool())
                        .unwrap_or(true);
                    let message = obj
                        .and_then(|o| o.get("message"))
                        .and_then(|v| v.as_str())
                        .map(ToString::to_string);
                    let mut profile = obj.and_then(|o| o.get("profile")).and_then(parse_profile);
                    if ok
                        && profile.is_none()
                        && manifest.auth.has_profile
                        && let Ok(p_raw) = js.call_v2_method(&["auth", "getProfile"], &[])
                    {
                        profile = parse_profile(&p_raw);
                    }
                    Ok(PluginV2AuthExecResult {
                        ok,
                        status: Some(if ok {
                            "confirmed".into()
                        } else {
                            "failed".into()
                        }),
                        message,
                        qr_key: None,
                        qr_url: None,
                        qr_image: None,
                        cooldown_secs: None,
                        profile,
                    })
                }
                PluginAuthActionRequest::PasswordLogin { username, password } => {
                    if !manifest.auth.password {
                        return Err("该插件不支持账号密码登录".into());
                    }
                    if username.trim().is_empty() || password.is_empty() {
                        return Err("请输入账号和密码".into());
                    }
                    let has_login_fn = js
                        .eval_v2_json(
                            "Boolean(globalThis.__x_v2_plugin.auth && globalThis.__x_v2_plugin.auth.password && typeof globalThis.__x_v2_plugin.auth.password.login === 'function')",
                        )?
                        .as_bool()
                        .unwrap_or(false);
                    let path: &[&str] = if has_login_fn {
                        &["auth", "password", "login"]
                    } else {
                        &["auth", "password"]
                    };
                    let raw = js.call_v2_method(
                        path,
                        &[serde_json::json!({
                            "username": username.trim(),
                            "password": password,
                        })],
                    )?;
                    let obj = raw.as_object();
                    let ok = obj
                        .and_then(|o| o.get("ok"))
                        .and_then(|v| v.as_bool())
                        .unwrap_or(true);
                    let message = obj
                        .and_then(|o| o.get("message"))
                        .and_then(|v| v.as_str())
                        .map(ToString::to_string);
                    let mut profile = obj.and_then(|o| o.get("profile")).and_then(parse_profile);
                    if ok
                        && profile.is_none()
                        && manifest.auth.has_profile
                        && let Ok(p_raw) = js.call_v2_method(&["auth", "getProfile"], &[])
                    {
                        profile = parse_profile(&p_raw);
                    }
                    Ok(PluginV2AuthExecResult {
                        ok,
                        status: Some(if ok {
                            "confirmed".into()
                        } else {
                            "failed".into()
                        }),
                        message,
                        qr_key: None,
                        qr_url: None,
                        qr_image: None,
                        cooldown_secs: None,
                        profile,
                    })
                }
                PluginAuthActionRequest::CookieLogin { cookie } => {
                    if !manifest.auth.cookie {
                        return Err("该插件不支持 Cookie 登录".into());
                    }
                    let trimmed_cookie = cookie.trim();
                    if trimmed_cookie.is_empty() {
                        return Err("请输入有效的 Cookie 字符串".into());
                    }
                    let has_cookie_fn = js
                        .eval_v2_json(
                            "Boolean(globalThis.__x_v2_plugin.auth && globalThis.__x_v2_plugin.auth.cookie && typeof globalThis.__x_v2_plugin.auth.cookie.login === 'function')",
                        )?
                        .as_bool()
                        .unwrap_or(false);
                    let mut message = None;
                    let mut profile = None;
                    let mut ok = true;
                    if has_cookie_fn {
                        let raw = js.call_v2_method(
                            &["auth", "cookie", "login"],
                            &[serde_json::json!({ "cookie": trimmed_cookie })],
                        )?;
                        if let Some(obj) = raw.as_object() {
                            ok = obj.get("ok").and_then(|v| v.as_bool()).unwrap_or(true);
                            message = obj
                                .get("message")
                                .and_then(|v| v.as_str())
                                .map(ToString::to_string);
                            profile = obj.get("profile").and_then(parse_profile);
                        }
                    } else if !http.set_cookie(&manifest.base_url, trimmed_cookie) {
                        return Err("写入 Cookie 失败（超出长度上限或格式无效）".into());
                    }
                    if ok
                        && profile.is_none()
                        && manifest.auth.has_profile
                        && let Ok(p_raw) = js.call_v2_method(&["auth", "getProfile"], &[])
                    {
                        let parsed = parse_profile(&p_raw);
                        if let Some(ref p) = parsed
                            && !p.logged_in
                        {
                            ok = false;
                            if message.is_none() {
                                message = Some(
                                    p.message
                                        .clone()
                                        .unwrap_or_else(|| "Cookie 未能通过登录态校验".into()),
                                );
                            }
                        }
                        profile = parsed;
                    }
                    Ok(PluginV2AuthExecResult {
                        ok,
                        status: Some(if ok {
                            "confirmed".into()
                        } else {
                            "failed".into()
                        }),
                        message,
                        qr_key: None,
                        qr_url: None,
                        qr_image: None,
                        cooldown_secs: None,
                        profile,
                    })
                }
                PluginAuthActionRequest::Logout => {
                    if manifest.auth.has_logout {
                        let _ = js.call_v2_method(&["auth", "logout"], &[]);
                    }
                    http.clear_all_cookies();
                    {
                        let mut st = state_arc.lock().unwrap_or_else(|e| e.into_inner());
                        st.cookies.clear();
                        st.login_info.clear();
                    }
                    Ok(PluginV2AuthExecResult {
                        ok: true,
                        status: Some("logged_out".into()),
                        message: Some("已退出登录并清除账号凭证".into()),
                        qr_key: None,
                        qr_url: None,
                        qr_image: None,
                        cooldown_secs: None,
                        profile: Some(PluginProfileDto {
                            logged_in: false,
                            ..Default::default()
                        }),
                    })
                }
            }
        })();

        let mut final_state = state_arc.lock().unwrap_or_else(|e| e.into_inner()).clone();
        final_state.cookies = cookies_arc
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone();
        let diagnostics = diag_arc.lock().unwrap_or_else(|e| e.into_inner()).clone();

        match exec_res {
            Ok(data) => Ok(SourceCallOutput {
                data,
                state: final_state,
                diagnostics,
            }),
            Err(err) => Err(SourceCallError::new(err, diagnostics)),
        }
    }
}

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use std::time::Duration;

use anyhow::{Context, bail};
use clap::{Args, Subcommand};
use xmedia_source::crud::infer_plugin_domain;
use xmedia_source::http::HttpClientConfig;
use xmedia_source::model::{ExecutionDiagnostics, SourceRuntimeState};
use xmedia_source::plugin_v2::{
    PluginAuthActionRequest, PluginV2Manifest, PluginV2Runner, inspect_js_plugin_manifest,
    is_js_plugin_source,
};

#[derive(Debug, Clone, Args)]
pub struct PluginExecFlags {
    /// Optional JSON file persisting isolated runtime state (cookies, storage, settings) across runs
    #[arg(long, value_name = "FILE")]
    pub state: Option<PathBuf>,

    /// Allow requests to loopback/private IP addresses for local mock servers
    #[arg(long)]
    pub allow_private_ips: bool,

    /// Optional HTTP/SOCKS5 proxy URL (e.g. http://127.0.0.1:7890)
    #[arg(long)]
    pub proxy: Option<String>,

    /// Per-operation timeout in seconds
    #[arg(long, default_value_t = 30)]
    pub timeout_secs: u64,
}

#[derive(Debug, Clone, Subcommand)]
pub enum PluginAuthSubcommand {
    /// Inspect current authentication status (`auth.getProfile`)
    Status,
    /// Create a login QR code (`auth.qrCode.create`)
    QrCreate,
    /// Poll a login QR code (`auth.qrCode.poll`)
    QrPoll {
        #[arg(long)]
        key: String,
    },
    /// Send an SMS OTP verification code (`auth.otp.send`)
    OtpSend {
        #[arg(long)]
        phone: String,
        #[arg(long, default_value = "86")]
        country_code: String,
    },
    /// Verify an SMS OTP code and persist login cookies (`auth.otp.verify`)
    OtpVerify {
        #[arg(long)]
        phone: String,
        #[arg(long)]
        code: String,
        #[arg(long, default_value = "86")]
        country_code: String,
    },
    /// Login with username and password (`auth.password`)
    Password {
        #[arg(long)]
        username: String,
        #[arg(long)]
        password: String,
    },
    /// Login by importing a raw Cookie header string (`auth.cookie`)
    Cookie {
        #[arg(long)]
        cookie: String,
    },
    /// Logout and clear persisted cookies and login state (`auth.logout`)
    Logout,
}

#[derive(Debug, Clone, Subcommand)]
pub enum PluginCommand {
    /// Validate plugin manifest (.js/.mjs) or rule structure (.json) without network access
    Check {
        /// Path to the plugin file (.js, .mjs, or .json)
        file: PathBuf,
    },
    /// Execute `search(ctx, { keyword, page, limit })` on a Plugin v2 JS module
    Search {
        file: PathBuf,
        keyword: String,
        #[arg(long, default_value_t = 1)]
        page: u32,
        #[arg(long, default_value_t = 20)]
        limit: u32,
        #[command(flatten)]
        flags: PluginExecFlags,
    },
    /// Execute `explore` / `browse` on a Plugin v2 JS module
    Explore {
        file: PathBuf,
        #[arg(long)]
        category: Option<String>,
        #[arg(long, default_value_t = 1)]
        page: u32,
        #[arg(long, default_value_t = 20)]
        limit: u32,
        #[command(flatten)]
        flags: PluginExecFlags,
    },
    /// Execute `getDetail` / `getTrackInfo` / `getBookInfo` / `getComicDetail`
    Detail {
        file: PathBuf,
        #[arg(long)]
        id: Option<String>,
        #[arg(long)]
        url: Option<String>,
        #[command(flatten)]
        flags: PluginExecFlags,
    },
    /// Execute `getToc` / `getChapters` for book or comic plugins
    Toc {
        file: PathBuf,
        #[arg(long)]
        url: String,
        #[command(flatten)]
        flags: PluginExecFlags,
    },
    /// Execute `getChapterContent` / `getContent` / `getPages` for book or comic plugins
    Content {
        file: PathBuf,
        #[arg(long)]
        url: String,
        #[arg(long, default_value_t = 0)]
        index: usize,
        #[arg(long, default_value = "")]
        title: String,
        #[command(flatten)]
        flags: PluginExecFlags,
    },
    /// Execute `getStream` / `getPlayInfo` for music or video plugins
    Stream {
        file: PathBuf,
        #[arg(long)]
        id: Option<String>,
        #[arg(long)]
        url: Option<String>,
        #[arg(long)]
        quality: Option<String>,
        #[command(flatten)]
        flags: PluginExecFlags,
    },
    /// Execute `getLyrics` for music plugins
    Lyrics {
        file: PathBuf,
        #[arg(long)]
        id: String,
        #[command(flatten)]
        flags: PluginExecFlags,
    },
    /// Execute Contract Auth actions (`status`, `qr-create`, `qr-poll`, `otp-send`, `otp-verify`, `cookie`, `logout`)
    Auth {
        file: PathBuf,
        #[command(subcommand)]
        auth_cmd: PluginAuthSubcommand,
        #[command(flatten)]
        flags: PluginExecFlags,
    },
    /// Read or update user settings in `--state <file>`
    Set {
        file: PathBuf,
        /// Setting assignment in `key=value` format (JSON literal or plain string)
        assignment: String,
        #[arg(long, value_name = "FILE")]
        state: PathBuf,
    },
}

fn load_state_file(path: Option<&Path>) -> anyhow::Result<SourceRuntimeState> {
    let Some(p) = path else {
        return Ok(SourceRuntimeState::default());
    };
    if !p.exists() {
        return Ok(SourceRuntimeState::default());
    }
    let raw = std::fs::read_to_string(p)
        .with_context(|| format!("failed to read state file {}", p.display()))?;
    if raw.trim().is_empty() {
        return Ok(SourceRuntimeState::default());
    }
    serde_json::from_str(&raw)
        .with_context(|| format!("failed to parse state file {}", p.display()))
}

fn save_state_file(path: Option<&Path>, state: &SourceRuntimeState) -> anyhow::Result<()> {
    let Some(p) = path else {
        return Ok(());
    };
    if let Some(parent) = p.parent()
        && !parent.as_os_str().is_empty()
    {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("failed creating parent directory {}", parent.display()))?;
    }
    let serialized = serde_json::to_string_pretty(state)?;
    std::fs::write(p, serialized)
        .with_context(|| format!("failed writing state file {}", p.display()))
}

fn build_http_config(flags: &PluginExecFlags) -> HttpClientConfig {
    HttpClientConfig {
        timeout_secs: flags.timeout_secs.max(3),
        allow_private_ips: flags.allow_private_ips,
        proxy_url: flags.proxy.clone(),
    }
}

fn emit_diagnostics(diag: &ExecutionDiagnostics) {
    for log in &diag.logs {
        eprintln!("[plugin:log] {log}");
    }
    for toast in &diag.toasts {
        eprintln!("[plugin:toast] {toast}");
    }
}

fn load_v2_file(path: &Path) -> anyhow::Result<(String, PluginV2Manifest)> {
    let raw = std::fs::read_to_string(path)
        .with_context(|| format!("failed reading plugin file {}", path.display()))?;
    if !is_js_plugin_source(&raw) {
        bail!(
            "{} is not a Plugin v2 JavaScript module (use `plugin check` to validate JSON rule files)",
            path.display()
        );
    }
    let manifest = inspect_js_plugin_manifest(&raw).map_err(|e| anyhow::anyhow!(e))?;
    Ok((raw, manifest))
}

pub fn check_plugin_file(path: &Path) -> anyhow::Result<serde_json::Value> {
    let raw = std::fs::read_to_string(path)
        .with_context(|| format!("failed reading plugin file {}", path.display()))?;
    if is_js_plugin_source(&raw) {
        let manifest = inspect_js_plugin_manifest(&raw).map_err(|e| anyhow::anyhow!(e))?;
        return Ok(serde_json::json!({
            "ok": true,
            "format": "js_module",
            "domain": manifest.domain.as_str(),
            "manifest": manifest,
        }));
    }

    let trimmed = raw.trim().trim_start_matches('\u{feff}').trim();
    let val: serde_json::Value =
        serde_json::from_str(trimmed).context("invalid JSON syntax in rule plugin")?;
    let count = match &val {
        serde_json::Value::Array(arr) => arr.len(),
        serde_json::Value::Object(_) => 1,
        _ => bail!("JSON rule plugin must be an object or array of objects"),
    };
    let domain = infer_plugin_domain(trimmed).map(|d| d.as_str());
    Ok(serde_json::json!({
        "ok": true,
        "format": "json_rule",
        "domain": domain,
        "entry_count": count,
    }))
}

pub fn run_plugin_cli(cmd: PluginCommand) -> anyhow::Result<()> {
    let output = execute_plugin_command(cmd)?;
    println!("{}", serde_json::to_string_pretty(&output)?);
    Ok(())
}

pub fn execute_plugin_command(cmd: PluginCommand) -> anyhow::Result<serde_json::Value> {
    match cmd {
        PluginCommand::Check { file } => check_plugin_file(&file),
        PluginCommand::Set {
            file,
            assignment,
            state,
        } => {
            let (_, manifest) = load_v2_file(&file)?;
            let Some((key_raw, val_raw)) = assignment.split_once('=') else {
                bail!("setting assignment must be formatted as key=value");
            };
            let key = key_raw.trim();
            if key.is_empty() {
                bail!("setting key cannot be empty");
            }
            let known = manifest.settings.iter().any(|s| s.key == key);
            if !known {
                bail!(
                    "unknown setting key '{}' for plugin '{}' (declared keys: {})",
                    key,
                    manifest.name,
                    manifest
                        .settings
                        .iter()
                        .map(|s| s.key.as_str())
                        .collect::<Vec<_>>()
                        .join(", ")
                );
            }
            let parsed_val = serde_json::from_str::<serde_json::Value>(val_raw.trim())
                .unwrap_or_else(|_| serde_json::Value::String(val_raw.trim().to_string()));
            let mut rt = load_state_file(Some(&state))?;
            rt.settings.insert(key.to_string(), parsed_val.clone());
            save_state_file(Some(&state), &rt)?;
            Ok(serde_json::json!({
                "ok": true,
                "key": key,
                "value": parsed_val,
                "state_file": state,
            }))
        }
        PluginCommand::Search {
            file,
            keyword,
            page,
            limit,
            flags,
        } => {
            let (js_code, manifest) = load_v2_file(&file)?;
            let initial_state = load_state_file(flags.state.as_deref())?;
            let timeout = Duration::from_secs(flags.timeout_secs.max(3));
            let out = PluginV2Runner::call_method_sync(
                &js_code,
                &manifest,
                initial_state,
                &build_http_config(&flags),
                timeout,
                timeout,
                Arc::new(AtomicBool::new(false)),
                &["search"],
                &[serde_json::json!({
                    "keyword": keyword,
                    "page": page.max(1),
                    "limit": limit.clamp(1, 100),
                })],
            )
            .map_err(|e| {
                emit_diagnostics(&e.diagnostics);
                anyhow::anyhow!(e.message)
            })?;
            emit_diagnostics(&out.diagnostics);
            save_state_file(flags.state.as_deref(), &out.state)?;
            Ok(out.data)
        }
        PluginCommand::Explore {
            file,
            category,
            page,
            limit,
            flags,
        } => {
            let (js_code, manifest) = load_v2_file(&file)?;
            let initial_state = load_state_file(flags.state.as_deref())?;
            let timeout = Duration::from_secs(flags.timeout_secs.max(3));
            let cat_id = category
                .or_else(|| manifest.explore_categories.first().map(|c| c.id.clone()))
                .unwrap_or_default();
            let out = PluginV2Runner::call_any_method_sync(
                &js_code,
                &manifest,
                initial_state,
                &build_http_config(&flags),
                timeout,
                timeout,
                Arc::new(AtomicBool::new(false)),
                &[&["explore"], &["browse"]],
                &[serde_json::json!({
                    "categoryId": cat_id,
                    "exploreUrl": cat_id,
                    "page": page.max(1),
                    "limit": limit.clamp(1, 100),
                })],
            )
            .map_err(|e| {
                emit_diagnostics(&e.diagnostics);
                anyhow::anyhow!(e.message)
            })?;
            emit_diagnostics(&out.diagnostics);
            save_state_file(flags.state.as_deref(), &out.state)?;
            Ok(out.data)
        }
        PluginCommand::Detail {
            file,
            id,
            url,
            flags,
        } => {
            let (js_code, manifest) = load_v2_file(&file)?;
            let initial_state = load_state_file(flags.state.as_deref())?;
            let timeout = Duration::from_secs(flags.timeout_secs.max(3));
            let id_val = id.unwrap_or_default();
            let url_val = url.unwrap_or_default();
            let out = PluginV2Runner::call_any_method_sync(
                &js_code,
                &manifest,
                initial_state,
                &build_http_config(&flags),
                timeout,
                timeout,
                Arc::new(AtomicBool::new(false)),
                &[
                    &["getTrackInfo"],
                    &["getDetail"],
                    &["getBookInfo"],
                    &["getComicDetail"],
                ],
                &[serde_json::json!({
                    "id": id_val,
                    "trackId": id_val,
                    "vodId": id_val,
                    "url": url_val,
                    "detailUrl": url_val,
                    "bookUrl": url_val,
                    "comicUrl": url_val,
                    "track": { "id": id_val, "trackId": id_val, "detailUrl": url_val },
                    "book": { "bookUrl": url_val },
                    "comic": { "comicUrl": url_val },
                })],
            )
            .map_err(|e| {
                emit_diagnostics(&e.diagnostics);
                anyhow::anyhow!(e.message)
            })?;
            emit_diagnostics(&out.diagnostics);
            save_state_file(flags.state.as_deref(), &out.state)?;
            Ok(out.data)
        }
        PluginCommand::Toc { file, url, flags } => {
            let (js_code, manifest) = load_v2_file(&file)?;
            let initial_state = load_state_file(flags.state.as_deref())?;
            let timeout = Duration::from_secs(flags.timeout_secs.max(3));
            let out = PluginV2Runner::call_any_method_sync(
                &js_code,
                &manifest,
                initial_state,
                &build_http_config(&flags),
                timeout,
                timeout,
                Arc::new(AtomicBool::new(false)),
                &[&["getToc"], &["getChapters"]],
                &[serde_json::json!({
                    "tocUrl": url,
                    "url": url,
                    "book": { "tocUrl": url },
                    "comic": { "tocUrl": url },
                })],
            )
            .map_err(|e| {
                emit_diagnostics(&e.diagnostics);
                anyhow::anyhow!(e.message)
            })?;
            emit_diagnostics(&out.diagnostics);
            save_state_file(flags.state.as_deref(), &out.state)?;
            Ok(out.data)
        }
        PluginCommand::Content {
            file,
            url,
            index,
            title,
            flags,
        } => {
            let (js_code, manifest) = load_v2_file(&file)?;
            let initial_state = load_state_file(flags.state.as_deref())?;
            let timeout = Duration::from_secs(flags.timeout_secs.max(3));
            let out = PluginV2Runner::call_any_method_sync(
                &js_code,
                &manifest,
                initial_state,
                &build_http_config(&flags),
                timeout,
                timeout,
                Arc::new(AtomicBool::new(false)),
                &[&["getChapterContent"], &["getContent"], &["getPages"]],
                &[serde_json::json!({
                    "chapterUrl": url,
                    "url": url,
                    "book": {},
                    "comic": {},
                    "chapter": {
                        "index": index,
                        "title": title,
                        "url": url,
                    },
                })],
            )
            .map_err(|e| {
                emit_diagnostics(&e.diagnostics);
                anyhow::anyhow!(e.message)
            })?;
            emit_diagnostics(&out.diagnostics);
            save_state_file(flags.state.as_deref(), &out.state)?;
            Ok(out.data)
        }
        PluginCommand::Stream {
            file,
            id,
            url,
            quality,
            flags,
        } => {
            let (js_code, manifest) = load_v2_file(&file)?;
            let initial_state = load_state_file(flags.state.as_deref())?;
            let timeout = Duration::from_secs(flags.timeout_secs.max(3));
            let id_val = id.unwrap_or_default();
            let url_val = url.unwrap_or_default();
            let out = PluginV2Runner::call_any_method_sync(
                &js_code,
                &manifest,
                initial_state,
                &build_http_config(&flags),
                timeout,
                timeout,
                Arc::new(AtomicBool::new(false)),
                &[&["getStream"], &["getPlayInfo"]],
                &[serde_json::json!({
                    "id": id_val,
                    "trackId": id_val,
                    "url": url_val,
                    "playUrl": url_val,
                    "quality": quality,
                    "track": {
                        "id": id_val,
                        "trackId": id_val,
                        "playUrl": if url_val.is_empty() { None } else { Some(url_val.clone()) },
                        "quality": quality,
                    },
                })],
            )
            .map_err(|e| {
                emit_diagnostics(&e.diagnostics);
                anyhow::anyhow!(e.message)
            })?;
            emit_diagnostics(&out.diagnostics);
            save_state_file(flags.state.as_deref(), &out.state)?;
            Ok(out.data)
        }
        PluginCommand::Lyrics { file, id, flags } => {
            let (js_code, manifest) = load_v2_file(&file)?;
            let initial_state = load_state_file(flags.state.as_deref())?;
            let timeout = Duration::from_secs(flags.timeout_secs.max(3));
            let out = PluginV2Runner::call_method_sync(
                &js_code,
                &manifest,
                initial_state,
                &build_http_config(&flags),
                timeout,
                timeout,
                Arc::new(AtomicBool::new(false)),
                &["getLyrics"],
                &[serde_json::json!({
                    "id": id,
                    "trackId": id,
                    "lyricId": id,
                    "track": { "id": id, "trackId": id, "lyricId": id },
                })],
            )
            .map_err(|e| {
                emit_diagnostics(&e.diagnostics);
                anyhow::anyhow!(e.message)
            })?;
            emit_diagnostics(&out.diagnostics);
            save_state_file(flags.state.as_deref(), &out.state)?;
            Ok(out.data)
        }
        PluginCommand::Auth {
            file,
            auth_cmd,
            flags,
        } => {
            let (js_code, manifest) = load_v2_file(&file)?;
            let initial_state = load_state_file(flags.state.as_deref())?;
            let timeout = Duration::from_secs(flags.timeout_secs.max(3));
            let req = match auth_cmd {
                PluginAuthSubcommand::Status => PluginAuthActionRequest::Status,
                PluginAuthSubcommand::QrCreate => PluginAuthActionRequest::QrCreate,
                PluginAuthSubcommand::QrPoll { key } => PluginAuthActionRequest::QrPoll { key },
                PluginAuthSubcommand::OtpSend {
                    phone,
                    country_code,
                } => PluginAuthActionRequest::OtpSend {
                    country_code,
                    phone,
                },
                PluginAuthSubcommand::OtpVerify {
                    phone,
                    code,
                    country_code,
                } => PluginAuthActionRequest::OtpVerify {
                    country_code,
                    phone,
                    code,
                },
                PluginAuthSubcommand::Password { username, password } => {
                    PluginAuthActionRequest::PasswordLogin { username, password }
                }
                PluginAuthSubcommand::Cookie { cookie } => {
                    PluginAuthActionRequest::CookieLogin { cookie }
                }
                PluginAuthSubcommand::Logout => PluginAuthActionRequest::Logout,
            };
            let out = PluginV2Runner::run_auth_action_sync(
                &js_code,
                &manifest,
                initial_state,
                req,
                &build_http_config(&flags),
                timeout,
            )
            .map_err(|e| {
                emit_diagnostics(&e.diagnostics);
                anyhow::anyhow!(e.message)
            })?;
            emit_diagnostics(&out.diagnostics);
            save_state_file(flags.state.as_deref(), &out.state)?;
            Ok(serde_json::json!({
                "ok": out.data.ok,
                "status": out.data.status,
                "message": out.data.message,
                "qr_key": out.data.qr_key,
                "qr_url": out.data.qr_url,
                "qr_image": out.data.qr_image,
                "cooldown_secs": out.data.cooldown_secs,
                "profile": out.data.profile,
            }))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plugin_cli_check_set_auth_and_stateful_stream_roundtrip() {
        let dir = tempfile::tempdir().unwrap();
        let plugin_path = dir.path().join("demo.js");
        let state_path = dir.path().join("state.json");

        std::fs::write(
            &plugin_path,
            r#"
export default definePlugin({
  id: "cli-demo",
  name: "CLI Demo Music",
  version: "1.0.0",
  domain: "music",
  baseUrl: "https://example.com",
  qualities: ["exhigh", "lossless"],
  settings: [
    { key: "level", label: "Level", type: "select", default: "exhigh", options: [
      { label: "320k", value: "exhigh" },
      { label: "FLAC", value: "lossless" }
    ] }
  ],
  auth: {
    cookie: true,
    async getProfile(ctx) {
      const uid = ctx.cookies.get("https://example.com", "UID");
      return uid ? { loggedIn: true, userId: uid, nickname: "CLIUser" } : { loggedIn: false };
    }
  },
  async getStream(ctx, { id, quality }) {
    const uid = ctx.cookies.get("https://example.com", "UID");
    const lv = quality || ctx.settings.level;
    return {
      url: "https://example.com/audio/" + id + ".flac",
      format: lv === "lossless" && uid ? "flac" : "mp3"
    };
  }
});
"#,
        )
        .unwrap();

        let check_out = execute_plugin_command(PluginCommand::Check {
            file: plugin_path.clone(),
        })
        .unwrap();
        assert_eq!(check_out["ok"], true);
        assert_eq!(check_out["format"], "js_module");
        assert_eq!(check_out["domain"], "music");

        execute_plugin_command(PluginCommand::Set {
            file: plugin_path.clone(),
            assignment: "level=lossless".into(),
            state: state_path.clone(),
        })
        .unwrap();

        let flags = PluginExecFlags {
            state: Some(state_path.clone()),
            allow_private_ips: false,
            proxy: None,
            timeout_secs: 10,
        };

        let auth_out = execute_plugin_command(PluginCommand::Auth {
            file: plugin_path.clone(),
            auth_cmd: PluginAuthSubcommand::Cookie {
                cookie: "UID=42".into(),
            },
            flags: flags.clone(),
        })
        .unwrap();
        assert_eq!(auth_out["ok"], true);
        assert_eq!(auth_out["profile"]["nickname"], "CLIUser");

        let stream_out = execute_plugin_command(PluginCommand::Stream {
            file: plugin_path,
            id: Some("99".into()),
            url: None,
            quality: None,
            flags,
        })
        .unwrap();
        assert_eq!(stream_out["format"], "flac");
    }
}

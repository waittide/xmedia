use std::path::Path;

use serde::{Deserialize, Serialize};
use tracing::{info, warn};
use xmedia_core::domain::error::{DomainError, DomainResult};

use crate::domain::config::XmediaWebhookConfig;
use crate::infra::scrapers::http_client::{
    DEFAULT_TOOL_USER_AGENT, SsrfRedirectGuard, get_or_build_http_client,
};

#[derive(Debug, Deserialize)]
struct RemoteLibraryItem {
    id: String,
    name: String,
    kind: RemoteLibraryKind,
}

#[derive(Debug, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum RemoteLibraryKind {
    Managed,
    LocalMount { root_path: String },
}

#[derive(Debug, Clone, Serialize)]
pub struct WebhookTestReportDto {
    pub ok: bool,
    pub matched_libraries: Vec<String>,
    pub total_libraries: usize,
    pub message: String,
}

fn build_webhook_client() -> DomainResult<reqwest::Client> {
    get_or_build_http_client(None, DEFAULT_TOOL_USER_AGENT, 10, SsrfRedirectGuard::None)
}

fn paths_overlap_str(a: &Path, b: &Path) -> bool {
    let ca = a.canonicalize().unwrap_or_else(|_| a.to_path_buf());
    let cb = b.canonicalize().unwrap_or_else(|_| b.to_path_buf());
    ca == cb || ca.starts_with(&cb) || cb.starts_with(&ca)
}

fn resolve_webhook_target(cfg: &XmediaWebhookConfig) -> DomainResult<(&str, &str)> {
    let base = cfg.base_url.trim().trim_end_matches('/');
    let api_key = cfg
        .api_key
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .ok_or_else(|| DomainError::Validation("missing xmedia API key".into()))?;
    if base.is_empty() {
        return Err(DomainError::Validation("missing xmedia base_url".into()));
    }
    Ok((base, api_key))
}

async fn fetch_remote_libraries(
    cfg: &XmediaWebhookConfig,
) -> DomainResult<(reqwest::Client, &str, &str, Vec<RemoteLibraryItem>)> {
    let (base, api_key) = resolve_webhook_target(cfg)?;
    let client = build_webhook_client()?;
    let resp = client
        .get(format!("{base}/api/v1/libraries"))
        .header("x-api-key", api_key)
        .send()
        .await
        .map_err(|e| DomainError::Storage(format!("connect to xmedia failed: {e}")))?;

    if !resp.status().is_success() {
        return Err(DomainError::Storage(format!(
            "xmedia returned HTTP {} (check base_url and admin API key)",
            resp.status()
        )));
    }

    let libs: Vec<RemoteLibraryItem> = resp.json().await.map_err(|e| {
        DomainError::Storage(format!("parse xmedia libraries response failed: {e}"))
    })?;
    Ok((client, base, api_key, libs))
}

async fn dispatch_matching_scans(
    client: &reqwest::Client,
    base: &str,
    api_key: &str,
    libs: Vec<RemoteLibraryItem>,
    changed_root_path: Option<&Path>,
    trigger_mode: &str,
) -> Vec<String> {
    let mut triggered = Vec::new();
    for lib in libs {
        if let RemoteLibraryKind::LocalMount { root_path } = lib.kind {
            let matches = match changed_root_path {
                Some(p) => paths_overlap_str(p, Path::new(&root_path)),
                None => true,
            };
            if !matches {
                continue;
            }
            let url = format!("{base}/api/v1/libraries/{}/scan", lib.id);
            match client.post(&url).header("x-api-key", api_key).send().await {
                Ok(r) if r.status().is_success() => {
                    info!(
                        library = %lib.name,
                        library_id = %lib.id,
                        mode = trigger_mode,
                        "triggered xmedia main server incremental scan via webhook"
                    );
                    triggered.push(lib.name);
                }
                Ok(r) => {
                    warn!(
                        library = %lib.name,
                        status = %r.status(),
                        mode = trigger_mode,
                        "xmedia scan webhook returned non-success"
                    );
                }
                Err(e) => {
                    warn!(
                        library = %lib.name,
                        error = %e,
                        mode = trigger_mode,
                        "xmedia scan webhook request failed"
                    );
                }
            }
        }
    }
    triggered
}

pub async fn test_xmedia_webhook(cfg: &XmediaWebhookConfig) -> DomainResult<WebhookTestReportDto> {
    let (_client, _base, _api_key, libs) = fetch_remote_libraries(cfg).await?;
    let mount_names: Vec<String> = libs
        .iter()
        .filter(|l| matches!(l.kind, RemoteLibraryKind::LocalMount { .. }))
        .map(|l| l.name.clone())
        .collect();

    Ok(WebhookTestReportDto {
        ok: true,
        matched_libraries: mount_names,
        total_libraries: libs.len(),
        message: format!("已连接主站，发现 {} 个媒体库", libs.len()),
    })
}

/// Triggers incremental library scans on the linked `xmedia` main server for any
/// `LocalMount` library whose root overlaps `changed_root_path`.
pub async fn notify_xmedia_rescan(cfg: &XmediaWebhookConfig, changed_root_path: &Path) {
    if !cfg.enabled {
        return;
    }
    let (client, base, api_key, libs) = match fetch_remote_libraries(cfg).await {
        Ok(res) => res,
        Err(DomainError::Validation(_)) => return,
        Err(err) => {
            warn!(error = %err, "failed contacting xmedia main server for rescan webhook");
            return;
        }
    };
    let _ = dispatch_matching_scans(
        &client,
        base,
        api_key,
        libs,
        Some(changed_root_path),
        "auto",
    )
    .await;
}

/// Manually triggers incremental library scans on the linked `xmedia` main server
/// for any `LocalMount` library whose root overlaps `changed_root_path` (or all
/// `LocalMount` libraries if `changed_root_path` is `None`), returning a structured report.
pub async fn trigger_xmedia_rescan(
    cfg: &XmediaWebhookConfig,
    changed_root_path: Option<&Path>,
) -> DomainResult<WebhookTestReportDto> {
    let (client, base, api_key, libs) = fetch_remote_libraries(cfg).await?;
    let total_libraries = libs.len();
    let triggered =
        dispatch_matching_scans(&client, base, api_key, libs, changed_root_path, "manual").await;

    let message = if triggered.is_empty() {
        "已连接主站，但未找到与当前目录重合的本地挂载媒体库".to_string()
    } else {
        format!(
            "已通知主站触发 {} 个媒体库扫库：{}",
            triggered.len(),
            triggered.join("、")
        )
    };

    Ok(WebhookTestReportDto {
        ok: !triggered.is_empty(),
        matched_libraries: triggered,
        total_libraries,
        message,
    })
}

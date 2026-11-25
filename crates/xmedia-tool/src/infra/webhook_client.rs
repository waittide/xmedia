use std::net::{IpAddr, SocketAddr};
use std::path::Path;
use std::sync::{Arc, OnceLock};
use std::time::Duration;

use async_trait::async_trait;
use reqwest::dns::{Addrs, Name, Resolve, Resolving};
use serde::Deserialize;
use tracing::{info, warn};
use uuid::Uuid;
use xmedia_core::domain::error::{DomainError, DomainResult};

use crate::domain::config::XmediaWebhookConfig;
use crate::domain::ops::WebhookTestReportDto;
use crate::domain::ports::HttpWebhookPort;
use crate::infra::scrapers::http_client::{
    DEFAULT_TOOL_USER_AGENT, is_blocked_metadata_or_link_local_ip,
};

#[derive(Debug, Default, Clone, Copy)]
pub struct ReqwestWebhookClient;

#[async_trait]
impl HttpWebhookPort for ReqwestWebhookClient {
    async fn test_webhook(&self, cfg: &XmediaWebhookConfig) -> DomainResult<WebhookTestReportDto> {
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

    async fn notify_rescan(&self, cfg: &XmediaWebhookConfig, changed_root_path: &Path) {
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

    async fn trigger_rescan(
        &self,
        cfg: &XmediaWebhookConfig,
        changed_root_path: Option<&Path>,
    ) -> DomainResult<WebhookTestReportDto> {
        let (client, base, api_key, libs) = fetch_remote_libraries(cfg).await?;
        let total_libraries = libs.len();
        let triggered =
            dispatch_matching_scans(&client, base, api_key, libs, changed_root_path, "manual")
                .await;

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
}

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

#[derive(Debug, Default)]
struct WebhookSafeResolver;

impl Resolve for WebhookSafeResolver {
    fn resolve(&self, name: Name) -> Resolving {
        let host = name.as_str().to_string();
        Box::pin(async move {
            let addrs: Vec<SocketAddr> = tokio::net::lookup_host((host.as_str(), 0))
                .await
                .map_err(|e| {
                    Box::new(std::io::Error::new(
                        std::io::ErrorKind::AddrNotAvailable,
                        format!("DNS resolution failed for webhook host '{host}': {e}"),
                    )) as Box<dyn std::error::Error + Send + Sync>
                })?
                .collect();
            if addrs.is_empty() {
                return Err(Box::new(std::io::Error::new(
                    std::io::ErrorKind::AddrNotAvailable,
                    format!("DNS resolution returned no addresses for webhook host '{host}'"),
                ))
                    as Box<dyn std::error::Error + Send + Sync>);
            }
            for addr in &addrs {
                if is_blocked_metadata_or_link_local_ip(addr.ip()) {
                    return Err(Box::new(std::io::Error::new(
                        std::io::ErrorKind::PermissionDenied,
                        format!(
                            "blocked link-local/metadata address '{}' for webhook host '{host}'",
                            addr.ip()
                        ),
                    ))
                        as Box<dyn std::error::Error + Send + Sync>);
                }
            }
            let boxed: Addrs = Box::new(addrs.into_iter());
            Ok(boxed)
        })
    }
}

fn build_webhook_client() -> DomainResult<reqwest::Client> {
    static CLIENT: OnceLock<Result<reqwest::Client, String>> = OnceLock::new();
    let cached = CLIENT.get_or_init(|| {
        reqwest::Client::builder()
            .no_proxy()
            .dns_resolver(Arc::new(WebhookSafeResolver))
            .user_agent(DEFAULT_TOOL_USER_AGENT)
            .timeout(Duration::from_secs(10))
            .connect_timeout(Duration::from_secs(5))
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .map_err(|e| format!("failed building webhook client: {e}"))
    });
    cached.clone().map_err(DomainError::Storage)
}

fn paths_overlap_str(a: &Path, b: &Path) -> bool {
    let ca = a.canonicalize().unwrap_or_else(|_| a.to_path_buf());
    let cb = b.canonicalize().unwrap_or_else(|_| b.to_path_buf());
    ca == cb || ca.starts_with(&cb) || cb.starts_with(&ca)
}

async fn resolve_webhook_target(cfg: &XmediaWebhookConfig) -> DomainResult<(&str, &str)> {
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
    let parsed = reqwest::Url::parse(base)
        .map_err(|_| DomainError::Validation("invalid xmedia webhook base_url".into()))?;
    if !matches!(parsed.scheme(), "http" | "https")
        || !parsed.username().is_empty()
        || parsed.password().is_some()
        || parsed.query().is_some()
        || parsed.fragment().is_some()
    {
        return Err(DomainError::Validation(
            "webhook base_url must be a clean http:// or https:// URL without embedded credentials"
                .into(),
        ));
    }
    let host = parsed
        .host_str()
        .ok_or_else(|| DomainError::Validation("webhook base_url missing host".into()))?;
    if let Ok(ip) = host
        .trim_matches(|c| c == '[' || c == ']')
        .parse::<IpAddr>()
    {
        if is_blocked_metadata_or_link_local_ip(ip) {
            return Err(DomainError::Validation(format!(
                "disallowed link-local/metadata/unspecified webhook IP '{ip}'"
            )));
        }
        return Ok((base, api_key));
    }
    let port = parsed.port_or_known_default().unwrap_or(80);
    let addrs: Vec<SocketAddr> = tokio::net::lookup_host((host, port))
        .await
        .map_err(|e| {
            DomainError::Validation(format!("DNS resolution failed for webhook '{host}': {e}"))
        })?
        .collect();
    if addrs.is_empty() {
        return Err(DomainError::Validation(format!(
            "DNS resolution returned no addresses for webhook '{host}'"
        )));
    }
    for addr in &addrs {
        if is_blocked_metadata_or_link_local_ip(addr.ip()) {
            return Err(DomainError::Validation(format!(
                "disallowed link-local/metadata webhook address '{}' for '{host}'",
                addr.ip()
            )));
        }
    }
    Ok((base, api_key))
}

async fn fetch_remote_libraries(
    cfg: &XmediaWebhookConfig,
) -> DomainResult<(reqwest::Client, &str, &str, Vec<RemoteLibraryItem>)> {
    let (base, api_key) = resolve_webhook_target(cfg).await?;
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
            let Ok(parsed_lib_id) = Uuid::parse_str(lib.id.trim()) else {
                warn!(
                    library = %lib.name,
                    raw_id = %lib.id,
                    "skipping webhook scan trigger for invalid non-UUID library id"
                );
                continue;
            };
            let url = format!("{base}/api/v1/libraries/{parsed_lib_id}/scan");
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

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn webhook_target_validation_blocks_cloud_metadata_and_unspecified_ips() {
        for bad in [
            "http://169.254.169.254",
            "http://0.0.0.0:8080",
            "http://[::]:8080",
            "http://[fe80::1]:8080",
            "http://user:pass@127.0.0.1:8080",
        ] {
            let cfg = XmediaWebhookConfig {
                enabled: true,
                base_url: bad.to_string(),
                api_key: Some("secret".into()),
                ..Default::default()
            };
            assert!(
                resolve_webhook_target(&cfg).await.is_err(),
                "expected {bad} to be rejected"
            );
        }

        let valid_local = XmediaWebhookConfig {
            enabled: true,
            base_url: "http://127.0.0.1:8080".into(),
            api_key: Some("secret".into()),
            ..Default::default()
        };
        assert!(resolve_webhook_target(&valid_local).await.is_ok());
    }
}

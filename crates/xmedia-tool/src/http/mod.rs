pub mod auth;
pub mod routes;
pub mod static_files;

use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;

use xmedia_core::infra::ffmpeg::FfmpegToolkit;

use crate::application::auth::ToolAuthService;
use crate::application::scraping::ScrapingOrchestratorService;
use crate::application::settings::ToolSettingsService;
use crate::application::workbench::MediaWorkbenchService;
use crate::domain::ports::HttpWebhookPort;

#[derive(Clone, Debug, Default)]
pub struct ToolRequestSecurityPolicy {
    pub bind_addr: Option<SocketAddr>,
    pub cors_allow_origins: Arc<Vec<String>>,
}

impl ToolRequestSecurityPolicy {
    pub fn new(bind_addr: Option<SocketAddr>, cors_allow_origins: &[String]) -> Self {
        let normalized = cors_allow_origins
            .iter()
            .map(|o| o.trim().trim_end_matches('/').to_string())
            .filter(|o| !o.is_empty())
            .collect();
        Self {
            bind_addr,
            cors_allow_origins: Arc::new(normalized),
        }
    }

    pub fn is_explicit_cors_origin(&self, raw_origin: &str) -> bool {
        let trimmed = raw_origin.trim().trim_end_matches('/');
        if trimmed.is_empty() || trimmed.eq_ignore_ascii_case("null") {
            return false;
        }
        self.cors_allow_origins
            .iter()
            .any(|allowed| allowed.eq_ignore_ascii_case(trimmed))
    }
}

#[derive(Clone)]
pub struct ToolHttpState {
    pub auth: ToolAuthService,
    pub settings: ToolSettingsService,
    pub workbench: MediaWorkbenchService,
    pub scraping: ScrapingOrchestratorService,
    pub webhook: Arc<dyn HttpWebhookPort>,
    pub ffmpeg: FfmpegToolkit,
    pub external_web_dir: Option<PathBuf>,
    pub security: ToolRequestSecurityPolicy,
}

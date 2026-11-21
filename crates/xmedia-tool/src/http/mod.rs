pub mod auth;
pub mod routes;
pub mod static_files;

use std::path::PathBuf;

use xmedia_core::infra::ffmpeg::FfmpegToolkit;

use crate::application::auth::ToolAuthService;
use crate::application::scraping::ScrapingOrchestratorService;
use crate::application::settings::ToolSettingsService;
use crate::application::workbench::MediaWorkbenchService;

#[derive(Clone)]
pub struct ToolHttpState {
    pub auth: ToolAuthService,
    pub settings: ToolSettingsService,
    pub workbench: MediaWorkbenchService,
    pub scraping: ScrapingOrchestratorService,
    pub ffmpeg: FfmpegToolkit,
    pub external_web_dir: Option<PathBuf>,
}

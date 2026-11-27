pub mod auth;
pub mod routes;
pub mod static_files;

use std::path::PathBuf;
use std::sync::Arc;

use axum::extract::FromRef;
use xmedia_core::application::ports::FontRepository;
use xmedia_core::infra::ffmpeg::FfmpegToolkit;

use crate::application::auth::ToolAuthService;
use crate::application::ports::{HttpWebhookPort, WorkspaceFsPort};
use crate::application::scraping::{ScrapeApplyService, ScraperCatalogService};
use crate::application::settings::ToolSettingsService;
use crate::application::workbench::{
    FormatToolsService, MetadataService, RenameService, WorkspaceBrowserService,
};

pub use auth::ToolRequestSecurityPolicy;

#[derive(Clone)]
pub struct ToolHttpState {
    pub auth: ToolAuthService,
    pub settings: ToolSettingsService,
    pub workspace: WorkspaceBrowserService,
    pub metadata: MetadataService,
    pub rename: RenameService,
    pub tools: FormatToolsService,
    pub scrapers: ScraperCatalogService,
    pub scrape_apply: ScrapeApplyService,
    pub fs: Arc<dyn WorkspaceFsPort>,
    pub webhook: Arc<dyn HttpWebhookPort>,
    pub fonts: Arc<dyn FontRepository>,
    pub ffmpeg: FfmpegToolkit,
    pub external_web_dir: Option<PathBuf>,
    pub security: ToolRequestSecurityPolicy,
}

impl FromRef<ToolHttpState> for Arc<dyn FontRepository> {
    fn from_ref(state: &ToolHttpState) -> Self {
        state.fonts.clone()
    }
}

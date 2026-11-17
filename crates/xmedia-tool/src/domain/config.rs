use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::domain::workspace::ManagedRoot;

/// Credentials and network options passed to pluggable scraper providers.
/// Includes both well-known fields and a flexible `extra` map so newly added providers
/// can read custom keys without changing core config schema.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ProviderCredentials {
    #[serde(default)]
    pub tmdb_api_key: Option<String>,
    #[serde(default)]
    pub bangumi_access_token: Option<String>,
    #[serde(default)]
    pub http_proxy: Option<String>,
    #[serde(default)]
    pub user_agent: Option<String>,
    #[serde(default)]
    pub extra: BTreeMap<String, String>,
}

impl ProviderCredentials {
    pub fn has_tmdb_key(&self) -> bool {
        self.tmdb_api_key
            .as_deref()
            .is_some_and(|k| !k.trim().is_empty())
    }

    pub fn get_extra(&self, key: &str) -> Option<&str> {
        self.extra
            .get(key)
            .map(String::as_str)
            .filter(|v| !v.trim().is_empty())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RenameTemplates {
    pub book: String,
    pub comic: String,
    pub music: String,
    pub video_movie: String,
    pub video_episode: String,
}

impl Default for RenameTemplates {
    fn default() -> Self {
        Self {
            book: "{author} - {title}".to_string(),
            comic: "{series} - #{number} {title}".to_string(),
            music: "{track} - {artist} - {title}".to_string(),
            video_movie: "{title} ({year})".to_string(),
            video_episode: "{show} - S{season}E{episode} - {title}".to_string(),
        }
    }
}

/// Persistent configuration aggregate for `xmedia-tool`.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ToolSettings {
    #[serde(default)]
    pub roots: Vec<ManagedRoot>,
    #[serde(default)]
    pub credentials: ProviderCredentials,
    #[serde(default)]
    pub rename_templates: RenameTemplates,
    #[serde(default = "default_locale")]
    pub default_scrape_locale: String,
}

fn default_locale() -> String {
    "zh-CN".to_string()
}

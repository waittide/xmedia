use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum VideoSourceTestMode {
    #[default]
    Full,
    Categories,
    Search,
    #[serde(alias = "explore")]
    Browse,
    Detail,
    Play,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OnlineVideoCategory {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub parent_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OnlineVideoSummary {
    pub vod_id: String,
    pub title: String,
    #[serde(default)]
    pub cover_url: Option<String>,
    #[serde(default)]
    pub proxy_cover_url: Option<String>,
    #[serde(default)]
    pub remarks: Option<String>,
    #[serde(default)]
    pub year: Option<String>,
    #[serde(default)]
    pub area: Option<String>,
    #[serde(default)]
    pub type_name: Option<String>,
    pub detail_url: String,
}

pub type OnlineVideoItem = OnlineVideoSummary;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OnlineVideoEpisode {
    pub index: usize,
    pub name: String,
    pub play_url: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OnlineVideoPlayGroup {
    pub name: String,
    pub episodes: Vec<OnlineVideoEpisode>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OnlineVideoDetail {
    pub id: String,
    pub title: String,
    #[serde(default)]
    pub cover_url: Option<String>,
    #[serde(default)]
    pub proxy_cover_url: Option<String>,
    #[serde(default)]
    pub intro: Option<String>,
    #[serde(default)]
    pub year: Option<String>,
    #[serde(default)]
    pub area: Option<String>,
    #[serde(default)]
    pub actors: Option<String>,
    #[serde(default)]
    pub director: Option<String>,
    #[serde(default)]
    pub remarks: Option<String>,
    pub play_groups: Vec<OnlineVideoPlayGroup>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OnlineVideoSubtitle {
    pub label: String,
    #[serde(default)]
    pub lang: Option<String>,
    pub url: String,
    #[serde(default)]
    pub proxy_url: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OnlineVideoPlayResult {
    pub stream_url: String,
    pub proxy_stream_url: String,
    pub stream_type: String,
    pub headers_applied: bool,
    #[serde(default)]
    pub subtitles: Vec<OnlineVideoSubtitle>,
}

#[derive(Debug, Clone)]
pub struct VideoBrowseOutput {
    pub items: Vec<OnlineVideoSummary>,
    pub total: Option<u64>,
    pub page: u32,
    pub page_count: Option<u32>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct VideoJsContext {
    pub vod_id: String,
    pub title: String,
    pub detail_url: String,
    pub play_url: String,
    pub group_name: String,
}

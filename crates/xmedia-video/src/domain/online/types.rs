use serde::{Deserialize, Serialize};

use crate::domain::video::normalize_string_list;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum VideoSourceTestMode {
    #[default]
    Full,
    Categories,
    Search,
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

impl OnlineVideoDetail {
    pub fn select_play_group(&self, preferred_name: Option<&str>) -> Option<&OnlineVideoPlayGroup> {
        if let Some(pref) = preferred_name.map(str::trim).filter(|s| !s.is_empty())
            && let Some(found) = self
                .play_groups
                .iter()
                .find(|g| !g.episodes.is_empty() && g.name.trim().eq_ignore_ascii_case(pref))
        {
            return Some(found);
        }
        self.play_groups.iter().find(|g| !g.episodes.is_empty())
    }

    pub fn parsed_year(&self) -> Option<i32> {
        parse_year_digits(self.year.as_deref())
    }

    pub fn area_genres(&self) -> Vec<String> {
        self.area
            .as_deref()
            .map(|a| {
                normalize_string_list(a.split([',', '，', '/', ' ']).map(str::to_string).collect())
            })
            .unwrap_or_default()
    }
}

fn parse_year_digits(raw: Option<&str>) -> Option<i32> {
    let s = raw?.trim();
    let digits: String = s.chars().filter(|c| c.is_ascii_digit()).take(4).collect();
    if digits.len() == 4 {
        digits
            .parse::<i32>()
            .ok()
            .filter(|&y| (1880..=2150).contains(&y))
    } else {
        None
    }
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

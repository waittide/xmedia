use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use xmedia_core::domain::library::MediaKind;

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
            book:
                "{author}/{series}/{subseries}/<Vol.{volume:2} >{title} ({year})/{title} - {author}"
                    .to_string(),
            comic: "{series}/{series} <Vol.{volume:2}> <Ch.{number:2}> {title}".to_string(),
            music: "{album_artist}/{album} ({year})/{track:2} - {title}".to_string(),
            video_movie: "{title} ({year})/{title} ({year})".to_string(),
            video_episode:
                "{show} ({year})/Season {season:2}/{show} - S{season:2}E{episode:2} - {title}"
                    .to_string(),
        }
    }
}

impl RenameTemplates {
    /// Upgrades untouched v1 flat default templates to v2 `{var:N}` + Jellyfin folder hierarchy defaults.
    pub fn upgrade_legacy_defaults(&mut self) -> bool {
        let mut changed = false;
        if self.book == "{author} - {title}" {
            self.book =
                "{author}/{series}/{subseries}/<Vol.{volume:2} >{title} ({year})/{title} - {author}"
                    .to_string();
            changed = true;
        }
        if self.comic == "{series} - #{number} {title}"
            || self.comic == "{series} - #{number:2} {title}"
        {
            self.comic = "{series}/{series} <Vol.{volume:2}> <Ch.{number:2}> {title}".to_string();
            changed = true;
        }
        if self.music == "{track} - {artist} - {title}"
            || self.music == "{track:2} - {artist} - {title}"
        {
            self.music = "{album_artist}/{album} ({year})/{track:2} - {title}".to_string();
            changed = true;
        }
        if self.video_movie == "{title} ({year})" {
            self.video_movie = "{title} ({year})/{title} ({year})".to_string();
            changed = true;
        }
        if self.video_episode == "{show} - S{season}E{episode} - {title}" {
            self.video_episode =
                "{show} ({year})/Season {season:2}/{show} - S{season:2}E{episode:2} - {title}"
                    .to_string();
            changed = true;
        }
        changed
    }
}

/// Per-domain scraper provider priority order (first = highest priority during field-level waterfall merge).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScraperPriorities {
    #[serde(default = "default_book_priority")]
    pub book: Vec<String>,
    #[serde(default = "default_comic_priority")]
    pub comic: Vec<String>,
    #[serde(default = "default_music_priority")]
    pub music: Vec<String>,
    #[serde(default = "default_video_priority")]
    pub video: Vec<String>,
}

fn default_book_priority() -> Vec<String> {
    vec![
        "douban".to_string(),
        "neodb".to_string(),
        "xinhua".to_string(),
        "qidian".to_string(),
        "fanqie".to_string(),
        "qimao".to_string(),
        "17k".to_string(),
        "ciweimao".to_string(),
        "sfacg".to_string(),
        "faloo".to_string(),
        "jjwxc".to_string(),
        "bangumi".to_string(),
        "anilist".to_string(),
        "openlibrary".to_string(),
    ]
}

fn default_comic_priority() -> Vec<String> {
    vec![
        "bangumi".to_string(),
        "douban".to_string(),
        "neodb".to_string(),
        "anilist".to_string(),
    ]
}

fn default_music_priority() -> Vec<String> {
    vec![
        "qq".to_string(),
        "netease".to_string(),
        "kugou".to_string(),
        "kuwo".to_string(),
        "migu".to_string(),
        "taihe".to_string(),
        "qishui".to_string(),
        "itunes".to_string(),
        "bangumi".to_string(),
        "musicbrainz".to_string(),
        "lrclib".to_string(),
    ]
}

fn default_video_priority() -> Vec<String> {
    vec!["tmdb".to_string(), "bangumi".to_string()]
}

impl Default for ScraperPriorities {
    fn default() -> Self {
        Self {
            book: default_book_priority(),
            comic: default_comic_priority(),
            music: default_music_priority(),
            video: default_video_priority(),
        }
    }
}

impl ScraperPriorities {
    /// Upgrades untouched legacy book/music priority lists so newly added Chinese webnovel
    /// and Chinese music scrapers take precedence in the configured default order.
    pub fn upgrade_legacy_defaults(&mut self) -> bool {
        let mut changed = false;
        if self.book == ["bangumi", "openlibrary"]
            || self.book
                == [
                    "qidian",
                    "fanqie",
                    "qimao",
                    "17k",
                    "ciweimao",
                    "sfacg",
                    "faloo",
                    "jjwxc",
                    "bangumi",
                    "openlibrary",
                ]
        {
            self.book = default_book_priority();
            changed = true;
        }
        if self.comic == ["bangumi"] {
            self.comic = default_comic_priority();
            changed = true;
        }
        if self.music == ["itunes", "musicbrainz", "lrclib"]
            || self.music == ["musicbrainz", "lrclib"]
            || self.music
                == [
                    "qq",
                    "netease",
                    "kugou",
                    "kuwo",
                    "itunes",
                    "musicbrainz",
                    "lrclib",
                ]
        {
            self.music = default_music_priority();
            changed = true;
        }
        changed
    }

    pub fn order_for(&self, kind: MediaKind) -> &[String] {
        match kind {
            MediaKind::Book => &self.book,
            MediaKind::Comic => &self.comic,
            MediaKind::Music => &self.music,
            MediaKind::Video => &self.video,
        }
    }
}

/// Default scrape metadata cache TTL: 7 days in milliseconds (604,800,000 ms).
pub const DEFAULT_SCRAPE_CACHE_TTL_MS: u64 = 7 * 24 * 3_600_000;

fn default_scrape_cache_ttl_ms() -> u64 {
    DEFAULT_SCRAPE_CACHE_TTL_MS
}

/// Persistent configuration aggregate for `xmedia-tool`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolSettings {
    #[serde(default)]
    pub roots: Vec<ManagedRoot>,
    #[serde(default)]
    pub credentials: ProviderCredentials,
    #[serde(default)]
    pub rename_templates: RenameTemplates,
    #[serde(default)]
    pub scraper_priorities: ScraperPriorities,
    #[serde(default = "default_locale")]
    pub default_scrape_locale: String,
    /// Metadata cache TTL in milliseconds (`0` disables caching).
    #[serde(default = "default_scrape_cache_ttl_ms")]
    pub scrape_cache_ttl_ms: u64,
}

impl Default for ToolSettings {
    fn default() -> Self {
        Self {
            roots: Vec::new(),
            credentials: ProviderCredentials::default(),
            rename_templates: RenameTemplates::default(),
            scraper_priorities: ScraperPriorities::default(),
            default_scrape_locale: default_locale(),
            scrape_cache_ttl_ms: default_scrape_cache_ttl_ms(),
        }
    }
}

fn default_locale() -> String {
    "zh-CN".to_string()
}

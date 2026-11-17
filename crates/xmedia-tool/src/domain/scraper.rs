use serde::{Deserialize, Serialize};
use xmedia_core::domain::library::MediaKind;

/// Granular scrape target kinds across the 4 media domains.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ScrapeTargetKind {
    Book,
    Comic,
    MusicTrack,
    MusicAlbum,
    Lyrics,
    Movie,
    TvShow,
}

impl ScrapeTargetKind {
    pub fn media_kind(self) -> MediaKind {
        match self {
            Self::Book => MediaKind::Book,
            Self::Comic => MediaKind::Comic,
            Self::MusicTrack | Self::MusicAlbum | Self::Lyrics => MediaKind::Music,
            Self::Movie | Self::TvShow => MediaKind::Video,
        }
    }
}

/// Self-describing metadata for a registered scraper provider.
/// Exposed via `/api/v1/tool/scrapers/providers` so the frontend dynamically renders
/// available providers without hardcoding provider lists.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProviderDescriptor {
    pub id: String,
    pub name: String,
    pub description: String,
    pub website: String,
    pub supported_targets: Vec<ScrapeTargetKind>,
    pub requires_api_key: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProviderStatusDto {
    #[serde(flatten)]
    pub descriptor: ProviderDescriptor,
    pub configured: bool,
}

/// Domain-neutral search query passed to any `ScraperProviderPort`.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ScrapeQuery {
    pub target: Option<ScrapeTargetKind>,
    /// Optional explicit provider IDs; when empty, all configured providers supporting `target` are queried.
    #[serde(default)]
    pub providers: Vec<String>,
    pub keyword: String,
    pub artist_or_author: Option<String>,
    pub album_or_series: Option<String>,
    pub year: Option<i32>,
    pub season: Option<u32>,
    pub episode: Option<u32>,
    pub duration_secs: Option<u32>,
    pub isbn: Option<String>,
    pub locale: Option<String>,
}

/// Lightweight candidate item returned during search.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScrapeCandidate {
    pub provider_id: String,
    pub provider_name: String,
    pub external_id: String,
    pub target: ScrapeTargetKind,
    pub title: String,
    pub subtitle: Option<String>,
    pub year: Option<i32>,
    pub summary: Option<String>,
    pub cover_url: Option<String>,
    /// Pre-fetched lyrics if the provider returns lyrics directly on search (e.g. LRCLIB).
    pub synced_lyrics: Option<String>,
    pub plain_lyrics: Option<String>,
}

/// Rich normalized metadata fetched from a selected candidate, ready to preview or apply
/// onto a Book, Comic, Music, or Video file.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ScrapedMetadata {
    pub provider_id: String,
    pub external_id: String,
    pub target: Option<ScrapeTargetKind>,
    pub title: Option<String>,
    pub original_title: Option<String>,
    pub series_or_album: Option<String>,
    pub creators: Vec<String>,
    pub illustrators_or_directors: Vec<String>,
    pub cast: Vec<String>,
    pub publisher_or_studio: Option<String>,
    pub year: Option<i32>,
    pub published_date: Option<String>,
    pub language: Option<String>,
    pub isbn: Option<String>,
    pub rating: Option<f32>,
    pub track_or_episode_number: Option<u32>,
    pub disc_or_season_number: Option<u32>,
    pub summary: Option<String>,
    pub genres: Vec<String>,
    pub tags: Vec<String>,
    pub cover_url: Option<String>,
    pub backdrop_url: Option<String>,
    pub synced_lyrics: Option<String>,
    pub plain_lyrics: Option<String>,
}

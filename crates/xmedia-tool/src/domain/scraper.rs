use std::collections::BTreeMap;

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
    /// Series-level overview for `tvshow.nfo` (kept separate so episode summary never clobbers it).
    #[serde(default)]
    pub show_summary: Option<String>,
    /// Season-level title for `season.nfo` (e.g. `"第 1 季"` / `"Season 1"`).
    #[serde(default)]
    pub season_title: Option<String>,
    /// Season-level overview for `season.nfo`.
    #[serde(default)]
    pub season_summary: Option<String>,
    /// Dedicated season poster URL (`Season XX/poster.jpg`).
    #[serde(default)]
    pub season_poster_url: Option<String>,
    /// Dedicated episode still/thumbnail URL (`<episode>-thumb.jpg`).
    #[serde(default)]
    pub episode_still_url: Option<String>,
    pub genres: Vec<String>,
    pub tags: Vec<String>,
    pub cover_url: Option<String>,
    pub backdrop_url: Option<String>,
    pub synced_lyrics: Option<String>,
    pub plain_lyrics: Option<String>,
    /// Preserves all contributing provider external IDs (e.g. `{"tmdb": "209867", "bangumi": "400602"}`)
    /// during multi-source priority merging.
    #[serde(default)]
    pub external_ids: BTreeMap<String, String>,
}

/// Identifies which scraper source contributed a given metadata field.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FieldSourceBadge {
    pub provider_id: String,
    pub provider_name: String,
}

/// Result of priority-ordered waterfall merging across multiple scraper sources.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct MergedScrapedResult {
    pub metadata: ScrapedMetadata,
    pub field_sources: BTreeMap<String, FieldSourceBadge>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProviderBriefDto {
    pub id: String,
    pub name: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct AggregateScrapeResponse {
    pub provider_order: Vec<ProviderBriefDto>,
    pub candidates: Vec<ScrapeCandidate>,
    pub selected_by_provider: BTreeMap<String, ScrapeCandidate>,
    pub details_by_provider: BTreeMap<String, ScrapedMetadata>,
    pub merged: MergedScrapedResult,
}

fn non_empty_str(opt: &Option<String>) -> Option<String> {
    opt.as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_owned)
}

/// Pure DDD Domain Service: merges `ScrapedMetadata` items ordered from highest priority to lowest priority.
///
/// Each field is populated by the highest-priority source that provides a non-empty value, while
/// missing fields (such as cover art, summary, ISBN, backdrop, or synced lyrics) fall through to
/// subsequent sources and record their exact `FieldSourceBadge` in `field_sources`.
pub fn merge_scraped_by_priority(
    ordered_sources: &[(FieldSourceBadge, ScrapedMetadata)],
    target: Option<ScrapeTargetKind>,
) -> MergedScrapedResult {
    let mut out = ScrapedMetadata {
        target,
        ..Default::default()
    };
    let mut sources: BTreeMap<String, FieldSourceBadge> = BTreeMap::new();

    for (badge, src) in ordered_sources {
        if out.provider_id.is_empty() && !src.provider_id.is_empty() {
            out.provider_id = src.provider_id.clone();
            out.external_id = src.external_id.clone();
        }
        if !src.provider_id.is_empty() && !src.external_id.is_empty() {
            out.external_ids
                .entry(src.provider_id.clone())
                .or_insert_with(|| src.external_id.clone());
        }
        for (k, v) in &src.external_ids {
            out.external_ids
                .entry(k.clone())
                .or_insert_with(|| v.clone());
        }
        if out.target.is_none() {
            out.target = src.target;
        }

        if out.title.is_none()
            && let Some(v) = non_empty_str(&src.title)
        {
            out.title = Some(v);
            sources.insert("title".to_string(), badge.clone());
        }
        if out.original_title.is_none()
            && let Some(v) = non_empty_str(&src.original_title)
        {
            out.original_title = Some(v);
            sources.insert("original_title".to_string(), badge.clone());
        }
        if out.series_or_album.is_none()
            && let Some(v) = non_empty_str(&src.series_or_album)
        {
            out.series_or_album = Some(v);
            sources.insert("series_or_album".to_string(), badge.clone());
        }
        if out.creators.is_empty() && !src.creators.is_empty() {
            out.creators = src.creators.clone();
            sources.insert("creators".to_string(), badge.clone());
        }
        if out.illustrators_or_directors.is_empty() && !src.illustrators_or_directors.is_empty() {
            out.illustrators_or_directors = src.illustrators_or_directors.clone();
            sources.insert("illustrators_or_directors".to_string(), badge.clone());
        }
        if out.cast.is_empty() && !src.cast.is_empty() {
            out.cast = src.cast.clone();
            sources.insert("cast".to_string(), badge.clone());
        }
        if out.publisher_or_studio.is_none()
            && let Some(v) = non_empty_str(&src.publisher_or_studio)
        {
            out.publisher_or_studio = Some(v);
            sources.insert("publisher_or_studio".to_string(), badge.clone());
        }
        if out.year.is_none()
            && let Some(y) = src.year.filter(|&y| y > 0)
        {
            out.year = Some(y);
            sources.insert("year".to_string(), badge.clone());
        }
        if out.published_date.is_none()
            && let Some(v) = non_empty_str(&src.published_date)
        {
            out.published_date = Some(v);
            sources
                .entry("year".to_string())
                .or_insert_with(|| badge.clone());
        }
        if out.language.is_none()
            && let Some(v) = non_empty_str(&src.language)
        {
            out.language = Some(v);
            sources.insert("language".to_string(), badge.clone());
        }
        if out.isbn.is_none()
            && let Some(v) = non_empty_str(&src.isbn)
        {
            out.isbn = Some(v);
            sources.insert("isbn".to_string(), badge.clone());
        }
        if out.rating.is_none()
            && let Some(r) = src.rating.filter(|&r| r > 0.0)
        {
            out.rating = Some(r);
            sources.insert("rating".to_string(), badge.clone());
        }
        if out.track_or_episode_number.is_none()
            && let Some(n) = src.track_or_episode_number.filter(|&n| n > 0)
        {
            out.track_or_episode_number = Some(n);
            sources.insert("track_or_episode_number".to_string(), badge.clone());
        }
        if out.disc_or_season_number.is_none()
            && let Some(n) = src.disc_or_season_number.filter(|&n| n > 0)
        {
            out.disc_or_season_number = Some(n);
            sources.insert("disc_or_season_number".to_string(), badge.clone());
        }
        if out.summary.is_none()
            && let Some(v) = non_empty_str(&src.summary)
        {
            out.summary = Some(v);
            sources.insert("summary".to_string(), badge.clone());
        }
        if out.show_summary.is_none()
            && let Some(v) = non_empty_str(&src.show_summary)
        {
            out.show_summary = Some(v);
            sources.insert("show_summary".to_string(), badge.clone());
        }
        if out.season_title.is_none()
            && let Some(v) = non_empty_str(&src.season_title)
        {
            out.season_title = Some(v);
            sources.insert("season_title".to_string(), badge.clone());
        }
        if out.season_summary.is_none()
            && let Some(v) = non_empty_str(&src.season_summary)
        {
            out.season_summary = Some(v);
            sources.insert("season_summary".to_string(), badge.clone());
        }
        if out.genres.is_empty() && !src.genres.is_empty() {
            out.genres = src.genres.clone();
            sources.insert("genres".to_string(), badge.clone());
        }
        if out.tags.is_empty() && !src.tags.is_empty() {
            out.tags = src.tags.clone();
            sources.insert("tags".to_string(), badge.clone());
        }
        if out.cover_url.is_none()
            && let Some(v) = non_empty_str(&src.cover_url)
        {
            out.cover_url = Some(v);
            sources.insert("cover_url".to_string(), badge.clone());
        }
        if out.backdrop_url.is_none()
            && let Some(v) = non_empty_str(&src.backdrop_url)
        {
            out.backdrop_url = Some(v);
            sources.insert("backdrop_url".to_string(), badge.clone());
        }
        if out.season_poster_url.is_none()
            && let Some(v) = non_empty_str(&src.season_poster_url)
        {
            out.season_poster_url = Some(v);
            sources.insert("season_poster_url".to_string(), badge.clone());
        }
        if out.episode_still_url.is_none()
            && let Some(v) = non_empty_str(&src.episode_still_url)
        {
            out.episode_still_url = Some(v);
            sources.insert("episode_still_url".to_string(), badge.clone());
        }
        if out.synced_lyrics.is_none()
            && let Some(v) = non_empty_str(&src.synced_lyrics)
        {
            out.synced_lyrics = Some(v);
            sources.insert("lyrics".to_string(), badge.clone());
        }
        if out.plain_lyrics.is_none()
            && let Some(v) = non_empty_str(&src.plain_lyrics)
        {
            out.plain_lyrics = Some(v);
            if out.synced_lyrics.is_none() {
                sources
                    .entry("lyrics".to_string())
                    .or_insert_with(|| badge.clone());
            }
        }
    }

    MergedScrapedResult {
        metadata: out,
        field_sources: sources,
    }
}

#[cfg(test)]
mod tests {
    use super::{FieldSourceBadge, ScrapeTargetKind, ScrapedMetadata, merge_scraped_by_priority};

    #[test]
    fn waterfall_merge_fills_missing_fields_by_priority_and_tracks_provenance() {
        let primary = (
            FieldSourceBadge {
                provider_id: "bangumi".into(),
                provider_name: "Bangumi 番组计划".into(),
            },
            ScrapedMetadata {
                provider_id: "bangumi".into(),
                external_id: "1001".into(),
                title: Some("三体".into()),
                creators: vec!["刘慈欣".into()],
                summary: Some("地球往事中文简介".into()),
                cover_url: None, // Missing cover in source #1!
                isbn: None,      // Missing ISBN in source #1!
                ..Default::default()
            },
        );
        let secondary = (
            FieldSourceBadge {
                provider_id: "openlibrary".into(),
                provider_name: "OpenLibrary".into(),
            },
            ScrapedMetadata {
                provider_id: "openlibrary".into(),
                external_id: "OL17365W".into(),
                title: Some("The Three-Body Problem".into()),
                summary: Some("English summary".into()),
                cover_url: Some("https://covers.openlibrary.org/b/id/1-L.jpg".into()),
                isbn: Some("9787536692930".into()),
                ..Default::default()
            },
        );

        let merged = merge_scraped_by_priority(&[primary, secondary], Some(ScrapeTargetKind::Book));
        // Primary wins on title & summary
        assert_eq!(merged.metadata.title.as_deref(), Some("三体"));
        assert_eq!(merged.metadata.summary.as_deref(), Some("地球往事中文简介"));
        assert_eq!(merged.field_sources["title"].provider_id, "bangumi");
        assert_eq!(merged.field_sources["summary"].provider_id, "bangumi");

        // Secondary fills missing cover & ISBN
        assert_eq!(
            merged.metadata.cover_url.as_deref(),
            Some("https://covers.openlibrary.org/b/id/1-L.jpg")
        );
        assert_eq!(merged.metadata.isbn.as_deref(), Some("9787536692930"));
        assert_eq!(merged.field_sources["cover_url"].provider_id, "openlibrary");
        assert_eq!(merged.field_sources["isbn"].provider_id, "openlibrary");
    }
}

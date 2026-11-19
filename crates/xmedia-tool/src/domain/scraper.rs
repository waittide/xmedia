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
        let allow_zero_season = out.target == Some(ScrapeTargetKind::TvShow)
            || src.target == Some(ScrapeTargetKind::TvShow);
        if out.disc_or_season_number.is_none()
            && let Some(n) = src
                .disc_or_season_number
                .filter(|&n| n > 0 || allow_zero_season)
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

fn normalize_title_for_match(raw: &str) -> String {
    raw.chars()
        .filter(|c| c.is_alphanumeric())
        .flat_map(char::to_lowercase)
        .collect()
}

fn normalize_isbn(raw: &str) -> String {
    raw.chars()
        .filter(|c| c.is_ascii_digit() || *c == 'X' || *c == 'x')
        .flat_map(char::to_uppercase)
        .collect()
}

/// Checks whether a lower-priority provider's metadata refers to the same work as the primary
/// result, blocking unrelated secondary enrichment during automatic `aggregate` waterfall merge.
pub fn is_compatible_secondary_source(
    primary: &ScrapedMetadata,
    secondary: &ScrapedMetadata,
    target: Option<ScrapeTargetKind>,
) -> bool {
    if let (Some(i1), Some(i2)) = (primary.isbn.as_deref(), secondary.isbn.as_deref()) {
        let n1 = normalize_isbn(i1);
        let n2 = normalize_isbn(i2);
        if !n1.is_empty() && !n2.is_empty() && n1 != n2 {
            return false;
        }
    }

    let eff_target = target.or(primary.target).or(secondary.target);

    if matches!(
        eff_target,
        Some(
            ScrapeTargetKind::Movie
                | ScrapeTargetKind::TvShow
                | ScrapeTargetKind::MusicTrack
                | ScrapeTargetKind::MusicAlbum
        )
    ) && let (Some(y1), Some(y2)) = (primary.year, secondary.year)
        && (y1 - y2).abs() > 2
    {
        return false;
    }

    if matches!(
        eff_target,
        Some(
            ScrapeTargetKind::MusicTrack | ScrapeTargetKind::MusicAlbum | ScrapeTargetKind::Lyrics
        )
    ) {
        let p_title_low = primary.title.as_deref().unwrap_or("").to_lowercase();
        let s_title_low = secondary.title.as_deref().unwrap_or("").to_lowercase();
        for cover_marker in ["翻唱", "原唱", "伴奏", "ktv", "karaoke", "铃声"] {
            if s_title_low.contains(cover_marker) && !p_title_low.contains(cover_marker) {
                return false;
            }
        }
    }

    let collect_titles = |m: &ScrapedMetadata| -> Vec<String> {
        [&m.title, &m.original_title, &m.series_or_album]
            .into_iter()
            .flatten()
            .map(|s| normalize_title_for_match(s))
            .filter(|s| !s.is_empty())
            .collect()
    };
    let p_titles = collect_titles(primary);
    let s_titles = collect_titles(secondary);
    if !p_titles.is_empty() && !s_titles.is_empty() {
        let has_title_overlap = p_titles.iter().any(|pt| {
            s_titles.iter().any(|st| {
                pt == st
                    || (pt.chars().count() >= 2
                        && st.chars().count() >= 2
                        && (pt.contains(st) || st.contains(pt)))
            })
        });
        if !has_title_overlap {
            let p_ascii = p_titles.iter().all(|t| t.is_ascii());
            let s_ascii = s_titles.iter().all(|t| t.is_ascii());
            if p_ascii && s_ascii {
                let p_creators: Vec<String> = primary
                    .creators
                    .iter()
                    .map(|c| normalize_title_for_match(c))
                    .filter(|c| !c.is_empty())
                    .collect();
                let s_creators: Vec<String> = secondary
                    .creators
                    .iter()
                    .map(|c| normalize_title_for_match(c))
                    .filter(|c| !c.is_empty())
                    .collect();
                let creator_match = p_creators.iter().any(|pc| {
                    s_creators.iter().any(|sc| {
                        pc == sc
                            || (pc.len() >= 2
                                && sc.len() >= 2
                                && (pc.contains(sc) || sc.contains(pc)))
                    })
                });
                if !creator_match {
                    return false;
                }
            }
        }
    }

    if matches!(
        eff_target,
        Some(
            ScrapeTargetKind::Book
                | ScrapeTargetKind::MusicTrack
                | ScrapeTargetKind::MusicAlbum
                | ScrapeTargetKind::Lyrics
        )
    ) && !primary.creators.is_empty()
        && !secondary.creators.is_empty()
    {
        let p_creators: Vec<String> = primary
            .creators
            .iter()
            .map(|c| normalize_title_for_match(c))
            .filter(|c| !c.is_empty())
            .collect();
        let s_creators: Vec<String> = secondary
            .creators
            .iter()
            .map(|c| normalize_title_for_match(c))
            .filter(|c| !c.is_empty())
            .collect();
        let p_ascii = p_creators.iter().all(|c| c.is_ascii());
        let s_ascii = s_creators.iter().all(|c| c.is_ascii());
        // Compare authors when both sides use the same script (e.g. both CJK webnovel platforms);
        // cross-script pairs like "刘慈欣" vs "Cixin Liu" remain allowed for ISBN/cover fallback.
        if !p_creators.is_empty() && !s_creators.is_empty() && p_ascii == s_ascii {
            let same_author = p_creators.iter().any(|pc| {
                s_creators
                    .iter()
                    .any(|sc| pc == sc || pc.contains(sc) || sc.contains(pc))
            });
            if !same_author {
                return false;
            }
        }
    }

    true
}

/// Scores how closely a candidate's primary/original title and year match the user's search query,
/// boosting exact canonical works above spin-off shorts (`～...～`, `OVA`, `ミニ`, `小剧场`).
pub fn score_candidate_relevance(
    keyword: &str,
    year_hint: Option<i32>,
    primary_title: &str,
    alt_title: Option<&str>,
    cand_year: Option<i32>,
    popularity_votes: u32,
) -> i32 {
    let kw_trim = keyword.trim().to_lowercase();
    let kw_norm = normalize_title_for_match(keyword);
    if kw_norm.is_empty() {
        return 0;
    }

    let pri_trim = primary_title.trim().to_lowercase();
    let pri_norm = normalize_title_for_match(primary_title);
    let alt_trim = alt_title
        .map(|s| s.trim().to_lowercase())
        .unwrap_or_default();
    let alt_norm = alt_title.map(normalize_title_for_match).unwrap_or_default();

    let mut score = 0i32;

    if pri_trim == kw_trim || (!alt_trim.is_empty() && alt_trim == kw_trim) {
        score += 1200;
    } else if pri_norm == kw_norm || (!alt_norm.is_empty() && alt_norm == kw_norm) {
        score += 950;
    } else if pri_norm.starts_with(&kw_norm)
        || (!alt_norm.is_empty() && alt_norm.starts_with(&kw_norm))
    {
        let extra_chars = pri_norm
            .chars()
            .count()
            .saturating_sub(kw_norm.chars().count()) as i32;
        score += (420 - extra_chars * 18).max(80);
    } else if pri_norm.contains(&kw_norm) || (!alt_norm.is_empty() && alt_norm.contains(&kw_norm)) {
        score += 120;
    }

    // Penalize spin-offs / mini specials when the user did not search for them explicitly
    let spinoff_markers = [
        "～",
        "~",
        "ミニ",
        "小剧场",
        "总集篇",
        "ova",
        "oad",
        "特典",
        "sp",
    ];
    for marker in spinoff_markers {
        if (pri_trim.contains(marker) || alt_trim.contains(marker)) && !kw_trim.contains(marker) {
            score -= 260;
            break;
        }
    }

    if let (Some(want_y), Some(got_y)) = (year_hint, cand_year) {
        if want_y == got_y {
            score += 280;
        } else if (want_y - got_y).abs() == 1 {
            score += 90;
        }
    }

    score += (popularity_votes.min(15_000) / 50) as i32;
    score
}

/// Promotes the highest-priority provider that achieved an exact normalized title (and optional
/// author) match to index `0` when the top-priority provider only returned a fuzzy/derivative hit.
/// Prevents platform-exclusive webnovels from being overshadowed by fanfics on higher-priority sites.
pub fn promote_exact_match_anchor(
    sources: &mut Vec<(FieldSourceBadge, ScrapedMetadata)>,
    query: &ScrapeQuery,
) {
    if sources.len() <= 1 {
        return;
    }
    if query.isbn.as_deref().is_some_and(|s| !s.trim().is_empty()) {
        return;
    }
    let kw_norm = normalize_title_for_match(&query.keyword);
    if kw_norm.is_empty() {
        return;
    }
    let author_norm = query
        .artist_or_author
        .as_deref()
        .map(normalize_title_for_match)
        .unwrap_or_default();

    let exact_title = |m: &ScrapedMetadata| -> bool {
        [&m.title, &m.original_title, &m.series_or_album]
            .into_iter()
            .flatten()
            .any(|t| normalize_title_for_match(t) == kw_norm)
    };
    let exact_author = |m: &ScrapedMetadata| -> bool {
        !author_norm.is_empty()
            && m.creators
                .iter()
                .any(|c| normalize_title_for_match(c) == author_norm)
    };

    if !author_norm.is_empty() {
        if exact_title(&sources[0].1) && exact_author(&sources[0].1) {
            return;
        }
        if let Some(idx) = sources
            .iter()
            .position(|(_, m)| exact_title(m) && exact_author(m))
        {
            if idx > 0 {
                let item = sources.remove(idx);
                sources.insert(0, item);
            }
            return;
        }
    }

    if exact_title(&sources[0].1) {
        return;
    }
    if let Some(idx) = sources.iter().position(|(_, m)| exact_title(m))
        && idx > 0
    {
        let item = sources.remove(idx);
        sources.insert(0, item);
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

    #[test]
    fn promote_exact_match_anchor_elevates_canonical_exclusive_novel_over_fanfic() {
        use super::{ScrapeQuery, is_compatible_secondary_source, promote_exact_match_anchor};

        let mut raw = vec![
            (
                FieldSourceBadge {
                    provider_id: "qidian".into(),
                    provider_name: "起点中文网".into(),
                },
                ScrapedMetadata {
                    provider_id: "qidian".into(),
                    external_id: "999".into(),
                    title: Some("十日终焉：我在异界斩神".into()),
                    creators: vec!["路人甲".into()],
                    ..Default::default()
                },
            ),
            (
                FieldSourceBadge {
                    provider_id: "fanqie".into(),
                    provider_name: "番茄小说".into(),
                },
                ScrapedMetadata {
                    provider_id: "fanqie".into(),
                    external_id: "7143038691944959011".into(),
                    title: Some("十日终焉".into()),
                    creators: vec!["杀虫队队员".into()],
                    ..Default::default()
                },
            ),
        ];

        let query = ScrapeQuery {
            keyword: "十日终焉".into(),
            target: Some(ScrapeTargetKind::Book),
            ..Default::default()
        };
        promote_exact_match_anchor(&mut raw, &query);
        assert_eq!(raw[0].0.provider_id, "fanqie");
        assert_eq!(raw[0].1.title.as_deref(), Some("十日终焉"));
        // Different CJK author & fanfic title is rejected as incompatible secondary source
        assert!(!is_compatible_secondary_source(
            &raw[0].1,
            &raw[1].1,
            Some(ScrapeTargetKind::Book)
        ));
    }

    #[test]
    fn music_secondary_source_blocks_cover_singer_and_allows_cross_script_artist() {
        use super::is_compatible_secondary_source;

        let qq_primary = ScrapedMetadata {
            provider_id: "qq".into(),
            external_id: "0039MnYb0qxYhV".into(),
            target: Some(ScrapeTargetKind::MusicTrack),
            title: Some("晴天".into()),
            series_or_album: Some("叶惠美".into()),
            creators: vec!["周杰伦".into()],
            year: Some(2003),
            ..Default::default()
        };

        let cover_song = ScrapedMetadata {
            provider_id: "netease".into(),
            external_id: "2668397359".into(),
            target: Some(ScrapeTargetKind::MusicTrack),
            title: Some("晴天 (原唱 周杰伦)".into()),
            creators: vec!["RyaVocal".into()],
            ..Default::default()
        };
        assert!(!is_compatible_secondary_source(
            &qq_primary,
            &cover_song,
            Some(ScrapeTargetKind::MusicTrack)
        ));

        let other_cjk_singer = ScrapedMetadata {
            provider_id: "lrclib".into(),
            external_id: "123".into(),
            target: Some(ScrapeTargetKind::MusicTrack),
            title: Some("晴天".into()),
            creators: vec!["别的歌手".into()],
            ..Default::default()
        };
        assert!(!is_compatible_secondary_source(
            &qq_primary,
            &other_cjk_singer,
            Some(ScrapeTargetKind::MusicTrack)
        ));

        let cross_script_same_artist = ScrapedMetadata {
            provider_id: "itunes".into(),
            external_id: "US:1".into(),
            target: Some(ScrapeTargetKind::MusicTrack),
            title: Some("晴天".into()),
            creators: vec!["Jay Chou".into()],
            year: Some(2003),
            ..Default::default()
        };
        assert!(is_compatible_secondary_source(
            &qq_primary,
            &cross_script_same_artist,
            Some(ScrapeTargetKind::MusicTrack)
        ));
    }
}

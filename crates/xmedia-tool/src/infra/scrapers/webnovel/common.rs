use std::collections::BTreeMap;

use xmedia_core::domain::error::{DomainError, DomainResult};

use crate::domain::scraper::{
    ScrapeCandidate, ScrapeQuery, ScrapeTargetKind, ScrapedMetadata, score_candidate_relevance,
};

fn score_novel_candidate(
    query: &ScrapeQuery,
    title: &str,
    author: Option<&str>,
    year: Option<i32>,
    popularity_votes: u32,
) -> i32 {
    let base_before_parens = title
        .find(['(', '（', '【'])
        .map(|idx| title[..idx].trim())
        .filter(|s| !s.is_empty() && *s != title.trim());
    let mut score = score_candidate_relevance(
        &query.keyword,
        query.year,
        title,
        base_before_parens,
        year,
        popularity_votes,
    );
    if let (Some(want_author), Some(cand_author)) = (
        query
            .artist_or_author
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty()),
        author.map(str::trim).filter(|s| !s.is_empty()),
    ) {
        let w = want_author.to_lowercase();
        let c = cand_author.to_lowercase();
        if w == c {
            score += 500;
        } else if c.contains(&w) || w.contains(&c) {
            score += 220;
        }
    }
    score
}

fn build_novel_subtitle(author: Option<&str>, category: Option<&str>) -> Option<String> {
    let mut parts = Vec::new();
    if let Some(a) = author.map(str::trim).filter(|s| !s.is_empty()) {
        parts.push(a.to_string());
    }
    if let Some(c) = category.map(str::trim).filter(|s| !s.is_empty()) {
        parts.push(c.to_string());
    }
    (!parts.is_empty()).then(|| parts.join(" · "))
}

pub(super) fn extract_digits_book_id(external_id: &str, site_label: &str) -> DomainResult<String> {
    let digits: String = external_id
        .trim()
        .chars()
        .filter(|c| c.is_ascii_digit())
        .collect();
    if digits.is_empty() {
        return Err(DomainError::Validation(format!(
            "empty {site_label} book id"
        )));
    }
    Ok(digits)
}

#[derive(Default)]
pub(super) struct NovelCandidateDraft {
    pub external_id: String,
    pub title: String,
    pub author: Option<String>,
    pub category: Option<String>,
    pub year: Option<i32>,
    pub summary: Option<String>,
    pub cover_url: Option<String>,
    pub popularity_votes: u32,
}

pub(super) fn rank_novel_candidates(
    provider_id: &str,
    provider_name: &str,
    query: &ScrapeQuery,
    drafts: impl IntoIterator<Item = NovelCandidateDraft>,
    limit: usize,
) -> Vec<ScrapeCandidate> {
    let mut scored: Vec<(i32, usize, ScrapeCandidate)> = drafts
        .into_iter()
        .enumerate()
        .filter_map(|(idx, d)| {
            if d.external_id.trim().is_empty() || d.title.trim().is_empty() {
                return None;
            }
            let author_ref = d.author.as_deref().filter(|s| !s.trim().is_empty());
            let cat_ref = d.category.as_deref().filter(|s| !s.trim().is_empty());
            let score =
                score_novel_candidate(query, &d.title, author_ref, d.year, d.popularity_votes);
            Some((
                score,
                idx,
                ScrapeCandidate {
                    provider_id: provider_id.to_string(),
                    provider_name: provider_name.to_string(),
                    external_id: d.external_id,
                    target: ScrapeTargetKind::Book,
                    title: d.title,
                    subtitle: build_novel_subtitle(author_ref, cat_ref),
                    year: d.year,
                    summary: d.summary.filter(|s| !s.trim().is_empty()),
                    cover_url: d.cover_url,
                    synced_lyrics: None,
                    plain_lyrics: None,
                },
            ))
        })
        .collect();

    scored.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| a.1.cmp(&b.1)));
    scored.into_iter().take(limit).map(|(_, _, c)| c).collect()
}

pub(super) fn novel_candidate_from_detail(
    provider_id: &str,
    provider_name: &str,
    external_id: String,
    detail: ScrapedMetadata,
) -> Option<ScrapeCandidate> {
    let title = detail.title?;
    Some(ScrapeCandidate {
        provider_id: provider_id.to_string(),
        provider_name: provider_name.to_string(),
        external_id,
        target: ScrapeTargetKind::Book,
        title,
        subtitle: build_novel_subtitle(
            detail.creators.first().map(String::as_str),
            detail.genres.first().map(String::as_str),
        ),
        year: detail.year,
        summary: detail.summary,
        cover_url: detail.cover_url,
        synced_lyrics: None,
        plain_lyrics: None,
    })
}

#[derive(Default)]
pub(super) struct NovelMetadataDraft {
    pub title: String,
    pub series_or_album: Option<String>,
    pub author: Option<String>,
    pub year: Option<i32>,
    pub published_date: Option<String>,
    pub rating: Option<f32>,
    pub summary: Option<String>,
    pub genres: Vec<String>,
    pub tags: Vec<String>,
    pub cover_url: Option<String>,
}

impl NovelMetadataDraft {
    pub(super) fn into_metadata(
        self,
        provider_id: &str,
        provider_name: &str,
        external_id: impl Into<String>,
    ) -> ScrapedMetadata {
        let ext_id = external_id.into();
        let mut external_ids = BTreeMap::new();
        external_ids.insert(provider_id.to_string(), ext_id.clone());
        ScrapedMetadata {
            provider_id: provider_id.to_string(),
            external_id: ext_id,
            target: Some(ScrapeTargetKind::Book),
            title: Some(self.title),
            series_or_album: self.series_or_album,
            creators: self
                .author
                .filter(|a| !a.trim().is_empty())
                .into_iter()
                .collect(),
            publisher_or_studio: Some(provider_name.to_string()),
            year: self.year,
            published_date: self.published_date,
            language: Some("zh-CN".to_string()),
            rating: self.rating,
            summary: self.summary,
            genres: self.genres,
            tags: self.tags,
            cover_url: self.cover_url,
            external_ids,
            ..Default::default()
        }
    }
}

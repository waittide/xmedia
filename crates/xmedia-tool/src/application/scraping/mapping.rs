use std::collections::BTreeMap;

use xmedia_core::domain::library::MediaKind;

use crate::domain::media::{
    BookMetaPatch, ComicMetaPatch, MediaFileDetail, MediaMetadataPatch, MediaMetadataPayload,
    MusicMetaPatch, VideoFileMeta, VideoMetaPatch,
};
use crate::domain::scraper::{
    AggregateScrapeResponse, ScrapeCandidate, ScrapeQuery, ScrapeTargetKind, ScrapedMetadata,
};

pub(super) fn compute_scrape_confidence(query: &ScrapeQuery, agg: &AggregateScrapeResponse) -> u8 {
    let meta = &agg.merged.metadata;
    let matched = meta.title.as_deref().is_some_and(|s| !s.trim().is_empty());
    let mut score: i32 = if matched { 45 } else { 0 };
    let kw = query.keyword.trim().to_lowercase();
    if let Some(ref mt) = meta.title {
        let b = mt.trim().to_lowercase();
        if !kw.is_empty() && !b.is_empty() {
            if kw == b {
                score += 35;
            } else if b.contains(&kw) || kw.contains(&b) {
                score += 22;
            }
        }
    }
    if let Some(qa) = query.artist_or_author.as_deref().map(str::trim)
        && !qa.is_empty()
    {
        let qa_low = qa.to_lowercase();
        let creator_match = meta.creators.iter().any(|c| {
            let cl = c.trim().to_lowercase();
            !cl.is_empty() && (cl == qa_low || cl.contains(&qa_low) || qa_low.contains(&cl))
        });
        if creator_match {
            score += 12;
        }
    }
    if let (Some(qy), Some(my)) = (query.year, meta.year)
        && (qy - my).abs() <= 1
    {
        score += 10;
    }
    if agg.details_by_provider.len() >= 2 {
        score += 10;
    }
    score.clamp(0, 100) as u8
}

pub(super) fn filter_scraped_metadata_by_groups(
    mut meta: ScrapedMetadata,
    groups: &[String],
) -> ScrapedMetadata {
    if groups.is_empty() {
        return meta;
    }
    let has = |g: &str| groups.iter().any(|item| item.eq_ignore_ascii_case(g));
    if !has("basic") {
        meta.title = None;
        meta.original_title = None;
        meta.series_or_album = None;
        meta.creators.clear();
        meta.illustrators_or_directors.clear();
        meta.cast.clear();
        meta.publisher_or_studio = None;
        meta.year = None;
        meta.published_date = None;
        meta.language = None;
        meta.isbn = None;
        meta.rating = None;
        meta.track_or_episode_number = None;
        meta.disc_or_season_number = None;
    }
    if !has("summary") {
        meta.summary = None;
        meta.show_summary = None;
        meta.season_title = None;
        meta.season_summary = None;
    }
    if !has("tags") {
        meta.genres.clear();
        meta.tags.clear();
    }
    if !has("lyrics") {
        meta.synced_lyrics = None;
        meta.plain_lyrics = None;
    }
    meta
}

pub(super) fn is_webnovel_serial_only_provider(id: &str) -> bool {
    matches!(
        id,
        "qidian" | "fanqie" | "qimao" | "17k" | "ciweimao" | "sfacg" | "faloo" | "jjwxc"
    )
}

pub(super) fn apply_video_locked_fields(
    vp: &mut VideoMetaPatch,
    locked_fields: &[String],
    apply_cover: &mut bool,
) {
    let is_locked = |name: &str| locked_fields.iter().any(|f| f.eq_ignore_ascii_case(name));
    if is_locked("title") {
        vp.title = None;
    }
    if is_locked("original_title") {
        vp.original_title = None;
    }
    if is_locked("show_title") {
        vp.show_title = None;
    }
    if is_locked("summary") || is_locked("overview") {
        vp.summary = None;
    }
    if is_locked("genres") {
        vp.genres = None;
    }
    if is_locked("tags") {
        vp.tags = None;
    }
    if is_locked("actors") || is_locked("cast") {
        vp.actors = None;
    }
    if is_locked("directors") {
        vp.directors = None;
    }
    if is_locked("studio") {
        vp.studio = None;
    }
    if is_locked("year") {
        vp.year = None;
    }
    if is_locked("rating") {
        vp.rating = None;
    }
    if is_locked("collection_set") || is_locked("set") {
        vp.collection_set = None;
    }
    if is_locked("cover") || is_locked("poster") {
        *apply_cover = false;
    }
}

fn has_lrc_timestamps(text: &str) -> bool {
    text.lines().any(|line| {
        let trimmed = line.trim_start();
        let Some(rest) = trimmed.strip_prefix('[') else {
            return false;
        };
        let Some((min_part, after_colon)) = rest.split_once(':') else {
            return false;
        };
        if min_part.is_empty()
            || min_part.len() > 4
            || !min_part.bytes().all(|b| b.is_ascii_digit())
        {
            return false;
        }
        let sec_bytes = after_colon.as_bytes();
        sec_bytes.len() >= 2 && sec_bytes[0].is_ascii_digit() && sec_bytes[1].is_ascii_digit()
    })
}

pub(super) fn existing_artwork_present(meta: &MediaMetadataPayload) -> bool {
    match meta {
        MediaMetadataPayload::Book(b) => b.has_cover,
        MediaMetadataPayload::Comic(c) => c.has_cover,
        MediaMetadataPayload::Music(m) => m.has_embedded_cover,
        MediaMetadataPayload::Video(v) => v.has_poster,
    }
}

fn non_empty_opt(s: &Option<String>) -> bool {
    s.as_ref().is_some_and(|v| !v.trim().is_empty())
}

pub(super) fn filter_video_patch_for_missing(
    vp: &mut VideoMetaPatch,
    cur: &VideoFileMeta,
    fallback_stem: &str,
) {
    if cur.has_nfo && !cur.title.trim().is_empty() && cur.title.trim() != fallback_stem {
        vp.title = None;
    }
    if cur.has_nfo {
        vp.is_episode = None;
    }
    if non_empty_opt(&cur.original_title) {
        vp.original_title = None;
    }
    if non_empty_opt(&cur.show_title) {
        vp.show_title = None;
    }
    if cur.season_number.is_some() {
        vp.season_number = None;
    }
    if cur.episode_number.is_some() {
        vp.episode_number = None;
    }
    if cur.year.is_some() {
        vp.year = None;
    }
    if cur.rating.is_some() {
        vp.rating = None;
    }
    if non_empty_opt(&cur.summary) {
        vp.summary = None;
    }
    if !cur.genres.is_empty() {
        vp.genres = None;
    }
    if !cur.tags.is_empty() {
        vp.tags = None;
    }
    if !cur.actors.is_empty() {
        vp.actors = None;
    }
    if !cur.directors.is_empty() {
        vp.directors = None;
    }
    if non_empty_opt(&cur.studio) {
        vp.studio = None;
    }
    if non_empty_opt(&cur.tmdb_id) {
        vp.tmdb_id = None;
    }
    if non_empty_opt(&cur.bangumi_id) {
        vp.bangumi_id = None;
    }
}

pub(super) fn filter_patch_for_missing(
    patch: &mut MediaMetadataPatch,
    current: &MediaMetadataPayload,
    fallback_stem: &str,
) {
    match (patch, current) {
        (MediaMetadataPatch::Book(bp), MediaMetadataPayload::Book(b)) => {
            let has_tag_block = !b.authors.is_empty()
                || b.publisher.is_some()
                || b.isbn.is_some()
                || !b.tags.is_empty()
                || b.description.is_some();
            if !b.title.trim().is_empty() && (b.title.trim() != fallback_stem || has_tag_block) {
                bp.title = None;
            }
            if !b.authors.is_empty() {
                bp.authors = None;
            }
            if non_empty_opt(&b.series) {
                bp.series = None;
            }
            if non_empty_opt(&b.subseries) {
                bp.subseries = None;
            }
            if non_empty_opt(&b.series_index) {
                bp.series_index = None;
            }
            if non_empty_opt(&b.description) {
                bp.description = None;
            }
            if non_empty_opt(&b.publisher) {
                bp.publisher = None;
            }
            if non_empty_opt(&b.published_at) {
                bp.published_at = None;
            }
            if non_empty_opt(&b.language) {
                bp.language = None;
            }
            if non_empty_opt(&b.isbn) {
                bp.isbn = None;
            }
            if !b.tags.is_empty() {
                bp.tags = None;
            }
        }
        (MediaMetadataPatch::Comic(cp), MediaMetadataPayload::Comic(c)) => {
            if non_empty_opt(&c.series) {
                cp.series = None;
            }
            if non_empty_opt(&c.series_group) {
                cp.series_group = None;
            }
            if non_empty_opt(&c.title) {
                cp.title = None;
            }
            if non_empty_opt(&c.number) {
                cp.number = None;
            }
            if c.volume.is_some() {
                cp.volume = None;
            }
            if non_empty_opt(&c.summary) {
                cp.summary = None;
            }
            if !c.writers.is_empty() {
                cp.writers = None;
            }
            if !c.pencillers.is_empty() {
                cp.pencillers = None;
            }
            if non_empty_opt(&c.publisher) {
                cp.publisher = None;
            }
            if !c.genres.is_empty() {
                cp.genres = None;
            }
            if !c.tags.is_empty() {
                cp.tags = None;
            }
            if c.year.is_some() {
                cp.year = None;
            }
            if non_empty_opt(&c.language_iso) {
                cp.language_iso = None;
            }
        }
        (MediaMetadataPatch::Music(mp), MediaMetadataPayload::Music(m)) => {
            let has_tags = !m.artist.trim().is_empty()
                || !m.album.trim().is_empty()
                || m.track_number.is_some()
                || m.year.is_some();
            if !m.title.trim().is_empty() && (m.title.trim() != fallback_stem || has_tags) {
                mp.title = None;
            }
            if !m.artist.trim().is_empty() {
                mp.artist = None;
            }
            if non_empty_opt(&m.album_artist) {
                mp.album_artist = None;
            }
            if !m.album.trim().is_empty() {
                mp.album = None;
            }
            if m.track_number.is_some() {
                mp.track_number = None;
            }
            if m.disc_number.is_some() {
                mp.disc_number = None;
            }
            if m.year.is_some() {
                mp.year = None;
            }
            if non_empty_opt(&m.genre) {
                mp.genre = None;
            }
            if non_empty_opt(&m.embedded_lyrics) {
                mp.embedded_lyrics = None;
            }
            if non_empty_opt(&m.sidecar_lrc) {
                mp.sidecar_lrc = None;
            }
        }
        (MediaMetadataPatch::Video(vp), MediaMetadataPayload::Video(v)) => {
            filter_video_patch_for_missing(vp, v, fallback_stem);
        }
        _ => {}
    }
}

pub(super) fn map_scraped_to_patch(
    media_kind: MediaKind,
    s: &ScrapedMetadata,
    write_sidecar_lrc: bool,
    existing_music_artist_non_empty: bool,
) -> MediaMetadataPatch {
    match media_kind {
        MediaKind::Book => MediaMetadataPatch::Book(BookMetaPatch {
            title: s.title.clone(),
            authors: (!s.creators.is_empty()).then(|| s.creators.clone()),
            series: s.series_or_album.clone(),
            subseries: None,
            series_index: s.track_or_episode_number.map(|n| n.to_string()),
            description: s.summary.clone(),
            publisher: s.publisher_or_studio.clone(),
            published_at: s
                .published_date
                .clone()
                .or_else(|| s.year.map(|y| y.to_string())),
            language: s.language.clone(),
            isbn: s.isbn.clone(),
            tags: {
                let mut merged = s.genres.clone();
                for t in &s.tags {
                    if !merged.contains(t) {
                        merged.push(t.clone());
                    }
                }
                (!merged.is_empty()).then_some(merged)
            },
            locked_fields: None,
        }),
        MediaKind::Comic => {
            let is_chapter_scrape = s.track_or_episode_number.is_some();
            MediaMetadataPatch::Comic(ComicMetaPatch {
                series: s.series_or_album.clone().or_else(|| s.title.clone()),
                series_group: None,
                title: is_chapter_scrape.then(|| s.title.clone()).flatten(),
                number: s.track_or_episode_number.map(|n| n.to_string()),
                volume: is_chapter_scrape
                    .then(|| s.disc_or_season_number.and_then(|v| i32::try_from(v).ok()))
                    .flatten(),
                summary: s.summary.clone(),
                writers: (!s.creators.is_empty()).then(|| s.creators.clone()),
                pencillers: (!s.illustrators_or_directors.is_empty())
                    .then(|| s.illustrators_or_directors.clone()),
                publisher: s.publisher_or_studio.clone(),
                genres: (!s.genres.is_empty()).then(|| s.genres.clone()),
                tags: (!s.tags.is_empty()).then(|| s.tags.clone()),
                year: s.year,
                language_iso: s.language.clone(),
                reading_direction: None,
            })
        }
        MediaKind::Music => {
            let best_lyrics = s.synced_lyrics.clone().or_else(|| s.plain_lyrics.clone());
            let sidecar_lrc = if write_sidecar_lrc {
                s.synced_lyrics
                    .as_deref()
                    .filter(|l| has_lrc_timestamps(l))
                    .map(str::to_string)
            } else {
                None
            };
            if s.target == Some(ScrapeTargetKind::Lyrics) {
                MediaMetadataPatch::Music(MusicMetaPatch {
                    embedded_lyrics: best_lyrics,
                    sidecar_lrc,
                    ..Default::default()
                })
            } else if s.target == Some(ScrapeTargetKind::MusicAlbum) {
                MediaMetadataPatch::Music(MusicMetaPatch {
                    title: None,
                    artist: if existing_music_artist_non_empty {
                        None
                    } else {
                        s.creators.first().cloned()
                    },
                    album_artist: (!s.creators.is_empty()).then(|| s.creators.join(", ")),
                    album: s.series_or_album.clone().or_else(|| s.title.clone()),
                    track_number: None,
                    disc_number: None,
                    year: s.year,
                    genre: s.genres.first().cloned(),
                    comment: None,
                    embedded_lyrics: None,
                    sidecar_lrc: None,
                })
            } else {
                MediaMetadataPatch::Music(MusicMetaPatch {
                    title: s.title.clone(),
                    artist: s.creators.first().cloned(),
                    album_artist: (!s.creators.is_empty()).then(|| s.creators.join(", ")),
                    album: s.series_or_album.clone(),
                    track_number: s.track_or_episode_number,
                    disc_number: s.disc_or_season_number,
                    year: s.year,
                    genre: s.genres.first().cloned(),
                    comment: None,
                    embedded_lyrics: best_lyrics,
                    sidecar_lrc,
                })
            }
        }
        MediaKind::Video => {
            let is_ep = s.target == Some(ScrapeTargetKind::TvShow)
                || s.track_or_episode_number.is_some()
                || s.series_or_album.is_some();
            MediaMetadataPatch::Video(VideoMetaPatch {
                is_episode: Some(is_ep),
                title: s.title.clone(),
                original_title: s.original_title.clone(),
                show_title: s.series_or_album.clone(),
                season_number: s.disc_or_season_number.and_then(|v| i32::try_from(v).ok()),
                episode_number: s.track_or_episode_number,
                year: s.year,
                rating: s.rating,
                summary: s.summary.clone(),
                genres: (!s.genres.is_empty()).then(|| s.genres.clone()),
                tags: (!s.tags.is_empty()).then(|| s.tags.clone()),
                actors: (!s.cast.is_empty()).then(|| s.cast.clone()),
                directors: (!s.illustrators_or_directors.is_empty())
                    .then(|| s.illustrators_or_directors.clone()),
                studio: s.publisher_or_studio.clone(),
                tmdb_id: s
                    .external_ids
                    .get("tmdb")
                    .cloned()
                    .or_else(|| (s.provider_id == "tmdb").then(|| s.external_id.clone())),
                bangumi_id: s
                    .external_ids
                    .get("bangumi")
                    .cloned()
                    .or_else(|| (s.provider_id == "bangumi").then(|| s.external_id.clone())),
                ..Default::default()
            })
        }
    }
}

fn strip_trailing_year_tag(raw: &str) -> String {
    let trimmed = raw.trim();
    if let Some(idx) = trimmed.rfind(['(', '[']) {
        let suffix = trimmed[idx..].trim();
        if suffix.len() == 6
            && (suffix.ends_with(')') || suffix.ends_with(']'))
            && suffix[1..5]
                .parse::<i32>()
                .is_ok_and(|y| (1900..=2099).contains(&y))
        {
            return trimmed[..idx].trim().to_string();
        }
    }
    trimmed.to_string()
}

fn strip_comic_chapter_suffix(raw: &str) -> String {
    let cleaned = raw.replace('_', " ");
    let mut s = cleaned.trim();
    while s.starts_with('[') || s.starts_with('(') {
        let close = if s.starts_with('[') { ']' } else { ')' };
        if let Some(pos) = s.find(close) {
            s = s[pos + 1..].trim();
        } else {
            break;
        }
    }
    let lower = s.to_ascii_lowercase();
    for marker in [" vol.", " ch.", " chapter ", " 第"] {
        if let Some(pos) = lower.find(marker) {
            let head = s[..pos].trim();
            if !head.is_empty() {
                return head.to_string();
            }
        }
    }
    s.to_string()
}

pub(super) fn build_auto_query_from_detail(d: &MediaFileDetail) -> ScrapeQuery {
    match &d.metadata {
        MediaMetadataPayload::Music(m) => ScrapeQuery {
            target: Some(ScrapeTargetKind::MusicTrack),
            keyword: if m.title.trim().is_empty() {
                d.entry.display_title.clone()
            } else {
                m.title.trim().to_string()
            },
            artist_or_author: (!m.artist.trim().is_empty()).then(|| m.artist.trim().to_string()),
            album_or_series: (!m.album.trim().is_empty()).then(|| m.album.trim().to_string()),
            year: m.year,
            duration_secs: (m.duration_secs > 0).then_some(m.duration_secs),
            ..Default::default()
        },
        MediaMetadataPayload::Video(v) => {
            let raw_kw = if v.is_episode {
                v.show_title
                    .as_deref()
                    .filter(|s| !s.trim().is_empty())
                    .unwrap_or_else(|| {
                        if v.title.trim().is_empty() {
                            d.entry.display_title.as_str()
                        } else {
                            v.title.as_str()
                        }
                    })
            } else if v.title.trim().is_empty() {
                d.entry.display_title.as_str()
            } else {
                v.title.as_str()
            };
            ScrapeQuery {
                target: Some(if v.is_episode {
                    ScrapeTargetKind::TvShow
                } else {
                    ScrapeTargetKind::Movie
                }),
                keyword: strip_trailing_year_tag(raw_kw),
                year: v.year,
                season: v.season_number,
                episode: v.episode_number,
                duration_secs: (v.duration_secs > 0).then_some(v.duration_secs),
                ..Default::default()
            }
        }
        MediaMetadataPayload::Comic(c) => {
            let title_or_disp = c
                .title
                .as_deref()
                .filter(|s| !s.trim().is_empty())
                .unwrap_or(&d.entry.display_title);
            let keyword = c
                .series
                .as_deref()
                .filter(|s| !s.trim().is_empty())
                .map(str::to_string)
                .unwrap_or_else(|| strip_comic_chapter_suffix(title_or_disp));
            ScrapeQuery {
                target: Some(ScrapeTargetKind::Comic),
                keyword,
                artist_or_author: c.writers.first().cloned(),
                year: c.year,
                ..Default::default()
            }
        }
        MediaMetadataPayload::Book(b) => {
            let parsed_year = b
                .published_at
                .as_deref()
                .and_then(|s| s.get(0..4))
                .and_then(|y| y.parse::<i32>().ok());
            ScrapeQuery {
                target: Some(ScrapeTargetKind::Book),
                keyword: if b.title.trim().is_empty() {
                    d.entry.display_title.clone()
                } else {
                    b.title.trim().to_string()
                },
                artist_or_author: b.authors.first().cloned(),
                album_or_series: b.series.clone(),
                isbn: b.isbn.clone(),
                year: parsed_year,
                ..Default::default()
            }
        }
    }
}

pub(super) fn fallback_metadata_from_candidate(cand: &ScrapeCandidate) -> ScrapedMetadata {
    let mut creators: Vec<String> = Vec::new();
    let mut genres: Vec<String> = Vec::new();
    let mut series_or_album: Option<String> = None;
    let mut publisher_or_studio: Option<String> = None;
    let mut language: Option<String> = None;

    const CN_NOVEL_PROVIDERS: &[&str] = &[
        "qidian", "fanqie", "qimao", "17k", "ciweimao", "sfacg", "faloo", "jjwxc",
    ];
    const MUSIC_PROVIDERS: &[&str] = &[
        "qq",
        "netease",
        "kugou",
        "kuwo",
        "migu",
        "taihe",
        "qishui",
        "musicbrainz",
        "itunes",
        "lrclib",
    ];

    if let Some(sub) = cand
        .subtitle
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
    {
        let parts: Vec<&str> = sub
            .split(" · ")
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .collect();
        if CN_NOVEL_PROVIDERS.contains(&cand.provider_id.as_str()) {
            if let Some(first) = parts.first() {
                creators.push((*first).to_string());
            }
            if let Some(second) = parts.get(1) {
                genres.push((*second).to_string());
            }
        } else if MUSIC_PROVIDERS.contains(&cand.provider_id.as_str()) {
            if let Some(first) = parts.first() {
                for artist in first
                    .split(['/', '、'])
                    .map(str::trim)
                    .filter(|s| !s.is_empty())
                {
                    if !creators.iter().any(|c| c == artist) {
                        creators.push(artist.to_string());
                    }
                }
            }
            if let Some(second) = parts.get(1) {
                series_or_album = Some((*second).to_string());
            }
        } else if matches!(
            cand.target,
            ScrapeTargetKind::Book | ScrapeTargetKind::Comic
        ) {
            if let Some(first) = parts.first() {
                for author in first.split(" / ").map(str::trim).filter(|s| !s.is_empty()) {
                    if !creators.iter().any(|c| c == author) {
                        creators.push(author.to_string());
                    }
                }
            }
            if let Some(second) = parts.get(1) {
                publisher_or_studio = Some((*second).to_string());
            }
        }
    }

    if CN_NOVEL_PROVIDERS.contains(&cand.provider_id.as_str()) {
        if publisher_or_studio.is_none() && !cand.provider_name.trim().is_empty() {
            publisher_or_studio = Some(cand.provider_name.clone());
        }
        language = Some("zh-CN".to_string());
    }

    let mut external_ids = BTreeMap::new();
    if !cand.provider_id.trim().is_empty() && !cand.external_id.trim().is_empty() {
        external_ids.insert(cand.provider_id.clone(), cand.external_id.clone());
    }

    ScrapedMetadata {
        provider_id: cand.provider_id.clone(),
        external_id: cand.external_id.clone(),
        target: Some(cand.target),
        title: Some(cand.title.clone()),
        series_or_album,
        creators,
        publisher_or_studio,
        year: cand.year,
        language,
        summary: cand.summary.clone(),
        genres,
        cover_url: cand.cover_url.clone(),
        synced_lyrics: cand.synced_lyrics.clone(),
        plain_lyrics: cand.plain_lyrics.clone(),
        external_ids,
        ..Default::default()
    }
}

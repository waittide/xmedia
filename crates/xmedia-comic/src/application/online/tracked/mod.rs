mod cache;

use std::collections::HashSet;

use jiff::Timestamp;
use serde::Deserialize;
use uuid::Uuid;
use xmedia_core::application::ports::SystemEvent;
use xmedia_core::domain::error::{DomainError, DomainResult};
use xmedia_core::domain::library::{Library, MediaKind};
use xmedia_core::domain::user::User;
use xmedia_source::http::{HttpClientConfig, resolve_literal_http_url};
use xmedia_source::proxy::{DEFAULT_IMAGE_PROXY_TIMEOUT_SECS, fetch_image};

pub use cache::ComicCacheReportDto;

use crate::application::dto::SeriesDetailDto;
use crate::domain::chapter::{Chapter, ChapterKind, ChapterSource, OnlineTrackedChapterMeta};
use crate::domain::id::SeriesId;
use crate::domain::online::{ComicJsContext, ComicSourceSpec, OnlineComicChapter};
use crate::domain::series::{CoverUpdate, OnlineTrackedSeriesMeta, Series, SeriesStatus};

use super::helpers::weak_lock;
use super::{COMIC_IMAGE_PROXY_MAX_BYTES, ComicOnlineService};

#[derive(Debug, Clone, Deserialize)]
pub struct TrackOnlineComicInput {
    pub source_id: Uuid,
    #[serde(alias = "book_url")]
    pub comic_url: String,
    #[serde(default)]
    pub toc_url: Option<String>,
    #[serde(default)]
    pub title: Option<String>,
    #[serde(default)]
    pub author: Option<String>,
    #[serde(default)]
    pub cover_url: Option<String>,
    #[serde(default)]
    pub summary: Option<String>,
    #[serde(default)]
    pub reading_direction: Option<String>,
    #[serde(default)]
    pub genres: Option<Vec<String>>,
    #[serde(default)]
    pub status: Option<String>,
}

impl ComicOnlineService {
    async fn ensure_managed_comic_library(&self) -> DomainResult<Library> {
        self.catalog.get_or_create_managed_library().await
    }

    async fn fetch_image_with_spec(
        &self,
        parsed: &ComicSourceSpec,
        raw_url: &str,
    ) -> DomainResult<(Vec<u8>, String)> {
        let mut headers = parsed.image_headers(Some(raw_url));
        headers.retain(|k, _| !k.eq_ignore_ascii_case("range"));
        let cfg = HttpClientConfig {
            timeout_secs: DEFAULT_IMAGE_PROXY_TIMEOUT_SECS,
            allow_private_ips: self.allow_private_ips,
            proxy_url: None,
        };
        let (bytes, mime) =
            fetch_image(raw_url, Some(&headers), COMIC_IMAGE_PROXY_MAX_BYTES, &cfg).await?;
        Ok((bytes, mime.to_string()))
    }

    /// Tracks an online comic into the unified managed comic library (hybrid mode):
    /// creates the series + TOC chapter entries immediately without downloading all pages;
    /// pages are fetched & packaged as CBZ transparently on first read / OPDS access or
    /// via explicit offline caching.
    pub async fn track_online_comic(
        &self,
        actor: &User,
        input: TrackOnlineComicInput,
    ) -> DomainResult<SeriesDetailDto> {
        let rec = self
            .source_crud
            .load_source_for_actor(actor, input.source_id)
            .await?;
        let parsed: ComicSourceSpec = serde_json::from_str(&rec.raw_json)
            .map_err(|e| DomainError::Validation(format!("漫画源 JSON 无效: {e}")))?;

        let clean_comic_url = resolve_literal_http_url(&input.comic_url, Some(&parsed.source_url))?;
        let clean_input_toc = input
            .toc_url
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(|u| resolve_literal_http_url(u, Some(&parsed.source_url)))
            .transpose()?;
        let clean_input_cover = input
            .cover_url
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(|u| resolve_literal_http_url(u, Some(&parsed.source_url)))
            .transpose()?;

        let lock = weak_lock(
            &self.track_locks,
            (input.source_id, clean_comic_url.clone()),
        );
        let _guard = lock.lock().await;

        let managed_lib = self.ensure_managed_comic_library().await?;
        if let Some(found) = self
            .series
            .query(crate::domain::repository::SeriesQuery::OnlineTracked {
                source_id: input.source_id,
                comic_url: clean_comic_url.clone(),
            })
            .await?
            .into_iter()
            .next()
        {
            self.catalog
                .bind_series_for_user(actor.id, found.id)
                .await?;
            return self.catalog.get_series_detail(actor, found.id).await;
        }

        let fallback_hint = ComicJsContext {
            name: input.title.clone().unwrap_or_default(),
            author: input.author.clone().unwrap_or_default(),
            book_url: clean_comic_url.clone(),
            toc_url: clean_input_toc.clone().unwrap_or_default(),
            cover_url: clean_input_cover.clone(),
            intro: input.summary.clone(),
            reading_direction: input.reading_direction.clone(),
            ..Default::default()
        };

        let detail_res = self
            .fetch_detail(
                actor,
                input.source_id,
                &clean_comic_url,
                Some(fallback_hint),
            )
            .await;

        let (title, author, summary, cover_url, toc_url, status_str, direction_str, mut toc_items) =
            match detail_res {
                Ok(resp) => {
                    let d = resp.detail;
                    let resolved_title = if !d.title.trim().is_empty() {
                        d.title
                    } else {
                        input
                            .title
                            .clone()
                            .unwrap_or_else(|| "Untitled Comic".into())
                    };
                    let resolved_toc = if !d.toc_url.trim().is_empty() {
                        d.toc_url
                    } else {
                        clean_input_toc
                            .clone()
                            .unwrap_or_else(|| clean_comic_url.clone())
                    };
                    (
                        resolved_title,
                        if !d.author.trim().is_empty() {
                            Some(d.author)
                        } else {
                            input.author.clone()
                        },
                        d.intro.or_else(|| input.summary.clone()),
                        d.cover_url.or(clean_input_cover),
                        resolved_toc,
                        d.status.or_else(|| input.status.clone()),
                        Some(d.reading_direction.as_ref().to_string()),
                        resp.chapters,
                    )
                }
                Err(_) => (
                    input
                        .title
                        .clone()
                        .filter(|s| !s.trim().is_empty())
                        .ok_or_else(|| {
                            DomainError::Validation("无法获取漫画详情且未提供标题".into())
                        })?,
                    input.author.clone(),
                    input.summary.clone(),
                    clean_input_cover,
                    clean_input_toc.unwrap_or_else(|| clean_comic_url.clone()),
                    input.status.clone(),
                    input.reading_direction.clone(),
                    Vec::new(),
                ),
            };

        let clean_toc_url = resolve_literal_http_url(&toc_url, Some(&parsed.source_url))
            .unwrap_or_else(|_| clean_comic_url.clone());

        if toc_items.is_empty() {
            let comic_ctx = ComicJsContext {
                name: title.clone(),
                author: author.clone().unwrap_or_default(),
                book_url: clean_comic_url.clone(),
                toc_url: clean_toc_url.clone(),
                cover_url: cover_url.clone(),
                intro: summary.clone(),
                reading_direction: direction_str.clone(),
                ..Default::default()
            };
            if let Ok(ch_resp) = self
                .fetch_chapters(actor, input.source_id, &clean_toc_url, comic_ctx)
                .await
            {
                toc_items = ch_resp.chapters;
            }
        }

        let series_meta = OnlineTrackedSeriesMeta {
            source_id: input.source_id,
            comic_url: clean_comic_url.clone(),
            toc_url: clean_toc_url.clone(),
        };
        let mut series = Series::new(
            managed_lib.id,
            title.clone(),
            crate::domain::series::SeriesSource::OnlineTracked(series_meta.clone()),
        )?;
        if let Some(ref au) = author {
            let writers: Vec<String> = au
                .split(['/', ',', '、', ';'])
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(ToOwned::to_owned)
                .collect();
            if !writers.is_empty() {
                series.writers = writers;
            }
        }
        if let Some(ref intro) = summary {
            series.summary = intro.trim().to_owned();
        }
        if let Some(ref genres) = input.genres {
            series.genres = genres
                .iter()
                .map(|g| g.trim().to_owned())
                .filter(|s| !s.is_empty())
                .collect();
        }
        if let Some(ref st) = status_str {
            series.status = parse_series_status_hint(st);
        }
        if let Some(ref dir) = direction_str {
            series.reading_direction = dir.parse().unwrap_or_default();
        }

        let now = Timestamp::now();
        if let Some(ref cov_url) = cover_url
            && let Ok((cov_bytes, _)) = self.fetch_image_with_spec(&parsed, cov_url).await
        {
            let thumb_key = format!("series_{}", series.id);
            if let Ok(saved_key) = self
                .thumbnails
                .generate_from_bytes(&thumb_key, &cov_bytes)
                .await
            {
                let _ = series.apply_cover(CoverUpdate::Scanned(saved_key), now);
            }
        }

        let created_count = self
            .append_online_chapters(&series, &series_meta, &toc_items, &HashSet::new(), 0)
            .await?;
        let _ = series.sync_chapter_count(created_count, now);
        series.updated_at = now;
        self.series.save(&series).await?;
        self.catalog
            .bind_series_for_user(actor.id, series.id)
            .await?;

        self.events.publish(SystemEvent::MediaChanged {
            media_kind: MediaKind::Comic,
        });

        self.catalog.get_series_detail(actor, series.id).await
    }

    /// Re-fetches the online TOC for a tracked comic series and appends any new chapters.
    pub async fn refresh_tracked_series(
        &self,
        actor: &User,
        series_id: SeriesId,
    ) -> DomainResult<SeriesDetailDto> {
        self.catalog.require_series_bound(actor, series_id).await?;
        let mut series = self
            .series
            .query(crate::domain::repository::SeriesQuery::Id(series_id))
            .await?
            .into_iter()
            .next()
            .ok_or_else(|| DomainError::NotFound("series not found".into()))?;
        let Some(series_ref) = series.source.online_tracking().cloned() else {
            return Err(DomainError::Validation("该漫画系列不是在线追更系列".into()));
        };

        let existing = self
            .chapters
            .query(crate::domain::repository::ChapterQuery::Series(series_id))
            .await?;
        let existing_urls: HashSet<String> = existing
            .iter()
            .filter_map(|c| c.source.online_tracking().map(|t| t.chapter_url.clone()))
            .collect();
        let start_number = existing.len() as u32;

        let comic_ctx = ComicJsContext {
            name: series.title.clone(),
            author: series.writers.join(", "),
            book_url: series_ref.comic_url.clone(),
            toc_url: series_ref.toc_url.clone(),
            cover_url: None,
            intro: Some(series.summary.clone()),
            reading_direction: Some(series.reading_direction.as_ref().to_string()),
            ..Default::default()
        };

        let toc_target = if !series_ref.toc_url.trim().is_empty() {
            series_ref.toc_url.clone()
        } else {
            series_ref.comic_url.clone()
        };
        let toc_resp = self
            .fetch_chapters(actor, series_ref.source_id, &toc_target, comic_ctx)
            .await?;

        let added = self
            .append_online_chapters(
                &series,
                &series_ref,
                &toc_resp.chapters,
                &existing_urls,
                start_number,
            )
            .await?;

        if added > 0 {
            let total = self
                .chapters
                .query(crate::domain::repository::ChapterQuery::Series(series_id))
                .await?
                .into_iter()
                .filter(|c| !c.is_missing)
                .count() as u32;
            let now = Timestamp::now();
            let _ = series.sync_chapter_count(total, now);
            series.updated_at = now;
            self.series.save(&series).await?;
            self.events.publish(SystemEvent::MediaChanged {
                media_kind: MediaKind::Comic,
            });
        }

        self.catalog.get_series_detail(actor, series_id).await
    }

    async fn append_online_chapters(
        &self,
        series: &Series,
        series_ref: &OnlineTrackedSeriesMeta,
        toc_items: &[OnlineComicChapter],
        existing_urls: &HashSet<String>,
        start_number: u32,
    ) -> DomainResult<u32> {
        let mut added = 0_u32;
        let mut seen = existing_urls.clone();
        for item in toc_items {
            if item.is_volume {
                continue;
            }
            let url = item.url.trim();
            if url.is_empty() || !seen.insert(url.to_string()) {
                continue;
            }
            let num = f64::from(start_number + added + 1);
            let ch_meta = OnlineTrackedChapterMeta {
                source_id: series_ref.source_id,
                comic_url: series_ref.comic_url.clone(),
                toc_url: series_ref.toc_url.clone(),
                chapter_url: url.to_string(),
                cached_archive_path: None,
            };
            let chapter = Chapter::new(
                series.id,
                series.library_id,
                ChapterKind::Chapter,
                num,
                None,
                item.title.clone(),
                0,
                ChapterSource::OnlineTracked(ch_meta),
                0,
                String::new(),
            )?;
            self.chapters.save(&chapter).await?;
            added += 1;
        }
        Ok(added)
    }
}

fn parse_series_status_hint(raw: &str) -> SeriesStatus {
    let lower = raw.trim().to_ascii_lowercase();
    if lower.contains("完结")
        || lower.contains("已完")
        || lower.contains("complete")
        || lower.contains("ended")
    {
        SeriesStatus::Completed
    } else if lower.contains("休刊") || lower.contains("暂停") || lower.contains("hiatus") {
        SeriesStatus::Hiatus
    } else {
        SeriesStatus::Ongoing
    }
}

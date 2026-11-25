use async_trait::async_trait;
use toasty::Db;
use uuid::Uuid;
use xmedia_core::domain::error::DomainResult;
use xmedia_core::domain::library::LibraryId;

use crate::domain::id::SeriesId;
use crate::domain::repository::SeriesRepository;
use crate::domain::series::{
    OnlineTrackedSeriesMeta, ReadingDirection, Series, SeriesSource, SeriesStatus,
};

use super::super::models::ComicSeriesModel;
use super::db_err;

#[derive(Clone)]
pub struct ToastySeriesRepository {
    db: Db,
}

impl ToastySeriesRepository {
    pub fn new(db: Db) -> Self {
        Self { db }
    }

    fn to_domain(m: ComicSeriesModel) -> Series {
        let source = if m.source_kind == "online_tracked" {
            let meta = m
                .online_meta
                .map(|j| j.0)
                .unwrap_or_else(|| OnlineTrackedSeriesMeta {
                    source_id: m.online_source_id.unwrap_or_else(Uuid::nil),
                    comic_url: m.online_comic_url.unwrap_or_default(),
                    toc_url: String::new(),
                });
            SeriesSource::OnlineTracked(meta)
        } else {
            SeriesSource::Local {
                source_dir: m.source_dir,
            }
        };

        Series {
            id: SeriesId(m.id),
            library_id: LibraryId(m.library_id),
            title: m.title,
            sort_title: m.sort_title,
            alt_titles: m.alt_titles.0,
            summary: m.summary,
            writers: m.writers.0,
            pencillers: m.pencillers.0,
            publisher: m.publisher,
            status: SeriesStatus::parse(&m.status),
            reading_direction: ReadingDirection::parse(&m.reading_direction),
            age_rating: m.age_rating,
            genres: m.genres.0,
            tags: m.tags.0,
            language: m.language,
            year: m.year.and_then(|y| i32::try_from(y).ok()),
            cover_path: m.cover_path,
            cover_is_custom: m.cover_is_custom,
            chapter_count: u32::try_from(m.chapter_count.max(0)).unwrap_or(0),
            source,
            metadata_locked: m.metadata_locked,
            created_at: m.created_at,
            updated_at: m.updated_at,
        }
    }
}

#[async_trait]
impl SeriesRepository for ToastySeriesRepository {
    async fn find_by_id(&self, id: SeriesId) -> DomainResult<Option<Series>> {
        let mut db = self.db.clone();
        let row = ComicSeriesModel::filter_by_id(id.0)
            .first()
            .exec(&mut db)
            .await
            .map_err(db_err)?;
        Ok(row.map(Self::to_domain))
    }

    async fn find_by_library_and_dir(
        &self,
        library_id: LibraryId,
        source_dir: &str,
    ) -> DomainResult<Option<Series>> {
        let mut db = self.db.clone();
        let row = ComicSeriesModel::filter_by_library_id(library_id.0)
            .filter(ComicSeriesModel::fields().source_kind().eq("local"))
            .filter(
                ComicSeriesModel::fields()
                    .source_dir()
                    .eq(source_dir.to_string()),
            )
            .first()
            .exec(&mut db)
            .await
            .map_err(db_err)?;
        Ok(row.map(Self::to_domain))
    }

    async fn find_online_tracked(
        &self,
        source_id: Uuid,
        comic_url: &str,
    ) -> DomainResult<Option<Series>> {
        let mut db = self.db.clone();
        let row = ComicSeriesModel::filter_by_online_source_id(source_id)
            .filter(
                ComicSeriesModel::fields()
                    .online_comic_url()
                    .eq(comic_url.to_string()),
            )
            .first()
            .exec(&mut db)
            .await
            .map_err(db_err)?;
        Ok(row.map(Self::to_domain))
    }

    async fn find_series_by_cover_key(&self, cover_key: &str) -> DomainResult<Vec<Series>> {
        let clean = cover_key.trim();
        if clean.is_empty() {
            return Ok(Vec::new());
        }
        let mut db = self.db.clone();
        let rows = ComicSeriesModel::filter_by_cover_path(clean.to_string())
            .exec(&mut db)
            .await
            .map_err(db_err)?;
        Ok(rows.into_iter().map(Self::to_domain).collect())
    }

    async fn list_all(&self) -> DomainResult<Vec<Series>> {
        let mut db = self.db.clone();
        let rows = ComicSeriesModel::all()
            .exec(&mut db)
            .await
            .map_err(db_err)?;
        Ok(rows.into_iter().map(Self::to_domain).collect())
    }

    async fn list_by_library(&self, library_id: LibraryId) -> DomainResult<Vec<Series>> {
        let mut db = self.db.clone();
        let rows = ComicSeriesModel::filter_by_library_id(library_id.0)
            .exec(&mut db)
            .await
            .map_err(db_err)?;
        Ok(rows.into_iter().map(Self::to_domain).collect())
    }

    async fn save(&self, s: &Series) -> DomainResult<()> {
        let mut db = self.db.clone();
        let existing = ComicSeriesModel::filter_by_id(s.id.0)
            .first()
            .exec(&mut db)
            .await
            .map_err(db_err)?;

        let year_i64 = s.year.map(i64::from);
        let ch_count = i64::from(s.chapter_count);
        let (source_kind, source_dir, online_source_id, online_comic_url, online_meta) =
            match &s.source {
                SeriesSource::Local { source_dir } => {
                    ("local".to_string(), source_dir.clone(), None, None, None)
                }
                SeriesSource::OnlineTracked(meta) => (
                    "online_tracked".to_string(),
                    String::new(),
                    Some(meta.source_id),
                    Some(meta.comic_url.clone()),
                    Some(toasty::Json(meta.clone())),
                ),
            };

        if let Some(mut record) = existing {
            record
                .update()
                .library_id(s.library_id.0)
                .title(s.title.clone())
                .sort_title(s.sort_title.clone())
                .alt_titles(toasty::Json(s.alt_titles.clone()))
                .summary(s.summary.clone())
                .writers(toasty::Json(s.writers.clone()))
                .pencillers(toasty::Json(s.pencillers.clone()))
                .publisher(s.publisher.clone())
                .status(s.status.as_str().to_string())
                .reading_direction(s.reading_direction.as_str().to_string())
                .age_rating(s.age_rating.clone())
                .genres(toasty::Json(s.genres.clone()))
                .tags(toasty::Json(s.tags.clone()))
                .language(s.language.clone())
                .year(year_i64)
                .cover_path(s.cover_path.clone())
                .cover_is_custom(s.cover_is_custom)
                .chapter_count(ch_count)
                .source_kind(source_kind)
                .source_dir(source_dir)
                .online_source_id(online_source_id)
                .online_comic_url(online_comic_url)
                .online_meta(online_meta)
                .metadata_locked(s.metadata_locked)
                .updated_at(s.updated_at)
                .exec(&mut db)
                .await
                .map_err(db_err)?;
        } else {
            toasty::create!(ComicSeriesModel {
                id: s.id.0,
                library_id: s.library_id.0,
                title: s.title.clone(),
                sort_title: s.sort_title.clone(),
                alt_titles: toasty::Json(s.alt_titles.clone()),
                summary: s.summary.clone(),
                writers: toasty::Json(s.writers.clone()),
                pencillers: toasty::Json(s.pencillers.clone()),
                publisher: s.publisher.clone(),
                status: s.status.as_str().to_string(),
                reading_direction: s.reading_direction.as_str().to_string(),
                age_rating: s.age_rating.clone(),
                genres: toasty::Json(s.genres.clone()),
                tags: toasty::Json(s.tags.clone()),
                language: s.language.clone(),
                year: year_i64,
                cover_path: s.cover_path.clone(),
                cover_is_custom: s.cover_is_custom,
                chapter_count: ch_count,
                source_kind: source_kind,
                source_dir: source_dir,
                online_source_id: online_source_id,
                online_comic_url: online_comic_url,
                online_meta: online_meta,
                metadata_locked: s.metadata_locked,
                created_at: s.created_at,
                updated_at: s.updated_at,
            })
            .exec(&mut db)
            .await
            .map_err(db_err)?;
        }
        Ok(())
    }

    async fn delete(&self, id: SeriesId) -> DomainResult<()> {
        let mut db = self.db.clone();
        ComicSeriesModel::filter_by_id(id.0)
            .delete()
            .exec(&mut db)
            .await
            .map_err(db_err)?;
        Ok(())
    }
}

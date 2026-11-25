use async_trait::async_trait;
use toasty::Db;
use uuid::Uuid;
use xmedia_core::domain::error::DomainResult;
use xmedia_core::domain::library::LibraryId;

use crate::domain::chapter::{
    Chapter, ChapterKind, ChapterSource, ChapterSourceKind, OnlineTrackedChapterMeta,
};
use crate::domain::id::{ChapterId, SeriesId};
use crate::domain::repository::ChapterRepository;

use super::super::models::ComicChapterModel;
use super::db_err;

#[derive(Clone)]
pub struct ToastyChapterRepository {
    db: Db,
}

impl ToastyChapterRepository {
    pub fn new(db: Db) -> Self {
        Self { db }
    }

    fn to_domain(m: ComicChapterModel) -> Chapter {
        let source = match ChapterSourceKind::parse(&m.source_kind) {
            ChapterSourceKind::OnlineTracked => {
                let mut meta =
                    m.online_meta
                        .map(|j| j.0)
                        .unwrap_or_else(|| OnlineTrackedChapterMeta {
                            source_id: m.online_source_id.unwrap_or_else(Uuid::nil),
                            comic_url: String::new(),
                            toc_url: String::new(),
                            chapter_url: m.online_chapter_url.unwrap_or_default(),
                            cached_archive_path: None,
                        });
                if m.cached_archive_path.is_some() {
                    meta.cached_archive_path = m.cached_archive_path;
                }
                ChapterSource::OnlineTracked(meta)
            }
            ChapterSourceKind::ImageDirectory => ChapterSource::ImageFolder {
                path: m.source_path,
            },
            ChapterSourceKind::ArchiveFile => ChapterSource::Archive {
                path: m.source_path,
            },
        };

        Chapter {
            id: ChapterId(m.id),
            series_id: SeriesId(m.series_id),
            library_id: LibraryId(m.library_id),
            kind: ChapterKind::parse(&m.kind),
            number: m.number,
            volume_number: m.volume_number,
            title: m.title,
            sort_key: m.sort_key,
            page_count: u32::try_from(m.page_count.max(0)).unwrap_or(0),
            source,
            file_size: u64::try_from(m.file_size.max(0)).unwrap_or(0),
            file_hash: m.file_hash,
            cover_path: m.cover_path,
            summary: m.summary,
            writers: m.writers.0,
            pencillers: m.pencillers.0,
            is_missing: m.is_missing,
            created_at: m.created_at,
            updated_at: m.updated_at,
        }
    }
}

#[async_trait]
impl ChapterRepository for ToastyChapterRepository {
    async fn find_by_id(&self, id: ChapterId) -> DomainResult<Option<Chapter>> {
        let mut db = self.db.clone();
        let row = ComicChapterModel::filter_by_id(id.0)
            .first()
            .exec(&mut db)
            .await
            .map_err(db_err)?;
        Ok(row.map(Self::to_domain))
    }

    async fn find_by_ids(&self, ids: &[ChapterId]) -> DomainResult<Vec<Chapter>> {
        if ids.is_empty() {
            return Ok(Vec::new());
        }
        let mut db = self.db.clone();
        let raw_ids: Vec<Uuid> = ids.iter().map(|i| i.0).collect();
        let rows = ComicChapterModel::filter(ComicChapterModel::fields().id().in_list(raw_ids))
            .exec(&mut db)
            .await
            .map_err(db_err)?;
        Ok(rows.into_iter().map(Self::to_domain).collect())
    }

    async fn find_chapters_by_cover_key(&self, cover_key: &str) -> DomainResult<Vec<Chapter>> {
        let clean = cover_key.trim();
        if clean.is_empty() {
            return Ok(Vec::new());
        }
        let mut db = self.db.clone();
        let rows = ComicChapterModel::filter_by_cover_path(clean.to_string())
            .exec(&mut db)
            .await
            .map_err(db_err)?;
        Ok(rows.into_iter().map(Self::to_domain).collect())
    }

    async fn list_by_series(&self, series_id: SeriesId) -> DomainResult<Vec<Chapter>> {
        let mut db = self.db.clone();
        let rows = ComicChapterModel::filter_by_series_id(series_id.0)
            .exec(&mut db)
            .await
            .map_err(db_err)?;
        Ok(rows.into_iter().map(Self::to_domain).collect())
    }

    async fn list_by_series_ids(&self, series_ids: &[SeriesId]) -> DomainResult<Vec<Chapter>> {
        if series_ids.is_empty() {
            return Ok(Vec::new());
        }
        let mut db = self.db.clone();
        let raw_ids: Vec<Uuid> = series_ids.iter().map(|s| s.0).collect();
        let rows =
            ComicChapterModel::filter(ComicChapterModel::fields().series_id().in_list(raw_ids))
                .exec(&mut db)
                .await
                .map_err(db_err)?;
        Ok(rows.into_iter().map(Self::to_domain).collect())
    }

    async fn list_by_library(&self, library_id: LibraryId) -> DomainResult<Vec<Chapter>> {
        let mut db = self.db.clone();
        let rows = ComicChapterModel::filter_by_library_id(library_id.0)
            .exec(&mut db)
            .await
            .map_err(db_err)?;
        Ok(rows.into_iter().map(Self::to_domain).collect())
    }

    async fn save(&self, c: &Chapter) -> DomainResult<()> {
        let mut db = self.db.clone();
        let existing = ComicChapterModel::filter_by_id(c.id.0)
            .first()
            .exec(&mut db)
            .await
            .map_err(db_err)?;

        let page_count = i64::from(c.page_count);
        let file_size = i64::try_from(c.file_size).unwrap_or(i64::MAX);
        let (
            source_kind,
            source_path,
            online_source_id,
            online_chapter_url,
            cached_archive_path,
            online_meta,
        ) = match &c.source {
            ChapterSource::Archive { path } => (
                ChapterSourceKind::ArchiveFile.as_str().to_string(),
                path.clone(),
                None,
                None,
                None,
                None,
            ),
            ChapterSource::ImageFolder { path } => (
                ChapterSourceKind::ImageDirectory.as_str().to_string(),
                path.clone(),
                None,
                None,
                None,
                None,
            ),
            ChapterSource::OnlineTracked(meta) => (
                ChapterSourceKind::OnlineTracked.as_str().to_string(),
                String::new(),
                Some(meta.source_id),
                Some(meta.chapter_url.clone()),
                meta.cached_archive_path.clone(),
                Some(toasty::Json(meta.clone())),
            ),
        };

        if let Some(mut record) = existing {
            record
                .update()
                .series_id(c.series_id.0)
                .library_id(c.library_id.0)
                .kind(c.kind.as_str().to_string())
                .number(c.number)
                .volume_number(c.volume_number)
                .title(c.title.clone())
                .sort_key(c.sort_key.clone())
                .page_count(page_count)
                .source_kind(source_kind)
                .source_path(source_path)
                .online_source_id(online_source_id)
                .online_chapter_url(online_chapter_url)
                .cached_archive_path(cached_archive_path)
                .online_meta(online_meta)
                .file_size(file_size)
                .file_hash(c.file_hash.clone())
                .cover_path(c.cover_path.clone())
                .summary(c.summary.clone())
                .writers(toasty::Json(c.writers.clone()))
                .pencillers(toasty::Json(c.pencillers.clone()))
                .is_missing(c.is_missing)
                .updated_at(c.updated_at)
                .exec(&mut db)
                .await
                .map_err(db_err)?;
        } else {
            toasty::create!(ComicChapterModel {
                id: c.id.0,
                series_id: c.series_id.0,
                library_id: c.library_id.0,
                kind: c.kind.as_str().to_string(),
                number: c.number,
                volume_number: c.volume_number,
                title: c.title.clone(),
                sort_key: c.sort_key.clone(),
                page_count: page_count,
                source_kind: source_kind,
                source_path: source_path,
                online_source_id: online_source_id,
                online_chapter_url: online_chapter_url,
                cached_archive_path: cached_archive_path,
                online_meta: online_meta,
                file_size: file_size,
                file_hash: c.file_hash.clone(),
                cover_path: c.cover_path.clone(),
                summary: c.summary.clone(),
                writers: toasty::Json(c.writers.clone()),
                pencillers: toasty::Json(c.pencillers.clone()),
                is_missing: c.is_missing,
                created_at: c.created_at,
                updated_at: c.updated_at,
            })
            .exec(&mut db)
            .await
            .map_err(db_err)?;
        }
        Ok(())
    }

    async fn delete(&self, id: ChapterId) -> DomainResult<()> {
        let mut db = self.db.clone();
        ComicChapterModel::filter_by_id(id.0)
            .delete()
            .exec(&mut db)
            .await
            .map_err(db_err)?;
        Ok(())
    }

    async fn delete_by_series(&self, series_id: SeriesId) -> DomainResult<()> {
        let mut db = self.db.clone();
        ComicChapterModel::filter_by_series_id(series_id.0)
            .delete()
            .exec(&mut db)
            .await
            .map_err(db_err)?;
        Ok(())
    }
}

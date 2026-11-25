pub(crate) mod models;
pub mod repositories;
mod source_repository;

use toasty::Db;
use toasty::schema::{Model, ModelSet};
use tracing::info;
use xmedia_core::domain::error::{DomainError, DomainResult};
use xmedia_core::infra::persistence::push_model_set;

use models::{
    ComicBookmarkModel, ComicChapterModel, ComicChapterProgressModel, ComicFavoriteSeriesModel,
    ComicSeriesModel, ComicSeriesReaderPrefModel, ComicShelfModel, ComicSourceRecordModel,
};

async fn ensure_comic_indexes(db: &mut Db) -> DomainResult<()> {
    for ddl in [
        "CREATE INDEX IF NOT EXISTS idx_comic_series_cover_path ON comic_series (cover_path)",
        "CREATE INDEX IF NOT EXISTS idx_comic_chapters_cover_path ON comic_chapters (cover_path)",
    ] {
        toasty::sql::statement(ddl)
            .exec(db)
            .await
            .map_err(|e| DomainError::Persistence(format!("failed to ensure comic index: {e}")))?;
    }
    Ok(())
}

pub fn register_comic_models(set: &mut ModelSet) {
    <ComicSeriesModel as Model>::register(set);
    <ComicChapterModel as Model>::register(set);
    <ComicChapterProgressModel as Model>::register(set);
    <ComicSeriesReaderPrefModel as Model>::register(set);
    <ComicFavoriteSeriesModel as Model>::register(set);
    <ComicShelfModel as Model>::register(set);
    <ComicBookmarkModel as Model>::register(set);
    <ComicSourceRecordModel as Model>::register(set);
}

pub async fn ensure_comic_schema(db: &mut Db, database_url: &str) -> DomainResult<()> {
    if ComicSeriesModel::all().limit(1).exec(db).await.is_err() {
        info!("initializing comic database schema via toasty");
        let mut set = ModelSet::new();
        register_comic_models(&mut set);
        push_model_set(database_url, "comic database", set).await?;
        if ComicSeriesModel::all().limit(1).exec(db).await.is_ok() {
            ensure_comic_indexes(db).await?;
        }
        return Ok(());
    }

    let mut missing = ModelSet::new();
    let mut labels = Vec::new();
    if ComicChapterModel::all().limit(1).exec(db).await.is_err() {
        <ComicChapterModel as Model>::register(&mut missing);
        labels.push("comic_chapters");
    }
    if ComicChapterProgressModel::all()
        .limit(1)
        .exec(db)
        .await
        .is_err()
    {
        <ComicChapterProgressModel as Model>::register(&mut missing);
        labels.push("comic_chapter_progresses");
    }
    if ComicSeriesReaderPrefModel::all()
        .limit(1)
        .exec(db)
        .await
        .is_err()
    {
        <ComicSeriesReaderPrefModel as Model>::register(&mut missing);
        labels.push("comic_series_reader_prefs");
    }
    if ComicFavoriteSeriesModel::all()
        .limit(1)
        .exec(db)
        .await
        .is_err()
    {
        <ComicFavoriteSeriesModel as Model>::register(&mut missing);
        labels.push("comic_favorite_series");
    }
    if ComicShelfModel::all().limit(1).exec(db).await.is_err() {
        <ComicShelfModel as Model>::register(&mut missing);
        labels.push("comic_shelves");
    }
    if ComicBookmarkModel::all().limit(1).exec(db).await.is_err() {
        <ComicBookmarkModel as Model>::register(&mut missing);
        labels.push("comic_bookmarks");
    }
    if ComicSourceRecordModel::all()
        .limit(1)
        .exec(db)
        .await
        .is_err()
    {
        <ComicSourceRecordModel as Model>::register(&mut missing);
        labels.push("comic_sources");
    }
    if !labels.is_empty() {
        let joined = labels.join(", ");
        info!("migrating comic tables ({joined}) via toasty");
        push_model_set(database_url, &joined, missing).await?;
    }
    ensure_comic_indexes(db).await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use tempfile::TempDir;
    use uuid::Uuid;

    use super::*;
    use crate::domain::repository::{ChapterRepository, SeriesRepository};
    use crate::infra::persistence::repositories::{
        ToastyChapterRepository, ToastySeriesRepository,
    };

    #[tokio::test]
    async fn ensure_comic_schema_creates_cover_indexes_and_lookup_queries_work() {
        let tmp = TempDir::new().unwrap();
        let db_url = format!("sqlite:{}", tmp.path().join("comic_idx.db").display());
        let mut set = ModelSet::new();
        register_comic_models(&mut set);
        let mut db = Db::builder().models(set).connect(&db_url).await.unwrap();
        ensure_comic_schema(&mut db, &db_url).await.unwrap();
        // Idempotent second run on existing schema
        ensure_comic_schema(&mut db, &db_url).await.unwrap();

        let rows = toasty::sql::query(
            "SELECT name FROM sqlite_master WHERE type = 'index' AND name IN ('idx_comic_series_cover_path', 'idx_comic_chapters_cover_path')",
        )
        .exec(&mut db)
        .await
        .unwrap();
        assert_eq!(rows.len(), 2);

        let now = jiff::Timestamp::now();
        let sid = Uuid::now_v7();
        let lib_id = Uuid::now_v7();
        toasty::create!(ComicSeriesModel {
            id: sid,
            library_id: lib_id,
            title: "Indexed Comic",
            sort_title: "Indexed Comic",
            alt_titles: toasty::Json(vec![]),
            summary: "",
            writers: toasty::Json(vec![]),
            pencillers: toasty::Json(vec![]),
            publisher: None,
            status: "ongoing",
            reading_direction: "rtl",
            age_rating: None,
            genres: toasty::Json(vec![]),
            tags: toasty::Json(vec![]),
            language: None,
            year: None,
            cover_path: Some("cov_abc.webp".into()),
            chapter_count: 1,
            source_dir: "/comics/indexed",
            metadata_locked: false,
            created_at: now,
            updated_at: now,
        })
        .exec(&mut db)
        .await
        .unwrap();

        let s_repo = ToastySeriesRepository::new(db.clone());
        let c_repo = ToastyChapterRepository::new(db);
        let found = s_repo
            .find_series_by_cover_key("cov_abc.webp")
            .await
            .unwrap();
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].id.0, sid);
        assert!(
            s_repo
                .find_series_by_cover_key("other.webp")
                .await
                .unwrap()
                .is_empty()
        );
        assert!(
            c_repo
                .find_chapters_by_cover_key("cov_abc.webp")
                .await
                .unwrap()
                .is_empty()
        );
    }
}

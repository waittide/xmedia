pub(crate) mod models;
pub mod repositories;
mod source_repository;

use toasty::Db;
use toasty::schema::{Model, ModelSet};
use tracing::info;
use xmedia_core::domain::error::{DomainError, DomainResult};

use models::{
    ComicBookmarkModel, ComicChapterModel, ComicChapterProgressModel, ComicFavoriteSeriesModel,
    ComicSeriesModel, ComicSeriesReaderPrefModel, ComicShelfModel, ComicSourceRecordModel,
};

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

async fn push_single_comic_model<M: Model>(
    database_url: &str,
    table_label: &str,
) -> DomainResult<()> {
    info!("migrating {table_label} table via toasty");
    let mut set = ModelSet::new();
    M::register(&mut set);
    let bootstrap = Db::builder()
        .models(set)
        .connect(database_url)
        .await
        .map_err(|e| DomainError::Persistence(format!("failed to connect comic db: {e}")))?;
    bootstrap.push_schema().await.map_err(|e| {
        DomainError::Persistence(format!("failed to push {table_label} schema: {e}"))
    })?;
    Ok(())
}

pub async fn ensure_comic_schema(db: &mut Db, database_url: &str) -> DomainResult<()> {
    if ComicSeriesModel::all().limit(1).exec(db).await.is_err() {
        info!("initializing comic database schema via toasty");
        let mut set = ModelSet::new();
        register_comic_models(&mut set);
        let bootstrap = Db::builder()
            .models(set)
            .connect(database_url)
            .await
            .map_err(|e| DomainError::Persistence(format!("failed to connect comic db: {e}")))?;
        bootstrap.push_schema().await.map_err(|e| {
            DomainError::Persistence(format!("failed to push comic database schema: {e}"))
        })?;
        return Ok(());
    }

    if ComicChapterModel::all().limit(1).exec(db).await.is_err() {
        push_single_comic_model::<ComicChapterModel>(database_url, "comic_chapters").await?;
    }
    if ComicChapterProgressModel::all()
        .limit(1)
        .exec(db)
        .await
        .is_err()
    {
        push_single_comic_model::<ComicChapterProgressModel>(
            database_url,
            "comic_chapter_progress",
        )
        .await?;
    }
    if ComicSeriesReaderPrefModel::all()
        .limit(1)
        .exec(db)
        .await
        .is_err()
    {
        push_single_comic_model::<ComicSeriesReaderPrefModel>(
            database_url,
            "comic_series_reader_prefs",
        )
        .await?;
    }
    if ComicFavoriteSeriesModel::all()
        .limit(1)
        .exec(db)
        .await
        .is_err()
    {
        push_single_comic_model::<ComicFavoriteSeriesModel>(database_url, "comic_favorite_series")
            .await?;
    }
    if ComicShelfModel::all().limit(1).exec(db).await.is_err() {
        push_single_comic_model::<ComicShelfModel>(database_url, "comic_shelves").await?;
    }
    if ComicBookmarkModel::all().limit(1).exec(db).await.is_err() {
        push_single_comic_model::<ComicBookmarkModel>(database_url, "comic_bookmarks").await?;
    }
    if ComicSourceRecordModel::all()
        .limit(1)
        .exec(db)
        .await
        .is_err()
    {
        push_single_comic_model::<ComicSourceRecordModel>(database_url, "comic_sources").await?;
    }
    Ok(())
}

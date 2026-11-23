pub(crate) mod models;
pub mod repositories;
mod source_repository;

use toasty::Db;
use toasty::schema::{Model, ModelSet};
use tracing::info;
use xmedia_core::domain::error::DomainResult;
use xmedia_core::infra::persistence::push_model_set;

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

pub async fn ensure_comic_schema(db: &mut Db, database_url: &str) -> DomainResult<()> {
    if ComicSeriesModel::all().limit(1).exec(db).await.is_err() {
        info!("initializing comic database schema via toasty");
        let mut set = ModelSet::new();
        register_comic_models(&mut set);
        return push_model_set(database_url, "comic database", set).await;
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
    Ok(())
}

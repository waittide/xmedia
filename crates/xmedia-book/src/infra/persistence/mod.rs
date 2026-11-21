pub(crate) mod models;
pub mod repositories;

use toasty::Db;
use toasty::schema::{Model, ModelSet};
use tracing::info;
use xmedia_core::domain::error::{DomainError, DomainResult};

use models::{
    BookBookmarkModel, BookFavoriteModel, BookModel, BookOnlineShelfModel,
    BookReadingProgressModel, BookShelfModel, BookSourceRecordModel,
};

pub fn register_book_models(set: &mut ModelSet) {
    <BookModel as Model>::register(set);
    <BookReadingProgressModel as Model>::register(set);
    <BookBookmarkModel as Model>::register(set);
    <BookFavoriteModel as Model>::register(set);
    <BookShelfModel as Model>::register(set);
    <BookSourceRecordModel as Model>::register(set);
    <BookOnlineShelfModel as Model>::register(set);
}

async fn push_single_model<M: Model>(database_url: &str, table_label: &str) -> DomainResult<()> {
    info!("migrating {table_label} table via toasty");
    let mut set = ModelSet::new();
    M::register(&mut set);
    let bootstrap = Db::builder()
        .models(set)
        .connect(database_url)
        .await
        .map_err(|e| DomainError::Persistence(format!("failed to connect book db: {e}")))?;
    bootstrap.push_schema().await.map_err(|e| {
        DomainError::Persistence(format!("failed to push {table_label} schema: {e}"))
    })?;
    Ok(())
}

pub async fn ensure_book_schema(db: &mut Db, database_url: &str) -> DomainResult<()> {
    if BookModel::all().limit(1).exec(db).await.is_err() {
        info!("initializing book database schema via toasty");
        let mut set = ModelSet::new();
        register_book_models(&mut set);
        let bootstrap = Db::builder()
            .models(set)
            .connect(database_url)
            .await
            .map_err(|e| DomainError::Persistence(format!("failed to connect book db: {e}")))?;
        bootstrap.push_schema().await.map_err(|e| {
            DomainError::Persistence(format!("failed to push book database schema: {e}"))
        })?;
        return Ok(());
    }

    if BookReadingProgressModel::all()
        .limit(1)
        .exec(db)
        .await
        .is_err()
    {
        push_single_model::<BookReadingProgressModel>(database_url, "book_reading_progress")
            .await?;
    }
    if BookBookmarkModel::all().limit(1).exec(db).await.is_err() {
        push_single_model::<BookBookmarkModel>(database_url, "book_bookmarks").await?;
    }
    if BookFavoriteModel::all().limit(1).exec(db).await.is_err() {
        push_single_model::<BookFavoriteModel>(database_url, "book_favorites").await?;
    }
    if BookShelfModel::all().limit(1).exec(db).await.is_err() {
        push_single_model::<BookShelfModel>(database_url, "book_shelves").await?;
    }
    if BookSourceRecordModel::all()
        .limit(1)
        .exec(db)
        .await
        .is_err()
    {
        push_single_model::<BookSourceRecordModel>(database_url, "book_sources").await?;
    }
    if BookOnlineShelfModel::all().limit(1).exec(db).await.is_err() {
        push_single_model::<BookOnlineShelfModel>(database_url, "book_online_books").await?;
    }
    Ok(())
}

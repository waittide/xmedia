pub(crate) mod models;
pub mod repositories;

use toasty::Db;
use toasty::schema::{Model, ModelSet};
use tracing::info;
use xmedia_core::domain::error::DomainResult;
use xmedia_core::infra::persistence::push_model_set;

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

pub async fn ensure_book_schema(db: &mut Db, database_url: &str) -> DomainResult<()> {
    if BookModel::all().limit(1).exec(db).await.is_err() {
        info!("initializing book database schema via toasty");
        let mut set = ModelSet::new();
        register_book_models(&mut set);
        return push_model_set(database_url, "book database", set).await;
    }

    let mut missing = ModelSet::new();
    let mut labels = Vec::new();
    if BookReadingProgressModel::all()
        .limit(1)
        .exec(db)
        .await
        .is_err()
    {
        <BookReadingProgressModel as Model>::register(&mut missing);
        labels.push("book_reading_progress");
    }
    if BookBookmarkModel::all().limit(1).exec(db).await.is_err() {
        <BookBookmarkModel as Model>::register(&mut missing);
        labels.push("book_bookmarks");
    }
    if BookFavoriteModel::all().limit(1).exec(db).await.is_err() {
        <BookFavoriteModel as Model>::register(&mut missing);
        labels.push("book_favorites");
    }
    if BookShelfModel::all().limit(1).exec(db).await.is_err() {
        <BookShelfModel as Model>::register(&mut missing);
        labels.push("book_shelves");
    }
    if BookSourceRecordModel::all()
        .limit(1)
        .exec(db)
        .await
        .is_err()
    {
        <BookSourceRecordModel as Model>::register(&mut missing);
        labels.push("book_sources");
    }
    if BookOnlineShelfModel::all().limit(1).exec(db).await.is_err() {
        <BookOnlineShelfModel as Model>::register(&mut missing);
        labels.push("book_online_books");
    }
    if !labels.is_empty() {
        let joined = labels.join(", ");
        info!("migrating book tables ({joined}) via toasty");
        push_model_set(database_url, &joined, missing).await?;
    }
    Ok(())
}

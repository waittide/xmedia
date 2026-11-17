pub(crate) mod models;
pub mod repositories;

use toasty::Db;
use toasty::schema::{Model, ModelSet};
use tracing::info;
use xmedia_core::domain::error::{DomainError, DomainResult};

use models::{
    BookBookmarkModel, BookFavoriteModel, BookModel, BookReadingProgressModel, BookShelfModel,
};

pub fn register_book_models(set: &mut ModelSet) {
    <BookModel as Model>::register(set);
    <BookReadingProgressModel as Model>::register(set);
    <BookBookmarkModel as Model>::register(set);
    <BookFavoriteModel as Model>::register(set);
    <BookShelfModel as Model>::register(set);
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
    }
    Ok(())
}

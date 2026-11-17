pub(crate) mod models;
pub mod repositories;

use toasty::Db;
use toasty::schema::{Model, ModelSet};
use tracing::info;
use xmedia_core::domain::error::{DomainError, DomainResult};

use models::{
    ComicChapterModel, ComicChapterProgressModel, ComicFavoriteSeriesModel, ComicSeriesModel,
    ComicSeriesReaderPrefModel, ComicShelfModel,
};

pub fn register_comic_models(set: &mut ModelSet) {
    <ComicSeriesModel as Model>::register(set);
    <ComicChapterModel as Model>::register(set);
    <ComicChapterProgressModel as Model>::register(set);
    <ComicSeriesReaderPrefModel as Model>::register(set);
    <ComicFavoriteSeriesModel as Model>::register(set);
    <ComicShelfModel as Model>::register(set);
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
    }
    Ok(())
}

pub(crate) mod models;
pub mod repositories;

use toasty::Db;
use toasty::schema::{Model, ModelSet};
use tracing::info;
use xmedia_core::domain::error::{DomainError, DomainResult};

use models::{
    VideoFavoriteModel, VideoItemModel, VideoProgressModel, VideoShelfModel, VideoShowModel,
};

pub fn register_video_models(set: &mut ModelSet) {
    <VideoShowModel as Model>::register(set);
    <VideoItemModel as Model>::register(set);
    <VideoProgressModel as Model>::register(set);
    <VideoFavoriteModel as Model>::register(set);
    <VideoShelfModel as Model>::register(set);
}

async fn push_single_video_model<M: Model>(
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
        .map_err(|e| DomainError::Persistence(format!("failed to connect video db: {e}")))?;
    bootstrap.push_schema().await.map_err(|e| {
        DomainError::Persistence(format!("failed to push {table_label} schema: {e}"))
    })?;
    Ok(())
}

pub async fn ensure_video_schema(db: &mut Db, database_url: &str) -> DomainResult<()> {
    if VideoItemModel::all().limit(1).exec(db).await.is_err() {
        info!("initializing video database schema via toasty");
        let mut set = ModelSet::new();
        register_video_models(&mut set);
        let bootstrap = Db::builder()
            .models(set)
            .connect(database_url)
            .await
            .map_err(|e| DomainError::Persistence(format!("failed to connect video db: {e}")))?;
        bootstrap.push_schema().await.map_err(|e| {
            DomainError::Persistence(format!("failed to push video database schema: {e}"))
        })?;
        return Ok(());
    }

    if VideoShowModel::all().limit(1).exec(db).await.is_err() {
        push_single_video_model::<VideoShowModel>(database_url, "video_shows").await?;
    }
    if VideoProgressModel::all().limit(1).exec(db).await.is_err() {
        push_single_video_model::<VideoProgressModel>(database_url, "video_progress").await?;
    }
    if VideoFavoriteModel::all().limit(1).exec(db).await.is_err() {
        push_single_video_model::<VideoFavoriteModel>(database_url, "video_favorites").await?;
    }
    if VideoShelfModel::all().limit(1).exec(db).await.is_err() {
        push_single_video_model::<VideoShelfModel>(database_url, "video_shelves").await?;
    }
    Ok(())
}

pub(crate) mod models;
pub mod repositories;

use toasty::Db;
use toasty::schema::{Model, ModelSet};
use tracing::info;
use xmedia_core::domain::error::DomainResult;
use xmedia_core::infra::persistence::push_model_set;

use models::{
    VideoFavoriteModel, VideoItemModel, VideoProgressModel, VideoShelfModel, VideoShowModel,
    VideoSourceRecordModel,
};

pub fn register_video_models(set: &mut ModelSet) {
    <VideoShowModel as Model>::register(set);
    <VideoItemModel as Model>::register(set);
    <VideoProgressModel as Model>::register(set);
    <VideoFavoriteModel as Model>::register(set);
    <VideoShelfModel as Model>::register(set);
    <VideoSourceRecordModel as Model>::register(set);
}

pub async fn ensure_video_schema(db: &mut Db, database_url: &str) -> DomainResult<()> {
    if VideoItemModel::all().limit(1).exec(db).await.is_err() {
        info!("initializing video database schema via toasty");
        let mut set = ModelSet::new();
        register_video_models(&mut set);
        return push_model_set(database_url, "video database", set).await;
    }

    let mut missing = ModelSet::new();
    let mut labels = Vec::new();
    if VideoShowModel::all().limit(1).exec(db).await.is_err() {
        <VideoShowModel as Model>::register(&mut missing);
        labels.push("video_shows");
    }
    if VideoProgressModel::all().limit(1).exec(db).await.is_err() {
        <VideoProgressModel as Model>::register(&mut missing);
        labels.push("video_progress");
    }
    if VideoFavoriteModel::all().limit(1).exec(db).await.is_err() {
        <VideoFavoriteModel as Model>::register(&mut missing);
        labels.push("video_favorites");
    }
    if VideoShelfModel::all().limit(1).exec(db).await.is_err() {
        <VideoShelfModel as Model>::register(&mut missing);
        labels.push("video_shelves");
    }
    if VideoSourceRecordModel::all()
        .limit(1)
        .exec(db)
        .await
        .is_err()
    {
        <VideoSourceRecordModel as Model>::register(&mut missing);
        labels.push("video_sources");
    }
    if !labels.is_empty() {
        let joined = labels.join(", ");
        info!("migrating video tables ({joined}) via toasty");
        push_model_set(database_url, &joined, missing).await?;
    }
    Ok(())
}

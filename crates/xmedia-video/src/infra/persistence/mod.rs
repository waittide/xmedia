pub(crate) mod models;
pub mod repositories;

use toasty::Db;
use toasty::schema::{Model, ModelSet};
use tracing::info;
use xmedia_core::domain::error::DomainResult;
use xmedia_core::infra::persistence::{ensure_sql_index, push_model_set};

use models::{
    VideoFavoriteModel, VideoItemModel, VideoProgressModel, VideoShelfModel, VideoShowModel,
    VideoSourceRecordModel,
};

pub async fn ensure_video_indexes(db: &mut Db) -> DomainResult<()> {
    for (idx, spec) in [
        ("idx_video_items_poster_key", "video_items (poster_key)"),
        ("idx_video_shows_poster_key", "video_shows (poster_key)"),
        (
            "idx_video_shows_online_tracked",
            "video_shows (online_source_id, online_item_id)",
        ),
    ] {
        ensure_sql_index(db, idx, spec).await?;
    }
    Ok(())
}

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
        push_model_set(database_url, "video database", set).await?;
        if VideoItemModel::all().limit(1).exec(db).await.is_ok() {
            ensure_video_indexes(db).await?;
        }
        return Ok(());
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
    ensure_video_indexes(db).await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use tempfile::TempDir;

    use super::*;
    use crate::domain::repository::{ShowScope, VideoCatalogRepository, VideoScope};
    use crate::infra::persistence::repositories::ToastyVideoCatalogRepository;

    #[tokio::test]
    async fn ensure_video_schema_creates_poster_indexes_and_lookup_queries_work() {
        let tmp = TempDir::new().unwrap();
        let db_url = format!("sqlite:{}", tmp.path().join("video_idx.db").display());
        let mut set = ModelSet::new();
        register_video_models(&mut set);
        let mut db = Db::builder().models(set).connect(&db_url).await.unwrap();
        ensure_video_schema(&mut db, &db_url).await.unwrap();
        ensure_video_schema(&mut db, &db_url).await.unwrap();

        let rows = toasty::sql::query(
            "SELECT name FROM sqlite_master WHERE type = 'index' AND name IN ('idx_video_items_poster_key', 'idx_video_shows_poster_key')",
        )
        .exec(&mut db)
        .await
        .unwrap();
        assert_eq!(rows.len(), 2);

        let repo = ToastyVideoCatalogRepository::new(db);
        assert!(
            repo.query_videos(VideoScope::PosterKey("missing.jpg"))
                .await
                .unwrap()
                .is_empty()
        );
        assert!(
            repo.query_shows(ShowScope::PosterKey("missing.jpg"))
                .await
                .unwrap()
                .is_empty()
        );
    }
}

pub(crate) mod models;
pub mod repositories;

use toasty::Db;
use toasty::schema::{Model, ModelSet};
use tracing::info;
use xmedia_core::domain::error::{DomainError, DomainResult};

use models::{
    MusicAlbumModel, MusicArtistModel, MusicBookmarkModel, MusicPlayQueueModel, MusicPlayStatModel,
    MusicPlaylistModel, MusicRatingModel, MusicScrobbleModel, MusicSourceRecordModel,
    MusicStarredModel, MusicTrackModel, MusicTrackTagModel,
};

pub fn register_music_models(set: &mut ModelSet) {
    <MusicArtistModel as Model>::register(set);
    <MusicAlbumModel as Model>::register(set);
    <MusicTrackModel as Model>::register(set);
    <MusicPlaylistModel as Model>::register(set);
    <MusicStarredModel as Model>::register(set);
    <MusicRatingModel as Model>::register(set);
    <MusicPlayStatModel as Model>::register(set);
    <MusicScrobbleModel as Model>::register(set);
    <MusicPlayQueueModel as Model>::register(set);
    <MusicBookmarkModel as Model>::register(set);
    <MusicTrackTagModel as Model>::register(set);
    <MusicSourceRecordModel as Model>::register(set);
}

async fn push_single_music_model<M: Model>(
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
        .map_err(|e| DomainError::Persistence(format!("failed to connect music db: {e}")))?;
    bootstrap.push_schema().await.map_err(|e| {
        DomainError::Persistence(format!("failed to push {table_label} schema: {e}"))
    })?;
    Ok(())
}

pub async fn ensure_music_schema(db: &mut Db, database_url: &str) -> DomainResult<()> {
    if MusicTrackModel::all().limit(1).exec(db).await.is_err() {
        info!("initializing music database schema via toasty");
        let mut set = ModelSet::new();
        register_music_models(&mut set);
        let bootstrap = Db::builder()
            .models(set)
            .connect(database_url)
            .await
            .map_err(|e| DomainError::Persistence(format!("failed to connect music db: {e}")))?;
        bootstrap.push_schema().await.map_err(|e| {
            DomainError::Persistence(format!("failed to push music database schema: {e}"))
        })?;
        return Ok(());
    }

    if MusicArtistModel::all().limit(1).exec(db).await.is_err() {
        push_single_music_model::<MusicArtistModel>(database_url, "music_artists").await?;
    }
    if MusicAlbumModel::all().limit(1).exec(db).await.is_err() {
        push_single_music_model::<MusicAlbumModel>(database_url, "music_albums").await?;
    }
    if MusicPlaylistModel::all().limit(1).exec(db).await.is_err() {
        push_single_music_model::<MusicPlaylistModel>(database_url, "music_playlists").await?;
    }
    if MusicStarredModel::all().limit(1).exec(db).await.is_err() {
        push_single_music_model::<MusicStarredModel>(database_url, "music_starred").await?;
    }
    if MusicRatingModel::all().limit(1).exec(db).await.is_err() {
        push_single_music_model::<MusicRatingModel>(database_url, "music_ratings").await?;
    }
    if MusicPlayStatModel::all().limit(1).exec(db).await.is_err() {
        push_single_music_model::<MusicPlayStatModel>(database_url, "music_play_stats").await?;
    }
    if MusicScrobbleModel::all().limit(1).exec(db).await.is_err() {
        push_single_music_model::<MusicScrobbleModel>(database_url, "music_scrobbles").await?;
    }
    if MusicPlayQueueModel::all().limit(1).exec(db).await.is_err() {
        push_single_music_model::<MusicPlayQueueModel>(database_url, "music_play_queues").await?;
    }
    if MusicBookmarkModel::all().limit(1).exec(db).await.is_err() {
        push_single_music_model::<MusicBookmarkModel>(database_url, "music_bookmarks").await?;
    }
    if MusicTrackTagModel::all().limit(1).exec(db).await.is_err() {
        push_single_music_model::<MusicTrackTagModel>(database_url, "music_track_tags").await?;
    }
    if MusicSourceRecordModel::all()
        .limit(1)
        .exec(db)
        .await
        .is_err()
    {
        push_single_music_model::<MusicSourceRecordModel>(database_url, "music_sources").await?;
    }
    Ok(())
}

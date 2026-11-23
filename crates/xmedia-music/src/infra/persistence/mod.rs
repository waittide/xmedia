pub(crate) mod models;
pub mod repositories;

use toasty::Db;
use toasty::schema::{Model, ModelSet};
use tracing::info;
use xmedia_core::domain::error::DomainResult;
use xmedia_core::infra::persistence::push_model_set;

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

pub async fn ensure_music_schema(db: &mut Db, database_url: &str) -> DomainResult<()> {
    if MusicTrackModel::all().limit(1).exec(db).await.is_err() {
        info!("initializing music database schema via toasty");
        let mut set = ModelSet::new();
        register_music_models(&mut set);
        return push_model_set(database_url, "music database", set).await;
    }

    let mut missing = ModelSet::new();
    let mut labels = Vec::new();
    if MusicArtistModel::all().limit(1).exec(db).await.is_err() {
        <MusicArtistModel as Model>::register(&mut missing);
        labels.push("music_artists");
    }
    if MusicAlbumModel::all().limit(1).exec(db).await.is_err() {
        <MusicAlbumModel as Model>::register(&mut missing);
        labels.push("music_albums");
    }
    if MusicPlaylistModel::all().limit(1).exec(db).await.is_err() {
        <MusicPlaylistModel as Model>::register(&mut missing);
        labels.push("music_playlists");
    }
    if MusicStarredModel::all().limit(1).exec(db).await.is_err() {
        <MusicStarredModel as Model>::register(&mut missing);
        labels.push("music_starred");
    }
    if MusicRatingModel::all().limit(1).exec(db).await.is_err() {
        <MusicRatingModel as Model>::register(&mut missing);
        labels.push("music_ratings");
    }
    if MusicPlayStatModel::all().limit(1).exec(db).await.is_err() {
        <MusicPlayStatModel as Model>::register(&mut missing);
        labels.push("music_play_stats");
    }
    if MusicScrobbleModel::all().limit(1).exec(db).await.is_err() {
        <MusicScrobbleModel as Model>::register(&mut missing);
        labels.push("music_scrobbles");
    }
    if MusicPlayQueueModel::all().limit(1).exec(db).await.is_err() {
        <MusicPlayQueueModel as Model>::register(&mut missing);
        labels.push("music_play_queues");
    }
    if MusicBookmarkModel::all().limit(1).exec(db).await.is_err() {
        <MusicBookmarkModel as Model>::register(&mut missing);
        labels.push("music_bookmarks");
    }
    if MusicTrackTagModel::all().limit(1).exec(db).await.is_err() {
        <MusicTrackTagModel as Model>::register(&mut missing);
        labels.push("music_track_tags");
    }
    if MusicSourceRecordModel::all()
        .limit(1)
        .exec(db)
        .await
        .is_err()
    {
        <MusicSourceRecordModel as Model>::register(&mut missing);
        labels.push("music_sources");
    }
    if !labels.is_empty() {
        let joined = labels.join(", ");
        info!("migrating music tables ({joined}) via toasty");
        push_model_set(database_url, &joined, missing).await?;
    }
    Ok(())
}

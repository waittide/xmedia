pub(crate) mod models;
pub mod repositories;

use toasty::Db;
use toasty::schema::{Model, ModelSet};
use tracing::info;
use xmedia_core::domain::error::{DomainError, DomainResult};

use models::{
    MusicAlbumModel, MusicArtistModel, MusicBookmarkModel, MusicPlayQueueModel, MusicPlayStatModel,
    MusicPlaylistModel, MusicRatingModel, MusicScrobbleModel, MusicStarredModel, MusicTrackModel,
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
    }
    Ok(())
}

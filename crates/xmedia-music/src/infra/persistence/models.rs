use jiff::Timestamp;
use uuid::Uuid;

use crate::domain::catalog::{ReplayGain, TrackId, TrackSource};

#[derive(Debug, toasty::Model)]
#[table = "music_artists"]
pub(crate) struct MusicArtistModel {
    #[key]
    pub id: Uuid,

    pub name: String,

    #[unique]
    pub name_lower: String,

    pub is_album_artist: bool,
    pub is_track_artist: bool,
    pub album_count: i64,
    pub track_count: i64,
    pub cover_key: Option<String>,
    pub created_at: Timestamp,
    pub updated_at: Timestamp,
}

#[derive(Debug, toasty::Model)]
#[table = "music_albums"]
pub(crate) struct MusicAlbumModel {
    #[key]
    pub id: Uuid,

    #[index]
    pub library_id: Uuid,

    #[unique]
    pub album_key: String,

    pub title: String,
    pub title_lower: String,

    #[index]
    pub album_artist_id: Uuid,

    pub album_artist_name: String,
    pub year: Option<i64>,
    pub genre: Option<String>,
    pub disc_count: i64,
    pub track_count: i64,
    pub duration_secs: i64,
    pub is_compilation: bool,
    pub cover_key: Option<String>,
    pub created_at: Timestamp,
    pub updated_at: Timestamp,
}

#[derive(Debug, toasty::Model)]
#[table = "music_tracks"]
pub(crate) struct MusicTrackModel {
    #[key]
    pub id: Uuid,

    #[index]
    pub library_id: Uuid,

    #[index]
    pub album_id: Uuid,

    #[index]
    pub artist_id: Uuid,

    #[index]
    pub album_artist_id: Uuid,

    pub title: String,
    pub title_lower: String,
    pub artist_name: String,
    pub album_artist_name: String,
    pub album_title: String,

    pub track_number: Option<i64>,
    pub disc_number: Option<i64>,
    pub year: Option<i64>,
    pub genre: Option<String>,
    pub duration_secs: i64,
    pub bit_rate_kbps: Option<i64>,
    pub sample_rate_hz: Option<i64>,
    pub bit_depth: Option<i64>,
    pub channels: Option<i64>,

    #[index]
    pub format: String,

    pub file_size: i64,
    pub file_hash: String,

    #[column(type = text)]
    pub replay_gain: toasty::Json<ReplayGain>,

    pub has_embedded_lyrics: bool,
    pub has_sidecar_lyrics: bool,
    pub is_compilation: bool,
    pub cover_key: Option<String>,

    #[column(type = text)]
    pub source: toasty::Json<TrackSource>,

    #[index]
    pub is_missing: bool,

    pub created_at: Timestamp,
    pub updated_at: Timestamp,
}

#[derive(Debug, toasty::Model)]
#[table = "music_playlists"]
pub(crate) struct MusicPlaylistModel {
    #[key]
    pub id: Uuid,

    #[index]
    pub owner_id: Uuid,

    pub owner_username: String,
    pub name: String,
    pub comment: Option<String>,

    #[index]
    pub is_public: bool,

    #[column(type = text)]
    pub track_ids: toasty::Json<Vec<TrackId>>,

    pub created_at: Timestamp,
    pub updated_at: Timestamp,
}

#[derive(Debug, toasty::Model)]
#[table = "music_starred_items"]
pub(crate) struct MusicStarredModel {
    #[key]
    pub star_key: String,

    #[index]
    pub user_id: Uuid,

    pub kind: String,

    #[index]
    pub target_id: Uuid,

    pub starred_at: Timestamp,
}

#[derive(Debug, toasty::Model)]
#[table = "music_ratings"]
pub(crate) struct MusicRatingModel {
    #[key]
    pub rating_key: String,

    #[index]
    pub user_id: Uuid,

    pub kind: String,

    #[index]
    pub target_id: Uuid,

    pub rating: i64,
    pub updated_at: Timestamp,
}

#[derive(Debug, toasty::Model)]
#[table = "music_play_stats"]
pub(crate) struct MusicPlayStatModel {
    #[key]
    pub stat_key: String,

    #[index]
    pub user_id: Uuid,

    #[index]
    pub track_id: Uuid,

    #[index]
    pub album_id: Uuid,

    pub play_count: i64,
    pub last_played_at: Timestamp,
}

#[derive(Debug, toasty::Model)]
#[table = "music_scrobbles"]
pub(crate) struct MusicScrobbleModel {
    #[key]
    pub id: Uuid,

    #[index]
    pub user_id: Uuid,

    #[index]
    pub track_id: Uuid,

    pub played_at: Timestamp,
}

#[derive(Debug, toasty::Model)]
#[table = "music_play_queues"]
pub(crate) struct MusicPlayQueueModel {
    #[key]
    pub user_id: Uuid,

    #[column(type = text)]
    pub track_ids: toasty::Json<Vec<TrackId>>,

    pub current_track_id: Option<Uuid>,
    pub position_ms: i64,
    pub changed_by: String,
    pub updated_at: Timestamp,
}

#[derive(Debug, toasty::Model)]
#[table = "music_bookmarks"]
pub(crate) struct MusicBookmarkModel {
    #[key]
    pub bookmark_key: String,

    #[index]
    pub user_id: Uuid,

    #[index]
    pub track_id: Uuid,

    pub position_ms: i64,
    pub comment: Option<String>,
    pub created_at: Timestamp,
    pub updated_at: Timestamp,
}

#[derive(Debug, toasty::Model)]
#[table = "music_track_tags"]
pub(crate) struct MusicTrackTagModel {
    #[key]
    pub composite_key: String,

    #[index]
    pub track_id: Uuid,

    #[index]
    pub tag_lower: String,

    pub tag: String,
    pub created_at: Timestamp,
}

use jiff::Timestamp;
use uuid::Uuid;
use xmedia_core::infra::ffmpeg::{ProbedAudioStream, ProbedSubtitleStream};

use crate::domain::id::VideoId;
use crate::domain::video::VideoSource;

#[derive(Debug, toasty::Model)]
#[table = "video_shows"]
pub(crate) struct VideoShowModel {
    #[key]
    pub id: Uuid,

    #[index]
    pub library_id: Uuid,

    #[unique]
    pub show_key: String,

    pub title: String,
    pub summary: String,
    pub year: Option<i64>,

    #[column(type = text)]
    pub genres: toasty::Json<Vec<String>>,

    #[column(type = text)]
    pub tags: toasty::Json<Vec<String>>,

    pub poster_key: Option<String>,
    pub season_count: i64,
    pub episode_count: i64,
    pub created_at: Timestamp,
    pub updated_at: Timestamp,
}

#[derive(Debug, toasty::Model)]
#[table = "video_items"]
pub(crate) struct VideoItemModel {
    #[key]
    pub id: Uuid,

    #[index]
    pub library_id: Uuid,

    #[index]
    pub show_id: Option<Uuid>,

    pub show_title: Option<String>,
    pub season_number: Option<i64>,
    pub episode_number: Option<i64>,

    pub title: String,
    pub summary: String,
    pub year: Option<i64>,

    #[column(type = text)]
    pub genres: toasty::Json<Vec<String>>,

    #[column(type = text)]
    pub tags: toasty::Json<Vec<String>>,

    pub container: String,
    pub file_size: i64,
    pub file_hash: String,
    pub duration_secs: i64,
    pub bit_rate_kbps: Option<i64>,
    pub width: Option<i64>,
    pub height: Option<i64>,
    pub video_codec: Option<String>,

    #[column(type = text)]
    pub audio_streams: toasty::Json<Vec<ProbedAudioStream>>,

    #[column(type = text)]
    pub subtitle_streams: toasty::Json<Vec<ProbedSubtitleStream>>,

    pub poster_key: Option<String>,

    #[column(type = text)]
    pub source: toasty::Json<VideoSource>,

    pub is_missing: bool,
    pub created_at: Timestamp,
    pub updated_at: Timestamp,
}

#[derive(Debug, toasty::Model)]
#[table = "video_watch_progress"]
pub(crate) struct VideoProgressModel {
    #[key]
    pub progress_key: String,

    #[index]
    pub user_id: Uuid,

    #[index]
    pub video_id: Uuid,

    pub position_ms: i64,
    pub duration_secs: i64,
    pub completed: bool,
    pub selected_audio_stream: Option<i64>,
    pub selected_subtitle: Option<String>,
    pub updated_at: Timestamp,
}

#[derive(Debug, toasty::Model)]
#[table = "video_favorites"]
pub(crate) struct VideoFavoriteModel {
    #[key]
    pub fav_key: String,

    #[index]
    pub user_id: Uuid,

    pub kind: String,
    pub target_id: Uuid,
    pub created_at: Timestamp,
}

#[derive(Debug, toasty::Model)]
#[table = "video_shelves"]
pub(crate) struct VideoShelfModel {
    #[key]
    pub id: Uuid,

    #[index]
    pub user_id: Uuid,

    pub name: String,
    pub description: String,

    #[column(type = text)]
    pub video_ids: toasty::Json<Vec<VideoId>>,

    pub created_at: Timestamp,
    pub updated_at: Timestamp,
}

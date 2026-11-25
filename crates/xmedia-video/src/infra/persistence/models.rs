use jiff::Timestamp;
use uuid::Uuid;
use xmedia_core::domain::media::{ProbedAudioStream, ProbedSubtitleStream};

use crate::domain::id::VideoId;
use crate::domain::video::VideoSource;
use crate::source::model::SourceRuntimeState;

#[derive(Debug, toasty::Model)]
#[table = "video_shows"]
pub(crate) struct VideoShowModel {
    #[key]
    pub id: Uuid,

    #[index]
    pub library_id: Uuid,

    #[unique]
    pub show_key: String,

    pub source_kind: String,

    #[index]
    pub online_source_id: Option<Uuid>,

    pub online_item_id: Option<String>,
    pub online_detail_url: Option<String>,

    pub title: String,
    pub summary: String,
    pub year: Option<i64>,

    #[column(type = text)]
    pub genres: toasty::Json<Vec<String>>,

    #[column(type = text)]
    pub tags: toasty::Json<Vec<String>>,

    #[index]
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

    #[index]
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

#[derive(Debug, toasty::Model)]
#[table = "video_sources"]
pub(crate) struct VideoSourceRecordModel {
    #[key]
    pub id: Uuid,

    pub name: String,
    pub source_url: String,
    pub group_name: String,
    pub enabled: bool,
    pub weight: i64,
    pub source_type: i64,

    #[column(type = text)]
    pub raw_json: String,

    #[column(type = text)]
    pub runtime_state: toasty::Json<SourceRuntimeState>,

    pub last_test_ok: Option<bool>,
    pub last_test_msg: Option<String>,
    pub created_at: Timestamp,
    pub updated_at: Timestamp,
}

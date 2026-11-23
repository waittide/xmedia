use jiff::Timestamp;
use uuid::Uuid;

use crate::domain::library::{LibraryKind, ScanStatus};
use crate::domain::online_item::OnlineItemProgress;
use crate::domain::user::UserPreferences;

#[derive(Debug, toasty::Model)]
#[table = "users"]
pub(crate) struct UserModel {
    #[key]
    pub id: Uuid,

    #[unique]
    pub username: String,

    pub display_name: String,
    pub password_hash: String,
    pub subsonic_secret_enc: String,

    #[unique]
    pub api_key: String,

    pub role: String,
    pub is_active: bool,

    #[column(type = text)]
    pub preferences: toasty::Json<UserPreferences>,

    pub created_at: Timestamp,
    pub updated_at: Timestamp,
}

#[derive(Debug, toasty::Model)]
#[table = "libraries"]
pub(crate) struct LibraryModel {
    #[key]
    pub id: Uuid,

    pub name: String,

    #[index]
    pub media_kind: String,

    #[index]
    pub is_managed: bool,

    #[column(type = text)]
    pub kind: toasty::Json<LibraryKind>,

    #[column(type = text)]
    pub scan_status: toasty::Json<ScanStatus>,

    pub last_scanned_at: Option<Timestamp>,
    pub created_at: Timestamp,
    pub updated_at: Timestamp,
}

#[derive(Debug, toasty::Model)]
#[table = "online_library_items"]
pub(crate) struct OnlineLibraryItemModel {
    #[key]
    pub id: Uuid,

    #[index]
    pub user_id: Uuid,

    #[index]
    pub media_kind: String,

    pub source_kind: String,
    pub source_id: String,

    #[column(type = text)]
    pub item_key: String,

    pub title: String,
    pub subtitle: Option<String>,

    #[column(type = text)]
    pub cover_url: Option<String>,

    #[column(type = text)]
    pub intro: Option<String>,

    pub kind: Option<String>,

    #[column(type = text)]
    pub extra: toasty::Json<serde_json::Value>,

    pub favorite: bool,
    pub favorited_at: Option<Timestamp>,

    #[column(type = text)]
    pub progress: toasty::Json<Option<OnlineItemProgress>>,

    pub progress_at: Option<Timestamp>,
    pub created_at: Timestamp,
    pub updated_at: Timestamp,
}

#[derive(Debug, toasty::Model)]
#[table = "user_activity_daily"]
pub(crate) struct UserActivityDailyModel {
    #[key]
    pub id: Uuid,

    #[index]
    pub user_id: Uuid,

    #[index]
    pub media_kind: String,

    #[index]
    pub activity_date: String,

    pub duration_secs: i64,
    pub active_sessions: i64,
    pub last_item_title: Option<String>,
    pub updated_at: Timestamp,
}

#[derive(Debug, toasty::Model)]
#[table = "user_media_bindings"]
pub(crate) struct UserMediaBindingModel {
    #[key]
    pub id: Uuid,

    #[index]
    pub user_id: Uuid,

    #[index]
    pub media_kind: String,

    #[index]
    pub item_id: Uuid,

    pub created_at: Timestamp,
}

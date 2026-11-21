use jiff::Timestamp;
use uuid::Uuid;

use crate::domain::library::{LibraryKind, ScanStatus};
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

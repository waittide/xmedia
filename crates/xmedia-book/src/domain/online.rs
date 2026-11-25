use jiff::Timestamp;
use uuid::Uuid;
use xmedia_core::domain::user::UserId;

pub use xmedia_source::model::SourceRuntimeState;

#[derive(Debug, Clone)]
pub struct BookSourceRecord {
    pub id: Uuid,
    pub name: String,
    pub source_url: String,
    pub group_name: String,
    pub enabled: bool,
    pub weight: i64,
    pub source_type: i64,
    pub raw_json: String,
    pub runtime_state: SourceRuntimeState,
    pub last_test_ok: Option<bool>,
    pub last_test_msg: Option<String>,
    pub created_at: Timestamp,
    pub updated_at: Timestamp,
}

#[derive(Debug, Clone)]
pub struct BookSourceMetadataUpdate {
    pub name: String,
    pub source_url: String,
    pub group_name: String,
    pub enabled: bool,
    pub weight: i64,
    pub source_type: i64,
    pub raw_json: String,
    pub variable: Option<String>,
    pub updated_at: Timestamp,
}

impl BookSourceRecord {
    /// Synchronizes mutable top-level metadata (`enabled`, `weight`, `bookSourceGroup`)
    /// into a Legado `raw_json` string so exported JSON stays consistent with DB state.
    pub fn sync_raw_json_fields(
        raw_json: &str,
        enabled: Option<bool>,
        weight: Option<i64>,
        group_name: Option<&str>,
    ) -> String {
        xmedia_source::crud::sync_raw_json_fields(
            raw_json,
            enabled,
            weight,
            group_name,
            "bookSourceGroup",
        )
    }
}

#[derive(Debug, Clone)]
pub struct OnlineShelfBook {
    pub id: Uuid,
    pub user_id: UserId,
    pub source_id: Uuid,
    pub name: String,
    pub author: String,
    pub book_url: String,
    pub toc_url: String,
    pub cover_url: Option<String>,
    pub intro: Option<String>,
    pub kind: Option<String>,
    pub last_chapter: Option<String>,
    pub total_chapters: i64,
    pub current_chapter_index: i64,
    pub current_chapter_title: Option<String>,
    pub scroll_ratio: f64,
    pub created_at: Timestamp,
    pub updated_at: Timestamp,
}

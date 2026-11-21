use jiff::Timestamp;
use uuid::Uuid;

use crate::source::model::SourceRuntimeState;

#[derive(Debug, Clone)]
pub struct VideoSourceRecord {
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
pub struct VideoSourceMetadataUpdate {
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

impl VideoSourceRecord {
    /// Synchronizes mutable metadata (`enabled`, `weight`, `group_name`) back into `raw_json`
    /// so exported JSON reflects live edits and batch updates.
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
            "sourceGroup",
        )
    }
}

use jiff::Timestamp;
use uuid::Uuid;

use crate::source::model::SourceRuntimeState;

#[derive(Debug, Clone)]
pub struct ComicSourceRecord {
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
pub struct ComicSourceMetadataUpdate {
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

impl ComicSourceRecord {
    /// Synchronizes mutable top-level metadata (`enabled`, `weight`, `group_name`)
    /// into `raw_json`, preserving `"bookSourceGroup"` when present without `"sourceGroup"`.
    pub fn sync_raw_json_fields(
        raw_json: &str,
        enabled: Option<bool>,
        weight: Option<i64>,
        group_name: Option<&str>,
    ) -> String {
        let group_key = if serde_json::from_str::<serde_json::Value>(raw_json)
            .ok()
            .and_then(|v| {
                v.as_object().map(|obj| {
                    obj.contains_key("bookSourceGroup")
                        || (!obj.contains_key("sourceGroup")
                            && (obj.contains_key("bookSourceName")
                                || obj.contains_key("bookSourceUrl")
                                || obj.contains_key("ruleBookInfo")
                                || obj.contains_key("ruleContent")))
                })
            })
            .unwrap_or(false)
        {
            "bookSourceGroup"
        } else {
            "sourceGroup"
        };
        xmedia_source::crud::sync_raw_json_fields(raw_json, enabled, weight, group_name, group_key)
    }
}

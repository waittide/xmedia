use jiff::Timestamp;
use uuid::Uuid;
use xmedia_core::domain::user::UserId;

use crate::source::model::SourceRuntimeState;

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

impl BookSourceRecord {
    /// Synchronizes mutable top-level metadata (`enabled`, `weight`, `bookSourceGroup`)
    /// into a Legado `raw_json` string so exported JSON stays consistent with DB state.
    pub fn sync_raw_json_fields(
        raw_json: &str,
        enabled: Option<bool>,
        weight: Option<i64>,
        group_name: Option<&str>,
    ) -> String {
        let Ok(mut val) = serde_json::from_str::<serde_json::Value>(raw_json) else {
            return raw_json.to_string();
        };
        let Some(obj) = val.as_object_mut() else {
            return raw_json.to_string();
        };
        if let Some(en) = enabled {
            obj.insert("enabled".into(), serde_json::Value::Bool(en));
        }
        if let Some(w) = weight {
            obj.insert("weight".into(), serde_json::Value::from(w));
        }
        if let Some(g) = group_name {
            let trimmed = g.trim();
            if trimmed.is_empty() {
                obj.remove("bookSourceGroup");
            } else {
                obj.insert(
                    "bookSourceGroup".into(),
                    serde_json::Value::String(trimmed.to_string()),
                );
            }
        }
        serde_json::to_string(&val).unwrap_or_else(|_| raw_json.to_string())
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

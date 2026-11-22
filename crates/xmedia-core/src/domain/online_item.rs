//! Per-user persisted state (favorite + reading/playback progress) for items that live in an
//! online rule source or a remote server (OPDS/Subsonic) rather than in a local library.
//!
//! Items are identified by the natural key `(user, media_kind, source_kind, source_id, item_key)`
//! so clients can idempotently upsert them straight from search/browse results.

use jiff::Timestamp;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::domain::error::{DomainError, DomainResult};
use crate::domain::library::MediaKind;
use crate::domain::user::UserId;

pub const MAX_ITEM_KEY_CHARS: usize = 2048;
pub const MAX_SOURCE_ID_CHARS: usize = 128;
pub const MAX_TITLE_CHARS: usize = 512;
pub const MAX_SHORT_TEXT_CHARS: usize = 512;
pub const MAX_INTRO_CHARS: usize = 8192;
pub const MAX_URL_CHARS: usize = 2048;
pub const MAX_EXTRA_JSON_BYTES: usize = 16 * 1024;

/// Where an online library item comes from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum OnlineItemSourceKind {
    /// A rule-based online source managed by this server (`source_id` is its UUID).
    #[default]
    Online,
    /// A remote server (OPDS / Subsonic) configured on the client; not verifiable server-side.
    Remote,
}

impl OnlineItemSourceKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Online => "online",
            Self::Remote => "remote",
        }
    }

    pub fn parse(raw: &str) -> DomainResult<Self> {
        match raw.trim().to_ascii_lowercase().as_str() {
            "online" => Ok(Self::Online),
            "remote" => Ok(Self::Remote),
            other => Err(DomainError::Validation(format!(
                "unsupported online item source kind: {other}"
            ))),
        }
    }
}

/// Reading / playback position inside an online item.
///
/// * comic: `unit_*` = chapter, `position` = 0-based page, `duration` = page count
/// * video: `unit_*` = episode (`unit_group` = play line), `position`/`duration` = seconds
/// * music: no unit, `position`/`duration` = seconds
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct OnlineItemProgress {
    #[serde(default)]
    pub unit_index: Option<i64>,
    #[serde(default)]
    pub unit_key: Option<String>,
    #[serde(default)]
    pub unit_title: Option<String>,
    #[serde(default)]
    pub unit_group: Option<String>,
    #[serde(default)]
    pub position: f64,
    #[serde(default)]
    pub duration: Option<f64>,
    #[serde(default)]
    pub total_units: Option<i64>,
    #[serde(default)]
    pub completed: bool,
}

impl OnlineItemProgress {
    /// Validates and clamps the progress payload into storable bounds.
    pub fn normalized(mut self) -> DomainResult<Self> {
        if !self.position.is_finite() {
            return Err(DomainError::Validation(
                "progress position is invalid".into(),
            ));
        }
        self.position = self.position.max(0.0);
        self.duration = match self.duration {
            Some(d) if !d.is_finite() => {
                return Err(DomainError::Validation(
                    "progress duration is invalid".into(),
                ));
            }
            Some(d) if d > 0.0 => Some(d),
            _ => None,
        };
        self.unit_index = self.unit_index.map(|i| i.max(0));
        self.total_units = self.total_units.filter(|t| *t > 0);
        self.unit_key = normalize_opt_text(self.unit_key, MAX_URL_CHARS);
        self.unit_title = normalize_opt_text(self.unit_title, MAX_SHORT_TEXT_CHARS);
        self.unit_group = normalize_opt_text(self.unit_group, MAX_SHORT_TEXT_CHARS);
        Ok(self)
    }

    /// Overall completion ratio in `0..=1`, combining unit index and in-unit position.
    pub fn ratio(&self) -> Option<f64> {
        if self.completed {
            return Some(1.0);
        }
        let in_unit = self.duration.map(|d| (self.position / d).clamp(0.0, 1.0));
        match (self.unit_index, self.total_units) {
            (Some(index), Some(total)) if total > 0 => {
                let done = index.min(total) as f64 + in_unit.unwrap_or(0.0);
                Some((done / total as f64).clamp(0.0, 1.0))
            }
            _ => in_unit,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct OnlineLibraryItem {
    pub id: Uuid,
    pub user_id: UserId,
    pub media_kind: MediaKind,
    pub source_kind: OnlineItemSourceKind,
    pub source_id: String,
    pub item_key: String,
    pub title: String,
    pub subtitle: Option<String>,
    pub cover_url: Option<String>,
    pub intro: Option<String>,
    pub kind: Option<String>,
    pub extra: serde_json::Value,
    pub favorite: bool,
    pub favorited_at: Option<Timestamp>,
    pub progress: Option<OnlineItemProgress>,
    pub progress_at: Option<Timestamp>,
    pub created_at: Timestamp,
    pub updated_at: Timestamp,
}

impl OnlineLibraryItem {
    pub fn matches_key(
        &self,
        source_kind: OnlineItemSourceKind,
        source_id: &str,
        item_key: &str,
    ) -> bool {
        self.source_kind == source_kind && self.source_id == source_id && self.item_key == item_key
    }

    pub fn set_favorite(&mut self, favorite: bool, now: Timestamp) {
        if favorite && !self.favorite {
            self.favorited_at = Some(now);
        } else if !favorite {
            self.favorited_at = None;
        }
        self.favorite = favorite;
    }

    pub fn set_progress(&mut self, progress: Option<OnlineItemProgress>, now: Timestamp) {
        self.progress_at = progress.as_ref().map(|_| now);
        self.progress = progress;
    }

    /// Whether the item belongs in a "continue reading/watching/listening" view.
    pub fn is_in_progress(&self) -> bool {
        self.progress.as_ref().is_some_and(|p| !p.completed)
    }
}

fn has_control_chars(raw: &str) -> bool {
    raw.chars().any(char::is_control)
}

fn truncate_chars(raw: &str, max_chars: usize) -> String {
    raw.chars().take(max_chars).collect()
}

/// Trims, drops empty values and truncates free-form text.
pub fn normalize_opt_text(raw: Option<String>, max_chars: usize) -> Option<String> {
    let trimmed = raw?.trim().to_string();
    if trimmed.is_empty() {
        None
    } else {
        Some(truncate_chars(&trimmed, max_chars))
    }
}

/// Validates an identifier-like field (source id / item key): non-empty, bounded, no control chars.
pub fn validate_key(raw: &str, field: &str, max_chars: usize) -> DomainResult<String> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Err(DomainError::Validation(format!("{field} cannot be empty")));
    }
    if trimmed.chars().count() > max_chars {
        return Err(DomainError::Validation(format!(
            "{field} cannot exceed {max_chars} characters"
        )));
    }
    if has_control_chars(trimmed) {
        return Err(DomainError::Validation(format!(
            "{field} contains control characters"
        )));
    }
    Ok(trimmed.to_string())
}

/// Validates the display title.
pub fn validate_title(raw: &str) -> DomainResult<String> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Err(DomainError::Validation("title cannot be empty".into()));
    }
    Ok(truncate_chars(trimmed, MAX_TITLE_CHARS))
}

/// Accepts only absolute `http(s)://` URLs or same-origin absolute paths (`/...`, not `//...`)
/// so stored cover URLs can never smuggle `javascript:`/`data:` schemes into the client.
pub fn validate_cover_url(raw: Option<String>) -> DomainResult<Option<String>> {
    let Some(url) = normalize_opt_text(raw, usize::MAX) else {
        return Ok(None);
    };
    if url.chars().count() > MAX_URL_CHARS {
        return Err(DomainError::Validation(format!(
            "cover_url cannot exceed {MAX_URL_CHARS} characters"
        )));
    }
    if has_control_chars(&url) || url.contains(char::is_whitespace) {
        return Err(DomainError::Validation("cover_url is invalid".into()));
    }
    let lower = url.to_ascii_lowercase();
    let is_http = lower.starts_with("http://") || lower.starts_with("https://");
    let is_local_path = url.starts_with('/') && !url.starts_with("//") && !url.starts_with("/\\");
    if !is_http && !is_local_path {
        return Err(DomainError::Validation(
            "cover_url must be an http(s) URL or a site-relative path".into(),
        ));
    }
    Ok(Some(url))
}

/// Ensures `extra` is a JSON object (or null) within the size budget.
pub fn validate_extra(raw: Option<serde_json::Value>) -> DomainResult<serde_json::Value> {
    let value = match raw {
        None | Some(serde_json::Value::Null) => {
            return Ok(serde_json::Value::Object(Default::default()));
        }
        Some(v @ serde_json::Value::Object(_)) => v,
        Some(_) => {
            return Err(DomainError::Validation(
                "extra must be a JSON object".into(),
            ));
        }
    };
    let size = serde_json::to_vec(&value)
        .map_err(|e| DomainError::Validation(format!("extra is not serializable: {e}")))?
        .len();
    if size > MAX_EXTRA_JSON_BYTES {
        return Err(DomainError::Validation(format!(
            "extra cannot exceed {MAX_EXTRA_JSON_BYTES} bytes"
        )));
    }
    Ok(value)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn source_kind_round_trips() {
        assert_eq!(
            OnlineItemSourceKind::parse(" Remote ").unwrap(),
            OnlineItemSourceKind::Remote
        );
        assert_eq!(OnlineItemSourceKind::Online.as_str(), "online");
        assert!(OnlineItemSourceKind::parse("local").is_err());
    }

    #[test]
    fn progress_normalization_clamps_and_rejects_nan() {
        let p = OnlineItemProgress {
            unit_index: Some(-3),
            unit_key: Some("  ".into()),
            unit_title: Some(" 第1话 ".into()),
            position: -5.0,
            duration: Some(0.0),
            total_units: Some(0),
            ..Default::default()
        }
        .normalized()
        .unwrap();
        assert_eq!(p.unit_index, Some(0));
        assert_eq!(p.unit_key, None);
        assert_eq!(p.unit_title.as_deref(), Some("第1话"));
        assert_eq!(p.position, 0.0);
        assert_eq!(p.duration, None);
        assert_eq!(p.total_units, None);

        let bad = OnlineItemProgress {
            position: f64::NAN,
            ..Default::default()
        };
        assert!(bad.normalized().is_err());
        let bad = OnlineItemProgress {
            duration: Some(f64::INFINITY),
            ..Default::default()
        };
        assert!(bad.normalized().is_err());
    }

    #[test]
    fn progress_ratio_combines_units_and_position() {
        let media = OnlineItemProgress {
            position: 30.0,
            duration: Some(120.0),
            ..Default::default()
        };
        assert_eq!(media.ratio(), Some(0.25));

        let comic = OnlineItemProgress {
            unit_index: Some(1),
            total_units: Some(4),
            position: 5.0,
            duration: Some(10.0),
            ..Default::default()
        };
        assert_eq!(comic.ratio(), Some(0.375));

        let done = OnlineItemProgress {
            completed: true,
            ..Default::default()
        };
        assert_eq!(done.ratio(), Some(1.0));
        assert_eq!(OnlineItemProgress::default().ratio(), None);
    }

    #[test]
    fn cover_url_rejects_dangerous_schemes() {
        assert_eq!(
            validate_cover_url(Some(" https://img.example/a.jpg ".into())).unwrap(),
            Some("https://img.example/a.jpg".into())
        );
        assert_eq!(
            validate_cover_url(Some("/_app/subsonic/x/cover/1".into())).unwrap(),
            Some("/_app/subsonic/x/cover/1".into())
        );
        assert_eq!(validate_cover_url(Some("   ".into())).unwrap(), None);
        for bad in [
            "javascript:alert(1)",
            "data:image/png;base64,AAAA",
            "//evil.example/a.png",
            "/\\evil.example",
            "https://a b",
        ] {
            assert!(validate_cover_url(Some(bad.into())).is_err(), "{bad}");
        }
    }

    #[test]
    fn extra_must_be_bounded_object() {
        assert_eq!(validate_extra(None).unwrap(), serde_json::json!({}));
        assert_eq!(
            validate_extra(Some(serde_json::json!({"toc_url": "x"}))).unwrap(),
            serde_json::json!({"toc_url": "x"})
        );
        assert!(validate_extra(Some(serde_json::json!([1, 2]))).is_err());
        let huge = "x".repeat(MAX_EXTRA_JSON_BYTES + 1);
        assert!(validate_extra(Some(serde_json::json!({ "k": huge }))).is_err());
    }

    #[test]
    fn keys_reject_empty_and_control_chars() {
        assert_eq!(validate_key(" abc ", "item_key", 10).unwrap(), "abc");
        assert!(validate_key("  ", "item_key", 10).is_err());
        assert!(validate_key("a\nb", "item_key", 10).is_err());
        assert!(validate_key("abcdefghijk", "item_key", 10).is_err());
        assert!(validate_title(" ").is_err());
    }

    #[test]
    fn favorite_and_progress_track_timestamps() {
        let now = Timestamp::now();
        let mut item = OnlineLibraryItem {
            id: Uuid::now_v7(),
            user_id: UserId::new(),
            media_kind: MediaKind::Comic,
            source_kind: OnlineItemSourceKind::Online,
            source_id: "s".into(),
            item_key: "k".into(),
            title: "t".into(),
            subtitle: None,
            cover_url: None,
            intro: None,
            kind: None,
            extra: serde_json::json!({}),
            favorite: false,
            favorited_at: None,
            progress: None,
            progress_at: None,
            created_at: now,
            updated_at: now,
        };
        item.set_favorite(true, now);
        assert_eq!(item.favorited_at, Some(now));
        item.set_favorite(true, Timestamp::now());
        assert_eq!(
            item.favorited_at,
            Some(now),
            "re-favoriting keeps original time"
        );
        item.set_favorite(false, now);
        assert_eq!(item.favorited_at, None);

        assert!(!item.is_in_progress());
        item.set_progress(Some(OnlineItemProgress::default()), now);
        assert!(item.is_in_progress());
        assert_eq!(item.progress_at, Some(now));
        item.set_progress(None, now);
        assert_eq!(item.progress_at, None);
        assert!(item.matches_key(OnlineItemSourceKind::Online, "s", "k"));
        assert!(!item.matches_key(OnlineItemSourceKind::Remote, "s", "k"));
    }
}

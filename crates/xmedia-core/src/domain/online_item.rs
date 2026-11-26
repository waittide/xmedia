//! Per-user persisted state (favorite + reading/playback progress) for items that live in an
//! online rule source or a remote server (OPDS/Subsonic) rather than in a local library.

use jiff::Timestamp;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::domain::error::{DomainError, DomainResult};
use crate::domain::library::MediaKind;
use crate::domain::user::UserId;

const MAX_ITEM_KEY_CHARS: usize = 2048;
const MAX_SOURCE_ID_CHARS: usize = 128;
const MAX_TITLE_CHARS: usize = 512;
const MAX_SHORT_TEXT_CHARS: usize = 512;
const MAX_INTRO_CHARS: usize = 8192;
const MAX_URL_CHARS: usize = 2048;
const MAX_EXTRA_JSON_BYTES: usize = 16 * 1024;

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

#[derive(Debug, Clone, PartialEq, Eq)]
struct OnlineItemKey {
    source_kind: OnlineItemSourceKind,
    source_id: String,
    source_uuid: Option<Uuid>,
    item_key: String,
}

impl OnlineItemKey {
    fn parse(
        source_kind: OnlineItemSourceKind,
        source_id: &str,
        item_key: &str,
    ) -> DomainResult<Self> {
        let source_id = validate_key(source_id, "source_id", MAX_SOURCE_ID_CHARS)?;
        let item_key = validate_key(item_key, "item_key", MAX_ITEM_KEY_CHARS)?;
        let (source_id, source_uuid) = match source_kind {
            OnlineItemSourceKind::Online => {
                let id = Uuid::parse_str(&source_id).map_err(|_| {
                    DomainError::Validation("source_id must be a UUID for online sources".into())
                })?;
                (id.to_string(), Some(id))
            }
            OnlineItemSourceKind::Remote => (source_id, None),
        };
        Ok(Self {
            source_kind,
            source_id,
            source_uuid,
            item_key,
        })
    }
}

/// Reading / playback position inside an online item.
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
    fn normalized(mut self) -> DomainResult<Self> {
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
    fn ratio(&self) -> Option<f64> {
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

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OnlineItemView {
    #[default]
    All,
    Continue,
    Favorites,
}

#[derive(Debug, Clone, Default)]
pub struct OnlineItemFilter {
    pub view: OnlineItemView,
    pub source_kind: Option<OnlineItemSourceKind>,
    pub source_id: Option<String>,
    pub query: Option<String>,
}

#[derive(Debug, Clone)]
pub struct OnlineItemFields {
    pub title: Option<String>,
    pub subtitle: Option<String>,
    pub cover_url: Option<String>,
    pub intro: Option<String>,
    pub kind: Option<String>,
    pub extra: Option<serde_json::Value>,
    pub favorite: Option<bool>,
    pub progress: Option<OnlineItemProgress>,
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
    pub fn filter_and_sort(mut items: Vec<Self>, filter: &OnlineItemFilter) -> Vec<Self> {
        let needle = normalize_search_needle(filter.query.as_deref());
        let source_id = normalize_source_filter(filter.source_id.as_deref());
        items.retain(|item| {
            item.matches_filter(
                filter.view,
                filter.source_kind,
                source_id.as_deref(),
                needle.as_deref(),
            )
        });
        sort_items_for_view(&mut items, filter.view);
        items
    }

    pub fn lookup_matching(
        items: Vec<Self>,
        raw_refs: &[(OnlineItemSourceKind, &str, &str)],
    ) -> Vec<Self> {
        let keys: Vec<OnlineItemKey> = raw_refs
            .iter()
            .filter_map(|(kind, sid, ikey)| OnlineItemKey::parse(*kind, sid, ikey).ok())
            .collect();
        if keys.is_empty() {
            return Vec::new();
        }
        items
            .into_iter()
            .filter(|item| {
                keys.iter()
                    .any(|k| item.matches_key(k.source_kind, &k.source_id, &k.item_key))
            })
            .collect()
    }

    #[allow(clippy::too_many_arguments)]
    pub fn upsert_in(
        existing: Vec<Self>,
        max_items: usize,
        user_id: UserId,
        media_kind: MediaKind,
        source_kind: OnlineItemSourceKind,
        raw_source_id: &str,
        raw_item_key: &str,
        fields: OnlineItemFields,
        now: Timestamp,
    ) -> DomainResult<(Self, Option<Uuid>)> {
        let key = OnlineItemKey::parse(source_kind, raw_source_id, raw_item_key)?;
        let count = existing.len();
        if let Some(mut item) = existing
            .into_iter()
            .find(|i| i.matches_key(key.source_kind, &key.source_id, &key.item_key))
        {
            item.apply_fields(fields, now)?;
            return Ok((item, None));
        }
        if count >= max_items {
            return Err(DomainError::Conflict(format!(
                "online item limit reached ({max_items})"
            )));
        }
        let new_source_uuid = key.source_uuid;
        let created = Self::create(user_id, media_kind, key, fields, now)?;
        Ok((created, new_source_uuid))
    }

    fn create(
        user_id: UserId,
        media_kind: MediaKind,
        key: OnlineItemKey,
        fields: OnlineItemFields,
        now: Timestamp,
    ) -> DomainResult<Self> {
        let title = validate_title(fields.title.as_deref().unwrap_or(""))?;
        let mut item = Self {
            id: Uuid::now_v7(),
            user_id,
            media_kind,
            source_kind: key.source_kind,
            source_id: key.source_id,
            item_key: key.item_key,
            title,
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
        item.apply_fields(
            OnlineItemFields {
                title: None,
                ..fields
            },
            now,
        )?;
        Ok(item)
    }

    pub fn apply_fields(&mut self, fields: OnlineItemFields, now: Timestamp) -> DomainResult<()> {
        if let Some(title) = fields.title {
            self.title = validate_title(&title)?;
        }
        if let Some(subtitle) = fields.subtitle {
            self.subtitle = normalize_opt_text(Some(subtitle), MAX_SHORT_TEXT_CHARS);
        }
        if let Some(cover_url) = fields.cover_url {
            self.cover_url = validate_cover_url(Some(cover_url))?;
        }
        if let Some(intro) = fields.intro {
            self.intro = normalize_opt_text(Some(intro), MAX_INTRO_CHARS);
        }
        if let Some(kind) = fields.kind {
            self.kind = normalize_opt_text(Some(kind), MAX_SHORT_TEXT_CHARS);
        }
        if let Some(extra) = fields.extra {
            self.extra = validate_extra(Some(extra))?;
        }
        if let Some(favorite) = fields.favorite {
            self.set_favorite(favorite, now);
        }
        if let Some(progress) = fields.progress {
            self.set_progress(Some(progress.normalized()?), now);
        }
        self.updated_at = now;
        Ok(())
    }

    pub fn progress_ratio(&self) -> Option<f64> {
        self.progress.as_ref().and_then(OnlineItemProgress::ratio)
    }

    pub fn apply_progress(
        &mut self,
        progress: Option<OnlineItemProgress>,
        now: Timestamp,
    ) -> DomainResult<()> {
        let normalized = progress.map(OnlineItemProgress::normalized).transpose()?;
        self.set_progress(normalized, now);
        self.updated_at = now;
        Ok(())
    }

    fn matches_key(
        &self,
        source_kind: OnlineItemSourceKind,
        source_id: &str,
        item_key: &str,
    ) -> bool {
        self.source_kind == source_kind && self.source_id == source_id && self.item_key == item_key
    }

    fn matches_filter(
        &self,
        view: OnlineItemView,
        source_kind: Option<OnlineItemSourceKind>,
        source_id: Option<&str>,
        needle: Option<&str>,
    ) -> bool {
        let view_ok = match view {
            OnlineItemView::All => true,
            OnlineItemView::Continue => self.is_in_progress(),
            OnlineItemView::Favorites => self.favorite,
        };
        view_ok
            && source_kind.is_none_or(|k| self.source_kind == k)
            && source_id.is_none_or(|s| self.source_id == s)
            && needle.is_none_or(|n| {
                self.title.to_lowercase().contains(n)
                    || self
                        .subtitle
                        .as_deref()
                        .is_some_and(|s| s.to_lowercase().contains(n))
            })
    }

    fn set_favorite(&mut self, favorite: bool, now: Timestamp) {
        if favorite && !self.favorite {
            self.favorited_at = Some(now);
        } else if !favorite {
            self.favorited_at = None;
        }
        self.favorite = favorite;
    }

    fn set_progress(&mut self, progress: Option<OnlineItemProgress>, now: Timestamp) {
        self.progress_at = progress.as_ref().map(|_| now);
        self.progress = progress;
    }

    fn is_in_progress(&self) -> bool {
        self.progress.as_ref().is_some_and(|p| !p.completed)
    }
}

fn normalize_search_needle(raw: Option<&str>) -> Option<String> {
    raw.map(|q| q.trim().to_lowercase())
        .filter(|q| !q.is_empty())
}

fn normalize_source_filter(raw: Option<&str>) -> Option<String> {
    raw.map(str::trim).filter(|s| !s.is_empty()).map(|s| {
        Uuid::parse_str(s)
            .map(|u| u.to_string())
            .unwrap_or_else(|_| s.to_string())
    })
}

fn sort_items_for_view(items: &mut [OnlineLibraryItem], view: OnlineItemView) {
    match view {
        OnlineItemView::All => items.sort_by_key(|i| std::cmp::Reverse(i.updated_at)),
        OnlineItemView::Continue => {
            items.sort_by_key(|i| std::cmp::Reverse(i.progress_at.unwrap_or(i.updated_at)))
        }
        OnlineItemView::Favorites => {
            items.sort_by_key(|i| std::cmp::Reverse(i.favorited_at.unwrap_or(i.updated_at)))
        }
    }
}

fn has_control_chars(raw: &str) -> bool {
    raw.chars().any(char::is_control)
}

fn truncate_chars(raw: &str, max_chars: usize) -> String {
    raw.chars().take(max_chars).collect()
}

fn normalize_opt_text(raw: Option<String>, max_chars: usize) -> Option<String> {
    let trimmed = raw?.trim().to_string();
    if trimmed.is_empty() {
        None
    } else {
        Some(truncate_chars(&trimmed, max_chars))
    }
}

fn validate_key(raw: &str, field: &str, max_chars: usize) -> DomainResult<String> {
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

fn validate_title(raw: &str) -> DomainResult<String> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Err(DomainError::Validation("title cannot be empty".into()));
    }
    Ok(truncate_chars(trimmed, MAX_TITLE_CHARS))
}

fn validate_cover_url(raw: Option<String>) -> DomainResult<Option<String>> {
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

fn validate_extra(raw: Option<serde_json::Value>) -> DomainResult<serde_json::Value> {
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
    fn source_kind_roundtrips() {
        assert_eq!(
            OnlineItemSourceKind::parse(" REMOTE ").unwrap(),
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

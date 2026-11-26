use std::fmt;
use std::str::FromStr;

use jiff::Timestamp;
use serde::{Deserialize, Serialize};
use uuid::Uuid;
use xmedia_core::domain::error::{DomainError, DomainResult};
use xmedia_core::domain::user::UserId;

use crate::domain::id::{VideoId, VideoShelfId};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WatchProgress {
    pub user_id: UserId,
    pub video_id: VideoId,
    pub position_secs: f64,
    pub duration_secs: u32,
    pub completed: bool,
    pub selected_audio_stream: Option<u32>,
    pub selected_subtitle: Option<String>,
    pub updated_at: Timestamp,
}

impl WatchProgress {
    #[allow(clippy::too_many_arguments)]
    pub fn reconcile(
        user_id: UserId,
        video_id: VideoId,
        video_duration_secs: u32,
        position_secs: f64,
        duration_override: Option<u32>,
        explicit_completed: Option<bool>,
        selected_audio_stream: Option<u32>,
        selected_subtitle: Option<String>,
        existing: Option<Self>,
    ) -> DomainResult<Self> {
        let clean_pos = validate_position_secs(position_secs)?;
        let duration_secs = resolve_duration_secs(duration_override, video_duration_secs);
        let completed = compute_completed(clean_pos, duration_secs, explicit_completed);
        Ok(Self {
            user_id,
            video_id,
            position_secs: clean_pos,
            duration_secs,
            completed,
            selected_audio_stream: selected_audio_stream
                .or_else(|| existing.as_ref().and_then(|e| e.selected_audio_stream)),
            selected_subtitle: selected_subtitle
                .or_else(|| existing.and_then(|e| e.selected_subtitle)),
            updated_at: Timestamp::now(),
        })
    }

    pub fn mark_watched(
        user_id: UserId,
        video_id: VideoId,
        duration_secs: u32,
        watched: bool,
        existing: Option<Self>,
    ) -> Self {
        let position_secs = if watched {
            f64::from(duration_secs)
        } else {
            0.0
        };
        Self {
            user_id,
            video_id,
            position_secs,
            duration_secs,
            completed: watched,
            selected_audio_stream: existing.as_ref().and_then(|e| e.selected_audio_stream),
            selected_subtitle: existing.and_then(|e| e.selected_subtitle),
            updated_at: Timestamp::now(),
        }
    }
}

fn validate_position_secs(position_secs: f64) -> DomainResult<f64> {
    if !position_secs.is_finite() {
        return Err(DomainError::Validation(
            "position_secs must be a finite number".into(),
        ));
    }
    Ok(position_secs.max(0.0))
}

fn resolve_duration_secs(override_secs: Option<u32>, fallback_secs: u32) -> u32 {
    override_secs.filter(|&d| d > 0).unwrap_or(fallback_secs)
}

fn compute_completed(
    position_secs: f64,
    duration_secs: u32,
    explicit_completed: Option<bool>,
) -> bool {
    if let Some(done) = explicit_completed {
        return done;
    }
    if duration_secs > 0 {
        position_secs >= f64::from(duration_secs) * 0.92
    } else {
        false
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum VideoFavoriteKind {
    Video,
    Show,
}

impl AsRef<str> for VideoFavoriteKind {
    fn as_ref(&self) -> &str {
        favorite_kind_str(*self)
    }
}

impl fmt::Display for VideoFavoriteKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(favorite_kind_str(*self))
    }
}

impl FromStr for VideoFavoriteKind {
    type Err = DomainError;

    fn from_str(raw: &str) -> DomainResult<Self> {
        parse_favorite_kind_str(raw)
    }
}

fn favorite_kind_str(kind: VideoFavoriteKind) -> &'static str {
    match kind {
        VideoFavoriteKind::Video => "video",
        VideoFavoriteKind::Show => "show",
    }
}

fn parse_favorite_kind_str(raw: &str) -> DomainResult<VideoFavoriteKind> {
    match raw.trim().to_ascii_lowercase().as_str() {
        "video" | "videos" => Ok(VideoFavoriteKind::Video),
        "show" | "shows" | "series" => Ok(VideoFavoriteKind::Show),
        other => Err(DomainError::Validation(format!(
            "unknown video favorite kind: {other}"
        ))),
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VideoFavoriteEntry {
    pub user_id: UserId,
    pub kind: VideoFavoriteKind,
    pub target_id: Uuid,
    pub created_at: Timestamp,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VideoShelf {
    pub id: VideoShelfId,
    pub user_id: UserId,
    pub name: String,
    pub description: String,
    pub video_ids: Vec<VideoId>,
    pub created_at: Timestamp,
    pub updated_at: Timestamp,
}

impl VideoShelf {
    pub fn new(
        user_id: UserId,
        name: impl Into<String>,
        description: impl Into<String>,
        video_ids: Vec<VideoId>,
    ) -> DomainResult<Self> {
        let name = validate_shelf_name(name.into())?;
        let now = Timestamp::now();
        Ok(Self {
            id: VideoShelfId::new(),
            user_id,
            name,
            description: description.into().trim().to_string(),
            video_ids,
            created_at: now,
            updated_at: now,
        })
    }

    pub fn update(
        &mut self,
        name: impl Into<String>,
        description: impl Into<String>,
        video_ids: Vec<VideoId>,
    ) -> DomainResult<()> {
        self.name = validate_shelf_name(name.into())?;
        self.description = description.into().trim().to_string();
        self.video_ids = video_ids;
        self.updated_at = Timestamp::now();
        Ok(())
    }
}

fn validate_shelf_name(raw: String) -> DomainResult<String> {
    let name = raw.trim().to_string();
    let char_len = name.chars().count();
    if char_len == 0 || char_len > 120 {
        return Err(DomainError::Validation(
            "shelf name must be between 1 and 120 characters".into(),
        ));
    }
    Ok(name)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn video_shelf_update_enforces_character_count_invariant_for_multibyte_names() {
        let mut shelf = VideoShelf::new(UserId::new(), "Initial", "Desc", Vec::new()).unwrap();
        let cjk_60 = "漫".repeat(60);
        assert_eq!(cjk_60.len(), 180);
        shelf
            .update(&cjk_60, "  Updated Desc  ", Vec::new())
            .expect("60 CJK chars (180 UTF-8 bytes) must be accepted");
        assert_eq!(shelf.name, cjk_60);
        assert_eq!(shelf.description, "Updated Desc");

        let cjk_121 = "漫".repeat(121);
        assert!(shelf.update(cjk_121, "", Vec::new()).is_err());
        assert!(shelf.update("   ", "", Vec::new()).is_err());
    }
}

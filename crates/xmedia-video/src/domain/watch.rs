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
    pub fn compute_completed(
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
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum VideoFavoriteKind {
    Video,
    Show,
}

impl VideoFavoriteKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Video => "video",
            Self::Show => "show",
        }
    }

    pub fn parse(raw: &str) -> DomainResult<Self> {
        match raw.trim().to_ascii_lowercase().as_str() {
            "video" | "videos" => Ok(Self::Video),
            "show" | "shows" | "series" => Ok(Self::Show),
            other => Err(DomainError::Validation(format!(
                "unknown video favorite kind: {other}"
            ))),
        }
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
        let name = name.into().trim().to_string();
        let char_len = name.chars().count();
        if char_len == 0 || char_len > 120 {
            return Err(DomainError::Validation(
                "shelf name must be between 1 and 120 characters".into(),
            ));
        }
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
}

use jiff::Timestamp;
use serde::{Deserialize, Serialize};
use uuid::Uuid;
use xmedia_core::domain::error::{DomainError, DomainResult};
use xmedia_core::domain::user::UserId;

use crate::domain::catalog::{AlbumId, TrackId};

xmedia_core::define_uuid_id!(PlaylistId);

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Playlist {
    pub id: PlaylistId,
    pub owner_id: UserId,
    pub owner_username: String,
    pub name: String,
    pub comment: Option<String>,
    pub is_public: bool,
    pub track_ids: Vec<TrackId>,
    pub created_at: Timestamp,
    pub updated_at: Timestamp,
}

impl Playlist {
    pub fn new(
        owner_id: UserId,
        owner_username: impl Into<String>,
        name: impl Into<String>,
        comment: Option<String>,
        is_public: bool,
    ) -> DomainResult<Self> {
        let name = Self::validate_name(name.into())?;
        let now = Timestamp::now();
        Ok(Self {
            id: PlaylistId::new(),
            owner_id,
            owner_username: owner_username.into(),
            name,
            comment: comment
                .map(|c| c.trim().to_string())
                .filter(|c| !c.is_empty()),
            is_public,
            track_ids: Vec::new(),
            created_at: now,
            updated_at: now,
        })
    }

    pub fn validate_name(raw: String) -> DomainResult<String> {
        let trimmed = raw.trim().to_string();
        if trimmed.is_empty() || trimmed.len() > 128 {
            return Err(DomainError::Validation(
                "playlist name must be between 1 and 128 characters".into(),
            ));
        }
        Ok(trimmed)
    }

    pub fn update_meta(
        &mut self,
        name: Option<String>,
        comment: Option<String>,
        is_public: Option<bool>,
    ) -> DomainResult<()> {
        if let Some(n) = name {
            self.name = Self::validate_name(n)?;
        }
        if let Some(c) = comment {
            let trimmed = c.trim().to_string();
            self.comment = if trimmed.is_empty() {
                None
            } else {
                Some(trimmed)
            };
        }
        if let Some(pub_flag) = is_public {
            self.is_public = pub_flag;
        }
        self.updated_at = Timestamp::now();
        Ok(())
    }

    pub fn replace_tracks(&mut self, track_ids: Vec<TrackId>) {
        self.track_ids = track_ids;
        self.updated_at = Timestamp::now();
    }

    pub fn append_tracks(&mut self, to_add: &[TrackId]) {
        self.track_ids.extend_from_slice(to_add);
        self.updated_at = Timestamp::now();
    }

    pub fn remove_indices(&mut self, mut indices: Vec<usize>) {
        indices.sort_unstable();
        indices.dedup();
        for idx in indices.into_iter().rev() {
            if idx < self.track_ids.len() {
                self.track_ids.remove(idx);
            }
        }
        self.updated_at = Timestamp::now();
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum StarTargetKind {
    Track,
    Album,
    Artist,
}

impl StarTargetKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Track => "track",
            Self::Album => "album",
            Self::Artist => "artist",
        }
    }

    pub fn parse(raw: &str) -> DomainResult<Self> {
        match raw.trim().to_ascii_lowercase().as_str() {
            "track" | "song" => Ok(Self::Track),
            "album" => Ok(Self::Album),
            "artist" => Ok(Self::Artist),
            other => Err(DomainError::Validation(format!(
                "unknown star target kind: {other}"
            ))),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StarredEntry {
    pub user_id: UserId,
    pub kind: StarTargetKind,
    pub target_id: Uuid,
    pub starred_at: Timestamp,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RatingEntry {
    pub user_id: UserId,
    pub kind: StarTargetKind,
    pub target_id: Uuid,
    pub rating: u8,
    pub updated_at: Timestamp,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TrackPlayStat {
    pub user_id: UserId,
    pub track_id: TrackId,
    pub album_id: AlbumId,
    pub play_count: u64,
    pub last_played_at: Timestamp,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ScrobbleEntry {
    pub id: Uuid,
    pub user_id: UserId,
    pub track_id: TrackId,
    pub played_at: Timestamp,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NowPlayingEntry {
    pub user_id: UserId,
    pub username: String,
    pub track_id: TrackId,
    pub client_name: String,
    pub updated_at: Timestamp,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PlayQueue {
    pub user_id: UserId,
    pub track_ids: Vec<TrackId>,
    pub current_track_id: Option<TrackId>,
    pub position_ms: u64,
    pub changed_by: String,
    pub updated_at: Timestamp,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AudioBookmark {
    pub user_id: UserId,
    pub track_id: TrackId,
    pub position_ms: u64,
    pub comment: Option<String>,
    pub created_at: Timestamp,
    pub updated_at: Timestamp,
}

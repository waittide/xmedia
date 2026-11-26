use std::collections::{HashMap, HashSet};

use jiff::Timestamp;
use serde::{Deserialize, Serialize};
use uuid::Uuid;
use xmedia_core::domain::error::{DomainError, DomainResult};
use xmedia_core::domain::user::UserId;

use crate::domain::catalog::{AlbumId, Track, TrackId};

xmedia_core::define_uuid_id!(PlaylistId);

#[derive(Debug, Clone)]
pub enum PlaylistTracksOp {
    Keep,
    Replace(Vec<TrackId>),
    Patch {
        remove_indices: Vec<usize>,
        append: Vec<TrackId>,
    },
}

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
        initial_tracks: Vec<TrackId>,
    ) -> DomainResult<Self> {
        let name = validate_playlist_name(name.into())?;
        let now = Timestamp::now();
        Ok(Self {
            id: PlaylistId::new(),
            owner_id,
            owner_username: owner_username.into(),
            name,
            comment: clean_optional_comment(comment),
            is_public,
            track_ids: initial_tracks,
            created_at: now,
            updated_at: now,
        })
    }

    pub fn apply_mutation(
        &mut self,
        name: Option<String>,
        comment: Option<String>,
        is_public: Option<bool>,
        tracks_op: PlaylistTracksOp,
    ) -> DomainResult<()> {
        let now = Timestamp::now();
        self.apply_meta_changes(name, comment, is_public)?;
        match tracks_op {
            PlaylistTracksOp::Keep => {}
            PlaylistTracksOp::Replace(ids) => self.replace_playlist_tracks(ids),
            PlaylistTracksOp::Patch {
                remove_indices,
                append,
            } => {
                if !remove_indices.is_empty() {
                    self.remove_playlist_indices(remove_indices);
                }
                if !append.is_empty() {
                    self.append_playlist_tracks(&append);
                }
            }
        }
        self.updated_at = now;
        Ok(())
    }

    pub fn summarize(&self, track_map: &HashMap<TrackId, Track>) -> (u32, u32, Option<String>) {
        let mut count = 0u32;
        let mut duration_secs = 0u32;
        let mut cover_key = None;
        for tid in &self.track_ids {
            if let Some(tr) = track_map.get(tid) {
                accumulate_playlist_track(tr, &mut count, &mut duration_secs, &mut cover_key);
            }
        }
        (count, duration_secs, cover_key)
    }

    fn apply_meta_changes(
        &mut self,
        name: Option<String>,
        comment: Option<String>,
        is_public: Option<bool>,
    ) -> DomainResult<()> {
        if let Some(n) = name {
            self.name = validate_playlist_name(n)?;
        }
        if let Some(c) = comment {
            self.comment = clean_optional_comment(Some(c));
        }
        if let Some(pub_flag) = is_public {
            self.is_public = pub_flag;
        }
        Ok(())
    }

    fn replace_playlist_tracks(&mut self, track_ids: Vec<TrackId>) {
        self.track_ids = track_ids;
    }

    fn append_playlist_tracks(&mut self, to_add: &[TrackId]) {
        self.track_ids.extend_from_slice(to_add);
    }

    fn remove_playlist_indices(&mut self, mut indices: Vec<usize>) {
        indices.sort_unstable();
        indices.dedup();
        for idx in indices.into_iter().rev() {
            if idx < self.track_ids.len() {
                self.track_ids.remove(idx);
            }
        }
    }
}

fn validate_playlist_name(raw: String) -> DomainResult<String> {
    let trimmed = raw.trim().to_string();
    let char_len = trimmed.chars().count();
    if char_len == 0 || char_len > 128 {
        return Err(DomainError::Validation(
            "playlist name must be between 1 and 128 characters".into(),
        ));
    }
    Ok(trimmed)
}

fn clean_optional_comment(comment: Option<String>) -> Option<String> {
    comment
        .map(|c| c.trim().to_string())
        .filter(|c| !c.is_empty())
}

fn accumulate_playlist_track(
    tr: &Track,
    count: &mut u32,
    duration_secs: &mut u32,
    cover_key: &mut Option<String>,
) {
    if tr.is_missing {
        return;
    }
    *count += 1;
    *duration_secs = duration_secs.saturating_add(tr.duration_secs);
    if cover_key.is_none() {
        *cover_key = tr.cover_key.clone();
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum StarTargetKind {
    Track,
    Album,
    Artist,
}

impl AsRef<str> for StarTargetKind {
    fn as_ref(&self) -> &str {
        match self {
            Self::Track => "track",
            Self::Album => "album",
            Self::Artist => "artist",
        }
    }
}

impl std::fmt::Display for StarTargetKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_ref())
    }
}

impl std::str::FromStr for StarTargetKind {
    type Err = DomainError;

    fn from_str(raw: &str) -> Result<Self, Self::Err> {
        parse_star_target_kind(raw)
    }
}

fn parse_star_target_kind(raw: &str) -> DomainResult<StarTargetKind> {
    match raw.trim().to_ascii_lowercase().as_str() {
        "track" | "song" => Ok(StarTargetKind::Track),
        "album" => Ok(StarTargetKind::Album),
        "artist" => Ok(StarTargetKind::Artist),
        other => Err(DomainError::Validation(format!(
            "unknown star target kind: {other}"
        ))),
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

#[derive(Debug, Clone, Default)]
pub struct UserEngagementSnapshot {
    pub starred: Vec<StarredEntry>,
    pub ratings: Vec<RatingEntry>,
    pub play_stats: Vec<TrackPlayStat>,
}

impl UserEngagementSnapshot {
    pub fn star_map(&self, kind: StarTargetKind) -> HashMap<Uuid, Timestamp> {
        collect_starred_by_kind(&self.starred, kind)
    }

    pub fn rating_map(&self, kind: StarTargetKind) -> HashMap<Uuid, u8> {
        collect_ratings_by_kind(&self.ratings, kind)
    }

    pub fn play_maps(
        &self,
        active_tracks: Option<&HashSet<TrackId>>,
    ) -> (HashMap<TrackId, (u64, Timestamp)>, HashMap<AlbumId, u64>) {
        let track_map = collect_track_play_stats(&self.play_stats, active_tracks);
        let album_map = accumulate_album_plays(&self.play_stats, active_tracks);
        (track_map, album_map)
    }
}

fn collect_starred_by_kind(
    entries: &[StarredEntry],
    kind: StarTargetKind,
) -> HashMap<Uuid, Timestamp> {
    entries
        .iter()
        .filter(|s| s.kind == kind)
        .map(|s| (s.target_id, s.starred_at))
        .collect()
}

fn collect_ratings_by_kind(entries: &[RatingEntry], kind: StarTargetKind) -> HashMap<Uuid, u8> {
    entries
        .iter()
        .filter(|r| r.kind == kind)
        .map(|r| (r.target_id, r.rating))
        .collect()
}

fn collect_track_play_stats(
    stats: &[TrackPlayStat],
    active_tracks: Option<&HashSet<TrackId>>,
) -> HashMap<TrackId, (u64, Timestamp)> {
    stats
        .iter()
        .filter(|st| active_tracks.is_none_or(|set| set.contains(&st.track_id)))
        .map(|st| (st.track_id, (st.play_count, st.last_played_at)))
        .collect()
}

fn accumulate_album_plays(
    stats: &[TrackPlayStat],
    active_tracks: Option<&HashSet<TrackId>>,
) -> HashMap<AlbumId, u64> {
    let mut counts: HashMap<AlbumId, u64> = HashMap::new();
    for st in stats {
        if active_tracks.is_none_or(|set| set.contains(&st.track_id)) {
            *counts.entry(st.album_id).or_default() += st.play_count;
        }
    }
    counts
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

impl PlayQueue {
    pub fn reconcile(
        existing: Option<Self>,
        user_id: UserId,
        track_ids: Vec<TrackId>,
        current_track_id: Option<TrackId>,
        position_ms: u64,
        changed_by: &str,
        now: Timestamp,
    ) -> Self {
        let mut queue = existing.unwrap_or_else(|| Self {
            user_id,
            track_ids: Vec::new(),
            current_track_id: None,
            position_ms: 0,
            changed_by: String::new(),
            updated_at: now,
        });
        queue.apply_queue_state(track_ids, current_track_id, position_ms, changed_by, now);
        queue
    }

    fn apply_queue_state(
        &mut self,
        track_ids: Vec<TrackId>,
        current_track_id: Option<TrackId>,
        position_ms: u64,
        changed_by: &str,
        now: Timestamp,
    ) {
        self.track_ids = track_ids;
        self.current_track_id = current_track_id;
        self.position_ms = position_ms;
        self.changed_by = normalize_changed_by(changed_by);
        self.updated_at = now;
    }
}

fn normalize_changed_by(changed_by: &str) -> String {
    let trimmed = changed_by.trim();
    if trimmed.is_empty() {
        "xmedia-web".to_string()
    } else {
        trimmed.to_string()
    }
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

impl AudioBookmark {
    pub fn new_or_updated(
        existing: Option<&Self>,
        user_id: UserId,
        track_id: TrackId,
        position_ms: u64,
        comment: Option<String>,
        now: Timestamp,
    ) -> Self {
        Self {
            user_id,
            track_id,
            position_ms,
            comment: normalize_bookmark_comment(comment),
            created_at: existing.map(|e| e.created_at).unwrap_or(now),
            updated_at: now,
        }
    }
}

fn normalize_bookmark_comment(comment: Option<String>) -> Option<String> {
    comment
        .map(|c| c.trim().to_string())
        .filter(|c| !c.is_empty())
}

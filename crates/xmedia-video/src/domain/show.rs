use jiff::Timestamp;
use serde::{Deserialize, Serialize};
use uuid::Uuid;
use xmedia_core::domain::error::{DomainError, DomainResult};
use xmedia_core::domain::library::LibraryId;

use crate::domain::id::ShowId;
use crate::domain::video::normalize_string_list;

#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ShowSource {
    #[default]
    Local,
    OnlineTracked {
        source_id: Uuid,
        item_id: String,
        detail_url: Option<String>,
    },
}

impl ShowSource {
    pub fn is_online_tracked(&self) -> bool {
        matches!(self, Self::OnlineTracked { .. })
    }

    pub fn online_tracked(&self) -> Option<(Uuid, &str, Option<&str>)> {
        match self {
            Self::Local => None,
            Self::OnlineTracked {
                source_id,
                item_id,
                detail_url,
            } => Some((*source_id, item_id.as_str(), detail_url.as_deref())),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VideoShow {
    pub id: ShowId,
    pub library_id: LibraryId,
    pub show_key: String,
    #[serde(default)]
    pub source: ShowSource,
    pub title: String,
    pub summary: String,
    pub year: Option<i32>,
    pub genres: Vec<String>,
    pub tags: Vec<String>,
    pub poster_key: Option<String>,
    pub season_count: u32,
    pub episode_count: u32,
    pub created_at: Timestamp,
    pub updated_at: Timestamp,
}

impl VideoShow {
    pub fn compute_show_key(library_id: LibraryId, title: &str) -> String {
        format!("{}:{}", library_id.0, title.trim().to_lowercase())
    }

    fn compute_tracked_unique_key(source_id: Uuid, item_id: &str) -> String {
        format!("tracked:{source_id}:{}", item_id.trim())
    }

    pub fn new_local(
        library_id: LibraryId,
        title: &str,
        year: Option<i32>,
        genres: Vec<String>,
        poster_key: Option<String>,
        now: Timestamp,
    ) -> Self {
        let clean_title = title.trim().to_string();
        let show_key = Self::compute_show_key(library_id, &clean_title);
        Self {
            id: ShowId::new(),
            library_id,
            show_key,
            source: ShowSource::Local,
            title: clean_title,
            summary: String::new(),
            year,
            genres,
            tags: Vec::new(),
            poster_key,
            season_count: 1,
            episode_count: 0,
            created_at: now,
            updated_at: now,
        }
    }

    #[allow(clippy::too_many_arguments)]
    pub fn new_online_tracked(
        library_id: LibraryId,
        source_id: Uuid,
        item_id: String,
        detail_url: Option<String>,
        title: String,
        summary: String,
        year: Option<i32>,
        genres: Vec<String>,
        poster_key: Option<String>,
        now: Timestamp,
    ) -> Self {
        let clean_item_id = item_id.trim().to_string();
        let show_key = Self::compute_tracked_unique_key(source_id, &clean_item_id);
        Self {
            id: ShowId::new(),
            library_id,
            show_key,
            source: ShowSource::OnlineTracked {
                source_id,
                item_id: clean_item_id,
                detail_url: detail_url
                    .map(|u| u.trim().to_string())
                    .filter(|u| !u.is_empty()),
            },
            title: title.trim().to_string(),
            summary: summary.trim().to_string(),
            year,
            genres,
            tags: Vec::new(),
            poster_key,
            season_count: 1,
            episode_count: 0,
            created_at: now,
            updated_at: now,
        }
    }

    pub fn is_online_tracked(&self) -> bool {
        self.source.is_online_tracked()
    }

    pub fn online_tracked(&self) -> Option<(Uuid, &str, Option<&str>)> {
        self.source.online_tracked()
    }

    pub fn set_poster_key(&mut self, poster_key: String, now: Timestamp) {
        self.poster_key = Some(poster_key);
        self.updated_at = now;
    }

    pub fn fill_missing_scan_metadata(
        &mut self,
        year: Option<i32>,
        genres: &[String],
        poster_key: Option<&str>,
        now: Timestamp,
    ) -> bool {
        let mut dirty = false;
        if self.year.is_none() && year.is_some() {
            self.year = year;
            dirty = true;
        }
        if self.genres.is_empty() && !genres.is_empty() {
            self.genres = genres.to_vec();
            dirty = true;
        }
        if self.poster_key.is_none()
            && let Some(pk) = poster_key
        {
            self.poster_key = Some(pk.to_string());
            dirty = true;
        }
        if dirty {
            self.updated_at = now;
        }
        dirty
    }

    pub fn fill_missing_online_metadata(
        &mut self,
        summary: Option<&str>,
        year: Option<i32>,
        poster_key: Option<String>,
        detail_url: Option<String>,
        now: Timestamp,
    ) -> bool {
        let mut dirty = false;
        if self.poster_key.is_none() && poster_key.is_some() {
            self.poster_key = poster_key;
            dirty = true;
        }
        if self.summary.is_empty()
            && let Some(intro) = summary.map(str::trim).filter(|s| !s.is_empty())
        {
            self.summary = intro.to_string();
            dirty = true;
        }
        if self.year.is_none() && year.is_some() {
            self.year = year;
            dirty = true;
        }
        if let ShowSource::OnlineTracked {
            detail_url: ref mut existing_detail,
            ..
        } = self.source
            && existing_detail.is_none()
            && let Some(durl) = detail_url
                .map(|u| u.trim().to_string())
                .filter(|u| !u.is_empty())
        {
            *existing_detail = Some(durl);
            dirty = true;
        }
        if dirty {
            self.updated_at = now;
        }
        dirty
    }

    /// Applies a metadata patch to this show, syncing `show_key` on rename for local shows and
    /// normalizing `genres`/`tags`. Returns `Ok(true)` if the show title changed.
    pub fn apply_patch(&mut self, patch: ShowMetadataPatch) -> DomainResult<bool> {
        let mut title_changed = false;
        if let Some(title) = patch.title {
            let trimmed = title.trim();
            if trimmed.is_empty() {
                return Err(DomainError::Validation("show title cannot be empty".into()));
            }
            if self.title != trimmed {
                self.title = trimmed.to_string();
                if !self.source.is_online_tracked() {
                    self.show_key = Self::compute_show_key(self.library_id, &self.title);
                }
                title_changed = true;
            }
        }
        if let Some(summary) = patch.summary {
            self.summary = summary.trim().to_string();
        }
        if let Some(year) = patch.year {
            self.year = year;
        }
        if let Some(genres) = patch.genres {
            self.genres = normalize_string_list(genres);
        }
        if let Some(tags) = patch.tags {
            self.tags = normalize_string_list(tags);
        }
        self.updated_at = Timestamp::now();
        Ok(title_changed)
    }
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct ShowMetadataPatch {
    pub title: Option<String>,
    pub summary: Option<String>,
    pub year: Option<Option<i32>>,
    pub genres: Option<Vec<String>>,
    pub tags: Option<Vec<String>>,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct ShowQueryFilter {
    pub library_id: Option<LibraryId>,
    pub genre: Option<String>,
    pub tag: Option<String>,
    pub search: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_show(library_id: LibraryId) -> VideoShow {
        let now = Timestamp::now();
        VideoShow {
            id: ShowId::new(),
            library_id,
            show_key: VideoShow::compute_show_key(library_id, "Frieren"),
            source: ShowSource::Local,
            title: "Frieren".into(),
            summary: String::new(),
            year: Some(2023),
            genres: vec!["Anime".into()],
            tags: vec![],
            poster_key: None,
            season_count: 1,
            episode_count: 12,
            created_at: now,
            updated_at: now,
        }
    }

    #[test]
    fn show_apply_patch_syncs_show_key_and_normalizes_genres_and_tags() {
        let lib_id = LibraryId::new();
        let mut show = sample_show(lib_id);

        let changed = show
            .apply_patch(ShowMetadataPatch {
                title: Some("  Frieren: Beyond Journey's End  ".into()),
                summary: Some("  An elf's journey.  ".into()),
                year: Some(Some(2024)),
                genres: Some(vec![
                    " Fantasy ".into(),
                    "fantasy".into(),
                    "".into(),
                    "   ".into(),
                    "Adventure".into(),
                    "ADVENTURE".into(),
                ]),
                tags: Some(vec![
                    " magic ".into(),
                    "Magic".into(),
                    "slice of life".into(),
                    "".into(),
                ]),
            })
            .unwrap();

        assert!(changed);
        assert_eq!(show.title, "Frieren: Beyond Journey's End");
        assert_eq!(
            show.show_key,
            VideoShow::compute_show_key(lib_id, "Frieren: Beyond Journey's End")
        );
        assert_eq!(show.summary, "An elf's journey.");
        assert_eq!(show.year, Some(2024));
        assert_eq!(show.genres, vec!["Fantasy", "Adventure"]);
        assert_eq!(show.tags, vec!["magic", "slice of life"]);

        let unchanged = show
            .apply_patch(ShowMetadataPatch {
                title: Some("Frieren: Beyond Journey's End".into()),
                ..Default::default()
            })
            .unwrap();
        assert!(!unchanged);

        let err = show.apply_patch(ShowMetadataPatch {
            title: Some("   ".into()),
            ..Default::default()
        });
        assert!(matches!(err, Err(DomainError::Validation(_))));
    }

    #[test]
    fn online_tracked_show_uses_typed_show_source_without_key_prefix_parsing() {
        let lib_id = LibraryId::new();
        let src_id = Uuid::now_v7();
        let now = Timestamp::now();
        let mut show = VideoShow::new_online_tracked(
            lib_id,
            src_id,
            "vod-42".into(),
            Some("https://example.com/vod/42".into()),
            "Original Show".into(),
            String::new(),
            None,
            vec![],
            None,
            now,
        );

        assert!(show.is_online_tracked());
        assert_eq!(
            show.online_tracked(),
            Some((src_id, "vod-42", Some("https://example.com/vod/42")))
        );

        let initial_key = show.show_key.clone();
        let title_changed = show
            .apply_patch(ShowMetadataPatch {
                title: Some("Renamed Online Show".into()),
                ..Default::default()
            })
            .unwrap();
        assert!(title_changed);
        assert_eq!(show.title, "Renamed Online Show");
        assert_eq!(show.show_key, initial_key);
        assert!(show.is_online_tracked());
    }
}

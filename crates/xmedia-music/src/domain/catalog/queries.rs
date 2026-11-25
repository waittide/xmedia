use serde::{Deserialize, Serialize};
use xmedia_core::domain::error::{DomainError, DomainResult};
use xmedia_core::domain::library::LibraryId;
use xmedia_core::domain::media::SortOrder;

use super::{AlbumId, ArtistId, TrackId};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum TrackSortField {
    Title,
    Artist,
    Album,
    #[default]
    CreatedAt,
    Year,
    Duration,
    TrackOrder,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TrackQuery {
    pub library_id: Option<LibraryId>,
    pub album_id: Option<AlbumId>,
    pub artist_id: Option<ArtistId>,
    pub genre: Option<String>,
    #[serde(default)]
    pub tag: Option<String>,
    pub year_from: Option<i32>,
    pub year_to: Option<i32>,
    pub search: Option<String>,
    pub sort_by: TrackSortField,
    pub sort_order: SortOrder,
    pub page: u32,
    pub page_size: u32,
    #[serde(skip)]
    pub exclude_online_tracked: bool,
}

impl Default for TrackQuery {
    fn default() -> Self {
        Self {
            library_id: None,
            album_id: None,
            artist_id: None,
            genre: None,
            tag: None,
            year_from: None,
            year_to: None,
            search: None,
            sort_by: TrackSortField::CreatedAt,
            sort_order: SortOrder::Desc,
            page: 1,
            page_size: 50,
            exclude_online_tracked: false,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub enum AlbumListType {
    #[default]
    Newest,
    Recent,
    Frequent,
    Random,
    Starred,
    AlphabeticalByName,
    AlphabeticalByArtist,
    ByYear,
    ByGenre,
}

impl AlbumListType {
    pub fn parse(raw: &str) -> DomainResult<Self> {
        match raw.trim() {
            "newest" => Ok(Self::Newest),
            "recent" => Ok(Self::Recent),
            "frequent" => Ok(Self::Frequent),
            "random" => Ok(Self::Random),
            "starred" => Ok(Self::Starred),
            "alphabeticalByName" | "alphabetical_by_name" => Ok(Self::AlphabeticalByName),
            "alphabeticalByArtist" | "alphabetical_by_artist" => Ok(Self::AlphabeticalByArtist),
            "byYear" | "by_year" => Ok(Self::ByYear),
            "byGenre" | "by_genre" => Ok(Self::ByGenre),
            other => Err(DomainError::Validation(format!(
                "unsupported album list type: {other}"
            ))),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AlbumListQuery {
    pub list_type: AlbumListType,
    pub library_id: Option<LibraryId>,
    pub genre: Option<String>,
    pub from_year: Option<i32>,
    pub to_year: Option<i32>,
    pub search: Option<String>,
    pub offset: u32,
    pub size: u32,
}

impl Default for AlbumListQuery {
    fn default() -> Self {
        Self {
            list_type: AlbumListType::Newest,
            library_id: None,
            genre: None,
            from_year: None,
            to_year: None,
            search: None,
            offset: 0,
            size: 30,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum ArtistRoleFilter {
    #[default]
    All,
    AlbumArtist,
    TrackArtist,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ArtistQuery {
    pub library_id: Option<LibraryId>,
    pub search: Option<String>,
    pub role_filter: ArtistRoleFilter,
    pub offset: u32,
    pub size: u32,
}

impl Default for ArtistQuery {
    fn default() -> Self {
        Self {
            library_id: None,
            search: None,
            role_filter: ArtistRoleFilter::All,
            offset: 0,
            size: 100,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GenreStat {
    pub name: String,
    pub song_count: u64,
    pub album_count: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MusicTagStat {
    pub name: String,
    pub song_count: u64,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct BoundCatalogCounts {
    pub active_track_ids: std::collections::HashSet<TrackId>,
    pub active_album_ids: std::collections::HashSet<AlbumId>,
    pub total_tracks: u64,
    pub total_albums: u64,
    pub total_artists: u64,
    pub genres_count: u64,
    pub libraries_count: u64,
    pub tags_count: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SearchQuery {
    pub query: String,
    pub library_id: Option<LibraryId>,
    pub artist_count: u32,
    pub artist_offset: u32,
    pub album_count: u32,
    pub album_offset: u32,
    pub song_count: u32,
    pub song_offset: u32,
}

impl Default for SearchQuery {
    fn default() -> Self {
        Self {
            query: String::new(),
            library_id: None,
            artist_count: 20,
            artist_offset: 0,
            album_count: 20,
            album_offset: 0,
            song_count: 20,
            song_offset: 0,
        }
    }
}

mod albums_artists;
mod bound_albums_artists;
mod bound_catalog;
mod sql;
mod tags_search;
mod tracks;

use std::collections::HashSet;

use async_trait::async_trait;
use toasty::Db;
use xmedia_core::domain::error::DomainResult;
use xmedia_core::domain::library::LibraryId;

use crate::domain::catalog::{
    Album, AlbumId, Artist, ArtistId, AudioFormat, PagedResult, Track, TrackId, TrackQuery,
};
use crate::domain::collection::StarTargetKind;
use crate::domain::repository::{
    AlbumScope, ArtistScope, CatalogFacetQuery, CatalogFacetResult, CatalogMutation,
    MusicCatalogRepository, TrackDeleteScope, TrackScope,
};
use crate::infra::persistence::models::{MusicAlbumModel, MusicArtistModel, MusicTrackModel};

#[derive(Clone)]
pub struct ToastyMusicCatalogRepository {
    pub(super) db: Db,
}

impl ToastyMusicCatalogRepository {
    pub fn new(db: Db) -> Self {
        Self { db }
    }

    pub(super) fn track_to_domain(m: MusicTrackModel) -> Track {
        let format = m.format.parse::<AudioFormat>().unwrap_or(AudioFormat::Mp3);
        Track {
            id: TrackId(m.id),
            library_id: LibraryId(m.library_id),
            album_id: AlbumId(m.album_id),
            artist_id: ArtistId(m.artist_id),
            album_artist_id: ArtistId(m.album_artist_id),
            title: m.title,
            artist_name: m.artist_name,
            album_artist_name: m.album_artist_name,
            album_title: m.album_title,
            track_number: m.track_number.and_then(|n| u32::try_from(n).ok()),
            disc_number: m.disc_number.and_then(|n| u32::try_from(n).ok()),
            year: m.year.and_then(|y| i32::try_from(y).ok()),
            genre: m.genre,
            duration_secs: u32::try_from(m.duration_secs.max(0)).unwrap_or(0),
            bit_rate_kbps: m.bit_rate_kbps.and_then(|b| u32::try_from(b).ok()),
            sample_rate_hz: m.sample_rate_hz.and_then(|s| u32::try_from(s).ok()),
            bit_depth: m.bit_depth.and_then(|d| u16::try_from(d).ok()),
            channels: m.channels.and_then(|c| u8::try_from(c).ok()),
            format,
            file_size: u64::try_from(m.file_size.max(0)).unwrap_or(0),
            file_hash: m.file_hash,
            replay_gain: m.replay_gain.0,
            has_embedded_lyrics: m.has_embedded_lyrics,
            has_sidecar_lyrics: m.has_sidecar_lyrics,
            is_compilation: m.is_compilation,
            cover_key: m.cover_key,
            source: m.source.0,
            is_missing: m.is_missing,
            created_at: m.created_at,
            updated_at: m.updated_at,
        }
    }

    pub(super) fn album_to_domain(m: MusicAlbumModel) -> Album {
        Album {
            id: AlbumId(m.id),
            library_id: LibraryId(m.library_id),
            album_key: m.album_key,
            title: m.title,
            album_artist_id: ArtistId(m.album_artist_id),
            album_artist_name: m.album_artist_name,
            year: m.year.and_then(|y| i32::try_from(y).ok()),
            genre: m.genre,
            disc_count: u32::try_from(m.disc_count.max(1)).unwrap_or(1),
            track_count: u32::try_from(m.track_count.max(0)).unwrap_or(0),
            duration_secs: u32::try_from(m.duration_secs.max(0)).unwrap_or(0),
            is_compilation: m.is_compilation,
            cover_key: m.cover_key,
            created_at: m.created_at,
            updated_at: m.updated_at,
        }
    }

    pub(super) fn artist_to_domain(m: MusicArtistModel) -> Artist {
        Artist {
            id: ArtistId(m.id),
            name: m.name,
            is_album_artist: m.is_album_artist,
            is_track_artist: m.is_track_artist,
            album_count: u32::try_from(m.album_count.max(0)).unwrap_or(0),
            track_count: u32::try_from(m.track_count.max(0)).unwrap_or(0),
            cover_key: m.cover_key,
            created_at: m.created_at,
            updated_at: m.updated_at,
        }
    }
}

fn unpaged<T>(items: Vec<T>) -> PagedResult<T> {
    let total = items.len() as u64;
    let page_size = u32::try_from(items.len().max(1)).unwrap_or(u32::MAX);
    PagedResult {
        items,
        total,
        page: 1,
        page_size,
    }
}

#[async_trait]
impl MusicCatalogRepository for ToastyMusicCatalogRepository {
    async fn query_tracks(&self, scope: TrackScope<'_>) -> DomainResult<Vec<Track>> {
        match scope {
            TrackScope::Id(id) => Ok(self.find_track_by_id_impl(id).await?.into_iter().collect()),
            TrackScope::Ids(ids) => self.find_tracks_by_ids_impl(ids).await,
            TrackScope::CoverKey(key) => self.find_tracks_by_cover_key_impl(key).await,
            TrackScope::Library(lib_id) => self.list_tracks_by_library_impl(lib_id).await,
            TrackScope::Album { album_id, allowed } => match allowed {
                Some(set) => self.list_bound_tracks_by_album_impl(set, album_id).await,
                None => self.list_tracks_by_album_impl(album_id).await,
            },
            TrackScope::Artist { artist_id, allowed } => match allowed {
                Some(set) => self.list_bound_tracks_by_artist_impl(set, artist_id).await,
                None => self.list_tracks_by_artist_impl(artist_id).await,
            },
            TrackScope::Offset {
                allowed,
                query,
                offset,
                limit,
            } => {
                self.list_bound_tracks_offset_impl(allowed, query, offset, limit)
                    .await
            }
            TrackScope::Random {
                allowed,
                size,
                library_id,
                genre,
                from_year,
                to_year,
            } => {
                self.list_random_bound_tracks_impl(
                    allowed, size, library_id, genre, from_year, to_year,
                )
                .await
            }
        }
    }

    async fn search_tracks(
        &self,
        allowed: Option<&HashSet<TrackId>>,
        query: &TrackQuery,
    ) -> DomainResult<PagedResult<Track>> {
        match allowed {
            Some(set) => self.query_bound_tracks_impl(set, query).await,
            None => self.query_tracks_impl(query).await,
        }
    }

    async fn query_albums(&self, scope: AlbumScope<'_>) -> DomainResult<PagedResult<Album>> {
        match scope {
            AlbumScope::Id(id) => Ok(unpaged(
                self.find_album_by_id_impl(id).await?.into_iter().collect(),
            )),
            AlbumScope::Key(key) => Ok(unpaged(
                self.find_album_by_key_impl(key)
                    .await?
                    .into_iter()
                    .collect(),
            )),
            AlbumScope::Ids(ids) => Ok(unpaged(self.find_albums_by_ids_impl(ids).await?)),
            AlbumScope::CoverKey(key) => {
                Ok(unpaged(self.find_albums_by_cover_key_impl(key).await?))
            }
            AlbumScope::BoundByIds {
                allowed,
                album_ids,
                library_id,
            } => Ok(unpaged(
                self.project_bound_albums_by_ids_impl(allowed, album_ids, library_id)
                    .await?,
            )),
            AlbumScope::BoundByArtist { allowed, artist_id } => Ok(unpaged(
                self.list_bound_albums_by_artist_impl(allowed, artist_id)
                    .await?,
            )),
            AlbumScope::BoundPaged {
                allowed,
                query,
                starred,
                play_stats,
            } => {
                self.query_bound_albums_impl(allowed, query, starred, play_stats)
                    .await
            }
        }
    }

    async fn query_artists(&self, scope: ArtistScope<'_>) -> DomainResult<PagedResult<Artist>> {
        match scope {
            ArtistScope::Id(id) => Ok(unpaged(
                self.find_artist_by_id_impl(id).await?.into_iter().collect(),
            )),
            ArtistScope::Name(name) => Ok(unpaged(
                self.find_artist_by_name_impl(name)
                    .await?
                    .into_iter()
                    .collect(),
            )),
            ArtistScope::Ids(ids) => Ok(unpaged(self.find_artists_by_ids_impl(ids).await?)),
            ArtistScope::BoundByIds {
                allowed,
                artist_ids,
                library_id,
            } => Ok(unpaged(
                self.project_bound_artists_by_ids_impl(allowed, artist_ids, library_id)
                    .await?,
            )),
            ArtistScope::BoundPaged { allowed, query } => {
                self.query_bound_artists_impl(allowed, query).await
            }
        }
    }

    async fn query_facets(&self, query: CatalogFacetQuery<'_>) -> DomainResult<CatalogFacetResult> {
        match query {
            CatalogFacetQuery::ActiveTrackIds => Ok(CatalogFacetResult::TrackIds(
                self.list_all_active_track_ids_impl().await?,
            )),
            CatalogFacetQuery::OverviewStats(allowed) => Ok(CatalogFacetResult::OverviewStats(
                self.get_bound_overview_stats_impl(allowed).await?,
            )),
            CatalogFacetQuery::LibraryTrackCounts(allowed) => {
                Ok(CatalogFacetResult::LibraryTrackCounts(
                    self.count_bound_tracks_by_library_impl(allowed).await?,
                ))
            }
            CatalogFacetQuery::Genres(allowed) => Ok(CatalogFacetResult::Genres(
                self.list_bound_genres_impl(allowed).await?,
            )),
            CatalogFacetQuery::Tags(Some(allowed)) => Ok(CatalogFacetResult::Tags(
                self.list_bound_tags_impl(allowed).await?,
            )),
            CatalogFacetQuery::Tags(None) => {
                Ok(CatalogFacetResult::Tags(self.list_tags_impl().await?))
            }
            CatalogFacetQuery::TrackTags(ids) => Ok(CatalogFacetResult::TrackTags(
                self.get_tags_for_tracks_impl(ids).await?,
            )),
            CatalogFacetQuery::HasBoundTarget { allowed, kind, id } => {
                let exists = match kind {
                    StarTargetKind::Track => {
                        self.has_bound_active_track_impl(allowed, TrackId(id))
                            .await?
                    }
                    StarTargetKind::Album => {
                        self.has_bound_active_album_impl(allowed, AlbumId(id))
                            .await?
                    }
                    StarTargetKind::Artist => {
                        self.has_bound_active_artist_impl(allowed, ArtistId(id))
                            .await?
                    }
                };
                Ok(CatalogFacetResult::Exists(exists))
            }
        }
    }

    async fn save(&self, mutation: CatalogMutation<'_>) -> DomainResult<()> {
        match mutation {
            CatalogMutation::Track(track) => self.save_track_impl(track).await,
            CatalogMutation::Album(album) => self.save_album_impl(album).await,
            CatalogMutation::Artist(artist) => self.save_artist_impl(artist).await,
            CatalogMutation::TrackTags { track_id, tags } => {
                let _ = self.set_track_tags_impl(track_id, tags).await?;
                Ok(())
            }
            CatalogMutation::RecomputeAggregates => self.recompute_catalog_aggregates_impl().await,
        }
    }

    async fn delete_tracks(&self, scope: TrackDeleteScope) -> DomainResult<()> {
        match scope {
            TrackDeleteScope::Id(id) => self.delete_track_impl(id).await,
            TrackDeleteScope::Library(lib_id) => self.delete_tracks_by_library_impl(lib_id).await,
        }
    }
}

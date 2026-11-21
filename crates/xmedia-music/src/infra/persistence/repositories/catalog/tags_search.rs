use std::collections::{BTreeMap, HashMap, HashSet};

use jiff::Timestamp;
use uuid::Uuid;
use xmedia_core::domain::error::DomainResult;

use super::ToastyMusicCatalogRepository;
use crate::domain::catalog::{
    Album, Artist, GenreStat, MusicTagStat, SearchQuery, SearchResult, Track, TrackId,
};
use crate::infra::persistence::models::{
    MusicAlbumModel, MusicArtistModel, MusicTrackModel, MusicTrackTagModel,
};
use crate::infra::persistence::repositories::db_err;

impl ToastyMusicCatalogRepository {
    pub(super) async fn list_genres_impl(&self) -> DomainResult<Vec<GenreStat>> {
        let mut db = self.db.clone();
        let rows = MusicTrackModel::filter_by_is_missing(false)
            .exec(&mut db)
            .await
            .map_err(db_err)?;

        let mut map: BTreeMap<String, (String, u64, HashSet<Uuid>)> = BTreeMap::new();
        for tr in rows {
            if let Some(ref g) = tr.genre {
                let trimmed = g.trim();
                if trimmed.is_empty() {
                    continue;
                }
                let entry = map
                    .entry(trimmed.to_lowercase())
                    .or_insert_with(|| (trimmed.to_string(), 0, HashSet::new()));
                entry.1 += 1;
                entry.2.insert(tr.album_id);
            }
        }

        let mut out: Vec<GenreStat> = map
            .into_values()
            .map(|(display, song_count, albums)| GenreStat {
                name: display,
                song_count,
                album_count: albums.len() as u64,
            })
            .collect();
        out.sort_by(|a, b| {
            b.song_count
                .cmp(&a.song_count)
                .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
        });
        Ok(out)
    }

    pub(super) async fn list_tags_impl(&self) -> DomainResult<Vec<MusicTagStat>> {
        let mut db = self.db.clone();
        let active_ids: HashSet<Uuid> = MusicTrackModel::filter_by_is_missing(false)
            .exec(&mut db)
            .await
            .map_err(db_err)?
            .into_iter()
            .map(|r| r.id)
            .collect();
        let rows = MusicTrackTagModel::all()
            .exec(&mut db)
            .await
            .map_err(db_err)?;

        let mut map: BTreeMap<String, (String, u64)> = BTreeMap::new();
        for row in rows {
            if !active_ids.contains(&row.track_id) {
                continue;
            }
            let entry = map.entry(row.tag_lower).or_insert_with(|| (row.tag, 0));
            entry.1 += 1;
        }

        let mut out: Vec<MusicTagStat> = map
            .into_values()
            .map(|(name, song_count)| MusicTagStat { name, song_count })
            .collect();
        out.sort_by(|a, b| {
            b.song_count
                .cmp(&a.song_count)
                .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
        });
        Ok(out)
    }

    pub(super) async fn count_active_tags_impl(
        &self,
        active_track_ids: &HashSet<TrackId>,
    ) -> DomainResult<u64> {
        if active_track_ids.is_empty() {
            return Ok(0);
        }
        let mut db = self.db.clone();
        let rows = MusicTrackTagModel::all()
            .exec(&mut db)
            .await
            .map_err(db_err)?;
        let mut distinct = HashSet::new();
        for row in rows {
            if active_track_ids.contains(&TrackId(row.track_id)) {
                distinct.insert(row.tag_lower);
            }
        }
        Ok(distinct.len() as u64)
    }

    pub(super) async fn get_tags_for_tracks_impl(
        &self,
        track_ids: &[TrackId],
    ) -> DomainResult<HashMap<TrackId, Vec<String>>> {
        if track_ids.is_empty() {
            return Ok(HashMap::new());
        }
        let mut db = self.db.clone();
        let mut map: HashMap<TrackId, Vec<String>> = HashMap::new();
        for chunk in track_ids.chunks(500) {
            let raw_ids: Vec<Uuid> = chunk.iter().map(|i| i.0).collect();
            let rows = MusicTrackTagModel::filter(
                MusicTrackTagModel::fields().track_id().in_list(raw_ids),
            )
            .exec(&mut db)
            .await
            .map_err(db_err)?;
            for row in rows {
                map.entry(TrackId(row.track_id)).or_default().push(row.tag);
            }
        }
        for list in map.values_mut() {
            list.sort_by_key(|t| t.to_lowercase());
        }
        Ok(map)
    }

    pub(super) async fn set_track_tags_impl(
        &self,
        track_id: TrackId,
        tags: Vec<String>,
    ) -> DomainResult<Vec<String>> {
        let mut db = self.db.clone();
        MusicTrackTagModel::filter_by_track_id(track_id.0)
            .delete()
            .exec(&mut db)
            .await
            .map_err(db_err)?;

        let now = Timestamp::now();
        let mut seen = HashSet::new();
        let mut saved = Vec::new();
        for raw in tags {
            let clean = raw.trim();
            if clean.is_empty() {
                continue;
            }
            let lower = clean.to_lowercase();
            if !seen.insert(lower.clone()) {
                continue;
            }
            let key = format!("{}:{}", track_id.0, lower);
            toasty::create!(MusicTrackTagModel {
                composite_key: key,
                track_id: track_id.0,
                tag_lower: lower,
                tag: clean.to_string(),
                created_at: now,
            })
            .exec(&mut db)
            .await
            .map_err(db_err)?;
            saved.push(clean.to_string());
        }
        saved.sort_by_key(|t| t.to_lowercase());
        Ok(saved)
    }

    pub(super) async fn search_catalog_impl(
        &self,
        query: &SearchQuery,
    ) -> DomainResult<SearchResult> {
        let mut db = self.db.clone();
        let q_lower = query.query.trim().trim_matches('"').to_lowercase();

        let artist_rows = MusicArtistModel::all()
            .exec(&mut db)
            .await
            .map_err(db_err)?;
        let mut artists: Vec<Artist> = artist_rows
            .into_iter()
            .map(Self::artist_to_domain)
            .filter(|a| a.track_count > 0 || a.album_count > 0)
            .filter(|a| q_lower.is_empty() || a.name.to_lowercase().contains(&q_lower))
            .collect();
        artists.sort_by_key(|a| a.name.to_lowercase());
        let artists = artists
            .into_iter()
            .skip(query.artist_offset as usize)
            .take(query.artist_count as usize)
            .collect();

        let mut album_builder = MusicAlbumModel::all();
        if let Some(lib_id) = query.library_id {
            album_builder =
                album_builder.filter(MusicAlbumModel::fields().library_id().eq(lib_id.0));
        }
        let album_rows = album_builder.exec(&mut db).await.map_err(db_err)?;
        let mut albums: Vec<Album> = album_rows
            .into_iter()
            .map(Self::album_to_domain)
            .filter(|a| a.track_count > 0)
            .filter(|a| {
                q_lower.is_empty()
                    || a.title.to_lowercase().contains(&q_lower)
                    || a.album_artist_name.to_lowercase().contains(&q_lower)
            })
            .collect();
        albums.sort_by_key(|a| a.title.to_lowercase());
        let albums = albums
            .into_iter()
            .skip(query.album_offset as usize)
            .take(query.album_count as usize)
            .collect();

        let mut track_builder =
            MusicTrackModel::all().filter(MusicTrackModel::fields().is_missing().eq(false));
        if let Some(lib_id) = query.library_id {
            track_builder =
                track_builder.filter(MusicTrackModel::fields().library_id().eq(lib_id.0));
        }
        let track_rows = track_builder.exec(&mut db).await.map_err(db_err)?;
        let mut tracks: Vec<Track> = track_rows
            .into_iter()
            .map(Self::track_to_domain)
            .filter(|t| {
                q_lower.is_empty()
                    || t.title.to_lowercase().contains(&q_lower)
                    || t.artist_name.to_lowercase().contains(&q_lower)
                    || t.album_title.to_lowercase().contains(&q_lower)
            })
            .collect();
        tracks.sort_by_key(|t| t.title.to_lowercase());
        let tracks = tracks
            .into_iter()
            .skip(query.song_offset as usize)
            .take(query.song_count as usize)
            .collect();

        Ok(SearchResult {
            artists,
            albums,
            tracks,
        })
    }
}

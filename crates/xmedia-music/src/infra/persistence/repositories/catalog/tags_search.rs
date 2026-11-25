use std::collections::{BTreeMap, HashMap, HashSet};

use jiff::Timestamp;
use uuid::Uuid;
use xmedia_core::domain::error::DomainResult;

use super::ToastyMusicCatalogRepository;
use crate::domain::catalog::{MusicTagStat, TrackId};
use crate::infra::persistence::models::{MusicTrackModel, MusicTrackTagModel};
use crate::infra::persistence::repositories::db_err;

impl ToastyMusicCatalogRepository {
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
}

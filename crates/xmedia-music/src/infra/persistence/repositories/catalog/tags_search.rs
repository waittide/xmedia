use std::collections::{HashMap, HashSet};

use jiff::Timestamp;
use uuid::Uuid;
use xmedia_core::domain::error::DomainResult;

use super::ToastyMusicCatalogRepository;
use super::sql::{SqlBuilder, row_fields, val_i64, val_string};
use crate::domain::catalog::{MusicTagStat, TrackId};
use crate::infra::persistence::models::MusicTrackTagModel;
use crate::infra::persistence::repositories::db_err;

impl ToastyMusicCatalogRepository {
    pub(super) async fn list_tags_impl(&self) -> DomainResult<Vec<MusicTagStat>> {
        let mut db = self.db.clone();
        let mut b = SqlBuilder::new(&db);
        let not_missing = b.not_missing("t.is_missing");
        let sql = format!(
            "SELECT MIN(tt.tag), COUNT(*) \
             FROM music_track_tags tt INNER JOIN music_tracks t ON t.id = tt.track_id \
             WHERE {not_missing} \
             GROUP BY tt.tag_lower \
             ORDER BY COUNT(*) DESC, tt.tag_lower ASC"
        );
        let rows = b.exec_rows(sql, &mut db).await?;
        let mut out = Vec::with_capacity(rows.len());
        for row in &rows {
            let f = row_fields(row)?;
            if f.len() >= 2 {
                out.push(MusicTagStat {
                    name: val_string(&f[0])?,
                    song_count: u64::try_from(val_i64(&f[1])?.max(0)).unwrap_or(0),
                });
            }
        }
        Ok(out)
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

use std::collections::HashSet;

use jiff::Timestamp;
use toasty::Db;
use toasty::stmt::Value;
use uuid::Uuid;
use xmedia_core::domain::error::{DomainError, DomainResult};
use xmedia_core::domain::library::LibraryId;

use crate::domain::catalog::{
    Album, AlbumId, Artist, ArtistId, AudioFormat, ReplayGain, SortOrder, Track, TrackId,
    TrackQuery, TrackSortField, TrackSource,
};
use crate::infra::persistence::repositories::db_err;

const INLINE_UUID_IN_LIMIT: usize = 500;

pub(super) const TRACK_SELECT_COLUMNS: &str = "t.id, t.library_id, t.album_id, t.artist_id, \
    t.album_artist_id, t.title, t.artist_name, t.album_artist_name, t.album_title, \
    t.track_number, t.disc_number, t.year, t.genre, t.duration_secs, t.bit_rate_kbps, \
    t.sample_rate_hz, t.bit_depth, t.channels, t.format, t.file_size, t.file_hash, \
    t.replay_gain, t.has_embedded_lyrics, t.has_sidecar_lyrics, t.is_compilation, \
    t.cover_key, t.source, t.is_missing, t.created_at, t.updated_at";

pub(super) const PROJECTED_ALBUM_SELECT_COLUMNS: &str = "a.id, a.library_id, a.album_key, \
    a.title, a.album_artist_id, a.album_artist_name, a.year, a.genre, \
    MAX(COALESCE(t.disc_number, 1)), COUNT(t.id), COALESCE(SUM(t.duration_secs), 0), \
    a.is_compilation, a.cover_key, a.created_at, a.updated_at";

#[derive(Default)]
pub(super) struct SqlBuilder {
    params: Vec<Value>,
}

impl SqlBuilder {
    pub(super) fn bind_val(&mut self, val: impl Into<Value>) -> String {
        self.params.push(val.into());
        format!("?{}", self.params.len())
    }

    pub(super) fn bind_uuid(&mut self, id: Uuid) -> String {
        self.bind_val(id.as_bytes().to_vec())
    }

    pub(super) fn bind_uuid_set<I>(&mut self, col: &str, uuids: I, count: usize) -> String
    where
        I: IntoIterator<Item = Uuid>,
    {
        if count <= INLINE_UUID_IN_LIMIT {
            let placeholders: Vec<String> = uuids.into_iter().map(|u| self.bind_uuid(u)).collect();
            format!("{col} IN ({})", placeholders.join(", "))
        } else {
            let hex_list: Vec<String> = uuids.into_iter().map(|u| u.simple().to_string()).collect();
            let json_arr = serde_json::to_string(&hex_list).unwrap_or_else(|_| "[]".to_string());
            let p = self.bind_val(json_arr);
            format!("{col} IN (SELECT unhex(value) FROM json_each({p}))")
        }
    }

    pub(super) fn bind_track_scope(&mut self, col: &str, allowed: &HashSet<TrackId>) -> String {
        self.bind_uuid_set(col, allowed.iter().map(|t| t.0), allowed.len())
    }

    pub(super) async fn exec_rows(self, sql: String, db: &mut Db) -> DomainResult<Vec<Value>> {
        let mut query = toasty::sql::query(sql);
        for param in self.params {
            query = query.bind(param);
        }
        query.exec(db).await.map_err(db_err)
    }
}

pub(super) fn append_track_query_conditions(
    b: &mut SqlBuilder,
    conds: &mut Vec<String>,
    allowed: Option<&HashSet<TrackId>>,
    query: &TrackQuery,
) {
    conds.push("t.is_missing = 0".to_string());
    if let Some(set) = allowed {
        conds.push(b.bind_track_scope("t.id", set));
    }
    if query.exclude_online_tracked {
        conds.push("COALESCE(json_extract(t.source, '$.kind'), '') != 'online_tracked'".into());
    }
    if let Some(lib_id) = query.library_id {
        let p = b.bind_uuid(lib_id.0);
        conds.push(format!("t.library_id = {p}"));
    }
    if let Some(alb_id) = query.album_id {
        let p = b.bind_uuid(alb_id.0);
        conds.push(format!("t.album_id = {p}"));
    }
    if let Some(ar_id) = query.artist_id {
        let p = b.bind_uuid(ar_id.0);
        conds.push(format!("(t.artist_id = {p} OR t.album_artist_id = {p})"));
    }
    if let Some(tag_lower) = query
        .tag
        .as_ref()
        .map(|t| t.trim().to_lowercase())
        .filter(|t| !t.is_empty())
    {
        let p = b.bind_val(tag_lower);
        conds.push(format!(
            "EXISTS (SELECT 1 FROM music_track_tags tt WHERE tt.track_id = t.id AND tt.tag_lower = {p})"
        ));
    }
    if let Some(genre_lower) = query
        .genre
        .as_ref()
        .map(|g| g.trim().to_lowercase())
        .filter(|g| !g.is_empty())
    {
        let p = b.bind_val(genre_lower);
        conds.push(format!("LOWER(TRIM(COALESCE(t.genre, ''))) = {p}"));
    }
    if let Some(fy) = query.year_from {
        let p = b.bind_val(i64::from(fy));
        conds.push(format!("(t.year IS NOT NULL AND t.year >= {p})"));
    }
    if let Some(ty) = query.year_to {
        let p = b.bind_val(i64::from(ty));
        conds.push(format!("(t.year IS NOT NULL AND t.year <= {p})"));
    }
    if let Some(kw_lower) = query
        .search
        .as_ref()
        .map(|s| s.trim().to_lowercase())
        .filter(|s| !s.is_empty())
    {
        let p = b.bind_val(kw_lower);
        conds.push(format!(
            "(INSTR(LOWER(t.title), {p}) > 0 OR INSTR(LOWER(t.artist_name), {p}) > 0 OR INSTR(LOWER(t.album_title), {p}) > 0)"
        ));
    }
}

pub(super) fn track_order_by_sql(sort_by: TrackSortField, sort_order: SortOrder) -> &'static str {
    match (sort_by, sort_order) {
        (TrackSortField::Title, SortOrder::Asc) => "LOWER(t.title) ASC, t.id ASC",
        (TrackSortField::Title, SortOrder::Desc) => "LOWER(t.title) DESC, t.id DESC",
        (TrackSortField::Artist, SortOrder::Asc) => {
            "LOWER(t.artist_name) ASC, LOWER(t.title) ASC, t.id ASC"
        }
        (TrackSortField::Artist, SortOrder::Desc) => {
            "LOWER(t.artist_name) DESC, LOWER(t.title) DESC, t.id DESC"
        }
        (TrackSortField::Album, SortOrder::Asc) => {
            "LOWER(t.album_title) ASC, COALESCE(t.disc_number, 1) ASC, COALESCE(t.track_number, 4294967295) ASC"
        }
        (TrackSortField::Album, SortOrder::Desc) => {
            "LOWER(t.album_title) DESC, COALESCE(t.disc_number, 1) DESC, COALESCE(t.track_number, 4294967295) DESC"
        }
        (TrackSortField::CreatedAt, SortOrder::Asc) => "t.created_at ASC, t.id ASC",
        (TrackSortField::CreatedAt, SortOrder::Desc) => "t.created_at DESC, t.id DESC",
        (TrackSortField::Year, SortOrder::Asc) => "t.year ASC, LOWER(t.title) ASC",
        (TrackSortField::Year, SortOrder::Desc) => "t.year DESC, LOWER(t.title) DESC",
        (TrackSortField::Duration, SortOrder::Asc) => "t.duration_secs ASC, LOWER(t.title) ASC",
        (TrackSortField::Duration, SortOrder::Desc) => "t.duration_secs DESC, LOWER(t.title) DESC",
        (TrackSortField::TrackOrder, SortOrder::Asc) => {
            "COALESCE(t.disc_number, 1) ASC, COALESCE(t.track_number, 4294967295) ASC, LOWER(t.title) ASC"
        }
        (TrackSortField::TrackOrder, SortOrder::Desc) => {
            "COALESCE(t.disc_number, 1) DESC, COALESCE(t.track_number, 4294967295) DESC, LOWER(t.title) DESC"
        }
    }
}

pub(super) fn row_fields(row: &Value) -> DomainResult<&[Value]> {
    match row {
        Value::Record(rec) => Ok(rec.as_slice()),
        other => Err(DomainError::Persistence(format!(
            "unexpected SQL row value: {other:?}"
        ))),
    }
}

pub(super) fn val_uuid(val: &Value) -> DomainResult<Uuid> {
    match val {
        Value::Bytes(bytes) => Uuid::from_slice(bytes)
            .map_err(|e| DomainError::Persistence(format!("invalid SQLite UUID blob: {e}"))),
        Value::Uuid(u) => Ok(*u),
        Value::String(s) => Uuid::parse_str(s)
            .map_err(|e| DomainError::Persistence(format!("invalid SQLite UUID text: {e}"))),
        other => Err(DomainError::Persistence(format!(
            "expected UUID column, got {other:?}"
        ))),
    }
}

pub(super) fn val_i64(val: &Value) -> DomainResult<i64> {
    match val {
        Value::I64(v) => Ok(*v),
        Value::U64(v) => Ok(*v as i64),
        Value::I32(v) => Ok(i64::from(*v)),
        Value::U32(v) => Ok(i64::from(*v)),
        other => Err(DomainError::Persistence(format!(
            "expected integer column, got {other:?}"
        ))),
    }
}

pub(super) fn val_opt_i64(val: &Value) -> DomainResult<Option<i64>> {
    match val {
        Value::Null => Ok(None),
        other => val_i64(other).map(Some),
    }
}

pub(super) fn val_bool(val: &Value) -> DomainResult<bool> {
    match val {
        Value::Bool(b) => Ok(*b),
        Value::I64(n) => Ok(*n != 0),
        other => Err(DomainError::Persistence(format!(
            "expected boolean column, got {other:?}"
        ))),
    }
}

pub(super) fn val_string(val: &Value) -> DomainResult<String> {
    match val {
        Value::String(s) => Ok(s.clone()),
        other => Err(DomainError::Persistence(format!(
            "expected text column, got {other:?}"
        ))),
    }
}

pub(super) fn val_opt_string(val: &Value) -> DomainResult<Option<String>> {
    match val {
        Value::Null => Ok(None),
        other => val_string(other).map(Some),
    }
}

pub(super) fn val_timestamp(val: &Value) -> DomainResult<Timestamp> {
    match val {
        Value::Timestamp(ts) => Ok(*ts),
        Value::String(s) => s
            .parse::<Timestamp>()
            .map_err(|e| DomainError::Persistence(format!("invalid SQLite timestamp '{s}': {e}"))),
        other => Err(DomainError::Persistence(format!(
            "expected timestamp column, got {other:?}"
        ))),
    }
}

pub(super) fn row_to_track(row: &Value) -> DomainResult<Track> {
    let f = row_fields(row)?;
    if f.len() < 30 {
        return Err(DomainError::Persistence(format!(
            "expected 30 music_tracks columns, got {}",
            f.len()
        )));
    }
    let format_raw = val_string(&f[18])?;
    let format = AudioFormat::parse(&format_raw).unwrap_or(AudioFormat::Mp3);
    let rg_json = val_string(&f[21])?;
    let replay_gain: ReplayGain = serde_json::from_str(&rg_json).unwrap_or_default();
    let source_json = val_string(&f[26])?;
    let source: TrackSource = serde_json::from_str(&source_json)
        .map_err(|e| DomainError::Persistence(format!("corrupt track source json: {e}")))?;

    Ok(Track {
        id: TrackId(val_uuid(&f[0])?),
        library_id: LibraryId(val_uuid(&f[1])?),
        album_id: AlbumId(val_uuid(&f[2])?),
        artist_id: ArtistId(val_uuid(&f[3])?),
        album_artist_id: ArtistId(val_uuid(&f[4])?),
        title: val_string(&f[5])?,
        artist_name: val_string(&f[6])?,
        album_artist_name: val_string(&f[7])?,
        album_title: val_string(&f[8])?,
        track_number: val_opt_i64(&f[9])?.and_then(|n| u32::try_from(n).ok()),
        disc_number: val_opt_i64(&f[10])?.and_then(|n| u32::try_from(n).ok()),
        year: val_opt_i64(&f[11])?.and_then(|y| i32::try_from(y).ok()),
        genre: val_opt_string(&f[12])?,
        duration_secs: u32::try_from(val_i64(&f[13])?.max(0)).unwrap_or(0),
        bit_rate_kbps: val_opt_i64(&f[14])?.and_then(|b| u32::try_from(b).ok()),
        sample_rate_hz: val_opt_i64(&f[15])?.and_then(|s| u32::try_from(s).ok()),
        bit_depth: val_opt_i64(&f[16])?.and_then(|d| u16::try_from(d).ok()),
        channels: val_opt_i64(&f[17])?.and_then(|c| u8::try_from(c).ok()),
        format,
        file_size: u64::try_from(val_i64(&f[19])?.max(0)).unwrap_or(0),
        file_hash: val_string(&f[20])?,
        replay_gain,
        has_embedded_lyrics: val_bool(&f[22])?,
        has_sidecar_lyrics: val_bool(&f[23])?,
        is_compilation: val_bool(&f[24])?,
        cover_key: val_opt_string(&f[25])?,
        source,
        is_missing: val_bool(&f[27])?,
        created_at: val_timestamp(&f[28])?,
        updated_at: val_timestamp(&f[29])?,
    })
}

pub(super) fn row_to_projected_album(row: &Value) -> DomainResult<Album> {
    let f = row_fields(row)?;
    if f.len() < 15 {
        return Err(DomainError::Persistence(format!(
            "expected 15 projected music_albums columns, got {}",
            f.len()
        )));
    }
    Ok(Album {
        id: AlbumId(val_uuid(&f[0])?),
        library_id: LibraryId(val_uuid(&f[1])?),
        album_key: val_string(&f[2])?,
        title: val_string(&f[3])?,
        album_artist_id: ArtistId(val_uuid(&f[4])?),
        album_artist_name: val_string(&f[5])?,
        year: val_opt_i64(&f[6])?.and_then(|y| i32::try_from(y).ok()),
        genre: val_opt_string(&f[7])?,
        disc_count: u32::try_from(val_i64(&f[8])?.max(1)).unwrap_or(1),
        track_count: u32::try_from(val_i64(&f[9])?.max(0)).unwrap_or(0),
        duration_secs: u32::try_from(val_i64(&f[10])?.max(0)).unwrap_or(0),
        is_compilation: val_bool(&f[11])?,
        cover_key: val_opt_string(&f[12])?,
        created_at: val_timestamp(&f[13])?,
        updated_at: val_timestamp(&f[14])?,
    })
}

pub(super) fn row_to_projected_artist(row: &Value) -> DomainResult<Artist> {
    let f = row_fields(row)?;
    if f.len() < 9 {
        return Err(DomainError::Persistence(format!(
            "expected 9 projected music_artists columns, got {}",
            f.len()
        )));
    }
    Ok(Artist {
        id: ArtistId(val_uuid(&f[0])?),
        name: val_string(&f[1])?,
        is_album_artist: val_bool(&f[2])?,
        is_track_artist: val_bool(&f[3])?,
        album_count: u32::try_from(val_i64(&f[4])?.max(0)).unwrap_or(0),
        track_count: u32::try_from(val_i64(&f[5])?.max(0)).unwrap_or(0),
        cover_key: val_opt_string(&f[6])?,
        created_at: val_timestamp(&f[7])?,
        updated_at: val_timestamp(&f[8])?,
    })
}

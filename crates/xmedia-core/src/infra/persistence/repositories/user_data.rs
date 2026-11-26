use std::collections::HashSet;

use async_trait::async_trait;
use jiff::Timestamp;
use toasty::Db;
use uuid::Uuid;

use crate::domain::activity::{UserActivityDaily, UserActivityRepository};
use crate::domain::error::{DomainError, DomainResult};
use crate::domain::library::MediaKind;
use crate::domain::online_item::{OnlineItemSourceKind, OnlineLibraryItem};
use crate::domain::repository::{
    BindingCountQuery, BindingDeleteScope, OnlineItemDeleteScope, OnlineLibraryItemRepository,
    UserMediaBindingRepository,
};
use crate::domain::user::UserId;

use super::super::models::{OnlineLibraryItemModel, UserActivityDailyModel, UserMediaBindingModel};

#[derive(Clone)]
pub struct ToastyOnlineLibraryItemRepository {
    db: Db,
}

impl ToastyOnlineLibraryItemRepository {
    pub fn new(db: Db) -> Self {
        Self { db }
    }

    fn model_to_domain(m: OnlineLibraryItemModel) -> DomainResult<OnlineLibraryItem> {
        Ok(OnlineLibraryItem {
            id: m.id,
            user_id: UserId(m.user_id),
            media_kind: MediaKind::parse(&m.media_kind)?,
            source_kind: OnlineItemSourceKind::parse(&m.source_kind)?,
            source_id: m.source_id,
            item_key: m.item_key,
            title: m.title,
            subtitle: m.subtitle,
            cover_url: m.cover_url,
            intro: m.intro,
            kind: m.kind,
            extra: m.extra.0,
            favorite: m.favorite,
            favorited_at: m.favorited_at,
            progress: m.progress.0,
            progress_at: m.progress_at,
            created_at: m.created_at,
            updated_at: m.updated_at,
        })
    }
}

fn online_item_err(action: &str, e: impl std::fmt::Display) -> DomainError {
    DomainError::Persistence(format!("{action} online library item failed: {e}"))
}

#[async_trait]
impl OnlineLibraryItemRepository for ToastyOnlineLibraryItemRepository {
    async fn list_by_user(
        &self,
        user_id: UserId,
        media_kind: MediaKind,
    ) -> DomainResult<Vec<OnlineLibraryItem>> {
        let mut db = self.db.clone();
        let rows = OnlineLibraryItemModel::filter_by_user_id(user_id.0)
            .exec(&mut db)
            .await
            .map_err(|e| online_item_err("list", e))?;
        let wanted = media_kind.as_str();
        rows.into_iter()
            .filter(|m| m.media_kind == wanted)
            .map(Self::model_to_domain)
            .collect()
    }

    async fn find_by_id(&self, id: Uuid) -> DomainResult<Option<OnlineLibraryItem>> {
        let mut db = self.db.clone();
        let row = OnlineLibraryItemModel::filter_by_id(id)
            .first()
            .exec(&mut db)
            .await
            .map_err(|e| online_item_err("find", e))?;
        row.map(Self::model_to_domain).transpose()
    }

    async fn save(&self, item: &OnlineLibraryItem) -> DomainResult<()> {
        let mut db = self.db.clone();
        let existing = OnlineLibraryItemModel::filter_by_id(item.id)
            .first()
            .exec(&mut db)
            .await
            .map_err(|e| online_item_err("load", e))?;
        if let Some(mut rec) = existing {
            rec.update()
                .title(item.title.clone())
                .subtitle(item.subtitle.clone())
                .cover_url(item.cover_url.clone())
                .intro(item.intro.clone())
                .kind(item.kind.clone())
                .extra(toasty::Json(item.extra.clone()))
                .favorite(item.favorite)
                .favorited_at(item.favorited_at)
                .progress(toasty::Json(item.progress.clone()))
                .progress_at(item.progress_at)
                .updated_at(item.updated_at)
                .exec(&mut db)
                .await
                .map_err(|e| online_item_err("update", e))?;
        } else {
            toasty::create!(OnlineLibraryItemModel {
                id: item.id,
                user_id: item.user_id.0,
                media_kind: item.media_kind.as_str().to_string(),
                source_kind: item.source_kind.as_str().to_string(),
                source_id: item.source_id.clone(),
                item_key: item.item_key.clone(),
                title: item.title.clone(),
                subtitle: item.subtitle.clone(),
                cover_url: item.cover_url.clone(),
                intro: item.intro.clone(),
                kind: item.kind.clone(),
                extra: toasty::Json(item.extra.clone()),
                favorite: item.favorite,
                favorited_at: item.favorited_at,
                progress: toasty::Json(item.progress.clone()),
                progress_at: item.progress_at,
                created_at: item.created_at,
                updated_at: item.updated_at,
            })
            .exec(&mut db)
            .await
            .map_err(|e| online_item_err("create", e))?;
        }
        Ok(())
    }

    async fn delete_scope(&self, scope: OnlineItemDeleteScope) -> DomainResult<()> {
        let mut db = self.db.clone();
        match scope {
            OnlineItemDeleteScope::Id(id) => {
                OnlineLibraryItemModel::filter_by_id(id)
                    .delete()
                    .exec(&mut db)
                    .await
                    .map_err(|e| online_item_err("delete", e))?;
            }
            OnlineItemDeleteScope::User(user_id) => {
                OnlineLibraryItemModel::filter_by_user_id(user_id.0)
                    .delete()
                    .exec(&mut db)
                    .await
                    .map_err(|e| online_item_err("purge", e))?;
            }
        }
        Ok(())
    }
}

#[derive(Clone)]
pub struct ToastyUserActivityRepository {
    db: Db,
}

impl ToastyUserActivityRepository {
    pub fn new(db: Db) -> Self {
        Self { db }
    }

    fn model_to_domain(m: UserActivityDailyModel) -> DomainResult<UserActivityDaily> {
        Ok(UserActivityDaily {
            id: m.id,
            user_id: UserId(m.user_id),
            media_kind: MediaKind::parse(&m.media_kind)?,
            activity_date: m.activity_date,
            duration_secs: u64::try_from(m.duration_secs.max(0)).unwrap_or(0),
            active_sessions: u32::try_from(m.active_sessions.max(0)).unwrap_or(0),
            last_item_title: m.last_item_title,
            updated_at: m.updated_at,
        })
    }
}

fn activity_err(action: &str, e: impl std::fmt::Display) -> DomainError {
    DomainError::Persistence(format!("{action} user activity failed: {e}"))
}

#[async_trait]
impl UserActivityRepository for ToastyUserActivityRepository {
    async fn list_by_user_and_domain(
        &self,
        user_id: UserId,
        media_kind: MediaKind,
    ) -> DomainResult<Vec<UserActivityDaily>> {
        let mut db = self.db.clone();
        let rows = UserActivityDailyModel::filter_by_user_id(user_id.0)
            .exec(&mut db)
            .await
            .map_err(|e| activity_err("list", e))?;
        let wanted = media_kind.as_str();
        rows.into_iter()
            .filter(|m| m.media_kind == wanted)
            .map(Self::model_to_domain)
            .collect()
    }

    async fn find_by_key(
        &self,
        user_id: UserId,
        media_kind: MediaKind,
        activity_date: &str,
    ) -> DomainResult<Option<UserActivityDaily>> {
        let mut db = self.db.clone();
        let rows = UserActivityDailyModel::filter_by_user_id(user_id.0)
            .exec(&mut db)
            .await
            .map_err(|e| activity_err("find", e))?;
        let wanted_kind = media_kind.as_str();
        rows.into_iter()
            .find(|m| m.media_kind == wanted_kind && m.activity_date == activity_date)
            .map(Self::model_to_domain)
            .transpose()
    }

    async fn save(&self, record: &UserActivityDaily) -> DomainResult<()> {
        let mut db = self.db.clone();
        let duration_i64 = i64::try_from(record.duration_secs).unwrap_or(i64::MAX);
        let sessions_i64 = i64::from(record.active_sessions);
        let existing = UserActivityDailyModel::filter_by_id(record.id)
            .first()
            .exec(&mut db)
            .await
            .map_err(|e| activity_err("probe", e))?;

        if let Some(mut rec) = existing {
            rec.update()
                .duration_secs(duration_i64)
                .active_sessions(sessions_i64)
                .last_item_title(record.last_item_title.clone())
                .updated_at(record.updated_at)
                .exec(&mut db)
                .await
                .map_err(|e| activity_err("update", e))?;
        } else {
            toasty::create!(UserActivityDailyModel {
                id: record.id,
                user_id: record.user_id.0,
                media_kind: record.media_kind.as_str().to_string(),
                activity_date: record.activity_date.clone(),
                duration_secs: duration_i64,
                active_sessions: sessions_i64,
                last_item_title: record.last_item_title.clone(),
                updated_at: record.updated_at,
            })
            .exec(&mut db)
            .await
            .map_err(|e| activity_err("create", e))?;
        }
        Ok(())
    }

    async fn delete_by_user(&self, user_id: UserId) -> DomainResult<()> {
        let mut db = self.db.clone();
        UserActivityDailyModel::filter_by_user_id(user_id.0)
            .delete()
            .exec(&mut db)
            .await
            .map_err(|e| activity_err("purge", e))?;
        Ok(())
    }
}

#[derive(Clone)]
pub struct ToastyUserMediaBindingRepository {
    db: Db,
}

impl ToastyUserMediaBindingRepository {
    pub fn new(db: Db) -> Self {
        Self { db }
    }

    async fn count_user_items(&self, user_id: UserId, media_kind: MediaKind) -> DomainResult<u64> {
        let mut db = self.db.clone();
        let cap = db.capability();
        let dialect = cap.sql.unwrap_or(toasty::Dialect::Sqlite);
        let (p1, p2) = match cap
            .sql_placeholder
            .unwrap_or(toasty::SqlPlaceholder::NumberedQuestionMark)
        {
            toasty::SqlPlaceholder::NumberedQuestionMark => ("?1", "?2"),
            toasty::SqlPlaceholder::DollarNumber => ("$1", "$2"),
            toasty::SqlPlaceholder::QuestionMark => ("?", "?"),
        };
        let uid_val: toasty::stmt::Value = match dialect {
            toasty::Dialect::Sqlite => user_id.0.as_bytes().to_vec().into(),
            _ => user_id.0.into(),
        };
        let sql = format!(
            "SELECT COUNT(DISTINCT item_id) FROM user_media_bindings WHERE user_id = {p1} AND media_kind = {p2}"
        );
        let rows = toasty::sql::query(sql)
            .bind(uid_val)
            .bind(media_kind.as_str().to_string())
            .exec(&mut db)
            .await
            .map_err(|e| binding_err("count user items", e))?;
        Ok(extract_sql_u64(&rows))
    }

    async fn count_item_users(&self, media_kind: MediaKind, item_id: Uuid) -> DomainResult<u64> {
        let mut db = self.db.clone();
        let rows = UserMediaBindingModel::filter_by_item_id(item_id)
            .exec(&mut db)
            .await
            .map_err(|e| binding_err("count", e))?;
        let wanted = media_kind.as_str();
        let unique_users: HashSet<Uuid> = rows
            .into_iter()
            .filter(|m| m.media_kind == wanted)
            .map(|m| m.user_id)
            .collect();
        Ok(unique_users.len() as u64)
    }
}

fn extract_sql_u64(rows: &[toasty::stmt::Value]) -> u64 {
    rows.first()
        .and_then(|r| match r {
            toasty::stmt::Value::Record(rec) => rec.first(),
            _ => None,
        })
        .and_then(|v| match v {
            toasty::stmt::Value::I64(n) => u64::try_from((*n).max(0)).ok(),
            toasty::stmt::Value::U64(n) => Some(*n),
            toasty::stmt::Value::I32(n) => u64::try_from((*n).max(0)).ok(),
            toasty::stmt::Value::U32(n) => Some(u64::from(*n)),
            _ => None,
        })
        .unwrap_or(0)
}

fn binding_err(action: &str, err: impl std::fmt::Display) -> DomainError {
    DomainError::Persistence(format!("{action} user media binding failed: {err}"))
}

#[async_trait]
impl UserMediaBindingRepository for ToastyUserMediaBindingRepository {
    async fn list_bound_item_ids(
        &self,
        user_id: UserId,
        media_kind: MediaKind,
    ) -> DomainResult<HashSet<Uuid>> {
        let mut db = self.db.clone();
        let rows = UserMediaBindingModel::filter_by_user_id(user_id.0)
            .exec(&mut db)
            .await
            .map_err(|e| binding_err("list", e))?;
        let wanted = media_kind.as_str();
        Ok(rows
            .into_iter()
            .filter(|m| m.media_kind == wanted)
            .map(|m| m.item_id)
            .collect())
    }

    async fn is_bound(
        &self,
        user_id: UserId,
        media_kind: MediaKind,
        item_id: Uuid,
    ) -> DomainResult<bool> {
        let mut db = self.db.clone();
        let rows = UserMediaBindingModel::filter_by_item_id(item_id)
            .exec(&mut db)
            .await
            .map_err(|e| binding_err("check", e))?;
        let wanted = media_kind.as_str();
        Ok(rows
            .into_iter()
            .any(|m| m.user_id == user_id.0 && m.media_kind == wanted))
    }

    async fn count(&self, query: BindingCountQuery) -> DomainResult<u64> {
        match query {
            BindingCountQuery::UserItems {
                user_id,
                media_kind,
            } => self.count_user_items(user_id, media_kind).await,
            BindingCountQuery::ItemUsers {
                media_kind,
                item_id,
            } => self.count_item_users(media_kind, item_id).await,
        }
    }

    async fn insert_batch(
        &self,
        user_id: UserId,
        media_kind: MediaKind,
        item_ids: &[Uuid],
        created_at: Timestamp,
    ) -> DomainResult<usize> {
        if item_ids.is_empty() {
            return Ok(0);
        }
        let existing = self.list_bound_item_ids(user_id, media_kind).await?;
        let mut seen = HashSet::with_capacity(item_ids.len());
        let to_insert: Vec<Uuid> = item_ids
            .iter()
            .copied()
            .filter(|id| seen.insert(*id) && !existing.contains(id))
            .collect();
        if to_insert.is_empty() {
            return Ok(0);
        }
        let wanted = media_kind.as_str().to_string();
        let mut db = self.db.clone();
        let mut tx = db
            .transaction()
            .await
            .map_err(|e| binding_err("begin tx", e))?;
        for item_id in &to_insert {
            toasty::create!(UserMediaBindingModel {
                id: Uuid::now_v7(),
                user_id: user_id.0,
                media_kind: wanted.clone(),
                item_id: *item_id,
                created_at,
            })
            .exec(&mut tx)
            .await
            .map_err(|e| binding_err("batch insert", e))?;
        }
        tx.commit().await.map_err(|e| binding_err("commit tx", e))?;
        Ok(to_insert.len())
    }

    async fn delete_scope(&self, scope: BindingDeleteScope) -> DomainResult<Vec<Uuid>> {
        let mut db = self.db.clone();
        match scope {
            BindingDeleteScope::One {
                user_id,
                media_kind,
                item_id,
            } => {
                let rows = UserMediaBindingModel::filter_by_item_id(item_id)
                    .exec(&mut db)
                    .await
                    .map_err(|e| binding_err("find", e))?;
                let wanted = media_kind.as_str();
                let mut removed = Vec::new();
                for row in rows {
                    if row.user_id == user_id.0 && row.media_kind == wanted {
                        UserMediaBindingModel::filter_by_id(row.id)
                            .delete()
                            .exec(&mut db)
                            .await
                            .map_err(|e| binding_err("delete", e))?;
                        if removed.is_empty() {
                            removed.push(item_id);
                        }
                    }
                }
                Ok(removed)
            }
            BindingDeleteScope::Item {
                media_kind,
                item_id,
            } => {
                let rows = UserMediaBindingModel::filter_by_item_id(item_id)
                    .exec(&mut db)
                    .await
                    .map_err(|e| binding_err("list item", e))?;
                let wanted = media_kind.as_str();
                for row in rows {
                    if row.media_kind == wanted {
                        UserMediaBindingModel::filter_by_id(row.id)
                            .delete()
                            .exec(&mut db)
                            .await
                            .map_err(|e| binding_err("delete item", e))?;
                    }
                }
                Ok(Vec::new())
            }
            BindingDeleteScope::UserMedia {
                user_id,
                media_kind,
            } => {
                let rows = UserMediaBindingModel::filter_by_user_id(user_id.0)
                    .exec(&mut db)
                    .await
                    .map_err(|e| binding_err("list user", e))?;
                let wanted = media_kind.as_str();
                let mut unbound_ids = Vec::new();
                for row in rows {
                    if row.media_kind == wanted {
                        unbound_ids.push(row.item_id);
                        UserMediaBindingModel::filter_by_id(row.id)
                            .delete()
                            .exec(&mut db)
                            .await
                            .map_err(|e| binding_err("purge user", e))?;
                    }
                }
                Ok(unbound_ids)
            }
            BindingDeleteScope::User(user_id) => {
                UserMediaBindingModel::filter_by_user_id(user_id.0)
                    .delete()
                    .exec(&mut db)
                    .await
                    .map_err(|e| binding_err("purge user all", e))?;
                Ok(Vec::new())
            }
        }
    }
}

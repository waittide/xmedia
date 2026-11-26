use std::collections::HashSet;
use std::sync::Arc;

use jiff::Timestamp;
use serde::{Deserialize, Serialize};
use tokio::sync::Mutex;
use uuid::Uuid;

use crate::application::ports::{RealtimeEventPort, SystemEvent};
use crate::domain::error::{DomainError, DomainResult};
use crate::domain::library::MediaKind;
use crate::domain::repository::{
    BindingCountQuery, BindingDeleteScope, UserMediaBindingRepository,
};
use crate::domain::user::{User, UserId};

#[derive(Debug, Default, Clone, Deserialize)]
#[serde(default)]
pub struct BindMediaItemsInput {
    pub item_ids: Vec<Uuid>,
    pub library_id: Option<Uuid>,
    pub user_id: Option<Uuid>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BindMediaItemsResultDto {
    pub bound_count: usize,
    pub total_requested: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UnbindMediaItemResultDto {
    pub unbound: bool,
    pub remaining_bindings: u64,
    pub deleted_from_pool: bool,
}

/// Manages per-user media bindings over the global deduplicated media pool.
pub struct MediaBindingService {
    repo: Arc<dyn UserMediaBindingRepository>,
    events: Option<Arc<dyn RealtimeEventPort>>,
    write_lock: Mutex<()>,
}

impl MediaBindingService {
    pub fn new(
        repo: Arc<dyn UserMediaBindingRepository>,
        events: Option<Arc<dyn RealtimeEventPort>>,
    ) -> Self {
        Self {
            repo,
            events,
            write_lock: Mutex::new(()),
        }
    }

    pub fn resolve_target_user(actor: &User, requested_user: Option<Uuid>) -> DomainResult<UserId> {
        match requested_user {
            Some(uid) if UserId(uid) != actor.id => {
                actor.ensure_admin()?;
                Ok(UserId(uid))
            }
            Some(uid) => Ok(UserId(uid)),
            None => Ok(actor.id),
        }
    }

    pub async fn list_bound_ids(
        &self,
        user_id: UserId,
        media_kind: MediaKind,
    ) -> DomainResult<HashSet<Uuid>> {
        self.repo.list_bound_item_ids(user_id, media_kind).await
    }

    pub async fn count_bound_items(
        &self,
        user_id: UserId,
        media_kind: MediaKind,
    ) -> DomainResult<u64> {
        self.repo
            .count(BindingCountQuery::UserItems {
                user_id,
                media_kind,
            })
            .await
    }

    pub async fn is_bound(
        &self,
        user_id: UserId,
        media_kind: MediaKind,
        item_id: Uuid,
    ) -> DomainResult<bool> {
        self.repo.is_bound(user_id, media_kind, item_id).await
    }

    pub async fn require_bound(
        &self,
        user_id: UserId,
        media_kind: MediaKind,
        item_id: Uuid,
        not_found_msg: &str,
    ) -> DomainResult<()> {
        if self.repo.is_bound(user_id, media_kind, item_id).await? {
            Ok(())
        } else {
            Err(DomainError::NotFound(not_found_msg.to_string()))
        }
    }

    fn emit_changed(&self, media_kind: MediaKind) {
        if let Some(ref ev) = self.events {
            ev.publish(SystemEvent::MediaChanged { media_kind });
        }
    }

    async fn count_item_users(&self, media_kind: MediaKind, item_id: Uuid) -> DomainResult<u64> {
        self.repo
            .count(BindingCountQuery::ItemUsers {
                media_kind,
                item_id,
            })
            .await
    }

    pub async fn bind(
        &self,
        user_id: UserId,
        media_kind: MediaKind,
        item_id: Uuid,
    ) -> DomainResult<bool> {
        Ok(self.bind_batch(user_id, media_kind, &[item_id]).await? > 0)
    }

    pub async fn bind_batch(
        &self,
        user_id: UserId,
        media_kind: MediaKind,
        item_ids: &[Uuid],
    ) -> DomainResult<usize> {
        if item_ids.is_empty() {
            return Ok(0);
        }
        let _guard = self.write_lock.lock().await;
        let created_count = self
            .repo
            .insert_batch(user_id, media_kind, item_ids, Timestamp::now())
            .await?;
        if created_count > 0 {
            self.emit_changed(media_kind);
        }
        Ok(created_count)
    }

    /// Atomically removes `(user_id, media_kind, item_id)` and, if no bindings remain,
    /// invokes `gc_if_orphaned` while still holding `write_lock`.
    pub async fn unbind_with_gc<F, Fut>(
        &self,
        user_id: UserId,
        media_kind: MediaKind,
        item_id: Uuid,
        gc_if_orphaned: F,
    ) -> DomainResult<(bool, u64, bool)>
    where
        F: FnOnce() -> Fut,
        Fut: std::future::Future<Output = DomainResult<bool>>,
    {
        let _guard = self.write_lock.lock().await;
        let removed = self
            .repo
            .delete_scope(BindingDeleteScope::One {
                user_id,
                media_kind,
                item_id,
            })
            .await?;
        let deleted = !removed.is_empty();
        let remaining = self.count_item_users(media_kind, item_id).await?;
        let gc_deleted = if remaining == 0 {
            gc_if_orphaned().await?
        } else {
            false
        };
        if deleted || gc_deleted {
            self.emit_changed(media_kind);
        }
        Ok((deleted, remaining, gc_deleted))
    }

    pub async fn gc_if_orphaned<F, Fut>(
        &self,
        media_kind: MediaKind,
        item_id: Uuid,
        gc_if_orphaned: F,
    ) -> DomainResult<bool>
    where
        F: FnOnce() -> Fut,
        Fut: std::future::Future<Output = DomainResult<bool>>,
    {
        let _guard = self.write_lock.lock().await;
        if self.count_item_users(media_kind, item_id).await? != 0 {
            return Ok(false);
        }
        let gc_deleted = gc_if_orphaned().await?;
        if gc_deleted {
            self.emit_changed(media_kind);
        }
        Ok(gc_deleted)
    }

    pub async fn purge(&self, scope: BindingDeleteScope) -> DomainResult<Vec<Uuid>> {
        let _guard = self.write_lock.lock().await;
        self.repo.delete_scope(scope).await
    }
}

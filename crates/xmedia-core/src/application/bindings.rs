use std::collections::HashSet;
use std::sync::Arc;

use jiff::Timestamp;
use serde::{Deserialize, Serialize};
use tokio::sync::Mutex;
use uuid::Uuid;

use crate::application::ports::{RealtimeEventPort, SystemEvent};
use crate::domain::error::{DomainError, DomainResult};
use crate::domain::library::MediaKind;
use crate::domain::repository::UserMediaBindingRepository;
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

    pub fn resolve_target_user_id(
        actor: &User,
        requested_user_id: Option<Uuid>,
    ) -> DomainResult<UserId> {
        match requested_user_id {
            None => Ok(actor.id),
            Some(uid) if uid == actor.id.0 => Ok(actor.id),
            Some(uid) => {
                if actor.role.is_admin() {
                    Ok(UserId(uid))
                } else {
                    Err(DomainError::Forbidden(
                        "only admins can manage media bindings for other users".into(),
                    ))
                }
            }
        }
    }

    pub async fn list_bound_ids(
        &self,
        user_id: UserId,
        media_kind: MediaKind,
    ) -> DomainResult<HashSet<Uuid>> {
        self.repo.list_bound_item_ids(user_id, media_kind).await
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

    pub async fn count_bindings_for_item(
        &self,
        media_kind: MediaKind,
        item_id: Uuid,
    ) -> DomainResult<u64> {
        self.repo.count_bindings_for_item(media_kind, item_id).await
    }

    pub async fn bind(
        &self,
        user_id: UserId,
        media_kind: MediaKind,
        item_id: Uuid,
    ) -> DomainResult<bool> {
        let _guard = self.write_lock.lock().await;
        let created = self
            .repo
            .insert_if_absent(user_id, media_kind, item_id, Timestamp::now())
            .await?;
        if created && let Some(ref ev) = self.events {
            ev.publish(SystemEvent::MediaChanged { media_kind });
        }
        Ok(created)
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
        let now = Timestamp::now();
        let mut seen = HashSet::with_capacity(item_ids.len());
        let mut created_count = 0usize;
        for &item_id in item_ids {
            if !seen.insert(item_id) {
                continue;
            }
            if self
                .repo
                .insert_if_absent(user_id, media_kind, item_id, now)
                .await?
            {
                created_count += 1;
            }
        }
        if created_count > 0
            && let Some(ref ev) = self.events
        {
            ev.publish(SystemEvent::MediaChanged { media_kind });
        }
        Ok(created_count)
    }

    /// Removes `(user_id, media_kind, item_id)` and returns `(was_bound, remaining_binding_count)`.
    pub async fn unbind(
        &self,
        user_id: UserId,
        media_kind: MediaKind,
        item_id: Uuid,
    ) -> DomainResult<(bool, u64)> {
        let _guard = self.write_lock.lock().await;
        let deleted = self
            .repo
            .delete_binding(user_id, media_kind, item_id)
            .await?;
        let remaining = self
            .repo
            .count_bindings_for_item(media_kind, item_id)
            .await?;
        if deleted && let Some(ref ev) = self.events {
            ev.publish(SystemEvent::MediaChanged { media_kind });
        }
        Ok((deleted, remaining))
    }

    /// Atomically removes `(user_id, media_kind, item_id)` and, if no bindings remain,
    /// invokes `gc_if_orphaned` while still holding `write_lock` so no concurrent bind
    /// can interleave before orphan cleanup completes.
    ///
    /// Callers must NOT invoke write methods on this `MediaBindingService` inside
    /// `gc_if_orphaned` (the binding count is already guaranteed to be 0).
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
        let deleted = self
            .repo
            .delete_binding(user_id, media_kind, item_id)
            .await?;
        let remaining = self
            .repo
            .count_bindings_for_item(media_kind, item_id)
            .await?;
        let gc_deleted = if remaining == 0 {
            gc_if_orphaned().await?
        } else {
            false
        };
        if (deleted || gc_deleted)
            && let Some(ref ev) = self.events
        {
            ev.publish(SystemEvent::MediaChanged { media_kind });
        }
        Ok((deleted, remaining, gc_deleted))
    }

    /// Re-checks under `write_lock` that `item_id` still has zero bindings and, if so,
    /// invokes `gc_if_orphaned` atomically before releasing the lock.
    ///
    /// Callers must NOT invoke write methods on this `MediaBindingService` inside
    /// `gc_if_orphaned`.
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
        let remaining = self
            .repo
            .count_bindings_for_item(media_kind, item_id)
            .await?;
        if remaining != 0 {
            return Ok(false);
        }
        let gc_deleted = gc_if_orphaned().await?;
        if gc_deleted && let Some(ref ev) = self.events {
            ev.publish(SystemEvent::MediaChanged { media_kind });
        }
        Ok(gc_deleted)
    }

    pub async fn delete_item_bindings(
        &self,
        media_kind: MediaKind,
        item_id: Uuid,
    ) -> DomainResult<()> {
        let _guard = self.write_lock.lock().await;
        self.repo.delete_by_item(media_kind, item_id).await
    }

    /// Deletes all bindings for `(user_id, media_kind)` and returns the previously bound item IDs
    /// so the domain can garbage-collect any unreferenced online-tracked items.
    pub async fn delete_user_bindings_for_media(
        &self,
        user_id: UserId,
        media_kind: MediaKind,
    ) -> DomainResult<Vec<Uuid>> {
        let _guard = self.write_lock.lock().await;
        self.repo
            .delete_by_user_and_media(user_id, media_kind)
            .await
    }

    /// Purges all remaining media bindings for `user_id` across every media domain.
    pub async fn purge_user(&self, user_id: UserId) -> DomainResult<()> {
        let _guard = self.write_lock.lock().await;
        self.repo.delete_by_user(user_id).await
    }
}

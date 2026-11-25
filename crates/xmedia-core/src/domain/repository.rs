use std::collections::HashSet;

use async_trait::async_trait;
use jiff::Timestamp;
use uuid::Uuid;

use crate::domain::error::DomainResult;
use crate::domain::library::{Library, LibraryId, MediaKind, ScanStatus};
use crate::domain::online_item::OnlineLibraryItem;
use crate::domain::user::{User, UserId, UserPreferences};

#[async_trait]
pub trait UserRepository: Send + Sync {
    async fn count(&self) -> DomainResult<u64>;
    async fn has_any_user(&self) -> DomainResult<bool> {
        Ok(self.count().await? > 0)
    }
    async fn find_by_id(&self, id: UserId) -> DomainResult<Option<User>>;
    async fn find_by_username(&self, username: &str) -> DomainResult<Option<User>>;
    async fn find_by_api_key(&self, api_key: &str) -> DomainResult<Option<User>>;
    async fn list_all(&self) -> DomainResult<Vec<User>>;
    async fn save(&self, user: &User) -> DomainResult<()>;
    async fn update_preferences(
        &self,
        id: UserId,
        preferences: &UserPreferences,
        updated_at: Timestamp,
    ) -> DomainResult<bool>;
    async fn update_credentials(
        &self,
        id: UserId,
        password_hash: &str,
        subsonic_secret_enc: &str,
        updated_at: Timestamp,
    ) -> DomainResult<bool>;
    async fn update_api_key(
        &self,
        id: UserId,
        api_key: &str,
        updated_at: Timestamp,
    ) -> DomainResult<bool>;
    async fn delete(&self, id: UserId) -> DomainResult<()>;
}

#[async_trait]
pub trait LibraryRepository: Send + Sync {
    async fn find_by_id(&self, id: LibraryId) -> DomainResult<Option<Library>>;
    async fn find_managed_by_media(&self, media_kind: MediaKind) -> DomainResult<Option<Library>>;
    async fn list_all(&self) -> DomainResult<Vec<Library>>;
    async fn list_by_media(&self, media_kind: MediaKind) -> DomainResult<Vec<Library>>;
    async fn save(&self, library: &Library) -> DomainResult<()>;
    async fn update_name(
        &self,
        id: LibraryId,
        name: &str,
        updated_at: Timestamp,
    ) -> DomainResult<bool>;
    async fn update_scan_state(
        &self,
        id: LibraryId,
        scan_status: &ScanStatus,
        last_scanned_at: Option<Timestamp>,
        updated_at: Timestamp,
    ) -> DomainResult<bool>;
    async fn delete(&self, id: LibraryId) -> DomainResult<()>;
}

/// Persists per-user favorites / progress of online (non-library) items.
#[async_trait]
pub trait OnlineLibraryItemRepository: Send + Sync {
    /// Lists all items of one media kind for a user (unordered).
    async fn list_by_user(
        &self,
        user_id: UserId,
        media_kind: MediaKind,
    ) -> DomainResult<Vec<OnlineLibraryItem>>;
    async fn find_by_id(&self, id: Uuid) -> DomainResult<Option<OnlineLibraryItem>>;
    async fn save(&self, item: &OnlineLibraryItem) -> DomainResult<()>;
    async fn delete(&self, id: Uuid) -> DomainResult<()>;
    async fn delete_by_user(&self, user_id: UserId) -> DomainResult<()>;
}

/// Persists per-user visibility bindings onto the global deduplicated media pool.
#[async_trait]
pub trait UserMediaBindingRepository: Send + Sync {
    async fn list_bound_item_ids(
        &self,
        user_id: UserId,
        media_kind: MediaKind,
    ) -> DomainResult<HashSet<Uuid>>;

    async fn is_bound(
        &self,
        user_id: UserId,
        media_kind: MediaKind,
        item_id: Uuid,
    ) -> DomainResult<bool>;

    async fn count_bindings_for_item(
        &self,
        media_kind: MediaKind,
        item_id: Uuid,
    ) -> DomainResult<u64>;

    async fn insert_if_absent(
        &self,
        user_id: UserId,
        media_kind: MediaKind,
        item_id: Uuid,
        created_at: Timestamp,
    ) -> DomainResult<bool>;

    async fn delete_binding(
        &self,
        user_id: UserId,
        media_kind: MediaKind,
        item_id: Uuid,
    ) -> DomainResult<bool>;

    async fn delete_by_item(&self, media_kind: MediaKind, item_id: Uuid) -> DomainResult<()>;

    async fn delete_by_user_and_media(
        &self,
        user_id: UserId,
        media_kind: MediaKind,
    ) -> DomainResult<Vec<Uuid>>;

    async fn delete_by_user(&self, user_id: UserId) -> DomainResult<()>;
}

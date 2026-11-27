use std::collections::HashSet;

use async_trait::async_trait;
use jiff::Timestamp;
use uuid::Uuid;

use crate::domain::error::DomainResult;
use crate::domain::library::{Library, LibraryId, MediaKind, ScanStatus};
use crate::domain::online_item::OnlineLibraryItem;
use crate::domain::user::{User, UserId, UserPreferences};

#[derive(Debug, Clone, Copy)]
pub enum UserLookup<'a> {
    Id(UserId),
    Username(&'a str),
    ApiKey(&'a str),
}

#[derive(Debug, Clone, Copy)]
pub enum UserMutation<'a> {
    Preferences(&'a UserPreferences),
    Credentials {
        password_hash: &'a str,
        subsonic_secret_enc: &'a str,
    },
    ApiKey(&'a str),
}

#[async_trait]
pub trait UserRepository: Send + Sync {
    async fn count(&self) -> DomainResult<u64>;
    async fn find(&self, key: UserLookup<'_>) -> DomainResult<Option<User>>;
    async fn list_all(&self) -> DomainResult<Vec<User>>;
    async fn save(&self, user: &User) -> DomainResult<()>;
    async fn update(
        &self,
        id: UserId,
        patch: UserMutation<'_>,
        updated_at: Timestamp,
    ) -> DomainResult<bool>;
    async fn delete(&self, id: UserId) -> DomainResult<()>;
}

#[derive(Debug, Clone, Copy)]
pub enum LibraryMutation<'a> {
    Name(&'a str),
    ScanState {
        status: &'a ScanStatus,
        last_scanned_at: Option<Timestamp>,
    },
}

#[async_trait]
pub trait LibraryRepository: Send + Sync {
    async fn find_by_id(&self, id: LibraryId) -> DomainResult<Option<Library>>;
    async fn find_managed_by_media(&self, media_kind: MediaKind) -> DomainResult<Option<Library>>;
    async fn list(&self, media_kind: Option<MediaKind>) -> DomainResult<Vec<Library>>;
    async fn save(&self, library: &Library) -> DomainResult<()>;
    async fn update(
        &self,
        id: LibraryId,
        patch: LibraryMutation<'_>,
        updated_at: Timestamp,
    ) -> DomainResult<bool>;
    async fn delete(&self, id: LibraryId) -> DomainResult<()>;
}

#[derive(Debug, Clone, Copy)]
pub enum OnlineItemDeleteScope {
    Id(Uuid),
    User(UserId),
}

/// Persists per-user favorites / progress of online (non-library) items.
#[async_trait]
pub trait OnlineLibraryItemRepository: Send + Sync {
    async fn list_by_user(
        &self,
        user_id: UserId,
        media_kind: MediaKind,
    ) -> DomainResult<Vec<OnlineLibraryItem>>;
    async fn find_by_id(&self, id: Uuid) -> DomainResult<Option<OnlineLibraryItem>>;
    async fn save(&self, item: &OnlineLibraryItem) -> DomainResult<()>;
    async fn delete_scope(&self, scope: OnlineItemDeleteScope) -> DomainResult<()>;
}

#[derive(Debug, Clone, Copy)]
pub enum BindingCountQuery {
    UserItems {
        user_id: UserId,
        media_kind: MediaKind,
    },
    ItemUsers {
        media_kind: MediaKind,
        item_id: Uuid,
    },
}

#[derive(Debug, Clone, Copy)]
pub enum BindingDeleteScope {
    One {
        user_id: UserId,
        media_kind: MediaKind,
        item_id: Uuid,
    },
    Item {
        media_kind: MediaKind,
        item_id: Uuid,
    },
    UserMedia {
        user_id: UserId,
        media_kind: MediaKind,
    },
    User(UserId),
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

    async fn count(&self, query: BindingCountQuery) -> DomainResult<u64>;

    async fn insert_batch(
        &self,
        user_id: UserId,
        media_kind: MediaKind,
        item_ids: &[Uuid],
        created_at: Timestamp,
    ) -> DomainResult<usize>;

    async fn delete_scope(&self, scope: BindingDeleteScope) -> DomainResult<Vec<Uuid>>;
}

/// Domain-agnostic persisted plugin record strictly isolated per user.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UserPluginRow {
    pub id: Uuid,
    pub user_id: Uuid,
    pub media_kind: MediaKind,
    pub format: String,
    pub name: String,
    pub source_url: String,
    pub group_name: String,
    pub enabled: bool,
    pub weight: i64,
    pub source_type: i64,
    pub raw_source: String,
    pub runtime_state: serde_json::Value,
    pub last_test_ok: Option<bool>,
    pub last_test_msg: Option<String>,
    pub created_at: Timestamp,
    pub updated_at: Timestamp,
}

/// Persistence port for per-user isolated media plugins across all domains.
#[async_trait]
pub trait UserPluginRepository: Send + Sync {
    async fn list_by_user(
        &self,
        user_id: UserId,
        media_kind: Option<MediaKind>,
    ) -> DomainResult<Vec<UserPluginRow>>;

    async fn list_all(&self, media_kind: Option<MediaKind>) -> DomainResult<Vec<UserPluginRow>>;

    async fn find_by_id(&self, id: Uuid) -> DomainResult<Option<UserPluginRow>>;

    async fn save(&self, row: &UserPluginRow) -> DomainResult<()>;

    async fn delete(&self, id: Uuid) -> DomainResult<()>;

    async fn purge_user(&self, user_id: UserId) -> DomainResult<usize>;
}

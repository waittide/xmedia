use async_trait::async_trait;

use crate::domain::error::DomainResult;
use crate::domain::library::{Library, LibraryId, MediaKind};
use crate::domain::user::{User, UserId};

#[async_trait]
pub trait UserRepository: Send + Sync {
    async fn count(&self) -> DomainResult<u64>;
    async fn find_by_id(&self, id: UserId) -> DomainResult<Option<User>>;
    async fn find_by_username(&self, username: &str) -> DomainResult<Option<User>>;
    async fn find_by_api_key(&self, api_key: &str) -> DomainResult<Option<User>>;
    async fn list_all(&self) -> DomainResult<Vec<User>>;
    async fn save(&self, user: &User) -> DomainResult<()>;
    async fn delete(&self, id: UserId) -> DomainResult<()>;
}

#[async_trait]
pub trait LibraryRepository: Send + Sync {
    async fn find_by_id(&self, id: LibraryId) -> DomainResult<Option<Library>>;
    async fn find_managed_by_media(&self, media_kind: MediaKind) -> DomainResult<Option<Library>>;
    async fn list_all(&self) -> DomainResult<Vec<Library>>;
    async fn list_by_media(&self, media_kind: MediaKind) -> DomainResult<Vec<Library>>;
    async fn save(&self, library: &Library) -> DomainResult<()>;
    async fn delete(&self, id: LibraryId) -> DomainResult<()>;
}

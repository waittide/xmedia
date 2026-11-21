use async_trait::async_trait;
use jiff::Timestamp;
use uuid::Uuid;
use xmedia_core::domain::error::DomainResult;
use xmedia_core::domain::library::LibraryId;
use xmedia_core::domain::user::UserId;

use crate::domain::book::{Book, BookId, BookQuery, FacetCount, PagedResult};
use crate::domain::online::{BookSourceRecord, OnlineShelfBook};
use crate::domain::reading::{Bookmark, BookmarkId, ReadingProgress, Shelf, ShelfId};
use crate::source::model::SourceRuntimeState;

#[async_trait]
pub trait BookRepository: Send + Sync {
    async fn find_by_id(&self, id: BookId) -> DomainResult<Option<Book>>;
    async fn find_by_ids(&self, ids: &[BookId]) -> DomainResult<Vec<Book>>;
    async fn list_by_library(&self, library_id: LibraryId) -> DomainResult<Vec<Book>>;
    async fn search(&self, query: &BookQuery) -> DomainResult<PagedResult<Book>>;
    async fn count_by_library(&self, library_id: LibraryId) -> DomainResult<u64>;
    async fn list_authors(&self) -> DomainResult<Vec<FacetCount>>;
    async fn list_tags(&self) -> DomainResult<Vec<FacetCount>>;
    async fn home_catalog_snapshot(
        &self,
        recent_limit: usize,
    ) -> DomainResult<(Vec<Book>, u64, u64, u64)>;
    async fn save(&self, book: &Book) -> DomainResult<()>;
    async fn delete(&self, id: BookId) -> DomainResult<()>;
    async fn delete_by_library(&self, library_id: LibraryId) -> DomainResult<()>;
}

#[async_trait]
pub trait BookReadingRepository: Send + Sync {
    async fn get_progress(
        &self,
        user_id: UserId,
        book_id: BookId,
    ) -> DomainResult<Option<ReadingProgress>>;
    async fn list_recent_progress(
        &self,
        user_id: UserId,
        limit: u32,
    ) -> DomainResult<Vec<ReadingProgress>>;
    async fn count_user_progress(&self, user_id: UserId) -> DomainResult<u64>;
    async fn save_progress(&self, progress: &ReadingProgress) -> DomainResult<()>;

    async fn list_bookmarks(&self, user_id: UserId, book_id: BookId)
    -> DomainResult<Vec<Bookmark>>;
    async fn list_user_bookmarks(&self, user_id: UserId) -> DomainResult<Vec<Bookmark>>;
    async fn save_bookmark(&self, bookmark: &Bookmark) -> DomainResult<()>;
    async fn delete_bookmark(&self, user_id: UserId, bookmark_id: BookmarkId) -> DomainResult<()>;

    async fn is_favorite(&self, user_id: UserId, book_id: BookId) -> DomainResult<bool>;
    async fn list_favorite_book_ids(&self, user_id: UserId) -> DomainResult<Vec<BookId>>;
    async fn set_favorite(
        &self,
        user_id: UserId,
        book_id: BookId,
        favorite: bool,
    ) -> DomainResult<()>;

    async fn list_shelves(&self, user_id: UserId) -> DomainResult<Vec<Shelf>>;
    async fn find_shelf(&self, user_id: UserId, shelf_id: ShelfId) -> DomainResult<Option<Shelf>>;
    async fn save_shelf(&self, shelf: &Shelf) -> DomainResult<()>;
    async fn delete_shelf(&self, user_id: UserId, shelf_id: ShelfId) -> DomainResult<()>;

    async fn delete_by_book(&self, book_id: BookId) -> DomainResult<()>;
    async fn delete_by_books(&self, book_ids: &[BookId]) -> DomainResult<()> {
        for &book_id in book_ids {
            self.delete_by_book(book_id).await?;
        }
        Ok(())
    }
    async fn delete_by_user(&self, user_id: UserId) -> DomainResult<()>;
}

#[async_trait]
pub trait BookSourceRepository: Send + Sync {
    async fn list_all(&self) -> DomainResult<Vec<BookSourceRecord>>;
    async fn find_by_id(&self, id: Uuid) -> DomainResult<Option<BookSourceRecord>>;
    async fn save(&self, source: &BookSourceRecord) -> DomainResult<()>;
    /// Updates an existing source row without resurrecting a deleted record.
    async fn update_existing(&self, source: &BookSourceRecord) -> DomainResult<bool>;
    async fn update_runtime_state(
        &self,
        id: Uuid,
        runtime_state: SourceRuntimeState,
        updated_at: Timestamp,
    ) -> DomainResult<()>;
    /// Updates test status & runtime state only if the source still exists.
    async fn update_test_result(
        &self,
        id: Uuid,
        runtime_state: SourceRuntimeState,
        last_test_ok: bool,
        last_test_msg: Option<String>,
        updated_at: Timestamp,
    ) -> DomainResult<bool>;
    /// Updates `weight` (and syncs `raw_json`) only if the source still exists, returning the latest record.
    async fn update_weight_if_exists(
        &self,
        id: Uuid,
        weight: i64,
        updated_at: Timestamp,
    ) -> DomainResult<Option<BookSourceRecord>>;
    /// Updates `enabled` / `group_name` (and syncs `raw_json`) on the latest row if it still exists.
    async fn update_batch_fields_if_exists(
        &self,
        id: Uuid,
        enabled: Option<bool>,
        group_name: Option<String>,
        updated_at: Timestamp,
    ) -> DomainResult<bool>;
    async fn delete(&self, id: Uuid) -> DomainResult<()>;
}

#[async_trait]
pub trait OnlineShelfRepository: Send + Sync {
    async fn list_by_user(&self, user_id: UserId) -> DomainResult<Vec<OnlineShelfBook>>;
    async fn find_by_id(&self, id: Uuid) -> DomainResult<Option<OnlineShelfBook>>;
    async fn save(&self, item: &OnlineShelfBook) -> DomainResult<()>;
    async fn delete(&self, id: Uuid) -> DomainResult<()>;
    async fn delete_by_user(&self, user_id: UserId) -> DomainResult<()>;
}

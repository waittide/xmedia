use async_trait::async_trait;
use xmedia_core::domain::error::DomainResult;
use xmedia_core::domain::library::LibraryId;
use xmedia_core::domain::user::UserId;

use crate::domain::book::{Book, BookId, BookQuery, FacetCount, PagedResult};
use crate::domain::reading::{Bookmark, BookmarkId, ReadingProgress, Shelf, ShelfId};

#[async_trait]
pub trait BookRepository: Send + Sync {
    async fn find_by_id(&self, id: BookId) -> DomainResult<Option<Book>>;
    async fn find_by_ids(&self, ids: &[BookId]) -> DomainResult<Vec<Book>>;
    async fn list_by_library(&self, library_id: LibraryId) -> DomainResult<Vec<Book>>;
    async fn search(&self, query: &BookQuery) -> DomainResult<PagedResult<Book>>;
    async fn count_by_library(&self, library_id: LibraryId) -> DomainResult<u64>;
    async fn list_authors(&self) -> DomainResult<Vec<FacetCount>>;
    async fn list_tags(&self) -> DomainResult<Vec<FacetCount>>;
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
    async fn delete_by_user(&self, user_id: UserId) -> DomainResult<()>;
}

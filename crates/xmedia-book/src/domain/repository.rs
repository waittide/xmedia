use async_trait::async_trait;
use xmedia_core::domain::error::DomainResult;
use xmedia_core::domain::library::LibraryId;
use xmedia_core::domain::user::UserId;

use crate::domain::book::{Book, BookId, BookQuery, PagedResult};
use crate::domain::reading::{Bookmark, BookmarkId, ReadingProgress, Shelf, ShelfId};

#[derive(Debug, Clone)]
pub enum BookCollectionQuery {
    Id(BookId),
    Ids(Vec<BookId>),
    Library(LibraryId),
}

#[derive(Debug, Clone, Copy)]
pub enum BookDeleteScope {
    Id(BookId),
    Library(LibraryId),
}

#[async_trait]
pub trait BookRepository: Send + Sync {
    async fn query(&self, scope: BookCollectionQuery) -> DomainResult<Vec<Book>>;
    async fn list_all_ids(&self) -> DomainResult<Vec<BookId>>;
    async fn search(&self, query: &BookQuery) -> DomainResult<PagedResult<Book>>;
    async fn save(&self, book: &Book) -> DomainResult<()>;
    async fn delete_scope(&self, scope: BookDeleteScope) -> DomainResult<()>;
}

#[derive(Debug, Clone)]
pub enum ReadingMutation<'a> {
    Progress(&'a ReadingProgress),
    Bookmark(&'a Bookmark),
    Shelf(&'a Shelf),
    Favorite {
        user_id: UserId,
        book_id: BookId,
        favorite: bool,
    },
}

#[derive(Debug, Clone)]
pub enum ReadingDeleteScope {
    Bookmark {
        user_id: UserId,
        bookmark_id: BookmarkId,
    },
    Shelf {
        user_id: UserId,
        shelf_id: ShelfId,
    },
    Book(BookId),
    Books(Vec<BookId>),
    User(UserId),
    UserBook {
        user_id: UserId,
        book_id: BookId,
    },
}

#[derive(Debug, Clone, Copy)]
pub enum ProgressScope {
    Book(BookId),
    Recent(u32),
}

#[async_trait]
pub trait BookReadingRepository: Send + Sync {
    async fn query_progress(
        &self,
        user_id: UserId,
        scope: ProgressScope,
    ) -> DomainResult<Vec<ReadingProgress>>;
    async fn list_bookmarks(
        &self,
        user_id: UserId,
        book_id: Option<BookId>,
    ) -> DomainResult<Vec<Bookmark>>;
    async fn list_favorite_book_ids(&self, user_id: UserId) -> DomainResult<Vec<BookId>>;
    async fn query_shelves(
        &self,
        user_id: UserId,
        shelf_id: Option<ShelfId>,
    ) -> DomainResult<Vec<Shelf>>;
    async fn save(&self, mutation: ReadingMutation<'_>) -> DomainResult<()>;
    async fn delete_scope(&self, scope: ReadingDeleteScope) -> DomainResult<()>;
}

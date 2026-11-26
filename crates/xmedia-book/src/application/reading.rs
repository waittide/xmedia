use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use serde::{Deserialize, Serialize};
use xmedia_core::application::bindings::MediaBindingService;
use xmedia_core::domain::error::{DomainError, DomainResult};
use xmedia_core::domain::library::MediaKind;
use xmedia_core::domain::user::User;

use crate::application::catalog::BookCatalogService;
use xmedia_core::domain::repository::LibraryRepository;

use crate::application::dto::{
    BookBookmarkItemDto, BookHomeOverviewDto, BookLibraryStatsDto, BookSummaryDto, ShelfDto,
};
use crate::domain::book::{Book, BookId};
use crate::domain::reading::{Bookmark, BookmarkId, ReadingProgress, Shelf, ShelfId};
use crate::domain::repository::{
    BookCollectionQuery, BookReadingRepository, BookRepository, ProgressScope, ReadingDeleteScope,
    ReadingMutation,
};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SaveProgressCommand {
    pub progress_percent: f64,
    pub locator: String,
    pub chapter_index: Option<u32>,
    pub page_number: Option<u32>,
    pub scroll_ratio: Option<f64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateBookmarkCommand {
    pub title: String,
    pub note: Option<String>,
    pub progress_percent: f64,
    pub locator: String,
    pub chapter_index: Option<u32>,
    pub page_number: Option<u32>,
    pub scroll_ratio: Option<f64>,
}

pub struct BookReadingService {
    readings: Arc<dyn BookReadingRepository>,
    books: Arc<dyn BookRepository>,
    libraries: Arc<dyn LibraryRepository>,
    bindings: Arc<MediaBindingService>,
    catalog: Arc<BookCatalogService>,
}

impl BookReadingService {
    pub fn new(
        readings: Arc<dyn BookReadingRepository>,
        books: Arc<dyn BookRepository>,
        libraries: Arc<dyn LibraryRepository>,
        bindings: Arc<MediaBindingService>,
        catalog: Arc<BookCatalogService>,
    ) -> Self {
        Self {
            readings,
            books,
            libraries,
            bindings,
            catalog,
        }
    }

    pub async fn home_overview(&self, actor: &User) -> DomainResult<BookHomeOverviewDto> {
        let bound_books = self.catalog.load_bound_active_books(actor.id).await?;
        let total_books = bound_books.len() as u64;
        let bound_id_set: HashSet<BookId> = bound_books.iter().map(|b| b.id).collect();
        let by_id: HashMap<BookId, Book> = bound_books.iter().map(|b| (b.id, b.clone())).collect();
        let (authors_count, tags_count, libraries_count, latest_raw) =
            Book::summarize_collection(bound_books);

        let libs = self.libraries.list(Some(MediaKind::Book)).await?;
        let lib_names: HashMap<_, _> = libs.into_iter().map(|l| (l.id, l.name)).collect();

        let fav_ids: Vec<BookId> = self
            .readings
            .list_favorite_book_ids(actor.id)
            .await?
            .into_iter()
            .filter(|id| bound_id_set.contains(id))
            .collect();
        let favorites_count = fav_ids.len() as u64;
        let fav_set: HashSet<BookId> = fav_ids.into_iter().collect();

        let all_progress: Vec<ReadingProgress> = self
            .readings
            .query_progress(actor.id, ProgressScope::Recent(u32::MAX))
            .await?
            .into_iter()
            .filter(|p| bound_id_set.contains(&p.book_id))
            .collect();
        let reading_count = all_progress.len() as u64;
        let recent_ids: Vec<BookId> = all_progress.iter().take(6).map(|r| r.book_id).collect();
        let progress_map: HashMap<BookId, ReadingProgress> =
            all_progress.into_iter().map(|p| (p.book_id, p)).collect();

        let recently_added = BookCatalogService::enrich_books_with_maps(
            &latest_raw,
            &lib_names,
            &fav_set,
            &progress_map,
        );

        let continue_reading = if recent_ids.is_empty() {
            Vec::new()
        } else {
            let ordered: Vec<Book> = recent_ids
                .into_iter()
                .filter_map(|id| by_id.get(&id).cloned())
                .collect();
            BookCatalogService::enrich_books_with_maps(
                &ordered,
                &lib_names,
                &fav_set,
                &progress_map,
            )
        };

        let bookmarks_count = self.count_user_bookmarks(actor).await?;
        let shelves = self.list_shelves(actor).await?;

        Ok(BookHomeOverviewDto {
            stats: BookLibraryStatsDto {
                total_books,
                reading_count,
                bookmarks_count,
                favorites_count,
                shelves_count: shelves.len() as u64,
                tags_count,
                authors_count,
                libraries_count,
            },
            continue_reading,
            recently_added,
        })
    }

    pub async fn get_progress(
        &self,
        actor: &User,
        book_id: BookId,
    ) -> DomainResult<Option<ReadingProgress>> {
        self.ensure_book_bound(actor, book_id).await?;
        Ok(self
            .readings
            .query_progress(actor.id, ProgressScope::Book(book_id))
            .await?
            .into_iter()
            .next())
    }

    pub async fn save_progress(
        &self,
        actor: &User,
        book_id: BookId,
        cmd: SaveProgressCommand,
    ) -> DomainResult<ReadingProgress> {
        self.ensure_book_bound(actor, book_id).await?;
        let existing = self
            .readings
            .query_progress(actor.id, ProgressScope::Book(book_id))
            .await?
            .into_iter()
            .next();
        let progress = ReadingProgress::upsert(
            existing,
            actor.id,
            book_id,
            cmd.progress_percent,
            cmd.locator,
            cmd.chapter_index,
            cmd.page_number,
            cmd.scroll_ratio,
        )?;

        self.readings
            .save(ReadingMutation::Progress(&progress))
            .await?;
        Ok(progress)
    }

    pub async fn list_recent_books(
        &self,
        actor: &User,
        limit: u32,
    ) -> DomainResult<Vec<BookSummaryDto>> {
        let bound_ids = self
            .bindings
            .list_bound_ids(actor.id, MediaKind::Book)
            .await?;
        if bound_ids.is_empty() {
            return Ok(Vec::new());
        }
        let recent = self
            .readings
            .query_progress(actor.id, ProgressScope::Recent(u32::MAX))
            .await?;
        let ids: Vec<BookId> = recent
            .iter()
            .filter(|r| bound_ids.contains(&r.book_id.0))
            .take(limit.clamp(1, 100) as usize)
            .map(|r| r.book_id)
            .collect();
        if ids.is_empty() {
            return Ok(Vec::new());
        }

        let found = self
            .books
            .query(BookCollectionQuery::Ids(ids.clone()))
            .await?;
        let by_id: HashMap<BookId, Book> = found.into_iter().map(|b| (b.id, b)).collect();

        let ordered: Vec<Book> = ids
            .into_iter()
            .filter_map(|id| by_id.get(&id).cloned())
            .collect();

        self.catalog.enrich_books(Some(actor), &ordered).await
    }

    pub async fn list_bookmarks(
        &self,
        actor: &User,
        book_id: BookId,
    ) -> DomainResult<Vec<Bookmark>> {
        self.ensure_book_bound(actor, book_id).await?;
        self.readings.list_bookmarks(actor.id, Some(book_id)).await
    }

    pub async fn list_all_bookmarks(&self, actor: &User) -> DomainResult<Vec<BookBookmarkItemDto>> {
        let bound_ids = self
            .bindings
            .list_bound_ids(actor.id, MediaKind::Book)
            .await?;
        if bound_ids.is_empty() {
            return Ok(Vec::new());
        }
        let bookmarks: Vec<Bookmark> = self
            .readings
            .list_bookmarks(actor.id, None)
            .await?
            .into_iter()
            .filter(|bm| bound_ids.contains(&bm.book_id.0))
            .collect();
        if bookmarks.is_empty() {
            return Ok(Vec::new());
        }
        let mut unique_ids = Vec::new();
        for bm in &bookmarks {
            if !unique_ids.contains(&bm.book_id) {
                unique_ids.push(bm.book_id);
            }
        }
        let found = self
            .books
            .query(BookCollectionQuery::Ids(unique_ids))
            .await?;
        let by_id: HashMap<BookId, Book> = found.into_iter().map(|b| (b.id, b)).collect();

        let mut out = Vec::with_capacity(bookmarks.len());
        for bm in bookmarks {
            if let Some(book) = by_id.get(&bm.book_id) {
                out.push(BookBookmarkItemDto {
                    bookmark: bm,
                    book_title: book.metadata.title.clone(),
                    book_authors: book.metadata.authors.clone(),
                    book_format: book.format,
                    has_cover: book.cover_key.is_some(),
                    book_updated_at: book.updated_at,
                });
            }
        }
        Ok(out)
    }

    pub async fn count_user_progress(&self, actor: &User) -> DomainResult<u64> {
        let bound_ids = self
            .bindings
            .list_bound_ids(actor.id, MediaKind::Book)
            .await?;
        let list = self
            .readings
            .query_progress(actor.id, ProgressScope::Recent(u32::MAX))
            .await?;
        Ok(list
            .into_iter()
            .filter(|p| bound_ids.contains(&p.book_id.0))
            .count() as u64)
    }

    pub async fn count_user_bookmarks(&self, actor: &User) -> DomainResult<u64> {
        let bound_ids = self
            .bindings
            .list_bound_ids(actor.id, MediaKind::Book)
            .await?;
        Ok(self
            .readings
            .list_bookmarks(actor.id, None)
            .await?
            .into_iter()
            .filter(|bm| bound_ids.contains(&bm.book_id.0))
            .count() as u64)
    }

    pub async fn count_user_favorites(&self, actor: &User) -> DomainResult<u64> {
        let bound_ids = self
            .bindings
            .list_bound_ids(actor.id, MediaKind::Book)
            .await?;
        Ok(self
            .readings
            .list_favorite_book_ids(actor.id)
            .await?
            .into_iter()
            .filter(|id| bound_ids.contains(&id.0))
            .count() as u64)
    }

    pub async fn create_bookmark(
        &self,
        actor: &User,
        book_id: BookId,
        cmd: CreateBookmarkCommand,
    ) -> DomainResult<Bookmark> {
        self.ensure_book_bound(actor, book_id).await?;
        let bookmark = Bookmark::create(
            actor.id,
            book_id,
            cmd.title,
            cmd.note,
            cmd.progress_percent,
            cmd.locator,
            cmd.chapter_index,
            cmd.page_number,
            cmd.scroll_ratio,
        )?;
        self.readings
            .save(ReadingMutation::Bookmark(&bookmark))
            .await?;
        Ok(bookmark)
    }

    pub async fn delete_bookmark(&self, actor: &User, bookmark_id: BookmarkId) -> DomainResult<()> {
        self.readings
            .delete_scope(ReadingDeleteScope::Bookmark {
                user_id: actor.id,
                bookmark_id,
            })
            .await
    }

    pub async fn set_favorite(
        &self,
        actor: &User,
        book_id: BookId,
        favorite: bool,
    ) -> DomainResult<()> {
        self.ensure_book_bound(actor, book_id).await?;
        self.readings
            .save(ReadingMutation::Favorite {
                user_id: actor.id,
                book_id,
                favorite,
            })
            .await
    }

    pub async fn batch_set_favorite(
        &self,
        actor: &User,
        book_ids: Vec<BookId>,
        favorite: bool,
    ) -> DomainResult<usize> {
        let bound_ids: HashSet<uuid::Uuid> = self
            .bindings
            .list_bound_ids(actor.id, MediaKind::Book)
            .await?;
        let mut count = 0;
        for book_id in book_ids {
            if bound_ids.contains(&book_id.0)
                && self
                    .books
                    .query(BookCollectionQuery::Id(book_id))
                    .await?
                    .into_iter()
                    .next()
                    .is_some()
            {
                self.readings
                    .save(ReadingMutation::Favorite {
                        user_id: actor.id,
                        book_id,
                        favorite,
                    })
                    .await?;
                count += 1;
            }
        }
        Ok(count)
    }

    pub async fn list_shelves(&self, actor: &User) -> DomainResult<Vec<ShelfDto>> {
        let bound_ids = self
            .bindings
            .list_bound_ids(actor.id, MediaKind::Book)
            .await?;
        let shelves = self.readings.query_shelves(actor.id, None).await?;
        Ok(shelves
            .into_iter()
            .map(|mut s| {
                s.retain_bound_books(&bound_ids);
                ShelfDto::from(&s)
            })
            .collect())
    }

    pub async fn create_shelf(&self, actor: &User, name: String) -> DomainResult<ShelfDto> {
        let shelf = Shelf::new(actor.id, name, false)?;
        self.readings.save(ReadingMutation::Shelf(&shelf)).await?;
        Ok(ShelfDto::from(&shelf))
    }

    pub async fn rename_shelf(
        &self,
        actor: &User,
        shelf_id: ShelfId,
        name: String,
    ) -> DomainResult<ShelfDto> {
        let mut shelf = self
            .readings
            .query_shelves(actor.id, Some(shelf_id))
            .await?
            .into_iter()
            .next()
            .ok_or_else(|| DomainError::NotFound(format!("shelf {shelf_id} not found")))?;
        if shelf.is_favorite {
            return Err(DomainError::Validation(
                "default favorites shelf cannot be renamed".into(),
            ));
        }
        shelf.rename(name)?;
        self.readings.save(ReadingMutation::Shelf(&shelf)).await?;
        Ok(ShelfDto::from(&shelf))
    }

    pub async fn delete_shelf(&self, actor: &User, shelf_id: ShelfId) -> DomainResult<()> {
        let shelf = self
            .readings
            .query_shelves(actor.id, Some(shelf_id))
            .await?
            .into_iter()
            .next()
            .ok_or_else(|| DomainError::NotFound(format!("shelf {shelf_id} not found")))?;
        if shelf.is_favorite {
            return Err(DomainError::Validation(
                "default favorites shelf cannot be deleted".into(),
            ));
        }
        self.readings
            .delete_scope(ReadingDeleteScope::Shelf {
                user_id: actor.id,
                shelf_id,
            })
            .await
    }

    pub async fn add_book_to_shelf(
        &self,
        actor: &User,
        shelf_id: ShelfId,
        book_id: BookId,
    ) -> DomainResult<ShelfDto> {
        self.ensure_book_bound(actor, book_id).await?;
        let mut shelf = self
            .readings
            .query_shelves(actor.id, Some(shelf_id))
            .await?
            .into_iter()
            .next()
            .ok_or_else(|| DomainError::NotFound(format!("shelf {shelf_id} not found")))?;
        shelf.set_contains(book_id, true);
        self.readings.save(ReadingMutation::Shelf(&shelf)).await?;
        Ok(ShelfDto::from(&shelf))
    }

    pub async fn remove_book_from_shelf(
        &self,
        actor: &User,
        shelf_id: ShelfId,
        book_id: BookId,
    ) -> DomainResult<ShelfDto> {
        let mut shelf = self
            .readings
            .query_shelves(actor.id, Some(shelf_id))
            .await?
            .into_iter()
            .next()
            .ok_or_else(|| DomainError::NotFound(format!("shelf {shelf_id} not found")))?;
        shelf.set_contains(book_id, false);
        self.readings.save(ReadingMutation::Shelf(&shelf)).await?;
        Ok(ShelfDto::from(&shelf))
    }

    async fn ensure_book_bound(&self, actor: &User, book_id: BookId) -> DomainResult<()> {
        self.bindings
            .require_bound(
                actor.id,
                MediaKind::Book,
                book_id.0,
                &format!("book {book_id} not found"),
            )
            .await?;
        if self
            .books
            .query(BookCollectionQuery::Id(book_id))
            .await?
            .into_iter()
            .next()
            .is_none()
        {
            return Err(DomainError::NotFound(format!("book {book_id} not found")));
        }
        Ok(())
    }
}

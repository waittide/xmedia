use std::collections::HashMap;
use std::sync::Arc;

use serde::{Deserialize, Serialize};
use xmedia_core::domain::error::{DomainError, DomainResult};
use xmedia_core::domain::user::User;

use crate::application::catalog::BookCatalogService;
use crate::application::dto::{BookSummaryDto, ShelfDto};
use crate::domain::book::{Book, BookId};
use crate::domain::reading::{
    Bookmark, BookmarkId, ReadingLocation, ReadingProgress, Shelf, ShelfId,
};
use crate::domain::repository::{BookReadingRepository, BookRepository};

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
}

impl BookReadingService {
    pub fn new(readings: Arc<dyn BookReadingRepository>, books: Arc<dyn BookRepository>) -> Self {
        Self { readings, books }
    }

    pub async fn get_progress(
        &self,
        actor: &User,
        book_id: BookId,
    ) -> DomainResult<Option<ReadingProgress>> {
        self.readings.get_progress(actor.id, book_id).await
    }

    pub async fn save_progress(
        &self,
        actor: &User,
        book_id: BookId,
        cmd: SaveProgressCommand,
    ) -> DomainResult<ReadingProgress> {
        self.ensure_book_exists(book_id).await?;
        let location = ReadingLocation::new(
            cmd.progress_percent,
            cmd.locator,
            cmd.chapter_index,
            cmd.page_number,
            cmd.scroll_ratio,
        )?;

        let progress = match self.readings.get_progress(actor.id, book_id).await? {
            Some(mut existing) => {
                existing.update_location(location);
                existing
            }
            None => ReadingProgress::new(actor.id, book_id, location),
        };

        self.readings.save_progress(&progress).await?;
        Ok(progress)
    }

    pub async fn list_recent_books(
        &self,
        actor: &User,
        limit: u32,
        catalog: &BookCatalogService,
    ) -> DomainResult<Vec<BookSummaryDto>> {
        let recent = self
            .readings
            .list_recent_progress(actor.id, limit.clamp(1, 100))
            .await?;
        if recent.is_empty() {
            return Ok(Vec::new());
        }

        let ids: Vec<BookId> = recent.iter().map(|r| r.book_id).collect();
        let found = self.books.find_by_ids(&ids).await?;
        let by_id: HashMap<BookId, Book> = found.into_iter().map(|b| (b.id, b)).collect();

        let ordered: Vec<Book> = ids
            .into_iter()
            .filter_map(|id| by_id.get(&id).cloned())
            .collect();

        catalog.enrich_books(Some(actor), &ordered).await
    }

    pub async fn list_bookmarks(
        &self,
        actor: &User,
        book_id: BookId,
    ) -> DomainResult<Vec<Bookmark>> {
        self.readings.list_bookmarks(actor.id, book_id).await
    }

    pub async fn create_bookmark(
        &self,
        actor: &User,
        book_id: BookId,
        cmd: CreateBookmarkCommand,
    ) -> DomainResult<Bookmark> {
        self.ensure_book_exists(book_id).await?;
        let location = ReadingLocation::new(
            cmd.progress_percent,
            cmd.locator,
            cmd.chapter_index,
            cmd.page_number,
            cmd.scroll_ratio,
        )?;
        let bookmark = Bookmark::new(actor.id, book_id, cmd.title, cmd.note, location)?;
        self.readings.save_bookmark(&bookmark).await?;
        Ok(bookmark)
    }

    pub async fn delete_bookmark(&self, actor: &User, bookmark_id: BookmarkId) -> DomainResult<()> {
        self.readings.delete_bookmark(actor.id, bookmark_id).await
    }

    pub async fn set_favorite(
        &self,
        actor: &User,
        book_id: BookId,
        favorite: bool,
    ) -> DomainResult<()> {
        self.ensure_book_exists(book_id).await?;
        self.readings
            .set_favorite(actor.id, book_id, favorite)
            .await
    }

    pub async fn batch_set_favorite(
        &self,
        actor: &User,
        book_ids: Vec<BookId>,
        favorite: bool,
    ) -> DomainResult<usize> {
        let mut count = 0;
        for book_id in book_ids {
            if self.books.find_by_id(book_id).await?.is_some() {
                self.readings
                    .set_favorite(actor.id, book_id, favorite)
                    .await?;
                count += 1;
            }
        }
        Ok(count)
    }

    pub async fn list_shelves(&self, actor: &User) -> DomainResult<Vec<ShelfDto>> {
        let shelves = self.readings.list_shelves(actor.id).await?;
        Ok(shelves.iter().map(ShelfDto::from).collect())
    }

    pub async fn create_shelf(&self, actor: &User, name: String) -> DomainResult<ShelfDto> {
        let shelf = Shelf::new(actor.id, name, false)?;
        self.readings.save_shelf(&shelf).await?;
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
            .find_shelf(actor.id, shelf_id)
            .await?
            .ok_or_else(|| DomainError::NotFound(format!("shelf {shelf_id} not found")))?;
        if shelf.is_favorite {
            return Err(DomainError::Validation(
                "default favorites shelf cannot be renamed".into(),
            ));
        }
        shelf.rename(name)?;
        self.readings.save_shelf(&shelf).await?;
        Ok(ShelfDto::from(&shelf))
    }

    pub async fn delete_shelf(&self, actor: &User, shelf_id: ShelfId) -> DomainResult<()> {
        let shelf = self
            .readings
            .find_shelf(actor.id, shelf_id)
            .await?
            .ok_or_else(|| DomainError::NotFound(format!("shelf {shelf_id} not found")))?;
        if shelf.is_favorite {
            return Err(DomainError::Validation(
                "default favorites shelf cannot be deleted".into(),
            ));
        }
        self.readings.delete_shelf(actor.id, shelf_id).await
    }

    pub async fn add_book_to_shelf(
        &self,
        actor: &User,
        shelf_id: ShelfId,
        book_id: BookId,
    ) -> DomainResult<ShelfDto> {
        self.ensure_book_exists(book_id).await?;
        let mut shelf = self
            .readings
            .find_shelf(actor.id, shelf_id)
            .await?
            .ok_or_else(|| DomainError::NotFound(format!("shelf {shelf_id} not found")))?;
        shelf.add_book(book_id);
        self.readings.save_shelf(&shelf).await?;
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
            .find_shelf(actor.id, shelf_id)
            .await?
            .ok_or_else(|| DomainError::NotFound(format!("shelf {shelf_id} not found")))?;
        shelf.remove_book(book_id);
        self.readings.save_shelf(&shelf).await?;
        Ok(ShelfDto::from(&shelf))
    }

    async fn ensure_book_exists(&self, book_id: BookId) -> DomainResult<()> {
        if self.books.find_by_id(book_id).await?.is_none() {
            return Err(DomainError::NotFound(format!("book {book_id} not found")));
        }
        Ok(())
    }
}

use std::collections::HashSet;

use async_trait::async_trait;
use jiff::Timestamp;
use toasty::Db;
use uuid::Uuid;
use xmedia_core::domain::error::DomainResult;
use xmedia_core::domain::user::UserId;

use super::db_err;
use crate::domain::book::BookId;
use crate::domain::reading::{Bookmark, BookmarkId, ReadingProgress, Shelf, ShelfId};
use crate::domain::repository::BookReadingRepository;
use crate::infra::persistence::models::{
    BookBookmarkModel, BookFavoriteModel, BookReadingProgressModel, BookShelfModel,
};

#[derive(Clone)]
pub struct ToastyBookReadingRepository {
    db: Db,
}

impl ToastyBookReadingRepository {
    pub fn new(db: Db) -> Self {
        Self { db }
    }

    fn pair_key(user_id: UserId, book_id: BookId) -> String {
        format!("{}:{}", user_id.0, book_id.0)
    }

    fn progress_to_domain(m: BookReadingProgressModel) -> ReadingProgress {
        ReadingProgress {
            user_id: UserId(m.user_id),
            book_id: BookId(m.book_id),
            location: m.location.0,
            last_read_at: m.last_read_at,
        }
    }

    fn bookmark_to_domain(m: BookBookmarkModel) -> Bookmark {
        Bookmark {
            id: BookmarkId(m.id),
            user_id: UserId(m.user_id),
            book_id: BookId(m.book_id),
            title: m.title,
            note: m.note,
            location: m.location.0,
            created_at: m.created_at,
        }
    }

    fn shelf_to_domain(m: BookShelfModel) -> Shelf {
        Shelf {
            id: ShelfId(m.id),
            user_id: UserId(m.user_id),
            name: m.name,
            is_favorite: m.is_favorite,
            book_ids: m.book_ids.0.into_iter().map(BookId).collect(),
            created_at: m.created_at,
            updated_at: m.updated_at,
        }
    }
}

#[async_trait]
impl BookReadingRepository for ToastyBookReadingRepository {
    async fn get_progress(
        &self,
        user_id: UserId,
        book_id: BookId,
    ) -> DomainResult<Option<ReadingProgress>> {
        let mut db = self.db.clone();
        let key = Self::pair_key(user_id, book_id);
        let row = BookReadingProgressModel::filter_by_progress_key(&key)
            .first()
            .exec(&mut db)
            .await
            .map_err(db_err)?;
        Ok(row.map(Self::progress_to_domain))
    }

    async fn list_recent_progress(
        &self,
        user_id: UserId,
        limit: u32,
    ) -> DomainResult<Vec<ReadingProgress>> {
        let mut db = self.db.clone();
        let mut rows = BookReadingProgressModel::filter_by_user_id(user_id.0)
            .exec(&mut db)
            .await
            .map_err(db_err)?;
        rows.sort_by_key(|a| std::cmp::Reverse(a.last_read_at));
        Ok(rows
            .into_iter()
            .take(limit as usize)
            .map(Self::progress_to_domain)
            .collect())
    }

    async fn count_user_progress(&self, user_id: UserId) -> DomainResult<u64> {
        let mut db = self.db.clone();
        let rows = BookReadingProgressModel::filter_by_user_id(user_id.0)
            .exec(&mut db)
            .await
            .map_err(db_err)?;
        Ok(rows.len() as u64)
    }

    async fn save_progress(&self, progress: &ReadingProgress) -> DomainResult<()> {
        let mut db = self.db.clone();
        let key = Self::pair_key(progress.user_id, progress.book_id);
        let existing = BookReadingProgressModel::filter_by_progress_key(&key)
            .first()
            .exec(&mut db)
            .await
            .map_err(db_err)?;

        if let Some(mut record) = existing {
            record
                .update()
                .location(toasty::Json(progress.location.clone()))
                .last_read_at(progress.last_read_at)
                .exec(&mut db)
                .await
                .map_err(db_err)?;
        } else {
            toasty::create!(BookReadingProgressModel {
                progress_key: key,
                user_id: progress.user_id.0,
                book_id: progress.book_id.0,
                location: toasty::Json(progress.location.clone()),
                last_read_at: progress.last_read_at,
            })
            .exec(&mut db)
            .await
            .map_err(db_err)?;
        }
        Ok(())
    }

    async fn list_bookmarks(
        &self,
        user_id: UserId,
        book_id: BookId,
    ) -> DomainResult<Vec<Bookmark>> {
        let mut db = self.db.clone();
        let mut rows = BookBookmarkModel::filter_by_user_id(user_id.0)
            .filter(BookBookmarkModel::fields().book_id().eq(book_id.0))
            .exec(&mut db)
            .await
            .map_err(db_err)?;
        rows.sort_by_key(|a| std::cmp::Reverse(a.created_at));
        Ok(rows.into_iter().map(Self::bookmark_to_domain).collect())
    }

    async fn list_user_bookmarks(&self, user_id: UserId) -> DomainResult<Vec<Bookmark>> {
        let mut db = self.db.clone();
        let mut rows = BookBookmarkModel::filter_by_user_id(user_id.0)
            .exec(&mut db)
            .await
            .map_err(db_err)?;
        rows.sort_by_key(|a| std::cmp::Reverse(a.created_at));
        Ok(rows.into_iter().map(Self::bookmark_to_domain).collect())
    }

    async fn save_bookmark(&self, bookmark: &Bookmark) -> DomainResult<()> {
        let mut db = self.db.clone();
        toasty::create!(BookBookmarkModel {
            id: bookmark.id.0,
            user_id: bookmark.user_id.0,
            book_id: bookmark.book_id.0,
            title: bookmark.title.clone(),
            note: bookmark.note.clone(),
            location: toasty::Json(bookmark.location.clone()),
            created_at: bookmark.created_at,
        })
        .exec(&mut db)
        .await
        .map_err(db_err)?;
        Ok(())
    }

    async fn delete_bookmark(&self, user_id: UserId, bookmark_id: BookmarkId) -> DomainResult<()> {
        let mut db = self.db.clone();
        if let Some(record) = BookBookmarkModel::filter_by_id(bookmark_id.0)
            .first()
            .exec(&mut db)
            .await
            .map_err(db_err)?
            && record.user_id == user_id.0
        {
            record.delete().exec(&mut db).await.map_err(db_err)?;
        }
        Ok(())
    }

    async fn is_favorite(&self, user_id: UserId, book_id: BookId) -> DomainResult<bool> {
        let mut db = self.db.clone();
        let key = Self::pair_key(user_id, book_id);
        let row = BookFavoriteModel::filter_by_fav_key(&key)
            .first()
            .exec(&mut db)
            .await
            .map_err(db_err)?;
        Ok(row.is_some())
    }

    async fn list_favorite_book_ids(&self, user_id: UserId) -> DomainResult<Vec<BookId>> {
        let mut db = self.db.clone();
        let mut rows = BookFavoriteModel::filter_by_user_id(user_id.0)
            .exec(&mut db)
            .await
            .map_err(db_err)?;
        rows.sort_by_key(|a| std::cmp::Reverse(a.created_at));
        Ok(rows.into_iter().map(|r| BookId(r.book_id)).collect())
    }

    async fn set_favorite(
        &self,
        user_id: UserId,
        book_id: BookId,
        favorite: bool,
    ) -> DomainResult<()> {
        let mut db = self.db.clone();
        let key = Self::pair_key(user_id, book_id);
        let existing = BookFavoriteModel::filter_by_fav_key(&key)
            .first()
            .exec(&mut db)
            .await
            .map_err(db_err)?;

        match (favorite, existing) {
            (true, None) => {
                toasty::create!(BookFavoriteModel {
                    fav_key: key,
                    user_id: user_id.0,
                    book_id: book_id.0,
                    created_at: Timestamp::now(),
                })
                .exec(&mut db)
                .await
                .map_err(db_err)?;
            }
            (false, Some(record)) => {
                record.delete().exec(&mut db).await.map_err(db_err)?;
            }
            _ => {}
        }
        Ok(())
    }

    async fn list_shelves(&self, user_id: UserId) -> DomainResult<Vec<Shelf>> {
        let mut db = self.db.clone();
        let mut rows = BookShelfModel::filter_by_user_id(user_id.0)
            .exec(&mut db)
            .await
            .map_err(db_err)?;
        rows.sort_by_key(|r| (!r.is_favorite, r.created_at));
        Ok(rows.into_iter().map(Self::shelf_to_domain).collect())
    }

    async fn find_shelf(&self, user_id: UserId, shelf_id: ShelfId) -> DomainResult<Option<Shelf>> {
        let mut db = self.db.clone();
        let row = BookShelfModel::filter_by_id(shelf_id.0)
            .first()
            .exec(&mut db)
            .await
            .map_err(db_err)?;
        Ok(row
            .filter(|r| r.user_id == user_id.0)
            .map(Self::shelf_to_domain))
    }

    async fn save_shelf(&self, shelf: &Shelf) -> DomainResult<()> {
        let mut db = self.db.clone();
        let existing = BookShelfModel::filter_by_id(shelf.id.0)
            .first()
            .exec(&mut db)
            .await
            .map_err(db_err)?;
        let raw_ids: Vec<Uuid> = shelf.book_ids.iter().map(|b| b.0).collect();

        if let Some(mut record) = existing {
            record
                .update()
                .name(shelf.name.clone())
                .is_favorite(shelf.is_favorite)
                .book_ids(toasty::Json(raw_ids))
                .updated_at(shelf.updated_at)
                .exec(&mut db)
                .await
                .map_err(db_err)?;
        } else {
            toasty::create!(BookShelfModel {
                id: shelf.id.0,
                user_id: shelf.user_id.0,
                name: shelf.name.clone(),
                is_favorite: shelf.is_favorite,
                book_ids: toasty::Json(raw_ids),
                created_at: shelf.created_at,
                updated_at: shelf.updated_at,
            })
            .exec(&mut db)
            .await
            .map_err(db_err)?;
        }
        Ok(())
    }

    async fn delete_shelf(&self, user_id: UserId, shelf_id: ShelfId) -> DomainResult<()> {
        let mut db = self.db.clone();
        if let Some(record) = BookShelfModel::filter_by_id(shelf_id.0)
            .first()
            .exec(&mut db)
            .await
            .map_err(db_err)?
            && record.user_id == user_id.0
        {
            record.delete().exec(&mut db).await.map_err(db_err)?;
        }
        Ok(())
    }

    async fn delete_by_book(&self, book_id: BookId) -> DomainResult<()> {
        self.delete_by_books(&[book_id]).await
    }

    async fn delete_by_books(&self, book_ids: &[BookId]) -> DomainResult<()> {
        if book_ids.is_empty() {
            return Ok(());
        }
        let mut db = self.db.clone();
        let raw_ids: Vec<Uuid> = book_ids.iter().map(|b| b.0).collect();
        let remove_set: HashSet<Uuid> = raw_ids.iter().copied().collect();

        if raw_ids.len() == 1 {
            let bid = raw_ids[0];
            BookReadingProgressModel::filter_by_book_id(bid)
                .delete()
                .exec(&mut db)
                .await
                .map_err(db_err)?;
            BookBookmarkModel::filter_by_book_id(bid)
                .delete()
                .exec(&mut db)
                .await
                .map_err(db_err)?;
            BookFavoriteModel::filter_by_book_id(bid)
                .delete()
                .exec(&mut db)
                .await
                .map_err(db_err)?;
        } else {
            BookReadingProgressModel::filter(
                BookReadingProgressModel::fields()
                    .book_id()
                    .in_list(raw_ids.clone()),
            )
            .delete()
            .exec(&mut db)
            .await
            .map_err(db_err)?;
            BookBookmarkModel::filter(
                BookBookmarkModel::fields()
                    .book_id()
                    .in_list(raw_ids.clone()),
            )
            .delete()
            .exec(&mut db)
            .await
            .map_err(db_err)?;
            BookFavoriteModel::filter(BookFavoriteModel::fields().book_id().in_list(raw_ids))
                .delete()
                .exec(&mut db)
                .await
                .map_err(db_err)?;
        }

        // Single pass over shelves for the entire batch of removed books.
        for mut shelf in BookShelfModel::all().exec(&mut db).await.map_err(db_err)? {
            if shelf.book_ids.0.is_empty() {
                continue;
            }
            if shelf.book_ids.0.iter().any(|id| remove_set.contains(id)) {
                let filtered: Vec<Uuid> = shelf
                    .book_ids
                    .0
                    .iter()
                    .copied()
                    .filter(|id| !remove_set.contains(id))
                    .collect();
                shelf
                    .update()
                    .book_ids(toasty::Json(filtered))
                    .updated_at(Timestamp::now())
                    .exec(&mut db)
                    .await
                    .map_err(db_err)?;
            }
        }
        Ok(())
    }

    async fn delete_by_user(&self, user_id: UserId) -> DomainResult<()> {
        let mut db = self.db.clone();
        BookReadingProgressModel::filter_by_user_id(user_id.0)
            .delete()
            .exec(&mut db)
            .await
            .map_err(db_err)?;
        BookBookmarkModel::filter_by_user_id(user_id.0)
            .delete()
            .exec(&mut db)
            .await
            .map_err(db_err)?;
        BookFavoriteModel::filter_by_user_id(user_id.0)
            .delete()
            .exec(&mut db)
            .await
            .map_err(db_err)?;
        BookShelfModel::filter_by_user_id(user_id.0)
            .delete()
            .exec(&mut db)
            .await
            .map_err(db_err)?;
        Ok(())
    }
}

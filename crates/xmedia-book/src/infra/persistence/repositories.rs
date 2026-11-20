use std::collections::{BTreeMap, HashSet};

use async_trait::async_trait;
use jiff::Timestamp;
use toasty::Db;
use uuid::Uuid;
use xmedia_core::domain::error::{DomainError, DomainResult};
use xmedia_core::domain::library::LibraryId;
use xmedia_core::domain::user::UserId;

use crate::domain::book::{
    Book, BookId, BookQuery, BookSortField, FacetCount, PagedResult, SortOrder, TagMatchMode,
};
use crate::domain::online::{BookSourceRecord, OnlineShelfBook};
use crate::domain::reading::{Bookmark, BookmarkId, ReadingProgress, Shelf, ShelfId};
use crate::domain::repository::{
    BookReadingRepository, BookRepository, BookSourceRepository, OnlineShelfRepository,
};
use crate::source::model::SourceRuntimeState;

use super::models::{
    BookBookmarkModel, BookFavoriteModel, BookModel, BookOnlineShelfModel,
    BookReadingProgressModel, BookShelfModel, BookSourceRecordModel,
};

fn db_err(err: impl std::fmt::Display) -> DomainError {
    DomainError::Persistence(err.to_string())
}

#[derive(Clone)]
pub struct ToastyBookRepository {
    db: Db,
}

impl ToastyBookRepository {
    pub fn new(db: Db) -> Self {
        Self { db }
    }

    fn to_domain(m: BookModel) -> Book {
        let format = m.parsed_format();
        Book {
            id: BookId(m.id),
            library_id: LibraryId(m.library_id),
            format,
            source: m.source.0,
            file_size: u64::try_from(m.file_size.max(0)).unwrap_or(0),
            file_hash: m.file_hash,
            cover_key: m.cover_key,
            metadata: m.metadata.0,
            is_missing: m.is_missing,
            created_at: m.created_at,
            updated_at: m.updated_at,
        }
    }
}

#[async_trait]
impl BookRepository for ToastyBookRepository {
    async fn find_by_id(&self, id: BookId) -> DomainResult<Option<Book>> {
        let mut db = self.db.clone();
        let row = BookModel::filter_by_id(id.0)
            .first()
            .exec(&mut db)
            .await
            .map_err(db_err)?;
        Ok(row.map(Self::to_domain))
    }

    async fn find_by_ids(&self, ids: &[BookId]) -> DomainResult<Vec<Book>> {
        if ids.is_empty() {
            return Ok(Vec::new());
        }
        let mut db = self.db.clone();
        let raw_ids: Vec<Uuid> = ids.iter().map(|i| i.0).collect();
        let rows = BookModel::filter(BookModel::fields().id().in_list(raw_ids))
            .exec(&mut db)
            .await
            .map_err(db_err)?;
        Ok(rows.into_iter().map(Self::to_domain).collect())
    }

    async fn list_by_library(&self, library_id: LibraryId) -> DomainResult<Vec<Book>> {
        let mut db = self.db.clone();
        let rows = BookModel::filter_by_library_id(library_id.0)
            .exec(&mut db)
            .await
            .map_err(db_err)?;
        Ok(rows.into_iter().map(Self::to_domain).collect())
    }

    async fn search(&self, query: &BookQuery) -> DomainResult<PagedResult<Book>> {
        let mut db = self.db.clone();

        let fav_filter: Option<HashSet<BookId>> = if let Some(user_id) = query.favorite_for_user {
            let favs = BookFavoriteModel::filter_by_user_id(user_id.0)
                .exec(&mut db)
                .await
                .map_err(db_err)?;
            Some(favs.into_iter().map(|f| BookId(f.book_id)).collect())
        } else {
            None
        };

        let shelf_filter: Option<HashSet<BookId>> = query
            .shelf_book_ids
            .as_ref()
            .map(|ids| ids.iter().copied().collect());

        let effective_lib_ids: Vec<Uuid> = query.library_ids.iter().map(|l| l.0).collect();

        let mut builder = BookModel::all();
        if effective_lib_ids.len() == 1 {
            builder = builder.filter(BookModel::fields().library_id().eq(effective_lib_ids[0]));
        } else if effective_lib_ids.len() > 1 {
            builder = builder.filter(BookModel::fields().library_id().in_list(effective_lib_ids));
        }

        if query.formats.len() == 1 {
            builder = builder.filter(
                BookModel::fields()
                    .format()
                    .eq(query.formats[0].as_str().to_string()),
            );
        } else if query.formats.len() > 1 {
            let fmt_strings: Vec<String> = query
                .formats
                .iter()
                .map(|f| f.as_str().to_string())
                .collect();
            builder = builder.filter(BookModel::fields().format().in_list(fmt_strings));
        }

        if !query.include_missing {
            builder = builder.filter(BookModel::fields().is_missing().eq(false));
        }

        let rows = builder.exec(&mut db).await.map_err(db_err)?;
        let keyword_lower = query.keyword.as_ref().map(|k| k.to_lowercase());
        let authors_lower: Vec<String> = query.authors.iter().map(|a| a.to_lowercase()).collect();
        let tags_lower: Vec<String> = query.tags.iter().map(|t| t.to_lowercase()).collect();

        let mut matched = Vec::new();
        for row in rows {
            let book = Self::to_domain(row);

            if let Some(favs) = &fav_filter
                && !favs.contains(&book.id)
            {
                continue;
            }
            if let Some(shelf_ids) = &shelf_filter
                && !shelf_ids.contains(&book.id)
            {
                continue;
            }
            if !authors_lower.is_empty() {
                let book_authors: Vec<String> = book
                    .metadata
                    .authors
                    .iter()
                    .map(|a| a.to_lowercase())
                    .collect();
                if !authors_lower.iter().any(|want| book_authors.contains(want)) {
                    continue;
                }
            }
            if !tags_lower.is_empty() {
                let book_tags: Vec<String> = book
                    .metadata
                    .tags
                    .iter()
                    .map(|t| t.to_lowercase())
                    .collect();
                let tag_ok = match query.tag_mode {
                    TagMatchMode::Any => tags_lower.iter().any(|want| book_tags.contains(want)),
                    TagMatchMode::All => tags_lower.iter().all(|want| book_tags.contains(want)),
                };
                if !tag_ok {
                    continue;
                }
            }
            if let Some(kw) = &keyword_lower {
                let in_title = book.metadata.title.to_lowercase().contains(kw);
                let in_author = book
                    .metadata
                    .authors
                    .iter()
                    .any(|a| a.to_lowercase().contains(kw));
                let in_tag = book
                    .metadata
                    .tags
                    .iter()
                    .any(|t| t.to_lowercase().contains(kw));
                let in_desc = book
                    .metadata
                    .description
                    .as_ref()
                    .is_some_and(|d| d.to_lowercase().contains(kw));
                let in_isbn = book
                    .metadata
                    .isbn
                    .as_ref()
                    .is_some_and(|i| i.to_lowercase().contains(kw));
                if !(in_title || in_author || in_tag || in_desc || in_isbn) {
                    continue;
                }
            }

            matched.push(book);
        }

        matched.sort_by(|a, b| {
            let ord = match query.sort_by {
                BookSortField::CreatedAt => a.created_at.cmp(&b.created_at),
                BookSortField::UpdatedAt => a.updated_at.cmp(&b.updated_at),
                BookSortField::Title => a
                    .metadata
                    .title
                    .to_lowercase()
                    .cmp(&b.metadata.title.to_lowercase()),
                BookSortField::FileSize => a.file_size.cmp(&b.file_size),
            };
            match query.sort_order {
                SortOrder::Asc => ord,
                SortOrder::Desc => ord.reverse(),
            }
        });

        let total = matched.len() as u64;
        let page = query.page.max(1);
        let page_size = query.page_size.max(1);
        let start = ((page - 1) as usize).saturating_mul(page_size as usize);
        let items = matched
            .into_iter()
            .skip(start)
            .take(page_size as usize)
            .collect();

        Ok(PagedResult {
            items,
            total,
            page,
            page_size,
        })
    }

    async fn count_by_library(&self, library_id: LibraryId) -> DomainResult<u64> {
        let mut db = self.db.clone();
        let rows = BookModel::filter_by_library_id(library_id.0)
            .filter(BookModel::fields().is_missing().eq(false))
            .exec(&mut db)
            .await
            .map_err(db_err)?;
        Ok(rows.len() as u64)
    }

    async fn list_authors(&self) -> DomainResult<Vec<FacetCount>> {
        let mut db = self.db.clone();
        let rows = BookModel::filter_by_is_missing(false)
            .exec(&mut db)
            .await
            .map_err(db_err)?;
        let mut counts: BTreeMap<String, (String, u64)> = BTreeMap::new();
        for row in rows {
            for author in &row.metadata.0.authors {
                let key = author.to_lowercase();
                let entry = counts.entry(key).or_insert_with(|| (author.clone(), 0));
                entry.1 += 1;
            }
        }
        let mut out: Vec<FacetCount> = counts
            .into_values()
            .map(|(name, count)| FacetCount { name, count })
            .collect();
        out.sort_by(|a, b| b.count.cmp(&a.count).then_with(|| a.name.cmp(&b.name)));
        Ok(out)
    }

    async fn list_tags(&self) -> DomainResult<Vec<FacetCount>> {
        let mut db = self.db.clone();
        let rows = BookModel::filter_by_is_missing(false)
            .exec(&mut db)
            .await
            .map_err(db_err)?;
        let mut counts: BTreeMap<String, (String, u64)> = BTreeMap::new();
        for row in rows {
            for tag in &row.metadata.0.tags {
                let key = tag.to_lowercase();
                let entry = counts.entry(key).or_insert_with(|| (tag.clone(), 0));
                entry.1 += 1;
            }
        }
        let mut out: Vec<FacetCount> = counts
            .into_values()
            .map(|(name, count)| FacetCount { name, count })
            .collect();
        out.sort_by(|a, b| b.count.cmp(&a.count).then_with(|| a.name.cmp(&b.name)));
        Ok(out)
    }

    async fn home_catalog_snapshot(
        &self,
        recent_limit: usize,
    ) -> DomainResult<(Vec<Book>, u64, u64, u64)> {
        let mut db = self.db.clone();
        let rows = BookModel::filter_by_is_missing(false)
            .exec(&mut db)
            .await
            .map_err(db_err)?;

        let total_books = rows.len() as u64;
        let mut author_keys = HashSet::new();
        let mut tag_keys = HashSet::new();
        let mut books: Vec<Book> = Vec::with_capacity(rows.len());

        for row in rows {
            for author in &row.metadata.0.authors {
                let clean = author.trim().to_lowercase();
                if !clean.is_empty() {
                    author_keys.insert(clean);
                }
            }
            for tag in &row.metadata.0.tags {
                let clean = tag.trim().to_lowercase();
                if !clean.is_empty() {
                    tag_keys.insert(clean);
                }
            }
            books.push(Self::to_domain(row));
        }

        books.sort_by_key(|b| std::cmp::Reverse(b.created_at));
        books.truncate(recent_limit);

        Ok((
            books,
            total_books,
            author_keys.len() as u64,
            tag_keys.len() as u64,
        ))
    }

    async fn save(&self, book: &Book) -> DomainResult<()> {
        let mut db = self.db.clone();
        let existing = BookModel::filter_by_id(book.id.0)
            .first()
            .exec(&mut db)
            .await
            .map_err(db_err)?;

        let title_lower = book.metadata.title.to_lowercase();
        let file_size = i64::try_from(book.file_size).unwrap_or(i64::MAX);

        if let Some(mut record) = existing {
            record
                .update()
                .library_id(book.library_id.0)
                .format(book.format.as_str().to_string())
                .title(book.metadata.title.clone())
                .title_lower(title_lower)
                .source(toasty::Json(book.source.clone()))
                .file_size(file_size)
                .file_hash(book.file_hash.clone())
                .cover_key(book.cover_key.clone())
                .metadata(toasty::Json(book.metadata.clone()))
                .is_missing(book.is_missing)
                .updated_at(book.updated_at)
                .exec(&mut db)
                .await
                .map_err(db_err)?;
        } else {
            toasty::create!(BookModel {
                id: book.id.0,
                library_id: book.library_id.0,
                format: book.format.as_str().to_string(),
                title: book.metadata.title.clone(),
                title_lower: title_lower,
                source: toasty::Json(book.source.clone()),
                file_size: file_size,
                file_hash: book.file_hash.clone(),
                cover_key: book.cover_key.clone(),
                metadata: toasty::Json(book.metadata.clone()),
                is_missing: book.is_missing,
                created_at: book.created_at,
                updated_at: book.updated_at,
            })
            .exec(&mut db)
            .await
            .map_err(db_err)?;
        }
        Ok(())
    }

    async fn delete(&self, id: BookId) -> DomainResult<()> {
        let mut db = self.db.clone();
        BookModel::filter_by_id(id.0)
            .delete()
            .exec(&mut db)
            .await
            .map_err(db_err)?;
        Ok(())
    }

    async fn delete_by_library(&self, library_id: LibraryId) -> DomainResult<()> {
        let mut db = self.db.clone();
        BookModel::filter_by_library_id(library_id.0)
            .delete()
            .exec(&mut db)
            .await
            .map_err(db_err)?;
        Ok(())
    }
}

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

#[derive(Clone)]
pub struct ToastyBookSourceRepository {
    db: Db,
}

impl ToastyBookSourceRepository {
    pub fn new(db: Db) -> Self {
        Self { db }
    }

    fn to_domain(m: BookSourceRecordModel) -> BookSourceRecord {
        BookSourceRecord {
            id: m.id,
            name: m.name,
            source_url: m.source_url,
            group_name: m.group_name,
            enabled: m.enabled,
            weight: m.weight,
            source_type: m.source_type,
            raw_json: m.raw_json,
            runtime_state: m.runtime_state.0,
            last_test_ok: m.last_test_ok,
            last_test_msg: m.last_test_msg,
            created_at: m.created_at,
            updated_at: m.updated_at,
        }
    }
}

#[async_trait]
impl BookSourceRepository for ToastyBookSourceRepository {
    async fn list_all(&self) -> DomainResult<Vec<BookSourceRecord>> {
        let mut db = self.db.clone();
        let rows = BookSourceRecordModel::all()
            .exec(&mut db)
            .await
            .map_err(db_err)?;
        Ok(rows.into_iter().map(Self::to_domain).collect())
    }

    async fn find_by_id(&self, id: Uuid) -> DomainResult<Option<BookSourceRecord>> {
        let mut db = self.db.clone();
        let row = BookSourceRecordModel::filter_by_id(id)
            .first()
            .exec(&mut db)
            .await
            .map_err(db_err)?;
        Ok(row.map(Self::to_domain))
    }

    async fn save(&self, source: &BookSourceRecord) -> DomainResult<()> {
        let mut db = self.db.clone();
        let existing = BookSourceRecordModel::filter_by_id(source.id)
            .first()
            .exec(&mut db)
            .await
            .map_err(db_err)?;
        if let Some(mut rec) = existing {
            rec.update()
                .name(source.name.clone())
                .source_url(source.source_url.clone())
                .group_name(source.group_name.clone())
                .enabled(source.enabled)
                .weight(source.weight)
                .source_type(source.source_type)
                .raw_json(source.raw_json.clone())
                .runtime_state(toasty::Json(source.runtime_state.clone()))
                .last_test_ok(source.last_test_ok)
                .last_test_msg(source.last_test_msg.clone())
                .updated_at(source.updated_at)
                .exec(&mut db)
                .await
                .map_err(db_err)?;
        } else {
            toasty::create!(BookSourceRecordModel {
                id: source.id,
                name: source.name.clone(),
                source_url: source.source_url.clone(),
                group_name: source.group_name.clone(),
                enabled: source.enabled,
                weight: source.weight,
                source_type: source.source_type,
                raw_json: source.raw_json.clone(),
                runtime_state: toasty::Json(source.runtime_state.clone()),
                last_test_ok: source.last_test_ok,
                last_test_msg: source.last_test_msg.clone(),
                created_at: source.created_at,
                updated_at: source.updated_at,
            })
            .exec(&mut db)
            .await
            .map_err(db_err)?;
        }
        Ok(())
    }

    async fn update_runtime_state(
        &self,
        id: Uuid,
        runtime_state: SourceRuntimeState,
        updated_at: Timestamp,
    ) -> DomainResult<()> {
        let mut db = self.db.clone();
        if let Some(mut rec) = BookSourceRecordModel::filter_by_id(id)
            .first()
            .exec(&mut db)
            .await
            .map_err(db_err)?
        {
            rec.update()
                .runtime_state(toasty::Json(runtime_state))
                .updated_at(updated_at)
                .exec(&mut db)
                .await
                .map_err(db_err)?;
        }
        Ok(())
    }

    async fn delete(&self, id: Uuid) -> DomainResult<()> {
        let mut db = self.db.clone();
        BookSourceRecordModel::filter_by_id(id)
            .delete()
            .exec(&mut db)
            .await
            .map_err(db_err)?;
        Ok(())
    }
}

#[derive(Clone)]
pub struct ToastyOnlineShelfRepository {
    db: Db,
}

impl ToastyOnlineShelfRepository {
    pub fn new(db: Db) -> Self {
        Self { db }
    }

    fn to_domain(m: BookOnlineShelfModel) -> OnlineShelfBook {
        OnlineShelfBook {
            id: m.id,
            user_id: UserId(m.user_id),
            source_id: m.source_id,
            name: m.name,
            author: m.author,
            book_url: m.book_url,
            toc_url: m.toc_url,
            cover_url: m.cover_url,
            intro: m.intro,
            kind: m.kind,
            last_chapter: m.last_chapter,
            total_chapters: m.total_chapters,
            current_chapter_index: m.current_chapter_index,
            current_chapter_title: m.current_chapter_title,
            scroll_ratio: m.scroll_ratio,
            created_at: m.created_at,
            updated_at: m.updated_at,
        }
    }
}

#[async_trait]
impl OnlineShelfRepository for ToastyOnlineShelfRepository {
    async fn list_by_user(&self, user_id: UserId) -> DomainResult<Vec<OnlineShelfBook>> {
        let mut db = self.db.clone();
        let mut rows = BookOnlineShelfModel::filter_by_user_id(user_id.0)
            .exec(&mut db)
            .await
            .map_err(db_err)?;
        rows.sort_by_key(|r| std::cmp::Reverse(r.updated_at));
        Ok(rows.into_iter().map(Self::to_domain).collect())
    }

    async fn find_by_id(&self, id: Uuid) -> DomainResult<Option<OnlineShelfBook>> {
        let mut db = self.db.clone();
        let row = BookOnlineShelfModel::filter_by_id(id)
            .first()
            .exec(&mut db)
            .await
            .map_err(db_err)?;
        Ok(row.map(Self::to_domain))
    }

    async fn save(&self, item: &OnlineShelfBook) -> DomainResult<()> {
        let mut db = self.db.clone();
        let existing = BookOnlineShelfModel::filter_by_id(item.id)
            .first()
            .exec(&mut db)
            .await
            .map_err(db_err)?;
        if let Some(mut rec) = existing {
            rec.update()
                .source_id(item.source_id)
                .name(item.name.clone())
                .author(item.author.clone())
                .book_url(item.book_url.clone())
                .toc_url(item.toc_url.clone())
                .cover_url(item.cover_url.clone())
                .intro(item.intro.clone())
                .kind(item.kind.clone())
                .last_chapter(item.last_chapter.clone())
                .total_chapters(item.total_chapters)
                .current_chapter_index(item.current_chapter_index)
                .current_chapter_title(item.current_chapter_title.clone())
                .scroll_ratio(item.scroll_ratio)
                .updated_at(item.updated_at)
                .exec(&mut db)
                .await
                .map_err(db_err)?;
        } else {
            toasty::create!(BookOnlineShelfModel {
                id: item.id,
                user_id: item.user_id.0,
                source_id: item.source_id,
                name: item.name.clone(),
                author: item.author.clone(),
                book_url: item.book_url.clone(),
                toc_url: item.toc_url.clone(),
                cover_url: item.cover_url.clone(),
                intro: item.intro.clone(),
                kind: item.kind.clone(),
                last_chapter: item.last_chapter.clone(),
                total_chapters: item.total_chapters,
                current_chapter_index: item.current_chapter_index,
                current_chapter_title: item.current_chapter_title.clone(),
                scroll_ratio: item.scroll_ratio,
                created_at: item.created_at,
                updated_at: item.updated_at,
            })
            .exec(&mut db)
            .await
            .map_err(db_err)?;
        }
        Ok(())
    }

    async fn delete(&self, id: Uuid) -> DomainResult<()> {
        let mut db = self.db.clone();
        BookOnlineShelfModel::filter_by_id(id)
            .delete()
            .exec(&mut db)
            .await
            .map_err(db_err)?;
        Ok(())
    }

    async fn delete_by_user(&self, user_id: UserId) -> DomainResult<()> {
        let mut db = self.db.clone();
        BookOnlineShelfModel::filter_by_user_id(user_id.0)
            .delete()
            .exec(&mut db)
            .await
            .map_err(db_err)?;
        Ok(())
    }
}

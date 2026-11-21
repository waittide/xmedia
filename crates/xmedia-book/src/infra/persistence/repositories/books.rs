use std::collections::{BTreeMap, HashSet};

use async_trait::async_trait;
use toasty::Db;
use uuid::Uuid;
use xmedia_core::domain::error::DomainResult;
use xmedia_core::domain::library::LibraryId;

use super::db_err;
use crate::domain::book::{
    Book, BookId, BookQuery, BookSortField, FacetCount, PagedResult, SortOrder, TagMatchMode,
};
use crate::domain::repository::BookRepository;
use crate::infra::persistence::models::{BookFavoriteModel, BookModel};

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

        let mut restricted_ids: Option<Vec<Uuid>> = match (&fav_filter, &shelf_filter) {
            (Some(favs), Some(shelf)) => Some(favs.intersection(shelf).map(|id| id.0).collect()),
            (Some(favs), None) => Some(favs.iter().map(|id| id.0).collect()),
            (None, Some(shelf)) => Some(shelf.iter().map(|id| id.0).collect()),
            (None, None) => None,
        };
        if let Some(ref mut ids) = restricted_ids {
            if ids.is_empty() {
                return Ok(PagedResult {
                    items: Vec::new(),
                    total: 0,
                    page: query.page,
                    page_size: query.page_size,
                });
            }
            ids.sort_unstable();
            ids.dedup();
        }

        let effective_lib_ids: Vec<Uuid> = query.library_ids.iter().map(|l| l.0).collect();

        let mut builder = BookModel::all();
        if let Some(ids) = restricted_ids {
            if ids.len() == 1 {
                builder = builder.filter(BookModel::fields().id().eq(ids[0]));
            } else {
                builder = builder.filter(BookModel::fields().id().in_list(ids));
            }
        }
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

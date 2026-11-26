use std::collections::HashSet;

use async_trait::async_trait;
use toasty::Db;
use uuid::Uuid;
use xmedia_core::domain::error::DomainResult;

use super::db_err;
use crate::domain::book::{Book, BookId, BookQuery, PagedResult};
use crate::domain::repository::{BookCollectionQuery, BookDeleteScope, BookRepository};
use crate::infra::persistence::models::{BookFavoriteModel, BookModel};

#[derive(Clone)]
pub struct ToastyBookRepository {
    db: Db,
}

impl ToastyBookRepository {
    pub fn new(db: Db) -> Self {
        Self { db }
    }
}

#[async_trait]
impl BookRepository for ToastyBookRepository {
    async fn query(&self, scope: BookCollectionQuery) -> DomainResult<Vec<Book>> {
        let mut db = self.db.clone();
        match scope {
            BookCollectionQuery::Id(id) => {
                let row = BookModel::filter_by_id(id.0)
                    .first()
                    .exec(&mut db)
                    .await
                    .map_err(db_err)?;
                Ok(row.map(BookModel::into_domain).into_iter().collect())
            }
            BookCollectionQuery::Ids(ids) => {
                if ids.is_empty() {
                    return Ok(Vec::new());
                }
                let mut raw_ids: Vec<Uuid> = ids.into_iter().map(|id| id.0).collect();
                raw_ids.sort_unstable();
                raw_ids.dedup();
                let rows = if raw_ids.len() == 1 {
                    BookModel::filter_by_id(raw_ids[0])
                        .exec(&mut db)
                        .await
                        .map_err(db_err)?
                } else {
                    BookModel::filter(BookModel::fields().id().in_list(raw_ids))
                        .exec(&mut db)
                        .await
                        .map_err(db_err)?
                };
                Ok(rows.into_iter().map(BookModel::into_domain).collect())
            }
            BookCollectionQuery::Library(library_id) => {
                let rows = BookModel::filter_by_library_id(library_id.0)
                    .exec(&mut db)
                    .await
                    .map_err(db_err)?;
                Ok(rows.into_iter().map(BookModel::into_domain).collect())
            }
        }
    }

    async fn list_all_ids(&self) -> DomainResult<Vec<BookId>> {
        let mut db = self.db.clone();
        let rows = BookModel::all().exec(&mut db).await.map_err(db_err)?;
        Ok(rows.into_iter().map(|m| BookId(m.id)).collect())
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

        let mut intersected: Option<HashSet<BookId>> = None;
        for set in [
            fav_filter.as_ref(),
            shelf_filter.as_ref(),
            query.allowed_book_ids.as_ref(),
        ]
        .into_iter()
        .flatten()
        {
            intersected = Some(match intersected {
                Some(prev) => prev.intersection(set).copied().collect(),
                None => set.clone(),
            });
        }
        let mut restricted_ids: Option<Vec<Uuid>> =
            intersected.map(|set| set.into_iter().map(|id| id.0).collect());
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
                    .eq(query.formats[0].to_string()),
            );
        } else if query.formats.len() > 1 {
            let fmt_strings: Vec<String> = query.formats.iter().map(ToString::to_string).collect();
            builder = builder.filter(BookModel::fields().format().in_list(fmt_strings));
        }

        if !query.include_missing {
            builder = builder.filter(BookModel::fields().is_missing().eq(false));
        }

        let rows = builder.exec(&mut db).await.map_err(db_err)?;
        let candidates: Vec<Book> = rows.into_iter().map(BookModel::into_domain).collect();
        Ok(query.evaluate(candidates, fav_filter.as_ref()))
    }

    async fn save(&self, book: &Book) -> DomainResult<()> {
        let mut db = self.db.clone();
        let existing = BookModel::filter_by_id(book.id.0)
            .first()
            .exec(&mut db)
            .await
            .map_err(db_err)?;

        let size_i64 = i64::try_from(book.file_size).unwrap_or(i64::MAX);
        let title = book.metadata.title.clone();
        let title_lower = title.to_lowercase();

        if let Some(mut record) = existing {
            record
                .update()
                .library_id(book.library_id.0)
                .format(book.format.to_string())
                .title(title)
                .title_lower(title_lower)
                .source(toasty::Json(book.source.clone()))
                .file_size(size_i64)
                .file_hash(book.file_hash.clone())
                .cover_key(book.cover_key.clone())
                .series(book.metadata.series.clone())
                .series_index(book.metadata.series_index)
                .rating(book.metadata.rating)
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
                format: book.format.to_string(),
                title: title,
                title_lower: title_lower,
                source: toasty::Json(book.source.clone()),
                file_size: size_i64,
                file_hash: book.file_hash.clone(),
                cover_key: book.cover_key.clone(),
                series: book.metadata.series.clone(),
                series_index: book.metadata.series_index,
                rating: book.metadata.rating,
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

    async fn delete_scope(&self, scope: BookDeleteScope) -> DomainResult<()> {
        let mut db = self.db.clone();
        match scope {
            BookDeleteScope::Id(id) => {
                BookModel::filter_by_id(id.0)
                    .delete()
                    .exec(&mut db)
                    .await
                    .map_err(db_err)?;
            }
            BookDeleteScope::Library(library_id) => {
                BookModel::filter_by_library_id(library_id.0)
                    .delete()
                    .exec(&mut db)
                    .await
                    .map_err(db_err)?;
            }
        }
        Ok(())
    }
}

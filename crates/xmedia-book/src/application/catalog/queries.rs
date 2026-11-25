use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::Path;

use xmedia_core::application::ports::{ByteRangeSpec, ByteRangeStream};
use xmedia_core::domain::error::{DomainError, DomainResult};
use xmedia_core::domain::library::{LibraryId, MediaKind};
use xmedia_core::domain::user::{User, UserId};

use super::BookCatalogService;
use crate::application::dto::{
    BookHomeOverviewDto, BookLibraryStatsDto, BookSummaryDto, CatalogSearchFilter,
    PoolBookSummaryDto,
};
use crate::application::ports::{BookAsset, CoverPayload, TocItem, TxtChapterContent};
use crate::application::reading::BookReadingService;
use crate::domain::book::{Book, BookFormat, BookId, BookQuery, FacetCount, PagedResult};
use crate::domain::reading::ReadingProgress;

impl BookCatalogService {
    pub async fn search_books(
        &self,
        actor: Option<&User>,
        filter: CatalogSearchFilter,
    ) -> DomainResult<PagedResult<BookSummaryDto>> {
        self.search_books_inner(actor, filter, true).await
    }

    pub async fn search_books_for_opds(
        &self,
        actor: Option<&User>,
        filter: CatalogSearchFilter,
    ) -> DomainResult<PagedResult<BookSummaryDto>> {
        self.search_books_inner(actor, filter, false).await
    }

    async fn search_books_inner(
        &self,
        actor: Option<&User>,
        filter: CatalogSearchFilter,
        include_user_state: bool,
    ) -> DomainResult<PagedResult<BookSummaryDto>> {
        let mut shelf_book_ids = None;
        if let Some(shelf_id) = filter.shelf_id {
            let actor = actor.ok_or_else(|| {
                DomainError::Unauthorized("sign-in required to filter by personal shelf".into())
            })?;
            let shelf = self
                .readings
                .find_shelf(actor.id, shelf_id)
                .await?
                .ok_or_else(|| DomainError::NotFound(format!("shelf {shelf_id} not found")))?;
            shelf_book_ids = Some(shelf.book_ids);
        }

        let favorite_for_user = if filter.favorites_only {
            let actor = actor.ok_or_else(|| {
                DomainError::Unauthorized("sign-in required to filter favorites".into())
            })?;
            Some(actor.id)
        } else {
            None
        };

        let mut authors: Vec<String> = filter
            .authors
            .into_iter()
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .collect();
        if let Some(single) = filter.author {
            let trimmed = single.trim();
            if !trimmed.is_empty() && !authors.iter().any(|a| a.eq_ignore_ascii_case(trimmed)) {
                authors.push(trimmed.to_string());
            }
        }

        let mut tags: Vec<String> = filter
            .tags
            .into_iter()
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .collect();
        if let Some(single) = filter.tag {
            let trimmed = single.trim();
            if !trimmed.is_empty() && !tags.iter().any(|t| t.eq_ignore_ascii_case(trimmed)) {
                tags.push(trimmed.to_string());
            }
        }

        let mut formats = filter.formats;
        if let Some(single) = filter.format
            && !formats.contains(&single)
        {
            formats.push(single);
        }

        let mut library_ids = filter.library_ids;
        if let Some(single) = filter.library_id
            && !library_ids.contains(&single)
        {
            library_ids.push(single);
        }

        let allowed_book_ids = if let Some(user) = actor {
            let bound = self
                .bindings
                .list_bound_ids(user.id, MediaKind::Book)
                .await?;
            Some(bound.into_iter().map(BookId).collect())
        } else {
            None
        };

        let query = BookQuery {
            keyword: filter.keyword.and_then(|s| {
                let t = s.trim().to_string();
                (!t.is_empty()).then_some(t)
            }),
            authors,
            tags,
            tag_mode: filter.tag_mode,
            formats,
            library_ids,
            favorite_for_user,
            shelf_book_ids,
            allowed_book_ids,
            exclude_online_tracked: false,
            include_missing: filter.include_missing,
            sort_by: filter.sort_by.unwrap_or_default(),
            sort_order: filter.sort_order.unwrap_or_default(),
            page: filter.page.unwrap_or(1).max(1),
            page_size: filter.page_size.unwrap_or(24).clamp(1, 200),
        };

        let page = self.books.search(&query).await?;
        let enrich_actor = if include_user_state { actor } else { None };
        let items = self.enrich_books(enrich_actor, &page.items).await?;

        Ok(PagedResult {
            items,
            total: page.total,
            page: page.page,
            page_size: page.page_size,
        })
    }

    pub async fn search_pool_books(
        &self,
        actor: &User,
        filter: CatalogSearchFilter,
    ) -> DomainResult<PagedResult<PoolBookSummaryDto>> {
        let mut authors = filter.authors;
        if let Some(single) = filter.author.and_then(|s| {
            let t = s.trim().to_string();
            (!t.is_empty()).then_some(t)
        }) && !authors.iter().any(|a| a.eq_ignore_ascii_case(&single))
        {
            authors.push(single);
        }
        let mut tags = filter.tags;
        if let Some(single) = filter.tag.and_then(|s| {
            let t = s.trim().to_string();
            (!t.is_empty()).then_some(t)
        }) && !tags.iter().any(|t| t.eq_ignore_ascii_case(&single))
        {
            tags.push(single);
        }
        let mut formats = filter.formats;
        if let Some(single) = filter.format
            && !formats.contains(&single)
        {
            formats.push(single);
        }
        let mut library_ids = filter.library_ids;
        if let Some(single) = filter.library_id
            && !library_ids.contains(&single)
        {
            library_ids.push(single);
        }

        let query = BookQuery {
            keyword: filter.keyword.and_then(|s| {
                let t = s.trim().to_string();
                (!t.is_empty()).then_some(t)
            }),
            authors,
            tags,
            tag_mode: filter.tag_mode,
            formats,
            library_ids,
            favorite_for_user: None,
            shelf_book_ids: None,
            allowed_book_ids: None,
            exclude_online_tracked: true,
            include_missing: false,
            sort_by: filter.sort_by.unwrap_or_default(),
            sort_order: filter.sort_order.unwrap_or_default(),
            page: filter.page.unwrap_or(1).max(1),
            page_size: filter.page_size.unwrap_or(24).clamp(1, 200),
        };

        let bound_ids = self
            .bindings
            .list_bound_ids(actor.id, MediaKind::Book)
            .await?;
        let page = self.books.search(&query).await?;
        let summaries = self.enrich_books(None, &page.items).await?;
        let items = summaries
            .into_iter()
            .map(|book| {
                let bound = bound_ids.contains(&book.id.0);
                PoolBookSummaryDto::new(book, bound)
            })
            .collect();

        Ok(PagedResult {
            items,
            total: page.total,
            page: page.page,
            page_size: page.page_size,
        })
    }

    async fn load_bound_active_books(&self, user_id: UserId) -> DomainResult<Vec<Book>> {
        let bound_ids = self
            .bindings
            .list_bound_ids(user_id, MediaKind::Book)
            .await?;
        if bound_ids.is_empty() {
            return Ok(Vec::new());
        }
        let ids: Vec<BookId> = bound_ids.into_iter().map(BookId).collect();
        let found = self.books.find_by_ids(&ids).await?;
        Ok(found.into_iter().filter(|b| !b.is_missing).collect())
    }

    pub async fn count_bound_books_by_library(
        &self,
        actor: &User,
    ) -> DomainResult<HashMap<LibraryId, u64>> {
        let books = self.load_bound_active_books(actor.id).await?;
        let mut counts: HashMap<LibraryId, u64> = HashMap::new();
        for b in books {
            *counts.entry(b.library_id).or_insert(0) += 1;
        }
        Ok(counts)
    }

    pub async fn list_authors(&self, actor: &User) -> DomainResult<Vec<FacetCount>> {
        let books = self.load_bound_active_books(actor.id).await?;
        let mut counts: BTreeMap<String, (String, u64)> = BTreeMap::new();
        for book in books {
            for author in &book.metadata.authors {
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

    pub async fn list_tags(&self, actor: &User) -> DomainResult<Vec<FacetCount>> {
        let books = self.load_bound_active_books(actor.id).await?;
        let mut counts: BTreeMap<String, (String, u64)> = BTreeMap::new();
        for book in books {
            for tag in &book.metadata.tags {
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

    pub async fn get_cover(
        &self,
        actor: Option<&User>,
        book_id: BookId,
    ) -> DomainResult<CoverPayload> {
        let book = self.find_book(book_id).await?;
        if book.source.is_online_tracked()
            && let Some(user) = actor
        {
            self.require_book_bound(user, book_id).await?;
        }
        let key = book
            .cover_key
            .ok_or_else(|| DomainError::NotFound(format!("cover for book {book_id} not found")))?;
        self.storage.read_cover(&key).await
    }

    pub async fn get_toc(&self, actor: &User, book_id: BookId) -> DomainResult<Vec<TocItem>> {
        self.require_book_bound(actor, book_id).await?;
        let (library, book) = self.find_book_with_library(book_id).await?;
        let parser = self.parser.clone();
        let format = book.format;
        if format == BookFormat::Epub
            && let Some(fs_path) = self.storage.resolve_book_fs_path(&library, &book)?
        {
            return tokio::task::spawn_blocking(move || {
                parser.list_toc_from_path(format, &fs_path)
            })
            .await
            .map_err(|e| DomainError::Storage(format!("list_toc task failed: {e}")))?;
        }
        let bytes = self.storage.read_book_bytes(&library, &book).await?;
        tokio::task::spawn_blocking(move || parser.list_toc(format, &bytes))
            .await
            .map_err(|e| DomainError::Storage(format!("list_toc task failed: {e}")))?
    }

    pub async fn get_txt_chapter(
        &self,
        actor: &User,
        book_id: BookId,
        chapter_index: u32,
    ) -> DomainResult<TxtChapterContent> {
        self.require_book_bound(actor, book_id).await?;
        let (library, book) = self.find_book_with_library(book_id).await?;
        if !book.format.is_text_chapter_based() {
            return Err(DomainError::Validation(
                "chapter endpoint is only supported for plain-text chapter formats (txt, mobi, azw3, fb2)".into(),
            ));
        }
        let bytes = self.storage.read_book_bytes(&library, &book).await?;
        let parser = self.parser.clone();
        let format = book.format;
        tokio::task::spawn_blocking(move || parser.read_text_chapter(format, &bytes, chapter_index))
            .await
            .map_err(|e| DomainError::Storage(format!("read_text_chapter task failed: {e}")))?
    }

    pub async fn get_epub_resource(
        &self,
        actor: &User,
        book_id: BookId,
        resource_path: &str,
    ) -> DomainResult<BookAsset> {
        self.require_book_bound(actor, book_id).await?;
        let (library, book) = self.find_book_with_library(book_id).await?;
        if book.format != BookFormat::Epub {
            return Err(DomainError::Validation(
                "resource endpoint is only supported for EPUB books".into(),
            ));
        }
        let parser = self.parser.clone();
        let res_path = resource_path.to_string();
        if let Some(fs_path) = self.storage.resolve_book_fs_path(&library, &book)? {
            return tokio::task::spawn_blocking(move || {
                parser.read_epub_resource_from_path(&fs_path, &res_path)
            })
            .await
            .map_err(|e| DomainError::Storage(format!("read_epub_resource task failed: {e}")))?;
        }
        let bytes = self.storage.read_book_bytes(&library, &book).await?;
        tokio::task::spawn_blocking(move || parser.read_epub_resource(&bytes, &res_path))
            .await
            .map_err(|e| DomainError::Storage(format!("read_epub_resource task failed: {e}")))?
    }

    pub async fn open_book_file_stream(
        &self,
        actor: &User,
        book_id: BookId,
        spec: ByteRangeSpec,
    ) -> DomainResult<(Book, ByteRangeStream)> {
        self.open_book_file_stream_for_user(actor.id, book_id, spec)
            .await
    }

    pub(crate) async fn open_book_file_stream_for_user(
        &self,
        user_id: UserId,
        book_id: BookId,
        spec: ByteRangeSpec,
    ) -> DomainResult<(Book, ByteRangeStream)> {
        self.bindings
            .require_bound(
                user_id,
                MediaKind::Book,
                book_id.0,
                &format!("book {book_id} not found"),
            )
            .await?;
        let (library, book) = self.find_book_with_library(book_id).await?;
        let stream = self.storage.open_book_stream(&library, &book, spec).await?;
        Ok((book, stream))
    }

    pub(crate) async fn open_path_stream(
        &self,
        path: &Path,
        spec: ByteRangeSpec,
    ) -> DomainResult<ByteRangeStream> {
        self.storage.open_path_stream(path, spec).await
    }

    fn enrich_books_with_maps(
        books: &[Book],
        lib_names: &HashMap<LibraryId, String>,
        fav_set: &HashSet<BookId>,
        progress_map: &HashMap<BookId, ReadingProgress>,
    ) -> Vec<BookSummaryDto> {
        books
            .iter()
            .map(|b| {
                BookSummaryDto::from_domain(
                    b,
                    lib_names.get(&b.library_id).cloned(),
                    fav_set.contains(&b.id),
                    progress_map.get(&b.id),
                )
            })
            .collect()
    }

    pub async fn enrich_books(
        &self,
        actor: Option<&User>,
        books: &[Book],
    ) -> DomainResult<Vec<BookSummaryDto>> {
        if books.is_empty() {
            return Ok(Vec::new());
        }

        let libs = self.libraries.list_by_media(MediaKind::Book).await?;
        let lib_names: HashMap<_, _> = libs.into_iter().map(|l| (l.id, l.name)).collect();

        let (fav_set, progress_map) = if let Some(actor) = actor {
            let fav_ids = self.readings.list_favorite_book_ids(actor.id).await?;
            let favs: HashSet<BookId> = fav_ids.into_iter().collect();

            let all_progress = self
                .readings
                .list_recent_progress(actor.id, u32::MAX)
                .await?;
            let progresses: HashMap<BookId, ReadingProgress> =
                all_progress.into_iter().map(|p| (p.book_id, p)).collect();
            (favs, progresses)
        } else {
            (HashSet::new(), HashMap::new())
        };

        Ok(Self::enrich_books_with_maps(
            books,
            &lib_names,
            &fav_set,
            &progress_map,
        ))
    }

    pub async fn home_overview(
        &self,
        actor: &User,
        reading_service: &BookReadingService,
    ) -> DomainResult<BookHomeOverviewDto> {
        let mut bound_books = self.load_bound_active_books(actor.id).await?;
        let total_books = bound_books.len() as u64;
        let bound_id_set: HashSet<BookId> = bound_books.iter().map(|b| b.id).collect();
        let mut author_keys = HashSet::new();
        let mut tag_keys = HashSet::new();
        let mut lib_id_set = HashSet::new();
        for book in &bound_books {
            lib_id_set.insert(book.library_id);
            for author in &book.metadata.authors {
                let clean = author.trim().to_lowercase();
                if !clean.is_empty() {
                    author_keys.insert(clean);
                }
            }
            for tag in &book.metadata.tags {
                let clean = tag.trim().to_lowercase();
                if !clean.is_empty() {
                    tag_keys.insert(clean);
                }
            }
        }
        let authors_count = author_keys.len() as u64;
        let tags_count = tag_keys.len() as u64;
        let libraries_count = lib_id_set.len() as u64;

        let by_id: HashMap<BookId, Book> = bound_books.iter().map(|b| (b.id, b.clone())).collect();
        bound_books.sort_by_key(|b| std::cmp::Reverse(b.created_at));
        bound_books.truncate(12);
        let latest_raw = bound_books;

        let libs = self.libraries.list_by_media(MediaKind::Book).await?;
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
            .list_recent_progress(actor.id, u32::MAX)
            .await?
            .into_iter()
            .filter(|p| bound_id_set.contains(&p.book_id))
            .collect();
        let reading_count = all_progress.len() as u64;
        let recent_ids: Vec<BookId> = all_progress.iter().take(6).map(|r| r.book_id).collect();
        let progress_map: HashMap<BookId, ReadingProgress> =
            all_progress.into_iter().map(|p| (p.book_id, p)).collect();

        let recently_added =
            Self::enrich_books_with_maps(&latest_raw, &lib_names, &fav_set, &progress_map);

        let continue_reading = if recent_ids.is_empty() {
            Vec::new()
        } else {
            let ordered: Vec<Book> = recent_ids
                .into_iter()
                .filter_map(|id| by_id.get(&id).cloned())
                .collect();
            Self::enrich_books_with_maps(&ordered, &lib_names, &fav_set, &progress_map)
        };

        let bookmarks_count = reading_service.count_user_bookmarks(actor).await?;
        let shelves = reading_service.list_shelves(actor).await?;

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
}

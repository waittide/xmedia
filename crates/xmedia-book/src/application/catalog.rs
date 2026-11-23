use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::Path;
use std::sync::Arc;

use bytes::Bytes;
use uuid::Uuid;
use xmedia_core::application::bindings::{
    BindMediaItemsInput, BindMediaItemsResultDto, MediaBindingService, UnbindMediaItemResultDto,
};
use xmedia_core::application::ports::{ByteRangeSpec, ByteRangeStream};
use xmedia_core::domain::error::{DomainError, DomainResult};
use xmedia_core::domain::library::{Library, LibraryId, MediaKind};
use xmedia_core::domain::repository::LibraryRepository;
use xmedia_core::domain::user::{User, UserId, require_admin};

use crate::application::dto::{
    BookHomeOverviewDto, BookLibraryStatsDto, BookSummaryDto, CatalogSearchFilter,
    PoolBookSummaryDto, UpdateMetadataPatch,
};
use crate::application::online::validate_safe_cover_image;
use crate::application::ports::{
    BookAsset, BookParserPort, BookStoragePort, CoverPayload, StoredManagedBook, TocItem,
    TxtChapterContent,
};
use crate::application::reading::BookReadingService;
use crate::domain::book::{
    Book, BookFormat, BookId, BookQuery, BookSource, FacetCount, PagedResult,
};
use crate::domain::reading::ReadingProgress;
use crate::domain::repository::{BookReadingRepository, BookRepository};

type BookDeletedHook = Arc<dyn Fn(BookId) + Send + Sync>;

pub struct BookCatalogService {
    books: Arc<dyn BookRepository>,
    libraries: Arc<dyn LibraryRepository>,
    readings: Arc<dyn BookReadingRepository>,
    bindings: Arc<MediaBindingService>,
    parser: Arc<dyn BookParserPort>,
    storage: Arc<dyn BookStoragePort>,
    on_book_deleted: std::sync::RwLock<Option<BookDeletedHook>>,
}

impl BookCatalogService {
    pub fn new(
        books: Arc<dyn BookRepository>,
        libraries: Arc<dyn LibraryRepository>,
        readings: Arc<dyn BookReadingRepository>,
        bindings: Arc<MediaBindingService>,
        parser: Arc<dyn BookParserPort>,
        storage: Arc<dyn BookStoragePort>,
    ) -> Self {
        Self {
            books,
            libraries,
            readings,
            bindings,
            parser,
            storage,
            on_book_deleted: std::sync::RwLock::new(None),
        }
    }

    pub fn bindings(&self) -> &Arc<MediaBindingService> {
        &self.bindings
    }

    pub async fn list_all_book_ids_for_backfill(&self) -> DomainResult<Vec<Uuid>> {
        Ok(self
            .books
            .list_all_ids()
            .await?
            .into_iter()
            .map(|id| id.0)
            .collect())
    }

    pub(crate) async fn bind_book_for_user(
        &self,
        user_id: UserId,
        book_id: BookId,
    ) -> DomainResult<bool> {
        self.bindings
            .bind(user_id, MediaKind::Book, book_id.0)
            .await
    }

    pub async fn require_book_bound(&self, actor: &User, book_id: BookId) -> DomainResult<()> {
        self.bindings
            .require_bound(
                actor.id,
                MediaKind::Book,
                book_id.0,
                &format!("book {book_id} not found"),
            )
            .await
    }

    pub(crate) fn set_on_book_deleted_hook(&self, hook: Arc<dyn Fn(BookId) + Send + Sync>) {
        let mut guard = self
            .on_book_deleted
            .write()
            .unwrap_or_else(|e| e.into_inner());
        *guard = Some(hook);
    }

    pub async fn upload_book(
        &self,
        actor: &User,
        file_name: &str,
        bytes: Bytes,
    ) -> DomainResult<BookSummaryDto> {
        require_admin(actor)?;
        if bytes.is_empty() {
            return Err(DomainError::Validation("uploaded file is empty".into()));
        }

        let path = Path::new(file_name);
        let ext = path
            .extension()
            .and_then(|s| s.to_str())
            .ok_or_else(|| DomainError::Validation("missing file extension".into()))?;
        let format = BookFormat::parse(ext)?;
        let fallback_title = path
            .file_stem()
            .and_then(|s| s.to_str())
            .filter(|s| !s.trim().is_empty())
            .unwrap_or("Untitled Book");

        let parser = self.parser.clone();
        let bytes_for_parse = bytes.clone();
        let fallback_owned = fallback_title.to_string();
        let parsed = tokio::task::spawn_blocking(move || {
            parser.parse_book(format, &bytes_for_parse, &fallback_owned)
        })
        .await
        .map_err(|e| DomainError::Storage(format!("book parse task failed: {e}")))??;

        let managed_lib = match self
            .libraries
            .find_managed_by_media(MediaKind::Book)
            .await?
        {
            Some(lib) => lib,
            None => {
                let lib =
                    Library::new_managed(MediaKind::Book, MediaKind::Book.default_managed_name())?;
                self.libraries.save(&lib).await?;
                lib
            }
        };

        let book_id = BookId::new();
        let stored = self
            .storage
            .save_managed_book(book_id, format, bytes)
            .await?;

        let cover_key = if let Some(cover) = &parsed.cover {
            Some(self.storage.save_cover(book_id, cover).await?)
        } else {
            None
        };

        let mut book = Book::new(
            managed_lib.id,
            format,
            BookSource::ManagedUpload {
                storage_key: stored.storage_key,
            },
            stored.file_size,
            stored.file_hash,
            cover_key,
            parsed.metadata,
        )?;
        book.id = book_id;

        self.books.save(&book).await?;
        self.bindings
            .bind(actor.id, MediaKind::Book, book.id.0)
            .await?;
        Ok(BookSummaryDto::from_domain(
            &book,
            Some(managed_lib.name),
            false,
            None,
        ))
    }

    pub async fn upload_book_from_path(
        &self,
        actor: &User,
        file_name: &str,
        temp_path: &Path,
    ) -> DomainResult<BookSummaryDto> {
        require_admin(actor)?;
        let data = tokio::fs::read(temp_path)
            .await
            .map_err(|e| DomainError::Storage(format!("failed reading temp uploaded book: {e}")))?;
        self.upload_book(actor, file_name, Bytes::from(data)).await
    }

    pub async fn search_books(
        &self,
        actor: Option<&User>,
        filter: CatalogSearchFilter,
    ) -> DomainResult<PagedResult<BookSummaryDto>> {
        self.search_books_inner(actor, filter, true).await
    }

    /// Search variant for OPDS feeds that still enforces `favorites_only`/`shelf_id` filtering
    /// via `actor` but skips per-user favorites and progress enrichment queries.
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
            authors: filter.authors,
            tags: filter.tags,
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
                PoolBookSummaryDto { book, bound }
            })
            .collect();

        Ok(PagedResult {
            items,
            total: page.total,
            page: page.page,
            page_size: page.page_size,
        })
    }

    pub async fn bind_books(
        &self,
        actor: &User,
        input: BindMediaItemsInput,
    ) -> DomainResult<BindMediaItemsResultDto> {
        let target_user_id = MediaBindingService::resolve_target_user_id(actor, input.user_id)?;
        let mut candidate_ids: Vec<Uuid> = Vec::new();

        if let Some(lib_id) = input.library_id {
            let lib_books = self.books.list_by_library(LibraryId(lib_id)).await?;
            for b in lib_books {
                if !b.is_missing && !b.source.is_online_tracked() {
                    candidate_ids.push(b.id.0);
                }
            }
        }

        if !input.item_ids.is_empty() {
            let requested: Vec<BookId> = input.item_ids.iter().copied().map(BookId).collect();
            let found = self.books.find_by_ids(&requested).await?;
            for b in found {
                if b.is_missing {
                    continue;
                }
                if !b.source.is_online_tracked()
                    || actor.role.is_admin()
                    || self
                        .bindings
                        .is_bound(actor.id, MediaKind::Book, b.id.0)
                        .await?
                {
                    candidate_ids.push(b.id.0);
                }
            }
        }

        let total_requested = candidate_ids.len();
        let bound_count = self
            .bindings
            .bind_batch(target_user_id, MediaKind::Book, &candidate_ids)
            .await?;
        Ok(BindMediaItemsResultDto {
            bound_count,
            total_requested,
        })
    }

    pub async fn unbind_book(
        &self,
        actor: &User,
        book_id: BookId,
        requested_user_id: Option<Uuid>,
    ) -> DomainResult<UnbindMediaItemResultDto> {
        let target_user_id = MediaBindingService::resolve_target_user_id(actor, requested_user_id)?;
        self.readings
            .delete_by_user_and_book(target_user_id, book_id)
            .await?;
        let (unbound, remaining_bindings) = self
            .bindings
            .unbind(target_user_id, MediaKind::Book, book_id.0)
            .await?;

        let mut deleted_from_pool = false;
        if remaining_bindings == 0
            && let Some(book) = self.books.find_by_id(book_id).await?
            && book.source.is_online_tracked()
        {
            self.delete_book_internal(book).await?;
            deleted_from_pool = true;
        }

        Ok(UnbindMediaItemResultDto {
            unbound,
            remaining_bindings,
            deleted_from_pool,
        })
    }

    pub async fn get_book(
        &self,
        actor: Option<&User>,
        book_id: BookId,
    ) -> DomainResult<BookSummaryDto> {
        if let Some(user) = actor {
            self.require_book_bound(user, book_id).await?;
        }
        let book = self.find_book(book_id).await?;
        let mut enriched = self.enrich_books(actor, &[book]).await?;
        enriched
            .pop()
            .ok_or_else(|| DomainError::NotFound(format!("book {book_id} not found")))
    }

    pub async fn update_metadata(
        &self,
        actor: &User,
        book_id: BookId,
        patch: UpdateMetadataPatch,
    ) -> DomainResult<BookSummaryDto> {
        require_admin(actor)?;
        let mut book = self.find_book(book_id).await?;
        let mut meta = book.metadata.clone();

        if let Some(title) = patch.title {
            meta.title = title;
        }
        if let Some(authors) = patch.authors {
            meta.authors = authors;
        }
        if let Some(description) = patch.description {
            meta.description = Some(description);
        }
        if let Some(isbn) = patch.isbn {
            meta.isbn = Some(isbn);
        }
        if let Some(language) = patch.language {
            meta.language = Some(language);
        }
        if let Some(publisher) = patch.publisher {
            meta.publisher = Some(publisher);
        }
        if let Some(published_at) = patch.published_at {
            meta.published_at = Some(published_at);
        }
        if let Some(tags) = patch.tags {
            meta.tags = tags;
        }

        book.update_metadata(meta)?;
        self.books.save(&book).await?;
        self.get_book(Some(actor), book_id).await
    }

    pub async fn batch_update_tags(
        &self,
        actor: &User,
        book_ids: Vec<BookId>,
        add_tags: Vec<String>,
        remove_tags: Vec<String>,
    ) -> DomainResult<usize> {
        require_admin(actor)?;
        let remove_lower: HashSet<String> = remove_tags
            .into_iter()
            .map(|t| t.trim().to_lowercase())
            .collect();

        let books = self.books.find_by_ids(&book_ids).await?;
        let mut count = 0;
        for mut book in books {
            let mut next_tags: Vec<String> = book
                .metadata
                .tags
                .iter()
                .filter(|t| !remove_lower.contains(&t.to_lowercase()))
                .cloned()
                .collect();
            next_tags.extend(add_tags.iter().cloned());
            book.set_tags(next_tags);
            self.books.save(&book).await?;
            count += 1;
        }
        Ok(count)
    }

    pub async fn replace_cover(
        &self,
        actor: &User,
        book_id: BookId,
        bytes: Bytes,
        mime_type: String,
    ) -> DomainResult<BookSummaryDto> {
        require_admin(actor)?;
        if bytes.is_empty() {
            return Err(DomainError::Validation("cover file cannot be empty".into()));
        }
        let safe_mime = validate_safe_cover_image(Some(&mime_type), &bytes)?;
        let mut book = self.find_book(book_id).await?;
        let old_key = book.cover_key.clone();
        let new_key = self
            .storage
            .save_cover(
                book.id,
                &CoverPayload {
                    bytes,
                    mime_type: safe_mime,
                },
            )
            .await?;
        book.set_cover(Some(new_key.clone()));
        self.books.save(&book).await?;
        if let Some(old_key) = old_key
            && old_key != new_key
        {
            let _ = self.storage.delete_cover(&old_key).await;
        }
        self.get_book(Some(actor), book_id).await
    }

    pub async fn delete_book(&self, actor: &User, book_id: BookId) -> DomainResult<()> {
        require_admin(actor)?;
        let book = self.find_book(book_id).await?;
        self.delete_book_internal(book).await
    }

    pub(crate) async fn delete_book_internal(&self, book: Book) -> DomainResult<()> {
        let book_id = book.id;
        self.bindings
            .delete_item_bindings(MediaKind::Book, book_id.0)
            .await?;
        self.readings.delete_by_book(book_id).await?;
        self.books.delete(book_id).await?;
        if let Some(storage_key) = book.source.storage_key()
            && let Err(err) = self.storage.delete_managed_book(storage_key).await
        {
            tracing::warn!(book_id = %book_id, error = %err, "failed to remove managed book file after DB deletion");
        }
        if let Some(cover_key) = &book.cover_key
            && let Err(err) = self.storage.delete_cover(cover_key).await
        {
            tracing::warn!(book_id = %book_id, error = %err, "failed to remove book cover after DB deletion");
        }
        if let Some(hook) = self
            .on_book_deleted
            .read()
            .unwrap_or_else(|e| e.into_inner())
            .clone()
        {
            hook(book_id);
        }
        Ok(())
    }

    pub(crate) async fn ensure_managed_library(&self) -> DomainResult<Library> {
        match self
            .libraries
            .find_managed_by_media(MediaKind::Book)
            .await?
        {
            Some(lib) => Ok(lib),
            None => {
                let lib =
                    Library::new_managed(MediaKind::Book, MediaKind::Book.default_managed_name())?;
                self.libraries.save(&lib).await?;
                Ok(lib)
            }
        }
    }

    pub(crate) async fn find_book_raw(&self, book_id: BookId) -> DomainResult<Book> {
        self.find_book(book_id).await
    }

    pub(crate) async fn find_online_tracked_book(
        &self,
        source_id: uuid::Uuid,
        book_url: &str,
    ) -> DomainResult<Option<Book>> {
        let Some(lib) = self
            .libraries
            .find_managed_by_media(MediaKind::Book)
            .await?
        else {
            return Ok(None);
        };
        let list = self.books.list_by_library(lib.id).await?;
        Ok(list.into_iter().find(|b| {
            matches!(
                &b.source,
                BookSource::OnlineTracked {
                    source_id: sid,
                    book_url: url,
                    ..
                } if *sid == source_id && url == book_url
            )
        }))
    }

    pub(crate) async fn save_book_raw(&self, book: &Book) -> DomainResult<()> {
        self.books.save(book).await
    }

    pub(crate) async fn save_managed_epub_and_cover(
        &self,
        book_id: BookId,
        epub_bytes: Bytes,
        cover: Option<CoverPayload>,
    ) -> DomainResult<(StoredManagedBook, Option<String>)> {
        let stored = self
            .storage
            .save_managed_book(book_id, BookFormat::Epub, epub_bytes)
            .await?;
        let cover_key = if let Some(payload) = cover {
            Some(self.storage.save_cover(book_id, &payload).await?)
        } else {
            None
        };
        Ok((stored, cover_key))
    }

    pub(crate) async fn delete_managed_book_file(&self, storage_key: &str) -> DomainResult<()> {
        self.storage.delete_managed_book(storage_key).await
    }

    pub(crate) async fn save_cover_raw(
        &self,
        book_id: BookId,
        payload: &CoverPayload,
    ) -> DomainResult<String> {
        self.storage.save_cover(book_id, payload).await
    }

    pub(crate) async fn read_cover_bytes(&self, cover_key: &str) -> Option<(Vec<u8>, String)> {
        self.storage
            .read_cover(cover_key)
            .await
            .ok()
            .map(|c| (c.bytes.to_vec(), c.mime_type))
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
        let bytes = self.storage.read_book_bytes(&library, &book).await?;
        let parser = self.parser.clone();
        let format = book.format;
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
        if book.format != BookFormat::Txt {
            return Err(DomainError::Validation(
                "chapter endpoint is only supported for TXT books".into(),
            ));
        }
        let bytes = self.storage.read_book_bytes(&library, &book).await?;
        let parser = self.parser.clone();
        tokio::task::spawn_blocking(move || parser.read_txt_chapter(&bytes, chapter_index))
            .await
            .map_err(|e| DomainError::Storage(format!("read_txt_chapter task failed: {e}")))?
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
        let bytes = self.storage.read_book_bytes(&library, &book).await?;
        let parser = self.parser.clone();
        let res_path = resource_path.to_string();
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

    fn enrich_books_with_maps(
        books: &[Book],
        lib_names: &HashMap<xmedia_core::domain::library::LibraryId, String>,
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

    async fn find_book(&self, book_id: BookId) -> DomainResult<Book> {
        self.books
            .find_by_id(book_id)
            .await?
            .ok_or_else(|| DomainError::NotFound(format!("book {book_id} not found")))
    }

    async fn find_book_with_library(&self, book_id: BookId) -> DomainResult<(Library, Book)> {
        let book = self.find_book(book_id).await?;
        if book.is_missing {
            return Err(DomainError::Validation(
                "book source file is currently marked missing on disk".into(),
            ));
        }
        let library = self
            .libraries
            .find_by_id(book.library_id)
            .await?
            .ok_or_else(|| {
                DomainError::NotFound(format!("library {} not found", book.library_id))
            })?;
        Ok((library, book))
    }
}

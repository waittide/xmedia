mod queries;

use std::collections::HashSet;
use std::path::Path;
use std::sync::Arc;

use bytes::Bytes;
use uuid::Uuid;
use xmedia_core::application::bindings::{
    BindMediaItemsInput, BindMediaItemsResultDto, MediaBindingService, UnbindMediaItemResultDto,
};
use xmedia_core::domain::error::{DomainError, DomainResult};
use xmedia_core::domain::library::{Library, LibraryId, MediaKind};
use xmedia_core::domain::repository::LibraryRepository;
use xmedia_core::domain::user::{User, UserId, require_admin};

use crate::application::dto::{BookSummaryDto, UpdateMetadataPatch};
use crate::application::online::validate_safe_cover_image;
use crate::application::ports::{BookParserPort, BookStoragePort, CoverPayload, StoredManagedBook};
use crate::domain::book::{Book, BookFormat, BookId, BookSource};
use crate::domain::repository::{BookReadingRepository, BookRepository};

type BookDeletedHook = Arc<dyn Fn(BookId) + Send + Sync>;

pub struct BookCatalogService {
    pub(super) books: Arc<dyn BookRepository>,
    pub(super) libraries: Arc<dyn LibraryRepository>,
    pub(super) readings: Arc<dyn BookReadingRepository>,
    pub(super) bindings: Arc<MediaBindingService>,
    pub(super) parser: Arc<dyn BookParserPort>,
    pub(super) storage: Arc<dyn BookStoragePort>,
    managed_lib_lock: tokio::sync::Mutex<()>,
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
            managed_lib_lock: tokio::sync::Mutex::new(()),
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

        let managed_lib = self.ensure_managed_library().await?;

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

        let book = Book::with_id(
            book_id,
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
        let (unbound, remaining_bindings, deleted_from_pool) = self
            .bindings
            .unbind_with_gc(target_user_id, MediaKind::Book, book_id.0, || async {
                if let Some(book) = self.books.find_by_id(book_id).await?
                    && book.source.is_online_tracked()
                {
                    self.delete_book_without_bindings(book).await?;
                    return Ok(true);
                }
                Ok(false)
            })
            .await?;

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
        book.apply_metadata_patch(patch)?;
        self.books.save(&book).await?;
        let mut enriched = self.enrich_books(Some(actor), &[book]).await?;
        enriched
            .pop()
            .ok_or_else(|| DomainError::NotFound(format!("book {book_id} not found")))
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
            .save_cover(book.id, &CoverPayload::new(bytes, safe_mime))
            .await?;
        book.set_cover(Some(new_key.clone()));
        self.books.save(&book).await?;
        if let Some(old_key) = old_key
            && old_key != new_key
        {
            let _ = self.storage.delete_cover(&old_key).await;
        }
        let mut enriched = self.enrich_books(Some(actor), &[book]).await?;
        enriched
            .pop()
            .ok_or_else(|| DomainError::NotFound(format!("book {book_id} not found")))
    }

    pub async fn delete_book(&self, actor: &User, book_id: BookId) -> DomainResult<()> {
        require_admin(actor)?;
        let book = self.find_book(book_id).await?;
        self.delete_book_internal(book).await
    }

    pub(crate) async fn delete_book_internal(&self, book: Book) -> DomainResult<()> {
        self.bindings
            .delete_item_bindings(MediaKind::Book, book.id.0)
            .await?;
        self.delete_book_without_bindings(book).await
    }

    pub(crate) async fn delete_book_without_bindings(&self, book: Book) -> DomainResult<()> {
        let book_id = book.id;
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
        let _guard = self.managed_lib_lock.lock().await;
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

    pub(super) async fn find_book(&self, book_id: BookId) -> DomainResult<Book> {
        self.books
            .find_by_id(book_id)
            .await?
            .ok_or_else(|| DomainError::NotFound(format!("book {book_id} not found")))
    }

    pub(super) async fn find_book_with_library(
        &self,
        book_id: BookId,
    ) -> DomainResult<(Library, Book)> {
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

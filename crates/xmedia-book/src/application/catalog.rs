use std::collections::{HashMap, HashSet};
use std::path::Path;
use std::sync::Arc;

use bytes::Bytes;
use xmedia_core::application::ports::{ByteRangeSpec, ByteRangeStream};
use xmedia_core::domain::error::{DomainError, DomainResult};
use xmedia_core::domain::library::{Library, MediaKind};
use xmedia_core::domain::repository::LibraryRepository;
use xmedia_core::domain::user::User;

use crate::application::dto::{BookSummaryDto, CatalogSearchFilter, UpdateMetadataPatch};
use crate::application::ports::{
    BookAsset, BookParserPort, BookStoragePort, CoverPayload, TocItem, TxtChapterContent,
};
use crate::domain::book::{
    Book, BookFormat, BookId, BookQuery, BookSource, FacetCount, PagedResult,
};
use crate::domain::reading::ReadingProgress;
use crate::domain::repository::{BookReadingRepository, BookRepository};

pub struct BookCatalogService {
    books: Arc<dyn BookRepository>,
    libraries: Arc<dyn LibraryRepository>,
    readings: Arc<dyn BookReadingRepository>,
    parser: Arc<dyn BookParserPort>,
    storage: Arc<dyn BookStoragePort>,
}

impl BookCatalogService {
    pub fn new(
        books: Arc<dyn BookRepository>,
        libraries: Arc<dyn LibraryRepository>,
        readings: Arc<dyn BookReadingRepository>,
        parser: Arc<dyn BookParserPort>,
        storage: Arc<dyn BookStoragePort>,
    ) -> Self {
        Self {
            books,
            libraries,
            readings,
            parser,
            storage,
        }
    }

    fn require_admin(actor: &User) -> DomainResult<()> {
        if actor.role.is_admin() {
            Ok(())
        } else {
            Err(DomainError::Forbidden(
                "administrator privileges required".into(),
            ))
        }
    }

    pub async fn upload_book(
        &self,
        actor: &User,
        file_name: &str,
        bytes: Bytes,
    ) -> DomainResult<BookSummaryDto> {
        Self::require_admin(actor)?;
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

        let parsed = self.parser.parse_book(format, &bytes, fallback_title)?;

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
        Ok(BookSummaryDto::from_domain(
            &book,
            Some(managed_lib.name),
            false,
            None,
        ))
    }

    pub async fn search_books(
        &self,
        actor: Option<&User>,
        filter: CatalogSearchFilter,
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
            include_missing: filter.include_missing,
            sort_by: filter.sort_by.unwrap_or_default(),
            sort_order: filter.sort_order.unwrap_or_default(),
            page: filter.page.unwrap_or(1).max(1),
            page_size: filter.page_size.unwrap_or(24).clamp(1, 200),
        };

        let page = self.books.search(&query).await?;
        let items = self.enrich_books(actor, &page.items).await?;

        Ok(PagedResult {
            items,
            total: page.total,
            page: page.page,
            page_size: page.page_size,
        })
    }

    pub async fn get_book(
        &self,
        actor: Option<&User>,
        book_id: BookId,
    ) -> DomainResult<BookSummaryDto> {
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
        Self::require_admin(actor)?;
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
        Self::require_admin(actor)?;
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
        Self::require_admin(actor)?;
        if bytes.is_empty() {
            return Err(DomainError::Validation("cover file cannot be empty".into()));
        }
        let mut book = self.find_book(book_id).await?;
        if let Some(old_key) = &book.cover_key {
            let _ = self.storage.delete_cover(old_key).await;
        }
        let key = self
            .storage
            .save_cover(book.id, &CoverPayload { bytes, mime_type })
            .await?;
        book.set_cover(Some(key));
        self.books.save(&book).await?;
        self.get_book(Some(actor), book_id).await
    }

    pub async fn delete_book(&self, actor: &User, book_id: BookId) -> DomainResult<()> {
        Self::require_admin(actor)?;
        let book = self.find_book(book_id).await?;
        if let BookSource::ManagedUpload { storage_key } = &book.source {
            let _ = self.storage.delete_managed_book(storage_key).await;
        }
        if let Some(cover_key) = &book.cover_key {
            let _ = self.storage.delete_cover(cover_key).await;
        }
        self.readings.delete_by_book(book_id).await?;
        self.books.delete(book_id).await
    }

    pub async fn list_authors(&self) -> DomainResult<Vec<FacetCount>> {
        self.books.list_authors().await
    }

    pub async fn list_tags(&self) -> DomainResult<Vec<FacetCount>> {
        self.books.list_tags().await
    }

    pub async fn get_cover(&self, book_id: BookId) -> DomainResult<CoverPayload> {
        let book = self.find_book(book_id).await?;
        let key = book
            .cover_key
            .ok_or_else(|| DomainError::NotFound(format!("cover for book {book_id} not found")))?;
        self.storage.read_cover(&key).await
    }

    pub async fn get_toc(&self, book_id: BookId) -> DomainResult<Vec<TocItem>> {
        let (library, book) = self.find_book_with_library(book_id).await?;
        let bytes = self.storage.read_book_bytes(&library, &book).await?;
        self.parser.list_toc(book.format, &bytes)
    }

    pub async fn get_txt_chapter(
        &self,
        book_id: BookId,
        chapter_index: u32,
    ) -> DomainResult<TxtChapterContent> {
        let (library, book) = self.find_book_with_library(book_id).await?;
        if book.format != BookFormat::Txt {
            return Err(DomainError::Validation(
                "chapter endpoint is only supported for TXT books".into(),
            ));
        }
        let bytes = self.storage.read_book_bytes(&library, &book).await?;
        self.parser.read_txt_chapter(&bytes, chapter_index)
    }

    pub async fn get_epub_resource(
        &self,
        book_id: BookId,
        resource_path: &str,
    ) -> DomainResult<BookAsset> {
        let (library, book) = self.find_book_with_library(book_id).await?;
        if book.format != BookFormat::Epub {
            return Err(DomainError::Validation(
                "resource endpoint is only supported for EPUB books".into(),
            ));
        }
        let bytes = self.storage.read_book_bytes(&library, &book).await?;
        self.parser.read_epub_resource(&bytes, resource_path)
    }

    pub async fn open_book_file_stream(
        &self,
        book_id: BookId,
        spec: ByteRangeSpec,
    ) -> DomainResult<(Book, ByteRangeStream)> {
        let (library, book) = self.find_book_with_library(book_id).await?;
        let stream = self.storage.open_book_stream(&library, &book, spec).await?;
        Ok((book, stream))
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

            let mut progresses: HashMap<BookId, ReadingProgress> = HashMap::new();
            for b in books {
                if let Some(p) = self.readings.get_progress(actor.id, b.id).await? {
                    progresses.insert(b.id, p);
                }
            }
            (favs, progresses)
        } else {
            (HashSet::new(), HashMap::new())
        };

        Ok(books
            .iter()
            .map(|b| {
                BookSummaryDto::from_domain(
                    b,
                    lib_names.get(&b.library_id).cloned(),
                    fav_set.contains(&b.id),
                    progress_map.get(&b.id),
                )
            })
            .collect())
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

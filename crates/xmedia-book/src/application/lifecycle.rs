use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use async_trait::async_trait;
use xmedia_core::application::library::ScanProgressReporter;
use xmedia_core::application::ports::{MediaDomainLifecycle, RealtimeEventPort};
use xmedia_core::domain::error::{DomainError, DomainResult};
use xmedia_core::domain::library::{Library, LibraryId, LibraryKind, MediaKind};
use xmedia_core::domain::repository::LibraryRepository;
use xmedia_core::domain::user::UserId;

use crate::application::ports::{BookParserPort, BookScannerPort, BookStoragePort};
use crate::domain::book::{Book, BookSource};
use crate::domain::repository::{BookReadingRepository, BookRepository, OnlineShelfRepository};

pub struct BookDomainLifecycle {
    libraries: Arc<dyn LibraryRepository>,
    books: Arc<dyn BookRepository>,
    readings: Arc<dyn BookReadingRepository>,
    online_shelves: Arc<dyn OnlineShelfRepository>,
    scanner: Arc<dyn BookScannerPort>,
    parser: Arc<dyn BookParserPort>,
    storage: Arc<dyn BookStoragePort>,
    events: Arc<dyn RealtimeEventPort>,
}

impl BookDomainLifecycle {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        libraries: Arc<dyn LibraryRepository>,
        books: Arc<dyn BookRepository>,
        readings: Arc<dyn BookReadingRepository>,
        online_shelves: Arc<dyn OnlineShelfRepository>,
        scanner: Arc<dyn BookScannerPort>,
        parser: Arc<dyn BookParserPort>,
        storage: Arc<dyn BookStoragePort>,
        events: Arc<dyn RealtimeEventPort>,
    ) -> Self {
        Self {
            libraries,
            books,
            readings,
            online_shelves,
            scanner,
            parser,
            storage,
            events,
        }
    }
}

#[async_trait]
impl MediaDomainLifecycle for BookDomainLifecycle {
    fn media_kind(&self) -> MediaKind {
        MediaKind::Book
    }

    async fn scan_library(&self, library: Library) -> DomainResult<()> {
        let root_path = match &library.kind {
            LibraryKind::LocalMount { root_path } => root_path.clone(),
            LibraryKind::Managed => {
                return Err(DomainError::Validation(
                    "managed upload library cannot be scanned".into(),
                ));
            }
        };

        let entries = self.scanner.scan_entries(&root_path).await?;
        let existing_books = self.books.list_by_library(library.id).await?;

        let entry_paths: HashSet<String> =
            entries.iter().map(|e| e.relative_path.clone()).collect();
        let mut existing_by_path: HashMap<String, Book> = HashMap::new();
        for book in existing_books {
            if let BookSource::MountedFile { relative_path, .. } = &book.source {
                existing_by_path.insert(relative_path.clone(), book);
            }
        }

        let library_id = library.id;
        let total = entries.len() as u64;
        let mut reporter = ScanProgressReporter::new(
            library,
            self.libraries.clone(),
            self.events.clone(),
            Some(total),
        );
        reporter.start().await;

        let mut seen_paths = HashSet::with_capacity(entries.len());
        let mut added = 0u64;
        let mut updated = 0u64;
        let mut missing = 0u64;

        for entry in entries {
            seen_paths.insert(entry.relative_path.clone());

            if let Some(mut existing) = existing_by_path.remove(&entry.relative_path) {
                let was_missing = existing.is_missing;
                let stored_mtime = match &existing.source {
                    BookSource::MountedFile { file_mtime, .. } => *file_mtime,
                    BookSource::ManagedUpload { .. } => 0,
                };
                let stats_unchanged =
                    existing.file_size == entry.file_size && stored_mtime == entry.file_mtime;

                if stats_unchanged && !was_missing {
                    // Nothing changed on disk or sidecars
                } else if stats_unchanged && was_missing {
                    // Missing file reappeared unchanged; preserve manual DB metadata
                    existing.mark_present(
                        entry.file_size,
                        existing.file_hash.clone(),
                        Some(entry.file_mtime),
                    );
                    self.books.save(&existing).await?;
                    updated += 1;
                } else if let Ok(loaded) = self
                    .scanner
                    .read_entry(&root_path, &entry.relative_path)
                    .await
                {
                    let parser = self.parser.clone();
                    let fmt = entry.format;
                    let bytes_for_parse = loaded.bytes.clone();
                    let fallback_title = entry.fallback_title.clone();
                    let sidecar_opf = loaded.sidecar_opf;
                    let sidecar_cover = loaded.sidecar_cover;
                    let parsed_res = tokio::task::spawn_blocking(move || {
                        parser.parse_book_with_sidecars(
                            fmt,
                            &bytes_for_parse,
                            &fallback_title,
                            sidecar_opf.as_deref(),
                            sidecar_cover,
                        )
                    })
                    .await
                    .ok()
                    .and_then(|r| r.ok());
                    if let Some(parsed) = parsed_res {
                        if let Some(cover) = &parsed.cover
                            && let Ok(cover_key) = self.storage.save_cover(existing.id, cover).await
                        {
                            existing.set_cover(Some(cover_key));
                        }
                        let _ = existing.update_metadata(parsed.metadata);
                    }

                    existing.mark_present(
                        entry.file_size,
                        loaded.file_hash,
                        Some(entry.file_mtime),
                    );
                    self.books.save(&existing).await?;
                    updated += 1;
                }
            } else if let Ok(loaded) = self
                .scanner
                .read_entry(&root_path, &entry.relative_path)
                .await
            {
                let parser = self.parser.clone();
                let fmt = entry.format;
                let bytes_for_parse = loaded.bytes.clone();
                let fallback_title = entry.fallback_title.clone();
                let sidecar_opf = loaded.sidecar_opf;
                let sidecar_cover = loaded.sidecar_cover;
                if let Some(parsed) = tokio::task::spawn_blocking(move || {
                    parser.parse_book_with_sidecars(
                        fmt,
                        &bytes_for_parse,
                        &fallback_title,
                        sidecar_opf.as_deref(),
                        sidecar_cover,
                    )
                })
                .await
                .ok()
                .and_then(|r| r.ok())
                {
                    // Check whether this is a moved/renamed existing book in the same library
                    let matched_old_key = existing_by_path
                        .iter()
                        .find(|(old_rel, old_book)| {
                            !entry_paths.contains(*old_rel)
                                && old_book.format == entry.format
                                && (old_book.file_hash == loaded.file_hash
                                    || old_book.file_hash == loaded.legacy_sha256)
                        })
                        .map(|(k, _)| k.clone());

                    if let Some(old_rel) = matched_old_key
                        && let Some(mut migrated) = existing_by_path.remove(&old_rel)
                    {
                        migrated.source = BookSource::MountedFile {
                            relative_path: entry.relative_path,
                            file_mtime: entry.file_mtime,
                        };
                        if let Some(cover) = &parsed.cover
                            && let Ok(cover_key) = self.storage.save_cover(migrated.id, cover).await
                        {
                            migrated.set_cover(Some(cover_key));
                        }
                        let _ = migrated.update_metadata(parsed.metadata);
                        migrated.mark_present(
                            entry.file_size,
                            loaded.file_hash,
                            Some(entry.file_mtime),
                        );
                        self.books.save(&migrated).await?;
                        updated += 1;
                    } else {
                        let mut book = Book::new(
                            library_id,
                            entry.format,
                            BookSource::MountedFile {
                                relative_path: entry.relative_path,
                                file_mtime: entry.file_mtime,
                            },
                            entry.file_size,
                            loaded.file_hash,
                            None,
                            parsed.metadata,
                        )?;

                        if let Some(cover) = &parsed.cover
                            && let Ok(cover_key) = self.storage.save_cover(book.id, cover).await
                        {
                            book.set_cover(Some(cover_key));
                        }

                        self.books.save(&book).await?;
                        added += 1;
                    }
                }
            }

            reporter.advance().await;
        }

        for (_, mut stale_book) in existing_by_path {
            if !seen_paths.contains(match &stale_book.source {
                BookSource::MountedFile { relative_path, .. } => relative_path.as_str(),
                BookSource::ManagedUpload { .. } => "",
            }) && !stale_book.is_missing
            {
                stale_book.mark_missing();
                self.books.save(&stale_book).await?;
                missing += 1;
            }
        }

        let total_scanned = seen_paths.len() as u64;
        reporter
            .complete_with_scanned(total_scanned, added, updated, missing)
            .await?;

        Ok(())
    }

    async fn count_library_items(&self, library_id: LibraryId) -> DomainResult<u64> {
        self.books.count_by_library(library_id).await
    }

    async fn on_library_deleted(&self, library_id: LibraryId) -> DomainResult<()> {
        let books = self.books.list_by_library(library_id).await?;
        let mut book_ids = Vec::with_capacity(books.len());
        for book in &books {
            if let BookSource::ManagedUpload { storage_key } = &book.source {
                let _ = self.storage.delete_managed_book(storage_key).await;
            }
            if let Some(cover_key) = &book.cover_key {
                let _ = self.storage.delete_cover(cover_key).await;
            }
            book_ids.push(book.id);
        }
        self.readings.delete_by_books(&book_ids).await?;
        self.books.delete_by_library(library_id).await
    }

    async fn on_user_deleted(&self, user_id: UserId) -> DomainResult<()> {
        self.readings.delete_by_user(user_id).await?;
        self.online_shelves.delete_by_user(user_id).await
    }
}

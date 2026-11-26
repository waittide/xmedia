use std::collections::HashMap;
use std::sync::Arc;

use jiff::Timestamp;
use xmedia_core::application::bindings::MediaBindingService;
use xmedia_core::domain::error::{DomainError, DomainResult};
use xmedia_core::domain::library::{LibraryId, MediaKind};
use xmedia_core::domain::repository::LibraryRepository;
use xmedia_core::domain::user::User;

use crate::application::catalog::ComicCatalogService;
use crate::application::dto::SeriesQueryFilter;
use crate::domain::chapter::{Chapter, ChapterSourceKind};
use crate::domain::id::{ChapterId, ComicShelfId, SeriesId};
use crate::domain::reading::ChapterProgress;
use crate::domain::repository::{
    ChapterQuery, ChapterRepository, ComicProgressScope, ComicReadingRepository, SeriesQuery,
    SeriesRepository,
};

#[derive(Debug, Clone)]
pub struct OpdsNavItem {
    pub id: String,
    pub title: String,
    pub summary: String,
    pub subsection_path: String,
    pub cover_url: Option<String>,
    pub updated_at: Timestamp,
}

#[derive(Debug, Clone)]
pub struct OpdsChapterEntry {
    pub id: ChapterId,
    pub series_title: String,
    pub title: String,
    pub summary: String,
    pub authors: Vec<String>,
    pub cover_url: Option<String>,
    pub page_count: u32,
    pub last_read_page: Option<u32>,
    pub last_read_at: Option<Timestamp>,
    pub can_download_archive: bool,
    pub archive_mime: &'static str,
    pub updated_at: Timestamp,
}

#[derive(Debug, Clone)]
pub enum OpdsFeedPayload {
    Navigation(Vec<OpdsNavItem>),
    Acquisition(Vec<OpdsChapterEntry>),
}

#[derive(Debug, Clone)]
pub enum OpdsFeedKind {
    Root,
    Continue,
    Latest,
    AllSeries(Option<String>),
    SeriesChapters(SeriesId),
    Libraries,
    LibrarySeries(LibraryId),
    Tags,
    TagSeries { name: String, is_genre: bool },
    Shelves,
    ShelfSeries(ComicShelfId),
    Favorites,
}

pub struct OpdsFeed {
    pub id: String,
    pub title: String,
    pub self_path: String,
    pub updated_at: Timestamp,
    pub payload: OpdsFeedPayload,
}

pub struct ComicOpdsService {
    libraries: Arc<dyn LibraryRepository>,
    series: Arc<dyn SeriesRepository>,
    chapters: Arc<dyn ChapterRepository>,
    reading: Arc<dyn ComicReadingRepository>,
    bindings: Arc<MediaBindingService>,
}

impl ComicOpdsService {
    pub fn new(
        libraries: Arc<dyn LibraryRepository>,
        series: Arc<dyn SeriesRepository>,
        chapters: Arc<dyn ChapterRepository>,
        reading: Arc<dyn ComicReadingRepository>,
        bindings: Arc<MediaBindingService>,
    ) -> Self {
        Self {
            libraries,
            series,
            chapters,
            reading,
            bindings,
        }
    }

    pub async fn serve_feed(
        &self,
        user: &User,
        catalog: &ComicCatalogService,
        opds_root: &str,
        ver_prefix: &str,
        kind: OpdsFeedKind,
    ) -> DomainResult<OpdsFeed> {
        match kind {
            OpdsFeedKind::Root => Ok(self.root_feed(ver_prefix)),
            OpdsFeedKind::Continue => {
                self.continue_reading_feed(user, catalog, opds_root, ver_prefix)
                    .await
            }
            OpdsFeedKind::Latest => {
                self.latest_series_feed(user, catalog, opds_root, ver_prefix)
                    .await
            }
            OpdsFeedKind::AllSeries(search) => {
                self.all_series_feed(user, catalog, search, opds_root, ver_prefix)
                    .await
            }
            OpdsFeedKind::SeriesChapters(series_id) => {
                self.series_chapters_feed(user, series_id, opds_root, ver_prefix)
                    .await
            }
            OpdsFeedKind::Libraries => self.libraries_feed(user, catalog, ver_prefix).await,
            OpdsFeedKind::LibrarySeries(lib_id) => {
                self.library_series_feed(user, catalog, lib_id, opds_root, ver_prefix)
                    .await
            }
            OpdsFeedKind::Tags => self.tags_feed(user, catalog, ver_prefix).await,
            OpdsFeedKind::TagSeries { name, is_genre } => {
                self.tag_series_feed(user, catalog, &name, is_genre, opds_root, ver_prefix)
                    .await
            }
            OpdsFeedKind::Shelves => self.shelves_feed(user, ver_prefix).await,
            OpdsFeedKind::ShelfSeries(shelf_id) => {
                self.shelf_series_feed(user, catalog, shelf_id, opds_root, ver_prefix)
                    .await
            }
            OpdsFeedKind::Favorites => {
                self.favorites_series_feed(user, catalog, opds_root, ver_prefix)
                    .await
            }
        }
    }

    fn root_feed(&self, ver_prefix: &str) -> OpdsFeed {
        let now = Timestamp::now();
        let items = vec![
            OpdsNavItem {
                id: "urn:xmedia:comic:opds:continue".into(),
                title: "Continue Reading / 继续阅读".into(),
                summary: "Resume chapters currently in progress".into(),
                subsection_path: format!("{ver_prefix}/continue"),
                cover_url: None,
                updated_at: now,
            },
            OpdsNavItem {
                id: "urn:xmedia:comic:opds:latest".into(),
                title: "Latest Updated / 最新更新系列".into(),
                summary: "Recently updated comic series".into(),
                subsection_path: format!("{ver_prefix}/latest"),
                cover_url: None,
                updated_at: now,
            },
            OpdsNavItem {
                id: "urn:xmedia:comic:opds:series".into(),
                title: "All Series / 全部系列".into(),
                summary: "Browse all comic series alphabetically".into(),
                subsection_path: format!("{ver_prefix}/series"),
                cover_url: None,
                updated_at: now,
            },
            OpdsNavItem {
                id: "urn:xmedia:comic:opds:libraries".into(),
                title: "Libraries / 书库".into(),
                summary: "Browse series grouped by library".into(),
                subsection_path: format!("{ver_prefix}/libraries"),
                cover_url: None,
                updated_at: now,
            },
            OpdsNavItem {
                id: "urn:xmedia:comic:opds:tags".into(),
                title: "Tags & Genres / 标签分类".into(),
                summary: "Browse series by tags and genres".into(),
                subsection_path: format!("{ver_prefix}/tags"),
                cover_url: None,
                updated_at: now,
            },
            OpdsNavItem {
                id: "urn:xmedia:comic:opds:shelves".into(),
                title: "Shelves & Favorites / 收藏书架".into(),
                summary: "Personal favorites and custom shelves".into(),
                subsection_path: format!("{ver_prefix}/shelves"),
                cover_url: None,
                updated_at: now,
            },
        ];

        OpdsFeed {
            id: "urn:xmedia:comic:opds:root".into(),
            title: "xmedia Comics OPDS Catalog".into(),
            self_path: ver_prefix.to_owned(),
            updated_at: now,
            payload: OpdsFeedPayload::Navigation(items),
        }
    }

    async fn continue_reading_feed(
        &self,
        user: &User,
        catalog: &ComicCatalogService,
        opds_root: &str,
        ver_prefix: &str,
    ) -> DomainResult<OpdsFeed> {
        let overview = catalog.home_overview(user).await?;
        let chapter_ids: Vec<ChapterId> = overview
            .continue_reading
            .iter()
            .map(|item| item.pointer.chapter_id)
            .collect();
        let mut chapter_map: HashMap<ChapterId, Chapter> = self
            .chapters
            .query(ChapterQuery::Ids(chapter_ids))
            .await?
            .into_iter()
            .map(|c| (c.id, c))
            .collect();
        let mut entries = Vec::with_capacity(overview.continue_reading.len());

        for item in overview.continue_reading {
            if let Some(ch) = chapter_map.remove(&item.pointer.chapter_id) {
                entries.push(to_opds_chapter(
                    opds_root,
                    &item.series.title,
                    &item.series.writers,
                    ch,
                    Some(item.pointer.current_page),
                    Some(item.pointer.last_read_at),
                ));
            }
        }

        Ok(OpdsFeed {
            id: "urn:xmedia:comic:opds:continue".into(),
            title: "Continue Reading".into(),
            self_path: format!("{ver_prefix}/continue"),
            updated_at: Timestamp::now(),
            payload: OpdsFeedPayload::Acquisition(entries),
        })
    }

    async fn latest_series_feed(
        &self,
        user: &User,
        catalog: &ComicCatalogService,
        opds_root: &str,
        ver_prefix: &str,
    ) -> DomainResult<OpdsFeed> {
        let list = catalog
            .list_series(
                user,
                SeriesQueryFilter {
                    sort_by: Some("updated_at".into()),
                    sort_dir: Some("desc".into()),
                    ..Default::default()
                },
            )
            .await?;

        let items = list
            .into_iter()
            .take(50)
            .map(|s| OpdsNavItem {
                id: format!("urn:xmedia:comic:series:{}", s.id),
                title: s.title,
                summary: format!("{} chapters", s.chapter_count),
                subsection_path: format!("{ver_prefix}/series/{}", s.id),
                cover_url: rewrite_opds_cover(opds_root, s.cover_url),
                updated_at: s.updated_at,
            })
            .collect();

        Ok(OpdsFeed {
            id: "urn:xmedia:comic:opds:latest".into(),
            title: "Latest Updated Series".into(),
            self_path: format!("{ver_prefix}/latest"),
            updated_at: Timestamp::now(),
            payload: OpdsFeedPayload::Navigation(items),
        })
    }

    async fn all_series_feed(
        &self,
        user: &User,
        catalog: &ComicCatalogService,
        search: Option<String>,
        opds_root: &str,
        ver_prefix: &str,
    ) -> DomainResult<OpdsFeed> {
        let list = catalog
            .list_series(
                user,
                SeriesQueryFilter {
                    search,
                    sort_by: Some("title".into()),
                    sort_dir: Some("asc".into()),
                    ..Default::default()
                },
            )
            .await?;

        let items = list
            .into_iter()
            .map(|s| OpdsNavItem {
                id: format!("urn:xmedia:comic:series:{}", s.id),
                title: s.title,
                summary: format!("{} chapters", s.chapter_count),
                subsection_path: format!("{ver_prefix}/series/{}", s.id),
                cover_url: rewrite_opds_cover(opds_root, s.cover_url),
                updated_at: s.updated_at,
            })
            .collect();

        Ok(OpdsFeed {
            id: "urn:xmedia:comic:opds:series".into(),
            title: "All Comic Series".into(),
            self_path: format!("{ver_prefix}/series"),
            updated_at: Timestamp::now(),
            payload: OpdsFeedPayload::Navigation(items),
        })
    }

    async fn libraries_feed(
        &self,
        user: &User,
        catalog: &ComicCatalogService,
        ver_prefix: &str,
    ) -> DomainResult<OpdsFeed> {
        let libs = self.libraries.list(Some(MediaKind::Comic)).await?;
        let counts = catalog.count_bound_series_by_library(user).await?;
        let mut items = Vec::with_capacity(libs.len());
        for l in libs {
            let count = counts.get(&l.id).copied().unwrap_or(0);
            if count == 0 {
                continue;
            }
            items.push(OpdsNavItem {
                id: format!("urn:xmedia:comic:library:{}", l.id),
                title: l.name,
                summary: format!("{count} series"),
                subsection_path: format!("{ver_prefix}/libraries/{}", l.id),
                cover_url: None,
                updated_at: l.updated_at,
            });
        }
        Ok(OpdsFeed {
            id: "urn:xmedia:comic:opds:libraries".into(),
            title: "Comic Libraries".into(),
            self_path: format!("{ver_prefix}/libraries"),
            updated_at: Timestamp::now(),
            payload: OpdsFeedPayload::Navigation(items),
        })
    }

    async fn library_series_feed(
        &self,
        user: &User,
        catalog: &ComicCatalogService,
        library_id: LibraryId,
        opds_root: &str,
        ver_prefix: &str,
    ) -> DomainResult<OpdsFeed> {
        let lib = self
            .libraries
            .find_by_id(library_id)
            .await?
            .filter(|l| l.media_kind == MediaKind::Comic)
            .ok_or_else(|| DomainError::NotFound("library not found".into()))?;
        let list = catalog
            .list_series(
                user,
                SeriesQueryFilter {
                    library_id: Some(library_id),
                    sort_by: Some("title".into()),
                    sort_dir: Some("asc".into()),
                    ..Default::default()
                },
            )
            .await?;
        if list.is_empty() {
            return Err(DomainError::NotFound("library not found".into()));
        }

        let items = list
            .into_iter()
            .map(|s| OpdsNavItem {
                id: format!("urn:xmedia:comic:series:{}", s.id),
                title: s.title,
                summary: format!("{} chapters", s.chapter_count),
                subsection_path: format!("{ver_prefix}/series/{}", s.id),
                cover_url: rewrite_opds_cover(opds_root, s.cover_url),
                updated_at: s.updated_at,
            })
            .collect();

        Ok(OpdsFeed {
            id: format!("urn:xmedia:comic:library:{library_id}"),
            title: lib.name,
            self_path: format!("{ver_prefix}/libraries/{library_id}"),
            updated_at: lib.updated_at,
            payload: OpdsFeedPayload::Navigation(items),
        })
    }

    async fn tags_feed(
        &self,
        user: &User,
        catalog: &ComicCatalogService,
        ver_prefix: &str,
    ) -> DomainResult<OpdsFeed> {
        let tax = catalog.taxonomy_overview(user).await?;
        let now = Timestamp::now();
        let mut items = Vec::new();
        for g in tax.genres {
            items.push(OpdsNavItem {
                id: format!("urn:xmedia:comic:genre:{}", g.name),
                title: format!("[Genre] {}", g.name),
                summary: format!("{} series", g.count),
                subsection_path: format!("{ver_prefix}/genres/{}", url_encode_segment(&g.name)),
                cover_url: None,
                updated_at: now,
            });
        }
        for t in tax.tags {
            items.push(OpdsNavItem {
                id: format!("urn:xmedia:comic:tag:{}", t.name),
                title: t.name.clone(),
                summary: format!("{} series", t.count),
                subsection_path: format!("{ver_prefix}/tags/{}", url_encode_segment(&t.name)),
                cover_url: None,
                updated_at: now,
            });
        }
        Ok(OpdsFeed {
            id: "urn:xmedia:comic:opds:tags".into(),
            title: "Tags & Genres".into(),
            self_path: format!("{ver_prefix}/tags"),
            updated_at: now,
            payload: OpdsFeedPayload::Navigation(items),
        })
    }

    async fn tag_series_feed(
        &self,
        user: &User,
        catalog: &ComicCatalogService,
        name: &str,
        is_genre: bool,
        opds_root: &str,
        ver_prefix: &str,
    ) -> DomainResult<OpdsFeed> {
        let filter = if is_genre {
            SeriesQueryFilter {
                genre: Some(name.to_owned()),
                ..Default::default()
            }
        } else {
            SeriesQueryFilter {
                tag: Some(name.to_owned()),
                ..Default::default()
            }
        };
        let list = catalog.list_series(user, filter).await?;
        let items = list
            .into_iter()
            .map(|s| OpdsNavItem {
                id: format!("urn:xmedia:comic:series:{}", s.id),
                title: s.title,
                summary: format!("{} chapters", s.chapter_count),
                subsection_path: format!("{ver_prefix}/series/{}", s.id),
                cover_url: rewrite_opds_cover(opds_root, s.cover_url),
                updated_at: s.updated_at,
            })
            .collect();

        let kind_seg = if is_genre { "genres" } else { "tags" };
        Ok(OpdsFeed {
            id: format!("urn:xmedia:comic:{kind_seg}:{name}"),
            title: name.to_owned(),
            self_path: format!("{ver_prefix}/{kind_seg}/{}", url_encode_segment(name)),
            updated_at: Timestamp::now(),
            payload: OpdsFeedPayload::Navigation(items),
        })
    }

    async fn shelves_feed(&self, user: &User, ver_prefix: &str) -> DomainResult<OpdsFeed> {
        let now = Timestamp::now();
        let bound_ids = self
            .bindings
            .list_bound_ids(user.id, MediaKind::Comic)
            .await?;
        let fav_count = self
            .reading
            .list_favorites(user.id)
            .await?
            .into_iter()
            .filter(|f| bound_ids.contains(&f.series_id.0))
            .count();
        let mut items = vec![OpdsNavItem {
            id: "urn:xmedia:comic:favorites".into(),
            title: "Favorites / 我的收藏".into(),
            summary: format!("{fav_count} favorite series"),
            subsection_path: format!("{ver_prefix}/favorites"),
            cover_url: None,
            updated_at: now,
        }];

        let shelves = self.reading.query_shelves(Some(user.id), None).await?;
        for sh in shelves {
            let count = sh
                .series_ids
                .iter()
                .filter(|sid| bound_ids.contains(&sid.0))
                .count();
            items.push(OpdsNavItem {
                id: format!("urn:xmedia:comic:shelf:{}", sh.id),
                title: sh.name,
                summary: format!("{count} series"),
                subsection_path: format!("{ver_prefix}/shelves/{}", sh.id),
                cover_url: None,
                updated_at: sh.updated_at,
            });
        }

        Ok(OpdsFeed {
            id: "urn:xmedia:comic:opds:shelves".into(),
            title: "Shelves & Favorites".into(),
            self_path: format!("{ver_prefix}/shelves"),
            updated_at: now,
            payload: OpdsFeedPayload::Navigation(items),
        })
    }

    async fn favorites_series_feed(
        &self,
        user: &User,
        catalog: &ComicCatalogService,
        opds_root: &str,
        ver_prefix: &str,
    ) -> DomainResult<OpdsFeed> {
        let list = catalog
            .list_series(
                user,
                SeriesQueryFilter {
                    favorites_only: true,
                    ..Default::default()
                },
            )
            .await?;

        let items = list
            .into_iter()
            .map(|s| OpdsNavItem {
                id: format!("urn:xmedia:comic:series:{}", s.id),
                title: s.title,
                summary: format!("{} chapters", s.chapter_count),
                subsection_path: format!("{ver_prefix}/series/{}", s.id),
                cover_url: rewrite_opds_cover(opds_root, s.cover_url),
                updated_at: s.updated_at,
            })
            .collect();

        Ok(OpdsFeed {
            id: "urn:xmedia:comic:favorites".into(),
            title: "Favorites".into(),
            self_path: format!("{ver_prefix}/favorites"),
            updated_at: Timestamp::now(),
            payload: OpdsFeedPayload::Navigation(items),
        })
    }

    async fn shelf_series_feed(
        &self,
        user: &User,
        catalog: &ComicCatalogService,
        shelf_id: ComicShelfId,
        opds_root: &str,
        ver_prefix: &str,
    ) -> DomainResult<OpdsFeed> {
        let shelf = self
            .reading
            .query_shelves(None, Some(shelf_id))
            .await?
            .into_iter()
            .next()
            .ok_or_else(|| DomainError::NotFound("shelf not found".into()))?;
        if shelf.user_id != user.id {
            return Err(DomainError::Forbidden("access denied to shelf".into()));
        }
        let list = catalog
            .list_series(
                user,
                SeriesQueryFilter {
                    shelf_id: Some(shelf.id),
                    ..Default::default()
                },
            )
            .await?;

        let items = list
            .into_iter()
            .map(|s| OpdsNavItem {
                id: format!("urn:xmedia:comic:series:{}", s.id),
                title: s.title,
                summary: format!("{} chapters", s.chapter_count),
                subsection_path: format!("{ver_prefix}/series/{}", s.id),
                cover_url: rewrite_opds_cover(opds_root, s.cover_url),
                updated_at: s.updated_at,
            })
            .collect();

        Ok(OpdsFeed {
            id: format!("urn:xmedia:comic:shelf:{}", shelf.id),
            title: shelf.name,
            self_path: format!("{ver_prefix}/shelves/{}", shelf.id),
            updated_at: shelf.updated_at,
            payload: OpdsFeedPayload::Navigation(items),
        })
    }

    async fn series_chapters_feed(
        &self,
        user: &User,
        series_id: SeriesId,
        opds_root: &str,
        ver_prefix: &str,
    ) -> DomainResult<OpdsFeed> {
        self.bindings
            .require_bound(user.id, MediaKind::Comic, series_id.0, "series not found")
            .await?;
        let series = self
            .series
            .query(SeriesQuery::Id(series_id))
            .await?
            .into_iter()
            .next()
            .ok_or_else(|| DomainError::NotFound("series not found".into()))?;
        let mut chapters = self.chapters.query(ChapterQuery::Series(series_id)).await?;
        chapters.sort_by(|a, b| a.cmp_order(b));

        let prog_list = self
            .reading
            .query_progress(user.id, ComicProgressScope::Series(series_id))
            .await?;
        let prog_map: HashMap<ChapterId, ChapterProgress> =
            prog_list.into_iter().map(|p| (p.chapter_id, p)).collect();

        let entries = chapters
            .into_iter()
            .filter(|c| !c.is_missing)
            .map(|ch| {
                let (last_page, last_at) = prog_map
                    .get(&ch.id)
                    .map(|p| (Some(p.current_page), Some(p.updated_at)))
                    .unwrap_or((None, None));
                to_opds_chapter(
                    opds_root,
                    &series.title,
                    &series.writers,
                    ch,
                    last_page,
                    last_at,
                )
            })
            .collect();

        Ok(OpdsFeed {
            id: format!("urn:xmedia:comic:series:{}", series.id),
            title: series.title,
            self_path: format!("{ver_prefix}/series/{}", series.id),
            updated_at: series.updated_at,
            payload: OpdsFeedPayload::Acquisition(entries),
        })
    }
}

fn rewrite_opds_cover(opds_root: &str, rest_cover_url: Option<String>) -> Option<String> {
    rest_cover_url.map(|u| {
        let key = u.rsplit('/').next().unwrap_or(&u);
        format!("{opds_root}/covers/{key}")
    })
}

fn to_opds_chapter(
    opds_root: &str,
    series_title: &str,
    series_writers: &[String],
    ch: Chapter,
    last_read_page: Option<u32>,
    last_read_at: Option<Timestamp>,
) -> OpdsChapterEntry {
    let source_kind = ChapterSourceKind::from(&ch.source);
    let archive_mime = ch
        .source
        .resolved_local()
        .and_then(|(k, p)| (k == ChapterSourceKind::ArchiveFile).then_some(p))
        .and_then(|p| xmedia_formats::comic::ComicFormatExt::from_path(std::path::Path::new(p)))
        .filter(|fmt| fmt.is_zip_based())
        .map_or("application/vnd.comicbook+zip", |fmt| fmt.mime_type());
    let authors = if ch.writers.is_empty() {
        series_writers.to_vec()
    } else {
        ch.writers.clone()
    };
    let cover_url = ch
        .cover_path
        .as_ref()
        .map(|k| format!("{opds_root}/covers/{k}"));
    let num_label = ch.number_label();
    let display_title = if ch.title.is_empty() {
        format!("#{num_label}")
    } else {
        format!("#{num_label} - {}", ch.title)
    };

    let effective_page_count = if source_kind == ChapterSourceKind::OnlineTracked {
        ch.page_count.max(1)
    } else {
        ch.page_count
    };

    OpdsChapterEntry {
        id: ch.id,
        series_title: series_title.to_owned(),
        title: display_title,
        summary: ch.summary,
        authors,
        cover_url,
        page_count: effective_page_count,
        last_read_page,
        last_read_at,
        can_download_archive: matches!(
            source_kind,
            ChapterSourceKind::ArchiveFile | ChapterSourceKind::OnlineTracked
        ),
        archive_mime,
        updated_at: ch.updated_at,
    }
}

fn url_encode_segment(input: &str) -> String {
    let mut encoded = String::with_capacity(input.len());
    for byte in input.as_bytes() {
        match *byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                encoded.push(*byte as char);
            }
            other => {
                encoded.push_str(&format!("%{other:02X}"));
            }
        }
    }
    encoded
}

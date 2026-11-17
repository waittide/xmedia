use std::collections::HashMap;
use std::sync::Arc;

use jiff::Timestamp;
use xmedia_core::domain::error::{DomainError, DomainResult};
use xmedia_core::domain::library::{LibraryId, MediaKind};
use xmedia_core::domain::repository::LibraryRepository;
use xmedia_core::domain::user::User;

use crate::application::catalog::ComicCatalogService;
use crate::application::dto::SeriesQueryFilter;
use crate::domain::chapter::{Chapter, ChapterSourceKind};
use crate::domain::id::{ChapterId, ComicShelfId, SeriesId};
use crate::domain::reading::{ChapterProgress, format_chapter_number};
use crate::domain::repository::{ChapterRepository, ComicReadingRepository, SeriesRepository};

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
}

impl ComicOpdsService {
    pub fn new(
        libraries: Arc<dyn LibraryRepository>,
        series: Arc<dyn SeriesRepository>,
        chapters: Arc<dyn ChapterRepository>,
        reading: Arc<dyn ComicReadingRepository>,
    ) -> Self {
        Self {
            libraries,
            series,
            chapters,
            reading,
        }
    }

    pub fn root_feed(&self, ver_prefix: &str) -> OpdsFeed {
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

    pub async fn continue_reading_feed(
        &self,
        user: &User,
        catalog: &ComicCatalogService,
        opds_root: &str,
        ver_prefix: &str,
    ) -> DomainResult<OpdsFeed> {
        let overview = catalog.home_overview(user).await?;
        let mut entries = Vec::with_capacity(overview.continue_reading.len());

        for item in overview.continue_reading {
            if let Some(ch) = self.chapters.find_by_id(item.pointer.chapter_id).await? {
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

    pub async fn latest_series_feed(
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

    pub async fn all_series_feed(
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

    pub async fn libraries_feed(&self, ver_prefix: &str) -> DomainResult<OpdsFeed> {
        let libs = self.libraries.list_by_media(MediaKind::Comic).await?;
        let mut items = Vec::with_capacity(libs.len());
        for l in libs {
            let count = self.series.count_by_library(l.id).await?;
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

    pub async fn library_series_feed(
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

    pub async fn tags_feed(
        &self,
        catalog: &ComicCatalogService,
        ver_prefix: &str,
    ) -> DomainResult<OpdsFeed> {
        let tax = catalog.taxonomy_overview().await?;
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

    pub async fn tag_series_feed(
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

    pub async fn shelves_feed(&self, user: &User, ver_prefix: &str) -> DomainResult<OpdsFeed> {
        let now = Timestamp::now();
        let favs = self.reading.list_favorites(user.id).await?;
        let mut items = vec![OpdsNavItem {
            id: "urn:xmedia:comic:favorites".into(),
            title: "Favorites / 我的收藏".into(),
            summary: format!("{} favorite series", favs.len()),
            subsection_path: format!("{ver_prefix}/favorites"),
            cover_url: None,
            updated_at: now,
        }];

        let shelves = self.reading.list_shelves(user.id).await?;
        for sh in shelves {
            items.push(OpdsNavItem {
                id: format!("urn:xmedia:comic:shelf:{}", sh.id),
                title: sh.name,
                summary: format!("{} series", sh.series_ids.len()),
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

    pub async fn favorites_series_feed(
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

    pub async fn shelf_series_feed(
        &self,
        user: &User,
        catalog: &ComicCatalogService,
        shelf_id: ComicShelfId,
        opds_root: &str,
        ver_prefix: &str,
    ) -> DomainResult<OpdsFeed> {
        let shelf = self
            .reading
            .find_shelf(shelf_id)
            .await?
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

    pub async fn series_chapters_feed(
        &self,
        user: &User,
        series_id: SeriesId,
        opds_root: &str,
        ver_prefix: &str,
    ) -> DomainResult<OpdsFeed> {
        let series = self
            .series
            .find_by_id(series_id)
            .await?
            .ok_or_else(|| DomainError::NotFound("series not found".into()))?;
        let mut chapters = self.chapters.list_by_series(series_id).await?;
        chapters.sort_by(|a, b| a.cmp_order(b));

        let prog_list = self
            .reading
            .list_series_progress(user.id, series_id)
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
    let archive_mime = if ch.source_path.to_lowercase().ends_with(".epub") {
        "application/epub+zip"
    } else {
        "application/vnd.comicbook+zip"
    };
    let authors = if ch.writers.is_empty() {
        series_writers.to_vec()
    } else {
        ch.writers.clone()
    };
    let cover_url = ch
        .cover_path
        .as_ref()
        .map(|k| format!("{opds_root}/covers/{k}"));
    let num_label = format_chapter_number(ch.number);
    let display_title = if ch.title.is_empty() {
        format!("#{num_label}")
    } else {
        format!("#{num_label} - {}", ch.title)
    };

    OpdsChapterEntry {
        id: ch.id,
        series_title: series_title.to_owned(),
        title: display_title,
        summary: ch.summary,
        authors,
        cover_url,
        page_count: ch.page_count,
        last_read_page,
        last_read_at,
        can_download_archive: ch.source_kind == ChapterSourceKind::ArchiveFile,
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

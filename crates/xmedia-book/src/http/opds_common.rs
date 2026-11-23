use serde::Deserialize;
use uuid::Uuid;
use xmedia_core::domain::library::{LibraryId, MediaKind};
use xmedia_core::domain::user::User;
use xmedia_core::http::ApiResult;

use crate::application::dto::{BookSummaryDto, CatalogSearchFilter};
use crate::domain::book::{BookSortField, PagedResult, SortOrder};
use crate::domain::reading::ShelfId;
use crate::http::state::BookHttpState;

#[derive(Default, Deserialize)]
pub(crate) struct OpdsQueryParams {
    pub page: Option<u32>,
    pub page_size: Option<u32>,
    pub q: Option<String>,
    pub query: Option<String>,
    pub name: Option<String>,
}

pub(crate) struct OpdsAcquisitionData {
    pub feed_id: String,
    pub title: String,
    pub rel_path: String,
    pub extra_query: Option<String>,
    pub page: PagedResult<BookSummaryDto>,
}

pub(crate) struct OpdsNavItem {
    pub id: String,
    pub title: String,
    pub v2_title: String,
    pub rel_path: String,
    pub content: String,
    pub is_acquisition: bool,
}

pub(crate) fn encode_query_pair(key: &str, value: &str) -> String {
    form_urlencoded::Serializer::new(String::new())
        .append_pair(key, value)
        .finish()
}

pub(crate) fn build_page_href(
    base_href: &str,
    extra_query: Option<&str>,
    page: u32,
    page_size: u32,
) -> String {
    match extra_query {
        Some(extra) if !extra.is_empty() => {
            format!("{base_href}?{extra}&page={page}&page_size={page_size}")
        }
        _ => format!("{base_href}?page={page}&page_size={page_size}"),
    }
}

pub(crate) fn root_nav_items() -> Vec<OpdsNavItem> {
    vec![
        OpdsNavItem {
            id: "urn:xmedia:book:opds:v1:new".into(),
            title: "Recently Added".into(),
            v2_title: "Recently Added".into(),
            rel_path: "new".into(),
            content: "Latest books added to the library".into(),
            is_acquisition: true,
        },
        OpdsNavItem {
            id: "urn:xmedia:book:opds:v1:all".into(),
            title: "All Books (A-Z)".into(),
            v2_title: "All Books (A-Z)".into(),
            rel_path: "all".into(),
            content: "Browse every book sorted by title".into(),
            is_acquisition: true,
        },
        OpdsNavItem {
            id: "urn:xmedia:book:opds:v1:recent".into(),
            title: "Continue Reading".into(),
            v2_title: "Continue Reading".into(),
            rel_path: "recent".into(),
            content: "Books with recent reading activity".into(),
            is_acquisition: true,
        },
        OpdsNavItem {
            id: "urn:xmedia:book:opds:v1:favorites".into(),
            title: "My Favorites".into(),
            v2_title: "My Favorites".into(),
            rel_path: "favorites".into(),
            content: "Starred favorite books".into(),
            is_acquisition: true,
        },
        OpdsNavItem {
            id: "urn:xmedia:book:opds:v1:libraries".into(),
            title: "By Library".into(),
            v2_title: "Libraries".into(),
            rel_path: "libraries".into(),
            content: "Browse books by library".into(),
            is_acquisition: false,
        },
        OpdsNavItem {
            id: "urn:xmedia:book:opds:v1:authors".into(),
            title: "By Author".into(),
            v2_title: "Authors".into(),
            rel_path: "authors".into(),
            content: "Browse books grouped by author".into(),
            is_acquisition: false,
        },
        OpdsNavItem {
            id: "urn:xmedia:book:opds:v1:tags".into(),
            title: "By Tag".into(),
            v2_title: "Tags".into(),
            rel_path: "tags".into(),
            content: "Browse books grouped by subject/tag".into(),
            is_acquisition: false,
        },
        OpdsNavItem {
            id: "urn:xmedia:book:opds:v1:shelves".into(),
            title: "Personal Shelves".into(),
            v2_title: "Personal Shelves".into(),
            rel_path: "shelves".into(),
            content: "Browse custom bookshelves".into(),
            is_acquisition: false,
        },
    ]
}

pub(crate) async fn query_feed_new(
    state: &BookHttpState,
    actor: &User,
    params: &OpdsQueryParams,
) -> ApiResult<OpdsAcquisitionData> {
    let page = state
        .services
        .catalog
        .search_books_for_opds(
            Some(actor),
            CatalogSearchFilter {
                sort_by: Some(BookSortField::CreatedAt),
                sort_order: Some(SortOrder::Desc),
                page: params.page,
                page_size: params.page_size,
                ..Default::default()
            },
        )
        .await?;
    Ok(OpdsAcquisitionData {
        feed_id: "urn:xmedia:book:opds:v1:new".into(),
        title: "Recently Added".into(),
        rel_path: "new".into(),
        extra_query: None,
        page,
    })
}

pub(crate) async fn query_feed_all(
    state: &BookHttpState,
    actor: &User,
    params: &OpdsQueryParams,
) -> ApiResult<OpdsAcquisitionData> {
    let page = state
        .services
        .catalog
        .search_books_for_opds(
            Some(actor),
            CatalogSearchFilter {
                sort_by: Some(BookSortField::Title),
                sort_order: Some(SortOrder::Asc),
                page: params.page,
                page_size: params.page_size,
                ..Default::default()
            },
        )
        .await?;
    Ok(OpdsAcquisitionData {
        feed_id: "urn:xmedia:book:opds:v1:all".into(),
        title: "All Books".into(),
        rel_path: "all".into(),
        extra_query: None,
        page,
    })
}

pub(crate) async fn query_feed_recent(
    state: &BookHttpState,
    actor: &User,
) -> ApiResult<OpdsAcquisitionData> {
    let items = state
        .services
        .reading
        .list_recent_books(actor, 50, &state.services.catalog)
        .await?;
    let total = items.len() as u64;
    Ok(OpdsAcquisitionData {
        feed_id: "urn:xmedia:book:opds:v1:recent".into(),
        title: "Continue Reading".into(),
        rel_path: "recent".into(),
        extra_query: None,
        page: PagedResult::new(items, total, 1, 50),
    })
}

pub(crate) async fn query_feed_favorites(
    state: &BookHttpState,
    actor: &User,
    params: &OpdsQueryParams,
) -> ApiResult<OpdsAcquisitionData> {
    let page = state
        .services
        .catalog
        .search_books_for_opds(
            Some(actor),
            CatalogSearchFilter {
                favorites_only: true,
                page: params.page,
                page_size: params.page_size,
                ..Default::default()
            },
        )
        .await?;
    Ok(OpdsAcquisitionData {
        feed_id: "urn:xmedia:book:opds:v1:favorites".into(),
        title: "My Favorites".into(),
        rel_path: "favorites".into(),
        extra_query: None,
        page,
    })
}

pub(crate) async fn query_feed_search(
    state: &BookHttpState,
    actor: &User,
    params: OpdsQueryParams,
    query_param_key: &str,
) -> ApiResult<OpdsAcquisitionData> {
    let kw = if query_param_key == "query" {
        params.query.or(params.q).unwrap_or_default()
    } else {
        params.q.or(params.query).unwrap_or_default()
    };
    let extra = encode_query_pair(query_param_key, &kw);
    let page = state
        .services
        .catalog
        .search_books_for_opds(
            Some(actor),
            CatalogSearchFilter {
                keyword: Some(kw.clone()),
                page: params.page,
                page_size: params.page_size,
                ..Default::default()
            },
        )
        .await?;
    Ok(OpdsAcquisitionData {
        feed_id: "urn:xmedia:book:opds:v1:search".into(),
        title: format!("Search: {kw}"),
        rel_path: "search".into(),
        extra_query: Some(extra),
        page,
    })
}

pub(crate) async fn query_nav_libraries(
    state: &BookHttpState,
    actor: &User,
) -> ApiResult<Vec<OpdsNavItem>> {
    let libs = state
        .libraries
        .list_libraries(Some(MediaKind::Book))
        .await?;
    let counts = state
        .services
        .catalog
        .count_bound_books_by_library(actor)
        .await?;
    Ok(libs
        .into_iter()
        .filter_map(|l| {
            let count = counts.get(&l.id).copied().unwrap_or(0);
            if count == 0 {
                return None;
            }
            Some(OpdsNavItem {
                id: format!("urn:xmedia:book:library:{}", l.id),
                v2_title: format!("{} ({count})", l.name),
                title: l.name,
                rel_path: format!("libraries/{}", l.id),
                content: format!("{count} books"),
                is_acquisition: true,
            })
        })
        .collect())
}

pub(crate) async fn query_feed_by_library(
    state: &BookHttpState,
    actor: &User,
    id: Uuid,
    params: &OpdsQueryParams,
) -> ApiResult<OpdsAcquisitionData> {
    let lib = state.libraries.get_library(LibraryId(id)).await?;
    let page = state
        .services
        .catalog
        .search_books_for_opds(
            Some(actor),
            CatalogSearchFilter {
                library_id: Some(LibraryId(id)),
                page: params.page,
                page_size: params.page_size,
                ..Default::default()
            },
        )
        .await?;
    Ok(OpdsAcquisitionData {
        feed_id: format!("urn:xmedia:book:library:{id}"),
        title: lib.name,
        rel_path: format!("libraries/{id}"),
        extra_query: None,
        page,
    })
}

pub(crate) async fn query_nav_authors(
    state: &BookHttpState,
    actor: &User,
) -> ApiResult<Vec<OpdsNavItem>> {
    let authors = state.services.catalog.list_authors(actor).await?;
    Ok(authors
        .into_iter()
        .map(|a| {
            let query = encode_query_pair("name", &a.name);
            OpdsNavItem {
                id: format!("urn:xmedia:book:author:{}", a.name),
                v2_title: format!("{} ({})", a.name, a.count),
                title: a.name,
                rel_path: format!("authors/books?{query}"),
                content: format!("{} books", a.count),
                is_acquisition: true,
            }
        })
        .collect())
}

pub(crate) async fn query_feed_by_author(
    state: &BookHttpState,
    actor: &User,
    params: OpdsQueryParams,
) -> ApiResult<OpdsAcquisitionData> {
    let author = params.name.unwrap_or_default();
    let extra = encode_query_pair("name", &author);
    let page = state
        .services
        .catalog
        .search_books_for_opds(
            Some(actor),
            CatalogSearchFilter {
                author: Some(author.clone()),
                page: params.page,
                page_size: params.page_size,
                ..Default::default()
            },
        )
        .await?;
    Ok(OpdsAcquisitionData {
        feed_id: format!("urn:xmedia:book:author:{author}"),
        title: format!("Author: {author}"),
        rel_path: "authors/books".into(),
        extra_query: Some(extra),
        page,
    })
}

pub(crate) async fn query_nav_tags(
    state: &BookHttpState,
    actor: &User,
) -> ApiResult<Vec<OpdsNavItem>> {
    let tags = state.services.catalog.list_tags(actor).await?;
    Ok(tags
        .into_iter()
        .map(|t| {
            let query = encode_query_pair("name", &t.name);
            OpdsNavItem {
                id: format!("urn:xmedia:book:tag:{}", t.name),
                v2_title: format!("{} ({})", t.name, t.count),
                title: t.name,
                rel_path: format!("tags/books?{query}"),
                content: format!("{} books", t.count),
                is_acquisition: true,
            }
        })
        .collect())
}

pub(crate) async fn query_feed_by_tag(
    state: &BookHttpState,
    actor: &User,
    params: OpdsQueryParams,
) -> ApiResult<OpdsAcquisitionData> {
    let tag = params.name.unwrap_or_default();
    let extra = encode_query_pair("name", &tag);
    let page = state
        .services
        .catalog
        .search_books_for_opds(
            Some(actor),
            CatalogSearchFilter {
                tag: Some(tag.clone()),
                page: params.page,
                page_size: params.page_size,
                ..Default::default()
            },
        )
        .await?;
    Ok(OpdsAcquisitionData {
        feed_id: format!("urn:xmedia:book:tag:{tag}"),
        title: format!("Tag: {tag}"),
        rel_path: "tags/books".into(),
        extra_query: Some(extra),
        page,
    })
}

pub(crate) async fn query_nav_shelves(
    state: &BookHttpState,
    actor: &User,
) -> ApiResult<Vec<OpdsNavItem>> {
    let shelves = state.services.reading.list_shelves(actor).await?;
    Ok(shelves
        .into_iter()
        .map(|s| OpdsNavItem {
            id: format!("urn:xmedia:book:shelf:{}", s.id),
            v2_title: format!("{} ({})", s.name, s.book_count),
            title: s.name,
            rel_path: format!("shelves/{}", s.id),
            content: format!("{} books", s.book_count),
            is_acquisition: true,
        })
        .collect())
}

pub(crate) async fn query_feed_by_shelf(
    state: &BookHttpState,
    actor: &User,
    id: Uuid,
    params: &OpdsQueryParams,
) -> ApiResult<OpdsAcquisitionData> {
    let page = state
        .services
        .catalog
        .search_books_for_opds(
            Some(actor),
            CatalogSearchFilter {
                shelf_id: Some(ShelfId(id)),
                page: params.page,
                page_size: params.page_size,
                ..Default::default()
            },
        )
        .await?;
    Ok(OpdsAcquisitionData {
        feed_id: format!("urn:xmedia:book:shelf:{id}"),
        title: "Shelf".into(),
        rel_path: format!("shelves/{id}"),
        extra_query: None,
        page,
    })
}

use axum::Router;
use axum::body::Body;
use axum::extract::{Path, Query, State};
use axum::http::header::CONTENT_TYPE;
use axum::http::{HeaderValue, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use serde::{Deserialize, Serialize};
use uuid::Uuid;
use xmedia_core::domain::library::{LibraryId, MediaKind};
use xmedia_core::http::ApiResult;

use crate::application::dto::{BookSummaryDto, CatalogSearchFilter};
use crate::domain::book::{BookSortField, PagedResult, SortOrder};
use crate::domain::reading::ShelfId;
use crate::http::extractors::BookOpdsAuth;
use crate::http::state::BookHttpState;

const OPDS2_MIME: &str = "application/opds+json; charset=utf-8";

pub fn router() -> Router<BookHttpState> {
    Router::new()
        .route("/v2", get(root_catalog))
        .route("/v2/catalog", get(root_catalog))
        .route("/v2/new", get(feed_new))
        .route("/v2/all", get(feed_all))
        .route("/v2/recent", get(feed_recent))
        .route("/v2/favorites", get(feed_favorites))
        .route("/v2/search", get(feed_search))
        .route("/v2/libraries", get(nav_libraries))
        .route("/v2/libraries/{id}", get(feed_by_library))
        .route("/v2/authors", get(nav_authors))
        .route("/v2/authors/books", get(feed_by_author))
        .route("/v2/tags", get(nav_tags))
        .route("/v2/tags/books", get(feed_by_tag))
        .route("/v2/shelves", get(nav_shelves))
        .route("/v2/shelves/{id}", get(feed_by_shelf))
}

#[derive(Serialize)]
struct Opds2Feed {
    metadata: Opds2FeedMeta,
    links: Vec<Opds2Link>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    navigation: Vec<Opds2Link>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    publications: Vec<Opds2Publication>,
}

#[derive(Serialize)]
struct Opds2FeedMeta {
    title: String,
    #[serde(rename = "numberOfItems", skip_serializing_if = "Option::is_none")]
    number_of_items: Option<u64>,
    #[serde(rename = "itemsPerPage", skip_serializing_if = "Option::is_none")]
    items_per_page: Option<u32>,
    #[serde(rename = "currentPage", skip_serializing_if = "Option::is_none")]
    current_page: Option<u32>,
}

#[derive(Serialize)]
struct Opds2Link {
    #[serde(skip_serializing_if = "Option::is_none")]
    rel: Option<&'static str>,
    href: String,
    #[serde(rename = "type")]
    media_type: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    title: Option<String>,
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    templated: bool,
}

#[derive(Serialize)]
struct Opds2Publication {
    metadata: Opds2PubMeta,
    links: Vec<Opds2Link>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    images: Vec<Opds2Link>,
}

#[derive(Serialize)]
struct Opds2PubMeta {
    #[serde(rename = "@type")]
    schema_type: &'static str,
    identifier: String,
    title: String,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    author: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    description: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    language: Option<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    subject: Vec<String>,
    modified: String,
}

#[derive(Default, Deserialize)]
struct Opds2Params {
    page: Option<u32>,
    page_size: Option<u32>,
    query: Option<String>,
    q: Option<String>,
    name: Option<String>,
}

fn opds2_response(feed: &Opds2Feed) -> Response {
    let json = serde_json::to_vec(feed).unwrap_or_default();
    let mut res = (StatusCode::OK, Body::from(json)).into_response();
    res.headers_mut()
        .insert(CONTENT_TYPE, HeaderValue::from_static(OPDS2_MIME));
    res
}

fn url_encode_param(input: &str) -> String {
    input
        .bytes()
        .map(|b| {
            if b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.' | b'~') {
                (b as char).to_string()
            } else {
                format!("%{b:02X}")
            }
        })
        .collect()
}

fn standard_feed_links(prefix: &str, self_href: String) -> Vec<Opds2Link> {
    vec![
        Opds2Link {
            rel: Some("self"),
            href: self_href,
            media_type: "application/opds+json".into(),
            title: None,
            templated: false,
        },
        Opds2Link {
            rel: Some("start"),
            href: format!("{prefix}/v2"),
            media_type: "application/opds+json".into(),
            title: Some("xmedia Books OPDS 2.0 Home".into()),
            templated: false,
        },
        Opds2Link {
            rel: Some("search"),
            href: format!("{prefix}/v2/search{{?query}}"),
            media_type: "application/opds+json".into(),
            title: Some("Search Books".into()),
            templated: true,
        },
    ]
}

fn to_publication(prefix: &str, book: &BookSummaryDto) -> Opds2Publication {
    let images = if book.has_cover {
        vec![Opds2Link {
            rel: None,
            href: format!("{prefix}/books/{}/cover", book.id),
            media_type: book
                .cover_mime
                .clone()
                .unwrap_or_else(|| "image/jpeg".into()),
            title: None,
            templated: false,
        }]
    } else {
        Vec::new()
    };

    Opds2Publication {
        metadata: Opds2PubMeta {
            schema_type: "http://schema.org/Book",
            identifier: format!("urn:uuid:{}", book.id),
            title: book.metadata.title.clone(),
            author: book.metadata.authors.clone(),
            description: book.metadata.description.clone(),
            language: book.metadata.language.clone(),
            subject: book.metadata.tags.clone(),
            modified: book.updated_at.to_string(),
        },
        links: vec![Opds2Link {
            rel: Some("http://opds-spec.org/acquisition"),
            href: format!("{prefix}/books/{}/download", book.id),
            media_type: book.format.mime_type().into(),
            title: None,
            templated: false,
        }],
        images,
    }
}

fn build_publications_feed(
    prefix: &str,
    title: &str,
    base_href: &str,
    extra_query: Option<&str>,
    page: PagedResult<BookSummaryDto>,
) -> Opds2Feed {
    let page_url = |p: u32| -> String {
        match extra_query {
            Some(extra) if !extra.is_empty() => {
                format!("{base_href}?{extra}&page={p}&page_size={}", page.page_size)
            }
            _ => format!("{base_href}?page={p}&page_size={}", page.page_size),
        }
    };

    let mut links = standard_feed_links(prefix, page_url(page.page));
    if page.has_prev() {
        links.push(Opds2Link {
            rel: Some("previous"),
            href: page_url(page.page - 1),
            media_type: "application/opds+json".into(),
            title: None,
            templated: false,
        });
    }
    if page.has_next() {
        links.push(Opds2Link {
            rel: Some("next"),
            href: page_url(page.page + 1),
            media_type: "application/opds+json".into(),
            title: None,
            templated: false,
        });
    }

    Opds2Feed {
        metadata: Opds2FeedMeta {
            title: title.to_string(),
            number_of_items: Some(page.total),
            items_per_page: Some(page.page_size),
            current_page: Some(page.page),
        },
        links,
        navigation: Vec::new(),
        publications: page
            .items
            .iter()
            .map(|b| to_publication(prefix, b))
            .collect(),
    }
}

fn nav_item(title: impl Into<String>, href: impl Into<String>) -> Opds2Link {
    Opds2Link {
        rel: Some("subsection"),
        href: href.into(),
        media_type: "application/opds+json".into(),
        title: Some(title.into()),
        templated: false,
    }
}

async fn root_catalog(
    State(state): State<BookHttpState>,
    BookOpdsAuth(_actor): BookOpdsAuth,
) -> Response {
    let p = state.opds_prefix.as_ref();
    let feed = Opds2Feed {
        metadata: Opds2FeedMeta {
            title: "xmedia Books OPDS 2.0 Catalog".into(),
            number_of_items: None,
            items_per_page: None,
            current_page: None,
        },
        links: standard_feed_links(p, format!("{p}/v2")),
        navigation: vec![
            nav_item("Recently Added", format!("{p}/v2/new")),
            nav_item("All Books (A-Z)", format!("{p}/v2/all")),
            nav_item("Continue Reading", format!("{p}/v2/recent")),
            nav_item("My Favorites", format!("{p}/v2/favorites")),
            nav_item("Libraries", format!("{p}/v2/libraries")),
            nav_item("Authors", format!("{p}/v2/authors")),
            nav_item("Tags", format!("{p}/v2/tags")),
            nav_item("Personal Shelves", format!("{p}/v2/shelves")),
        ],
        publications: Vec::new(),
    };
    opds2_response(&feed)
}

async fn feed_new(
    State(state): State<BookHttpState>,
    BookOpdsAuth(actor): BookOpdsAuth,
    Query(params): Query<Opds2Params>,
) -> ApiResult<Response> {
    let p = state.opds_prefix.as_ref();
    let page = state
        .services
        .catalog
        .search_books(
            Some(&actor),
            CatalogSearchFilter {
                sort_by: Some(BookSortField::CreatedAt),
                sort_order: Some(SortOrder::Desc),
                page: params.page,
                page_size: params.page_size,
                ..Default::default()
            },
        )
        .await?;
    let base = format!("{p}/v2/new");
    Ok(opds2_response(&build_publications_feed(
        p,
        "Recently Added",
        &base,
        None,
        page,
    )))
}

async fn feed_all(
    State(state): State<BookHttpState>,
    BookOpdsAuth(actor): BookOpdsAuth,
    Query(params): Query<Opds2Params>,
) -> ApiResult<Response> {
    let p = state.opds_prefix.as_ref();
    let page = state
        .services
        .catalog
        .search_books(
            Some(&actor),
            CatalogSearchFilter {
                sort_by: Some(BookSortField::Title),
                sort_order: Some(SortOrder::Asc),
                page: params.page,
                page_size: params.page_size,
                ..Default::default()
            },
        )
        .await?;
    let base = format!("{p}/v2/all");
    Ok(opds2_response(&build_publications_feed(
        p,
        "All Books",
        &base,
        None,
        page,
    )))
}

async fn feed_recent(
    State(state): State<BookHttpState>,
    BookOpdsAuth(actor): BookOpdsAuth,
) -> ApiResult<Response> {
    let p = state.opds_prefix.as_ref();
    let items = state
        .services
        .reading
        .list_recent_books(&actor, 50, &state.services.catalog)
        .await?;
    let total = items.len() as u64;
    let page = PagedResult {
        items,
        total,
        page: 1,
        page_size: 50,
    };
    let base = format!("{p}/v2/recent");
    Ok(opds2_response(&build_publications_feed(
        p,
        "Continue Reading",
        &base,
        None,
        page,
    )))
}

async fn feed_favorites(
    State(state): State<BookHttpState>,
    BookOpdsAuth(actor): BookOpdsAuth,
    Query(params): Query<Opds2Params>,
) -> ApiResult<Response> {
    let p = state.opds_prefix.as_ref();
    let page = state
        .services
        .catalog
        .search_books(
            Some(&actor),
            CatalogSearchFilter {
                favorites_only: true,
                page: params.page,
                page_size: params.page_size,
                ..Default::default()
            },
        )
        .await?;
    let base = format!("{p}/v2/favorites");
    Ok(opds2_response(&build_publications_feed(
        p,
        "My Favorites",
        &base,
        None,
        page,
    )))
}

async fn feed_search(
    State(state): State<BookHttpState>,
    BookOpdsAuth(actor): BookOpdsAuth,
    Query(params): Query<Opds2Params>,
) -> ApiResult<Response> {
    let p = state.opds_prefix.as_ref();
    let kw = params.query.or(params.q).unwrap_or_default();
    let extra = format!("query={}", url_encode_param(&kw));
    let page = state
        .services
        .catalog
        .search_books(
            Some(&actor),
            CatalogSearchFilter {
                keyword: Some(kw.clone()),
                page: params.page,
                page_size: params.page_size,
                ..Default::default()
            },
        )
        .await?;
    let base = format!("{p}/v2/search");
    Ok(opds2_response(&build_publications_feed(
        p,
        &format!("Search: {kw}"),
        &base,
        Some(&extra),
        page,
    )))
}

async fn nav_libraries(
    State(state): State<BookHttpState>,
    BookOpdsAuth(_actor): BookOpdsAuth,
) -> ApiResult<Response> {
    let p = state.opds_prefix.as_ref();
    let libs = state
        .libraries
        .list_libraries(Some(MediaKind::Book))
        .await?;
    let navigation = libs
        .into_iter()
        .map(|l| {
            nav_item(
                format!("{} ({})", l.name, l.item_count),
                format!("{p}/v2/libraries/{}", l.id),
            )
        })
        .collect();
    Ok(opds2_response(&Opds2Feed {
        metadata: Opds2FeedMeta {
            title: "Libraries".into(),
            number_of_items: None,
            items_per_page: None,
            current_page: None,
        },
        links: standard_feed_links(p, format!("{p}/v2/libraries")),
        navigation,
        publications: Vec::new(),
    }))
}

async fn feed_by_library(
    State(state): State<BookHttpState>,
    BookOpdsAuth(actor): BookOpdsAuth,
    Path(id): Path<Uuid>,
    Query(params): Query<Opds2Params>,
) -> ApiResult<Response> {
    let p = state.opds_prefix.as_ref();
    let lib = state.libraries.get_library(LibraryId(id)).await?;
    let page = state
        .services
        .catalog
        .search_books(
            Some(&actor),
            CatalogSearchFilter {
                library_id: Some(LibraryId(id)),
                page: params.page,
                page_size: params.page_size,
                ..Default::default()
            },
        )
        .await?;
    let base = format!("{p}/v2/libraries/{id}");
    Ok(opds2_response(&build_publications_feed(
        p, &lib.name, &base, None, page,
    )))
}

async fn nav_authors(
    State(state): State<BookHttpState>,
    BookOpdsAuth(_actor): BookOpdsAuth,
) -> ApiResult<Response> {
    let p = state.opds_prefix.as_ref();
    let authors = state.services.catalog.list_authors().await?;
    let navigation = authors
        .into_iter()
        .map(|a| {
            nav_item(
                format!("{} ({})", a.name, a.count),
                format!("{p}/v2/authors/books?name={}", url_encode_param(&a.name)),
            )
        })
        .collect();
    Ok(opds2_response(&Opds2Feed {
        metadata: Opds2FeedMeta {
            title: "Authors".into(),
            number_of_items: None,
            items_per_page: None,
            current_page: None,
        },
        links: standard_feed_links(p, format!("{p}/v2/authors")),
        navigation,
        publications: Vec::new(),
    }))
}

async fn feed_by_author(
    State(state): State<BookHttpState>,
    BookOpdsAuth(actor): BookOpdsAuth,
    Query(params): Query<Opds2Params>,
) -> ApiResult<Response> {
    let p = state.opds_prefix.as_ref();
    let name = params.name.unwrap_or_default();
    let extra = format!("name={}", url_encode_param(&name));
    let page = state
        .services
        .catalog
        .search_books(
            Some(&actor),
            CatalogSearchFilter {
                author: Some(name.clone()),
                page: params.page,
                page_size: params.page_size,
                ..Default::default()
            },
        )
        .await?;
    let base = format!("{p}/v2/authors/books");
    Ok(opds2_response(&build_publications_feed(
        p,
        &format!("Author: {name}"),
        &base,
        Some(&extra),
        page,
    )))
}

async fn nav_tags(
    State(state): State<BookHttpState>,
    BookOpdsAuth(_actor): BookOpdsAuth,
) -> ApiResult<Response> {
    let p = state.opds_prefix.as_ref();
    let tags = state.services.catalog.list_tags().await?;
    let navigation = tags
        .into_iter()
        .map(|t| {
            nav_item(
                format!("{} ({})", t.name, t.count),
                format!("{p}/v2/tags/books?name={}", url_encode_param(&t.name)),
            )
        })
        .collect();
    Ok(opds2_response(&Opds2Feed {
        metadata: Opds2FeedMeta {
            title: "Tags".into(),
            number_of_items: None,
            items_per_page: None,
            current_page: None,
        },
        links: standard_feed_links(p, format!("{p}/v2/tags")),
        navigation,
        publications: Vec::new(),
    }))
}

async fn feed_by_tag(
    State(state): State<BookHttpState>,
    BookOpdsAuth(actor): BookOpdsAuth,
    Query(params): Query<Opds2Params>,
) -> ApiResult<Response> {
    let p = state.opds_prefix.as_ref();
    let name = params.name.unwrap_or_default();
    let extra = format!("name={}", url_encode_param(&name));
    let page = state
        .services
        .catalog
        .search_books(
            Some(&actor),
            CatalogSearchFilter {
                tag: Some(name.clone()),
                page: params.page,
                page_size: params.page_size,
                ..Default::default()
            },
        )
        .await?;
    let base = format!("{p}/v2/tags/books");
    Ok(opds2_response(&build_publications_feed(
        p,
        &format!("Tag: {name}"),
        &base,
        Some(&extra),
        page,
    )))
}

async fn nav_shelves(
    State(state): State<BookHttpState>,
    BookOpdsAuth(actor): BookOpdsAuth,
) -> ApiResult<Response> {
    let p = state.opds_prefix.as_ref();
    let shelves = state.services.reading.list_shelves(&actor).await?;
    let navigation = shelves
        .into_iter()
        .map(|s| {
            nav_item(
                format!("{} ({})", s.name, s.book_count),
                format!("{p}/v2/shelves/{}", s.id),
            )
        })
        .collect();
    Ok(opds2_response(&Opds2Feed {
        metadata: Opds2FeedMeta {
            title: "Personal Shelves".into(),
            number_of_items: None,
            items_per_page: None,
            current_page: None,
        },
        links: standard_feed_links(p, format!("{p}/v2/shelves")),
        navigation,
        publications: Vec::new(),
    }))
}

async fn feed_by_shelf(
    State(state): State<BookHttpState>,
    BookOpdsAuth(actor): BookOpdsAuth,
    Path(id): Path<Uuid>,
    Query(params): Query<Opds2Params>,
) -> ApiResult<Response> {
    let p = state.opds_prefix.as_ref();
    let page = state
        .services
        .catalog
        .search_books(
            Some(&actor),
            CatalogSearchFilter {
                shelf_id: Some(ShelfId(id)),
                page: params.page,
                page_size: params.page_size,
                ..Default::default()
            },
        )
        .await?;
    let base = format!("{p}/v2/shelves/{id}");
    Ok(opds2_response(&build_publications_feed(
        p, "Shelf", &base, None, page,
    )))
}

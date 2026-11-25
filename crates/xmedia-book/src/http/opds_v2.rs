use axum::Router;
use axum::body::Body;
use axum::extract::{Path, Query, State};
use axum::http::header::{CONTENT_TYPE, X_CONTENT_TYPE_OPTIONS};
use axum::http::{HeaderValue, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use serde::Serialize;
use uuid::Uuid;
use xmedia_core::http::{ApiResult, OpdsAuth};

use crate::application::dto::BookSummaryDto;
use crate::http::opds_common::{
    OpdsAcquisitionData, OpdsNavItem, OpdsQueryParams, build_page_href, query_feed_all,
    query_feed_by_author, query_feed_by_library, query_feed_by_shelf, query_feed_by_tag,
    query_feed_favorites, query_feed_new, query_feed_recent, query_feed_search, query_nav_authors,
    query_nav_libraries, query_nav_shelves, query_nav_tags, root_nav_items,
};
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
struct Opds2BelongsTo {
    series: Opds2SeriesInfo,
}

#[derive(Serialize)]
struct Opds2SeriesInfo {
    name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    position: Option<f64>,
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
    #[serde(rename = "belongsTo", skip_serializing_if = "Option::is_none")]
    belongs_to: Option<Opds2BelongsTo>,
    #[serde(skip_serializing_if = "Option::is_none")]
    series: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    series_index: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    rating: Option<f64>,
    modified: String,
}

fn opds2_response(feed: &Opds2Feed) -> Response {
    let Ok(json) = serde_json::to_vec(feed) else {
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            "failed to serialize OPDS2 feed",
        )
            .into_response();
    };
    let mut res = (StatusCode::OK, Body::from(json)).into_response();
    let headers = res.headers_mut();
    headers.insert(CONTENT_TYPE, HeaderValue::from_static(OPDS2_MIME));
    headers.insert(X_CONTENT_TYPE_OPTIONS, HeaderValue::from_static("nosniff"));
    res
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

    let belongs_to = book.metadata.series.as_ref().map(|name| Opds2BelongsTo {
        series: Opds2SeriesInfo {
            name: name.clone(),
            position: book.metadata.series_index,
        },
    });

    Opds2Publication {
        metadata: Opds2PubMeta {
            schema_type: "http://schema.org/Book",
            identifier: format!("urn:uuid:{}", book.id),
            title: book.metadata.title.clone(),
            author: book.metadata.authors.clone(),
            description: book.metadata.description.clone(),
            language: book.metadata.language.clone(),
            subject: book.metadata.tags.clone(),
            belongs_to,
            series: book.metadata.series.clone(),
            series_index: book.metadata.series_index,
            rating: book.metadata.rating,
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

fn render_navigation_feed(
    prefix: &str,
    title: &str,
    self_rel: Option<&str>,
    items: &[OpdsNavItem],
) -> Response {
    let self_href = match self_rel {
        Some(rel) => format!("{prefix}/v2/{rel}"),
        None => format!("{prefix}/v2"),
    };
    let navigation = items
        .iter()
        .map(|item| Opds2Link {
            rel: Some("subsection"),
            href: format!("{prefix}/v2/{}", item.rel_path),
            media_type: "application/opds+json".into(),
            title: Some(item.v2_title.clone()),
            templated: false,
        })
        .collect();
    opds2_response(&Opds2Feed {
        metadata: Opds2FeedMeta {
            title: title.to_string(),
            number_of_items: None,
            items_per_page: None,
            current_page: None,
        },
        links: standard_feed_links(prefix, self_href),
        navigation,
        publications: Vec::new(),
    })
}

fn render_publications_feed(prefix: &str, data: &OpdsAcquisitionData) -> Response {
    let base_href = format!("{prefix}/v2/{}", data.rel_path);
    let page = &data.page;
    let page_url =
        |p: u32| build_page_href(&base_href, data.extra_query.as_deref(), p, page.page_size);

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

    opds2_response(&Opds2Feed {
        metadata: Opds2FeedMeta {
            title: data.title.clone(),
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
    })
}

async fn root_catalog(State(state): State<BookHttpState>, OpdsAuth(_actor): OpdsAuth) -> Response {
    let items = root_nav_items();
    render_navigation_feed(
        &state.opds_prefix,
        "xmedia Books OPDS 2.0 Catalog",
        None,
        &items,
    )
}

async fn feed_new(
    State(state): State<BookHttpState>,
    OpdsAuth(actor): OpdsAuth,
    Query(params): Query<OpdsQueryParams>,
) -> ApiResult<Response> {
    let data = query_feed_new(&state, &actor, &params).await?;
    Ok(render_publications_feed(&state.opds_prefix, &data))
}

async fn feed_all(
    State(state): State<BookHttpState>,
    OpdsAuth(actor): OpdsAuth,
    Query(params): Query<OpdsQueryParams>,
) -> ApiResult<Response> {
    let data = query_feed_all(&state, &actor, &params).await?;
    Ok(render_publications_feed(&state.opds_prefix, &data))
}

async fn feed_recent(
    State(state): State<BookHttpState>,
    OpdsAuth(actor): OpdsAuth,
) -> ApiResult<Response> {
    let data = query_feed_recent(&state, &actor).await?;
    Ok(render_publications_feed(&state.opds_prefix, &data))
}

async fn feed_favorites(
    State(state): State<BookHttpState>,
    OpdsAuth(actor): OpdsAuth,
    Query(params): Query<OpdsQueryParams>,
) -> ApiResult<Response> {
    let data = query_feed_favorites(&state, &actor, &params).await?;
    Ok(render_publications_feed(&state.opds_prefix, &data))
}

async fn feed_search(
    State(state): State<BookHttpState>,
    OpdsAuth(actor): OpdsAuth,
    Query(params): Query<OpdsQueryParams>,
) -> ApiResult<Response> {
    let data = query_feed_search(&state, &actor, params, "query").await?;
    Ok(render_publications_feed(&state.opds_prefix, &data))
}

async fn nav_libraries(
    State(state): State<BookHttpState>,
    OpdsAuth(actor): OpdsAuth,
) -> ApiResult<Response> {
    let items = query_nav_libraries(&state, &actor).await?;
    Ok(render_navigation_feed(
        &state.opds_prefix,
        "Libraries",
        Some("libraries"),
        &items,
    ))
}

async fn feed_by_library(
    State(state): State<BookHttpState>,
    OpdsAuth(actor): OpdsAuth,
    Path(id): Path<Uuid>,
    Query(params): Query<OpdsQueryParams>,
) -> ApiResult<Response> {
    let data = query_feed_by_library(&state, &actor, id, &params).await?;
    Ok(render_publications_feed(&state.opds_prefix, &data))
}

async fn nav_authors(
    State(state): State<BookHttpState>,
    OpdsAuth(actor): OpdsAuth,
) -> ApiResult<Response> {
    let items = query_nav_authors(&state, &actor).await?;
    Ok(render_navigation_feed(
        &state.opds_prefix,
        "Authors",
        Some("authors"),
        &items,
    ))
}

async fn feed_by_author(
    State(state): State<BookHttpState>,
    OpdsAuth(actor): OpdsAuth,
    Query(params): Query<OpdsQueryParams>,
) -> ApiResult<Response> {
    let data = query_feed_by_author(&state, &actor, params).await?;
    Ok(render_publications_feed(&state.opds_prefix, &data))
}

async fn nav_tags(
    State(state): State<BookHttpState>,
    OpdsAuth(actor): OpdsAuth,
) -> ApiResult<Response> {
    let items = query_nav_tags(&state, &actor).await?;
    Ok(render_navigation_feed(
        &state.opds_prefix,
        "Tags",
        Some("tags"),
        &items,
    ))
}

async fn feed_by_tag(
    State(state): State<BookHttpState>,
    OpdsAuth(actor): OpdsAuth,
    Query(params): Query<OpdsQueryParams>,
) -> ApiResult<Response> {
    let data = query_feed_by_tag(&state, &actor, params).await?;
    Ok(render_publications_feed(&state.opds_prefix, &data))
}

async fn nav_shelves(
    State(state): State<BookHttpState>,
    OpdsAuth(actor): OpdsAuth,
) -> ApiResult<Response> {
    let items = query_nav_shelves(&state, &actor).await?;
    Ok(render_navigation_feed(
        &state.opds_prefix,
        "Personal Shelves",
        Some("shelves"),
        &items,
    ))
}

async fn feed_by_shelf(
    State(state): State<BookHttpState>,
    OpdsAuth(actor): OpdsAuth,
    Path(id): Path<Uuid>,
    Query(params): Query<OpdsQueryParams>,
) -> ApiResult<Response> {
    let data = query_feed_by_shelf(&state, &actor, id, &params).await?;
    Ok(render_publications_feed(&state.opds_prefix, &data))
}

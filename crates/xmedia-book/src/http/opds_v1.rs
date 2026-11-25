use std::fmt::Write as _;

use axum::Router;
use axum::body::Body;
use axum::extract::{Path, Query, State};
use axum::http::header::{
    CACHE_CONTROL, CONTENT_SECURITY_POLICY, CONTENT_TYPE, X_CONTENT_TYPE_OPTIONS,
};
use axum::http::{HeaderMap, HeaderValue, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use jiff::Timestamp;
use uuid::Uuid;
use xmedia_core::domain::xml::xml_escape;
use xmedia_core::http::{ApiResult, OpdsAuth};
use xmedia_formats::common::format_clean_float;

use crate::domain::book::BookId;
use crate::http::opds_common::{
    OpdsAcquisitionData, OpdsNavItem, OpdsQueryParams, build_page_href, query_feed_all,
    query_feed_by_author, query_feed_by_library, query_feed_by_shelf, query_feed_by_tag,
    query_feed_favorites, query_feed_new, query_feed_recent, query_feed_search, query_nav_authors,
    query_nav_libraries, query_nav_shelves, query_nav_tags, root_nav_items,
};
use crate::http::rest::serve_book_file_http;
use crate::http::state::BookHttpState;

const NAV_MIME: &str = "application/atom+xml;profile=opds-catalog;kind=navigation;charset=utf-8";
const ACQ_MIME: &str = "application/atom+xml;profile=opds-catalog;kind=acquisition;charset=utf-8";
const OPENSEARCH_MIME: &str = "application/opensearchdescription+xml;charset=utf-8";

pub fn router() -> Router<BookHttpState> {
    Router::new()
        .route("/v1.2", get(root_catalog))
        .route("/v1.2/catalog", get(root_catalog))
        .route("/v1.2/opensearch.xml", get(opensearch_descriptor))
        .route("/v1.2/new", get(feed_new))
        .route("/v1.2/all", get(feed_all))
        .route("/v1.2/recent", get(feed_recent))
        .route("/v1.2/favorites", get(feed_favorites))
        .route("/v1.2/search", get(feed_search))
        .route("/v1.2/libraries", get(nav_libraries))
        .route("/v1.2/libraries/{id}", get(feed_by_library))
        .route("/v1.2/authors", get(nav_authors))
        .route("/v1.2/authors/books", get(feed_by_author))
        .route("/v1.2/tags", get(nav_tags))
        .route("/v1.2/tags/books", get(feed_by_tag))
        .route("/v1.2/shelves", get(nav_shelves))
        .route("/v1.2/shelves/{id}", get(feed_by_shelf))
        .route("/books/{id}/cover", get(opds_cover))
        .route("/books/{id}/download", get(opds_download))
}

fn xml_response(mime: &'static str, body: String) -> Response {
    let mut res = (StatusCode::OK, Body::from(body)).into_response();
    let headers = res.headers_mut();
    headers.insert(CONTENT_TYPE, HeaderValue::from_static(mime));
    headers.insert(X_CONTENT_TYPE_OPTIONS, HeaderValue::from_static("nosniff"));
    res
}

fn render_navigation_feed(
    prefix: &str,
    feed_id: &str,
    title: &str,
    self_rel: Option<&str>,
    entries: &[OpdsNavItem],
) -> Response {
    let now = Timestamp::now().to_string();
    let start_href = format!("{prefix}/v1.2");
    let self_href = match self_rel {
        Some(rel) => format!("{prefix}/v1.2/{rel}"),
        None => start_href.clone(),
    };
    let search_href = format!("{prefix}/v1.2/opensearch.xml");
    let mut xml = String::with_capacity(2048);
    let _ = writeln!(xml, r#"<?xml version="1.0" encoding="UTF-8"?>"#);
    let _ = writeln!(
        xml,
        r#"<feed xmlns="http://www.w3.org/2005/Atom" xmlns:opds="http://opds-spec.org/2010/catalog">"#
    );
    let _ = writeln!(xml, "  <id>{}</id>", xml_escape(feed_id));
    let _ = writeln!(xml, "  <title>{}</title>", xml_escape(title));
    let _ = writeln!(xml, "  <updated>{now}</updated>");
    let _ = writeln!(xml, "  <author><name>xmedia</name></author>");
    let _ = writeln!(
        xml,
        r#"  <link rel="self" href="{}" type="{NAV_MIME}"/>"#,
        xml_escape(&self_href)
    );
    let _ = writeln!(
        xml,
        r#"  <link rel="start" href="{}" type="{NAV_MIME}"/>"#,
        xml_escape(&start_href)
    );
    let _ = writeln!(
        xml,
        r#"  <link rel="search" href="{}" type="{OPENSEARCH_MIME}" title="Search Books"/>"#,
        xml_escape(&search_href)
    );

    for entry in entries {
        let href = format!("{prefix}/v1.2/{}", entry.rel_path);
        let kind_mime = if entry.is_acquisition {
            ACQ_MIME
        } else {
            NAV_MIME
        };
        let _ = writeln!(xml, "  <entry>");
        let _ = writeln!(xml, "    <id>{}</id>", xml_escape(&entry.id));
        let _ = writeln!(xml, "    <title>{}</title>", xml_escape(&entry.title));
        let _ = writeln!(xml, "    <updated>{now}</updated>");
        let _ = writeln!(
            xml,
            r#"    <content type="text">{}</content>"#,
            xml_escape(&entry.content)
        );
        let _ = writeln!(
            xml,
            r#"    <link rel="subsection" href="{}" type="{kind_mime}"/>"#,
            xml_escape(&href),
        );
        let _ = writeln!(xml, "  </entry>");
    }

    let _ = writeln!(xml, "</feed>");
    xml_response(NAV_MIME, xml)
}

fn render_acquisition_feed(prefix: &str, data: &OpdsAcquisitionData) -> Response {
    let now = Timestamp::now().to_string();
    let start_href = format!("{prefix}/v1.2");
    let search_href = format!("{prefix}/v1.2/opensearch.xml");
    let base_href = format!("{prefix}/v1.2/{}", data.rel_path);
    let page = &data.page;
    let page_href =
        |p: u32| build_page_href(&base_href, data.extra_query.as_deref(), p, page.page_size);

    let mut xml = String::with_capacity(4096);
    let _ = writeln!(xml, r#"<?xml version="1.0" encoding="UTF-8"?>"#);
    let _ = writeln!(
        xml,
        r#"<feed xmlns="http://www.w3.org/2005/Atom" xmlns:dc="http://purl.org/dc/terms/" xmlns:opds="http://opds-spec.org/2010/catalog" xmlns:opensearch="http://a9.com/-/spec/opensearch/1.1/" xmlns:calibre="http://calibre.kovidgoyal.net/2009/metadata">"#
    );
    let _ = writeln!(xml, "  <id>{}</id>", xml_escape(&data.feed_id));
    let _ = writeln!(xml, "  <title>{}</title>", xml_escape(&data.title));
    let _ = writeln!(xml, "  <updated>{now}</updated>");
    let _ = writeln!(xml, "  <author><name>xmedia</name></author>");
    let _ = writeln!(
        xml,
        r#"  <link rel="self" href="{}" type="{ACQ_MIME}"/>"#,
        xml_escape(&page_href(page.page))
    );
    let _ = writeln!(
        xml,
        r#"  <link rel="start" href="{}" type="{NAV_MIME}"/>"#,
        xml_escape(&start_href)
    );
    let _ = writeln!(
        xml,
        r#"  <link rel="first" href="{}" type="{ACQ_MIME}"/>"#,
        xml_escape(&page_href(1))
    );
    if page.has_prev() {
        let _ = writeln!(
            xml,
            r#"  <link rel="previous" href="{}" type="{ACQ_MIME}"/>"#,
            xml_escape(&page_href(page.page - 1))
        );
    }
    if page.has_next() {
        let _ = writeln!(
            xml,
            r#"  <link rel="next" href="{}" type="{ACQ_MIME}"/>"#,
            xml_escape(&page_href(page.page + 1))
        );
    }
    let _ = writeln!(
        xml,
        r#"  <link rel="search" href="{}" type="{OPENSEARCH_MIME}" title="Search Books"/>"#,
        xml_escape(&search_href)
    );
    let _ = writeln!(
        xml,
        "  <opensearch:totalResults>{}</opensearch:totalResults>",
        page.total
    );
    let _ = writeln!(
        xml,
        "  <opensearch:itemsPerPage>{}</opensearch:itemsPerPage>",
        page.page_size
    );

    for book in &page.items {
        let _ = writeln!(xml, "  <entry>");
        let _ = writeln!(xml, "    <id>urn:uuid:{}</id>", book.id);
        let _ = writeln!(
            xml,
            "    <title>{}</title>",
            xml_escape(&book.metadata.title)
        );
        let _ = writeln!(xml, "    <updated>{}</updated>", book.updated_at);
        for author in &book.metadata.authors {
            let _ = writeln!(
                xml,
                "    <author><name>{}</name></author>",
                xml_escape(author)
            );
        }
        if let Some(lang) = &book.metadata.language {
            let _ = writeln!(xml, "    <dc:language>{}</dc:language>", xml_escape(lang));
        }
        if let Some(pub_name) = &book.metadata.publisher {
            let _ = writeln!(
                xml,
                "    <dc:publisher>{}</dc:publisher>",
                xml_escape(pub_name)
            );
        }
        if let Some(isbn) = &book.metadata.isbn {
            let _ = writeln!(
                xml,
                "    <dc:identifier>urn:isbn:{}</dc:identifier>",
                xml_escape(isbn)
            );
        }
        if let Some(series) = &book.metadata.series {
            let _ = writeln!(
                xml,
                "    <calibre:series>{}</calibre:series>",
                xml_escape(series)
            );
        }
        if let Some(series_index) = book.metadata.series_index {
            let formatted = format_clean_float(series_index);
            if !formatted.is_empty() {
                let _ = writeln!(
                    xml,
                    "    <calibre:series_index>{}</calibre:series_index>",
                    xml_escape(&formatted)
                );
            }
        }
        if let Some(rating) = book.metadata.rating {
            let formatted = format_clean_float(rating);
            if !formatted.is_empty() {
                let _ = writeln!(
                    xml,
                    "    <calibre:rating>{}</calibre:rating>",
                    xml_escape(&formatted)
                );
            }
        }
        for tag in &book.metadata.tags {
            let _ = writeln!(
                xml,
                r#"    <category term="{0}" label="{0}"/>"#,
                xml_escape(tag)
            );
        }
        if let Some(desc) = &book.metadata.description {
            let _ = writeln!(xml, "    <summary>{}</summary>", xml_escape(desc));
        }
        if book.has_cover {
            let cover_mime = xml_escape(book.cover_mime.as_deref().unwrap_or("image/jpeg"));
            let cover_href = format!("{prefix}/books/{}/cover", book.id);
            let _ = writeln!(
                xml,
                r#"    <link rel="http://opds-spec.org/image" href="{}" type="{cover_mime}"/>"#,
                xml_escape(&cover_href)
            );
            let _ = writeln!(
                xml,
                r#"    <link rel="http://opds-spec.org/image/thumbnail" href="{}" type="{cover_mime}"/>"#,
                xml_escape(&cover_href)
            );
        }
        let dl_href = format!("{prefix}/books/{}/download", book.id);
        let _ = writeln!(
            xml,
            r#"    <link rel="http://opds-spec.org/acquisition" href="{}" type="{}"/>"#,
            xml_escape(&dl_href),
            xml_escape(book.format.mime_type())
        );
        let _ = writeln!(xml, "  </entry>");
    }

    let _ = writeln!(xml, "</feed>");
    xml_response(ACQ_MIME, xml)
}

async fn root_catalog(State(state): State<BookHttpState>, OpdsAuth(_actor): OpdsAuth) -> Response {
    let entries = root_nav_items();
    render_navigation_feed(
        &state.opds_prefix,
        "urn:xmedia:book:opds:v1:root",
        "xmedia Books OPDS 1.2 Catalog",
        None,
        &entries,
    )
}

async fn opensearch_descriptor(
    State(state): State<BookHttpState>,
    OpdsAuth(_actor): OpdsAuth,
) -> Response {
    let p = xml_escape(state.opds_prefix.as_ref());
    let body = format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<OpenSearchDescription xmlns="http://a9.com/-/spec/opensearch/1.1/">
  <ShortName>xmedia Books</ShortName>
  <Description>Search books in xmedia library</Description>
  <InputEncoding>UTF-8</InputEncoding>
  <OutputEncoding>UTF-8</OutputEncoding>
  <Url type="{ACQ_MIME}" template="{p}/v1.2/search?q={{searchTerms}}"/>
</OpenSearchDescription>
"#
    );
    xml_response(OPENSEARCH_MIME, body)
}

async fn feed_new(
    State(state): State<BookHttpState>,
    OpdsAuth(actor): OpdsAuth,
    Query(params): Query<OpdsQueryParams>,
) -> ApiResult<Response> {
    let data = query_feed_new(&state, &actor, &params).await?;
    Ok(render_acquisition_feed(&state.opds_prefix, &data))
}

async fn feed_all(
    State(state): State<BookHttpState>,
    OpdsAuth(actor): OpdsAuth,
    Query(params): Query<OpdsQueryParams>,
) -> ApiResult<Response> {
    let data = query_feed_all(&state, &actor, &params).await?;
    Ok(render_acquisition_feed(&state.opds_prefix, &data))
}

async fn feed_recent(
    State(state): State<BookHttpState>,
    OpdsAuth(actor): OpdsAuth,
) -> ApiResult<Response> {
    let data = query_feed_recent(&state, &actor).await?;
    Ok(render_acquisition_feed(&state.opds_prefix, &data))
}

async fn feed_favorites(
    State(state): State<BookHttpState>,
    OpdsAuth(actor): OpdsAuth,
    Query(params): Query<OpdsQueryParams>,
) -> ApiResult<Response> {
    let data = query_feed_favorites(&state, &actor, &params).await?;
    Ok(render_acquisition_feed(&state.opds_prefix, &data))
}

async fn feed_search(
    State(state): State<BookHttpState>,
    OpdsAuth(actor): OpdsAuth,
    Query(params): Query<OpdsQueryParams>,
) -> ApiResult<Response> {
    let data = query_feed_search(&state, &actor, params, "q").await?;
    Ok(render_acquisition_feed(&state.opds_prefix, &data))
}

async fn nav_libraries(
    State(state): State<BookHttpState>,
    OpdsAuth(actor): OpdsAuth,
) -> ApiResult<Response> {
    let entries = query_nav_libraries(&state, &actor).await?;
    Ok(render_navigation_feed(
        &state.opds_prefix,
        "urn:xmedia:book:opds:v1:libraries",
        "Libraries",
        Some("libraries"),
        &entries,
    ))
}

async fn feed_by_library(
    State(state): State<BookHttpState>,
    OpdsAuth(actor): OpdsAuth,
    Path(id): Path<Uuid>,
    Query(params): Query<OpdsQueryParams>,
) -> ApiResult<Response> {
    let data = query_feed_by_library(&state, &actor, id, &params).await?;
    Ok(render_acquisition_feed(&state.opds_prefix, &data))
}

async fn nav_authors(
    State(state): State<BookHttpState>,
    OpdsAuth(actor): OpdsAuth,
) -> ApiResult<Response> {
    let entries = query_nav_authors(&state, &actor).await?;
    Ok(render_navigation_feed(
        &state.opds_prefix,
        "urn:xmedia:book:opds:v1:authors",
        "Authors",
        Some("authors"),
        &entries,
    ))
}

async fn feed_by_author(
    State(state): State<BookHttpState>,
    OpdsAuth(actor): OpdsAuth,
    Query(params): Query<OpdsQueryParams>,
) -> ApiResult<Response> {
    let data = query_feed_by_author(&state, &actor, params).await?;
    Ok(render_acquisition_feed(&state.opds_prefix, &data))
}

async fn nav_tags(
    State(state): State<BookHttpState>,
    OpdsAuth(actor): OpdsAuth,
) -> ApiResult<Response> {
    let entries = query_nav_tags(&state, &actor).await?;
    Ok(render_navigation_feed(
        &state.opds_prefix,
        "urn:xmedia:book:opds:v1:tags",
        "Tags",
        Some("tags"),
        &entries,
    ))
}

async fn feed_by_tag(
    State(state): State<BookHttpState>,
    OpdsAuth(actor): OpdsAuth,
    Query(params): Query<OpdsQueryParams>,
) -> ApiResult<Response> {
    let data = query_feed_by_tag(&state, &actor, params).await?;
    Ok(render_acquisition_feed(&state.opds_prefix, &data))
}

async fn nav_shelves(
    State(state): State<BookHttpState>,
    OpdsAuth(actor): OpdsAuth,
) -> ApiResult<Response> {
    let entries = query_nav_shelves(&state, &actor).await?;
    Ok(render_navigation_feed(
        &state.opds_prefix,
        "urn:xmedia:book:opds:v1:shelves",
        "Personal Shelves",
        Some("shelves"),
        &entries,
    ))
}

async fn feed_by_shelf(
    State(state): State<BookHttpState>,
    OpdsAuth(actor): OpdsAuth,
    Path(id): Path<Uuid>,
    Query(params): Query<OpdsQueryParams>,
) -> ApiResult<Response> {
    let data = query_feed_by_shelf(&state, &actor, id, &params).await?;
    Ok(render_acquisition_feed(&state.opds_prefix, &data))
}

async fn opds_cover(
    State(state): State<BookHttpState>,
    OpdsAuth(actor): OpdsAuth,
    Path(id): Path<Uuid>,
) -> ApiResult<Response> {
    let book_id = BookId(id);
    state
        .services
        .catalog
        .require_book_bound(&actor, book_id)
        .await?;
    let cover = state
        .services
        .catalog
        .get_cover(Some(&actor), book_id)
        .await?;
    let mut res = (StatusCode::OK, Body::from(cover.bytes)).into_response();
    if let Ok(ct) = HeaderValue::from_str(&cover.mime_type) {
        res.headers_mut().insert(CONTENT_TYPE, ct);
    }
    res.headers_mut()
        .insert(X_CONTENT_TYPE_OPTIONS, HeaderValue::from_static("nosniff"));
    res.headers_mut().insert(
        CONTENT_SECURITY_POLICY,
        HeaderValue::from_static("sandbox; default-src 'none'; img-src 'self' data:"),
    );
    res.headers_mut().insert(
        CACHE_CONTROL,
        HeaderValue::from_static("private, max-age=600"),
    );
    Ok(res)
}

async fn opds_download(
    State(state): State<BookHttpState>,
    OpdsAuth(actor): OpdsAuth,
    Path(id): Path<Uuid>,
    headers: HeaderMap,
) -> ApiResult<Response> {
    serve_book_file_http(&state, &actor, BookId(id), &headers, true).await
}

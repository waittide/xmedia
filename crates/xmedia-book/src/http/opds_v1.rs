use std::fmt::Write as _;

use axum::Router;
use axum::body::Body;
use axum::extract::{Path, Query, State};
use axum::http::header::{CACHE_CONTROL, CONTENT_TYPE};
use axum::http::{HeaderMap, HeaderValue, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use jiff::Timestamp;
use serde::Deserialize;
use uuid::Uuid;
use xmedia_core::domain::library::{LibraryId, MediaKind};
use xmedia_core::http::ApiResult;

use crate::application::dto::{BookSummaryDto, CatalogSearchFilter};
use crate::domain::book::{BookId, BookSortField, PagedResult, SortOrder};
use crate::domain::reading::ShelfId;
use crate::http::extractors::BookOpdsAuth;
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

fn xml_escape(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    for ch in input.chars() {
        match ch {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&apos;"),
            _ => out.push(ch),
        }
    }
    out
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

fn xml_response(mime: &'static str, body: String) -> Response {
    let mut res = (StatusCode::OK, Body::from(body)).into_response();
    res.headers_mut()
        .insert(CONTENT_TYPE, HeaderValue::from_static(mime));
    res
}

struct NavEntry {
    id: String,
    title: String,
    href: String,
    content: String,
    kind_mime: &'static str,
}

fn render_navigation_feed(
    prefix: &str,
    feed_id: &str,
    title: &str,
    self_href: &str,
    entries: &[NavEntry],
) -> String {
    let now = Timestamp::now().to_string();
    let start_href = format!("{prefix}/v1.2");
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
        xml_escape(self_href)
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
            r#"    <link rel="subsection" href="{}" type="{}"/>"#,
            xml_escape(&entry.href),
            entry.kind_mime
        );
        let _ = writeln!(xml, "  </entry>");
    }

    let _ = writeln!(xml, "</feed>");
    xml
}

fn render_acquisition_feed(
    prefix: &str,
    feed_id: &str,
    title: &str,
    base_href: &str,
    extra_query: Option<&str>,
    page: &PagedResult<BookSummaryDto>,
) -> String {
    let now = Timestamp::now().to_string();
    let start_href = format!("{prefix}/v1.2");
    let search_href = format!("{prefix}/v1.2/opensearch.xml");
    let build_page_href = |p: u32| -> String {
        match extra_query {
            Some(extra) if !extra.is_empty() => {
                format!("{base_href}?{extra}&page={p}&page_size={}", page.page_size)
            }
            _ => format!("{base_href}?page={p}&page_size={}", page.page_size),
        }
    };

    let mut xml = String::with_capacity(4096);
    let _ = writeln!(xml, r#"<?xml version="1.0" encoding="UTF-8"?>"#);
    let _ = writeln!(
        xml,
        r#"<feed xmlns="http://www.w3.org/2005/Atom" xmlns:dc="http://purl.org/dc/terms/" xmlns:opds="http://opds-spec.org/2010/catalog" xmlns:opensearch="http://a9.com/-/spec/opensearch/1.1/">"#
    );
    let _ = writeln!(xml, "  <id>{}</id>", xml_escape(feed_id));
    let _ = writeln!(xml, "  <title>{}</title>", xml_escape(title));
    let _ = writeln!(xml, "  <updated>{now}</updated>");
    let _ = writeln!(xml, "  <author><name>xmedia</name></author>");
    let _ = writeln!(
        xml,
        r#"  <link rel="self" href="{}" type="{ACQ_MIME}"/>"#,
        xml_escape(&build_page_href(page.page))
    );
    let _ = writeln!(
        xml,
        r#"  <link rel="start" href="{}" type="{NAV_MIME}"/>"#,
        xml_escape(&start_href)
    );
    let _ = writeln!(
        xml,
        r#"  <link rel="first" href="{}" type="{ACQ_MIME}"/>"#,
        xml_escape(&build_page_href(1))
    );
    if page.has_prev() {
        let _ = writeln!(
            xml,
            r#"  <link rel="previous" href="{}" type="{ACQ_MIME}"/>"#,
            xml_escape(&build_page_href(page.page - 1))
        );
    }
    if page.has_next() {
        let _ = writeln!(
            xml,
            r#"  <link rel="next" href="{}" type="{ACQ_MIME}"/>"#,
            xml_escape(&build_page_href(page.page + 1))
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
            let cover_mime = book.cover_mime.as_deref().unwrap_or("image/jpeg");
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
            book.format.mime_type()
        );
        let _ = writeln!(xml, "  </entry>");
    }

    let _ = writeln!(xml, "</feed>");
    xml
}

#[derive(Default, Deserialize)]
struct OpdsPageParams {
    page: Option<u32>,
    page_size: Option<u32>,
    q: Option<String>,
    query: Option<String>,
    name: Option<String>,
}

async fn root_catalog(
    State(state): State<BookHttpState>,
    BookOpdsAuth(_actor): BookOpdsAuth,
) -> Response {
    let p = state.opds_prefix.as_ref();
    let entries = vec![
        NavEntry {
            id: "urn:xmedia:book:opds:v1:new".into(),
            title: "Recently Added".into(),
            href: format!("{p}/v1.2/new"),
            content: "Latest books added to the library".into(),
            kind_mime: ACQ_MIME,
        },
        NavEntry {
            id: "urn:xmedia:book:opds:v1:all".into(),
            title: "All Books (A-Z)".into(),
            href: format!("{p}/v1.2/all"),
            content: "Browse every book sorted by title".into(),
            kind_mime: ACQ_MIME,
        },
        NavEntry {
            id: "urn:xmedia:book:opds:v1:recent".into(),
            title: "Continue Reading".into(),
            href: format!("{p}/v1.2/recent"),
            content: "Books with recent reading activity".into(),
            kind_mime: ACQ_MIME,
        },
        NavEntry {
            id: "urn:xmedia:book:opds:v1:favorites".into(),
            title: "My Favorites".into(),
            href: format!("{p}/v1.2/favorites"),
            content: "Starred favorite books".into(),
            kind_mime: ACQ_MIME,
        },
        NavEntry {
            id: "urn:xmedia:book:opds:v1:libraries".into(),
            title: "By Library".into(),
            href: format!("{p}/v1.2/libraries"),
            content: "Browse books by library".into(),
            kind_mime: NAV_MIME,
        },
        NavEntry {
            id: "urn:xmedia:book:opds:v1:authors".into(),
            title: "By Author".into(),
            href: format!("{p}/v1.2/authors"),
            content: "Browse books grouped by author".into(),
            kind_mime: NAV_MIME,
        },
        NavEntry {
            id: "urn:xmedia:book:opds:v1:tags".into(),
            title: "By Tag".into(),
            href: format!("{p}/v1.2/tags"),
            content: "Browse books grouped by subject/tag".into(),
            kind_mime: NAV_MIME,
        },
        NavEntry {
            id: "urn:xmedia:book:opds:v1:shelves".into(),
            title: "Personal Shelves".into(),
            href: format!("{p}/v1.2/shelves"),
            content: "Browse custom bookshelves".into(),
            kind_mime: NAV_MIME,
        },
    ];

    let self_href = format!("{p}/v1.2");
    xml_response(
        NAV_MIME,
        render_navigation_feed(
            p,
            "urn:xmedia:book:opds:v1:root",
            "xmedia Books OPDS 1.2 Catalog",
            &self_href,
            &entries,
        ),
    )
}

async fn opensearch_descriptor(
    State(state): State<BookHttpState>,
    BookOpdsAuth(_actor): BookOpdsAuth,
) -> Response {
    let p = state.opds_prefix.as_ref();
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
    BookOpdsAuth(actor): BookOpdsAuth,
    Query(params): Query<OpdsPageParams>,
) -> ApiResult<Response> {
    let p = state.opds_prefix.as_ref();
    let page = state
        .services
        .catalog
        .search_books_for_opds(
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
    let base = format!("{p}/v1.2/new");
    Ok(xml_response(
        ACQ_MIME,
        render_acquisition_feed(
            p,
            "urn:xmedia:book:opds:v1:new",
            "Recently Added",
            &base,
            None,
            &page,
        ),
    ))
}

async fn feed_all(
    State(state): State<BookHttpState>,
    BookOpdsAuth(actor): BookOpdsAuth,
    Query(params): Query<OpdsPageParams>,
) -> ApiResult<Response> {
    let p = state.opds_prefix.as_ref();
    let page = state
        .services
        .catalog
        .search_books_for_opds(
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
    let base = format!("{p}/v1.2/all");
    Ok(xml_response(
        ACQ_MIME,
        render_acquisition_feed(
            p,
            "urn:xmedia:book:opds:v1:all",
            "All Books",
            &base,
            None,
            &page,
        ),
    ))
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
    let base = format!("{p}/v1.2/recent");
    Ok(xml_response(
        ACQ_MIME,
        render_acquisition_feed(
            p,
            "urn:xmedia:book:opds:v1:recent",
            "Continue Reading",
            &base,
            None,
            &page,
        ),
    ))
}

async fn feed_favorites(
    State(state): State<BookHttpState>,
    BookOpdsAuth(actor): BookOpdsAuth,
    Query(params): Query<OpdsPageParams>,
) -> ApiResult<Response> {
    let p = state.opds_prefix.as_ref();
    let page = state
        .services
        .catalog
        .search_books_for_opds(
            Some(&actor),
            CatalogSearchFilter {
                favorites_only: true,
                page: params.page,
                page_size: params.page_size,
                ..Default::default()
            },
        )
        .await?;
    let base = format!("{p}/v1.2/favorites");
    Ok(xml_response(
        ACQ_MIME,
        render_acquisition_feed(
            p,
            "urn:xmedia:book:opds:v1:favorites",
            "My Favorites",
            &base,
            None,
            &page,
        ),
    ))
}

async fn feed_search(
    State(state): State<BookHttpState>,
    BookOpdsAuth(actor): BookOpdsAuth,
    Query(params): Query<OpdsPageParams>,
) -> ApiResult<Response> {
    let p = state.opds_prefix.as_ref();
    let kw = params.q.or(params.query).unwrap_or_default();
    let extra = format!("q={}", url_encode_param(&kw));
    let page = state
        .services
        .catalog
        .search_books_for_opds(
            Some(&actor),
            CatalogSearchFilter {
                keyword: Some(kw.clone()),
                page: params.page,
                page_size: params.page_size,
                ..Default::default()
            },
        )
        .await?;
    let base = format!("{p}/v1.2/search");
    Ok(xml_response(
        ACQ_MIME,
        render_acquisition_feed(
            p,
            "urn:xmedia:book:opds:v1:search",
            &format!("Search: {kw}"),
            &base,
            Some(&extra),
            &page,
        ),
    ))
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
    let entries: Vec<NavEntry> = libs
        .into_iter()
        .map(|l| NavEntry {
            id: format!("urn:xmedia:book:library:{}", l.id),
            title: l.name,
            href: format!("{p}/v1.2/libraries/{}", l.id),
            content: format!("{} books", l.item_count),
            kind_mime: ACQ_MIME,
        })
        .collect();
    let self_href = format!("{p}/v1.2/libraries");
    Ok(xml_response(
        NAV_MIME,
        render_navigation_feed(
            p,
            "urn:xmedia:book:opds:v1:libraries",
            "Libraries",
            &self_href,
            &entries,
        ),
    ))
}

async fn feed_by_library(
    State(state): State<BookHttpState>,
    BookOpdsAuth(actor): BookOpdsAuth,
    Path(id): Path<Uuid>,
    Query(params): Query<OpdsPageParams>,
) -> ApiResult<Response> {
    let p = state.opds_prefix.as_ref();
    let lib = state.libraries.get_library(LibraryId(id)).await?;
    let page = state
        .services
        .catalog
        .search_books_for_opds(
            Some(&actor),
            CatalogSearchFilter {
                library_id: Some(LibraryId(id)),
                page: params.page,
                page_size: params.page_size,
                ..Default::default()
            },
        )
        .await?;
    let base = format!("{p}/v1.2/libraries/{id}");
    Ok(xml_response(
        ACQ_MIME,
        render_acquisition_feed(
            p,
            &format!("urn:xmedia:book:library:{id}"),
            &lib.name,
            &base,
            None,
            &page,
        ),
    ))
}

async fn nav_authors(
    State(state): State<BookHttpState>,
    BookOpdsAuth(_actor): BookOpdsAuth,
) -> ApiResult<Response> {
    let p = state.opds_prefix.as_ref();
    let authors = state.services.catalog.list_authors().await?;
    let entries: Vec<NavEntry> = authors
        .into_iter()
        .map(|a| NavEntry {
            id: format!("urn:xmedia:book:author:{}", a.name),
            title: a.name.clone(),
            href: format!("{p}/v1.2/authors/books?name={}", url_encode_param(&a.name)),
            content: format!("{} books", a.count),
            kind_mime: ACQ_MIME,
        })
        .collect();
    let self_href = format!("{p}/v1.2/authors");
    Ok(xml_response(
        NAV_MIME,
        render_navigation_feed(
            p,
            "urn:xmedia:book:opds:v1:authors",
            "Authors",
            &self_href,
            &entries,
        ),
    ))
}

async fn feed_by_author(
    State(state): State<BookHttpState>,
    BookOpdsAuth(actor): BookOpdsAuth,
    Query(params): Query<OpdsPageParams>,
) -> ApiResult<Response> {
    let p = state.opds_prefix.as_ref();
    let author = params.name.unwrap_or_default();
    let extra = format!("name={}", url_encode_param(&author));
    let page = state
        .services
        .catalog
        .search_books_for_opds(
            Some(&actor),
            CatalogSearchFilter {
                author: Some(author.clone()),
                page: params.page,
                page_size: params.page_size,
                ..Default::default()
            },
        )
        .await?;
    let base = format!("{p}/v1.2/authors/books");
    Ok(xml_response(
        ACQ_MIME,
        render_acquisition_feed(
            p,
            &format!("urn:xmedia:book:author:{author}"),
            &format!("Author: {author}"),
            &base,
            Some(&extra),
            &page,
        ),
    ))
}

async fn nav_tags(
    State(state): State<BookHttpState>,
    BookOpdsAuth(_actor): BookOpdsAuth,
) -> ApiResult<Response> {
    let p = state.opds_prefix.as_ref();
    let tags = state.services.catalog.list_tags().await?;
    let entries: Vec<NavEntry> = tags
        .into_iter()
        .map(|t| NavEntry {
            id: format!("urn:xmedia:book:tag:{}", t.name),
            title: t.name.clone(),
            href: format!("{p}/v1.2/tags/books?name={}", url_encode_param(&t.name)),
            content: format!("{} books", t.count),
            kind_mime: ACQ_MIME,
        })
        .collect();
    let self_href = format!("{p}/v1.2/tags");
    Ok(xml_response(
        NAV_MIME,
        render_navigation_feed(
            p,
            "urn:xmedia:book:opds:v1:tags",
            "Tags",
            &self_href,
            &entries,
        ),
    ))
}

async fn feed_by_tag(
    State(state): State<BookHttpState>,
    BookOpdsAuth(actor): BookOpdsAuth,
    Query(params): Query<OpdsPageParams>,
) -> ApiResult<Response> {
    let p = state.opds_prefix.as_ref();
    let tag = params.name.unwrap_or_default();
    let extra = format!("name={}", url_encode_param(&tag));
    let page = state
        .services
        .catalog
        .search_books_for_opds(
            Some(&actor),
            CatalogSearchFilter {
                tag: Some(tag.clone()),
                page: params.page,
                page_size: params.page_size,
                ..Default::default()
            },
        )
        .await?;
    let base = format!("{p}/v1.2/tags/books");
    Ok(xml_response(
        ACQ_MIME,
        render_acquisition_feed(
            p,
            &format!("urn:xmedia:book:tag:{tag}"),
            &format!("Tag: {tag}"),
            &base,
            Some(&extra),
            &page,
        ),
    ))
}

async fn nav_shelves(
    State(state): State<BookHttpState>,
    BookOpdsAuth(actor): BookOpdsAuth,
) -> ApiResult<Response> {
    let p = state.opds_prefix.as_ref();
    let shelves = state.services.reading.list_shelves(&actor).await?;
    let entries: Vec<NavEntry> = shelves
        .into_iter()
        .map(|s| NavEntry {
            id: format!("urn:xmedia:book:shelf:{}", s.id),
            title: s.name,
            href: format!("{p}/v1.2/shelves/{}", s.id),
            content: format!("{} books", s.book_count),
            kind_mime: ACQ_MIME,
        })
        .collect();
    let self_href = format!("{p}/v1.2/shelves");
    Ok(xml_response(
        NAV_MIME,
        render_navigation_feed(
            p,
            "urn:xmedia:book:opds:v1:shelves",
            "Personal Shelves",
            &self_href,
            &entries,
        ),
    ))
}

async fn feed_by_shelf(
    State(state): State<BookHttpState>,
    BookOpdsAuth(actor): BookOpdsAuth,
    Path(id): Path<Uuid>,
    Query(params): Query<OpdsPageParams>,
) -> ApiResult<Response> {
    let p = state.opds_prefix.as_ref();
    let page = state
        .services
        .catalog
        .search_books_for_opds(
            Some(&actor),
            CatalogSearchFilter {
                shelf_id: Some(ShelfId(id)),
                page: params.page,
                page_size: params.page_size,
                ..Default::default()
            },
        )
        .await?;
    let base = format!("{p}/v1.2/shelves/{id}");
    Ok(xml_response(
        ACQ_MIME,
        render_acquisition_feed(
            p,
            &format!("urn:xmedia:book:shelf:{id}"),
            "Shelf",
            &base,
            None,
            &page,
        ),
    ))
}

async fn opds_cover(
    State(state): State<BookHttpState>,
    BookOpdsAuth(_actor): BookOpdsAuth,
    Path(id): Path<Uuid>,
) -> ApiResult<Response> {
    let cover = state.services.catalog.get_cover(BookId(id)).await?;
    let mut res = (StatusCode::OK, Body::from(cover.bytes)).into_response();
    if let Ok(ct) = HeaderValue::from_str(&cover.mime_type) {
        res.headers_mut().insert(CONTENT_TYPE, ct);
    }
    res.headers_mut().insert(
        CACHE_CONTROL,
        HeaderValue::from_static("private, max-age=600"),
    );
    Ok(res)
}

async fn opds_download(
    State(state): State<BookHttpState>,
    BookOpdsAuth(_actor): BookOpdsAuth,
    Path(id): Path<Uuid>,
    headers: HeaderMap,
) -> ApiResult<Response> {
    serve_book_file_http(&state, BookId(id), &headers, true).await
}

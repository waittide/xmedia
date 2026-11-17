use axum::body::Body;
use axum::extract::{Path, Query, State};
use axum::http::{HeaderMap, HeaderValue, StatusCode, header};
use axum::response::{IntoResponse, Redirect, Response};
use axum::routing::get;
use axum::{Json, Router};
use serde::{Deserialize, Serialize};
use serde_json::json;
use uuid::Uuid;
use xmedia_core::domain::library::LibraryId;
use xmedia_core::http::ApiResult;

use crate::application::dto::UpdateProgressInput;
use crate::application::opds::{OpdsChapterEntry, OpdsFeed, OpdsFeedPayload, OpdsNavItem};
use crate::domain::id::{ChapterId, ComicShelfId, SeriesId};
use crate::http::extractors::ComicOpdsAuth;
use crate::http::rest::respond_with_etag;
use crate::http::state::ComicHttpState;

#[derive(Debug, Deserialize, Default)]
struct OpdsSearchQuery {
    q: Option<String>,
}

pub fn router() -> Router<ComicHttpState> {
    Router::new()
        .route(
            "/",
            get(|State(state): State<ComicHttpState>| async move {
                Redirect::temporary(&format!("{}/v1.2", state.opds_prefix))
            }),
        )
        .route("/covers/{key}", get(opds_cover))
        // OPDS 1.2 (Atom XML + PSE 1.1)
        .route("/v1.2", get(v12_root))
        .route("/v1.2/opensearch.xml", get(v12_opensearch))
        .route("/v1.2/continue", get(v12_continue))
        .route("/v1.2/latest", get(v12_latest))
        .route("/v1.2/series", get(v12_all_series))
        .route("/v1.2/series/{id}", get(v12_series_chapters))
        .route("/v1.2/libraries", get(v12_libraries))
        .route("/v1.2/libraries/{id}", get(v12_library_series))
        .route("/v1.2/tags", get(v12_tags))
        .route("/v1.2/tags/{tag}", get(v12_tag_series))
        .route("/v1.2/genres/{genre}", get(v12_genre_series))
        .route("/v1.2/shelves", get(v12_shelves))
        .route("/v1.2/shelves/{id}", get(v12_shelf_series))
        .route("/v1.2/favorites", get(v12_favorites))
        .route("/v1.2/chapters/{id}/download", get(opds_download_chapter))
        .route(
            "/v1.2/chapters/{id}/pse/{page_index}",
            get(opds_pse_stream_page),
        )
        // OPDS 2.0 (JSON)
        .route("/v2", get(v20_root))
        .route("/v2.0", get(v20_root))
        .route("/v2.0/continue", get(v20_continue))
        .route("/v2.0/latest", get(v20_latest))
        .route("/v2.0/series", get(v20_all_series))
        .route("/v2.0/series/{id}", get(v20_series_chapters))
        .route("/v2.0/libraries", get(v20_libraries))
        .route("/v2.0/libraries/{id}", get(v20_library_series))
        .route("/v2.0/tags", get(v20_tags))
        .route("/v2.0/tags/{tag}", get(v20_tag_series))
        .route("/v2.0/genres/{genre}", get(v20_genre_series))
        .route("/v2.0/shelves", get(v20_shelves))
        .route("/v2.0/shelves/{id}", get(v20_shelf_series))
        .route("/v2.0/favorites", get(v20_favorites))
}

async fn opds_cover(
    State(state): State<ComicHttpState>,
    ComicOpdsAuth(_user): ComicOpdsAuth,
    headers: HeaderMap,
    Path(key): Path<String>,
) -> ApiResult<Response> {
    let thumb = state.services.catalog.read_cover(&key).await?;
    Ok(respond_with_etag(thumb, &headers))
}

async fn opds_download_chapter(
    State(state): State<ComicHttpState>,
    ComicOpdsAuth(_user): ComicOpdsAuth,
    Path(id): Path<Uuid>,
) -> ApiResult<Response> {
    let (bytes, filename, mime) = state
        .services
        .catalog
        .download_chapter_archive(ChapterId(id))
        .await?;
    let mut res = Response::new(Body::from(bytes));
    *res.status_mut() = StatusCode::OK;
    if let Ok(ct) = HeaderValue::from_str(&mime) {
        res.headers_mut().insert(header::CONTENT_TYPE, ct);
    }
    if let Ok(cd) = HeaderValue::from_str(&format!("attachment; filename=\"{filename}\"")) {
        res.headers_mut().insert(header::CONTENT_DISPOSITION, cd);
    }
    Ok(res)
}

async fn opds_pse_stream_page(
    State(state): State<ComicHttpState>,
    ComicOpdsAuth(user): ComicOpdsAuth,
    headers: HeaderMap,
    Path((id, page_index)): Path<(Uuid, u32)>,
) -> ApiResult<Response> {
    let chapter_id = ChapterId(id);
    let page_number = page_index.saturating_add(1);
    let page = state
        .services
        .catalog
        .read_chapter_page(chapter_id, page_number)
        .await?;

    let _ = state
        .services
        .reading
        .update_progress(
            user.id,
            chapter_id,
            UpdateProgressInput {
                current_page: page_number,
                is_completed: None,
            },
        )
        .await;

    Ok(respond_with_etag(page, &headers))
}

async fn v12_opensearch(
    State(state): State<ComicHttpState>,
    ComicOpdsAuth(_user): ComicOpdsAuth,
) -> Response {
    let p = state.opds_prefix.as_ref();
    let xml = format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<OpenSearchDescription xmlns="http://a9.com/-/spec/opensearch/1.1/">
  <ShortName>xmedia Comics</ShortName>
  <Description>Search comic series in xmedia</Description>
  <InputEncoding>UTF-8</InputEncoding>
  <OutputEncoding>UTF-8</OutputEncoding>
  <Url type="application/atom+xml;profile=opds-catalog;kind=navigation" template="{p}/v1.2/series?q={{searchTerms}}"/>
</OpenSearchDescription>"#
    );
    (
        StatusCode::OK,
        [(
            header::CONTENT_TYPE,
            "application/opensearchdescription+xml; charset=utf-8",
        )],
        xml,
    )
        .into_response()
}

// --- OPDS 1.2 Handlers ---

async fn v12_root(
    State(state): State<ComicHttpState>,
    ComicOpdsAuth(_u): ComicOpdsAuth,
) -> Response {
    let p = state.opds_prefix.as_ref();
    let vp = format!("{p}/v1.2");
    render_atom_feed(p, &state.services.opds.root_feed(&vp))
}

async fn v12_continue(
    State(state): State<ComicHttpState>,
    ComicOpdsAuth(user): ComicOpdsAuth,
) -> ApiResult<Response> {
    let p = state.opds_prefix.as_ref();
    let vp = format!("{p}/v1.2");
    let feed = state
        .services
        .opds
        .continue_reading_feed(&user, &state.services.catalog, p, &vp)
        .await?;
    Ok(render_atom_feed(p, &feed))
}

async fn v12_latest(
    State(state): State<ComicHttpState>,
    ComicOpdsAuth(user): ComicOpdsAuth,
) -> ApiResult<Response> {
    let p = state.opds_prefix.as_ref();
    let vp = format!("{p}/v1.2");
    let feed = state
        .services
        .opds
        .latest_series_feed(&user, &state.services.catalog, p, &vp)
        .await?;
    Ok(render_atom_feed(p, &feed))
}

async fn v12_all_series(
    State(state): State<ComicHttpState>,
    ComicOpdsAuth(user): ComicOpdsAuth,
    Query(q): Query<OpdsSearchQuery>,
) -> ApiResult<Response> {
    let p = state.opds_prefix.as_ref();
    let vp = format!("{p}/v1.2");
    let feed = state
        .services
        .opds
        .all_series_feed(&user, &state.services.catalog, q.q, p, &vp)
        .await?;
    Ok(render_atom_feed(p, &feed))
}

async fn v12_series_chapters(
    State(state): State<ComicHttpState>,
    ComicOpdsAuth(user): ComicOpdsAuth,
    Path(id): Path<Uuid>,
) -> ApiResult<Response> {
    let p = state.opds_prefix.as_ref();
    let vp = format!("{p}/v1.2");
    let feed = state
        .services
        .opds
        .series_chapters_feed(&user, SeriesId(id), p, &vp)
        .await?;
    Ok(render_atom_feed(p, &feed))
}

async fn v12_libraries(
    State(state): State<ComicHttpState>,
    ComicOpdsAuth(_user): ComicOpdsAuth,
) -> ApiResult<Response> {
    let p = state.opds_prefix.as_ref();
    let vp = format!("{p}/v1.2");
    let feed = state.services.opds.libraries_feed(&vp).await?;
    Ok(render_atom_feed(p, &feed))
}

async fn v12_library_series(
    State(state): State<ComicHttpState>,
    ComicOpdsAuth(user): ComicOpdsAuth,
    Path(id): Path<Uuid>,
) -> ApiResult<Response> {
    let p = state.opds_prefix.as_ref();
    let vp = format!("{p}/v1.2");
    let feed = state
        .services
        .opds
        .library_series_feed(&user, &state.services.catalog, LibraryId(id), p, &vp)
        .await?;
    Ok(render_atom_feed(p, &feed))
}

async fn v12_tags(
    State(state): State<ComicHttpState>,
    ComicOpdsAuth(_u): ComicOpdsAuth,
) -> ApiResult<Response> {
    let p = state.opds_prefix.as_ref();
    let vp = format!("{p}/v1.2");
    let feed = state
        .services
        .opds
        .tags_feed(&state.services.catalog, &vp)
        .await?;
    Ok(render_atom_feed(p, &feed))
}

async fn v12_tag_series(
    State(state): State<ComicHttpState>,
    ComicOpdsAuth(user): ComicOpdsAuth,
    Path(tag): Path<String>,
) -> ApiResult<Response> {
    let p = state.opds_prefix.as_ref();
    let vp = format!("{p}/v1.2");
    let feed = state
        .services
        .opds
        .tag_series_feed(&user, &state.services.catalog, &tag, false, p, &vp)
        .await?;
    Ok(render_atom_feed(p, &feed))
}

async fn v12_genre_series(
    State(state): State<ComicHttpState>,
    ComicOpdsAuth(user): ComicOpdsAuth,
    Path(genre): Path<String>,
) -> ApiResult<Response> {
    let p = state.opds_prefix.as_ref();
    let vp = format!("{p}/v1.2");
    let feed = state
        .services
        .opds
        .tag_series_feed(&user, &state.services.catalog, &genre, true, p, &vp)
        .await?;
    Ok(render_atom_feed(p, &feed))
}

async fn v12_shelves(
    State(state): State<ComicHttpState>,
    ComicOpdsAuth(user): ComicOpdsAuth,
) -> ApiResult<Response> {
    let p = state.opds_prefix.as_ref();
    let vp = format!("{p}/v1.2");
    let feed = state.services.opds.shelves_feed(&user, &vp).await?;
    Ok(render_atom_feed(p, &feed))
}

async fn v12_shelf_series(
    State(state): State<ComicHttpState>,
    ComicOpdsAuth(user): ComicOpdsAuth,
    Path(id): Path<Uuid>,
) -> ApiResult<Response> {
    let p = state.opds_prefix.as_ref();
    let vp = format!("{p}/v1.2");
    let feed = state
        .services
        .opds
        .shelf_series_feed(&user, &state.services.catalog, ComicShelfId(id), p, &vp)
        .await?;
    Ok(render_atom_feed(p, &feed))
}

async fn v12_favorites(
    State(state): State<ComicHttpState>,
    ComicOpdsAuth(user): ComicOpdsAuth,
) -> ApiResult<Response> {
    let p = state.opds_prefix.as_ref();
    let vp = format!("{p}/v1.2");
    let feed = state
        .services
        .opds
        .favorites_series_feed(&user, &state.services.catalog, p, &vp)
        .await?;
    Ok(render_atom_feed(p, &feed))
}

// --- OPDS 2.0 Handlers ---

async fn v20_root(
    State(state): State<ComicHttpState>,
    ComicOpdsAuth(_u): ComicOpdsAuth,
) -> Response {
    let p = state.opds_prefix.as_ref();
    let vp = format!("{p}/v2.0");
    render_opds2_json(p, &state.services.opds.root_feed(&vp))
}

async fn v20_continue(
    State(state): State<ComicHttpState>,
    ComicOpdsAuth(user): ComicOpdsAuth,
) -> ApiResult<Response> {
    let p = state.opds_prefix.as_ref();
    let vp = format!("{p}/v2.0");
    let feed = state
        .services
        .opds
        .continue_reading_feed(&user, &state.services.catalog, p, &vp)
        .await?;
    Ok(render_opds2_json(p, &feed))
}

async fn v20_latest(
    State(state): State<ComicHttpState>,
    ComicOpdsAuth(user): ComicOpdsAuth,
) -> ApiResult<Response> {
    let p = state.opds_prefix.as_ref();
    let vp = format!("{p}/v2.0");
    let feed = state
        .services
        .opds
        .latest_series_feed(&user, &state.services.catalog, p, &vp)
        .await?;
    Ok(render_opds2_json(p, &feed))
}

async fn v20_all_series(
    State(state): State<ComicHttpState>,
    ComicOpdsAuth(user): ComicOpdsAuth,
    Query(q): Query<OpdsSearchQuery>,
) -> ApiResult<Response> {
    let p = state.opds_prefix.as_ref();
    let vp = format!("{p}/v2.0");
    let feed = state
        .services
        .opds
        .all_series_feed(&user, &state.services.catalog, q.q, p, &vp)
        .await?;
    Ok(render_opds2_json(p, &feed))
}

async fn v20_series_chapters(
    State(state): State<ComicHttpState>,
    ComicOpdsAuth(user): ComicOpdsAuth,
    Path(id): Path<Uuid>,
) -> ApiResult<Response> {
    let p = state.opds_prefix.as_ref();
    let vp = format!("{p}/v2.0");
    let feed = state
        .services
        .opds
        .series_chapters_feed(&user, SeriesId(id), p, &vp)
        .await?;
    Ok(render_opds2_json(p, &feed))
}

async fn v20_libraries(
    State(state): State<ComicHttpState>,
    ComicOpdsAuth(_u): ComicOpdsAuth,
) -> ApiResult<Response> {
    let p = state.opds_prefix.as_ref();
    let vp = format!("{p}/v2.0");
    let feed = state.services.opds.libraries_feed(&vp).await?;
    Ok(render_opds2_json(p, &feed))
}

async fn v20_library_series(
    State(state): State<ComicHttpState>,
    ComicOpdsAuth(user): ComicOpdsAuth,
    Path(id): Path<Uuid>,
) -> ApiResult<Response> {
    let p = state.opds_prefix.as_ref();
    let vp = format!("{p}/v2.0");
    let feed = state
        .services
        .opds
        .library_series_feed(&user, &state.services.catalog, LibraryId(id), p, &vp)
        .await?;
    Ok(render_opds2_json(p, &feed))
}

async fn v20_tags(
    State(state): State<ComicHttpState>,
    ComicOpdsAuth(_u): ComicOpdsAuth,
) -> ApiResult<Response> {
    let p = state.opds_prefix.as_ref();
    let vp = format!("{p}/v2.0");
    let feed = state
        .services
        .opds
        .tags_feed(&state.services.catalog, &vp)
        .await?;
    Ok(render_opds2_json(p, &feed))
}

async fn v20_tag_series(
    State(state): State<ComicHttpState>,
    ComicOpdsAuth(user): ComicOpdsAuth,
    Path(tag): Path<String>,
) -> ApiResult<Response> {
    let p = state.opds_prefix.as_ref();
    let vp = format!("{p}/v2.0");
    let feed = state
        .services
        .opds
        .tag_series_feed(&user, &state.services.catalog, &tag, false, p, &vp)
        .await?;
    Ok(render_opds2_json(p, &feed))
}

async fn v20_genre_series(
    State(state): State<ComicHttpState>,
    ComicOpdsAuth(user): ComicOpdsAuth,
    Path(genre): Path<String>,
) -> ApiResult<Response> {
    let p = state.opds_prefix.as_ref();
    let vp = format!("{p}/v2.0");
    let feed = state
        .services
        .opds
        .tag_series_feed(&user, &state.services.catalog, &genre, true, p, &vp)
        .await?;
    Ok(render_opds2_json(p, &feed))
}

async fn v20_shelves(
    State(state): State<ComicHttpState>,
    ComicOpdsAuth(user): ComicOpdsAuth,
) -> ApiResult<Response> {
    let p = state.opds_prefix.as_ref();
    let vp = format!("{p}/v2.0");
    let feed = state.services.opds.shelves_feed(&user, &vp).await?;
    Ok(render_opds2_json(p, &feed))
}

async fn v20_shelf_series(
    State(state): State<ComicHttpState>,
    ComicOpdsAuth(user): ComicOpdsAuth,
    Path(id): Path<Uuid>,
) -> ApiResult<Response> {
    let p = state.opds_prefix.as_ref();
    let vp = format!("{p}/v2.0");
    let feed = state
        .services
        .opds
        .shelf_series_feed(&user, &state.services.catalog, ComicShelfId(id), p, &vp)
        .await?;
    Ok(render_opds2_json(p, &feed))
}

async fn v20_favorites(
    State(state): State<ComicHttpState>,
    ComicOpdsAuth(user): ComicOpdsAuth,
) -> ApiResult<Response> {
    let p = state.opds_prefix.as_ref();
    let vp = format!("{p}/v2.0");
    let feed = state
        .services
        .opds
        .favorites_series_feed(&user, &state.services.catalog, p, &vp)
        .await?;
    Ok(render_opds2_json(p, &feed))
}

// --- Serializers ---

fn render_atom_feed(prefix: &str, feed: &OpdsFeed) -> Response {
    let is_acq = matches!(feed.payload, OpdsFeedPayload::Acquisition(_));
    let kind = if is_acq { "acquisition" } else { "navigation" };
    let feed_mime = format!("application/atom+xml;profile=opds-catalog;kind={kind}");
    let updated = feed.updated_at.to_string();

    let mut xml = String::with_capacity(4096);
    xml.push_str(r#"<?xml version="1.0" encoding="UTF-8"?>"#);
    xml.push('\n');
    xml.push_str(
        r#"<feed xmlns="http://www.w3.org/2005/Atom" xmlns:opds="http://opds-spec.org/2010/catalog" xmlns:pse="http://vaemendis.net/opds-pse/ns">"#,
    );
    xml.push_str(&format!("\n  <id>{}</id>", escape_xml(&feed.id)));
    xml.push_str(&format!("\n  <title>{}</title>", escape_xml(&feed.title)));
    xml.push_str(&format!("\n  <updated>{updated}</updated>"));
    xml.push_str("\n  <author><name>xmedia</name></author>");
    xml.push_str(&format!(
        "\n  <link rel=\"self\" href=\"{}\" type=\"{feed_mime}\"/>",
        escape_xml(&feed.self_path)
    ));
    xml.push_str(&format!(
        "\n  <link rel=\"start\" href=\"{prefix}/v1.2\" type=\"application/atom+xml;profile=opds-catalog;kind=navigation\"/>"
    ));
    xml.push_str(&format!(
        "\n  <link rel=\"search\" href=\"{prefix}/v1.2/opensearch.xml\" type=\"application/opensearchdescription+xml\"/>"
    ));

    match &feed.payload {
        OpdsFeedPayload::Navigation(items) => {
            for item in items {
                append_nav_entry_xml(&mut xml, item);
            }
        }
        OpdsFeedPayload::Acquisition(entries) => {
            for entry in entries {
                append_acq_entry_xml(&mut xml, prefix, entry);
            }
        }
    }

    xml.push_str("\n</feed>\n");

    let content_type = format!("{feed_mime}; charset=utf-8");
    let mut res = Response::new(Body::from(xml));
    *res.status_mut() = StatusCode::OK;
    if let Ok(hv) = HeaderValue::from_str(&content_type) {
        res.headers_mut().insert(header::CONTENT_TYPE, hv);
    }
    res
}

fn append_nav_entry_xml(xml: &mut String, item: &OpdsNavItem) {
    let updated = item.updated_at.to_string();
    let sub_kind = if item.subsection_path.contains("/series/")
        || item.subsection_path.ends_with("/continue")
    {
        "acquisition"
    } else {
        "navigation"
    };
    xml.push_str("\n  <entry>");
    xml.push_str(&format!("\n    <id>{}</id>", escape_xml(&item.id)));
    xml.push_str(&format!("\n    <title>{}</title>", escape_xml(&item.title)));
    xml.push_str(&format!("\n    <updated>{updated}</updated>"));
    xml.push_str(&format!(
        "\n    <content type=\"text\">{}</content>",
        escape_xml(&item.summary)
    ));
    xml.push_str(&format!(
        "\n    <link rel=\"subsection\" href=\"{}\" type=\"application/atom+xml;profile=opds-catalog;kind={sub_kind}\"/>",
        escape_xml(&item.subsection_path)
    ));
    if let Some(cover) = &item.cover_url {
        xml.push_str(&format!(
            "\n    <link rel=\"http://opds-spec.org/image/thumbnail\" href=\"{}\" type=\"image/webp\"/>",
            escape_xml(cover)
        ));
    }
    xml.push_str("\n  </entry>");
}

fn append_acq_entry_xml(xml: &mut String, prefix: &str, entry: &OpdsChapterEntry) {
    let updated = entry.updated_at.to_string();
    xml.push_str("\n  <entry>");
    xml.push_str(&format!(
        "\n    <id>urn:xmedia:comic:chapter:{}</id>",
        entry.id
    ));
    xml.push_str(&format!(
        "\n    <title>{} - {}</title>",
        escape_xml(&entry.series_title),
        escape_xml(&entry.title)
    ));
    xml.push_str(&format!("\n    <updated>{updated}</updated>"));
    for author in &entry.authors {
        xml.push_str(&format!(
            "\n    <author><name>{}</name></author>",
            escape_xml(author)
        ));
    }
    if !entry.summary.is_empty() {
        xml.push_str(&format!(
            "\n    <summary>{}</summary>",
            escape_xml(&entry.summary)
        ));
    }
    if let Some(cover) = &entry.cover_url {
        xml.push_str(&format!(
            "\n    <link rel=\"http://opds-spec.org/image\" href=\"{}\" type=\"image/webp\"/>",
            escape_xml(cover)
        ));
        xml.push_str(&format!(
            "\n    <link rel=\"http://opds-spec.org/image/thumbnail\" href=\"{}\" type=\"image/webp\"/>",
            escape_xml(cover)
        ));
    }
    if entry.can_download_archive {
        xml.push_str(&format!(
            "\n    <link rel=\"http://opds-spec.org/acquisition\" href=\"{prefix}/v1.2/chapters/{}/download\" type=\"{}\"/>",
            entry.id, entry.archive_mime
        ));
    }

    let mut pse_attrs = format!("pse:count=\"{}\"", entry.page_count);
    if let Some(last_page) = entry.last_read_page {
        pse_attrs.push_str(&format!(" pse:lastRead=\"{last_page}\""));
    }
    if let Some(last_ts) = entry.last_read_at {
        pse_attrs.push_str(&format!(" pse:lastReadDate=\"{last_ts}\""));
    }
    xml.push_str(&format!(
        "\n    <link rel=\"http://vaemendis.net/opds-pse/stream\" href=\"{prefix}/v1.2/chapters/{}/pse/{{pageNumber}}\" type=\"image/jpeg\" {pse_attrs}/>",
        entry.id
    ));

    xml.push_str("\n  </entry>");
}

#[derive(Serialize)]
struct Opds2Response {
    metadata: serde_json::Value,
    links: Vec<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    navigation: Option<Vec<serde_json::Value>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    publications: Option<Vec<serde_json::Value>>,
}

fn render_opds2_json(prefix: &str, feed: &OpdsFeed) -> Response {
    let modified = feed.updated_at.to_string();
    let links = vec![
        json!({
            "rel": "self",
            "href": feed.self_path,
            "type": "application/opds+json"
        }),
        json!({
            "rel": "start",
            "href": format!("{prefix}/v2.0"),
            "type": "application/opds+json"
        }),
        json!({
            "rel": "search",
            "href": format!("{prefix}/v2.0/series{{?q}}"),
            "type": "application/opds+json",
            "templated": true
        }),
    ];

    let (navigation, publications) = match &feed.payload {
        OpdsFeedPayload::Navigation(items) => {
            let nav: Vec<serde_json::Value> = items
                .iter()
                .map(|i| {
                    json!({
                        "title": i.title,
                        "href": i.subsection_path,
                        "type": "application/opds+json",
                        "properties": {
                            "subtitle": i.summary
                        }
                    })
                })
                .collect();
            (Some(nav), None)
        }
        OpdsFeedPayload::Acquisition(entries) => {
            let pubs: Vec<serde_json::Value> = entries
                .iter()
                .map(|e| {
                    let mut pub_links = vec![json!({
                        "rel": "http://vaemendis.net/opds-pse/stream",
                        "href": format!("{prefix}/v1.2/chapters/{}/pse/{{pageNumber}}", e.id),
                        "type": "image/jpeg",
                        "templated": true,
                        "properties": {
                            "numberOfItems": e.page_count,
                            "pse:lastRead": e.last_read_page,
                        }
                    })];
                    if e.can_download_archive {
                        pub_links.push(json!({
                            "rel": "http://opds-spec.org/acquisition",
                            "href": format!("{prefix}/v1.2/chapters/{}/download", e.id),
                            "type": e.archive_mime,
                        }));
                    }
                    let images: Vec<serde_json::Value> = e
                        .cover_url
                        .iter()
                        .map(|c| json!({ "href": c, "type": "image/webp" }))
                        .collect();
                    json!({
                        "metadata": {
                            "@type": "http://schema.org/ComicIssue",
                            "identifier": format!("urn:xmedia:comic:chapter:{}", e.id),
                            "title": format!("{} - {}", e.series_title, e.title),
                            "description": e.summary,
                            "author": e.authors,
                            "numberOfPages": e.page_count,
                            "modified": e.updated_at.to_string(),
                        },
                        "links": pub_links,
                        "images": images,
                    })
                })
                .collect();
            (None, Some(pubs))
        }
    };

    let body = Opds2Response {
        metadata: json!({
            "title": feed.title,
            "modified": modified,
        }),
        links,
        navigation,
        publications,
    };

    let mut res = Json(body).into_response();
    res.headers_mut().insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static("application/opds+json; charset=utf-8"),
    );
    res
}

fn escape_xml(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    for ch in input.chars() {
        match ch {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&apos;"),
            other => out.push(other),
        }
    }
    out
}

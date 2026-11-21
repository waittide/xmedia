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
use xmedia_core::domain::user::User;
use xmedia_core::domain::xml::xml_escape;
use xmedia_core::http::{ApiResult, OpdsAuth, into_range_http_response, parse_http_range_spec};

use crate::application::dto::UpdateProgressInput;
use crate::application::opds::{OpdsChapterEntry, OpdsFeed, OpdsFeedPayload, OpdsNavItem};
use crate::domain::id::{ChapterId, ComicShelfId, SeriesId};
use crate::http::rest::respond_with_etag;
use crate::http::state::ComicHttpState;

#[derive(Debug, Deserialize, Default)]
struct OpdsSearchQuery {
    q: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum OpdsVersion {
    V12,
    V20,
}

impl OpdsVersion {
    fn version_prefix(self, opds_prefix: &str) -> String {
        match self {
            Self::V12 => format!("{opds_prefix}/v1.2"),
            Self::V20 => format!("{opds_prefix}/v2.0"),
        }
    }

    fn render(self, opds_prefix: &str, feed: &OpdsFeed) -> Response {
        match self {
            Self::V12 => render_atom_feed(opds_prefix, feed),
            Self::V20 => render_opds2_json(opds_prefix, feed),
        }
    }
}

enum FeedKind {
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

pub fn router() -> Router<ComicHttpState> {
    let base = Router::new()
        .route(
            "/",
            get(|State(state): State<ComicHttpState>| async move {
                Redirect::temporary(&format!("{}/v1.2", state.opds_prefix))
            }),
        )
        .route("/covers/{key}", get(opds_cover))
        // OPDS 1.2 specific endpoints (OpenSearch + PSE 1.1 + Archive Download)
        .route("/v1.2/opensearch.xml", get(v12_opensearch))
        .route("/v1.2/chapters/{id}/download", get(opds_download_chapter))
        .route(
            "/v1.2/chapters/{id}/pse/{page_index}",
            get(opds_pse_stream_page),
        )
        // OPDS 2.0 short alias
        .route(
            "/v2",
            get(
                |State(state): State<ComicHttpState>, OpdsAuth(user): OpdsAuth| async move {
                    serve_feed(state, user, OpdsVersion::V20, FeedKind::Root).await
                },
            ),
        );

    let with_v12 = mount_versioned_feeds(base, "/v1.2", OpdsVersion::V12);
    mount_versioned_feeds(with_v12, "/v2.0", OpdsVersion::V20)
}

fn mount_versioned_feeds(
    router: Router<ComicHttpState>,
    base: &str,
    version: OpdsVersion,
) -> Router<ComicHttpState> {
    router
        .route(
            base,
            get(
                move |State(state): State<ComicHttpState>, OpdsAuth(user): OpdsAuth| async move {
                    serve_feed(state, user, version, FeedKind::Root).await
                },
            ),
        )
        .route(
            &format!("{base}/continue"),
            get(
                move |State(state): State<ComicHttpState>, OpdsAuth(user): OpdsAuth| async move {
                    serve_feed(state, user, version, FeedKind::Continue).await
                },
            ),
        )
        .route(
            &format!("{base}/latest"),
            get(
                move |State(state): State<ComicHttpState>, OpdsAuth(user): OpdsAuth| async move {
                    serve_feed(state, user, version, FeedKind::Latest).await
                },
            ),
        )
        .route(
            &format!("{base}/series"),
            get(
                move |State(state): State<ComicHttpState>,
                      OpdsAuth(user): OpdsAuth,
                      Query(q): Query<OpdsSearchQuery>| async move {
                    serve_feed(state, user, version, FeedKind::AllSeries(q.q)).await
                },
            ),
        )
        .route(
            &format!("{base}/series/{{id}}"),
            get(
                move |State(state): State<ComicHttpState>,
                      OpdsAuth(user): OpdsAuth,
                      Path(id): Path<Uuid>| async move {
                    serve_feed(state, user, version, FeedKind::SeriesChapters(SeriesId(id))).await
                },
            ),
        )
        .route(
            &format!("{base}/libraries"),
            get(
                move |State(state): State<ComicHttpState>, OpdsAuth(user): OpdsAuth| async move {
                    serve_feed(state, user, version, FeedKind::Libraries).await
                },
            ),
        )
        .route(
            &format!("{base}/libraries/{{id}}"),
            get(
                move |State(state): State<ComicHttpState>,
                      OpdsAuth(user): OpdsAuth,
                      Path(id): Path<Uuid>| async move {
                    serve_feed(state, user, version, FeedKind::LibrarySeries(LibraryId(id))).await
                },
            ),
        )
        .route(
            &format!("{base}/tags"),
            get(
                move |State(state): State<ComicHttpState>, OpdsAuth(user): OpdsAuth| async move {
                    serve_feed(state, user, version, FeedKind::Tags).await
                },
            ),
        )
        .route(
            &format!("{base}/tags/{{tag}}"),
            get(
                move |State(state): State<ComicHttpState>,
                      OpdsAuth(user): OpdsAuth,
                      Path(tag): Path<String>| async move {
                    serve_feed(
                        state,
                        user,
                        version,
                        FeedKind::TagSeries {
                            name: tag,
                            is_genre: false,
                        },
                    )
                    .await
                },
            ),
        )
        .route(
            &format!("{base}/genres/{{genre}}"),
            get(
                move |State(state): State<ComicHttpState>,
                      OpdsAuth(user): OpdsAuth,
                      Path(genre): Path<String>| async move {
                    serve_feed(
                        state,
                        user,
                        version,
                        FeedKind::TagSeries {
                            name: genre,
                            is_genre: true,
                        },
                    )
                    .await
                },
            ),
        )
        .route(
            &format!("{base}/shelves"),
            get(
                move |State(state): State<ComicHttpState>, OpdsAuth(user): OpdsAuth| async move {
                    serve_feed(state, user, version, FeedKind::Shelves).await
                },
            ),
        )
        .route(
            &format!("{base}/shelves/{{id}}"),
            get(
                move |State(state): State<ComicHttpState>,
                      OpdsAuth(user): OpdsAuth,
                      Path(id): Path<Uuid>| async move {
                    serve_feed(
                        state,
                        user,
                        version,
                        FeedKind::ShelfSeries(ComicShelfId(id)),
                    )
                    .await
                },
            ),
        )
        .route(
            &format!("{base}/favorites"),
            get(
                move |State(state): State<ComicHttpState>, OpdsAuth(user): OpdsAuth| async move {
                    serve_feed(state, user, version, FeedKind::Favorites).await
                },
            ),
        )
}

async fn serve_feed(
    state: ComicHttpState,
    user: User,
    version: OpdsVersion,
    kind: FeedKind,
) -> ApiResult<Response> {
    let p = state.opds_prefix.as_ref();
    let vp = version.version_prefix(p);
    let opds = &state.services.opds;
    let catalog = &state.services.catalog;

    let feed = match kind {
        FeedKind::Root => opds.root_feed(&vp),
        FeedKind::Continue => opds.continue_reading_feed(&user, catalog, p, &vp).await?,
        FeedKind::Latest => opds.latest_series_feed(&user, catalog, p, &vp).await?,
        FeedKind::AllSeries(search) => opds.all_series_feed(&user, catalog, search, p, &vp).await?,
        FeedKind::SeriesChapters(series_id) => {
            opds.series_chapters_feed(&user, series_id, p, &vp).await?
        }
        FeedKind::Libraries => opds.libraries_feed(&vp).await?,
        FeedKind::LibrarySeries(lib_id) => {
            opds.library_series_feed(&user, catalog, lib_id, p, &vp)
                .await?
        }
        FeedKind::Tags => opds.tags_feed(catalog, &vp).await?,
        FeedKind::TagSeries { name, is_genre } => {
            opds.tag_series_feed(&user, catalog, &name, is_genre, p, &vp)
                .await?
        }
        FeedKind::Shelves => opds.shelves_feed(&user, &vp).await?,
        FeedKind::ShelfSeries(shelf_id) => {
            opds.shelf_series_feed(&user, catalog, shelf_id, p, &vp)
                .await?
        }
        FeedKind::Favorites => opds.favorites_series_feed(&user, catalog, p, &vp).await?,
    };

    Ok(version.render(p, &feed))
}

async fn opds_cover(
    State(state): State<ComicHttpState>,
    OpdsAuth(_user): OpdsAuth,
    headers: HeaderMap,
    Path(key): Path<String>,
) -> ApiResult<Response> {
    let thumb = state.services.catalog.read_cover(&key).await?;
    Ok(respond_with_etag(thumb, &headers))
}

async fn opds_download_chapter(
    State(state): State<ComicHttpState>,
    OpdsAuth(_user): OpdsAuth,
    headers: HeaderMap,
    Path(id): Path<Uuid>,
) -> ApiResult<Response> {
    let spec = parse_http_range_spec(&headers);
    let (stream, filename, mime) = state
        .services
        .catalog
        .open_chapter_archive_stream(ChapterId(id), spec)
        .await?;
    Ok(into_range_http_response(
        stream,
        spec.is_partial(),
        &mime,
        Some(&filename),
        true,
        None,
    ))
}

async fn opds_pse_stream_page(
    State(state): State<ComicHttpState>,
    OpdsAuth(user): OpdsAuth,
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
    OpdsAuth(_user): OpdsAuth,
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
    xml.push_str(&format!("\n  <id>{}</id>", xml_escape(&feed.id)));
    xml.push_str(&format!("\n  <title>{}</title>", xml_escape(&feed.title)));
    xml.push_str(&format!("\n  <updated>{updated}</updated>"));
    xml.push_str("\n  <author><name>xmedia</name></author>");
    xml.push_str(&format!(
        "\n  <link rel=\"self\" href=\"{}\" type=\"{feed_mime}\"/>",
        xml_escape(&feed.self_path)
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

fn infer_cover_mime(cover_url: &str) -> &'static str {
    let clean = cover_url
        .split('?')
        .next()
        .unwrap_or(cover_url)
        .to_ascii_lowercase();
    if clean.ends_with(".png") {
        "image/png"
    } else if clean.ends_with(".webp") {
        "image/webp"
    } else if clean.ends_with(".gif") {
        "image/gif"
    } else if clean.ends_with(".avif") {
        "image/avif"
    } else {
        "image/jpeg"
    }
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
    xml.push_str(&format!("\n    <id>{}</id>", xml_escape(&item.id)));
    xml.push_str(&format!("\n    <title>{}</title>", xml_escape(&item.title)));
    xml.push_str(&format!("\n    <updated>{updated}</updated>"));
    xml.push_str(&format!(
        "\n    <content type=\"text\">{}</content>",
        xml_escape(&item.summary)
    ));
    xml.push_str(&format!(
        "\n    <link rel=\"subsection\" href=\"{}\" type=\"application/atom+xml;profile=opds-catalog;kind={sub_kind}\"/>",
        xml_escape(&item.subsection_path)
    ));
    if let Some(cover) = &item.cover_url {
        let cover_mime = infer_cover_mime(cover);
        xml.push_str(&format!(
            "\n    <link rel=\"http://opds-spec.org/image/thumbnail\" href=\"{}\" type=\"{cover_mime}\"/>",
            xml_escape(cover)
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
        xml_escape(&entry.series_title),
        xml_escape(&entry.title)
    ));
    xml.push_str(&format!("\n    <updated>{updated}</updated>"));
    for author in &entry.authors {
        xml.push_str(&format!(
            "\n    <author><name>{}</name></author>",
            xml_escape(author)
        ));
    }
    if !entry.summary.is_empty() {
        xml.push_str(&format!(
            "\n    <summary>{}</summary>",
            xml_escape(&entry.summary)
        ));
    }
    if let Some(cover) = &entry.cover_url {
        let cover_mime = infer_cover_mime(cover);
        xml.push_str(&format!(
            "\n    <link rel=\"http://opds-spec.org/image\" href=\"{}\" type=\"{cover_mime}\"/>",
            xml_escape(cover)
        ));
        xml.push_str(&format!(
            "\n    <link rel=\"http://opds-spec.org/image/thumbnail\" href=\"{}\" type=\"{cover_mime}\"/>",
            xml_escape(cover)
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
        let last_read_zero_based = last_page.saturating_sub(1);
        pse_attrs.push_str(&format!(" pse:lastRead=\"{last_read_zero_based}\""));
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
                            "pse:lastRead": e.last_read_page.map(|p| p.saturating_sub(1)),
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
                        .map(|c| json!({ "href": c, "type": infer_cover_mime(c) }))
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

#[cfg(test)]
mod tests {
    use axum::body::to_bytes;
    use jiff::Timestamp;

    use super::*;

    #[tokio::test]
    async fn render_atom_and_opds2_navigation_feed_escapes_xml_and_preserves_schema() {
        let now = Timestamp::from_second(1_700_000_000).unwrap();
        let feed = OpdsFeed {
            id: "urn:xmedia:comic:opds:root".into(),
            title: "Tom & Jerry <Comics> \"Special\" 'Ed'".into(),
            self_path: "/opds/comic/v1.2".into(),
            updated_at: now,
            payload: OpdsFeedPayload::Navigation(vec![OpdsNavItem {
                id: "urn:xmedia:comic:series:1".into(),
                title: "A & B <Vol.1>".into(),
                summary: "3 chapters & extras".into(),
                subsection_path: "/opds/comic/v1.2/series/1".into(),
                cover_url: Some("/opds/comic/covers/abc.png".into()),
                updated_at: now,
            }]),
        };

        let atom_res = OpdsVersion::V12.render("/opds/comic", &feed);
        assert_eq!(atom_res.status(), StatusCode::OK);
        assert_eq!(
            atom_res.headers().get(header::CONTENT_TYPE).unwrap(),
            "application/atom+xml;profile=opds-catalog;kind=navigation; charset=utf-8"
        );
        let xml = String::from_utf8(
            to_bytes(atom_res.into_body(), usize::MAX)
                .await
                .unwrap()
                .to_vec(),
        )
        .unwrap();
        assert!(xml.contains("Tom &amp; Jerry &lt;Comics&gt; &quot;Special&quot; &apos;Ed&apos;"));
        assert!(xml.contains("<title>A &amp; B &lt;Vol.1&gt;</title>"));
        assert!(xml.contains("kind=acquisition"));
        assert!(xml.contains("href=\"/opds/comic/covers/abc.png\""));

        let v2_feed = OpdsFeed {
            self_path: "/opds/comic/v2.0".into(),
            ..feed
        };
        let json_res = OpdsVersion::V20.render("/opds/comic", &v2_feed);
        assert_eq!(json_res.status(), StatusCode::OK);
        assert_eq!(
            json_res.headers().get(header::CONTENT_TYPE).unwrap(),
            "application/opds+json; charset=utf-8"
        );
        let body: serde_json::Value =
            serde_json::from_slice(&to_bytes(json_res.into_body(), usize::MAX).await.unwrap())
                .unwrap();
        assert_eq!(
            body["metadata"]["title"],
            "Tom & Jerry <Comics> \"Special\" 'Ed'"
        );
        assert_eq!(body["navigation"][0]["title"], "A & B <Vol.1>");
        assert!(body.get("publications").is_none());
    }

    #[tokio::test]
    async fn render_atom_and_opds2_acquisition_feed_includes_pse_and_download_links() {
        let now = Timestamp::from_second(1_700_000_000).unwrap();
        let chapter_id = ChapterId::new();
        let feed = OpdsFeed {
            id: "urn:xmedia:comic:series:test".into(),
            title: "Witch Hat".into(),
            self_path: "/opds/comic/v1.2/series/test".into(),
            updated_at: now,
            payload: OpdsFeedPayload::Acquisition(vec![OpdsChapterEntry {
                id: chapter_id,
                series_title: "Witch & Hat".into(),
                title: "#1 - <Start>".into(),
                summary: "Intro & pilot".into(),
                authors: vec!["Author <A>".into()],
                cover_url: Some("/opds/comic/covers/hash.jpg".into()),
                page_count: 24,
                last_read_page: Some(7),
                last_read_at: Some(now),
                can_download_archive: true,
                archive_mime: "application/vnd.comicbook+zip",
                updated_at: now,
            }]),
        };

        let atom_res = OpdsVersion::V12.render("/opds/comic", &feed);
        assert_eq!(
            atom_res.headers().get(header::CONTENT_TYPE).unwrap(),
            "application/atom+xml;profile=opds-catalog;kind=acquisition; charset=utf-8"
        );
        let xml = String::from_utf8(
            to_bytes(atom_res.into_body(), usize::MAX)
                .await
                .unwrap()
                .to_vec(),
        )
        .unwrap();
        assert!(xml.contains("Witch &amp; Hat - #1 - &lt;Start&gt;"));
        assert!(xml.contains("<author><name>Author &lt;A&gt;</name></author>"));
        assert!(xml.contains("pse:count=\"24\" pse:lastRead=\"6\""));
        assert!(xml.contains(&format!("/opds/comic/v1.2/chapters/{chapter_id}/download")));

        let json_res = OpdsVersion::V20.render("/opds/comic", &feed);
        let body: serde_json::Value =
            serde_json::from_slice(&to_bytes(json_res.into_body(), usize::MAX).await.unwrap())
                .unwrap();
        assert!(body.get("navigation").is_none());
        let pub0 = &body["publications"][0];
        assert_eq!(pub0["metadata"]["numberOfPages"], 24);
        assert_eq!(pub0["links"][0]["properties"]["pse:lastRead"], 6);
        assert_eq!(pub0["links"][1]["rel"], "http://opds-spec.org/acquisition");
        assert_eq!(pub0["images"][0]["href"], "/opds/comic/covers/hash.jpg");
    }
}

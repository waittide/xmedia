mod render;
#[cfg(test)]
mod tests;

use axum::Router;
use axum::extract::{Path, Query, State};
use axum::http::{HeaderMap, StatusCode, header};
use axum::response::{IntoResponse, Redirect, Response};
use axum::routing::get;
use serde::Deserialize;
use uuid::Uuid;
use xmedia_core::domain::library::LibraryId;
use xmedia_core::domain::user::User;
use xmedia_core::domain::xml::xml_escape;
use xmedia_core::http::{ApiResult, OpdsAuth, into_range_http_response, parse_http_range_spec};

use crate::application::dto::UpdateProgressInput;
use crate::application::opds::OpdsFeed;
use crate::domain::id::{ChapterId, ComicShelfId, SeriesId};
use crate::http::rest::respond_with_etag;
use crate::http::state::ComicHttpState;

#[derive(Debug, Deserialize, Default)]
struct OpdsSearchQuery {
    q: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum OpdsVersion {
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

    pub(super) fn render(self, opds_prefix: &str, feed: &OpdsFeed) -> Response {
        match self {
            Self::V12 => render::render_atom_feed(opds_prefix, feed),
            Self::V20 => render::render_opds2_json(opds_prefix, feed),
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
        .route("/v1.2/opensearch.xml", get(v12_opensearch))
        .route("/v1.2/chapters/{id}/download", get(opds_download_chapter))
        .route(
            "/v1.2/chapters/{id}/pse/{page_index}",
            get(opds_pse_stream_page),
        )
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
        FeedKind::Libraries => opds.libraries_feed(&user, catalog, &vp).await?,
        FeedKind::LibrarySeries(lib_id) => {
            opds.library_series_feed(&user, catalog, lib_id, p, &vp)
                .await?
        }
        FeedKind::Tags => opds.tags_feed(&user, catalog, &vp).await?,
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
    OpdsAuth(user): OpdsAuth,
    headers: HeaderMap,
    Path(key): Path<String>,
) -> ApiResult<Response> {
    let thumb = state.services.catalog.read_cover(&user, &key).await?;
    Ok(respond_with_etag(thumb, &headers))
}

async fn opds_download_chapter(
    State(state): State<ComicHttpState>,
    OpdsAuth(user): OpdsAuth,
    headers: HeaderMap,
    Path(id): Path<Uuid>,
) -> ApiResult<Response> {
    let chapter_id = ChapterId(id);
    state
        .services
        .catalog
        .require_chapter_bound(&user, chapter_id)
        .await?;
    state
        .services
        .online
        .ensure_tracked_chapter_cached(chapter_id)
        .await?;
    let spec = parse_http_range_spec(&headers);
    let (stream, filename, mime) = state
        .services
        .catalog
        .open_chapter_archive_stream(&user, chapter_id, spec)
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
    state
        .services
        .catalog
        .require_chapter_bound(&user, chapter_id)
        .await?;
    state
        .services
        .online
        .ensure_tracked_chapter_cached(chapter_id)
        .await?;
    let page_number = page_index.saturating_add(1);
    let page = state
        .services
        .catalog
        .read_chapter_page(&user, chapter_id, page_number)
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
    let p = xml_escape(state.opds_prefix.as_ref());
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

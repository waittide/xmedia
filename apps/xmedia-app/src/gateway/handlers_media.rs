use axum::Json;
use axum::body::Bytes;
use axum::extract::{Path as AxumPath, Query, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};

use super::StandaloneGatewayState;
use super::dto::{
    OpdsBrowseQuery, OpdsImportRequest, OpdsProxyImageQuery, SubsonicAlbumsQuery,
    SubsonicAlbumsResponse, SubsonicImportRequest, SubsonicSearchQuery, app_error,
    load_source_with_protocol, mark_source_ok, remote_error_response, with_app_headers,
};
use crate::remote_sources::RemoteProtocol;

pub(super) async fn opds_browse(
    State(state): State<StandaloneGatewayState>,
    AxumPath(id): AxumPath<String>,
    Query(query): Query<OpdsBrowseQuery>,
) -> Response {
    let source = match load_source_with_protocol(&state, &id, RemoteProtocol::Opds) {
        Ok(s) => s,
        Err(err) => return err.into_response(),
    };

    match state
        .opds
        .browse_opds(&source, query.url.as_deref(), query.q.as_deref())
        .await
    {
        Ok(feed) => {
            mark_source_ok(&state, &id);
            with_app_headers(Json(feed).into_response())
        }
        Err(err) => remote_error_response("OPDS", err),
    }
}

pub(super) async fn opds_proxy_image(
    State(state): State<StandaloneGatewayState>,
    AxumPath(id): AxumPath<String>,
    Query(query): Query<OpdsProxyImageQuery>,
) -> Response {
    let source = match load_source_with_protocol(&state, &id, RemoteProtocol::Opds) {
        Ok(s) => s,
        Err(err) => return err.into_response(),
    };

    let resolved_url =
        crate::opds_client::expand_pse_template_url(&query.url, query.page, query.max_width);
    match state.opds.proxy_image(&source, &resolved_url).await {
        Ok(res) => res,
        Err(err) => remote_error_response("OPDS", err),
    }
}

pub(super) async fn opds_import(
    State(state): State<StandaloneGatewayState>,
    AxumPath(id): AxumPath<String>,
    Json(input): Json<OpdsImportRequest>,
) -> Response {
    let source = match load_source_with_protocol(&state, &id, RemoteProtocol::Opds) {
        Ok(s) => s,
        Err(err) => return err.into_response(),
    };

    match state
        .opds
        .import_to_local(
            &source,
            &input.url,
            input.filename.as_deref(),
            &state.local_engine,
        )
        .await
    {
        Ok(result) => with_app_headers((StatusCode::CREATED, Json(result)).into_response()),
        Err(err) => remote_error_response("OPDS", err),
    }
}

pub(super) async fn subsonic_list_albums(
    State(state): State<StandaloneGatewayState>,
    AxumPath(id): AxumPath<String>,
    Query(query): Query<SubsonicAlbumsQuery>,
) -> Response {
    let source = match load_source_with_protocol(&state, &id, RemoteProtocol::Subsonic) {
        Ok(s) => s,
        Err(err) => return err.into_response(),
    };

    let list_type = query.list_type.as_deref().unwrap_or("newest");
    let size = query.size.unwrap_or(30);
    let offset = query.offset.unwrap_or(0);

    match state
        .subsonic
        .list_albums(&source, list_type, size, offset)
        .await
    {
        Ok(albums) => {
            mark_source_ok(&state, &id);
            with_app_headers(Json(SubsonicAlbumsResponse { albums }).into_response())
        }
        Err(err) => remote_error_response("Subsonic", err),
    }
}

pub(super) async fn subsonic_get_album(
    State(state): State<StandaloneGatewayState>,
    AxumPath((id, album_id)): AxumPath<(String, String)>,
) -> Response {
    let source = match load_source_with_protocol(&state, &id, RemoteProtocol::Subsonic) {
        Ok(s) => s,
        Err(err) => return err.into_response(),
    };

    match state.subsonic.get_album(&source, &album_id).await {
        Ok(detail) => {
            mark_source_ok(&state, &id);
            with_app_headers(Json(detail).into_response())
        }
        Err(err) => remote_error_response("Subsonic", err),
    }
}

pub(super) async fn subsonic_search(
    State(state): State<StandaloneGatewayState>,
    AxumPath(id): AxumPath<String>,
    Query(query): Query<SubsonicSearchQuery>,
) -> Response {
    let source = match load_source_with_protocol(&state, &id, RemoteProtocol::Subsonic) {
        Ok(s) => s,
        Err(err) => return err.into_response(),
    };

    match state
        .subsonic
        .search(&source, query.q.as_deref().unwrap_or(""))
        .await
    {
        Ok(res) => {
            mark_source_ok(&state, &id);
            with_app_headers(Json(res).into_response())
        }
        Err(err) => remote_error_response("Subsonic", err),
    }
}

pub(super) async fn subsonic_proxy_cover(
    State(state): State<StandaloneGatewayState>,
    AxumPath((id, cover_id)): AxumPath<(String, String)>,
) -> Response {
    let source = match load_source_with_protocol(&state, &id, RemoteProtocol::Subsonic) {
        Ok(s) => s,
        Err(err) => return err.into_response(),
    };

    match state.subsonic.proxy_cover(&source, &cover_id).await {
        Ok(res) => res,
        Err(err) => remote_error_response("Subsonic", err),
    }
}

pub(super) async fn subsonic_proxy_stream(
    State(state): State<StandaloneGatewayState>,
    AxumPath((id, song_id)): AxumPath<(String, String)>,
    method: axum::http::Method,
    headers: HeaderMap,
) -> Response {
    let source = match load_source_with_protocol(&state, &id, RemoteProtocol::Subsonic) {
        Ok(s) => s,
        Err(err) => return err.into_response(),
    };

    match state
        .subsonic
        .proxy_stream(
            &source,
            &song_id,
            &headers,
            method == axum::http::Method::HEAD,
        )
        .await
    {
        Ok(res) => res,
        Err(err) => remote_error_response("Subsonic", err),
    }
}

pub(super) async fn subsonic_import_song(
    State(state): State<StandaloneGatewayState>,
    AxumPath((id, song_id)): AxumPath<(String, String)>,
    body: Bytes,
) -> Response {
    let source = match load_source_with_protocol(&state, &id, RemoteProtocol::Subsonic) {
        Ok(s) => s,
        Err(err) => return err.into_response(),
    };

    let input: SubsonicImportRequest = if body.is_empty() {
        SubsonicImportRequest::default()
    } else {
        match serde_json::from_slice(&body) {
            Ok(v) => v,
            Err(err) => {
                return app_error(
                    StatusCode::BAD_REQUEST,
                    "invalid_json",
                    format!("Invalid import request body: {err}"),
                );
            }
        }
    };

    match state
        .subsonic
        .import_song_to_local(
            &source,
            &song_id,
            input.filename.as_deref(),
            &state.local_engine,
        )
        .await
    {
        Ok(result) => with_app_headers((StatusCode::CREATED, Json(result)).into_response()),
        Err(err) => remote_error_response("Subsonic", err),
    }
}

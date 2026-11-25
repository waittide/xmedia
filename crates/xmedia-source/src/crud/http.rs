use std::sync::Arc;

use axum::body::Body;
use axum::extract::{DefaultBodyLimit, FromRef, Path, State};
use axum::http::header::{CONTENT_TYPE, X_CONTENT_TYPE_OPTIONS};
use axum::http::{HeaderValue, StatusCode};
use axum::response::Response;
use axum::routing::{get, post};
use axum::{Json, Router};
use uuid::Uuid;
use xmedia_core::application::auth::AuthService;
use xmedia_core::http::{ApiResult, WebAuth};

use super::dto::{
    BatchSourcesInput, BatchSourcesReportDto, ExportSourcesBatchRequest, ImportSourcesReportDto,
    ImportSourcesRequest, ReorderSourcesRequest, SourceSummaryDto, SourceVariableResponse,
    UpdateSourceInput,
};
use super::service::{SourceCrudService, SourceSpecAdapter};

/// Maximum JSON request body accepted by `/sources/import` (`32 MiB`).
pub const SOURCE_IMPORT_BODY_LIMIT_BYTES: usize = 32 * 1024 * 1024;

/// Builds a `200 OK` HTTP response containing raw UTF-8 JSON with `nosniff`.
pub fn into_raw_json_export_response(raw_json: String) -> Response {
    let mut resp = Response::new(Body::from(raw_json));
    resp.headers_mut().insert(
        CONTENT_TYPE,
        HeaderValue::from_static("application/json; charset=utf-8"),
    );
    resp.headers_mut()
        .insert(X_CONTENT_TYPE_OPTIONS, HeaderValue::from_static("nosniff"));
    resp
}

/// Reusable Axum router mounted at `/sources` (via `.nest("/sources", ...)` when no extra `/sources/*`
/// routes are registered on the outer router).
pub fn sources_crud_router<S, Spec>() -> Router<S>
where
    S: Clone + Send + Sync + 'static,
    Arc<AuthService>: FromRef<S>,
    Arc<SourceCrudService<Spec>>: FromRef<S>,
    Spec: SourceSpecAdapter,
{
    Router::new()
        .route("/", get(list_sources::<Spec>).post(create_source::<Spec>))
        .route("/reorder", post(reorder_sources::<Spec>))
        .route("/batch", post(batch_sources::<Spec>))
        .route("/export-batch", post(export_sources_batch::<Spec>))
        .route(
            "/import",
            post(import_sources::<Spec>)
                .layer(DefaultBodyLimit::max(SOURCE_IMPORT_BODY_LIMIT_BYTES)),
        )
        .route(
            "/{id}",
            get(get_source::<Spec>)
                .patch(update_source::<Spec>)
                .delete(delete_source::<Spec>),
        )
        .route("/{id}/variable", get(get_source_variable::<Spec>))
        .route("/{id}/export", get(export_source::<Spec>))
}

/// Reusable Axum router with explicit `/sources` route prefixes so domains can `.merge(...)` it
/// directly alongside domain-specific `.route("/sources/{id}/login-action", ...)` and
/// `.route("/sources/{id}/test", ...)` routes without Axum `nest` prefix collisions.
pub fn sources_prefixed_crud_router<S, Spec>() -> Router<S>
where
    S: Clone + Send + Sync + 'static,
    Arc<AuthService>: FromRef<S>,
    Arc<SourceCrudService<Spec>>: FromRef<S>,
    Spec: SourceSpecAdapter,
{
    Router::new()
        .route(
            "/sources",
            get(list_sources::<Spec>).post(create_source::<Spec>),
        )
        .route("/sources/reorder", post(reorder_sources::<Spec>))
        .route("/sources/batch", post(batch_sources::<Spec>))
        .route("/sources/export-batch", post(export_sources_batch::<Spec>))
        .route(
            "/sources/import",
            post(import_sources::<Spec>)
                .layer(DefaultBodyLimit::max(SOURCE_IMPORT_BODY_LIMIT_BYTES)),
        )
        .route(
            "/sources/{id}",
            get(get_source::<Spec>)
                .patch(update_source::<Spec>)
                .delete(delete_source::<Spec>),
        )
        .route("/sources/{id}/variable", get(get_source_variable::<Spec>))
        .route("/sources/{id}/export", get(export_source::<Spec>))
}

async fn list_sources<Spec: SourceSpecAdapter>(
    State(svc): State<Arc<SourceCrudService<Spec>>>,
    WebAuth(user): WebAuth,
) -> ApiResult<Json<Vec<SourceSummaryDto>>> {
    Ok(Json(svc.list_sources(&user).await?))
}

async fn get_source<Spec: SourceSpecAdapter>(
    State(svc): State<Arc<SourceCrudService<Spec>>>,
    WebAuth(user): WebAuth,
    Path(id): Path<Uuid>,
) -> ApiResult<Json<SourceSummaryDto>> {
    Ok(Json(svc.get_source(&user, id).await?))
}

async fn create_source<Spec: SourceSpecAdapter>(
    State(svc): State<Arc<SourceCrudService<Spec>>>,
    WebAuth(user): WebAuth,
    Json(req): Json<ImportSourcesRequest>,
) -> ApiResult<Json<SourceSummaryDto>> {
    Ok(Json(
        svc.create_or_upsert_source(&user, &req.raw_json).await?,
    ))
}

async fn import_sources<Spec: SourceSpecAdapter>(
    State(svc): State<Arc<SourceCrudService<Spec>>>,
    WebAuth(user): WebAuth,
    Json(req): Json<ImportSourcesRequest>,
) -> ApiResult<Json<ImportSourcesReportDto>> {
    Ok(Json(svc.import_sources(&user, &req.raw_json).await?))
}

async fn reorder_sources<Spec: SourceSpecAdapter>(
    State(svc): State<Arc<SourceCrudService<Spec>>>,
    WebAuth(user): WebAuth,
    Json(req): Json<ReorderSourcesRequest>,
) -> ApiResult<Json<Vec<SourceSummaryDto>>> {
    Ok(Json(svc.reorder_sources(&user, req.ordered_ids).await?))
}

async fn batch_sources<Spec: SourceSpecAdapter>(
    State(svc): State<Arc<SourceCrudService<Spec>>>,
    WebAuth(user): WebAuth,
    Json(req): Json<BatchSourcesInput>,
) -> ApiResult<Json<BatchSourcesReportDto>> {
    Ok(Json(svc.batch_sources(&user, req).await?))
}

async fn get_source_variable<Spec: SourceSpecAdapter>(
    State(svc): State<Arc<SourceCrudService<Spec>>>,
    WebAuth(user): WebAuth,
    Path(id): Path<Uuid>,
) -> ApiResult<Json<SourceVariableResponse>> {
    let variable = svc.get_source_variable(&user, id).await?;
    Ok(Json(SourceVariableResponse { variable }))
}

async fn export_sources_batch<Spec: SourceSpecAdapter>(
    State(svc): State<Arc<SourceCrudService<Spec>>>,
    WebAuth(user): WebAuth,
    Json(req): Json<ExportSourcesBatchRequest>,
) -> ApiResult<Response> {
    let raw = svc.export_sources_batch_json(&user, req.ids).await?;
    Ok(into_raw_json_export_response(raw))
}

async fn update_source<Spec: SourceSpecAdapter>(
    State(svc): State<Arc<SourceCrudService<Spec>>>,
    WebAuth(user): WebAuth,
    Path(id): Path<Uuid>,
    Json(req): Json<UpdateSourceInput>,
) -> ApiResult<Json<SourceSummaryDto>> {
    Ok(Json(svc.update_source(&user, id, req).await?))
}

async fn delete_source<Spec: SourceSpecAdapter>(
    State(svc): State<Arc<SourceCrudService<Spec>>>,
    WebAuth(user): WebAuth,
    Path(id): Path<Uuid>,
) -> ApiResult<StatusCode> {
    svc.delete_source(&user, id).await?;
    Ok(StatusCode::NO_CONTENT)
}

async fn export_source<Spec: SourceSpecAdapter>(
    State(svc): State<Arc<SourceCrudService<Spec>>>,
    WebAuth(user): WebAuth,
    Path(id): Path<Uuid>,
) -> ApiResult<Response> {
    let raw = svc.export_source_json(&user, id).await?;
    Ok(into_raw_json_export_response(raw))
}

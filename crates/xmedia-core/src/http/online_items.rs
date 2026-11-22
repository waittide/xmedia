//! Generic REST router for per-user online item favorites/progress. Each media domain nests it
//! under `/{domain}/online/items` with its own [`OnlineLibraryService`].

use std::sync::Arc;

use axum::extract::{FromRef, Path, Query, State};
use axum::http::StatusCode;
use axum::routing::{get, post, put};
use axum::{Json, Router};
use uuid::Uuid;

use crate::application::auth::AuthService;
use crate::application::online_items::{
    LookupOnlineItemsInput, OnlineItemDto, OnlineItemListDto, OnlineItemQuery,
    OnlineLibraryService, PatchOnlineItemInput, UpsertOnlineItemInput,
};
use crate::domain::online_item::OnlineItemProgress;
use crate::http::{ApiResult, WebAuth};

type Svc = State<Arc<OnlineLibraryService>>;

/// Routes (relative to the nest point, e.g. `/api/v1/comic/online/items`):
///
/// * `GET    /`              list (`view`, `source_kind`, `source_id`, `q`, `limit`, `offset`)
/// * `PUT    /`              idempotent upsert by natural key
/// * `POST   /lookup`        batch lookup by natural keys
/// * `GET|PATCH|DELETE /{id}`
/// * `PUT|DELETE /{id}/progress`
pub fn online_items_router<S>() -> Router<S>
where
    S: Clone + Send + Sync + 'static,
    Arc<AuthService>: FromRef<S>,
    Arc<OnlineLibraryService>: FromRef<S>,
{
    Router::new()
        .route("/", get(list_items).put(upsert_item))
        .route("/lookup", post(lookup_items))
        .route("/{id}", get(get_item).patch(patch_item).delete(delete_item))
        .route(
            "/{id}/progress",
            put(update_progress).delete(clear_progress),
        )
}

async fn list_items(
    State(svc): Svc,
    WebAuth(user): WebAuth,
    Query(query): Query<OnlineItemQuery>,
) -> ApiResult<Json<OnlineItemListDto>> {
    Ok(Json(svc.list(user.id, query).await?))
}

async fn upsert_item(
    State(svc): Svc,
    WebAuth(user): WebAuth,
    Json(input): Json<UpsertOnlineItemInput>,
) -> ApiResult<Json<OnlineItemDto>> {
    Ok(Json(svc.upsert(user.id, input).await?))
}

async fn lookup_items(
    State(svc): Svc,
    WebAuth(user): WebAuth,
    Json(input): Json<LookupOnlineItemsInput>,
) -> ApiResult<Json<OnlineItemListDto>> {
    Ok(Json(svc.lookup(user.id, input).await?))
}

async fn get_item(
    State(svc): Svc,
    WebAuth(user): WebAuth,
    Path(id): Path<Uuid>,
) -> ApiResult<Json<OnlineItemDto>> {
    Ok(Json(svc.get(user.id, id).await?))
}

async fn patch_item(
    State(svc): Svc,
    WebAuth(user): WebAuth,
    Path(id): Path<Uuid>,
    Json(input): Json<PatchOnlineItemInput>,
) -> ApiResult<Json<OnlineItemDto>> {
    Ok(Json(svc.patch(user.id, id, input).await?))
}

async fn delete_item(
    State(svc): Svc,
    WebAuth(user): WebAuth,
    Path(id): Path<Uuid>,
) -> ApiResult<StatusCode> {
    svc.delete(user.id, id).await?;
    Ok(StatusCode::NO_CONTENT)
}

async fn update_progress(
    State(svc): Svc,
    WebAuth(user): WebAuth,
    Path(id): Path<Uuid>,
    Json(input): Json<OnlineItemProgress>,
) -> ApiResult<Json<OnlineItemDto>> {
    Ok(Json(svc.update_progress(user.id, id, input).await?))
}

async fn clear_progress(
    State(svc): Svc,
    WebAuth(user): WebAuth,
    Path(id): Path<Uuid>,
) -> ApiResult<Json<OnlineItemDto>> {
    Ok(Json(svc.clear_progress(user.id, id).await?))
}

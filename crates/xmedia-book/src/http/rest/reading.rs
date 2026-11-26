use axum::Json;
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use serde::Deserialize;
use uuid::Uuid;
use xmedia_core::http::{ApiResult, WebAuth};

use crate::application::dto::{BookBookmarkItemDto, BookHomeOverviewDto, BookSummaryDto, ShelfDto};
use crate::application::reading::{CreateBookmarkCommand, SaveProgressCommand};
use crate::domain::book::BookId;
use crate::domain::reading::{Bookmark, BookmarkId, ReadingProgress, ShelfId};
use crate::http::state::BookHttpState;

#[derive(Default, Deserialize)]
pub(super) struct LimitQuery {
    limit: Option<u32>,
}

pub(super) async fn home_overview(
    State(state): State<BookHttpState>,
    WebAuth(actor): WebAuth,
) -> ApiResult<Json<BookHomeOverviewDto>> {
    Ok(Json(state.services.reading.home_overview(&actor).await?))
}

pub(super) async fn list_recent_books(
    State(state): State<BookHttpState>,
    WebAuth(actor): WebAuth,
    Query(q): Query<LimitQuery>,
) -> ApiResult<Json<Vec<BookSummaryDto>>> {
    let items = state
        .services
        .reading
        .list_recent_books(&actor, q.limit.unwrap_or(20))
        .await?;
    Ok(Json(items))
}

pub(super) async fn get_progress(
    State(state): State<BookHttpState>,
    WebAuth(actor): WebAuth,
    Path(id): Path<Uuid>,
) -> ApiResult<Json<Option<ReadingProgress>>> {
    Ok(Json(
        state
            .services
            .reading
            .get_progress(&actor, BookId(id))
            .await?,
    ))
}

pub(super) async fn save_progress(
    State(state): State<BookHttpState>,
    WebAuth(actor): WebAuth,
    Path(id): Path<Uuid>,
    Json(cmd): Json<SaveProgressCommand>,
) -> ApiResult<Json<ReadingProgress>> {
    Ok(Json(
        state
            .services
            .reading
            .save_progress(&actor, BookId(id), cmd)
            .await?,
    ))
}

#[derive(Deserialize)]
pub(super) struct SetFavoriteRequest {
    favorite: bool,
}

pub(super) async fn set_favorite(
    State(state): State<BookHttpState>,
    WebAuth(actor): WebAuth,
    Path(id): Path<Uuid>,
    Json(req): Json<SetFavoriteRequest>,
) -> ApiResult<StatusCode> {
    state
        .services
        .reading
        .set_favorite(&actor, BookId(id), req.favorite)
        .await?;
    Ok(StatusCode::NO_CONTENT)
}

pub(super) async fn list_bookmarks(
    State(state): State<BookHttpState>,
    WebAuth(actor): WebAuth,
    Path(id): Path<Uuid>,
) -> ApiResult<Json<Vec<Bookmark>>> {
    Ok(Json(
        state
            .services
            .reading
            .list_bookmarks(&actor, BookId(id))
            .await?,
    ))
}

pub(super) async fn list_all_bookmarks(
    State(state): State<BookHttpState>,
    WebAuth(actor): WebAuth,
) -> ApiResult<Json<Vec<BookBookmarkItemDto>>> {
    Ok(Json(
        state.services.reading.list_all_bookmarks(&actor).await?,
    ))
}

pub(super) async fn create_bookmark(
    State(state): State<BookHttpState>,
    WebAuth(actor): WebAuth,
    Path(id): Path<Uuid>,
    Json(cmd): Json<CreateBookmarkCommand>,
) -> ApiResult<(StatusCode, Json<Bookmark>)> {
    let bm = state
        .services
        .reading
        .create_bookmark(&actor, BookId(id), cmd)
        .await?;
    Ok((StatusCode::CREATED, Json(bm)))
}

pub(super) async fn delete_bookmark(
    State(state): State<BookHttpState>,
    WebAuth(actor): WebAuth,
    Path(bookmark_id): Path<Uuid>,
) -> ApiResult<StatusCode> {
    state
        .services
        .reading
        .delete_bookmark(&actor, BookmarkId(bookmark_id))
        .await?;
    Ok(StatusCode::NO_CONTENT)
}

pub(super) async fn list_shelves(
    State(state): State<BookHttpState>,
    WebAuth(actor): WebAuth,
) -> ApiResult<Json<Vec<ShelfDto>>> {
    Ok(Json(state.services.reading.list_shelves(&actor).await?))
}

#[derive(Deserialize)]
pub(super) struct ShelfNameRequest {
    name: String,
}

pub(super) async fn create_shelf(
    State(state): State<BookHttpState>,
    WebAuth(actor): WebAuth,
    Json(req): Json<ShelfNameRequest>,
) -> ApiResult<(StatusCode, Json<ShelfDto>)> {
    let shelf = state
        .services
        .reading
        .create_shelf(&actor, req.name)
        .await?;
    Ok((StatusCode::CREATED, Json(shelf)))
}

pub(super) async fn rename_shelf(
    State(state): State<BookHttpState>,
    WebAuth(actor): WebAuth,
    Path(shelf_id): Path<Uuid>,
    Json(req): Json<ShelfNameRequest>,
) -> ApiResult<Json<ShelfDto>> {
    Ok(Json(
        state
            .services
            .reading
            .rename_shelf(&actor, ShelfId(shelf_id), req.name)
            .await?,
    ))
}

pub(super) async fn delete_shelf(
    State(state): State<BookHttpState>,
    WebAuth(actor): WebAuth,
    Path(shelf_id): Path<Uuid>,
) -> ApiResult<StatusCode> {
    state
        .services
        .reading
        .delete_shelf(&actor, ShelfId(shelf_id))
        .await?;
    Ok(StatusCode::NO_CONTENT)
}

pub(super) async fn add_book_to_shelf(
    State(state): State<BookHttpState>,
    WebAuth(actor): WebAuth,
    Path((shelf_id, book_id)): Path<(Uuid, Uuid)>,
) -> ApiResult<Json<ShelfDto>> {
    Ok(Json(
        state
            .services
            .reading
            .add_book_to_shelf(&actor, ShelfId(shelf_id), BookId(book_id))
            .await?,
    ))
}

pub(super) async fn remove_book_from_shelf(
    State(state): State<BookHttpState>,
    WebAuth(actor): WebAuth,
    Path((shelf_id, book_id)): Path<(Uuid, Uuid)>,
) -> ApiResult<Json<ShelfDto>> {
    Ok(Json(
        state
            .services
            .reading
            .remove_book_from_shelf(&actor, ShelfId(shelf_id), BookId(book_id))
            .await?,
    ))
}

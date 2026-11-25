use axum::extract::{Query, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use serde::{Deserialize, Serialize};
use uuid::Uuid;
use xmedia_core::http::ApiResult;

use crate::domain::media::MediaFileDetail;
use crate::domain::ops::{
    BatchComicSeriesSyncInput, BatchComicSeriesSyncReportDto, ComicImageReencodeFormat,
    ComicOptimizeReportDto, ComicPagesSnapshotDto, MergeComicsReportDto, SaveComicPageInput,
};
use crate::domain::workspace::RootId;
use crate::http::ToolHttpState;
use crate::http::routes::{FileTargetQuery, apply_sandboxed_asset_headers, default_jpeg_quality};

pub(super) fn router() -> Router<ToolHttpState> {
    Router::new()
        .route("/comics/pages", get(get_comic_pages).put(put_comic_pages))
        .route("/comics/page-image", get(get_comic_page_image))
        .route("/comics/optimize", post(optimize_comic))
        .route("/comics/merge", post(merge_comics))
        .route("/comics/batch-sync", post(batch_sync_comics))
}

#[derive(Deserialize)]
struct ComicPageImageQuery {
    root_id: Uuid,
    path: String,
    entry: String,
    max_width: Option<u32>,
}

async fn get_comic_pages(
    State(state): State<ToolHttpState>,
    Query(q): Query<FileTargetQuery>,
) -> ApiResult<Json<ComicPagesSnapshotDto>> {
    let snap = state
        .workbench
        .get_comic_pages(RootId(q.root_id), &q.path)
        .await?;
    Ok(Json(snap))
}

async fn get_comic_page_image(
    State(state): State<ToolHttpState>,
    Query(q): Query<ComicPageImageQuery>,
) -> ApiResult<Response> {
    let payload = state
        .workbench
        .read_comic_page(RootId(q.root_id), &q.path, &q.entry, q.max_width)
        .await?;
    let mut resp = (StatusCode::OK, payload.bytes).into_response();
    apply_sandboxed_asset_headers(
        resp.headers_mut(),
        &payload.mime_type,
        "private, max-age=120",
    );
    Ok(resp)
}

#[derive(Deserialize)]
struct SaveComicPagesRequest {
    root_id: Uuid,
    path: String,
    pages: Vec<SaveComicPageInput>,
    #[serde(default)]
    reencode: ComicImageReencodeFormat,
    #[serde(default = "default_jpeg_quality")]
    jpeg_quality: u8,
    max_width: Option<u32>,
}

#[derive(Serialize)]
struct ComicOptimizeResponse {
    report: ComicOptimizeReportDto,
    detail: MediaFileDetail,
}

async fn put_comic_pages(
    State(state): State<ToolHttpState>,
    Json(req): Json<SaveComicPagesRequest>,
) -> ApiResult<Json<ComicOptimizeResponse>> {
    let (report, detail) = state
        .workbench
        .save_comic_pages(
            RootId(req.root_id),
            &req.path,
            req.pages,
            req.reencode,
            req.jpeg_quality,
            req.max_width,
        )
        .await?;
    Ok(Json(ComicOptimizeResponse { report, detail }))
}

#[derive(Deserialize)]
struct OptimizeComicRequest {
    root_id: Uuid,
    path: String,
    #[serde(default)]
    reencode: ComicImageReencodeFormat,
    #[serde(default = "default_jpeg_quality")]
    jpeg_quality: u8,
    max_width: Option<u32>,
}

async fn optimize_comic(
    State(state): State<ToolHttpState>,
    Json(req): Json<OptimizeComicRequest>,
) -> ApiResult<Json<ComicOptimizeResponse>> {
    let (report, detail) = state
        .workbench
        .optimize_comic(
            RootId(req.root_id),
            &req.path,
            req.reencode,
            req.jpeg_quality,
            req.max_width,
        )
        .await?;
    Ok(Json(ComicOptimizeResponse { report, detail }))
}

#[derive(Deserialize)]
struct MergeComicsRequest {
    root_id: Uuid,
    source_paths: Vec<String>,
    output_rel_path: String,
    series: Option<String>,
    title: Option<String>,
    volume: Option<f64>,
}

#[derive(Serialize)]
struct MergeComicsResponse {
    report: MergeComicsReportDto,
    detail: MediaFileDetail,
}

async fn merge_comics(
    State(state): State<ToolHttpState>,
    Json(req): Json<MergeComicsRequest>,
) -> ApiResult<Json<MergeComicsResponse>> {
    let (report, detail) = state
        .workbench
        .merge_comics(
            RootId(req.root_id),
            req.source_paths,
            &req.output_rel_path,
            req.series,
            req.title,
            req.volume,
        )
        .await?;
    Ok(Json(MergeComicsResponse { report, detail }))
}

#[derive(Deserialize)]
struct BatchSyncComicsRequest {
    root_id: Uuid,
    paths: Vec<String>,
    #[serde(flatten)]
    input: BatchComicSeriesSyncInput,
}

async fn batch_sync_comics(
    State(state): State<ToolHttpState>,
    Json(req): Json<BatchSyncComicsRequest>,
) -> ApiResult<Json<BatchComicSeriesSyncReportDto>> {
    let report = state
        .workbench
        .batch_sync_comic_series(RootId(req.root_id), req.paths, req.input)
        .await?;
    Ok(Json(report))
}

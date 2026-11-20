use axum::extract::State;
use axum::routing::post;
use axum::{Json, Router};
use serde::{Deserialize, Serialize};
use uuid::Uuid;
use xmedia_core::http::ApiResult;

use crate::domain::media::MediaFileDetail;
use crate::domain::workspace::RootId;
use crate::http::ToolHttpState;
use crate::http::routes::default_jpeg_quality;
use crate::infra::inspectors::music_ops::{
    BatchMusicAlbumSyncInput, BatchMusicAlbumSyncReportDto, CueSplitReportDto,
    MusicCoverOptimizeReportDto, MusicLyricsToolInput, MusicLyricsToolReportDto,
    ReplayGainReportDto,
};

pub(super) fn router() -> Router<ToolHttpState> {
    Router::new()
        .route("/music/lyrics-tool", post(run_music_lyrics_tool))
        .route("/music/optimize-cover", post(optimize_music_cover_route))
        .route("/music/replaygain", post(apply_music_replaygain))
        .route("/music/split-cue", post(split_music_cue))
        .route("/music/batch-sync", post(batch_sync_music_album))
}

#[derive(Deserialize)]
struct MusicLyricsToolRequest {
    root_id: Uuid,
    path: String,
    #[serde(flatten)]
    input: MusicLyricsToolInput,
}

#[derive(Serialize)]
struct MusicLyricsToolResponse {
    report: MusicLyricsToolReportDto,
    detail: MediaFileDetail,
}

async fn run_music_lyrics_tool(
    State(state): State<ToolHttpState>,
    Json(req): Json<MusicLyricsToolRequest>,
) -> ApiResult<Json<MusicLyricsToolResponse>> {
    let (report, detail) = state
        .workbench
        .run_music_lyrics_tool(RootId(req.root_id), &req.path, req.input)
        .await?;
    Ok(Json(MusicLyricsToolResponse { report, detail }))
}

#[derive(Deserialize)]
struct OptimizeMusicCoverRequest {
    root_id: Uuid,
    path: String,
    #[serde(default = "default_cover_max_dim")]
    max_dimension: u32,
    #[serde(default = "default_jpeg_quality")]
    jpeg_quality: u8,
}

fn default_cover_max_dim() -> u32 {
    1000
}

#[derive(Serialize)]
struct OptimizeMusicCoverResponse {
    report: MusicCoverOptimizeReportDto,
    detail: MediaFileDetail,
}

async fn optimize_music_cover_route(
    State(state): State<ToolHttpState>,
    Json(req): Json<OptimizeMusicCoverRequest>,
) -> ApiResult<Json<OptimizeMusicCoverResponse>> {
    let (report, detail) = state
        .workbench
        .optimize_music_cover(
            RootId(req.root_id),
            &req.path,
            req.max_dimension,
            req.jpeg_quality,
        )
        .await?;
    Ok(Json(OptimizeMusicCoverResponse { report, detail }))
}

#[derive(Deserialize)]
struct ReplayGainRequest {
    root_id: Uuid,
    paths: Vec<String>,
}

async fn apply_music_replaygain(
    State(state): State<ToolHttpState>,
    Json(req): Json<ReplayGainRequest>,
) -> ApiResult<Json<ReplayGainReportDto>> {
    let report = state
        .workbench
        .apply_music_replaygain(RootId(req.root_id), req.paths)
        .await?;
    Ok(Json(report))
}

#[derive(Deserialize)]
struct SplitCueRequest {
    root_id: Uuid,
    path: String,
}

async fn split_music_cue(
    State(state): State<ToolHttpState>,
    Json(req): Json<SplitCueRequest>,
) -> ApiResult<Json<CueSplitReportDto>> {
    let report = state
        .workbench
        .split_music_cue(RootId(req.root_id), &req.path)
        .await?;
    Ok(Json(report))
}

#[derive(Deserialize)]
struct BatchSyncMusicAlbumRequest {
    root_id: Uuid,
    paths: Vec<String>,
    #[serde(flatten)]
    input: BatchMusicAlbumSyncInput,
}

async fn batch_sync_music_album(
    State(state): State<ToolHttpState>,
    Json(req): Json<BatchSyncMusicAlbumRequest>,
) -> ApiResult<Json<BatchMusicAlbumSyncReportDto>> {
    let report = state
        .workbench
        .batch_sync_music_album(RootId(req.root_id), req.paths, req.input)
        .await?;
    Ok(Json(report))
}

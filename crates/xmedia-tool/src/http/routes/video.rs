use axum::extract::{Query, State};
use axum::routing::{get, post};
use axum::{Json, Router};
use serde::{Deserialize, Serialize};
use uuid::Uuid;
use xmedia_core::http::ApiResult;

use crate::domain::media::MediaFileDetail;
use crate::domain::workspace::RootId;
use crate::http::ToolHttpState;
use crate::http::routes::FileTargetQuery;
use crate::infra::inspectors::video_ops::{
    BatchRemapEpisodesInput, BatchRemapEpisodesReportDto, ExtractSubtitleReportDto,
    ProcessSubtitleInput, ProcessSubtitleReportDto, VideoSubtitleInfoDto,
};

pub(super) fn router() -> Router<ToolHttpState> {
    Router::new()
        .route("/video/subtitles", get(list_video_subtitles_route))
        .route(
            "/video/subtitles/process",
            post(process_video_subtitle_route),
        )
        .route(
            "/video/subtitles/extract",
            post(extract_video_subtitle_route),
        )
        .route(
            "/video/episodes/batch-remap",
            post(batch_remap_episodes_route),
        )
}

async fn list_video_subtitles_route(
    State(state): State<ToolHttpState>,
    Query(q): Query<FileTargetQuery>,
) -> ApiResult<Json<Vec<VideoSubtitleInfoDto>>> {
    let items = state
        .workbench
        .list_video_subtitles(RootId(q.root_id), &q.path)
        .await?;
    Ok(Json(items))
}

#[derive(Deserialize)]
struct ProcessVideoSubtitleRequest {
    root_id: Uuid,
    path: String,
    #[serde(flatten)]
    input: ProcessSubtitleInput,
}

#[derive(Serialize)]
struct ProcessVideoSubtitleResponse {
    report: ProcessSubtitleReportDto,
    detail: MediaFileDetail,
}

async fn process_video_subtitle_route(
    State(state): State<ToolHttpState>,
    Json(req): Json<ProcessVideoSubtitleRequest>,
) -> ApiResult<Json<ProcessVideoSubtitleResponse>> {
    let (report, detail) = state
        .workbench
        .process_video_subtitle(RootId(req.root_id), &req.path, req.input)
        .await?;
    Ok(Json(ProcessVideoSubtitleResponse { report, detail }))
}

#[derive(Deserialize)]
struct ExtractVideoSubtitleRequest {
    root_id: Uuid,
    path: String,
    stream_index: u32,
    language_tag: Option<String>,
    #[serde(default = "default_sub_ext")]
    output_ext: String,
}

fn default_sub_ext() -> String {
    "srt".to_string()
}

#[derive(Serialize)]
struct ExtractVideoSubtitleResponse {
    report: ExtractSubtitleReportDto,
    detail: MediaFileDetail,
}

async fn extract_video_subtitle_route(
    State(state): State<ToolHttpState>,
    Json(req): Json<ExtractVideoSubtitleRequest>,
) -> ApiResult<Json<ExtractVideoSubtitleResponse>> {
    let (report, detail) = state
        .workbench
        .extract_video_embedded_subtitle(
            RootId(req.root_id),
            &req.path,
            req.stream_index,
            req.language_tag,
            req.output_ext,
        )
        .await?;
    Ok(Json(ExtractVideoSubtitleResponse { report, detail }))
}

#[derive(Deserialize)]
struct BatchRemapEpisodesRequest {
    root_id: Uuid,
    paths: Vec<String>,
    #[serde(flatten)]
    input: BatchRemapEpisodesInput,
}

async fn batch_remap_episodes_route(
    State(state): State<ToolHttpState>,
    Json(req): Json<BatchRemapEpisodesRequest>,
) -> ApiResult<Json<BatchRemapEpisodesReportDto>> {
    let report = state
        .workbench
        .batch_remap_video_episodes(RootId(req.root_id), req.paths, req.input)
        .await?;
    Ok(Json(report))
}

use axum::extract::{Query, State};
use axum::routing::{get, post};
use axum::{Json, Router};
use serde::{Deserialize, Serialize};
use uuid::Uuid;
use xmedia_core::http::ApiResult;

use crate::application::ops::{
    BatchRemapEpisodesInput, ExtractSubtitleReportDto, ProcessSubtitleInput,
    ProcessSubtitleReportDto, VideoSubtitleInfoDto,
};
use crate::application::workbench::{
    BatchSyncReportDto, BatchSyncSpec, VideoWorkbenchOp, VideoWorkbenchResult,
};
use crate::domain::media::MediaFileDetail;
use crate::domain::workspace::RootId;
use crate::http::ToolHttpState;
use crate::http::routes::FileTargetQuery;

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
    let VideoWorkbenchResult::Subtitles(items) = state
        .tools
        .run_video(VideoWorkbenchOp::ListSubtitles {
            root_id: RootId(q.root_id),
            rel_path: q.path,
        })
        .await?
    else {
        unreachable!()
    };
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
    let VideoWorkbenchResult::ProcessedSubtitle(report, detail) = state
        .tools
        .run_video(VideoWorkbenchOp::ProcessSubtitle {
            root_id: RootId(req.root_id),
            rel_path: req.path,
            input: req.input,
        })
        .await?
    else {
        unreachable!()
    };
    Ok(Json(ProcessVideoSubtitleResponse {
        report,
        detail: *detail,
    }))
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
    let VideoWorkbenchResult::ExtractedSubtitle(report, detail) = state
        .tools
        .run_video(VideoWorkbenchOp::ExtractSubtitle {
            root_id: RootId(req.root_id),
            rel_path: req.path,
            stream_index: req.stream_index,
            language_tag: req.language_tag,
            output_ext: req.output_ext,
        })
        .await?
    else {
        unreachable!()
    };
    Ok(Json(ExtractVideoSubtitleResponse {
        report,
        detail: *detail,
    }))
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
) -> ApiResult<Json<BatchSyncReportDto>> {
    let report = state
        .metadata
        .batch_sync(
            RootId(req.root_id),
            req.paths,
            BatchSyncSpec::VideoEpisodes(req.input),
        )
        .await?;
    Ok(Json(report))
}

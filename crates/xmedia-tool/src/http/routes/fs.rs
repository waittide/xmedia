use axum::body::Body;
use axum::extract::{DefaultBodyLimit, Multipart, Query, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::routing::{get, patch, post};
use axum::{Json, Router};
use serde::{Deserialize, Serialize};
use uuid::Uuid;
use xmedia_core::domain::error::{DomainError, DomainResult};
use xmedia_core::domain::library::MediaKind;
use xmedia_core::http::{ApiError, ApiResult};
use xmedia_core::infra::thumbnail::sniff_raster_image_mime;

use crate::application::batch_metadata::{
    BatchMetadataEditInput, BatchMetadataEditReportDto, MetadataHistoryBatchDto,
    UndoMetadataReportDto,
};
use crate::application::ports::ArtworkIo;
use crate::application::workbench::{
    PreviewRenameOptions, RenameHistoryBatchDto, UndoRenameReportDto, WorkspaceArtworkOutput,
};
use crate::domain::media::{
    DeleteFilesReportDto, DirectorySnapshot, MediaFileDetail, MediaMetadataPatch,
    RenameConflictPolicy, RenamePlanItem,
};
use crate::domain::workspace::RootId;
use crate::http::ToolHttpState;
use crate::http::routes::{FileTargetQuery, apply_sandboxed_asset_headers, spawn_ndjson_stream};

pub(super) fn router() -> Router<ToolHttpState> {
    Router::new()
        .route("/fs/browse", get(browse_fs))
        .route("/fs/browse/stream", get(browse_fs_stream))
        .route("/files/inspect", get(inspect_file))
        .route("/files/metadata", patch(write_metadata))
        .route("/files/metadata/batch", post(batch_edit_metadata_route))
        .route("/files/metadata/history", get(list_metadata_history_route))
        .route("/files/metadata/undo", post(undo_metadata_batch_route))
        .route(
            "/files/artwork",
            get(get_artwork)
                .post(upload_artwork)
                .layer(DefaultBodyLimit::max(32 * 1024 * 1024)),
        )
        .route("/files/artwork/from-url", post(upload_artwork_from_url))
        .route("/files/rename/preview", post(preview_rename))
        .route("/files/rename/preview/stream", post(preview_rename_stream))
        .route("/files/rename/execute", post(execute_rename))
        .route("/files/rename/execute/stream", post(execute_rename_stream))
        .route("/files/rename/history", get(list_rename_history))
        .route("/files/rename/undo", post(undo_rename_batch))
        .route("/files/delete", post(delete_files_route))
}

#[derive(Deserialize)]
struct BrowseQuery {
    root_id: Uuid,
    #[serde(default)]
    path: String,
    kind: Option<MediaKind>,
    #[serde(default)]
    recursive: bool,
}

async fn browse_fs(
    State(state): State<ToolHttpState>,
    Query(q): Query<BrowseQuery>,
) -> ApiResult<Json<DirectorySnapshot>> {
    let snap = state
        .workspace
        .browse(RootId(q.root_id), &q.path, q.kind, q.recursive, None)
        .await?;
    Ok(Json(snap))
}

async fn browse_fs_stream(
    State(state): State<ToolHttpState>,
    Query(q): Query<BrowseQuery>,
) -> Response {
    spawn_ndjson_stream(move |prog_tx| async move {
        state
            .workspace
            .browse(
                RootId(q.root_id),
                &q.path,
                q.kind,
                q.recursive,
                Some(prog_tx),
            )
            .await
    })
}

async fn inspect_file(
    State(state): State<ToolHttpState>,
    Query(q): Query<FileTargetQuery>,
) -> ApiResult<Json<MediaFileDetail>> {
    let detail = state
        .workspace
        .inspect(RootId(q.root_id), &q.path, q.kind)
        .await?;
    Ok(Json(detail))
}

#[derive(Deserialize)]
struct WriteMetadataRequest {
    root_id: Uuid,
    path: String,
    #[serde(flatten)]
    patch: MediaMetadataPatch,
}

async fn write_metadata(
    State(state): State<ToolHttpState>,
    Json(req): Json<WriteMetadataRequest>,
) -> ApiResult<Json<MediaFileDetail>> {
    let detail = state
        .metadata
        .write(RootId(req.root_id), &req.path, req.patch)
        .await?;
    Ok(Json(detail))
}

#[derive(Deserialize)]
struct BatchMetadataEditRequest {
    root_id: Uuid,
    #[serde(flatten)]
    input: BatchMetadataEditInput,
}

async fn batch_edit_metadata_route(
    State(state): State<ToolHttpState>,
    Json(req): Json<BatchMetadataEditRequest>,
) -> ApiResult<Json<BatchMetadataEditReportDto>> {
    let report = state
        .metadata
        .batch_edit(RootId(req.root_id), req.input)
        .await?;
    Ok(Json(report))
}

#[derive(Deserialize)]
struct HistoryQuery {
    root_id: Option<Uuid>,
}

#[derive(Deserialize)]
struct UndoBatchRequest {
    batch_id: Uuid,
}

async fn list_metadata_history_route(
    State(state): State<ToolHttpState>,
    Query(q): Query<HistoryQuery>,
) -> ApiResult<Json<Vec<MetadataHistoryBatchDto>>> {
    let items = state.metadata.list_history(q.root_id.map(RootId)).await;
    Ok(Json(items))
}

async fn undo_metadata_batch_route(
    State(state): State<ToolHttpState>,
    Json(req): Json<UndoBatchRequest>,
) -> ApiResult<Json<UndoMetadataReportDto>> {
    let report = state.metadata.undo_batch(req.batch_id).await?;
    Ok(Json(report))
}

async fn get_artwork(
    State(state): State<ToolHttpState>,
    Query(q): Query<FileTargetQuery>,
) -> ApiResult<Response> {
    let WorkspaceArtworkOutput::Read(Some(payload)) = state
        .workspace
        .artwork(
            RootId(q.root_id),
            &q.path,
            q.kind,
            ArtworkIo::Read {
                slot: q.slot.as_deref(),
            },
        )
        .await?
    else {
        return Err(ApiError(DomainError::NotFound(
            "no cover/poster artwork present".into(),
        )));
    };
    let mut res = (StatusCode::OK, Body::from(payload.bytes)).into_response();
    apply_sandboxed_asset_headers(res.headers_mut(), &payload.mime_type, "no-cache");
    Ok(res)
}

fn detect_uploaded_artwork_mime(bytes: &[u8], _client_mime: Option<&str>) -> DomainResult<String> {
    let sniffed = sniff_raster_image_mime(bytes).ok_or_else(|| {
        DomainError::Validation(
            "unsupported artwork format; only raster images (PNG, JPEG, WebP, GIF, BMP, ICO, AVIF) are allowed".into(),
        )
    })?;
    Ok(sniffed.to_string())
}

async fn upload_artwork(
    State(state): State<ToolHttpState>,
    mut multipart: Multipart,
) -> ApiResult<Json<MediaFileDetail>> {
    let mut root_id: Option<Uuid> = None;
    let mut rel_path: Option<String> = None;
    let mut slot = "cover".to_string();
    let mut preferred_kind: Option<MediaKind> = None;
    let mut file_bytes: Option<Vec<u8>> = None;
    let mut file_mime: Option<String> = None;

    while let Some(field) = multipart
        .next_field()
        .await
        .map_err(|e| ApiError(DomainError::Validation(format!("multipart error: {e}"))))?
    {
        let name = field.name().unwrap_or_default().to_string();
        match name.as_str() {
            "root_id" => {
                let text = field.text().await.map_err(|e| {
                    ApiError(DomainError::Validation(format!(
                        "multipart text error: {e}"
                    )))
                })?;
                root_id = Uuid::parse_str(text.trim()).ok();
            }
            "path" => {
                let text = field.text().await.map_err(|e| {
                    ApiError(DomainError::Validation(format!(
                        "multipart text error: {e}"
                    )))
                })?;
                rel_path = Some(text);
            }
            "slot" => {
                let s = field.text().await.map_err(|e| {
                    ApiError(DomainError::Validation(format!(
                        "multipart text error: {e}"
                    )))
                })?;
                if !s.trim().is_empty() {
                    slot = s.trim().to_string();
                }
            }
            "kind" => {
                let k = field.text().await.map_err(|e| {
                    ApiError(DomainError::Validation(format!(
                        "multipart text error: {e}"
                    )))
                })?;
                preferred_kind = serde_json::from_value(serde_json::Value::String(k)).ok();
            }
            "file" => {
                file_mime = field.content_type().map(|s| s.to_string());
                let raw = field.bytes().await.map_err(|e| {
                    ApiError(DomainError::Validation(format!("upload read error: {e}")))
                })?;
                file_bytes = Some(raw.to_vec());
            }
            _ => {}
        }
    }

    let root_uuid =
        root_id.ok_or_else(|| ApiError(DomainError::Validation("missing root_id".into())))?;
    let path_str =
        rel_path.ok_or_else(|| ApiError(DomainError::Validation("missing path".into())))?;
    let bytes = file_bytes
        .filter(|b| !b.is_empty())
        .ok_or_else(|| ApiError(DomainError::Validation("missing image file".into())))?;

    let mime = detect_uploaded_artwork_mime(&bytes, file_mime.as_deref())?;

    let WorkspaceArtworkOutput::Written(detail) = state
        .workspace
        .artwork(
            RootId(root_uuid),
            &path_str,
            preferred_kind,
            ArtworkIo::Write {
                slot: &slot,
                bytes: &bytes,
                mime_type: &mime,
            },
        )
        .await?
    else {
        unreachable!()
    };
    Ok(Json(*detail))
}

#[derive(Deserialize)]
struct UploadArtworkFromUrlRequest {
    root_id: Uuid,
    path: String,
    url: String,
    #[serde(default = "default_artwork_slot")]
    slot: String,
    #[serde(default)]
    kind: Option<MediaKind>,
}

fn default_artwork_slot() -> String {
    "cover".to_string()
}

async fn upload_artwork_from_url(
    State(state): State<ToolHttpState>,
    Json(req): Json<UploadArtworkFromUrlRequest>,
) -> ApiResult<Json<MediaFileDetail>> {
    let detail = state
        .scrape_apply
        .apply_remote_artwork(
            RootId(req.root_id),
            &req.path,
            &req.url,
            &req.slot,
            req.kind,
        )
        .await?;
    Ok(Json(detail))
}

#[derive(Deserialize)]
struct PreviewRenameRequest {
    root_id: Uuid,
    paths: Vec<String>,
    template: Option<String>,
    episode_template: Option<String>,
    kind: Option<MediaKind>,
    #[serde(default)]
    conflict_policy: RenameConflictPolicy,
}

async fn preview_rename(
    State(state): State<ToolHttpState>,
    Json(req): Json<PreviewRenameRequest>,
) -> ApiResult<Json<Vec<RenamePlanItem>>> {
    let plans = state
        .rename
        .preview(
            RootId(req.root_id),
            &req.paths,
            PreviewRenameOptions {
                template_override: req.template.as_deref(),
                episode_template_override: req.episode_template.as_deref(),
                preferred_kind: req.kind,
                conflict_policy: req.conflict_policy,
            },
            None,
        )
        .await?;
    Ok(Json(plans))
}

async fn preview_rename_stream(
    State(state): State<ToolHttpState>,
    Json(req): Json<PreviewRenameRequest>,
) -> Response {
    spawn_ndjson_stream(move |prog_tx| async move {
        state
            .rename
            .preview(
                RootId(req.root_id),
                &req.paths,
                PreviewRenameOptions {
                    template_override: req.template.as_deref(),
                    episode_template_override: req.episode_template.as_deref(),
                    preferred_kind: req.kind,
                    conflict_policy: req.conflict_policy,
                },
                Some(prog_tx),
            )
            .await
    })
}

#[derive(Deserialize)]
struct ExecuteRenameRequest {
    root_id: Uuid,
    plans: Vec<RenamePlanItem>,
}

#[derive(Serialize)]
struct ExecuteRenameResponse {
    moved_count: usize,
}

async fn execute_rename(
    State(state): State<ToolHttpState>,
    Json(req): Json<ExecuteRenameRequest>,
) -> ApiResult<Json<ExecuteRenameResponse>> {
    let moved_count = state
        .rename
        .execute(RootId(req.root_id), &req.plans, None)
        .await?;
    Ok(Json(ExecuteRenameResponse { moved_count }))
}

async fn execute_rename_stream(
    State(state): State<ToolHttpState>,
    Json(req): Json<ExecuteRenameRequest>,
) -> Response {
    spawn_ndjson_stream(move |prog_tx| async move {
        let moved_count = state
            .rename
            .execute(RootId(req.root_id), &req.plans, Some(prog_tx))
            .await?;
        Ok(ExecuteRenameResponse { moved_count })
    })
}

async fn list_rename_history(
    State(state): State<ToolHttpState>,
    Query(q): Query<HistoryQuery>,
) -> ApiResult<Json<Vec<RenameHistoryBatchDto>>> {
    let items = state.rename.list_history(q.root_id.map(RootId)).await;
    Ok(Json(items))
}

async fn undo_rename_batch(
    State(state): State<ToolHttpState>,
    Json(req): Json<UndoBatchRequest>,
) -> ApiResult<Json<UndoRenameReportDto>> {
    let report = state.rename.undo_batch(req.batch_id).await?;
    Ok(Json(report))
}

#[derive(Deserialize)]
struct DeleteFilesRequest {
    root_id: Uuid,
    paths: Vec<String>,
    #[serde(default)]
    delete_companions: bool,
}

async fn delete_files_route(
    State(state): State<ToolHttpState>,
    Json(req): Json<DeleteFilesRequest>,
) -> ApiResult<Json<DeleteFilesReportDto>> {
    let report = state
        .workspace
        .delete(RootId(req.root_id), &req.paths, req.delete_companions)
        .await?;
    Ok(Json(report))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detect_uploaded_artwork_mime_trusts_magic_bytes_and_rejects_svg_or_unknown() {
        let png = [0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A];
        assert_eq!(
            detect_uploaded_artwork_mime(&png, Some("image/jpeg")).unwrap(),
            "image/png"
        );
        assert_eq!(
            detect_uploaded_artwork_mime(&png, Some("image/svg+xml")).unwrap(),
            "image/png"
        );

        let jpeg = [0xFF, 0xD8, 0xFF, 0xE0];
        assert_eq!(
            detect_uploaded_artwork_mime(&jpeg, Some("image/png")).unwrap(),
            "image/jpeg"
        );

        assert!(
            detect_uploaded_artwork_mime(b"<svg xmlns='http://www.w3.org/2000/svg'/>", None)
                .is_err()
        );
        assert!(detect_uploaded_artwork_mime(b"not-a-real-image", Some("image/svg+xml")).is_err());
        assert!(detect_uploaded_artwork_mime(b"not-a-real-image", Some("image/jpeg")).is_err());
    }
}

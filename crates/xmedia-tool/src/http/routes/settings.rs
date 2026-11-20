use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::routing::{get, patch, post};
use axum::{Json, Router};
use serde::Deserialize;
use uuid::Uuid;
use xmedia_core::domain::library::MediaKind;
use xmedia_core::http::ApiResult;

use crate::application::inbox::{InboxScanReportDto, scan_and_import_inbox};
use crate::application::settings::{PublicSettingsDto, UpdateSettingsInput};
use crate::application::webhook::{
    WebhookTestReportDto, test_xmedia_webhook, trigger_xmedia_rescan,
};
use crate::domain::workspace::{ManagedRoot, RootId};
use crate::http::ToolHttpState;

pub(super) fn router() -> Router<ToolHttpState> {
    Router::new()
        .route("/settings", get(get_settings).patch(update_settings))
        .route("/settings/test-webhook", post(test_webhook_route))
        .route("/settings/trigger-webhook", post(trigger_webhook_route))
        .route("/inbox/scan", post(scan_inbox_route))
        .route("/roots", post(add_root))
        .route("/roots/{id}", patch(update_root).delete(remove_root))
}

async fn get_settings(State(state): State<ToolHttpState>) -> ApiResult<Json<PublicSettingsDto>> {
    Ok(Json(state.settings.get_public_settings().await?))
}

async fn update_settings(
    State(state): State<ToolHttpState>,
    Json(input): Json<UpdateSettingsInput>,
) -> ApiResult<Json<PublicSettingsDto>> {
    Ok(Json(state.settings.update_settings(input).await?))
}

#[derive(Deserialize)]
struct AddRootRequest {
    name: String,
    path: String,
    default_kind: Option<MediaKind>,
    #[serde(default)]
    read_only: bool,
}

async fn add_root(
    State(state): State<ToolHttpState>,
    Json(req): Json<AddRootRequest>,
) -> ApiResult<(StatusCode, Json<ManagedRoot>)> {
    let root = state
        .settings
        .add_root_with_options(req.name, req.path, req.default_kind, req.read_only)
        .await?;
    Ok((StatusCode::CREATED, Json(root)))
}

#[derive(Deserialize)]
struct UpdateRootRequest {
    read_only: bool,
}

async fn update_root(
    State(state): State<ToolHttpState>,
    Path(id): Path<Uuid>,
    Json(req): Json<UpdateRootRequest>,
) -> ApiResult<Json<ManagedRoot>> {
    let root = state
        .settings
        .set_root_read_only(RootId(id), req.read_only)
        .await?;
    Ok(Json(root))
}

async fn remove_root(
    State(state): State<ToolHttpState>,
    Path(id): Path<Uuid>,
) -> ApiResult<StatusCode> {
    state.settings.remove_root(RootId(id)).await?;
    Ok(StatusCode::NO_CONTENT)
}

async fn test_webhook_route(
    State(state): State<ToolHttpState>,
) -> ApiResult<Json<WebhookTestReportDto>> {
    let settings = state.settings.get_raw().await?;
    let report = test_xmedia_webhook(&settings.xmedia_webhook).await?;
    Ok(Json(report))
}

#[derive(Deserialize, Default)]
struct TriggerWebhookReq {
    #[serde(default)]
    root_id: Option<Uuid>,
}

async fn trigger_webhook_route(
    State(state): State<ToolHttpState>,
    body: Option<Json<TriggerWebhookReq>>,
) -> ApiResult<Json<WebhookTestReportDto>> {
    let settings = state.settings.get_raw().await?;
    let root_path = body.and_then(|Json(req)| {
        req.root_id.and_then(|rid| {
            settings
                .roots
                .iter()
                .find(|r| r.id == RootId(rid))
                .map(|r| r.path.clone())
        })
    });
    let report = trigger_xmedia_rescan(&settings.xmedia_webhook, root_path.as_deref()).await?;
    Ok(Json(report))
}

async fn scan_inbox_route(
    State(state): State<ToolHttpState>,
) -> ApiResult<Json<InboxScanReportDto>> {
    let report =
        scan_and_import_inbox(&state.settings, &state.workbench, &state.scraping, 3).await?;
    Ok(Json(report))
}

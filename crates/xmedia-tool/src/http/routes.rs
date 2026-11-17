use axum::body::Body;
use axum::extract::{DefaultBodyLimit, Multipart, Path, Query, State};
use axum::http::header::{CACHE_CONTROL, CONTENT_TYPE, SET_COOKIE};
use axum::http::request::Parts;
use axum::http::{HeaderValue, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::{delete, get, post};
use axum::{Json, Router};
use serde::{Deserialize, Serialize};
use uuid::Uuid;
use xmedia_core::application::dto::FfmpegStatusDto;
use xmedia_core::domain::error::DomainError;
use xmedia_core::domain::library::MediaKind;
use xmedia_core::http::{ApiError, ApiResult};
use xmedia_core::infra::thumbnail::sniff_image_mime;

use crate::application::settings::{PublicSettingsDto, UpdateSettingsInput};
use crate::domain::media::{
    DirectorySnapshot, MediaFileDetail, MediaMetadataPatch, RenamePlanItem,
};
use crate::domain::scraper::{
    AggregateScrapeResponse, ProviderStatusDto, ScrapeCandidate, ScrapeQuery, ScrapeTargetKind,
    ScrapedMetadata,
};
use crate::domain::workspace::{ManagedRoot, RootId};
use crate::http::ToolHttpState;
use crate::http::auth::{ToolAuthGuard, build_tool_session_cookie, extract_tool_token};

pub fn tool_api_router() -> Router<ToolHttpState> {
    Router::new()
        .route("/status", get(get_tool_status))
        .route("/auth/unlock", post(unlock_tool))
        .route("/settings", get(get_settings).patch(update_settings))
        .route("/roots", post(add_root))
        .route("/roots/{id}", delete(remove_root))
        .route("/fs/browse", get(browse_fs))
        .route("/files/inspect", get(inspect_file))
        .route("/files/metadata", axum::routing::patch(write_metadata))
        .route(
            "/files/artwork",
            get(get_artwork)
                .post(upload_artwork)
                .layer(DefaultBodyLimit::max(32 * 1024 * 1024)),
        )
        .route("/files/rename/preview", post(preview_rename))
        .route("/files/rename/execute", post(execute_rename))
        .route("/scrapers/providers", get(list_scraper_providers))
        .route("/scrapers/search", post(search_scrapers))
        .route("/scrapers/aggregate", post(aggregate_scrapers))
        .route("/scrapers/detail", get(get_scraper_detail))
        .route("/scrapers/apply", post(apply_scraped_metadata))
}

#[derive(Serialize)]
struct ToolStatusResponse {
    version: &'static str,
    requires_password: bool,
    authenticated: bool,
    ffmpeg: FfmpegStatusDto,
}

async fn get_tool_status(
    State(state): State<ToolHttpState>,
    parts: Parts,
) -> Json<ToolStatusResponse> {
    let provided = extract_tool_token(&parts);
    let authenticated = state.auth.verify_token(provided.as_deref()).is_ok();
    let ffmpeg = state.ffmpeg.check_status().await;
    Json(ToolStatusResponse {
        version: env!("CARGO_PKG_VERSION"),
        requires_password: state.auth.requires_password(),
        authenticated,
        ffmpeg,
    })
}

#[derive(Deserialize)]
struct UnlockRequest {
    password: String,
}

#[derive(Serialize)]
struct UnlockResponse {
    token: String,
}

async fn unlock_tool(
    State(state): State<ToolHttpState>,
    Json(req): Json<UnlockRequest>,
) -> ApiResult<Response> {
    let token = state.auth.unlock(&req.password)?;
    let cookie = build_tool_session_cookie(&token);
    let mut res = (StatusCode::OK, Json(UnlockResponse { token })).into_response();
    res.headers_mut().insert(SET_COOKIE, cookie);
    Ok(res)
}

async fn get_settings(
    _auth: ToolAuthGuard,
    State(state): State<ToolHttpState>,
) -> ApiResult<Json<PublicSettingsDto>> {
    Ok(Json(state.settings.get_public_settings().await?))
}

async fn update_settings(
    _auth: ToolAuthGuard,
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
}

async fn add_root(
    _auth: ToolAuthGuard,
    State(state): State<ToolHttpState>,
    Json(req): Json<AddRootRequest>,
) -> ApiResult<(StatusCode, Json<ManagedRoot>)> {
    let root = state
        .settings
        .add_root(req.name, req.path, req.default_kind)
        .await?;
    Ok((StatusCode::CREATED, Json(root)))
}

async fn remove_root(
    _auth: ToolAuthGuard,
    State(state): State<ToolHttpState>,
    Path(id): Path<Uuid>,
) -> ApiResult<StatusCode> {
    state.settings.remove_root(RootId(id)).await?;
    Ok(StatusCode::NO_CONTENT)
}

#[derive(Deserialize)]
struct BrowseQuery {
    root_id: Uuid,
    #[serde(default)]
    path: String,
    kind: Option<MediaKind>,
}

async fn browse_fs(
    _auth: ToolAuthGuard,
    State(state): State<ToolHttpState>,
    Query(q): Query<BrowseQuery>,
) -> ApiResult<Json<DirectorySnapshot>> {
    let snap = state
        .workbench
        .browse_directory(RootId(q.root_id), &q.path, q.kind)
        .await?;
    Ok(Json(snap))
}

#[derive(Deserialize)]
struct FileTargetQuery {
    root_id: Uuid,
    path: String,
    kind: Option<MediaKind>,
}

async fn inspect_file(
    _auth: ToolAuthGuard,
    State(state): State<ToolHttpState>,
    Query(q): Query<FileTargetQuery>,
) -> ApiResult<Json<MediaFileDetail>> {
    let detail = state
        .workbench
        .inspect_file(RootId(q.root_id), &q.path, q.kind)
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
    _auth: ToolAuthGuard,
    State(state): State<ToolHttpState>,
    Json(req): Json<WriteMetadataRequest>,
) -> ApiResult<Json<MediaFileDetail>> {
    let detail = state
        .workbench
        .write_metadata(RootId(req.root_id), &req.path, req.patch)
        .await?;
    Ok(Json(detail))
}

async fn get_artwork(
    _auth: ToolAuthGuard,
    State(state): State<ToolHttpState>,
    Query(q): Query<FileTargetQuery>,
) -> ApiResult<Response> {
    let Some(payload) = state
        .workbench
        .read_artwork(RootId(q.root_id), &q.path, q.kind)
        .await?
    else {
        return Err(ApiError(DomainError::NotFound(
            "no cover/poster artwork present".into(),
        )));
    };
    let mut res = (StatusCode::OK, Body::from(payload.bytes)).into_response();
    if let Ok(ct) = HeaderValue::from_str(&payload.mime_type) {
        res.headers_mut().insert(CONTENT_TYPE, ct);
    }
    res.headers_mut()
        .insert(CACHE_CONTROL, HeaderValue::from_static("no-cache"));
    Ok(res)
}

async fn upload_artwork(
    _auth: ToolAuthGuard,
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
                let text = field.text().await.unwrap_or_default();
                root_id = Uuid::parse_str(text.trim()).ok();
            }
            "path" => {
                rel_path = Some(field.text().await.unwrap_or_default());
            }
            "slot" => {
                let s = field.text().await.unwrap_or_default();
                if !s.trim().is_empty() {
                    slot = s.trim().to_string();
                }
            }
            "kind" => {
                let k = field.text().await.unwrap_or_default();
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

    let mime = file_mime
        .filter(|m| m.starts_with("image/"))
        .unwrap_or_else(|| sniff_image_mime(&bytes).to_string());

    let detail = state
        .workbench
        .write_artwork(
            RootId(root_uuid),
            &path_str,
            &bytes,
            &mime,
            &slot,
            preferred_kind,
        )
        .await?;
    Ok(Json(detail))
}

#[derive(Deserialize)]
struct PreviewRenameRequest {
    root_id: Uuid,
    paths: Vec<String>,
    template: Option<String>,
    kind: Option<MediaKind>,
}

async fn preview_rename(
    _auth: ToolAuthGuard,
    State(state): State<ToolHttpState>,
    Json(req): Json<PreviewRenameRequest>,
) -> ApiResult<Json<Vec<RenamePlanItem>>> {
    let plans = state
        .workbench
        .preview_rename(
            RootId(req.root_id),
            &req.paths,
            req.template.as_deref(),
            req.kind,
        )
        .await?;
    Ok(Json(plans))
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
    _auth: ToolAuthGuard,
    State(state): State<ToolHttpState>,
    Json(req): Json<ExecuteRenameRequest>,
) -> ApiResult<Json<ExecuteRenameResponse>> {
    let moved_count = state
        .workbench
        .execute_rename(RootId(req.root_id), &req.plans)
        .await?;
    Ok(Json(ExecuteRenameResponse { moved_count }))
}

#[derive(Deserialize, Default)]
struct ProvidersQuery {
    target: Option<ScrapeTargetKind>,
    domain: Option<MediaKind>,
}

async fn list_scraper_providers(
    _auth: ToolAuthGuard,
    State(state): State<ToolHttpState>,
    Query(q): Query<ProvidersQuery>,
) -> ApiResult<Json<Vec<ProviderStatusDto>>> {
    Ok(Json(
        state.scraping.list_providers(q.target, q.domain).await?,
    ))
}

async fn search_scrapers(
    _auth: ToolAuthGuard,
    State(state): State<ToolHttpState>,
    Json(query): Json<ScrapeQuery>,
) -> ApiResult<Json<Vec<ScrapeCandidate>>> {
    Ok(Json(state.scraping.search(query).await?))
}

async fn aggregate_scrapers(
    _auth: ToolAuthGuard,
    State(state): State<ToolHttpState>,
    Json(query): Json<ScrapeQuery>,
) -> ApiResult<Json<AggregateScrapeResponse>> {
    Ok(Json(state.scraping.aggregate(query).await?))
}

#[derive(Deserialize)]
struct ScraperDetailQuery {
    provider_id: String,
    external_id: String,
    target: ScrapeTargetKind,
    locale: Option<String>,
}

async fn get_scraper_detail(
    _auth: ToolAuthGuard,
    State(state): State<ToolHttpState>,
    Query(q): Query<ScraperDetailQuery>,
) -> ApiResult<Json<ScrapedMetadata>> {
    let detail = state
        .scraping
        .fetch_detail(
            &q.provider_id,
            &q.external_id,
            q.target,
            q.locale.as_deref(),
        )
        .await?;
    Ok(Json(detail))
}

#[derive(Deserialize)]
struct ApplyScrapedRequest {
    root_id: Uuid,
    path: String,
    media_kind: MediaKind,
    metadata: ScrapedMetadata,
    #[serde(default = "default_true")]
    apply_cover: bool,
    #[serde(default = "default_true")]
    write_sidecar_lrc: bool,
}

const fn default_true() -> bool {
    true
}

async fn apply_scraped_metadata(
    _auth: ToolAuthGuard,
    State(state): State<ToolHttpState>,
    Json(req): Json<ApplyScrapedRequest>,
) -> ApiResult<Json<MediaFileDetail>> {
    let detail = state
        .scraping
        .apply_to_file(
            RootId(req.root_id),
            &req.path,
            req.media_kind,
            req.metadata,
            req.apply_cover,
            req.write_sidecar_lrc,
        )
        .await?;
    Ok(Json(detail))
}

use std::path::PathBuf;
use std::sync::Arc;

use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::extract::{FromRequestParts, Path, Query, State};
use axum::http::request::Parts;
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, patch, post, put};
use axum::{Json, Router};
use serde::{Deserialize, Serialize};
use tokio::sync::broadcast::error::RecvError;
use uuid::Uuid;
use xmedia_core::application::auth::AuthService;
use xmedia_core::application::dto::{
    ChangePasswordInput, CreateLibraryInput, CreateUserInput, LibraryDto, LoginInput,
    RenameLibraryInput, SetupAdminInput, SetupStatusDto, SystemCapabilitiesDto, UpdateUserInput,
    UserDto,
};
use xmedia_core::application::library::LibraryService;
use xmedia_core::domain::library::{LibraryId, MediaKind};
use xmedia_core::domain::user::{User, UserId, UserPreferences};
use xmedia_core::http::{
    ApiError, ApiResult, attach_cookie, authenticate_web_parts, build_clear_cookie,
    build_session_cookie,
};
use xmedia_core::infra::events::BroadcastEventHub;
use xmedia_core::infra::ffmpeg::FfmpegToolkit;

#[derive(Clone)]
pub struct GatewayState {
    pub auth: Arc<AuthService>,
    pub libraries: Arc<LibraryService>,
    pub events: Arc<BroadcastEventHub>,
    pub ffmpeg: FfmpegToolkit,
    pub enabled_modules: Arc<Vec<MediaKind>>,
    pub external_web_dir: Option<PathBuf>,
}

pub struct CoreWebAuth(pub User);

impl FromRequestParts<GatewayState> for CoreWebAuth {
    type Rejection = ApiError;

    async fn from_request_parts(
        parts: &mut Parts,
        state: &GatewayState,
    ) -> Result<Self, Self::Rejection> {
        let user = authenticate_web_parts(parts, &state.auth).await?;
        Ok(Self(user))
    }
}

pub fn core_api_router() -> Router<GatewayState> {
    Router::new()
        .route("/auth/setup-status", get(setup_status))
        .route("/auth/setup", post(setup_admin))
        .route("/auth/login", post(login))
        .route("/auth/logout", post(logout))
        .route("/auth/me", get(get_me))
        .route("/auth/preferences", put(update_preferences))
        .route("/auth/password", post(change_password))
        .route("/auth/rotate-api-key", post(rotate_api_key))
        .route("/users", get(list_users).post(create_user))
        .route("/users/{id}", patch(update_user).delete(delete_user))
        .route("/libraries", get(list_libraries).post(create_library))
        .route(
            "/libraries/{id}",
            get(get_library)
                .patch(rename_library)
                .delete(delete_library),
        )
        .route("/libraries/{id}/scan", post(trigger_library_scan))
        .route("/system/capabilities", get(system_capabilities))
        .route("/system/overview", get(system_overview))
        .route("/events/ws", get(events_ws))
}

#[derive(Serialize)]
struct OkResponse {
    ok: bool,
}

#[derive(Deserialize, Default)]
struct LibraryFilterParams {
    media_kind: Option<String>,
}

#[derive(Serialize)]
pub struct DomainStatSummary {
    pub media_kind: MediaKind,
    pub library_count: usize,
    pub item_count: u64,
    pub scanning: bool,
}

#[derive(Serialize)]
pub struct SystemOverviewDto {
    pub capabilities: SystemCapabilitiesDto,
    pub domains: Vec<DomainStatSummary>,
    pub libraries: Vec<LibraryDto>,
}

async fn setup_status(State(state): State<GatewayState>) -> ApiResult<Json<SetupStatusDto>> {
    Ok(Json(state.auth.setup_status().await?))
}

async fn setup_admin(
    State(state): State<GatewayState>,
    headers: HeaderMap,
    Json(input): Json<SetupAdminInput>,
) -> ApiResult<Response> {
    let session = state.auth.setup_initial_admin(input).await?;
    let cookie = build_session_cookie(&session.token, &headers);
    let res = (StatusCode::CREATED, Json(session)).into_response();
    Ok(attach_cookie(res, cookie))
}

async fn login(
    State(state): State<GatewayState>,
    headers: HeaderMap,
    Json(input): Json<LoginInput>,
) -> ApiResult<Response> {
    let session = state.auth.login(&input.username, &input.password).await?;
    let cookie = build_session_cookie(&session.token, &headers);
    let res = (StatusCode::OK, Json(session)).into_response();
    Ok(attach_cookie(res, cookie))
}

async fn logout(headers: HeaderMap) -> Response {
    let cookie = build_clear_cookie(&headers);
    let res = Json(OkResponse { ok: true }).into_response();
    attach_cookie(res, cookie)
}

async fn get_me(
    State(state): State<GatewayState>,
    CoreWebAuth(user): CoreWebAuth,
) -> ApiResult<Json<UserDto>> {
    Ok(Json(state.auth.get_me(&user).await?))
}

async fn update_preferences(
    State(state): State<GatewayState>,
    CoreWebAuth(user): CoreWebAuth,
    Json(prefs): Json<UserPreferences>,
) -> ApiResult<Json<UserDto>> {
    Ok(Json(state.auth.update_preferences(&user, prefs).await?))
}

async fn change_password(
    State(state): State<GatewayState>,
    CoreWebAuth(user): CoreWebAuth,
    Json(input): Json<ChangePasswordInput>,
) -> ApiResult<Json<UserDto>> {
    Ok(Json(state.auth.change_own_password(&user, input).await?))
}

async fn rotate_api_key(
    State(state): State<GatewayState>,
    CoreWebAuth(user): CoreWebAuth,
) -> ApiResult<Json<UserDto>> {
    Ok(Json(state.auth.rotate_api_key(&user).await?))
}

async fn list_users(
    State(state): State<GatewayState>,
    CoreWebAuth(user): CoreWebAuth,
) -> ApiResult<Json<Vec<UserDto>>> {
    Ok(Json(state.auth.list_users(&user).await?))
}

async fn create_user(
    State(state): State<GatewayState>,
    CoreWebAuth(user): CoreWebAuth,
    Json(input): Json<CreateUserInput>,
) -> ApiResult<(StatusCode, Json<UserDto>)> {
    let created = state.auth.create_user(&user, input).await?;
    Ok((StatusCode::CREATED, Json(created)))
}

async fn update_user(
    State(state): State<GatewayState>,
    CoreWebAuth(user): CoreWebAuth,
    Path(id): Path<Uuid>,
    Json(input): Json<UpdateUserInput>,
) -> ApiResult<Json<UserDto>> {
    Ok(Json(
        state.auth.update_user(&user, UserId(id), input).await?,
    ))
}

async fn delete_user(
    State(state): State<GatewayState>,
    CoreWebAuth(user): CoreWebAuth,
    Path(id): Path<Uuid>,
) -> ApiResult<Json<OkResponse>> {
    state.auth.delete_user(&user, UserId(id)).await?;
    Ok(Json(OkResponse { ok: true }))
}

async fn list_libraries(
    State(state): State<GatewayState>,
    CoreWebAuth(_user): CoreWebAuth,
    Query(q): Query<LibraryFilterParams>,
) -> ApiResult<Json<Vec<LibraryDto>>> {
    let kind = match q.media_kind.as_deref() {
        Some(raw) if !raw.trim().is_empty() => Some(MediaKind::parse(raw)?),
        _ => None,
    };
    Ok(Json(state.libraries.list_libraries(kind).await?))
}

async fn get_library(
    State(state): State<GatewayState>,
    CoreWebAuth(_user): CoreWebAuth,
    Path(id): Path<Uuid>,
) -> ApiResult<Json<LibraryDto>> {
    Ok(Json(state.libraries.get_library(LibraryId(id)).await?))
}

async fn create_library(
    State(state): State<GatewayState>,
    CoreWebAuth(user): CoreWebAuth,
    Json(input): Json<CreateLibraryInput>,
) -> ApiResult<(StatusCode, Json<LibraryDto>)> {
    let dto = state.libraries.create_local_mount(&user, input).await?;
    Ok((StatusCode::CREATED, Json(dto)))
}

async fn rename_library(
    State(state): State<GatewayState>,
    CoreWebAuth(user): CoreWebAuth,
    Path(id): Path<Uuid>,
    Json(input): Json<RenameLibraryInput>,
) -> ApiResult<Json<LibraryDto>> {
    Ok(Json(
        state
            .libraries
            .rename_library(&user, LibraryId(id), input)
            .await?,
    ))
}

async fn delete_library(
    State(state): State<GatewayState>,
    CoreWebAuth(user): CoreWebAuth,
    Path(id): Path<Uuid>,
) -> ApiResult<Json<OkResponse>> {
    state.libraries.delete_library(&user, LibraryId(id)).await?;
    Ok(Json(OkResponse { ok: true }))
}

async fn trigger_library_scan(
    State(state): State<GatewayState>,
    CoreWebAuth(user): CoreWebAuth,
    Path(id): Path<Uuid>,
) -> ApiResult<Json<LibraryDto>> {
    Ok(Json(
        state.libraries.trigger_scan(&user, LibraryId(id)).await?,
    ))
}

async fn system_capabilities(
    State(state): State<GatewayState>,
) -> ApiResult<Json<SystemCapabilitiesDto>> {
    let ffmpeg = state.ffmpeg.check_status().await;
    Ok(Json(SystemCapabilitiesDto {
        version: env!("CARGO_PKG_VERSION").to_string(),
        modules: (*state.enabled_modules).clone(),
        ffmpeg,
    }))
}

async fn system_overview(
    State(state): State<GatewayState>,
    CoreWebAuth(_user): CoreWebAuth,
) -> ApiResult<Json<SystemOverviewDto>> {
    let ffmpeg = state.ffmpeg.check_status().await;
    let libraries = state.libraries.list_libraries(None).await?;
    let mut domains = Vec::with_capacity(state.enabled_modules.len());

    for &kind in state.enabled_modules.iter() {
        let matching: Vec<&LibraryDto> =
            libraries.iter().filter(|l| l.media_kind == kind).collect();
        let item_count: u64 = matching.iter().map(|l| l.item_count).sum();
        let scanning = matching.iter().any(|l| l.scan_status.is_scanning());
        domains.push(DomainStatSummary {
            media_kind: kind,
            library_count: matching.len(),
            item_count,
            scanning,
        });
    }

    Ok(Json(SystemOverviewDto {
        capabilities: SystemCapabilitiesDto {
            version: env!("CARGO_PKG_VERSION").to_string(),
            modules: (*state.enabled_modules).clone(),
            ffmpeg,
        },
        domains,
        libraries,
    }))
}

async fn events_ws(
    ws: WebSocketUpgrade,
    State(state): State<GatewayState>,
    CoreWebAuth(_user): CoreWebAuth,
) -> Response {
    let rx = state.events.subscribe();
    ws.on_upgrade(move |socket| forward_events(socket, rx))
}

async fn forward_events(
    mut socket: WebSocket,
    mut rx: tokio::sync::broadcast::Receiver<xmedia_core::application::ports::SystemEvent>,
) {
    loop {
        match rx.recv().await {
            Ok(event) => {
                if let Ok(json) = serde_json::to_string(&event)
                    && socket.send(Message::Text(json.into())).await.is_err()
                {
                    break;
                }
            }
            Err(RecvError::Lagged(_)) => continue,
            Err(RecvError::Closed) => break,
        }
    }
}

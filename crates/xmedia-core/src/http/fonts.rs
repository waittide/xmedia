use std::sync::Arc;

use axum::body::Body;
use axum::extract::{DefaultBodyLimit, FromRef, Multipart, Path, State};
use axum::http::header::{CACHE_CONTROL, CONTENT_LENGTH, CONTENT_TYPE, X_CONTENT_TYPE_OPTIONS};
use axum::http::{HeaderValue, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::{delete, get};
use axum::{Json, Router};
use serde::{Deserialize, Serialize};
use uuid::Uuid;
use xmedia_formats::font::inspect_font_bytes;

#[cfg(feature = "server")]
use crate::application::auth::AuthService;
use crate::application::dto::CustomFontDto;
use crate::application::ports::FontRepository;
use crate::domain::error::DomainError;
#[cfg(feature = "server")]
use crate::http::WebAuth;
use crate::http::{ApiError, ApiResult};
#[cfg(feature = "server")]
use crate::infra::fonts::FsFontStore;
use crate::infra::fonts::MAX_FONT_FILE_BYTES;

pub const FONT_UPLOAD_BODY_LIMIT_BYTES: usize = MAX_FONT_FILE_BYTES as usize;
const MAX_CUSTOM_NAME_FIELD_BYTES: usize = 512;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct FontDeleteOkDto {
    pub ok: bool,
}

#[cfg(feature = "server")]
pub fn core_fonts_router<S>() -> Router<S>
where
    S: Clone + Send + Sync + 'static,
    Arc<AuthService>: FromRef<S>,
    Arc<FsFontStore>: FromRef<S>,
{
    Router::new()
        .route(
            "/fonts",
            get(authed_list_fonts::<S>)
                .post(authed_upload_fonts::<S>)
                .layer(DefaultBodyLimit::max(FONT_UPLOAD_BODY_LIMIT_BYTES)),
        )
        .route("/fonts/{id}", delete(authed_delete_font::<S>))
        .route("/fonts/{id}/file", get(authed_get_font_file::<S>))
}

pub fn port_fonts_router<S>() -> Router<S>
where
    S: Clone + Send + Sync + 'static,
    Arc<dyn FontRepository>: FromRef<S>,
{
    Router::new()
        .route(
            "/fonts",
            get(port_list_fonts::<S>)
                .post(port_upload_fonts::<S>)
                .layer(DefaultBodyLimit::max(FONT_UPLOAD_BODY_LIMIT_BYTES)),
        )
        .route("/fonts/{id}", delete(port_delete_font::<S>))
        .route("/fonts/{id}/file", get(port_get_font_file::<S>))
}

#[cfg(feature = "server")]
async fn authed_list_fonts<S>(
    State(state): State<S>,
    WebAuth(_actor): WebAuth,
) -> ApiResult<Json<Vec<CustomFontDto>>>
where
    Arc<FsFontStore>: FromRef<S>,
{
    let store = Arc::<FsFontStore>::from_ref(&state);
    list_custom_fonts(store.as_ref()).await
}

#[cfg(feature = "server")]
async fn authed_upload_fonts<S>(
    State(state): State<S>,
    WebAuth(actor): WebAuth,
    multipart: Multipart,
) -> ApiResult<(StatusCode, Json<Vec<CustomFontDto>>)>
where
    Arc<FsFontStore>: FromRef<S>,
{
    actor.ensure_admin()?;
    let store = Arc::<FsFontStore>::from_ref(&state);
    upload_custom_fonts(store.as_ref(), multipart).await
}

#[cfg(feature = "server")]
async fn authed_delete_font<S>(
    State(state): State<S>,
    WebAuth(actor): WebAuth,
    Path(id): Path<Uuid>,
) -> ApiResult<Json<FontDeleteOkDto>>
where
    Arc<FsFontStore>: FromRef<S>,
{
    actor.ensure_admin()?;
    let store = Arc::<FsFontStore>::from_ref(&state);
    delete_custom_font(store.as_ref(), id).await
}

#[cfg(feature = "server")]
async fn authed_get_font_file<S>(
    State(state): State<S>,
    WebAuth(_actor): WebAuth,
    Path(id): Path<Uuid>,
) -> ApiResult<Response>
where
    Arc<FsFontStore>: FromRef<S>,
{
    let store = Arc::<FsFontStore>::from_ref(&state);
    serve_custom_font_file(store.as_ref(), id).await
}

async fn port_list_fonts<S>(State(state): State<S>) -> ApiResult<Json<Vec<CustomFontDto>>>
where
    Arc<dyn FontRepository>: FromRef<S>,
{
    let store = Arc::<dyn FontRepository>::from_ref(&state);
    list_custom_fonts(store.as_ref()).await
}

async fn port_upload_fonts<S>(
    State(state): State<S>,
    multipart: Multipart,
) -> ApiResult<(StatusCode, Json<Vec<CustomFontDto>>)>
where
    Arc<dyn FontRepository>: FromRef<S>,
{
    let store = Arc::<dyn FontRepository>::from_ref(&state);
    upload_custom_fonts(store.as_ref(), multipart).await
}

async fn port_delete_font<S>(
    State(state): State<S>,
    Path(id): Path<Uuid>,
) -> ApiResult<Json<FontDeleteOkDto>>
where
    Arc<dyn FontRepository>: FromRef<S>,
{
    let store = Arc::<dyn FontRepository>::from_ref(&state);
    delete_custom_font(store.as_ref(), id).await
}

async fn port_get_font_file<S>(State(state): State<S>, Path(id): Path<Uuid>) -> ApiResult<Response>
where
    Arc<dyn FontRepository>: FromRef<S>,
{
    let store = Arc::<dyn FontRepository>::from_ref(&state);
    serve_custom_font_file(store.as_ref(), id).await
}

pub async fn list_custom_fonts(store: &dyn FontRepository) -> ApiResult<Json<Vec<CustomFontDto>>> {
    Ok(Json(store.list().await?))
}

pub async fn upload_custom_fonts(
    store: &dyn FontRepository,
    mut multipart: Multipart,
) -> ApiResult<(StatusCode, Json<Vec<CustomFontDto>>)> {
    let mut custom_name: Option<String> = None;
    let mut pending_files: Vec<(String, Vec<u8>)> = Vec::new();

    while let Some(mut field) = multipart
        .next_field()
        .await
        .map_err(|e| ApiError(DomainError::Validation(format!("multipart error: {e}"))))?
    {
        let field_name = field.name().unwrap_or("").to_string();
        let file_name_opt = field.file_name().map(str::to_string);

        if field_name == "name" && file_name_opt.is_none() {
            let mut text_bytes = Vec::new();
            while let Some(chunk) = field.chunk().await.map_err(|e| {
                ApiError(DomainError::Validation(format!(
                    "failed reading name field: {e}"
                )))
            })? {
                if text_bytes.len().saturating_add(chunk.len()) > MAX_CUSTOM_NAME_FIELD_BYTES {
                    return Err(ApiError(DomainError::Validation(
                        "custom font name field is too large".into(),
                    )));
                }
                text_bytes.extend_from_slice(&chunk);
            }
            if let Ok(text) = String::from_utf8(text_bytes) {
                let trimmed = text.trim();
                if !trimmed.is_empty() {
                    custom_name = Some(trimmed.to_string());
                }
            }
            continue;
        }

        let is_file_field =
            file_name_opt.is_some() || field_name == "file" || field_name == "files";
        if !is_file_field {
            continue;
        }

        let file_name = file_name_opt
            .filter(|s| !s.trim().is_empty())
            .unwrap_or_else(|| "font.bin".to_string());
        let mut data = Vec::new();
        while let Some(chunk) = field.chunk().await.map_err(|e| {
            ApiError(DomainError::Validation(format!(
                "font upload stream error: {e}"
            )))
        })? {
            let next_len = (data.len() as u64).saturating_add(chunk.len() as u64);
            if next_len > MAX_FONT_FILE_BYTES {
                return Err(ApiError(DomainError::Validation(format!(
                    "uploaded font exceeds maximum allowed size ({MAX_FONT_FILE_BYTES} bytes)"
                ))));
            }
            data.extend_from_slice(&chunk);
        }

        inspect_font_bytes(&file_name, &data).map_err(DomainError::from)?;
        pending_files.push((file_name, data));
    }

    if pending_files.is_empty() {
        return Err(ApiError(DomainError::Validation(
            "multipart form must include at least one font file".into(),
        )));
    }

    let mut created = Vec::with_capacity(pending_files.len());
    for (file_name, bytes) in pending_files {
        let dto = store
            .add(&file_name, custom_name.as_deref(), &bytes)
            .await?;
        created.push(dto);
    }

    Ok((StatusCode::CREATED, Json(created)))
}

pub async fn delete_custom_font(
    store: &dyn FontRepository,
    id: Uuid,
) -> ApiResult<Json<FontDeleteOkDto>> {
    store.delete(id).await?;
    Ok(Json(FontDeleteOkDto { ok: true }))
}

pub async fn serve_custom_font_file(store: &dyn FontRepository, id: Uuid) -> ApiResult<Response> {
    let (dto, bytes) = store.open_file(id).await?;
    let content_len = bytes.len();
    let mut res = (StatusCode::OK, Body::from(bytes)).into_response();
    let headers = res.headers_mut();
    if let Ok(ct) = HeaderValue::from_str(&dto.mime_type) {
        headers.insert(CONTENT_TYPE, ct);
    }
    if let Ok(cl) = HeaderValue::from_str(&content_len.to_string()) {
        headers.insert(CONTENT_LENGTH, cl);
    }
    headers.insert(X_CONTENT_TYPE_OPTIONS, HeaderValue::from_static("nosniff"));
    headers.insert(
        CACHE_CONTROL,
        HeaderValue::from_static("private, max-age=31536000, immutable"),
    );
    Ok(res)
}

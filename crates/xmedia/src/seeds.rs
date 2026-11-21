#[cfg(any(feature = "comic", feature = "music", feature = "video"))]
use axum::Json;
#[cfg(any(feature = "comic", feature = "music", feature = "video"))]
use axum::Router;
#[cfg(any(feature = "comic", feature = "music", feature = "video"))]
use axum::body::Bytes;
#[cfg(any(feature = "comic", feature = "music", feature = "video"))]
use axum::extract::State;
#[cfg(any(feature = "comic", feature = "music", feature = "video"))]
use axum::routing::post;
#[cfg(any(feature = "comic", feature = "music", feature = "video"))]
use xmedia_core::domain::error::{DomainError, DomainResult};
#[cfg(any(feature = "comic", feature = "music", feature = "video"))]
use xmedia_core::domain::user::require_admin;
#[cfg(any(feature = "comic", feature = "music", feature = "video"))]
use xmedia_core::http::{ApiResult, WebAuth};

#[cfg(feature = "comic")]
pub const COMIC_SEEDS_JSON: &str = include_str!("../../../seeds/sources/comic.json");
#[cfg(feature = "music")]
pub const MUSIC_SEEDS_JSON: &str = include_str!("../../../seeds/sources/music.json");
#[cfg(feature = "video")]
pub const VIDEO_SEEDS_JSON: &str = include_str!("../../../seeds/sources/video.json");

#[cfg(any(feature = "comic", feature = "music", feature = "video"))]
fn validate_optional_empty_json_object(body: &[u8]) -> DomainResult<()> {
    let text = std::str::from_utf8(body)
        .map_err(|_| DomainError::Validation("request body must be valid UTF-8 JSON".into()))?;
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return Ok(());
    }
    let value: serde_json::Value = serde_json::from_str(trimmed)
        .map_err(|e| DomainError::Validation(format!("invalid JSON request body: {e}")))?;
    if !value.is_object() {
        return Err(DomainError::Validation(
            "request body must be an empty body or a JSON object".into(),
        ));
    }
    Ok(())
}

#[cfg(feature = "comic")]
pub(crate) async fn seed_comic_sources(
    services: &xmedia_comic::application::ComicServices,
) -> DomainResult<xmedia_comic::application::online::ImportComicSourcesReportDto> {
    services.online.seed_missing_sources(COMIC_SEEDS_JSON).await
}

#[cfg(feature = "comic")]
async fn import_comic_seeds(
    State(state): State<xmedia_comic::http::state::ComicHttpState>,
    WebAuth(user): WebAuth,
    body: Bytes,
) -> ApiResult<Json<xmedia_comic::application::online::ImportComicSourcesReportDto>> {
    require_admin(&user)?;
    validate_optional_empty_json_object(&body)?;
    let report = state
        .services
        .online
        .import_missing_sources(&user, COMIC_SEEDS_JSON)
        .await?;
    Ok(Json(report))
}

#[cfg(feature = "comic")]
pub(crate) fn comic_seed_router(state: xmedia_comic::http::state::ComicHttpState) -> Router {
    Router::new()
        .route("/sources/import-seeds", post(import_comic_seeds))
        .with_state(state)
}

#[cfg(feature = "music")]
pub(crate) async fn seed_music_sources(
    services: &xmedia_music::application::MusicServices,
) -> DomainResult<xmedia_music::application::online::ImportMusicSourcesReportDto> {
    services.online.seed_missing_sources(MUSIC_SEEDS_JSON).await
}

#[cfg(feature = "music")]
async fn import_music_seeds(
    State(state): State<xmedia_music::http::state::MusicHttpState>,
    WebAuth(user): WebAuth,
    body: Bytes,
) -> ApiResult<Json<xmedia_music::application::online::ImportMusicSourcesReportDto>> {
    require_admin(&user)?;
    validate_optional_empty_json_object(&body)?;
    let report = state
        .services
        .online
        .import_missing_sources(&user, MUSIC_SEEDS_JSON)
        .await?;
    Ok(Json(report))
}

#[cfg(feature = "music")]
pub(crate) fn music_seed_router(state: xmedia_music::http::state::MusicHttpState) -> Router {
    Router::new()
        .route("/sources/import-seeds", post(import_music_seeds))
        .with_state(state)
}

#[cfg(feature = "video")]
pub(crate) async fn seed_video_sources(
    services: &xmedia_video::application::VideoServices,
) -> DomainResult<xmedia_video::application::online::ImportVideoSourcesReportDto> {
    services.online.seed_missing_sources(VIDEO_SEEDS_JSON).await
}

#[cfg(feature = "video")]
async fn import_video_seeds(
    State(state): State<xmedia_video::http::state::VideoHttpState>,
    WebAuth(user): WebAuth,
    body: Bytes,
) -> ApiResult<Json<xmedia_video::application::online::ImportVideoSourcesReportDto>> {
    require_admin(&user)?;
    validate_optional_empty_json_object(&body)?;
    let report = state
        .services
        .online
        .import_missing_sources(&user, VIDEO_SEEDS_JSON)
        .await?;
    Ok(Json(report))
}

#[cfg(feature = "video")]
pub(crate) fn video_seed_router(state: xmedia_video::http::state::VideoHttpState) -> Router {
    Router::new()
        .route("/sources/import-seeds", post(import_video_seeds))
        .with_state(state)
}

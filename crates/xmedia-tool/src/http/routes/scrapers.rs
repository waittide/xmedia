use axum::extract::{Query, State};
use axum::http::header::{CACHE_CONTROL, CONTENT_TYPE};
use axum::http::{HeaderValue, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use serde::{Deserialize, Serialize};
use uuid::Uuid;
use xmedia_core::domain::library::MediaKind;
use xmedia_core::http::ApiResult;

use crate::application::scraping::{AutoScrapeOptions, AutoScrapeResultDto, DryRunScrapeItemDto};
use crate::domain::media::MediaFileDetail;
use crate::domain::scraper::{
    AggregateScrapeResponse, ProviderStatusDto, ScrapeCandidate, ScrapeQuery, ScrapeTargetKind,
    ScrapedMetadata,
};
use crate::domain::workspace::RootId;
use crate::http::ToolHttpState;
use crate::infra::scrape_cache::ScrapeCacheStatsDto;

pub(super) fn router() -> Router<ToolHttpState> {
    Router::new()
        .route("/scrapers/providers", get(list_scraper_providers))
        .route("/scrapers/search", post(search_scrapers))
        .route("/scrapers/aggregate", post(aggregate_scrapers))
        .route("/scrapers/detail", get(get_scraper_detail))
        .route("/scrapers/image-proxy", get(proxy_scraper_image))
        .route("/scrapers/apply", post(apply_scraped_metadata))
        .route("/scrapers/auto-apply", post(auto_apply_scraped_metadata))
        .route("/scrapers/dry-run", post(dry_run_scrape_files))
        .route(
            "/scrapers/cache",
            get(get_scrape_cache_stats).delete(clear_scrape_cache),
        )
}

#[derive(Deserialize, Default)]
struct ProvidersQuery {
    target: Option<ScrapeTargetKind>,
    domain: Option<MediaKind>,
}

async fn list_scraper_providers(
    State(state): State<ToolHttpState>,
    Query(q): Query<ProvidersQuery>,
) -> ApiResult<Json<Vec<ProviderStatusDto>>> {
    Ok(Json(
        state.scraping.list_providers(q.target, q.domain).await?,
    ))
}

async fn search_scrapers(
    State(state): State<ToolHttpState>,
    Json(query): Json<ScrapeQuery>,
) -> ApiResult<Json<Vec<ScrapeCandidate>>> {
    Ok(Json(state.scraping.search(query).await?))
}

async fn aggregate_scrapers(
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
    #[serde(default)]
    organize_by_template: Option<bool>,
    #[serde(default)]
    only_fill_missing: bool,
    #[serde(default)]
    write_album_cover: bool,
}

const fn default_true() -> bool {
    true
}

async fn apply_scraped_metadata(
    State(state): State<ToolHttpState>,
    Json(req): Json<ApplyScrapedRequest>,
) -> ApiResult<Json<MediaFileDetail>> {
    let detail = state
        .scraping
        .apply_to_file_ext(
            RootId(req.root_id),
            &req.path,
            req.media_kind,
            req.metadata,
            req.apply_cover,
            req.write_sidecar_lrc,
            req.organize_by_template,
            req.only_fill_missing,
            req.write_album_cover,
        )
        .await?;
    Ok(Json(detail))
}

#[derive(Deserialize)]
struct AutoApplyScrapeRequest {
    root_id: Uuid,
    path: String,
    media_kind: MediaKind,
    #[serde(flatten)]
    options: AutoScrapeOptions,
}

async fn auto_apply_scraped_metadata(
    State(state): State<ToolHttpState>,
    Json(req): Json<AutoApplyScrapeRequest>,
) -> ApiResult<Json<AutoScrapeResultDto>> {
    let res = state
        .scraping
        .auto_scrape_file_with_options(RootId(req.root_id), &req.path, req.media_kind, req.options)
        .await?;
    Ok(Json(res))
}

#[derive(Deserialize)]
struct DryRunScrapeRequest {
    root_id: Uuid,
    paths: Vec<String>,
    locale: Option<String>,
}

async fn dry_run_scrape_files(
    State(state): State<ToolHttpState>,
    Json(req): Json<DryRunScrapeRequest>,
) -> ApiResult<Json<Vec<DryRunScrapeItemDto>>> {
    let items = state
        .scraping
        .dry_run_files(RootId(req.root_id), req.paths, req.locale)
        .await?;
    Ok(Json(items))
}

async fn get_scrape_cache_stats(
    State(state): State<ToolHttpState>,
) -> ApiResult<Json<ScrapeCacheStatsDto>> {
    Ok(Json(state.scraping.cache_stats().await?))
}

#[derive(Serialize)]
struct ClearScrapeCacheResponse {
    removed_count: usize,
}

async fn clear_scrape_cache(
    State(state): State<ToolHttpState>,
) -> ApiResult<Json<ClearScrapeCacheResponse>> {
    let removed_count = state.scraping.clear_cache()?;
    Ok(Json(ClearScrapeCacheResponse { removed_count }))
}

#[derive(Deserialize)]
struct ImageProxyQuery {
    url: String,
}

async fn proxy_scraper_image(
    State(state): State<ToolHttpState>,
    Query(q): Query<ImageProxyQuery>,
) -> ApiResult<Response> {
    let (bytes, mime) = state.scraping.proxy_scraper_image(&q.url).await?;
    let mut res = (StatusCode::OK, bytes).into_response();
    if let Ok(hv) = HeaderValue::from_str(&mime) {
        res.headers_mut().insert(CONTENT_TYPE, hv);
    }
    res.headers_mut().insert(
        CACHE_CONTROL,
        HeaderValue::from_static("private, max-age=86400"),
    );
    Ok(res)
}

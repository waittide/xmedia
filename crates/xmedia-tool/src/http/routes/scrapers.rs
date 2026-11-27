use axum::extract::{Query, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use serde::{Deserialize, Serialize};
use uuid::Uuid;
use xmedia_core::domain::library::MediaKind;
use xmedia_core::http::ApiResult;

use crate::application::ops::ScrapeCacheStatsDto;
use crate::application::scraping::{
    ApplyScrapeInput, AutoScrapeOptions, AutoScrapeResultDto, CacheManagementResult,
    DryRunScrapeItemDto,
};
use crate::domain::media::MediaFileDetail;
use crate::domain::scraper::{
    AggregateScrapeResponse, ProviderStatusDto, ScrapeCandidate, ScrapeQuery, ScrapeTargetKind,
    ScrapedMetadata,
};
use crate::domain::workspace::RootId;
use crate::http::ToolHttpState;
use crate::http::routes::apply_sandboxed_asset_headers;

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
        state.scrapers.list_providers(q.target, q.domain).await?,
    ))
}

async fn search_scrapers(
    State(state): State<ToolHttpState>,
    Json(query): Json<ScrapeQuery>,
) -> ApiResult<Json<Vec<ScrapeCandidate>>> {
    Ok(Json(state.scrapers.search(query).await?))
}

async fn aggregate_scrapers(
    State(state): State<ToolHttpState>,
    Json(query): Json<ScrapeQuery>,
) -> ApiResult<Json<AggregateScrapeResponse>> {
    Ok(Json(state.scrapers.aggregate(query).await?))
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
        .scrapers
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
        .scrape_apply
        .apply(ApplyScrapeInput {
            root_id: RootId(req.root_id),
            rel_path: req.path,
            media_kind: req.media_kind,
            scraped: req.metadata,
            apply_cover: req.apply_cover,
            write_sidecar_lrc: req.write_sidecar_lrc,
            organize_by_template: req.organize_by_template,
            only_fill_missing: req.only_fill_missing,
            write_album_cover: req.write_album_cover,
        })
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
        .scrape_apply
        .auto_scrape(RootId(req.root_id), &req.path, req.media_kind, req.options)
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
        .scrape_apply
        .dry_run(RootId(req.root_id), req.paths, req.locale)
        .await?;
    Ok(Json(items))
}

async fn get_scrape_cache_stats(
    State(state): State<ToolHttpState>,
) -> ApiResult<Json<ScrapeCacheStatsDto>> {
    let CacheManagementResult::Stats(stats) = state.scrapers.manage_cache(false).await? else {
        unreachable!()
    };
    Ok(Json(stats))
}

#[derive(Serialize)]
struct ClearScrapeCacheResponse {
    removed_count: usize,
}

async fn clear_scrape_cache(
    State(state): State<ToolHttpState>,
) -> ApiResult<Json<ClearScrapeCacheResponse>> {
    let CacheManagementResult::Cleared(removed_count) = state.scrapers.manage_cache(true).await?
    else {
        unreachable!()
    };
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
    let (bytes, mime) = state.scrapers.proxy_image(&q.url).await?;
    let mut res = (StatusCode::OK, bytes).into_response();
    apply_sandboxed_asset_headers(res.headers_mut(), &mime, "private, max-age=86400");
    Ok(res)
}

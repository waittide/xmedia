use std::convert::Infallible;
use std::pin::Pin;
use std::task::{Context, Poll};

use axum::body::Body;
use axum::extract::{DefaultBodyLimit, Multipart, Path, Query, State};
use axum::http::header::{CACHE_CONTROL, CONTENT_TYPE, SET_COOKIE};
use axum::http::request::Parts;
use axum::http::{HeaderValue, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::{delete, get, post};
use axum::{Json, Router};
use bytes::Bytes;
use futures_core::Stream;
use serde::{Deserialize, Serialize};
use tokio::sync::mpsc;
use uuid::Uuid;
use xmedia_core::application::dto::FfmpegStatusDto;
use xmedia_core::domain::error::{DomainError, DomainResult};
use xmedia_core::domain::library::MediaKind;
use xmedia_core::http::{ApiError, ApiResult};
use xmedia_core::infra::thumbnail::sniff_image_mime;

use crate::application::scraping::AutoScrapeResultDto;
use crate::application::settings::{PublicSettingsDto, UpdateSettingsInput};
use crate::application::workbench::WorkbenchProgress;
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
use crate::infra::inspectors::book_content::{
    BookContentDocumentDto, EditableBookChapterDto, InsertedEpubImageDto, SaveBookChapterInput,
    TransformBookContentResultDto,
};
use crate::infra::inspectors::comic_ops::{
    BatchComicSeriesSyncInput, BatchComicSeriesSyncReportDto, ComicImageReencodeFormat,
    ComicOptimizeReportDto, ComicPagesSnapshotDto, MergeComicsReportDto, SaveComicPageInput,
};
use crate::infra::inspectors::music_ops::{
    BatchMusicAlbumSyncInput, BatchMusicAlbumSyncReportDto, CueSplitReportDto,
    MusicCoverOptimizeReportDto, MusicLyricsToolInput, MusicLyricsToolReportDto,
    ReplayGainReportDto,
};
use crate::infra::inspectors::video_ops::{
    BatchRemapEpisodesInput, BatchRemapEpisodesReportDto, ExtractSubtitleReportDto,
    ProcessSubtitleInput, ProcessSubtitleReportDto, VideoSubtitleInfoDto,
};
use crate::infra::inspectors::zh_text::BookTextTransformOptions;
use crate::infra::scrape_cache::ScrapeCacheStatsDto;

pub fn tool_api_router() -> Router<ToolHttpState> {
    Router::new()
        .route("/status", get(get_tool_status))
        .route("/auth/unlock", post(unlock_tool))
        .route("/settings", get(get_settings).patch(update_settings))
        .route("/roots", post(add_root))
        .route("/roots/{id}", delete(remove_root))
        .route("/fs/browse", get(browse_fs))
        .route("/fs/browse/stream", get(browse_fs_stream))
        .route("/files/inspect", get(inspect_file))
        .route("/files/metadata", axum::routing::patch(write_metadata))
        .route(
            "/files/artwork",
            get(get_artwork)
                .post(upload_artwork)
                .layer(DefaultBodyLimit::max(32 * 1024 * 1024)),
        )
        .route("/files/rename/preview", post(preview_rename))
        .route("/files/rename/preview/stream", post(preview_rename_stream))
        .route("/files/rename/execute", post(execute_rename))
        .route("/files/rename/execute/stream", post(execute_rename_stream))
        .route("/scrapers/providers", get(list_scraper_providers))
        .route("/scrapers/search", post(search_scrapers))
        .route("/scrapers/aggregate", post(aggregate_scrapers))
        .route("/scrapers/detail", get(get_scraper_detail))
        .route("/scrapers/image-proxy", get(proxy_scraper_image))
        .route("/scrapers/apply", post(apply_scraped_metadata))
        .route("/scrapers/auto-apply", post(auto_apply_scraped_metadata))
        .route(
            "/scrapers/cache",
            get(get_scrape_cache_stats).delete(clear_scrape_cache),
        )
        .route(
            "/books/content",
            get(get_book_content)
                .put(put_book_content)
                .layer(DefaultBodyLimit::max(64 * 1024 * 1024)),
        )
        .route("/books/epub-asset", get(get_book_epub_asset))
        .route(
            "/books/epub-image",
            post(upload_book_epub_image).layer(DefaultBodyLimit::max(32 * 1024 * 1024)),
        )
        .route("/books/resegment-txt", post(resegment_txt_book))
        .route(
            "/books/transform",
            post(transform_book_content).layer(DefaultBodyLimit::max(64 * 1024 * 1024)),
        )
        .route("/books/polish", post(polish_book))
        .route(
            "/books/convert-epub",
            post(convert_txt_book_to_epub).layer(DefaultBodyLimit::max(64 * 1024 * 1024)),
        )
        .route("/comics/pages", get(get_comic_pages).put(put_comic_pages))
        .route("/comics/page-image", get(get_comic_page_image))
        .route("/comics/optimize", post(optimize_comic))
        .route("/comics/merge", post(merge_comics))
        .route("/comics/batch-sync", post(batch_sync_comics))
        .route("/music/lyrics-tool", post(run_music_lyrics_tool))
        .route("/music/optimize-cover", post(optimize_music_cover_route))
        .route("/music/replaygain", post(apply_music_replaygain))
        .route("/music/split-cue", post(split_music_cue))
        .route("/music/batch-sync", post(batch_sync_music_album))
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
    #[serde(default)]
    recursive: bool,
}

async fn browse_fs(
    _auth: ToolAuthGuard,
    State(state): State<ToolHttpState>,
    Query(q): Query<BrowseQuery>,
) -> ApiResult<Json<DirectorySnapshot>> {
    let snap = state
        .workbench
        .browse_directory(RootId(q.root_id), &q.path, q.kind, q.recursive)
        .await?;
    Ok(Json(snap))
}

#[derive(Deserialize)]
struct FileTargetQuery {
    root_id: Uuid,
    path: String,
    kind: Option<MediaKind>,
    #[serde(default)]
    slot: Option<String>,
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
        .read_artwork(RootId(q.root_id), &q.path, q.kind, q.slot.as_deref())
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
    episode_template: Option<String>,
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
            req.episode_template.as_deref(),
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
    _auth: ToolAuthGuard,
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
}

async fn auto_apply_scraped_metadata(
    _auth: ToolAuthGuard,
    State(state): State<ToolHttpState>,
    Json(req): Json<AutoApplyScrapeRequest>,
) -> ApiResult<Json<AutoScrapeResultDto>> {
    let res = state
        .scraping
        .auto_scrape_file(RootId(req.root_id), &req.path, req.media_kind)
        .await?;
    Ok(Json(res))
}

async fn get_scrape_cache_stats(
    _auth: ToolAuthGuard,
    State(state): State<ToolHttpState>,
) -> ApiResult<Json<ScrapeCacheStatsDto>> {
    Ok(Json(state.scraping.cache_stats().await?))
}

#[derive(Serialize)]
struct ClearScrapeCacheResponse {
    removed_count: usize,
}

async fn clear_scrape_cache(
    _auth: ToolAuthGuard,
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
    _auth: ToolAuthGuard,
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

struct MpscByteStream {
    rx: mpsc::Receiver<Result<Bytes, Infallible>>,
}

impl Stream for MpscByteStream {
    type Item = Result<Bytes, Infallible>;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        self.rx.poll_recv(cx)
    }
}

#[derive(Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum NdjsonFrame<T: Serialize> {
    Progress {
        processed: u64,
        total: Option<u64>,
        #[serde(skip_serializing_if = "Option::is_none")]
        detail: Option<String>,
    },
    Done {
        data: T,
    },
    Error {
        message: String,
    },
}

fn encode_ndjson_line<T: Serialize>(frame: &NdjsonFrame<T>) -> Bytes {
    let mut line = serde_json::to_vec(frame).unwrap_or_default();
    line.push(b'\n');
    Bytes::from(line)
}

fn spawn_ndjson_stream<T, Fut, Work>(work: Work) -> Response
where
    T: Serialize + Send + 'static,
    Fut: std::future::Future<Output = DomainResult<T>> + Send + 'static,
    Work: FnOnce(mpsc::Sender<WorkbenchProgress>) -> Fut + Send + 'static,
{
    let (byte_tx, byte_rx) = mpsc::channel::<Result<Bytes, Infallible>>(64);
    let (prog_tx, mut prog_rx) = mpsc::channel::<WorkbenchProgress>(64);

    let progress_byte_tx = byte_tx.clone();
    let forward_handle = tokio::spawn(async move {
        while let Some(p) = prog_rx.recv().await {
            let line = encode_ndjson_line::<T>(&NdjsonFrame::Progress {
                processed: p.processed,
                total: p.total,
                detail: p.detail,
            });
            if progress_byte_tx.send(Ok(line)).await.is_err() {
                break;
            }
        }
    });

    tokio::spawn(async move {
        let res = work(prog_tx).await;
        let _ = forward_handle.await;
        let final_line = match res {
            Ok(data) => encode_ndjson_line(&NdjsonFrame::Done { data }),
            Err(err) => encode_ndjson_line::<T>(&NdjsonFrame::Error {
                message: err.to_string(),
            }),
        };
        let _ = byte_tx.send(Ok(final_line)).await;
    });

    let mut res = (
        StatusCode::OK,
        Body::from_stream(MpscByteStream { rx: byte_rx }),
    )
        .into_response();
    res.headers_mut().insert(
        CONTENT_TYPE,
        HeaderValue::from_static("application/x-ndjson; charset=utf-8"),
    );
    res.headers_mut()
        .insert(CACHE_CONTROL, HeaderValue::from_static("no-cache"));
    res.headers_mut()
        .insert("x-accel-buffering", HeaderValue::from_static("no"));
    res
}

async fn browse_fs_stream(
    _auth: ToolAuthGuard,
    State(state): State<ToolHttpState>,
    Query(q): Query<BrowseQuery>,
) -> Response {
    spawn_ndjson_stream(move |prog_tx| async move {
        state
            .workbench
            .browse_directory_with_progress(
                RootId(q.root_id),
                &q.path,
                q.kind,
                q.recursive,
                move |p| {
                    let _ = prog_tx.try_send(p);
                },
            )
            .await
    })
}

async fn preview_rename_stream(
    _auth: ToolAuthGuard,
    State(state): State<ToolHttpState>,
    Json(req): Json<PreviewRenameRequest>,
) -> Response {
    spawn_ndjson_stream(move |prog_tx| async move {
        state
            .workbench
            .preview_rename_with_progress(
                RootId(req.root_id),
                &req.paths,
                req.template.as_deref(),
                req.episode_template.as_deref(),
                req.kind,
                move |p| {
                    let _ = prog_tx.try_send(p);
                },
            )
            .await
    })
}

async fn execute_rename_stream(
    _auth: ToolAuthGuard,
    State(state): State<ToolHttpState>,
    Json(req): Json<ExecuteRenameRequest>,
) -> Response {
    spawn_ndjson_stream(move |prog_tx| async move {
        let moved_count = state
            .workbench
            .execute_rename_with_progress(RootId(req.root_id), &req.plans, move |p| {
                let _ = prog_tx.try_send(p);
            })
            .await?;
        Ok(ExecuteRenameResponse { moved_count })
    })
}

#[derive(Deserialize)]
struct BookContentQuery {
    root_id: Uuid,
    path: String,
}

#[derive(Deserialize)]
struct BookEpubAssetQuery {
    root_id: Uuid,
    path: String,
    asset: String,
}

async fn get_book_epub_asset(
    _auth: ToolAuthGuard,
    State(state): State<ToolHttpState>,
    Query(q): Query<BookEpubAssetQuery>,
) -> ApiResult<Response> {
    let payload = state
        .workbench
        .read_epub_asset(RootId(q.root_id), &q.path, &q.asset)
        .await?;
    let mut res = (StatusCode::OK, Body::from(payload.bytes)).into_response();
    if let Ok(ct) = HeaderValue::from_str(&payload.mime_type) {
        res.headers_mut().insert(CONTENT_TYPE, ct);
    }
    res.headers_mut().insert(
        CACHE_CONTROL,
        HeaderValue::from_static("private, max-age=300"),
    );
    Ok(res)
}

async fn get_book_content(
    _auth: ToolAuthGuard,
    State(state): State<ToolHttpState>,
    Query(q): Query<BookContentQuery>,
) -> ApiResult<Json<BookContentDocumentDto>> {
    let doc = state
        .workbench
        .get_book_content(RootId(q.root_id), &q.path)
        .await?;
    Ok(Json(doc))
}

#[derive(Deserialize)]
struct ResegmentTxtRequest {
    root_id: Uuid,
    path: String,
    #[serde(default)]
    chapter_pattern: Option<String>,
    #[serde(default)]
    volume_pattern: Option<String>,
}

async fn resegment_txt_book(
    _auth: ToolAuthGuard,
    State(state): State<ToolHttpState>,
    Json(req): Json<ResegmentTxtRequest>,
) -> ApiResult<Json<Vec<EditableBookChapterDto>>> {
    let chapters = state
        .workbench
        .resegment_txt_book(
            RootId(req.root_id),
            &req.path,
            req.chapter_pattern,
            req.volume_pattern,
        )
        .await?;
    Ok(Json(chapters))
}

#[derive(Deserialize)]
struct SaveBookContentRequest {
    root_id: Uuid,
    path: String,
    chapters: Vec<SaveBookChapterInput>,
    #[serde(default)]
    stylesheets: Option<Vec<String>>,
    #[serde(default)]
    force_horizontal_spine: bool,
}

async fn put_book_content(
    _auth: ToolAuthGuard,
    State(state): State<ToolHttpState>,
    Json(req): Json<SaveBookContentRequest>,
) -> ApiResult<Json<MediaFileDetail>> {
    let detail = state
        .workbench
        .save_book_content(
            RootId(req.root_id),
            &req.path,
            req.chapters,
            req.stylesheets,
            req.force_horizontal_spine,
        )
        .await?;
    Ok(Json(detail))
}

#[derive(Deserialize)]
struct TransformBookContentRequest {
    chapters: Vec<EditableBookChapterDto>,
    #[serde(default)]
    stylesheets: Vec<String>,
    options: BookTextTransformOptions,
}

async fn transform_book_content(
    _auth: ToolAuthGuard,
    State(state): State<ToolHttpState>,
    Json(req): Json<TransformBookContentRequest>,
) -> ApiResult<Json<TransformBookContentResultDto>> {
    let out = state
        .workbench
        .transform_book_content(req.chapters, req.stylesheets, req.options)
        .await?;
    Ok(Json(out))
}

#[derive(Deserialize)]
struct PolishBookRequest {
    root_id: Uuid,
    path: String,
}

#[derive(Serialize)]
struct PolishBookResponse {
    jacket_updated: bool,
    removed_orphan_files: usize,
    detail: MediaFileDetail,
}

async fn polish_book(
    _auth: ToolAuthGuard,
    State(state): State<ToolHttpState>,
    Json(req): Json<PolishBookRequest>,
) -> ApiResult<Json<PolishBookResponse>> {
    let (report, detail) = state
        .workbench
        .polish_book(RootId(req.root_id), &req.path)
        .await?;
    Ok(Json(PolishBookResponse {
        jacket_updated: report.jacket_updated,
        removed_orphan_files: report.removed_orphan_files,
        detail,
    }))
}

async fn upload_book_epub_image(
    _auth: ToolAuthGuard,
    State(state): State<ToolHttpState>,
    mut multipart: Multipart,
) -> ApiResult<Json<InsertedEpubImageDto>> {
    let mut root_id: Option<Uuid> = None;
    let mut rel_path: Option<String> = None;
    let mut chapter_source_href: Option<String> = None;
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
            "chapter_source_href" => {
                let text = field.text().await.unwrap_or_default();
                if !text.trim().is_empty() {
                    chapter_source_href = Some(text.trim().to_string());
                }
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

    let inserted = state
        .workbench
        .insert_epub_chapter_image(
            RootId(root_uuid),
            &path_str,
            chapter_source_href,
            bytes,
            file_mime,
        )
        .await?;
    Ok(Json(inserted))
}

#[derive(Deserialize)]
struct ConvertTxtToEpubRequest {
    root_id: Uuid,
    path: String,
    #[serde(default)]
    chapters: Option<Vec<SaveBookChapterInput>>,
    #[serde(default)]
    stylesheets: Option<Vec<String>>,
    #[serde(default)]
    delete_source_txt: bool,
}

#[derive(Serialize)]
struct ConvertTxtToEpubResponse {
    detail: MediaFileDetail,
    deleted_source_txt: bool,
}

async fn convert_txt_book_to_epub(
    _auth: ToolAuthGuard,
    State(state): State<ToolHttpState>,
    Json(req): Json<ConvertTxtToEpubRequest>,
) -> ApiResult<Json<ConvertTxtToEpubResponse>> {
    let detail = state
        .workbench
        .convert_txt_book_to_epub(
            RootId(req.root_id),
            &req.path,
            req.chapters,
            req.stylesheets,
            req.delete_source_txt,
        )
        .await?;
    Ok(Json(ConvertTxtToEpubResponse {
        detail,
        deleted_source_txt: req.delete_source_txt,
    }))
}

#[derive(Deserialize)]
struct ComicPageImageQuery {
    root_id: Uuid,
    path: String,
    entry: String,
    max_width: Option<u32>,
}

async fn get_comic_pages(
    _auth: ToolAuthGuard,
    State(state): State<ToolHttpState>,
    Query(q): Query<FileTargetQuery>,
) -> ApiResult<Json<ComicPagesSnapshotDto>> {
    let snap = state
        .workbench
        .get_comic_pages(RootId(q.root_id), &q.path)
        .await?;
    Ok(Json(snap))
}

async fn get_comic_page_image(
    _auth: ToolAuthGuard,
    State(state): State<ToolHttpState>,
    Query(q): Query<ComicPageImageQuery>,
) -> ApiResult<Response> {
    let payload = state
        .workbench
        .read_comic_page(RootId(q.root_id), &q.path, &q.entry, q.max_width)
        .await?;
    let mut resp = (StatusCode::OK, payload.bytes).into_response();
    if let Ok(ct) = HeaderValue::from_str(&payload.mime_type) {
        resp.headers_mut().insert(CONTENT_TYPE, ct);
    }
    resp.headers_mut().insert(
        CACHE_CONTROL,
        HeaderValue::from_static("private, max-age=120"),
    );
    Ok(resp)
}

#[derive(Deserialize)]
struct SaveComicPagesRequest {
    root_id: Uuid,
    path: String,
    pages: Vec<SaveComicPageInput>,
    #[serde(default)]
    reencode: ComicImageReencodeFormat,
    #[serde(default = "default_jpeg_quality")]
    jpeg_quality: u8,
    max_width: Option<u32>,
}

fn default_jpeg_quality() -> u8 {
    84
}

#[derive(Serialize)]
struct ComicOptimizeResponse {
    report: ComicOptimizeReportDto,
    detail: MediaFileDetail,
}

async fn put_comic_pages(
    _auth: ToolAuthGuard,
    State(state): State<ToolHttpState>,
    Json(req): Json<SaveComicPagesRequest>,
) -> ApiResult<Json<ComicOptimizeResponse>> {
    let (report, detail) = state
        .workbench
        .save_comic_pages(
            RootId(req.root_id),
            &req.path,
            req.pages,
            req.reencode,
            req.jpeg_quality,
            req.max_width,
        )
        .await?;
    Ok(Json(ComicOptimizeResponse { report, detail }))
}

#[derive(Deserialize)]
struct OptimizeComicRequest {
    root_id: Uuid,
    path: String,
    #[serde(default)]
    reencode: ComicImageReencodeFormat,
    #[serde(default = "default_jpeg_quality")]
    jpeg_quality: u8,
    max_width: Option<u32>,
}

async fn optimize_comic(
    _auth: ToolAuthGuard,
    State(state): State<ToolHttpState>,
    Json(req): Json<OptimizeComicRequest>,
) -> ApiResult<Json<ComicOptimizeResponse>> {
    let (report, detail) = state
        .workbench
        .optimize_comic(
            RootId(req.root_id),
            &req.path,
            req.reencode,
            req.jpeg_quality,
            req.max_width,
        )
        .await?;
    Ok(Json(ComicOptimizeResponse { report, detail }))
}

#[derive(Deserialize)]
struct MergeComicsRequest {
    root_id: Uuid,
    source_paths: Vec<String>,
    output_rel_path: String,
    series: Option<String>,
    title: Option<String>,
    volume: Option<i32>,
}

#[derive(Serialize)]
struct MergeComicsResponse {
    report: MergeComicsReportDto,
    detail: MediaFileDetail,
}

async fn merge_comics(
    _auth: ToolAuthGuard,
    State(state): State<ToolHttpState>,
    Json(req): Json<MergeComicsRequest>,
) -> ApiResult<Json<MergeComicsResponse>> {
    let (report, detail) = state
        .workbench
        .merge_comics(
            RootId(req.root_id),
            req.source_paths,
            &req.output_rel_path,
            req.series,
            req.title,
            req.volume,
        )
        .await?;
    Ok(Json(MergeComicsResponse { report, detail }))
}

#[derive(Deserialize)]
struct BatchSyncComicsRequest {
    root_id: Uuid,
    paths: Vec<String>,
    #[serde(flatten)]
    input: BatchComicSeriesSyncInput,
}

async fn batch_sync_comics(
    _auth: ToolAuthGuard,
    State(state): State<ToolHttpState>,
    Json(req): Json<BatchSyncComicsRequest>,
) -> ApiResult<Json<BatchComicSeriesSyncReportDto>> {
    let report = state
        .workbench
        .batch_sync_comic_series(RootId(req.root_id), req.paths, req.input)
        .await?;
    Ok(Json(report))
}

#[derive(Deserialize)]
struct MusicLyricsToolRequest {
    root_id: Uuid,
    path: String,
    #[serde(flatten)]
    input: MusicLyricsToolInput,
}

#[derive(Serialize)]
struct MusicLyricsToolResponse {
    report: MusicLyricsToolReportDto,
    detail: MediaFileDetail,
}

async fn run_music_lyrics_tool(
    _auth: ToolAuthGuard,
    State(state): State<ToolHttpState>,
    Json(req): Json<MusicLyricsToolRequest>,
) -> ApiResult<Json<MusicLyricsToolResponse>> {
    let (report, detail) = state
        .workbench
        .run_music_lyrics_tool(RootId(req.root_id), &req.path, req.input)
        .await?;
    Ok(Json(MusicLyricsToolResponse { report, detail }))
}

#[derive(Deserialize)]
struct OptimizeMusicCoverRequest {
    root_id: Uuid,
    path: String,
    #[serde(default = "default_cover_max_dim")]
    max_dimension: u32,
    #[serde(default = "default_jpeg_quality")]
    jpeg_quality: u8,
}

fn default_cover_max_dim() -> u32 {
    1000
}

#[derive(Serialize)]
struct OptimizeMusicCoverResponse {
    report: MusicCoverOptimizeReportDto,
    detail: MediaFileDetail,
}

async fn optimize_music_cover_route(
    _auth: ToolAuthGuard,
    State(state): State<ToolHttpState>,
    Json(req): Json<OptimizeMusicCoverRequest>,
) -> ApiResult<Json<OptimizeMusicCoverResponse>> {
    let (report, detail) = state
        .workbench
        .optimize_music_cover(
            RootId(req.root_id),
            &req.path,
            req.max_dimension,
            req.jpeg_quality,
        )
        .await?;
    Ok(Json(OptimizeMusicCoverResponse { report, detail }))
}

#[derive(Deserialize)]
struct ReplayGainRequest {
    root_id: Uuid,
    paths: Vec<String>,
}

async fn apply_music_replaygain(
    _auth: ToolAuthGuard,
    State(state): State<ToolHttpState>,
    Json(req): Json<ReplayGainRequest>,
) -> ApiResult<Json<ReplayGainReportDto>> {
    let report = state
        .workbench
        .apply_music_replaygain(
            RootId(req.root_id),
            req.paths,
            state.ffmpeg.ffmpeg_bin().to_string(),
        )
        .await?;
    Ok(Json(report))
}

#[derive(Deserialize)]
struct SplitCueRequest {
    root_id: Uuid,
    path: String,
}

async fn split_music_cue(
    _auth: ToolAuthGuard,
    State(state): State<ToolHttpState>,
    Json(req): Json<SplitCueRequest>,
) -> ApiResult<Json<CueSplitReportDto>> {
    let report = state
        .workbench
        .split_music_cue(
            RootId(req.root_id),
            &req.path,
            state.ffmpeg.ffmpeg_bin().to_string(),
        )
        .await?;
    Ok(Json(report))
}

#[derive(Deserialize)]
struct BatchSyncMusicAlbumRequest {
    root_id: Uuid,
    paths: Vec<String>,
    #[serde(flatten)]
    input: BatchMusicAlbumSyncInput,
}

async fn batch_sync_music_album(
    _auth: ToolAuthGuard,
    State(state): State<ToolHttpState>,
    Json(req): Json<BatchSyncMusicAlbumRequest>,
) -> ApiResult<Json<BatchMusicAlbumSyncReportDto>> {
    let report = state
        .workbench
        .batch_sync_music_album(RootId(req.root_id), req.paths, req.input)
        .await?;
    Ok(Json(report))
}

async fn list_video_subtitles_route(
    _auth: ToolAuthGuard,
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
    _auth: ToolAuthGuard,
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
    _auth: ToolAuthGuard,
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
            state.ffmpeg.ffmpeg_bin().to_string(),
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
    _auth: ToolAuthGuard,
    State(state): State<ToolHttpState>,
    Json(req): Json<BatchRemapEpisodesRequest>,
) -> ApiResult<Json<BatchRemapEpisodesReportDto>> {
    let report = state
        .workbench
        .batch_remap_video_episodes(RootId(req.root_id), req.paths, req.input)
        .await?;
    Ok(Json(report))
}

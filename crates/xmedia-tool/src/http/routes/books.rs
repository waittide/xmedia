use axum::body::Body;
use axum::extract::{DefaultBodyLimit, Multipart, Query, State};
use axum::http::header::{CACHE_CONTROL, CONTENT_TYPE};
use axum::http::{HeaderValue, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use serde::{Deserialize, Serialize};
use uuid::Uuid;
use xmedia_core::domain::error::DomainError;
use xmedia_core::http::{ApiError, ApiResult};

use crate::domain::media::MediaFileDetail;
use crate::domain::workspace::RootId;
use crate::http::ToolHttpState;
use crate::infra::inspectors::book_content::{
    BookContentDocumentDto, EditableBookChapterDto, InsertedEpubImageDto, SaveBookChapterInput,
    TransformBookContentResultDto,
};
use crate::infra::inspectors::book_normalize::{
    NormalizeBookToEpubOptions, NormalizeBooksBatchReportDto,
};
use crate::infra::inspectors::zh_text::BookTextTransformOptions;

pub(super) fn router() -> Router<ToolHttpState> {
    Router::new()
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
        .route("/books/normalize-epub", post(normalize_books_to_epub_route))
        .route(
            "/books/convert-epub",
            post(convert_txt_book_to_epub).layer(DefaultBodyLimit::max(64 * 1024 * 1024)),
        )
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

#[derive(Deserialize)]
struct NormalizeBooksToEpubRequest {
    root_id: Uuid,
    paths: Vec<String>,
    #[serde(default)]
    options: NormalizeBookToEpubOptions,
}

async fn normalize_books_to_epub_route(
    State(state): State<ToolHttpState>,
    Json(req): Json<NormalizeBooksToEpubRequest>,
) -> ApiResult<Json<NormalizeBooksBatchReportDto>> {
    let report = state
        .workbench
        .normalize_books_to_epub(RootId(req.root_id), &req.paths, req.options)
        .await?;
    Ok(Json(report))
}

async fn upload_book_epub_image(
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

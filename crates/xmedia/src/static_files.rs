use std::path::{Component, Path, PathBuf};

use axum::body::Body;
use axum::extract::State;
use axum::http::header::{CACHE_CONTROL, CONTENT_TYPE};
use axum::http::{HeaderValue, StatusCode, Uri};
use axum::response::{Html, IntoResponse, Response};
use rust_embed::RustEmbed;

use crate::gateway::GatewayState;

#[derive(RustEmbed)]
#[folder = "../../web/dist"]
#[allow_missing = true]
struct EmbeddedWebAssets;

fn normalize_asset_path(uri_path: &str) -> Option<PathBuf> {
    let trimmed = uri_path.trim_start_matches('/');
    let path = Path::new(trimmed);
    let mut clean = PathBuf::new();
    for comp in path.components() {
        match comp {
            Component::Normal(part) => clean.push(part),
            Component::CurDir => {}
            Component::ParentDir | Component::RootDir | Component::Prefix(_) => return None,
        }
    }
    Some(clean)
}

fn asset_response(path_hint: &str, bytes: Vec<u8>, immutable: bool) -> Response {
    let mime = mime_guess::from_path(path_hint).first_or_octet_stream();
    let mut res = (StatusCode::OK, Body::from(bytes)).into_response();
    if let Ok(ct) = HeaderValue::from_str(mime.as_ref()) {
        res.headers_mut().insert(CONTENT_TYPE, ct);
    }
    let cache = if immutable {
        "public, max-age=31536000, immutable"
    } else {
        "no-cache"
    };
    res.headers_mut()
        .insert(CACHE_CONTROL, HeaderValue::from_static(cache));
    res
}

pub async fn serve_spa(State(state): State<GatewayState>, uri: Uri) -> Response {
    let req_path = uri.path();
    if req_path.starts_with("/api/")
        || req_path.starts_with("/opds/")
        || req_path.starts_with("/rest/")
        || req_path == "/rest"
    {
        return (StatusCode::NOT_FOUND, "Not Found").into_response();
    }

    let rel_opt = normalize_asset_path(req_path);
    let rel_str = rel_opt
        .as_ref()
        .map(|p| {
            p.components()
                .map(|c| c.as_os_str().to_string_lossy().into_owned())
                .collect::<Vec<_>>()
                .join("/")
        })
        .unwrap_or_default();

    let is_hashed_asset = rel_str.starts_with("assets/");

    if let Some(ref ext_dir) = state.external_web_dir {
        if !rel_str.is_empty() {
            let candidate = ext_dir.join(&rel_str);
            if candidate.is_file()
                && let Ok(bytes) = tokio::fs::read(&candidate).await
            {
                return asset_response(&rel_str, bytes, is_hashed_asset);
            }
        }
        let index_file = ext_dir.join("index.html");
        if index_file.is_file()
            && let Ok(bytes) = tokio::fs::read(&index_file).await
        {
            return asset_response("index.html", bytes, false);
        }
    }

    if !rel_str.is_empty()
        && let Some(embedded) = EmbeddedWebAssets::get(&rel_str)
    {
        return asset_response(&rel_str, embedded.data.into_owned(), is_hashed_asset);
    }

    if let Some(index) = EmbeddedWebAssets::get("index.html") {
        return asset_response("index.html", index.data.into_owned(), false);
    }

    Html(
        "<!doctype html><html><head><meta charset=\"utf-8\"><title>xmedia</title></head>\
         <body><h1>xmedia server is running</h1><p>Unified Web SPA assets are not built yet.</p></body></html>",
    )
    .into_response()
}

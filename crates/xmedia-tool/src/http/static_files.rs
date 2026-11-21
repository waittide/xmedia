use std::path::{Component, Path, PathBuf};

use axum::body::Body;
use axum::extract::State;
use axum::http::header::{CACHE_CONTROL, CONTENT_TYPE};
use axum::http::{HeaderValue, StatusCode, Uri};
use axum::response::{Html, IntoResponse, Response};
use rust_embed::RustEmbed;

use crate::http::ToolHttpState;

#[derive(RustEmbed)]
#[folder = "../../web/dist-tool"]
#[allow_missing = true]
struct EmbeddedToolWebAssets;

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

const TOOL_INDEX_CANDIDATES: [&str; 2] = ["tool.html", "index.html"];

fn asset_response(path_hint: &str, body: impl Into<Body>, immutable: bool) -> Response {
    let mime = mime_guess::from_path(path_hint).first_or_octet_stream();
    let mut res = (StatusCode::OK, body.into()).into_response();
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

pub async fn serve_tool_spa(State(state): State<ToolHttpState>, uri: Uri) -> Response {
    let req_path = uri.path();
    if req_path.starts_with("/api/") {
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
        if let Some((html_name, bytes)) = read_external_index(ext_dir).await {
            return asset_response(html_name, bytes, false);
        }
    }

    if !rel_str.is_empty()
        && let Some(embedded) = EmbeddedToolWebAssets::get(&rel_str)
    {
        return asset_response(&rel_str, embedded.data, is_hashed_asset);
    }

    for html_name in TOOL_INDEX_CANDIDATES {
        if let Some(index) = EmbeddedToolWebAssets::get(html_name) {
            return asset_response(html_name, index.data, false);
        }
    }

    Html(
        "<!doctype html><html><head><meta charset=\"utf-8\"><title>xmedia-tool</title></head>\
         <body><h1>xmedia-tool is running</h1><p>Frontend assets are not built yet.</p></body></html>",
    )
    .into_response()
}

async fn read_external_index(ext_dir: &Path) -> Option<(&'static str, Vec<u8>)> {
    for html_name in TOOL_INDEX_CANDIDATES {
        let html_file = ext_dir.join(html_name);
        if html_file.is_file()
            && let Ok(bytes) = tokio::fs::read(&html_file).await
        {
            return Some((html_name, bytes));
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn external_index_prefers_tool_html_then_falls_back_to_index_html() {
        let tmp = tempfile::TempDir::new().unwrap();
        std::fs::write(tmp.path().join("index.html"), b"<html>index</html>").unwrap();
        let (name, bytes) = read_external_index(tmp.path()).await.unwrap();
        assert_eq!(name, "index.html");
        assert_eq!(bytes, b"<html>index</html>");

        std::fs::write(tmp.path().join("tool.html"), b"<html>tool</html>").unwrap();
        let (name, bytes) = read_external_index(tmp.path()).await.unwrap();
        assert_eq!(name, "tool.html");
        assert_eq!(bytes, b"<html>tool</html>");
    }
}

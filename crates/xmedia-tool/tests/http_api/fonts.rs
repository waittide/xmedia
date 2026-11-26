use axum::body::{Body, to_bytes};
use axum::http::{Request, StatusCode, header};
use tempfile::TempDir;
use tower::ServiceExt;
use xmedia_tool::{XMediaToolApp, XMediaToolConfig};

fn build_multipart(boundary: &str, field: &str, filename: &str, bytes: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    out.extend_from_slice(
        format!(
            "--{boundary}\r\nContent-Disposition: form-data; name=\"{field}\"; filename=\"{filename}\"\r\nContent-Type: application/octet-stream\r\n\r\n"
        )
        .as_bytes(),
    );
    out.extend_from_slice(bytes);
    out.extend_from_slice(format!("\r\n--{boundary}--\r\n").as_bytes());
    out
}

#[tokio::test]
async fn tool_custom_fonts_api_upload_list_file_delete_and_csp_work() {
    let tmp = TempDir::new().unwrap();
    let app_instance = XMediaToolApp::init(XMediaToolConfig {
        data_dir: tmp.path().join("tool-data"),
        initial_roots: Vec::new(),
        password: None,
        ffmpeg_bin: "__missing_ffmpeg__".into(),
        external_web_dir: None,
    })
    .await
    .unwrap();
    let app = app_instance.router;

    // 1. Tool SPA CSP allows blob: in font-src
    let spa_res = app
        .clone()
        .oneshot(Request::builder().uri("/").body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(spa_res.status(), StatusCode::OK);
    let csp = spa_res
        .headers()
        .get(header::CONTENT_SECURITY_POLICY)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");
    assert!(
        csp.contains("font-src 'self' data: blob:"),
        "unexpected tool SPA CSP: {csp}"
    );

    // 2. Reject invalid magic bytes on POST /api/fonts
    let boundary = "ToolFontBoundary";
    let bad_res = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/fonts")
                .header(
                    header::CONTENT_TYPE,
                    format!("multipart/form-data; boundary={boundary}"),
                )
                .body(Body::from(build_multipart(
                    boundary,
                    "file",
                    "not-a-font.otf",
                    b"not-valid-font-bytes",
                )))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(bad_res.status(), StatusCode::UNSUPPORTED_MEDIA_TYPE);

    // 3. Upload OTF font on POST /api/fonts
    let otf_bytes = b"OTTO\x00\x00\x00\x00\x00\x00\x00\x00synthetic-otf";
    let upload_res = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/fonts")
                .header(
                    header::CONTENT_TYPE,
                    format!("multipart/form-data; boundary={boundary}"),
                )
                .body(Body::from(build_multipart(
                    boundary,
                    "file",
                    "SourceHanSerif-Regular.otf",
                    otf_bytes,
                )))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(upload_res.status(), StatusCode::CREATED);
    let created: serde_json::Value =
        serde_json::from_slice(&to_bytes(upload_res.into_body(), usize::MAX).await.unwrap())
            .unwrap();
    let arr = created.as_array().unwrap();
    assert_eq!(arr.len(), 1);
    let font_id = arr[0]["id"].as_str().unwrap().to_string();
    assert_eq!(arr[0]["name"], "SourceHanSerif-Regular");
    assert_eq!(arr[0]["format"], "otf");
    assert_eq!(arr[0]["css_format"], "opentype");
    assert_eq!(arr[0]["mime_type"], "font/otf");

    // 4. GET /api/fonts & GET /api/fonts/{id}/file
    let list_res = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/api/fonts")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(list_res.status(), StatusCode::OK);
    let listed: serde_json::Value =
        serde_json::from_slice(&to_bytes(list_res.into_body(), usize::MAX).await.unwrap()).unwrap();
    assert_eq!(listed.as_array().unwrap().len(), 1);

    // Ensure existing /api/v1/tool/status still works alongside /api/fonts
    let status_res = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/api/v1/tool/status")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(status_res.status(), StatusCode::OK);

    let file_res = app
        .clone()
        .oneshot(
            Request::builder()
                .uri(format!("/api/fonts/{font_id}/file"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(file_res.status(), StatusCode::OK);
    assert_eq!(
        file_res
            .headers()
            .get(header::CONTENT_TYPE)
            .and_then(|v| v.to_str().ok()),
        Some("font/otf")
    );
    assert_eq!(
        file_res
            .headers()
            .get(header::X_CONTENT_TYPE_OPTIONS)
            .and_then(|v| v.to_str().ok()),
        Some("nosniff")
    );
    assert_eq!(
        file_res
            .headers()
            .get(header::CACHE_CONTROL)
            .and_then(|v| v.to_str().ok()),
        Some("private, max-age=31536000, immutable")
    );
    let raw = to_bytes(file_res.into_body(), usize::MAX).await.unwrap();
    assert_eq!(raw.as_ref(), otf_bytes);

    // 5. DELETE /api/fonts/{id}
    let del_res = app
        .clone()
        .oneshot(
            Request::builder()
                .method("DELETE")
                .uri(format!("/api/fonts/{font_id}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(del_res.status(), StatusCode::OK);
    let del_json: serde_json::Value =
        serde_json::from_slice(&to_bytes(del_res.into_body(), usize::MAX).await.unwrap()).unwrap();
    assert_eq!(del_json["ok"], true);
}

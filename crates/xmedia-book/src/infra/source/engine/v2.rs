use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use std::time::Duration;

use xmedia_source::http::{HttpClientConfig, resolve_relative_url};
use xmedia_source::model::{SourceCallError, SourceCallOutput, SourceRuntimeState};
use xmedia_source::plugin_v2::{PluginV2Manifest, PluginV2Runner};

use super::super::content_clean::clean_chapter_html_to_text;
use crate::domain::online::{
    BookJsContext, ChapterJsContext, OnlineBookDetail, OnlineChapterContent, OnlineChapterItem,
    SearchBookResult,
};

fn val_str(obj: &serde_json::Map<String, serde_json::Value>, keys: &[&str]) -> Option<String> {
    for k in keys {
        if let Some(v) = obj.get(*k) {
            match v {
                serde_json::Value::String(s) if !s.trim().is_empty() => {
                    return Some(s.trim().to_string());
                }
                serde_json::Value::Number(n) => return Some(n.to_string()),
                _ => {}
            }
        }
    }
    None
}

fn book_ctx_json(ctx: &BookJsContext) -> serde_json::Value {
    serde_json::json!({
        "name": ctx.name,
        "author": ctx.author,
        "bookUrl": ctx.book_url,
        "tocUrl": ctx.toc_url,
        "coverUrl": ctx.cover_url,
        "intro": ctx.intro,
    })
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn search_v2_sync(
    manifest: &PluginV2Manifest,
    js_code: &str,
    initial_state: SourceRuntimeState,
    http_config: &HttpClientConfig,
    js_timeout: Duration,
    keyword: &str,
    page: u32,
    op_timeout: Duration,
    cancel_flag: Arc<AtomicBool>,
) -> Result<SourceCallOutput<Vec<SearchBookResult>>, SourceCallError> {
    let out = PluginV2Runner::call_method_sync(
        js_code,
        manifest,
        initial_state,
        http_config,
        js_timeout,
        op_timeout,
        cancel_flag,
        &["search"],
        &[serde_json::json!({
            "keyword": keyword,
            "page": page.max(1),
        })],
    )?;

    let arr = match &out.data {
        serde_json::Value::Array(a) => Some(a),
        serde_json::Value::Object(m) => m
            .get("books")
            .or_else(|| m.get("items"))
            .or_else(|| m.get("results"))
            .and_then(|v| v.as_array()),
        _ => None,
    };
    let mut results = Vec::new();
    if let Some(items) = arr {
        for item in items {
            let Some(obj) = item.as_object() else {
                continue;
            };
            let Some(name) = val_str(obj, &["name", "title"]) else {
                continue;
            };
            let Some(raw_url) = val_str(obj, &["bookUrl", "book_url", "url"]) else {
                continue;
            };
            let book_url = resolve_relative_url(&raw_url, &manifest.base_url);
            let author = val_str(obj, &["author"]).unwrap_or_default();
            let cover_url = val_str(obj, &["coverUrl", "cover_url"])
                .map(|u| resolve_relative_url(&u, &manifest.base_url));
            results.push(SearchBookResult {
                name,
                author,
                book_url,
                cover_url,
                intro: val_str(obj, &["intro", "description"]),
                kind: val_str(obj, &["kind", "category"]),
                last_chapter: val_str(obj, &["lastChapter", "last_chapter"]),
                word_count: val_str(obj, &["wordCount", "word_count"]),
            });
        }
    }

    Ok(SourceCallOutput {
        data: results,
        state: out.state,
        diagnostics: out.diagnostics,
    })
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn fetch_book_info_v2_sync(
    manifest: &PluginV2Manifest,
    js_code: &str,
    initial_state: SourceRuntimeState,
    http_config: &HttpClientConfig,
    js_timeout: Duration,
    book_url: &str,
    fallback_hint: Option<&BookJsContext>,
    op_timeout: Duration,
    cancel_flag: Arc<AtomicBool>,
) -> Result<SourceCallOutput<OnlineBookDetail>, SourceCallError> {
    if !manifest.has_detail {
        return Ok(SourceCallOutput {
            data: OnlineBookDetail {
                name: fallback_hint.map(|h| h.name.clone()).unwrap_or_default(),
                author: fallback_hint.map(|h| h.author.clone()).unwrap_or_default(),
                book_url: book_url.to_string(),
                toc_url: book_url.to_string(),
                cover_url: fallback_hint.and_then(|h| h.cover_url.clone()),
                intro: fallback_hint.and_then(|h| h.intro.clone()),
                kind: None,
                last_chapter: None,
                word_count: None,
            },
            state: initial_state,
            diagnostics: Default::default(),
        });
    }

    let hint_json = fallback_hint
        .map(book_ctx_json)
        .unwrap_or_else(|| serde_json::json!({ "bookUrl": book_url }));
    let out = PluginV2Runner::call_any_method_sync(
        js_code,
        manifest,
        initial_state,
        http_config,
        js_timeout,
        op_timeout,
        cancel_flag,
        &[&["getBookInfo"], &["getDetail"]],
        &[serde_json::json!({
            "bookUrl": book_url,
            "url": book_url,
            "book": hint_json,
        })],
    )?;

    let obj = out.data.as_object();
    let name = obj
        .and_then(|o| val_str(o, &["name", "title"]))
        .or_else(|| fallback_hint.map(|h| h.name.clone()))
        .unwrap_or_default();
    let author = obj
        .and_then(|o| val_str(o, &["author"]))
        .or_else(|| fallback_hint.map(|h| h.author.clone()))
        .unwrap_or_default();
    let toc_url = obj
        .and_then(|o| val_str(o, &["tocUrl", "toc_url"]))
        .map(|u| resolve_relative_url(&u, book_url))
        .unwrap_or_else(|| book_url.to_string());
    let cover_url = obj
        .and_then(|o| val_str(o, &["coverUrl", "cover_url"]))
        .map(|u| resolve_relative_url(&u, book_url))
        .or_else(|| fallback_hint.and_then(|h| h.cover_url.clone()));
    let intro = obj
        .and_then(|o| val_str(o, &["intro", "description"]))
        .or_else(|| fallback_hint.and_then(|h| h.intro.clone()));

    Ok(SourceCallOutput {
        data: OnlineBookDetail {
            name,
            author,
            book_url: book_url.to_string(),
            toc_url,
            cover_url,
            intro,
            kind: obj.and_then(|o| val_str(o, &["kind", "category"])),
            last_chapter: obj.and_then(|o| val_str(o, &["lastChapter", "last_chapter"])),
            word_count: obj.and_then(|o| val_str(o, &["wordCount", "word_count"])),
        },
        state: out.state,
        diagnostics: out.diagnostics,
    })
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn fetch_toc_v2_sync(
    manifest: &PluginV2Manifest,
    js_code: &str,
    initial_state: SourceRuntimeState,
    http_config: &HttpClientConfig,
    js_timeout: Duration,
    toc_url: &str,
    book_ctx: &BookJsContext,
    op_timeout: Duration,
    cancel_flag: Arc<AtomicBool>,
) -> Result<SourceCallOutput<Vec<OnlineChapterItem>>, SourceCallError> {
    let out = PluginV2Runner::call_any_method_sync(
        js_code,
        manifest,
        initial_state,
        http_config,
        js_timeout,
        op_timeout,
        cancel_flag,
        &[&["getToc"], &["getChapters"]],
        &[serde_json::json!({
            "tocUrl": toc_url,
            "url": toc_url,
            "book": book_ctx_json(book_ctx),
        })],
    )?;

    let arr = match &out.data {
        serde_json::Value::Array(a) => Some(a),
        serde_json::Value::Object(m) => m
            .get("chapters")
            .or_else(|| m.get("items"))
            .and_then(|v| v.as_array()),
        _ => None,
    };
    let mut chapters = Vec::new();
    if let Some(items) = arr {
        for item in items {
            let Some(obj) = item.as_object() else {
                continue;
            };
            let Some(title) = val_str(obj, &["title", "name", "chapterName"]) else {
                continue;
            };
            let raw_url = val_str(obj, &["url", "chapterUrl", "chapter_url"]).unwrap_or_default();
            let is_volume = obj
                .get("isVolume")
                .or_else(|| obj.get("is_volume"))
                .and_then(|v| v.as_bool())
                .unwrap_or(raw_url.is_empty());
            let resolved = if raw_url.is_empty() {
                String::new()
            } else {
                resolve_relative_url(&raw_url, toc_url)
            };
            chapters.push(OnlineChapterItem {
                index: chapters.len(),
                title,
                url: resolved,
                update_time: val_str(obj, &["updateTime", "update_time"]),
                is_volume,
            });
        }
    }

    Ok(SourceCallOutput {
        data: chapters,
        state: out.state,
        diagnostics: out.diagnostics,
    })
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn fetch_chapter_content_v2_sync(
    manifest: &PluginV2Manifest,
    js_code: &str,
    initial_state: SourceRuntimeState,
    http_config: &HttpClientConfig,
    js_timeout: Duration,
    chapter_url: &str,
    book_ctx: &BookJsContext,
    chapter_ctx: &ChapterJsContext,
    op_timeout: Duration,
    cancel_flag: Arc<AtomicBool>,
) -> Result<SourceCallOutput<OnlineChapterContent>, SourceCallError> {
    let out = PluginV2Runner::call_any_method_sync(
        js_code,
        manifest,
        initial_state,
        http_config,
        js_timeout,
        op_timeout,
        cancel_flag,
        &[&["getChapterContent"], &["getContent"]],
        &[serde_json::json!({
            "chapterUrl": chapter_url,
            "url": chapter_url,
            "book": book_ctx_json(book_ctx),
            "chapter": {
                "index": chapter_ctx.index,
                "title": chapter_ctx.title,
                "url": chapter_ctx.url,
            },
        })],
    )?;

    let (title, content, raw_html) = match &out.data {
        serde_json::Value::String(s) => {
            let cleaned = clean_chapter_html_to_text(s);
            let text = if cleaned.trim().is_empty() {
                s.clone()
            } else {
                cleaned
            };
            (chapter_ctx.title.clone(), text, s.clone())
        }
        serde_json::Value::Object(obj) => {
            let t = val_str(obj, &["title"]).unwrap_or_else(|| chapter_ctx.title.clone());
            let raw_h = val_str(obj, &["rawHtml", "raw_html", "html"]).unwrap_or_default();
            let c = val_str(obj, &["content", "text"]).unwrap_or_else(|| {
                if !raw_h.is_empty() {
                    clean_chapter_html_to_text(&raw_h)
                } else {
                    String::new()
                }
            });
            let html = if raw_h.is_empty() { c.clone() } else { raw_h };
            (t, c, html)
        }
        _ => (chapter_ctx.title.clone(), String::new(), String::new()),
    };

    Ok(SourceCallOutput {
        data: OnlineChapterContent {
            title,
            content,
            raw_html,
        },
        state: out.state,
        diagnostics: out.diagnostics,
    })
}

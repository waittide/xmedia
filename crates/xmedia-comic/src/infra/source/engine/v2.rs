use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use std::time::Duration;

use xmedia_source::http::resolve_relative_url;
use xmedia_source::model::{SourceCallError, SourceCallOutput, SourceRuntimeState};
use xmedia_source::plugin_v2::{PluginV2Manifest, PluginV2Runner};

use super::ComicSourceEngine;
use crate::domain::online::{
    ComicChapterJsContext, ComicJsContext, ComicSourceSpec, OnlineComicChapter, OnlineComicDetail,
    OnlineComicItem,
};
use crate::domain::series::ReadingDirection;

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

fn comic_ctx_json(ctx: &ComicJsContext) -> serde_json::Value {
    serde_json::json!({
        "title": ctx.name,
        "name": ctx.name,
        "author": ctx.author,
        "comicUrl": ctx.book_url,
        "tocUrl": ctx.toc_url,
        "coverUrl": ctx.cover_url,
        "intro": ctx.intro,
        "readingDirection": ctx.reading_direction,
    })
}

fn parse_v2_comic_list(val: &serde_json::Value, base_url: &str) -> Vec<OnlineComicItem> {
    let arr = match val {
        serde_json::Value::Array(a) => Some(a),
        serde_json::Value::Object(m) => m
            .get("comics")
            .or_else(|| m.get("items"))
            .or_else(|| m.get("results"))
            .and_then(|v| v.as_array()),
        _ => None,
    };
    let mut out = Vec::new();
    if let Some(items) = arr {
        for item in items {
            let Some(obj) = item.as_object() else {
                continue;
            };
            let Some(title) = val_str(obj, &["title", "name"]) else {
                continue;
            };
            let Some(raw_url) = val_str(obj, &["comicUrl", "comic_url", "bookUrl", "url"]) else {
                continue;
            };
            let comic_url = resolve_relative_url(&raw_url, base_url);
            let cover_url = val_str(obj, &["coverUrl", "cover_url"])
                .map(|u| resolve_relative_url(&u, base_url));
            out.push(OnlineComicItem {
                title,
                author: val_str(obj, &["author"]).unwrap_or_default(),
                comic_url,
                cover_url,
                intro: val_str(obj, &["intro", "description"]),
                kind: val_str(obj, &["kind", "category"]),
                last_chapter: val_str(obj, &["lastChapter", "last_chapter"]),
                word_count: val_str(obj, &["wordCount", "word_count"]),
            });
        }
    }
    out
}

impl ComicSourceEngine {
    #[allow(clippy::too_many_arguments)]
    pub(super) fn search_v2_sync(
        &self,
        manifest: &PluginV2Manifest,
        js_code: &str,
        initial_state: SourceRuntimeState,
        keyword: &str,
        page: u32,
        op_timeout: Duration,
        cancel_flag: Arc<AtomicBool>,
    ) -> Result<SourceCallOutput<Vec<OnlineComicItem>>, SourceCallError> {
        let out = PluginV2Runner::call_method_sync(
            js_code,
            manifest,
            initial_state,
            &self.http_config,
            self.js_timeout,
            op_timeout,
            cancel_flag,
            &["search"],
            &[serde_json::json!({
                "keyword": keyword,
                "page": page.max(1),
            })],
        )?;
        Ok(SourceCallOutput {
            data: parse_v2_comic_list(&out.data, &manifest.base_url),
            state: out.state,
            diagnostics: out.diagnostics,
        })
    }

    #[allow(clippy::too_many_arguments)]
    pub(super) fn explore_v2_sync(
        &self,
        manifest: &PluginV2Manifest,
        js_code: &str,
        initial_state: SourceRuntimeState,
        explore_url: &str,
        page: u32,
        op_timeout: Duration,
        cancel_flag: Arc<AtomicBool>,
    ) -> Result<SourceCallOutput<Vec<OnlineComicItem>>, SourceCallError> {
        let out = PluginV2Runner::call_any_method_sync(
            js_code,
            manifest,
            initial_state,
            &self.http_config,
            self.js_timeout,
            op_timeout,
            cancel_flag,
            &[&["explore"], &["browse"]],
            &[serde_json::json!({
                "categoryId": explore_url.trim(),
                "exploreUrl": explore_url.trim(),
                "page": page.max(1),
            })],
        )?;
        Ok(SourceCallOutput {
            data: parse_v2_comic_list(&out.data, &manifest.base_url),
            state: out.state,
            diagnostics: out.diagnostics,
        })
    }

    #[allow(clippy::too_many_arguments)]
    pub(super) fn fetch_detail_v2_sync(
        &self,
        source: &ComicSourceSpec,
        manifest: &PluginV2Manifest,
        js_code: &str,
        initial_state: SourceRuntimeState,
        comic_url: &str,
        fallback_hint: Option<&ComicJsContext>,
        op_timeout: Duration,
        cancel_flag: Arc<AtomicBool>,
    ) -> Result<SourceCallOutput<OnlineComicDetail>, SourceCallError> {
        if !manifest.has_detail {
            return Ok(SourceCallOutput {
                data: OnlineComicDetail {
                    title: fallback_hint.map(|h| h.name.clone()).unwrap_or_default(),
                    author: fallback_hint.map(|h| h.author.clone()).unwrap_or_default(),
                    comic_url: comic_url.to_string(),
                    toc_url: comic_url.to_string(),
                    cover_url: fallback_hint.and_then(|h| h.cover_url.clone()),
                    intro: fallback_hint.and_then(|h| h.intro.clone()),
                    kind: None,
                    status: None,
                    last_chapter: None,
                    word_count: None,
                    reading_direction: Some(source.effective_reading_direction()),
                },
                state: initial_state,
                diagnostics: Default::default(),
            });
        }

        let hint_json = fallback_hint
            .map(comic_ctx_json)
            .unwrap_or_else(|| serde_json::json!({ "comicUrl": comic_url }));
        let out = PluginV2Runner::call_any_method_sync(
            js_code,
            manifest,
            initial_state,
            &self.http_config,
            self.js_timeout,
            op_timeout,
            cancel_flag,
            &[&["getDetail"], &["getComicDetail"]],
            &[serde_json::json!({
                "comicUrl": comic_url,
                "url": comic_url,
                "comic": hint_json,
            })],
        )?;

        let obj = out.data.as_object();
        let title = obj
            .and_then(|o| val_str(o, &["title", "name"]))
            .or_else(|| fallback_hint.map(|h| h.name.clone()))
            .unwrap_or_default();
        let author = obj
            .and_then(|o| val_str(o, &["author"]))
            .or_else(|| fallback_hint.map(|h| h.author.clone()))
            .unwrap_or_default();
        let toc_url = obj
            .and_then(|o| val_str(o, &["tocUrl", "toc_url"]))
            .map(|u| resolve_relative_url(&u, comic_url))
            .unwrap_or_else(|| comic_url.to_string());
        let cover_url = obj
            .and_then(|o| val_str(o, &["coverUrl", "cover_url"]))
            .map(|u| resolve_relative_url(&u, comic_url))
            .or_else(|| fallback_hint.and_then(|h| h.cover_url.clone()));
        let intro = obj
            .and_then(|o| val_str(o, &["intro", "description"]))
            .or_else(|| fallback_hint.and_then(|h| h.intro.clone()));
        let reading_direction = obj
            .and_then(|o| val_str(o, &["readingDirection", "reading_direction"]))
            .and_then(|s| s.parse::<ReadingDirection>().ok())
            .or_else(|| Some(source.effective_reading_direction()));

        Ok(SourceCallOutput {
            data: OnlineComicDetail {
                title,
                author,
                comic_url: comic_url.to_string(),
                toc_url,
                cover_url,
                intro,
                kind: obj.and_then(|o| val_str(o, &["kind", "category"])),
                status: obj.and_then(|o| val_str(o, &["status"])),
                last_chapter: obj.and_then(|o| val_str(o, &["lastChapter", "last_chapter"])),
                word_count: obj.and_then(|o| val_str(o, &["wordCount", "word_count"])),
                reading_direction,
            },
            state: out.state,
            diagnostics: out.diagnostics,
        })
    }

    #[allow(clippy::too_many_arguments)]
    pub(super) fn fetch_chapters_v2_sync(
        &self,
        manifest: &PluginV2Manifest,
        js_code: &str,
        initial_state: SourceRuntimeState,
        toc_url: &str,
        comic_ctx: &ComicJsContext,
        op_timeout: Duration,
        cancel_flag: Arc<AtomicBool>,
    ) -> Result<SourceCallOutput<Vec<OnlineComicChapter>>, SourceCallError> {
        let out = PluginV2Runner::call_any_method_sync(
            js_code,
            manifest,
            initial_state,
            &self.http_config,
            self.js_timeout,
            op_timeout,
            cancel_flag,
            &[&["getChapters"], &["getToc"]],
            &[serde_json::json!({
                "tocUrl": toc_url,
                "url": toc_url,
                "comic": comic_ctx_json(comic_ctx),
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
                let raw_url =
                    val_str(obj, &["url", "chapterUrl", "chapter_url"]).unwrap_or_default();
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
                chapters.push(OnlineComicChapter {
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
    pub(super) fn fetch_pages_v2_sync(
        &self,
        manifest: &PluginV2Manifest,
        js_code: &str,
        initial_state: SourceRuntimeState,
        chapter_url: &str,
        comic_ctx: &ComicJsContext,
        chapter_ctx: &ComicChapterJsContext,
        op_timeout: Duration,
        cancel_flag: Arc<AtomicBool>,
    ) -> Result<SourceCallOutput<Vec<String>>, SourceCallError> {
        let out = PluginV2Runner::call_any_method_sync(
            js_code,
            manifest,
            initial_state,
            &self.http_config,
            self.js_timeout,
            op_timeout,
            cancel_flag,
            &[&["getPages"], &["getContent"]],
            &[serde_json::json!({
                "chapterUrl": chapter_url,
                "url": chapter_url,
                "comic": comic_ctx_json(comic_ctx),
                "chapter": {
                    "index": chapter_ctx.index,
                    "title": chapter_ctx.title,
                    "url": chapter_ctx.url,
                },
            })],
        )?;

        let arr = match &out.data {
            serde_json::Value::Array(a) => Some(a),
            serde_json::Value::Object(m) => m
                .get("pages")
                .or_else(|| m.get("images"))
                .or_else(|| m.get("items"))
                .and_then(|v| v.as_array()),
            _ => None,
        };
        let mut pages = Vec::new();
        if let Some(items) = arr {
            for item in items {
                let raw = match item {
                    serde_json::Value::String(s) if !s.trim().is_empty() => {
                        Some(s.trim().to_string())
                    }
                    serde_json::Value::Object(obj) => val_str(obj, &["url", "imageUrl", "src"]),
                    _ => None,
                };
                if let Some(u) = raw {
                    pages.push(resolve_relative_url(&u, chapter_url));
                }
            }
        }

        Ok(SourceCallOutput {
            data: pages,
            state: out.state,
            diagnostics: out.diagnostics,
        })
    }
}

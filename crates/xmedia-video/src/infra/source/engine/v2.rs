use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use std::time::Duration;

use xmedia_source::http::{build_sanitized_headers, resolve_relative_url};
use xmedia_source::model::{SourceCallError, SourceCallOutput, SourceRuntimeState};
use xmedia_source::plugin_v2::{PluginV2Manifest, PluginV2Runner};

use super::rules::is_http_url;
use super::{VideoSourceEngine, applecms};
use crate::domain::online::{
    OnlineVideoCategory, OnlineVideoDetail, OnlineVideoEpisode, OnlineVideoPlayGroup,
    OnlineVideoPlayResult, OnlineVideoSubtitle, OnlineVideoSummary, VideoBrowseOutput,
    VideoSourceSpec,
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

fn parse_v2_video_items(val: &serde_json::Value, base_url: &str) -> Vec<OnlineVideoSummary> {
    let arr = match val {
        serde_json::Value::Array(a) => Some(a),
        serde_json::Value::Object(m) => m
            .get("items")
            .or_else(|| m.get("videos"))
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
            let vod_id = val_str(obj, &["vodId", "vod_id", "id"]).unwrap_or_default();
            let detail_url = val_str(obj, &["detailUrl", "detail_url", "url"])
                .map(|u| resolve_relative_url(&u, base_url))
                .unwrap_or_default();
            if vod_id.is_empty() && detail_url.is_empty() {
                continue;
            }
            let cover_url = val_str(obj, &["coverUrl", "cover_url", "picUrl"])
                .map(|u| resolve_relative_url(&u, base_url));
            out.push(OnlineVideoSummary {
                vod_id,
                title,
                cover_url,
                proxy_cover_url: None,
                remarks: val_str(obj, &["remarks", "note"]),
                year: val_str(obj, &["year"]),
                area: val_str(obj, &["area"]),
                type_name: val_str(obj, &["typeName", "type_name", "category"]),
                detail_url,
            });
        }
    }
    out
}

impl VideoSourceEngine {
    pub(super) fn fetch_categories_v2_sync(
        &self,
        manifest: &PluginV2Manifest,
        js_code: &str,
        initial_state: SourceRuntimeState,
        op_timeout: Duration,
        cancel_flag: Arc<AtomicBool>,
    ) -> Result<SourceCallOutput<Vec<OnlineVideoCategory>>, SourceCallError> {
        let fallback_cats = || -> Vec<OnlineVideoCategory> {
            manifest
                .explore_categories
                .iter()
                .map(|c| OnlineVideoCategory {
                    id: c.id.clone(),
                    name: c.title.clone(),
                    parent_id: None,
                })
                .collect()
        };

        if !PluginV2Runner::has_method_sync(js_code, &["getCategories"]) {
            return Ok(SourceCallOutput {
                data: fallback_cats(),
                state: initial_state,
                diagnostics: Default::default(),
            });
        }

        let out = PluginV2Runner::call_method_sync(
            js_code,
            manifest,
            initial_state,
            &self.http_config,
            self.js_timeout,
            op_timeout,
            cancel_flag,
            &["getCategories"],
            &[],
        )?;

        let cats = out
            .data
            .as_array()
            .map(|arr| {
                arr.iter()
                    .filter_map(|item| {
                        let obj = item.as_object()?;
                        let id = val_str(obj, &["id"])?;
                        let name = val_str(obj, &["name", "title"])?;
                        Some(OnlineVideoCategory {
                            id,
                            name,
                            parent_id: val_str(obj, &["parentId", "parent_id"]),
                        })
                    })
                    .collect::<Vec<_>>()
            })
            .unwrap_or_else(fallback_cats);

        Ok(SourceCallOutput {
            data: cats,
            state: out.state,
            diagnostics: out.diagnostics,
        })
    }

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
    ) -> Result<SourceCallOutput<Vec<OnlineVideoSummary>>, SourceCallError> {
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
            data: parse_v2_video_items(&out.data, &manifest.base_url),
            state: out.state,
            diagnostics: out.diagnostics,
        })
    }

    #[allow(clippy::too_many_arguments)]
    pub(super) fn browse_v2_sync(
        &self,
        manifest: &PluginV2Manifest,
        js_code: &str,
        initial_state: SourceRuntimeState,
        category_id: Option<&str>,
        page: u32,
        op_timeout: Duration,
        cancel_flag: Arc<AtomicBool>,
    ) -> Result<SourceCallOutput<VideoBrowseOutput>, SourceCallError> {
        let page_num = page.max(1);
        let cat = category_id
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .or_else(|| manifest.explore_categories.first().map(|c| c.id.as_str()));

        let out = PluginV2Runner::call_any_method_sync(
            js_code,
            manifest,
            initial_state,
            &self.http_config,
            self.js_timeout,
            op_timeout,
            cancel_flag,
            &[&["browse"], &["explore"]],
            &[serde_json::json!({
                "categoryId": cat,
                "page": page_num,
            })],
        )?;

        let items = parse_v2_video_items(&out.data, &manifest.base_url);
        let obj = out.data.as_object();
        let total = obj.and_then(|o| o.get("total")).and_then(|v| v.as_u64());
        let page_count = obj
            .and_then(|o| o.get("pageCount").or_else(|| o.get("page_count")))
            .and_then(|v| v.as_u64())
            .map(|n| n as u32);

        Ok(SourceCallOutput {
            data: VideoBrowseOutput {
                items,
                total,
                page: page_num,
                page_count,
            },
            state: out.state,
            diagnostics: out.diagnostics,
        })
    }

    #[allow(clippy::too_many_arguments)]
    pub(super) fn fetch_detail_v2_sync(
        &self,
        manifest: &PluginV2Manifest,
        js_code: &str,
        initial_state: SourceRuntimeState,
        video_id: Option<&str>,
        detail_url: Option<&str>,
        op_timeout: Duration,
        cancel_flag: Arc<AtomicBool>,
    ) -> Result<SourceCallOutput<OnlineVideoDetail>, SourceCallError> {
        let out = PluginV2Runner::call_method_sync(
            js_code,
            manifest,
            initial_state,
            &self.http_config,
            self.js_timeout,
            op_timeout,
            cancel_flag,
            &["getDetail"],
            &[serde_json::json!({
                "vodId": video_id,
                "id": video_id,
                "detailUrl": detail_url,
            })],
        )?;

        let obj = out.data.as_object().ok_or_else(|| {
            SourceCallError::new(
                "getDetail 未返回有效的影视详情对象",
                out.diagnostics.clone(),
            )
        })?;
        let base = detail_url.unwrap_or(&manifest.base_url);
        let id = val_str(obj, &["id", "vodId", "vod_id"])
            .or_else(|| video_id.map(ToString::to_string))
            .unwrap_or_default();
        let title = val_str(obj, &["title", "name"])
            .ok_or_else(|| SourceCallError::new("影视详情缺少 title", out.diagnostics.clone()))?;
        let cover_url =
            val_str(obj, &["coverUrl", "cover_url"]).map(|u| resolve_relative_url(&u, base));

        let mut play_groups = Vec::new();
        if let Some(groups_arr) = obj
            .get("playGroups")
            .or_else(|| obj.get("play_groups"))
            .and_then(|v| v.as_array())
        {
            for g in groups_arr {
                let Some(g_obj) = g.as_object() else {
                    continue;
                };
                let g_name =
                    val_str(g_obj, &["name", "title"]).unwrap_or_else(|| "默认线路".into());
                let mut episodes = Vec::new();
                if let Some(eps_arr) = g_obj.get("episodes").and_then(|v| v.as_array()) {
                    for ep in eps_arr {
                        let Some(ep_obj) = ep.as_object() else {
                            continue;
                        };
                        let ep_name = val_str(ep_obj, &["name", "title"])
                            .unwrap_or_else(|| format!("第{}集", episodes.len() + 1));
                        let Some(raw_play) = val_str(ep_obj, &["playUrl", "play_url", "url"])
                        else {
                            continue;
                        };
                        episodes.push(OnlineVideoEpisode {
                            index: episodes.len(),
                            name: ep_name,
                            play_url: resolve_relative_url(&raw_play, base),
                        });
                    }
                }
                if !episodes.is_empty() {
                    play_groups.push(OnlineVideoPlayGroup {
                        name: g_name,
                        episodes,
                    });
                }
            }
        }

        Ok(SourceCallOutput {
            data: OnlineVideoDetail {
                id,
                title,
                cover_url,
                proxy_cover_url: None,
                intro: val_str(obj, &["intro", "description"]),
                year: val_str(obj, &["year"]),
                area: val_str(obj, &["area"]),
                actors: val_str(obj, &["actors"]),
                director: val_str(obj, &["director"]),
                remarks: val_str(obj, &["remarks"]),
                play_groups,
            },
            state: out.state,
            diagnostics: out.diagnostics,
        })
    }

    #[allow(clippy::too_many_arguments)]
    pub(super) fn resolve_play_v2_sync(
        &self,
        source: &VideoSourceSpec,
        manifest: &PluginV2Manifest,
        js_code: &str,
        initial_state: SourceRuntimeState,
        play_url: &str,
        group_name: Option<&str>,
        op_timeout: Duration,
        cancel_flag: Arc<AtomicBool>,
    ) -> Result<SourceCallOutput<OnlineVideoPlayResult>, SourceCallError> {
        let base_url = source.effective_source_url();
        let effective_media_headers = source.build_media_headers(base_url, None);
        let headers_applied = !build_sanitized_headers(&effective_media_headers).is_empty();
        let resolved_input = resolve_relative_url(play_url.trim(), base_url);

        if !manifest.has_stream {
            if !is_http_url(&resolved_input) {
                return Err(SourceCallError::new(
                    "未能解析出有效的 HTTP(S) 视频流地址",
                    Default::default(),
                ));
            }
            let stype = applecms::infer_stream_type(&resolved_input).to_string();
            return Ok(SourceCallOutput {
                data: OnlineVideoPlayResult {
                    stream_url: resolved_input,
                    proxy_stream_url: String::new(),
                    stream_type: stype,
                    headers_applied,
                    subtitles: Vec::new(),
                },
                state: initial_state,
                diagnostics: Default::default(),
            });
        }

        let out = PluginV2Runner::call_any_method_sync(
            js_code,
            manifest,
            initial_state,
            &self.http_config,
            self.js_timeout,
            op_timeout,
            cancel_flag,
            &[&["getPlayInfo"], &["getStream"]],
            &[serde_json::json!({
                "playUrl": resolved_input,
                "url": resolved_input,
                "groupName": group_name,
            })],
        )?;

        let (raw_stream, stype_opt, subtitles) = match &out.data {
            serde_json::Value::String(s) => (s.trim().to_string(), None, Vec::new()),
            serde_json::Value::Object(obj) => {
                let u = val_str(obj, &["streamUrl", "stream_url", "url"]).unwrap_or_default();
                let st = val_str(obj, &["streamType", "stream_type", "format"]);
                let subs = obj
                    .get("subtitles")
                    .and_then(|v| v.as_array())
                    .map(|arr| {
                        arr.iter()
                            .filter_map(|item| {
                                let so = item.as_object()?;
                                let sub_url = val_str(so, &["url"])?;
                                Some(OnlineVideoSubtitle {
                                    label: val_str(so, &["label", "name"])
                                        .unwrap_or_else(|| "字幕".into()),
                                    lang: val_str(so, &["lang"]),
                                    url: resolve_relative_url(&sub_url, base_url),
                                    proxy_url: None,
                                })
                            })
                            .collect::<Vec<_>>()
                    })
                    .unwrap_or_default();
                (u, st, subs)
            }
            _ => (String::new(), None, Vec::new()),
        };

        let final_stream = resolve_relative_url(&raw_stream, base_url);
        if !is_http_url(&final_stream) {
            return Err(SourceCallError::new(
                "未能解析出有效的 HTTP(S) 视频流地址",
                out.diagnostics,
            ));
        }
        let stream_type =
            stype_opt.unwrap_or_else(|| applecms::infer_stream_type(&final_stream).to_string());

        Ok(SourceCallOutput {
            data: OnlineVideoPlayResult {
                stream_url: final_stream,
                proxy_stream_url: String::new(),
                stream_type,
                headers_applied,
                subtitles,
            },
            state: out.state,
            diagnostics: out.diagnostics,
        })
    }
}

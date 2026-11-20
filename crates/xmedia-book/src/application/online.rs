use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, SystemTime};

use bytes::Bytes;
use jiff::Timestamp;
use md5::{Digest, Md5};
use serde::{Deserialize, Serialize};
use toasty::Db;
use tokio::sync::RwLock;
use uuid::Uuid;
use xmedia_core::domain::error::{DomainError, DomainResult};
use xmedia_core::domain::user::{User, UserId};

use crate::application::catalog::BookCatalogService;
use crate::infra::persistence::models::{BookOnlineShelfModel, BookSourceRecordModel};
use crate::source::BookSourceEngine;
use crate::source::http::{DEFAULT_WEBVIEW_UA, HttpClientConfig, validate_target_url};
use crate::source::model::{
    BookJsContext, ChapterJsContext, ExecutionDiagnostics, LegadoBookSource, LoginUiItem,
    OnlineBookDetail, OnlineChapterContent, OnlineChapterItem, SearchBookResult,
    SourceRuntimeState,
};
use crate::source::packager::{
    PackBookMetadata, PackChapterInput, package_chapters_to_epub, package_chapters_to_txt,
};

fn db_err(err: impl std::fmt::Display) -> DomainError {
    DomainError::Persistence(err.to_string())
}

fn require_admin(actor: &User) -> DomainResult<()> {
    if actor.role.is_admin() {
        Ok(())
    } else {
        Err(DomainError::Forbidden("需要管理员权限管理书源".into()))
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BookSourceSummaryDto {
    pub id: Uuid,
    pub name: String,
    pub source_url: String,
    pub group_name: String,
    pub comment: Option<String>,
    pub enabled: bool,
    pub weight: i64,
    pub source_type: i64,
    pub has_login_url: bool,
    pub login_ui: Vec<LoginUiItem>,
    pub login_info: HashMap<String, String>,
    pub has_variable: bool,
    pub variable_preview: String,
    pub cookie_hosts: Vec<String>,
    pub last_test_ok: Option<bool>,
    pub last_test_msg: Option<String>,
    pub updated_at: Timestamp,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ImportBookSourcesReportDto {
    pub imported_count: usize,
    pub updated_count: usize,
    pub sources: Vec<BookSourceSummaryDto>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct UpdateBookSourceInput {
    pub enabled: Option<bool>,
    pub weight: Option<i64>,
    pub group_name: Option<String>,
    pub raw_json: Option<String>,
    pub variable: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LoginActionResponseDto {
    pub result: String,
    pub diagnostics: ExecutionDiagnostics,
    pub source: BookSourceSummaryDto,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SourceTestReportDto {
    pub ok: bool,
    pub keyword: String,
    pub result_count: usize,
    pub sample_titles: Vec<String>,
    pub error: Option<String>,
    pub diagnostics: ExecutionDiagnostics,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AggregatedSearchItemDto {
    pub source_id: Uuid,
    pub source_name: String,
    pub book: SearchBookResult,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SourceSearchErrorDto {
    pub source_id: Uuid,
    pub source_name: String,
    pub error: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AggregatedSearchResponseDto {
    pub keyword: String,
    pub page: u32,
    pub items: Vec<AggregatedSearchItemDto>,
    pub errors: Vec<SourceSearchErrorDto>,
    pub toasts: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OnlineBookDetailResponseDto {
    pub source_id: Uuid,
    pub source_name: String,
    pub detail: OnlineBookDetail,
    pub diagnostics: ExecutionDiagnostics,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OnlineTocResponseDto {
    pub source_id: Uuid,
    pub chapters: Vec<OnlineChapterItem>,
    pub diagnostics: ExecutionDiagnostics,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OnlineContentResponseDto {
    pub source_id: Uuid,
    pub chapter_index: usize,
    pub content: OnlineChapterContent,
    pub cached: bool,
    pub diagnostics: ExecutionDiagnostics,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OnlineShelfBookDto {
    pub id: Uuid,
    pub source_id: Uuid,
    pub source_name: String,
    pub name: String,
    pub author: String,
    pub book_url: String,
    pub toc_url: String,
    pub cover_url: Option<String>,
    pub intro: Option<String>,
    pub kind: Option<String>,
    pub last_chapter: Option<String>,
    pub total_chapters: i64,
    pub current_chapter_index: i64,
    pub current_chapter_title: Option<String>,
    pub scroll_ratio: f64,
    pub updated_at: Timestamp,
}

#[derive(Debug, Clone, Deserialize)]
pub struct AddOnlineShelfInput {
    pub source_id: Uuid,
    pub name: String,
    pub author: String,
    pub book_url: String,
    pub toc_url: String,
    pub cover_url: Option<String>,
    pub intro: Option<String>,
    pub kind: Option<String>,
    pub last_chapter: Option<String>,
    pub total_chapters: Option<i64>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct UpdateOnlineShelfInput {
    pub current_chapter_index: Option<i64>,
    pub current_chapter_title: Option<String>,
    pub scroll_ratio: Option<f64>,
    pub total_chapters: Option<i64>,
    pub last_chapter: Option<String>,
    pub toc_url: Option<String>,
    // Optional source switch
    pub source_id: Option<Uuid>,
    pub book_url: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum OnlineDownloadFormat {
    Epub,
    Txt,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum DownloadJobStatus {
    Pending,
    Running,
    Completed,
    Failed,
}

#[derive(Debug, Clone, Deserialize)]
pub struct StartDownloadInput {
    pub source_id: Uuid,
    pub book_name: String,
    pub author: String,
    pub book_url: String,
    pub toc_url: String,
    pub cover_url: Option<String>,
    pub intro: Option<String>,
    pub format: OnlineDownloadFormat,
    pub start_chapter: Option<usize>,
    pub end_chapter: Option<usize>,
    #[serde(default)]
    pub import_to_library: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OnlineDownloadJobDto {
    pub id: Uuid,
    pub user_id: Uuid,
    pub source_id: Uuid,
    pub source_name: String,
    pub book_name: String,
    pub author: String,
    pub format: OnlineDownloadFormat,
    pub status: DownloadJobStatus,
    pub total_chapters: usize,
    pub processed_chapters: usize,
    pub failed_chapters: usize,
    pub current_chapter_title: Option<String>,
    pub output_file_name: Option<String>,
    pub file_size: Option<u64>,
    pub import_to_library: bool,
    pub imported_book_id: Option<Uuid>,
    pub error: Option<String>,
    pub created_at: Timestamp,
    pub updated_at: Timestamp,
}

struct StoredDownloadJob {
    dto: OnlineDownloadJobDto,
    output_path: Option<PathBuf>,
}

pub struct BookOnlineService {
    db: Db,
    catalog: Arc<BookCatalogService>,
    engine: BookSourceEngine,
    cache_dir: PathBuf,
    download_dir: PathBuf,
    downloads: Arc<RwLock<HashMap<Uuid, StoredDownloadJob>>>,
    allow_private_ips: bool,
}

impl BookOnlineService {
    pub fn new(db: Db, data_dir: &Path, catalog: Arc<BookCatalogService>) -> DomainResult<Self> {
        Self::with_engine(db, data_dir, catalog, BookSourceEngine::default(), false)
    }

    pub fn with_engine(
        db: Db,
        data_dir: &Path,
        catalog: Arc<BookCatalogService>,
        engine: BookSourceEngine,
        allow_private_ips: bool,
    ) -> DomainResult<Self> {
        let cache_dir = data_dir.join("online_book_cache");
        let download_dir = data_dir.join("online_downloads");
        std::fs::create_dir_all(&cache_dir).map_err(|e| {
            DomainError::Storage(format!(
                "创建在线书源缓存目录 '{}' 失败: {e}",
                cache_dir.display()
            ))
        })?;
        std::fs::create_dir_all(&download_dir).map_err(|e| {
            DomainError::Storage(format!(
                "创建在线下载输出目录 '{}' 失败: {e}",
                download_dir.display()
            ))
        })?;

        Ok(Self {
            db,
            catalog,
            engine,
            cache_dir,
            download_dir,
            downloads: Arc::new(RwLock::new(HashMap::new())),
            allow_private_ips,
        })
    }

    fn model_to_summary(m: &BookSourceRecordModel, is_admin: bool) -> BookSourceSummaryDto {
        let parsed = serde_json::from_str::<LegadoBookSource>(&m.raw_json).ok();
        let login_ui = parsed
            .as_ref()
            .map(LegadoBookSource::parse_login_ui)
            .unwrap_or_default();
        let has_login_url = parsed
            .as_ref()
            .and_then(|p| p.login_url.as_deref())
            .is_some_and(|s| !s.trim().is_empty())
            || !login_ui.is_empty();
        let comment = parsed.as_ref().and_then(|p| p.book_source_comment.clone());

        let (login_info, variable_preview, cookie_hosts) = if is_admin {
            let mut masked = HashMap::new();
            for (k, v) in &m.runtime_state.0.login_info {
                let is_pwd = k.contains("密码") || k.to_ascii_lowercase().contains("password");
                masked.insert(
                    k.clone(),
                    if is_pwd && !v.is_empty() {
                        "••••••••".to_string()
                    } else {
                        v.clone()
                    },
                );
            }
            let preview: String = m.runtime_state.0.variable.chars().take(240).collect();
            let mut hosts: Vec<String> = m.runtime_state.0.cookies.keys().cloned().collect();
            hosts.sort();
            (masked, preview, hosts)
        } else {
            (HashMap::new(), String::new(), Vec::new())
        };

        BookSourceSummaryDto {
            id: m.id,
            name: m.name.clone(),
            source_url: m.source_url.clone(),
            group_name: m.group_name.clone(),
            comment,
            enabled: m.enabled,
            weight: m.weight,
            source_type: m.source_type,
            has_login_url,
            login_ui: if is_admin { login_ui } else { Vec::new() },
            login_info,
            has_variable: !m.runtime_state.0.variable.trim().is_empty(),
            variable_preview,
            cookie_hosts,
            last_test_ok: m.last_test_ok,
            last_test_msg: m.last_test_msg.clone(),
            updated_at: m.updated_at,
        }
    }

    async fn load_source_model(&self, id: Uuid) -> DomainResult<BookSourceRecordModel> {
        let mut db = self.db.clone();
        BookSourceRecordModel::filter_by_id(id)
            .first()
            .exec(&mut db)
            .await
            .map_err(db_err)?
            .ok_or_else(|| DomainError::NotFound(format!("未找到书源 {id}")))
    }

    async fn persist_runtime_state(
        &self,
        id: Uuid,
        new_state: SourceRuntimeState,
    ) -> DomainResult<()> {
        let mut db = self.db.clone();
        if let Some(mut rec) = BookSourceRecordModel::filter_by_id(id)
            .first()
            .exec(&mut db)
            .await
            .map_err(db_err)?
        {
            rec.update()
                .runtime_state(toasty::Json(new_state))
                .updated_at(Timestamp::now())
                .exec(&mut db)
                .await
                .map_err(db_err)?;
        }
        Ok(())
    }

    pub async fn list_sources(&self, actor: &User) -> DomainResult<Vec<BookSourceSummaryDto>> {
        let mut db = self.db.clone();
        let mut rows = BookSourceRecordModel::all()
            .exec(&mut db)
            .await
            .map_err(db_err)?;
        if !actor.role.is_admin() {
            rows.retain(|r| r.enabled);
        }
        rows.sort_by(|a, b| {
            b.enabled
                .cmp(&a.enabled)
                .then_with(|| b.weight.cmp(&a.weight))
                .then_with(|| a.name.cmp(&b.name))
        });
        Ok(rows
            .iter()
            .map(|r| Self::model_to_summary(r, actor.role.is_admin()))
            .collect())
    }

    pub async fn import_sources(
        &self,
        actor: &User,
        raw_json: &str,
    ) -> DomainResult<ImportBookSourcesReportDto> {
        require_admin(actor)?;
        let parsed_batch =
            LegadoBookSource::parse_json_batch(raw_json).map_err(DomainError::Validation)?;

        let mut db = self.db.clone();
        let existing_all = BookSourceRecordModel::all()
            .exec(&mut db)
            .await
            .map_err(db_err)?;

        let mut existing_by_key: HashMap<(String, String), BookSourceRecordModel> = existing_all
            .into_iter()
            .map(|m| ((m.source_url.clone(), m.name.clone()), m))
            .collect();

        let mut imported_count = 0usize;
        let mut updated_count = 0usize;
        let mut out = Vec::with_capacity(parsed_batch.len());
        let now = Timestamp::now();

        for (src, item_raw) in parsed_batch {
            let group_name = src.book_source_group.clone().unwrap_or_default();
            let key = (src.book_source_url.clone(), src.book_source_name.clone());

            if let Some(mut rec) = existing_by_key.remove(&key) {
                rec.update()
                    .group_name(group_name.clone())
                    .enabled(src.enabled)
                    .weight(src.weight)
                    .source_type(src.book_source_type)
                    .raw_json(item_raw.clone())
                    .updated_at(now)
                    .exec(&mut db)
                    .await
                    .map_err(db_err)?;
                rec.group_name = group_name;
                rec.enabled = src.enabled;
                rec.weight = src.weight;
                rec.source_type = src.book_source_type;
                rec.raw_json = item_raw;
                rec.updated_at = now;
                updated_count += 1;
                out.push(Self::model_to_summary(&rec, true));
            } else {
                let id = Uuid::now_v7();
                let mut rt = SourceRuntimeState::default();
                rt.ensure_android_id(&src.book_source_url);
                let created = toasty::create!(BookSourceRecordModel {
                    id,
                    name: src.book_source_name.clone(),
                    source_url: src.book_source_url.clone(),
                    group_name,
                    enabled: src.enabled,
                    weight: src.weight,
                    source_type: src.book_source_type,
                    raw_json: item_raw,
                    runtime_state: toasty::Json(rt),
                    last_test_ok: None,
                    last_test_msg: None,
                    created_at: now,
                    updated_at: now,
                })
                .exec(&mut db)
                .await
                .map_err(db_err)?;
                imported_count += 1;
                out.push(Self::model_to_summary(&created, true));
            }
        }

        Ok(ImportBookSourcesReportDto {
            imported_count,
            updated_count,
            sources: out,
        })
    }

    pub async fn update_source(
        &self,
        actor: &User,
        id: Uuid,
        input: UpdateBookSourceInput,
    ) -> DomainResult<BookSourceSummaryDto> {
        require_admin(actor)?;
        let mut rec = self.load_source_model(id).await?;
        let mut db = self.db.clone();
        let now = Timestamp::now();

        let mut next_name = rec.name.clone();
        let mut next_url = rec.source_url.clone();
        let mut next_group = rec.group_name.clone();
        let mut next_enabled = rec.enabled;
        let mut next_weight = rec.weight;
        let mut next_type = rec.source_type;
        let mut next_raw = rec.raw_json.clone();
        let mut next_state = rec.runtime_state.0.clone();

        if let Some(raw) = input.raw_json {
            let mut batch =
                LegadoBookSource::parse_json_batch(&raw).map_err(DomainError::Validation)?;
            let (parsed, clean_raw) = batch.remove(0);
            next_name = parsed.book_source_name;
            next_url = parsed.book_source_url;
            next_group = parsed.book_source_group.unwrap_or_default();
            next_type = parsed.book_source_type;
            next_raw = clean_raw;
        }
        if let Some(en) = input.enabled {
            next_enabled = en;
        }
        if let Some(w) = input.weight {
            next_weight = w;
        }
        if let Some(g) = input.group_name {
            next_group = g;
        }
        if let Some(v) = input.variable {
            next_state.variable = v;
        }

        rec.update()
            .name(next_name.clone())
            .source_url(next_url.clone())
            .group_name(next_group.clone())
            .enabled(next_enabled)
            .weight(next_weight)
            .source_type(next_type)
            .raw_json(next_raw.clone())
            .runtime_state(toasty::Json(next_state.clone()))
            .updated_at(now)
            .exec(&mut db)
            .await
            .map_err(db_err)?;

        rec.name = next_name;
        rec.source_url = next_url;
        rec.group_name = next_group;
        rec.enabled = next_enabled;
        rec.weight = next_weight;
        rec.source_type = next_type;
        rec.raw_json = next_raw;
        rec.runtime_state = toasty::Json(next_state);
        rec.updated_at = now;

        Ok(Self::model_to_summary(&rec, true))
    }

    pub async fn delete_source(&self, actor: &User, id: Uuid) -> DomainResult<()> {
        require_admin(actor)?;
        let rec = self.load_source_model(id).await?;
        let mut db = self.db.clone();
        rec.delete().exec(&mut db).await.map_err(db_err)?;
        let src_cache = self.cache_dir.join(id.to_string());
        if src_cache.is_dir() {
            let _ = std::fs::remove_dir_all(src_cache);
        }
        Ok(())
    }

    pub async fn export_source_json(&self, actor: &User, id: Uuid) -> DomainResult<String> {
        require_admin(actor)?;
        let rec = self.load_source_model(id).await?;
        Ok(rec.raw_json)
    }

    pub async fn run_login_action(
        &self,
        actor: &User,
        id: Uuid,
        mut login_info: HashMap<String, String>,
        action: Option<String>,
    ) -> DomainResult<LoginActionResponseDto> {
        require_admin(actor)?;
        let rec = self.load_source_model(id).await?;
        let parsed: LegadoBookSource = serde_json::from_str(&rec.raw_json)
            .map_err(|e| DomainError::Validation(format!("书源 JSON 无效: {e}")))?;

        // Preserve existing real password if frontend echoed masked placeholder back
        for (k, v) in login_info.iter_mut() {
            if v == "••••••••"
                && let Some(orig) = rec.runtime_state.0.login_info.get(k)
            {
                *v = orig.clone();
            }
        }

        let engine = self.engine.clone();
        let prev_state = rec.runtime_state.0.clone();
        let action_clone = action.clone();

        let out = tokio::task::spawn_blocking(move || {
            engine.run_login_action_sync(&parsed, prev_state, login_info, action_clone.as_deref())
        })
        .await
        .map_err(|e| DomainError::Storage(format!("执行登录脚本线程失败: {e}")))?
        .map_err(DomainError::Validation)?;

        self.persist_runtime_state(id, out.state).await?;
        let refreshed = self.load_source_model(id).await?;

        Ok(LoginActionResponseDto {
            result: out.data,
            diagnostics: out.diagnostics,
            source: Self::model_to_summary(&refreshed, true),
        })
    }

    pub async fn test_source(
        &self,
        actor: &User,
        id: Uuid,
        keyword_override: Option<String>,
    ) -> DomainResult<SourceTestReportDto> {
        require_admin(actor)?;
        let mut rec = self.load_source_model(id).await?;
        let parsed: LegadoBookSource = serde_json::from_str(&rec.raw_json)
            .map_err(|e| DomainError::Validation(format!("书源 JSON 无效: {e}")))?;

        let kw = keyword_override
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .or_else(|| {
                parsed
                    .rule_search
                    .as_ref()
                    .and_then(|r| r.check_key_word.clone())
                    .map(|s| s.trim().to_string())
                    .filter(|s| !s.is_empty())
            })
            .unwrap_or_else(|| "我的".to_string());

        let engine = self.engine.clone();
        let prev_state = rec.runtime_state.0.clone();
        let kw_clone = kw.clone();
        let res = tokio::task::spawn_blocking(move || {
            engine.search_sync(&parsed, prev_state, &kw_clone, 1)
        })
        .await
        .map_err(|e| DomainError::Storage(format!("测试书源线程失败: {e}")))?;

        let mut db = self.db.clone();
        let now = Timestamp::now();
        match res {
            Ok(out) => {
                let sample_titles: Vec<String> =
                    out.data.iter().take(5).map(|b| b.name.clone()).collect();
                let count = out.data.len();
                let msg = format!("命中 {count} 本（关键词：{kw}）");
                rec.update()
                    .runtime_state(toasty::Json(out.state))
                    .last_test_ok(Some(count > 0))
                    .last_test_msg(Some(msg))
                    .updated_at(now)
                    .exec(&mut db)
                    .await
                    .map_err(db_err)?;
                Ok(SourceTestReportDto {
                    ok: count > 0,
                    keyword: kw,
                    result_count: count,
                    sample_titles,
                    error: if count == 0 {
                        Some("搜索未返回条目，请检查是否需要登录或更换线路".into())
                    } else {
                        None
                    },
                    diagnostics: out.diagnostics,
                })
            }
            Err(err) => {
                rec.update()
                    .last_test_ok(Some(false))
                    .last_test_msg(Some(err.chars().take(180).collect()))
                    .updated_at(now)
                    .exec(&mut db)
                    .await
                    .map_err(db_err)?;
                Ok(SourceTestReportDto {
                    ok: false,
                    keyword: kw,
                    result_count: 0,
                    sample_titles: Vec::new(),
                    error: Some(err),
                    diagnostics: ExecutionDiagnostics::default(),
                })
            }
        }
    }

    pub async fn search(
        &self,
        keyword: &str,
        source_ids: Option<Vec<Uuid>>,
        page: u32,
    ) -> DomainResult<AggregatedSearchResponseDto> {
        let kw = keyword.trim();
        if kw.is_empty() {
            return Err(DomainError::Validation("请输入搜索关键词".into()));
        }

        let mut db = self.db.clone();
        let mut sources = BookSourceRecordModel::all()
            .exec(&mut db)
            .await
            .map_err(db_err)?;

        if let Some(filter_ids) = source_ids.filter(|v| !v.is_empty()) {
            sources.retain(|s| filter_ids.contains(&s.id));
        } else {
            sources.retain(|s| s.enabled);
        }

        if sources.is_empty() {
            return Err(DomainError::Validation("没有可用的已启用书源".into()));
        }

        sources.sort_by(|a, b| b.weight.cmp(&a.weight).then_with(|| a.name.cmp(&b.name)));
        let page_num = page.max(1);

        let mut handles = Vec::with_capacity(sources.len());
        for rec in sources {
            let engine = self.engine.clone();
            let kw_owned = kw.to_string();
            let parsed_res = serde_json::from_str::<LegadoBookSource>(&rec.raw_json);
            let sid = rec.id;
            let sname = rec.name.clone();
            let state = rec.runtime_state.0.clone();

            handles.push(tokio::spawn(async move {
                let parsed = match parsed_res {
                    Ok(p) => p,
                    Err(e) => {
                        return (sid, sname, Err(format!("书源解析失败: {e}")));
                    }
                };
                let join = tokio::task::spawn_blocking(move || {
                    engine.search_sync(&parsed, state, &kw_owned, page_num)
                })
                .await;
                match join {
                    Ok(res) => (sid, sname, res),
                    Err(e) => (sid, sname, Err(format!("搜索任务异常: {e}"))),
                }
            }));
        }

        let mut items = Vec::new();
        let mut errors = Vec::new();
        let mut toasts = Vec::new();

        for h in handles {
            if let Ok((sid, sname, res)) = h.await {
                match res {
                    Ok(out) => {
                        let _ = self.persist_runtime_state(sid, out.state).await;
                        toasts.extend(out.diagnostics.toasts);
                        for book in out.data {
                            items.push(AggregatedSearchItemDto {
                                source_id: sid,
                                source_name: sname.clone(),
                                book,
                            });
                        }
                    }
                    Err(err) => {
                        errors.push(SourceSearchErrorDto {
                            source_id: sid,
                            source_name: sname,
                            error: err,
                        });
                    }
                }
            }
        }

        Ok(AggregatedSearchResponseDto {
            keyword: kw.to_string(),
            page: page_num,
            items,
            errors,
            toasts,
        })
    }

    pub async fn fetch_book_info(
        &self,
        source_id: Uuid,
        book_url: &str,
        fallback_hint: Option<BookJsContext>,
    ) -> DomainResult<OnlineBookDetailResponseDto> {
        let rec = self.load_source_model(source_id).await?;
        let parsed: LegadoBookSource = serde_json::from_str(&rec.raw_json)
            .map_err(|e| DomainError::Validation(format!("书源 JSON 无效: {e}")))?;

        let engine = self.engine.clone();
        let url_owned = book_url.to_string();
        let prev_state = rec.runtime_state.0.clone();

        let out = tokio::task::spawn_blocking(move || {
            engine.fetch_book_info_sync(&parsed, prev_state, &url_owned, fallback_hint.as_ref())
        })
        .await
        .map_err(|e| DomainError::Storage(format!("获取书籍详情失败: {e}")))?
        .map_err(DomainError::Validation)?;

        let _ = self.persist_runtime_state(source_id, out.state).await;

        Ok(OnlineBookDetailResponseDto {
            source_id,
            source_name: rec.name,
            detail: out.data,
            diagnostics: out.diagnostics,
        })
    }

    pub async fn fetch_toc(
        &self,
        source_id: Uuid,
        toc_url: &str,
        book_ctx: BookJsContext,
    ) -> DomainResult<OnlineTocResponseDto> {
        let rec = self.load_source_model(source_id).await?;
        let parsed: LegadoBookSource = serde_json::from_str(&rec.raw_json)
            .map_err(|e| DomainError::Validation(format!("书源 JSON 无效: {e}")))?;

        let engine = self.engine.clone();
        let url_owned = toc_url.to_string();
        let prev_state = rec.runtime_state.0.clone();

        let out = tokio::task::spawn_blocking(move || {
            engine.fetch_toc_sync(&parsed, prev_state, &url_owned, &book_ctx)
        })
        .await
        .map_err(|e| DomainError::Storage(format!("获取目录失败: {e}")))?
        .map_err(DomainError::Validation)?;

        let _ = self.persist_runtime_state(source_id, out.state).await;

        Ok(OnlineTocResponseDto {
            source_id,
            chapters: out.data,
            diagnostics: out.diagnostics,
        })
    }

    fn chapter_cache_path(&self, source_id: Uuid, chapter_url: &str) -> PathBuf {
        let mut h = Md5::new();
        h.update(chapter_url.as_bytes());
        let key = hex::encode(h.finalize());
        self.cache_dir
            .join(source_id.to_string())
            .join(format!("{key}.json"))
    }

    fn read_cached_chapter(
        &self,
        source_id: Uuid,
        chapter_url: &str,
    ) -> Option<OnlineChapterContent> {
        let path = self.chapter_cache_path(source_id, chapter_url);
        let meta = std::fs::metadata(&path).ok()?;
        // Expire cached chapters after 14 days
        if let Ok(modified) = meta.modified()
            && let Ok(age) = SystemTime::now().duration_since(modified)
            && age > Duration::from_secs(14 * 24 * 3600)
        {
            let _ = std::fs::remove_file(&path);
            return None;
        }
        let raw = std::fs::read_to_string(&path).ok()?;
        serde_json::from_str::<OnlineChapterContent>(&raw).ok()
    }

    fn write_cached_chapter(
        &self,
        source_id: Uuid,
        chapter_url: &str,
        content: &OnlineChapterContent,
    ) {
        if content.content.trim().is_empty() {
            return;
        }
        let path = self.chapter_cache_path(source_id, chapter_url);
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        if let Ok(json) = serde_json::to_string(content) {
            let _ = std::fs::write(path, json);
        }
    }

    pub async fn fetch_chapter_content(
        &self,
        source_id: Uuid,
        chapter_url: &str,
        book_ctx: BookJsContext,
        chapter_ctx: ChapterJsContext,
        force_refresh: bool,
    ) -> DomainResult<OnlineContentResponseDto> {
        if !force_refresh && let Some(cached) = self.read_cached_chapter(source_id, chapter_url) {
            return Ok(OnlineContentResponseDto {
                source_id,
                chapter_index: chapter_ctx.index,
                content: cached,
                cached: true,
                diagnostics: ExecutionDiagnostics::default(),
            });
        }

        let rec = self.load_source_model(source_id).await?;
        let parsed: LegadoBookSource = serde_json::from_str(&rec.raw_json)
            .map_err(|e| DomainError::Validation(format!("书源 JSON 无效: {e}")))?;

        let engine = self.engine.clone();
        let url_owned = chapter_url.to_string();
        let prev_state = rec.runtime_state.0.clone();
        let ch_idx = chapter_ctx.index;

        let out = tokio::task::spawn_blocking(move || {
            engine.fetch_chapter_content_sync(
                &parsed,
                prev_state,
                &url_owned,
                &book_ctx,
                &chapter_ctx,
            )
        })
        .await
        .map_err(|e| DomainError::Storage(format!("获取章节正文失败: {e}")))?
        .map_err(DomainError::Validation)?;

        let _ = self.persist_runtime_state(source_id, out.state).await;
        self.write_cached_chapter(source_id, chapter_url, &out.data);

        Ok(OnlineContentResponseDto {
            source_id,
            chapter_index: ch_idx,
            content: out.data,
            cached: false,
            diagnostics: out.diagnostics,
        })
    }

    // -----------------------------------------------------------------------
    // Per-User Online Bookshelf
    // -----------------------------------------------------------------------

    pub async fn list_shelf(&self, user_id: UserId) -> DomainResult<Vec<OnlineShelfBookDto>> {
        let mut db = self.db.clone();
        let mut rows = BookOnlineShelfModel::filter_by_user_id(user_id.0)
            .exec(&mut db)
            .await
            .map_err(db_err)?;
        rows.sort_by_key(|r| std::cmp::Reverse(r.updated_at));

        let sources = BookSourceRecordModel::all()
            .exec(&mut db)
            .await
            .map_err(db_err)?;
        let src_names: HashMap<Uuid, String> =
            sources.into_iter().map(|s| (s.id, s.name)).collect();

        Ok(rows
            .into_iter()
            .map(|r| OnlineShelfBookDto {
                id: r.id,
                source_id: r.source_id,
                source_name: src_names
                    .get(&r.source_id)
                    .cloned()
                    .unwrap_or_else(|| "已删除书源".to_string()),
                name: r.name,
                author: r.author,
                book_url: r.book_url,
                toc_url: r.toc_url,
                cover_url: r.cover_url,
                intro: r.intro,
                kind: r.kind,
                last_chapter: r.last_chapter,
                total_chapters: r.total_chapters,
                current_chapter_index: r.current_chapter_index,
                current_chapter_title: r.current_chapter_title,
                scroll_ratio: r.scroll_ratio,
                updated_at: r.updated_at,
            })
            .collect())
    }

    pub async fn add_to_shelf(
        &self,
        user_id: UserId,
        input: AddOnlineShelfInput,
    ) -> DomainResult<OnlineShelfBookDto> {
        let src = self.load_source_model(input.source_id).await?;
        let mut db = self.db.clone();
        let existing_list = BookOnlineShelfModel::filter_by_user_id(user_id.0)
            .exec(&mut db)
            .await
            .map_err(db_err)?;
        let now = Timestamp::now();

        if let Some(mut existing) = existing_list
            .into_iter()
            .find(|b| b.source_id == input.source_id && b.book_url == input.book_url)
        {
            let next_total = input.total_chapters.unwrap_or(existing.total_chapters);
            let next_toc = if input.toc_url.trim().is_empty() {
                existing.toc_url.clone()
            } else {
                input.toc_url.clone()
            };
            let next_cover = input
                .cover_url
                .clone()
                .or_else(|| existing.cover_url.clone());
            let next_intro = input.intro.clone().or_else(|| existing.intro.clone());
            let next_kind = input.kind.clone().or_else(|| existing.kind.clone());
            let next_last = input
                .last_chapter
                .clone()
                .or_else(|| existing.last_chapter.clone());
            existing
                .update()
                .name(input.name.clone())
                .author(input.author.clone())
                .toc_url(next_toc.clone())
                .cover_url(next_cover.clone())
                .intro(next_intro.clone())
                .kind(next_kind.clone())
                .last_chapter(next_last.clone())
                .total_chapters(next_total)
                .updated_at(now)
                .exec(&mut db)
                .await
                .map_err(db_err)?;

            return Ok(OnlineShelfBookDto {
                id: existing.id,
                source_id: existing.source_id,
                source_name: src.name,
                name: input.name,
                author: input.author,
                book_url: existing.book_url,
                toc_url: next_toc,
                cover_url: next_cover,
                intro: next_intro,
                kind: next_kind,
                last_chapter: next_last,
                total_chapters: next_total,
                current_chapter_index: existing.current_chapter_index,
                current_chapter_title: existing.current_chapter_title,
                scroll_ratio: existing.scroll_ratio,
                updated_at: now,
            });
        }

        let id = Uuid::now_v7();
        let total = input.total_chapters.unwrap_or(0);
        let created = toasty::create!(BookOnlineShelfModel {
            id,
            user_id: user_id.0,
            source_id: input.source_id,
            name: input.name.clone(),
            author: input.author.clone(),
            book_url: input.book_url.clone(),
            toc_url: input.toc_url.clone(),
            cover_url: input.cover_url.clone(),
            intro: input.intro.clone(),
            kind: input.kind.clone(),
            last_chapter: input.last_chapter.clone(),
            total_chapters: total,
            current_chapter_index: 0,
            current_chapter_title: None,
            scroll_ratio: 0.0,
            created_at: now,
            updated_at: now,
        })
        .exec(&mut db)
        .await
        .map_err(db_err)?;

        Ok(OnlineShelfBookDto {
            id: created.id,
            source_id: created.source_id,
            source_name: src.name,
            name: created.name,
            author: created.author,
            book_url: created.book_url,
            toc_url: created.toc_url,
            cover_url: created.cover_url,
            intro: created.intro,
            kind: created.kind,
            last_chapter: created.last_chapter,
            total_chapters: created.total_chapters,
            current_chapter_index: created.current_chapter_index,
            current_chapter_title: created.current_chapter_title,
            scroll_ratio: created.scroll_ratio,
            updated_at: created.updated_at,
        })
    }

    pub async fn update_shelf_book(
        &self,
        user_id: UserId,
        id: Uuid,
        input: UpdateOnlineShelfInput,
    ) -> DomainResult<OnlineShelfBookDto> {
        let mut db = self.db.clone();
        let mut rec = BookOnlineShelfModel::filter_by_id(id)
            .first()
            .exec(&mut db)
            .await
            .map_err(db_err)?
            .ok_or_else(|| DomainError::NotFound("未找到在线书架条目".into()))?;
        if rec.user_id != user_id.0 {
            return Err(DomainError::Forbidden("无权操作他人在线书架".into()));
        }

        let now = Timestamp::now();
        let next_source_id = input.source_id.unwrap_or(rec.source_id);
        let next_book_url = input.book_url.unwrap_or_else(|| rec.book_url.clone());
        let next_toc_url = input.toc_url.unwrap_or_else(|| rec.toc_url.clone());
        let next_idx = input
            .current_chapter_index
            .unwrap_or(rec.current_chapter_index);
        let next_title = input
            .current_chapter_title
            .or_else(|| rec.current_chapter_title.clone());
        let next_scroll = input.scroll_ratio.unwrap_or(rec.scroll_ratio);
        let next_total = input.total_chapters.unwrap_or(rec.total_chapters);
        let next_last = input.last_chapter.or_else(|| rec.last_chapter.clone());

        rec.update()
            .source_id(next_source_id)
            .book_url(next_book_url.clone())
            .toc_url(next_toc_url.clone())
            .current_chapter_index(next_idx)
            .current_chapter_title(next_title.clone())
            .scroll_ratio(next_scroll)
            .total_chapters(next_total)
            .last_chapter(next_last.clone())
            .updated_at(now)
            .exec(&mut db)
            .await
            .map_err(db_err)?;

        let src_name = self
            .load_source_model(next_source_id)
            .await
            .map(|s| s.name)
            .unwrap_or_else(|_| "未知书源".to_string());

        Ok(OnlineShelfBookDto {
            id: rec.id,
            source_id: next_source_id,
            source_name: src_name,
            name: rec.name,
            author: rec.author,
            book_url: next_book_url,
            toc_url: next_toc_url,
            cover_url: rec.cover_url,
            intro: rec.intro,
            kind: rec.kind,
            last_chapter: next_last,
            total_chapters: next_total,
            current_chapter_index: next_idx,
            current_chapter_title: next_title,
            scroll_ratio: next_scroll,
            updated_at: now,
        })
    }

    pub async fn remove_from_shelf(&self, user_id: UserId, id: Uuid) -> DomainResult<()> {
        let mut db = self.db.clone();
        let rec = BookOnlineShelfModel::filter_by_id(id)
            .first()
            .exec(&mut db)
            .await
            .map_err(db_err)?
            .ok_or_else(|| DomainError::NotFound("未找到在线书架条目".into()))?;
        if rec.user_id != user_id.0 {
            return Err(DomainError::Forbidden("无权删除他人在线书架".into()));
        }
        rec.delete().exec(&mut db).await.map_err(db_err)?;
        Ok(())
    }

    // -----------------------------------------------------------------------
    // Background Download + EPUB 3.0 / TXT Packaging + Optional Library Import
    // -----------------------------------------------------------------------

    pub async fn start_download(
        self: &Arc<Self>,
        actor: &User,
        input: StartDownloadInput,
    ) -> DomainResult<OnlineDownloadJobDto> {
        if input.import_to_library {
            require_admin(actor)?;
        }
        let src = self.load_source_model(input.source_id).await?;
        let job_id = Uuid::now_v7();
        let now = Timestamp::now();

        let dto = OnlineDownloadJobDto {
            id: job_id,
            user_id: actor.id.0,
            source_id: input.source_id,
            source_name: src.name,
            book_name: input.book_name.clone(),
            author: input.author.clone(),
            format: input.format,
            status: DownloadJobStatus::Pending,
            total_chapters: 0,
            processed_chapters: 0,
            failed_chapters: 0,
            current_chapter_title: None,
            output_file_name: None,
            file_size: None,
            import_to_library: input.import_to_library,
            imported_book_id: None,
            error: None,
            created_at: now,
            updated_at: now,
        };

        {
            let mut guard = self.downloads.write().await;
            guard.insert(
                job_id,
                StoredDownloadJob {
                    dto: dto.clone(),
                    output_path: None,
                },
            );
        }

        let svc = Arc::clone(self);
        let actor_clone = actor.clone();
        tokio::spawn(async move {
            svc.execute_download_job(job_id, actor_clone, input).await;
        });

        Ok(dto)
    }

    async fn execute_download_job(
        self: Arc<Self>,
        job_id: Uuid,
        actor: User,
        input: StartDownloadInput,
    ) {
        self.update_job(job_id, |j| {
            j.status = DownloadJobStatus::Running;
            j.current_chapter_title = Some("正在拉取章节目录…".into());
        })
        .await;

        let book_ctx = BookJsContext {
            name: input.book_name.clone(),
            author: input.author.clone(),
            book_url: input.book_url.clone(),
            toc_url: input.toc_url.clone(),
            cover_url: input.cover_url.clone(),
            intro: input.intro.clone(),
            ..Default::default()
        };

        let toc_res = self
            .fetch_toc(input.source_id, &input.toc_url, book_ctx.clone())
            .await;
        let all_chapters = match toc_res {
            Ok(t) => t
                .chapters
                .into_iter()
                .filter(|c| !c.is_volume && !c.url.trim().is_empty())
                .collect::<Vec<_>>(),
            Err(e) => {
                self.fail_job(job_id, format!("获取目录失败: {e}")).await;
                return;
            }
        };

        if all_chapters.is_empty() {
            self.fail_job(job_id, "目录为空，无可下载章节".into()).await;
            return;
        }

        let start = input.start_chapter.unwrap_or(0).min(all_chapters.len());
        let end = input
            .end_chapter
            .map(|e| (e + 1).min(all_chapters.len()))
            .unwrap_or(all_chapters.len())
            .max(start);
        let selected = &all_chapters[start..end];
        if selected.is_empty() {
            self.fail_job(job_id, "所选章节区间为空".into()).await;
            return;
        }

        self.update_job(job_id, |j| {
            j.total_chapters = selected.len();
        })
        .await;

        let mut packed_chapters = Vec::with_capacity(selected.len());
        let mut failed_count = 0usize;

        for (i, ch) in selected.iter().enumerate() {
            self.update_job(job_id, |j| {
                j.current_chapter_title = Some(ch.title.clone());
            })
            .await;

            let mut cur_book_ctx = book_ctx.clone();
            cur_book_ctx.dur_chapter_index = ch.index;
            cur_book_ctx.dur_chapter_title = ch.title.clone();

            let ch_ctx = ChapterJsContext {
                index: ch.index,
                title: ch.title.clone(),
                url: ch.url.clone(),
            };

            match self
                .fetch_chapter_content(input.source_id, &ch.url, cur_book_ctx, ch_ctx, false)
                .await
            {
                Ok(resp) if !resp.content.content.trim().is_empty() => {
                    packed_chapters.push(PackChapterInput {
                        title: if resp.content.title.trim().is_empty() {
                            ch.title.clone()
                        } else {
                            resp.content.title
                        },
                        content: resp.content.content,
                    });
                    if !resp.cached {
                        // Gentle pacing for uncached remote fetches
                        tokio::time::sleep(Duration::from_millis(120)).await;
                    }
                }
                _ => {
                    failed_count += 1;
                    packed_chapters.push(PackChapterInput {
                        title: ch.title.clone(),
                        content: "[本章抓取失败，请稍后重试]".into(),
                    });
                }
            }

            self.update_job(job_id, |j| {
                j.processed_chapters = i + 1;
                j.failed_chapters = failed_count;
            })
            .await;
        }

        // Optional cover download for EPUB
        let cover_bytes = if input.format == OnlineDownloadFormat::Epub
            && let Some(ref cover_url) = input.cover_url
            && !cover_url.trim().is_empty()
        {
            self.fetch_cover_proxy(cover_url).await.ok()
        } else {
            None
        };

        self.update_job(job_id, |j| {
            j.current_chapter_title = Some("正在封装电子书文件…".into());
        })
        .await;

        let pack_meta = PackBookMetadata {
            title: input.book_name.clone(),
            author: input.author.clone(),
            description: input.intro.clone(),
            cover_bytes,
        };

        let packaged = match input.format {
            OnlineDownloadFormat::Epub => package_chapters_to_epub(&pack_meta, &packed_chapters),
            OnlineDownloadFormat::Txt => Ok(package_chapters_to_txt(&pack_meta, &packed_chapters)),
        };

        let bytes = match packaged {
            Ok(b) => b,
            Err(e) => {
                self.fail_job(job_id, e).await;
                return;
            }
        };

        let ext = match input.format {
            OnlineDownloadFormat::Epub => "epub",
            OnlineDownloadFormat::Txt => "txt",
        };
        let safe_stem = sanitize_file_stem(&input.book_name);
        let file_name = format!("{safe_stem}.{ext}");
        let job_dir = self.download_dir.join(job_id.to_string());
        if let Err(e) = std::fs::create_dir_all(&job_dir) {
            self.fail_job(job_id, format!("创建输出目录失败: {e}"))
                .await;
            return;
        }
        let out_path = job_dir.join(&file_name);
        if let Err(e) = std::fs::write(&out_path, &bytes) {
            self.fail_job(job_id, format!("写入输出文件失败: {e}"))
                .await;
            return;
        }

        let file_size = bytes.len() as u64;
        let mut imported_book_id = None;
        if input.import_to_library {
            self.update_job(job_id, |j| {
                j.current_chapter_title = Some("正在导入本地图书库…".into());
            })
            .await;
            match self
                .catalog
                .upload_book(&actor, &file_name, Bytes::from(bytes))
                .await
            {
                Ok(summary) => {
                    imported_book_id = Some(summary.id.0);
                }
                Err(e) => {
                    self.fail_job(job_id, format!("封装成功但导入图书库失败: {e}"))
                        .await;
                    return;
                }
            }
        }

        let mut guard = self.downloads.write().await;
        if let Some(slot) = guard.get_mut(&job_id) {
            slot.output_path = Some(out_path);
            slot.dto.status = DownloadJobStatus::Completed;
            slot.dto.output_file_name = Some(file_name);
            slot.dto.file_size = Some(file_size);
            slot.dto.imported_book_id = imported_book_id;
            slot.dto.current_chapter_title = None;
            slot.dto.updated_at = Timestamp::now();
        }
    }

    async fn update_job(&self, job_id: Uuid, f: impl FnOnce(&mut OnlineDownloadJobDto)) {
        let mut guard = self.downloads.write().await;
        if let Some(slot) = guard.get_mut(&job_id) {
            f(&mut slot.dto);
            slot.dto.updated_at = Timestamp::now();
        }
    }

    async fn fail_job(&self, job_id: Uuid, msg: String) {
        self.update_job(job_id, |j| {
            j.status = DownloadJobStatus::Failed;
            j.error = Some(msg);
            j.current_chapter_title = None;
        })
        .await;
    }

    pub async fn list_downloads(&self, user_id: UserId) -> Vec<OnlineDownloadJobDto> {
        let guard = self.downloads.read().await;
        let mut list: Vec<OnlineDownloadJobDto> = guard
            .values()
            .filter(|j| j.dto.user_id == user_id.0)
            .map(|j| j.dto.clone())
            .collect();
        list.sort_by_key(|j| std::cmp::Reverse(j.created_at));
        list
    }

    pub async fn get_download(
        &self,
        user_id: UserId,
        job_id: Uuid,
    ) -> DomainResult<OnlineDownloadJobDto> {
        let guard = self.downloads.read().await;
        let slot = guard
            .get(&job_id)
            .ok_or_else(|| DomainError::NotFound("未找到下载任务".into()))?;
        if slot.dto.user_id != user_id.0 {
            return Err(DomainError::Forbidden("无权查看他人下载任务".into()));
        }
        Ok(slot.dto.clone())
    }

    pub async fn read_download_file(
        &self,
        user_id: UserId,
        job_id: Uuid,
    ) -> DomainResult<(String, &'static str, Vec<u8>)> {
        let (file_name, format, path) = {
            let guard = self.downloads.read().await;
            let slot = guard
                .get(&job_id)
                .ok_or_else(|| DomainError::NotFound("未找到下载任务".into()))?;
            if slot.dto.user_id != user_id.0 {
                return Err(DomainError::Forbidden("无权下载他人任务产物".into()));
            }
            let path = slot
                .output_path
                .clone()
                .ok_or_else(|| DomainError::Validation("任务尚未完成或产物不存在".into()))?;
            let fname = slot
                .dto
                .output_file_name
                .clone()
                .unwrap_or_else(|| "novel.epub".to_string());
            (fname, slot.dto.format, path)
        };

        let bytes = tokio::fs::read(&path)
            .await
            .map_err(|e| DomainError::Storage(format!("读取下载文件失败: {e}")))?;
        let mime = match format {
            OnlineDownloadFormat::Epub => "application/epub+zip",
            OnlineDownloadFormat::Txt => "text/plain; charset=utf-8",
        };
        Ok((file_name, mime, bytes))
    }

    pub async fn fetch_cover_proxy(&self, raw_url: &str) -> DomainResult<(Vec<u8>, String)> {
        let validated = validate_target_url(raw_url.trim(), self.allow_private_ips)
            .map_err(DomainError::Validation)?;
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(12))
            .user_agent(DEFAULT_WEBVIEW_UA)
            .build()
            .map_err(|e| DomainError::Storage(e.to_string()))?;

        let resp = client
            .get(validated)
            .send()
            .await
            .map_err(|e| DomainError::Validation(format!("拉取封面失败: {e}")))?;

        if !resp.status().is_success() {
            return Err(DomainError::Validation(format!(
                "封面响应状态异常: {}",
                resp.status()
            )));
        }

        let ct = resp
            .headers()
            .get(reqwest::header::CONTENT_TYPE)
            .and_then(|v| v.to_str().ok())
            .unwrap_or("image/jpeg")
            .split(';')
            .next()
            .unwrap_or("image/jpeg")
            .trim()
            .to_string();

        let bytes = resp
            .bytes()
            .await
            .map_err(|e| DomainError::Validation(format!("读取封面失败: {e}")))?;
        if bytes.len() > 8 * 1024 * 1024 {
            return Err(DomainError::Validation("封面图片体积超过 8MB 限制".into()));
        }
        Ok((bytes.to_vec(), ct))
    }
}

fn sanitize_file_stem(name: &str) -> String {
    let cleaned: String = name
        .trim()
        .chars()
        .map(|c| {
            if matches!(c, '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|') || c.is_control() {
                '_'
            } else {
                c
            }
        })
        .collect();
    let trimmed = cleaned.trim_matches('.').trim();
    if trimmed.is_empty() {
        "novel".to_string()
    } else {
        trimmed.chars().take(80).collect()
    }
}

#[allow(dead_code)]
fn _keep_http_cfg(_: HttpClientConfig) {}

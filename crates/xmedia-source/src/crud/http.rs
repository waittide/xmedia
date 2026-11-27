use std::collections::HashMap;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

use async_trait::async_trait;
use axum::body::Body;
use axum::extract::{DefaultBodyLimit, FromRef, Path, Query, State};
use axum::http::header::{CONTENT_TYPE, X_CONTENT_TYPE_OPTIONS};
use axum::http::{HeaderValue, StatusCode};
use axum::response::Response;
use axum::routing::{get, post};
use axum::{Json, Router};
use uuid::Uuid;
use xmedia_core::application::auth::AuthService;
use xmedia_core::domain::error::{DomainError, DomainResult};
use xmedia_core::domain::library::MediaKind;
use xmedia_core::domain::user::User;
use xmedia_core::http::{ApiResult, WebAuth};

use super::dto::{
    BatchSourcesInput, BatchSourcesReportDto, ExportSourcesBatchRequest, ImportSourcesReportDto,
    ImportSourcesRequest, ListPluginsQuery, LoginActionRequest, LoginActionResponseDto,
    ReorderSourcesRequest, SourceSummaryDto, SourceVariableResponse, UpdateSourceInput,
};
use super::service::{SourceCrudService, SourceSpecAdapter};
use crate::http::HttpClientConfig;
use crate::model::PluginFormat;
use crate::plugin_v2::{
    PluginAuthActionRequest, PluginAuthActionResponseDto, UpdatePluginSettingsRequest,
};

/// Maximum request body accepted by `/plugins/import` (`32 MiB`).
pub const SOURCE_IMPORT_BODY_LIMIT_BYTES: usize = 32 * 1024 * 1024;

/// Builds a `200 OK` HTTP response containing raw UTF-8 plugin source with `nosniff`.
pub fn into_raw_plugin_export_response(raw: String, format: PluginFormat) -> Response {
    let mut resp = Response::new(Body::from(raw));
    let content_type = match format {
        PluginFormat::JsonRule => "application/json; charset=utf-8",
        PluginFormat::JsModule => "application/javascript; charset=utf-8",
    };
    resp.headers_mut()
        .insert(CONTENT_TYPE, HeaderValue::from_static(content_type));
    resp.headers_mut()
        .insert(X_CONTENT_TYPE_OPTIONS, HeaderValue::from_static("nosniff"));
    resp
}

pub fn into_raw_json_export_response(raw_json: String) -> Response {
    into_raw_plugin_export_response(raw_json, PluginFormat::JsonRule)
}

/// Per-domain plugin adapter registered into [`PluginRegistry`].
#[async_trait]
pub trait PluginDomainHandler: Send + Sync {
    fn domain(&self) -> MediaKind;

    async fn has_owned_plugin(&self, actor: &User, id: Uuid) -> DomainResult<bool>;

    async fn list_plugins(&self, actor: &User) -> DomainResult<Vec<SourceSummaryDto>>;

    async fn get_plugin(&self, actor: &User, id: Uuid) -> DomainResult<SourceSummaryDto>;

    async fn create_or_upsert_plugin(
        &self,
        actor: &User,
        raw_source: &str,
    ) -> DomainResult<SourceSummaryDto>;

    async fn import_plugins(
        &self,
        actor: &User,
        raw_source: &str,
    ) -> DomainResult<ImportSourcesReportDto>;

    async fn update_plugin(
        &self,
        actor: &User,
        id: Uuid,
        input: UpdateSourceInput,
    ) -> DomainResult<SourceSummaryDto>;

    async fn delete_plugin(&self, actor: &User, id: Uuid) -> DomainResult<()>;

    async fn reorder_plugins(
        &self,
        actor: &User,
        ordered_ids: Vec<Uuid>,
    ) -> DomainResult<Vec<SourceSummaryDto>>;

    async fn batch_plugins(
        &self,
        actor: &User,
        input: BatchSourcesInput,
    ) -> DomainResult<BatchSourcesReportDto>;

    async fn get_plugin_variable(&self, actor: &User, id: Uuid) -> DomainResult<String>;

    async fn export_plugin(&self, actor: &User, id: Uuid) -> DomainResult<(String, PluginFormat)>;

    async fn export_plugins_batch(
        &self,
        actor: &User,
        ids: Vec<Uuid>,
    ) -> DomainResult<(String, PluginFormat)>;

    async fn run_login_action(
        &self,
        actor: &User,
        id: Uuid,
        req: LoginActionRequest,
    ) -> DomainResult<LoginActionResponseDto>;

    async fn run_auth_action(
        &self,
        actor: &User,
        id: Uuid,
        req: PluginAuthActionRequest,
    ) -> DomainResult<PluginAuthActionResponseDto>;

    async fn update_plugin_settings(
        &self,
        actor: &User,
        id: Uuid,
        req: UpdatePluginSettingsRequest,
    ) -> DomainResult<SourceSummaryDto>;

    async fn test_plugin(
        &self,
        actor: &User,
        id: Uuid,
        payload: serde_json::Value,
    ) -> DomainResult<serde_json::Value>;
}

type BoxFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;
type LoginHandlerFn = dyn Fn(User, Uuid, LoginActionRequest) -> BoxFuture<'static, DomainResult<LoginActionResponseDto>>
    + Send
    + Sync;
type TestHandlerFn = dyn Fn(User, Uuid, serde_json::Value) -> BoxFuture<'static, DomainResult<serde_json::Value>>
    + Send
    + Sync;

/// Zero-boilerplate [`PluginDomainHandler`] backed by [`SourceCrudService<Spec>`] plus
/// domain-specific `login_action` and `test` closures.
pub struct CrudPluginDomainHandler<Spec: SourceSpecAdapter> {
    crud: Arc<SourceCrudService<Spec>>,
    http_config: HttpClientConfig,
    login_fn: Box<LoginHandlerFn>,
    test_fn: Box<TestHandlerFn>,
}

impl<Spec: SourceSpecAdapter> CrudPluginDomainHandler<Spec> {
    pub fn into_arc<L, LFut, T, TFut>(
        crud: Arc<SourceCrudService<Spec>>,
        login_fn: L,
        test_fn: T,
    ) -> Arc<dyn PluginDomainHandler>
    where
        L: Fn(User, Uuid, LoginActionRequest) -> LFut + Send + Sync + 'static,
        LFut: Future<Output = DomainResult<LoginActionResponseDto>> + Send + 'static,
        T: Fn(User, Uuid, serde_json::Value) -> TFut + Send + Sync + 'static,
        TFut: Future<Output = DomainResult<serde_json::Value>> + Send + 'static,
    {
        Self::into_arc_with_http_config(crud, HttpClientConfig::default(), login_fn, test_fn)
    }

    pub fn into_arc_with_http_config<L, LFut, T, TFut>(
        crud: Arc<SourceCrudService<Spec>>,
        http_config: HttpClientConfig,
        login_fn: L,
        test_fn: T,
    ) -> Arc<dyn PluginDomainHandler>
    where
        L: Fn(User, Uuid, LoginActionRequest) -> LFut + Send + Sync + 'static,
        LFut: Future<Output = DomainResult<LoginActionResponseDto>> + Send + 'static,
        T: Fn(User, Uuid, serde_json::Value) -> TFut + Send + Sync + 'static,
        TFut: Future<Output = DomainResult<serde_json::Value>> + Send + 'static,
    {
        Arc::new(Self {
            crud,
            http_config,
            login_fn: Box::new(move |u, id, req| Box::pin(login_fn(u, id, req))),
            test_fn: Box::new(move |u, id, payload| Box::pin(test_fn(u, id, payload))),
        })
    }
}

#[async_trait]
impl<Spec: SourceSpecAdapter> PluginDomainHandler for CrudPluginDomainHandler<Spec> {
    fn domain(&self) -> MediaKind {
        Spec::MEDIA_KIND
    }

    async fn has_owned_plugin(&self, actor: &User, id: Uuid) -> DomainResult<bool> {
        self.crud.has_owned_source(actor, id).await
    }

    async fn list_plugins(&self, actor: &User) -> DomainResult<Vec<SourceSummaryDto>> {
        self.crud.list_sources(actor).await
    }

    async fn get_plugin(&self, actor: &User, id: Uuid) -> DomainResult<SourceSummaryDto> {
        self.crud.get_source(actor, id).await
    }

    async fn create_or_upsert_plugin(
        &self,
        actor: &User,
        raw_source: &str,
    ) -> DomainResult<SourceSummaryDto> {
        self.crud.create_or_upsert_source(actor, raw_source).await
    }

    async fn import_plugins(
        &self,
        actor: &User,
        raw_source: &str,
    ) -> DomainResult<ImportSourcesReportDto> {
        self.crud.import_sources(actor, raw_source).await
    }

    async fn update_plugin(
        &self,
        actor: &User,
        id: Uuid,
        input: UpdateSourceInput,
    ) -> DomainResult<SourceSummaryDto> {
        self.crud.update_source(actor, id, input).await
    }

    async fn delete_plugin(&self, actor: &User, id: Uuid) -> DomainResult<()> {
        self.crud.delete_source(actor, id).await
    }

    async fn reorder_plugins(
        &self,
        actor: &User,
        ordered_ids: Vec<Uuid>,
    ) -> DomainResult<Vec<SourceSummaryDto>> {
        self.crud.reorder_sources(actor, ordered_ids).await
    }

    async fn batch_plugins(
        &self,
        actor: &User,
        input: BatchSourcesInput,
    ) -> DomainResult<BatchSourcesReportDto> {
        self.crud.batch_sources(actor, input).await
    }

    async fn get_plugin_variable(&self, actor: &User, id: Uuid) -> DomainResult<String> {
        self.crud.get_source_variable(actor, id).await
    }

    async fn export_plugin(&self, actor: &User, id: Uuid) -> DomainResult<(String, PluginFormat)> {
        self.crud.export_source(actor, id).await
    }

    async fn export_plugins_batch(
        &self,
        actor: &User,
        ids: Vec<Uuid>,
    ) -> DomainResult<(String, PluginFormat)> {
        self.crud.export_sources_batch(actor, ids).await
    }

    async fn run_login_action(
        &self,
        actor: &User,
        id: Uuid,
        req: LoginActionRequest,
    ) -> DomainResult<LoginActionResponseDto> {
        (self.login_fn)(actor.clone(), id, req).await
    }

    async fn run_auth_action(
        &self,
        actor: &User,
        id: Uuid,
        req: PluginAuthActionRequest,
    ) -> DomainResult<PluginAuthActionResponseDto> {
        self.crud
            .run_plugin_auth(actor, id, req, self.http_config.clone())
            .await
    }

    async fn update_plugin_settings(
        &self,
        actor: &User,
        id: Uuid,
        req: UpdatePluginSettingsRequest,
    ) -> DomainResult<SourceSummaryDto> {
        self.crud
            .update_plugin_settings(actor, id, req.values)
            .await
    }

    async fn test_plugin(
        &self,
        actor: &User,
        id: Uuid,
        payload: serde_json::Value,
    ) -> DomainResult<serde_json::Value> {
        (self.test_fn)(actor.clone(), id, payload).await
    }
}

/// Multi-domain dispatcher backing unified `/api/v1/plugins` endpoints.
#[derive(Clone, Default)]
pub struct PluginRegistry {
    handlers: Arc<HashMap<MediaKind, Arc<dyn PluginDomainHandler>>>,
    ordered_domains: Arc<Vec<MediaKind>>,
}

impl PluginRegistry {
    pub fn new(handlers: Vec<Arc<dyn PluginDomainHandler>>) -> Self {
        let mut map = HashMap::with_capacity(handlers.len());
        let mut ordered = Vec::with_capacity(handlers.len());
        for h in handlers {
            let d = h.domain();
            if !map.contains_key(&d) {
                ordered.push(d);
            }
            map.insert(d, h);
        }
        Self {
            handlers: Arc::new(map),
            ordered_domains: Arc::new(ordered),
        }
    }

    pub fn single(handler: Arc<dyn PluginDomainHandler>) -> Self {
        Self::new(vec![handler])
    }

    pub fn get_domain(&self, domain: MediaKind) -> DomainResult<&Arc<dyn PluginDomainHandler>> {
        self.handlers.get(&domain).ok_or_else(|| {
            DomainError::UnsupportedFormat(format!("插件领域 '{}' 未启用", domain.as_str()))
        })
    }

    pub async fn resolve_owned_handler(
        &self,
        actor: &User,
        id: Uuid,
    ) -> DomainResult<&Arc<dyn PluginDomainHandler>> {
        actor.ensure_active()?;
        for domain in self.ordered_domains.iter() {
            if let Some(handler) = self.handlers.get(domain)
                && handler.has_owned_plugin(actor, id).await?
            {
                return Ok(handler);
            }
        }
        Err(DomainError::NotFound(format!("未找到插件 {id}")))
    }

    pub fn resolve_target_domain(
        &self,
        explicit: Option<MediaKind>,
        raw_source: Option<&str>,
    ) -> DomainResult<&Arc<dyn PluginDomainHandler>> {
        if let Some(domain) = explicit {
            return self.get_domain(domain);
        }
        if let Some(raw) = raw_source
            && let Some(inferred) = infer_plugin_domain(raw)
        {
            return self.get_domain(inferred);
        }
        if self.ordered_domains.len() == 1 {
            return self.get_domain(self.ordered_domains[0]);
        }
        Err(DomainError::Validation(
            "请指定插件所属领域 domain (book / comic / music / video)".into(),
        ))
    }
}

/// Infers `MediaKind` from a JSON rule payload or JS v2 plugin source when `domain` is omitted.
pub fn infer_plugin_domain(raw: &str) -> Option<MediaKind> {
    let trimmed = raw.trim().trim_start_matches('\u{feff}').trim();
    if trimmed.is_empty() {
        return None;
    }
    if let Ok(val) = serde_json::from_str::<serde_json::Value>(trimmed) {
        let sample = match &val {
            serde_json::Value::Object(_) => Some(&val),
            serde_json::Value::Array(arr) => arr.iter().find(|v| v.is_object()),
            _ => None,
        }?;
        let obj = sample.as_object()?;
        if let Some(d) = obj.get("domain").and_then(|v| v.as_str())
            && let Ok(kind) = MediaKind::parse(d.trim())
        {
            return Some(kind);
        }
        if obj.contains_key("bookSourceUrl")
            || obj.contains_key("bookSourceName")
            || obj.contains_key("ruleBookInfo")
            || obj.contains_key("ruleToc")
        {
            return Some(MediaKind::Book);
        }
        if obj.contains_key("comicSourceUrl")
            || obj.contains_key("comicSourceName")
            || obj.contains_key("ruleComicDetail")
            || obj.contains_key("rulePages")
        {
            return Some(MediaKind::Comic);
        }
        if obj.contains_key("musicSourceUrl")
            || obj.contains_key("musicSourceName")
            || obj.contains_key("ruleTrack")
            || obj.contains_key("ruleStream")
            || obj.contains_key("qualities")
        {
            return Some(MediaKind::Music);
        }
        if obj.contains_key("videoSourceUrl")
            || obj.contains_key("videoSourceName")
            || obj.contains_key("apiUrl")
            || obj.contains_key("cmsType")
            || obj.contains_key("rulePlay")
            || obj.contains_key("ruleEpisodes")
        {
            return Some(MediaKind::Video);
        }
        return None;
    }

    for kind in [
        MediaKind::Book,
        MediaKind::Comic,
        MediaKind::Music,
        MediaKind::Video,
    ] {
        let needle_single = format!("'{}'", kind.as_str());
        let needle_double = format!("\"{}\"", kind.as_str());
        for line in trimmed.lines().take(60) {
            if line.contains("domain")
                && (line.contains(&needle_single) || line.contains(&needle_double))
            {
                return Some(kind);
            }
        }
    }
    None
}

#[derive(Clone)]
pub struct PluginsHttpState {
    pub auth: Arc<AuthService>,
    pub registry: Arc<PluginRegistry>,
}

impl FromRef<PluginsHttpState> for Arc<AuthService> {
    fn from_ref(state: &PluginsHttpState) -> Self {
        state.auth.clone()
    }
}

impl FromRef<PluginsHttpState> for Arc<PluginRegistry> {
    fn from_ref(state: &PluginsHttpState) -> Self {
        state.registry.clone()
    }
}

/// Builds the unified `/plugins` router (mounted at `/api/v1/plugins`).
pub fn plugins_router(auth: Arc<AuthService>, registry: Arc<PluginRegistry>) -> Router {
    let state = PluginsHttpState { auth, registry };
    plugins_stateful_router::<PluginsHttpState>().with_state(state)
}

/// Generic `/plugins` router extractable from any outer `State` providing `Arc<AuthService>` and `Arc<PluginRegistry>`.
pub fn plugins_stateful_router<S>() -> Router<S>
where
    S: Clone + Send + Sync + 'static,
    Arc<AuthService>: FromRef<S>,
    Arc<PluginRegistry>: FromRef<S>,
{
    Router::new()
        .route("/", get(list_plugins).post(create_plugin))
        .route("/reorder", post(reorder_plugins))
        .route("/batch", post(batch_plugins))
        .route("/export-batch", post(export_plugins_batch))
        .route(
            "/import",
            post(import_plugins).layer(DefaultBodyLimit::max(SOURCE_IMPORT_BODY_LIMIT_BYTES)),
        )
        .route(
            "/{id}",
            get(get_plugin).patch(update_plugin).delete(delete_plugin),
        )
        .route("/{id}/variable", get(get_plugin_variable))
        .route("/{id}/export", get(export_plugin))
        .route("/{id}/login-action", post(run_plugin_login_action))
        .route("/{id}/auth", post(run_plugin_auth_action))
        .route("/{id}/settings", axum::routing::put(update_plugin_settings))
        .route("/{id}/test", post(test_plugin))
}

async fn list_plugins(
    State(registry): State<Arc<PluginRegistry>>,
    WebAuth(user): WebAuth,
    Query(query): Query<ListPluginsQuery>,
) -> ApiResult<Json<Vec<SourceSummaryDto>>> {
    if let Some(domain) = query.domain {
        let handler = registry.get_domain(domain)?;
        return Ok(Json(handler.list_plugins(&user).await?));
    }
    user.ensure_active()?;
    let mut out = Vec::new();
    for domain in registry.ordered_domains.iter() {
        if let Some(handler) = registry.handlers.get(domain) {
            out.extend(handler.list_plugins(&user).await?);
        }
    }
    out.sort_by(|a, b| b.weight.cmp(&a.weight).then_with(|| a.name.cmp(&b.name)));
    Ok(Json(out))
}

async fn get_plugin(
    State(registry): State<Arc<PluginRegistry>>,
    WebAuth(user): WebAuth,
    Path(id): Path<Uuid>,
) -> ApiResult<Json<SourceSummaryDto>> {
    let handler = registry.resolve_owned_handler(&user, id).await?;
    Ok(Json(handler.get_plugin(&user, id).await?))
}

async fn create_plugin(
    State(registry): State<Arc<PluginRegistry>>,
    WebAuth(user): WebAuth,
    Query(query): Query<ListPluginsQuery>,
    Json(req): Json<ImportSourcesRequest>,
) -> ApiResult<Json<SourceSummaryDto>> {
    let handler =
        registry.resolve_target_domain(req.domain.or(query.domain), Some(&req.raw_json))?;
    Ok(Json(
        handler
            .create_or_upsert_plugin(&user, &req.raw_json)
            .await?,
    ))
}

async fn import_plugins(
    State(registry): State<Arc<PluginRegistry>>,
    WebAuth(user): WebAuth,
    Query(query): Query<ListPluginsQuery>,
    Json(req): Json<ImportSourcesRequest>,
) -> ApiResult<Json<ImportSourcesReportDto>> {
    let handler =
        registry.resolve_target_domain(req.domain.or(query.domain), Some(&req.raw_json))?;
    Ok(Json(handler.import_plugins(&user, &req.raw_json).await?))
}

async fn reorder_plugins(
    State(registry): State<Arc<PluginRegistry>>,
    WebAuth(user): WebAuth,
    Query(query): Query<ListPluginsQuery>,
    Json(req): Json<ReorderSourcesRequest>,
) -> ApiResult<Json<Vec<SourceSummaryDto>>> {
    if let Some(domain) = req.domain.or(query.domain) {
        let handler = registry.get_domain(domain)?;
        return Ok(Json(handler.reorder_plugins(&user, req.ordered_ids).await?));
    }
    if let Some(&first_id) = req.ordered_ids.first() {
        let handler = registry.resolve_owned_handler(&user, first_id).await?;
        return Ok(Json(handler.reorder_plugins(&user, req.ordered_ids).await?));
    }
    if registry.ordered_domains.len() == 1 {
        let handler = registry.get_domain(registry.ordered_domains[0])?;
        return Ok(Json(handler.reorder_plugins(&user, req.ordered_ids).await?));
    }
    user.ensure_active()?;
    Ok(Json(Vec::new()))
}

async fn batch_plugins(
    State(registry): State<Arc<PluginRegistry>>,
    WebAuth(user): WebAuth,
    Query(query): Query<ListPluginsQuery>,
    Json(req): Json<BatchSourcesInput>,
) -> ApiResult<Json<BatchSourcesReportDto>> {
    if let Some(domain) = req.domain.or(query.domain) {
        let handler = registry.get_domain(domain)?;
        return Ok(Json(handler.batch_plugins(&user, req).await?));
    }
    user.ensure_active()?;
    if req.ids.is_empty() {
        return Err(DomainError::Validation("请至少选择一条插件".into()).into());
    }
    let mut total_affected = 0usize;
    for domain in registry.ordered_domains.iter() {
        if let Some(handler) = registry.handlers.get(domain) {
            let report = handler.batch_plugins(&user, req.clone()).await?;
            total_affected += report.affected_count;
        }
    }
    Ok(Json(BatchSourcesReportDto {
        affected_count: total_affected,
    }))
}

async fn export_plugins_batch(
    State(registry): State<Arc<PluginRegistry>>,
    WebAuth(user): WebAuth,
    Query(query): Query<ListPluginsQuery>,
    Json(req): Json<ExportSourcesBatchRequest>,
) -> ApiResult<Response> {
    if let Some(domain) = req.domain.or(query.domain) {
        let handler = registry.get_domain(domain)?;
        let (raw, format) = handler.export_plugins_batch(&user, req.ids).await?;
        return Ok(into_raw_plugin_export_response(raw, format));
    }
    user.ensure_active()?;
    if req.ids.is_empty() {
        return Err(DomainError::Validation("请至少选择一条插件".into()).into());
    }
    if let Some(&first_id) = req.ids.first() {
        let handler = registry.resolve_owned_handler(&user, first_id).await?;
        let (raw, format) = handler.export_plugins_batch(&user, req.ids).await?;
        return Ok(into_raw_plugin_export_response(raw, format));
    }
    Err(DomainError::NotFound("未找到可导出的插件".into()).into())
}

async fn get_plugin_variable(
    State(registry): State<Arc<PluginRegistry>>,
    WebAuth(user): WebAuth,
    Path(id): Path<Uuid>,
) -> ApiResult<Json<SourceVariableResponse>> {
    let handler = registry.resolve_owned_handler(&user, id).await?;
    let variable = handler.get_plugin_variable(&user, id).await?;
    Ok(Json(SourceVariableResponse { variable }))
}

async fn update_plugin(
    State(registry): State<Arc<PluginRegistry>>,
    WebAuth(user): WebAuth,
    Path(id): Path<Uuid>,
    Json(req): Json<UpdateSourceInput>,
) -> ApiResult<Json<SourceSummaryDto>> {
    let handler = registry.resolve_owned_handler(&user, id).await?;
    Ok(Json(handler.update_plugin(&user, id, req).await?))
}

async fn delete_plugin(
    State(registry): State<Arc<PluginRegistry>>,
    WebAuth(user): WebAuth,
    Path(id): Path<Uuid>,
) -> ApiResult<StatusCode> {
    let handler = registry.resolve_owned_handler(&user, id).await?;
    handler.delete_plugin(&user, id).await?;
    Ok(StatusCode::NO_CONTENT)
}

async fn export_plugin(
    State(registry): State<Arc<PluginRegistry>>,
    WebAuth(user): WebAuth,
    Path(id): Path<Uuid>,
) -> ApiResult<Response> {
    let handler = registry.resolve_owned_handler(&user, id).await?;
    let (raw, format) = handler.export_plugin(&user, id).await?;
    Ok(into_raw_plugin_export_response(raw, format))
}

async fn run_plugin_login_action(
    State(registry): State<Arc<PluginRegistry>>,
    WebAuth(user): WebAuth,
    Path(id): Path<Uuid>,
    Json(req): Json<LoginActionRequest>,
) -> ApiResult<Json<LoginActionResponseDto>> {
    let handler = registry.resolve_owned_handler(&user, id).await?;
    Ok(Json(handler.run_login_action(&user, id, req).await?))
}

async fn run_plugin_auth_action(
    State(registry): State<Arc<PluginRegistry>>,
    WebAuth(user): WebAuth,
    Path(id): Path<Uuid>,
    Json(req): Json<PluginAuthActionRequest>,
) -> ApiResult<Json<PluginAuthActionResponseDto>> {
    let handler = registry.resolve_owned_handler(&user, id).await?;
    Ok(Json(handler.run_auth_action(&user, id, req).await?))
}

async fn update_plugin_settings(
    State(registry): State<Arc<PluginRegistry>>,
    WebAuth(user): WebAuth,
    Path(id): Path<Uuid>,
    Json(req): Json<UpdatePluginSettingsRequest>,
) -> ApiResult<Json<SourceSummaryDto>> {
    let handler = registry.resolve_owned_handler(&user, id).await?;
    Ok(Json(handler.update_plugin_settings(&user, id, req).await?))
}

async fn test_plugin(
    State(registry): State<Arc<PluginRegistry>>,
    WebAuth(user): WebAuth,
    Path(id): Path<Uuid>,
    body: axum::body::Bytes,
) -> ApiResult<Json<serde_json::Value>> {
    let payload = if body.iter().all(|b| b.is_ascii_whitespace()) {
        serde_json::json!({})
    } else {
        serde_json::from_slice::<serde_json::Value>(&body)
            .map_err(|e| DomainError::Validation(format!("测试请求 JSON 格式无效: {e}")))?
    };
    let handler = registry.resolve_owned_handler(&user, id).await?;
    Ok(Json(handler.test_plugin(&user, id, payload).await?))
}

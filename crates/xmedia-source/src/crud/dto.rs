use std::collections::HashMap;

use jiff::Timestamp;
use serde::{Deserialize, Serialize};
use uuid::Uuid;
use xmedia_core::domain::library::MediaKind;

use crate::model::{ExecutionDiagnostics, LoginUiItem, PluginFormat};

pub const MASKED_SECRET_PLACEHOLDER: &str = "••••••••";
pub const VARIABLE_PREVIEW_MAX_CHARS: usize = 240;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SourceSummaryDto {
    pub id: Uuid,
    pub domain: MediaKind,
    pub format: PluginFormat,
    pub name: String,
    pub source_url: String,
    pub group_name: String,
    pub comment: Option<String>,
    pub check_keyword: Option<String>,
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
pub struct ImportSourcesReportDto<S = SourceSummaryDto> {
    pub imported_count: usize,
    pub updated_count: usize,
    pub sources: Vec<S>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct UpdateSourceInput {
    pub enabled: Option<bool>,
    pub weight: Option<i64>,
    pub group_name: Option<String>,
    #[serde(alias = "content")]
    pub raw_json: Option<String>,
    pub variable: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "action", rename_all = "snake_case")]
pub enum BatchSourceAction {
    Enable,
    Disable,
    SetGroup { group_name: String },
    Delete,
}

#[derive(Debug, Clone, Deserialize)]
pub struct BatchSourcesInput {
    #[serde(default)]
    pub domain: Option<MediaKind>,
    pub ids: Vec<Uuid>,
    #[serde(flatten)]
    pub action: BatchSourceAction,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BatchSourcesReportDto {
    pub affected_count: usize,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct ListPluginsQuery {
    pub domain: Option<MediaKind>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ImportSourcesRequest {
    #[serde(default)]
    pub domain: Option<MediaKind>,
    #[serde(alias = "content")]
    pub raw_json: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ReorderSourcesRequest {
    #[serde(default)]
    pub domain: Option<MediaKind>,
    pub ordered_ids: Vec<Uuid>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ExportSourcesBatchRequest {
    #[serde(default)]
    pub domain: Option<MediaKind>,
    pub ids: Vec<Uuid>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct LoginActionRequest {
    #[serde(default)]
    pub login_info: HashMap<String, String>,
    pub action: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LoginActionResponseDto<S = SourceSummaryDto> {
    pub result: String,
    pub diagnostics: ExecutionDiagnostics,
    pub source: S,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SourceVariableResponse {
    pub variable: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SourceTestStepDto {
    pub stage: String,
    pub label: String,
    pub ok: bool,
    pub elapsed_ms: u64,
    pub summary: String,
    pub error: Option<String>,
    pub diagnostics: ExecutionDiagnostics,
}

/// Generic test report usable by book/comic/music/video domains with flattened domain payload.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SourceTestReportDto<M, P = serde_json::Value> {
    pub ok: bool,
    pub mode: M,
    pub keyword: String,
    pub total_elapsed_ms: u64,
    pub result_count: usize,
    pub sample_titles: Vec<String>,
    pub error: Option<String>,
    pub diagnostics: ExecutionDiagnostics,
    pub steps: Vec<SourceTestStepDto>,
    #[serde(flatten)]
    pub payload: P,
}

pub fn merge_diagnostics(acc: &mut ExecutionDiagnostics, d: &ExecutionDiagnostics) {
    acc.logs.extend(d.logs.iter().cloned());
    acc.toasts.extend(d.toasts.iter().cloned());
    acc.browser_urls.extend(d.browser_urls.iter().cloned());
}

pub fn summarize_steps_outcome(steps: &[SourceTestStepDto]) -> (bool, Option<String>) {
    let ok = !steps.is_empty() && steps.iter().all(|s| s.ok);
    let first_error = steps.iter().find_map(|s| s.error.clone());
    (ok, first_error)
}

pub fn format_timeout_step(elapsed_ms: u64, message: impl Into<String>) -> SourceTestStepDto {
    let msg = message.into();
    SourceTestStepDto {
        stage: "timeout".into(),
        label: "总超时控制".into(),
        ok: false,
        elapsed_ms,
        summary: msg.clone(),
        error: Some(msg),
        diagnostics: ExecutionDiagnostics::default(),
    }
}

pub fn format_test_failure_summary(steps: &[SourceTestStepDto]) -> String {
    if let Some(failed) = steps.iter().find(|s| !s.ok) {
        let reason: String = failed
            .error
            .as_deref()
            .unwrap_or(&failed.summary)
            .chars()
            .take(120)
            .collect();
        format!("{} 失败：{reason}", failed.label)
    } else {
        "测试失败".into()
    }
}

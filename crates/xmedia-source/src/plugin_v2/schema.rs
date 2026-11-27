use std::collections::HashMap;

use serde::{Deserialize, Serialize};
use xmedia_core::domain::library::MediaKind;

use crate::crud::SourceSummaryDto;
use crate::model::{ExecutionDiagnostics, default_true};

fn default_country_code() -> String {
    "86".to_string()
}

/// Declarative authentication capabilities exported by a Plugin v2 module (`plugin.auth`).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct PluginAuthCapsDto {
    #[serde(default)]
    pub qr_code: bool,
    #[serde(default)]
    pub otp: bool,
    #[serde(default)]
    pub password: bool,
    #[serde(default)]
    pub cookie: bool,
    #[serde(default)]
    pub has_profile: bool,
    #[serde(default)]
    pub has_logout: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cookie_hint: Option<String>,
}

impl PluginAuthCapsDto {
    pub fn is_supported(&self) -> bool {
        self.qr_code
            || self.otp
            || self.password
            || self.cookie
            || self.has_profile
            || self.has_logout
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PluginSettingOptionDto {
    pub label: String,
    pub value: String,
}

/// Declarative user-configurable setting item exported by a Plugin v2 module (`plugin.settings`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PluginSettingFieldDto {
    pub key: String,
    pub label: String,
    #[serde(rename = "type")]
    pub field_type: String,
    #[serde(default)]
    pub default_value: serde_json::Value,
    #[serde(default)]
    pub options: Vec<PluginSettingOptionDto>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub placeholder: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PluginExploreCategoryDto {
    pub title: String,
    pub id: String,
}

/// Static manifest extracted safely (with 0 network access) from a Plugin v2 JS module.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PluginV2Manifest {
    pub id: String,
    pub name: String,
    pub version: String,
    pub domain: MediaKind,
    pub base_url: String,
    #[serde(default)]
    pub group: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub check_keyword: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rate_limit: Option<String>,
    #[serde(default)]
    pub headers: HashMap<String, String>,
    #[serde(default)]
    pub media_headers: HashMap<String, String>,
    #[serde(default)]
    pub qualities: Vec<String>,
    #[serde(default)]
    pub explore_categories: Vec<PluginExploreCategoryDto>,
    #[serde(default)]
    pub has_search: bool,
    #[serde(default)]
    pub has_categories: bool,
    #[serde(default)]
    pub has_explore: bool,
    #[serde(default)]
    pub has_detail: bool,
    #[serde(default)]
    pub has_toc: bool,
    #[serde(default)]
    pub has_content: bool,
    #[serde(default)]
    pub has_stream: bool,
    #[serde(default)]
    pub has_lyrics: bool,
    #[serde(default)]
    pub has_cover_resolver: bool,
    #[serde(default)]
    pub auth: PluginAuthCapsDto,
    #[serde(default)]
    pub settings: Vec<PluginSettingFieldDto>,
    #[serde(default = "default_true")]
    pub enabled: bool,
    #[serde(default)]
    pub weight: i64,
}

/// Authenticated upstream account profile returned by `plugin.auth.getProfile(ctx)`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct PluginProfileDto {
    #[serde(default)]
    pub logged_in: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub user_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub nickname: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub avatar_url: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub vip_label: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
}

/// Request payload for `POST /api/v1/plugins/{id}/auth`.
#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "action", rename_all = "snake_case")]
pub enum PluginAuthActionRequest {
    Status,
    QrCreate,
    QrPoll {
        key: String,
    },
    OtpSend {
        #[serde(default = "default_country_code")]
        country_code: String,
        phone: String,
    },
    OtpVerify {
        #[serde(default = "default_country_code")]
        country_code: String,
        phone: String,
        code: String,
    },
    PasswordLogin {
        username: String,
        password: String,
    },
    CookieLogin {
        cookie: String,
    },
    Logout,
}

/// Unified response payload for `POST /api/v1/plugins/{id}/auth`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PluginAuthActionResponseDto<S = SourceSummaryDto> {
    pub ok: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub status: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub qr_key: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub qr_url: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub qr_image: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cooldown_secs: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub profile: Option<PluginProfileDto>,
    pub diagnostics: ExecutionDiagnostics,
    pub source: S,
}

/// Request payload for `PUT /api/v1/plugins/{id}/settings`.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct UpdatePluginSettingsRequest {
    pub values: HashMap<String, serde_json::Value>,
}

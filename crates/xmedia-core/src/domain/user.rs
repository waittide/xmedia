use jiff::Timestamp;
use serde::{Deserialize, Serialize};

use crate::domain::error::{DomainError, DomainResult};

crate::define_uuid_id!(UserId);

/// Verifies that the acting user holds administrator privileges.
pub fn require_admin(actor: &User) -> DomainResult<()> {
    if !actor.role.is_admin() {
        return Err(DomainError::Forbidden(
            "administrator privileges required".into(),
        ));
    }
    Ok(())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum UserRole {
    Admin,
    #[default]
    Member,
}

impl UserRole {
    pub fn is_admin(self) -> bool {
        matches!(self, Self::Admin)
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Admin => "admin",
            Self::Member => "member",
        }
    }

    pub fn parse(value: &str) -> DomainResult<Self> {
        match value.trim().to_ascii_lowercase().as_str() {
            "admin" => Ok(Self::Admin),
            "member" | "reader" | "listener" | "user" => Ok(Self::Member),
            other => Err(DomainError::Validation(format!(
                "unknown user role: {other}"
            ))),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum ThemeMode {
    Light,
    Dark,
    #[default]
    System,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum Locale {
    #[serde(rename = "zh-CN")]
    #[default]
    ZhCn,
    #[serde(rename = "en-US")]
    EnUs,
}

// --- Book Reader Preferences ---

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum ReadingFlow {
    #[default]
    Paginated,
    Scrolled,
}

const DEFAULT_FONT_FAMILY: &str = "system-ui";
const DEFAULT_FONT_SIZE: u16 = 17;
const MIN_FONT_SIZE: u16 = 10;
const MAX_FONT_SIZE: u16 = 48;
const DEFAULT_LINE_HEIGHT: f32 = 1.7;
const MIN_LINE_HEIGHT: f32 = 1.0;
const MAX_LINE_HEIGHT: f32 = 3.0;
const DEFAULT_MARGIN_PX: u16 = 36;
const MAX_MARGIN_PX: u16 = 240;
const DEFAULT_PDF_SCALE: f32 = 1.25;
const MIN_PDF_SCALE: f32 = 0.25;
const MAX_PDF_SCALE: f32 = 4.0;
const DEFAULT_PRELOAD_PAGES: u8 = 3;
const MAX_PRELOAD_PAGES: u8 = 20;
const DEFAULT_WEBTOON_WIDTH: u16 = 920;
const MIN_WEBTOON_WIDTH: u16 = 320;
const MAX_WEBTOON_WIDTH: u16 = 2400;
const DEFAULT_VOLUME: f32 = 0.85;
const DEFAULT_PLAYBACK_RATE: f32 = 1.0;
const MIN_PLAYBACK_RATE: f32 = 0.25;
const MAX_PLAYBACK_RATE: f32 = 4.0;

/// Normalizes an MD3 seed color to lowercase `#rrggbb` (expanding `#rgb`).
fn sanitize_seed_color(raw: &str) -> Option<String> {
    let hex = raw.trim().strip_prefix('#')?;
    if !hex.bytes().all(|b| b.is_ascii_hexdigit()) {
        return None;
    }
    let expanded = match hex.len() {
        3 => hex.chars().flat_map(|c| [c, c]).collect::<String>(),
        6 => hex.to_string(),
        _ => return None,
    };
    Some(format!("#{}", expanded.to_ascii_lowercase()))
}

/// Normalizes an optional app icon preset id; `None` represents the default (`cobalt`).
fn sanitize_app_icon(raw: &str) -> Option<String> {
    match raw.trim().to_ascii_lowercase().as_str() {
        "porcelain" => Some("porcelain".to_string()),
        "obsidian" => Some("obsidian".to_string()),
        "monolith" => Some("monolith".to_string()),
        _ => None,
    }
}

fn clamp_finite(value: f32, min: f32, max: f32, fallback: f32) -> f32 {
    if value.is_finite() {
        value.clamp(min, max)
    } else {
        fallback
    }
}

fn normalize_non_empty(value: &mut String, fallback: &str, max_chars: usize) {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        *value = fallback.to_string();
    } else {
        *value = trimmed.chars().take(max_chars).collect();
    }
}

fn normalize_opt_string(opt: &mut Option<String>, max_chars: usize) {
    if let Some(raw) = opt.as_deref() {
        let trimmed = raw.trim();
        if trimmed.is_empty() {
            *opt = None;
        } else {
            *opt = Some(trimmed.chars().take(max_chars).collect());
        }
    }
}

fn default_font_family() -> String {
    DEFAULT_FONT_FAMILY.to_string()
}
fn default_font_size() -> u16 {
    DEFAULT_FONT_SIZE
}
fn default_line_height() -> f32 {
    DEFAULT_LINE_HEIGHT
}
fn default_margin_px() -> u16 {
    DEFAULT_MARGIN_PX
}
fn default_pdf_scale() -> f32 {
    DEFAULT_PDF_SCALE
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BookReaderPreferences {
    #[serde(default = "default_font_family")]
    pub font_family: String,
    #[serde(default = "default_font_size")]
    pub font_size: u16,
    #[serde(default = "default_line_height")]
    pub line_height: f32,
    #[serde(default = "default_margin_px")]
    pub margin_px: u16,
    #[serde(default)]
    pub flow: ReadingFlow,
    #[serde(default = "default_pdf_scale")]
    pub pdf_scale: f32,
}

impl BookReaderPreferences {
    pub fn normalize(&mut self) {
        normalize_non_empty(&mut self.font_family, DEFAULT_FONT_FAMILY, 128);
        self.font_size = self.font_size.clamp(MIN_FONT_SIZE, MAX_FONT_SIZE);
        self.line_height = clamp_finite(
            self.line_height,
            MIN_LINE_HEIGHT,
            MAX_LINE_HEIGHT,
            DEFAULT_LINE_HEIGHT,
        );
        self.margin_px = self.margin_px.min(MAX_MARGIN_PX);
        self.pdf_scale = clamp_finite(
            self.pdf_scale,
            MIN_PDF_SCALE,
            MAX_PDF_SCALE,
            DEFAULT_PDF_SCALE,
        );
    }
}

impl Default for BookReaderPreferences {
    fn default() -> Self {
        Self {
            font_family: default_font_family(),
            font_size: DEFAULT_FONT_SIZE,
            line_height: DEFAULT_LINE_HEIGHT,
            margin_px: DEFAULT_MARGIN_PX,
            flow: ReadingFlow::default(),
            pdf_scale: DEFAULT_PDF_SCALE,
        }
    }
}

// --- Comic Reader Preferences ---

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum ComicReaderMode {
    #[default]
    RtlDouble,
    LtrSingle,
    LtrDouble,
    Webtoon,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum ComicFitMode {
    #[default]
    FitHeight,
    FitWidth,
    Original,
}

fn default_true() -> bool {
    true
}
fn default_preload_pages() -> u8 {
    DEFAULT_PRELOAD_PAGES
}
fn default_webtoon_width() -> u16 {
    DEFAULT_WEBTOON_WIDTH
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ComicReaderPreferences {
    #[serde(default)]
    pub default_reader_mode: ComicReaderMode,
    #[serde(default)]
    pub default_fit_mode: ComicFitMode,
    #[serde(default = "default_true")]
    pub offset_cover_page: bool,
    #[serde(default)]
    pub dim_images_in_dark: bool,
    #[serde(default = "default_preload_pages")]
    pub preload_pages: u8,
    #[serde(default = "default_webtoon_width")]
    pub webtoon_width: u16,
}

impl ComicReaderPreferences {
    pub fn normalize(&mut self) {
        self.preload_pages = self.preload_pages.min(MAX_PRELOAD_PAGES);
        self.webtoon_width = self
            .webtoon_width
            .clamp(MIN_WEBTOON_WIDTH, MAX_WEBTOON_WIDTH);
    }
}

impl Default for ComicReaderPreferences {
    fn default() -> Self {
        Self {
            default_reader_mode: ComicReaderMode::default(),
            default_fit_mode: ComicFitMode::default(),
            offset_cover_page: true,
            dim_images_in_dark: false,
            preload_pages: DEFAULT_PRELOAD_PAGES,
            webtoon_width: DEFAULT_WEBTOON_WIDTH,
        }
    }
}

// --- Music Playback Preferences ---

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum AudioTranscodeProfile {
    #[default]
    Raw,
    Opus128,
    Mp3192,
    Mp3320,
    Aac256,
}

impl AudioTranscodeProfile {
    pub fn format_and_bitrate(self) -> Option<(&'static str, u32)> {
        match self {
            Self::Raw => None,
            Self::Opus128 => Some(("opus", 128)),
            Self::Mp3192 => Some(("mp3", 192)),
            Self::Mp3320 => Some(("mp3", 320)),
            Self::Aac256 => Some(("aac", 256)),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum MusicPlayMode {
    #[default]
    Sequence,
    LoopOne,
    Shuffle,
}

fn default_volume() -> f32 {
    DEFAULT_VOLUME
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MusicPlaybackPreferences {
    #[serde(default)]
    pub transcode_profile: AudioTranscodeProfile,
    #[serde(default = "default_volume")]
    pub volume: f32,
    #[serde(default)]
    pub muted: bool,
    #[serde(default)]
    pub play_mode: MusicPlayMode,
}

impl MusicPlaybackPreferences {
    pub fn normalize(&mut self) {
        self.volume = clamp_finite(self.volume, 0.0, 1.0, DEFAULT_VOLUME);
    }
}

impl Default for MusicPlaybackPreferences {
    fn default() -> Self {
        Self {
            transcode_profile: AudioTranscodeProfile::Raw,
            volume: DEFAULT_VOLUME,
            muted: false,
            play_mode: MusicPlayMode::Sequence,
        }
    }
}

// --- Video Playback Preferences ---

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum VideoQualityProfile {
    #[default]
    Auto,
    Direct,
    P1080,
    P720,
    P480,
}

fn default_playback_rate() -> f32 {
    DEFAULT_PLAYBACK_RATE
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct VideoPlaybackPreferences {
    #[serde(default)]
    pub default_quality: VideoQualityProfile,
    #[serde(default)]
    pub preferred_audio_lang: Option<String>,
    #[serde(default)]
    pub preferred_subtitle_lang: Option<String>,
    #[serde(default = "default_true")]
    pub autoplay_next: bool,
    #[serde(default = "default_playback_rate")]
    pub playback_rate: f32,
}

impl VideoPlaybackPreferences {
    pub fn normalize(&mut self) {
        normalize_opt_string(&mut self.preferred_audio_lang, 32);
        normalize_opt_string(&mut self.preferred_subtitle_lang, 32);
        self.playback_rate = clamp_finite(
            self.playback_rate,
            MIN_PLAYBACK_RATE,
            MAX_PLAYBACK_RATE,
            DEFAULT_PLAYBACK_RATE,
        );
    }
}

impl Default for VideoPlaybackPreferences {
    fn default() -> Self {
        Self {
            default_quality: VideoQualityProfile::Auto,
            preferred_audio_lang: None,
            preferred_subtitle_lang: None,
            autoplay_next: true,
            playback_rate: DEFAULT_PLAYBACK_RATE,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct UserPreferences {
    #[serde(default)]
    pub theme_mode: ThemeMode,
    /// Single MD3 seed color (`#rrggbb`); `None` means the client default seed.
    #[serde(default)]
    pub seed_color: Option<String>,
    /// Optional icon variant id (`porcelain` | `obsidian` | `monolith`); `None` means default (`cobalt`).
    #[serde(default)]
    pub app_icon: Option<String>,
    #[serde(default)]
    pub locale: Locale,
    #[serde(default)]
    pub book: BookReaderPreferences,
    #[serde(default)]
    pub comic: ComicReaderPreferences,
    #[serde(default)]
    pub music: MusicPlaybackPreferences,
    #[serde(default)]
    pub video: VideoPlaybackPreferences,
}

impl UserPreferences {
    /// Clamps and sanitizes all preference values into safe bounds.
    pub fn normalize(&mut self) {
        self.seed_color = self.seed_color.as_deref().and_then(sanitize_seed_color);
        self.app_icon = self.app_icon.as_deref().and_then(sanitize_app_icon);
        self.book.normalize();
        self.comic.normalize();
        self.music.normalize();
        self.video.normalize();
    }

    pub fn normalized(mut self) -> Self {
        self.normalize();
        self
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct User {
    pub id: UserId,
    pub username: String,
    pub display_name: String,
    pub password_hash: String,
    pub subsonic_secret_enc: String,
    pub api_key: String,
    pub role: UserRole,
    pub is_active: bool,
    pub preferences: UserPreferences,
    pub created_at: Timestamp,
    pub updated_at: Timestamp,
}

impl User {
    pub fn new(
        username: impl Into<String>,
        display_name: impl Into<String>,
        password_hash: impl Into<String>,
        subsonic_secret_enc: impl Into<String>,
        api_key: impl Into<String>,
        role: UserRole,
    ) -> DomainResult<Self> {
        let username = Self::validate_username(username.into())?;
        let display_name = display_name.into().trim().to_string();
        if display_name.chars().count() > 128 {
            return Err(DomainError::Validation(
                "display name cannot exceed 128 characters".into(),
            ));
        }
        let display_name = if display_name.is_empty() {
            username.clone()
        } else {
            display_name
        };
        let password_hash = password_hash.into();
        if password_hash.trim().is_empty() {
            return Err(DomainError::Validation(
                "password hash cannot be empty".into(),
            ));
        }
        let subsonic_secret_enc = subsonic_secret_enc.into();
        let api_key = api_key.into();
        if api_key.trim().is_empty() {
            return Err(DomainError::Validation("api key cannot be empty".into()));
        }

        let now = Timestamp::now();
        Ok(Self {
            id: UserId::new(),
            username,
            display_name,
            password_hash,
            subsonic_secret_enc,
            api_key,
            role,
            is_active: true,
            preferences: UserPreferences::default(),
            created_at: now,
            updated_at: now,
        })
    }

    pub fn validate_username(raw: String) -> DomainResult<String> {
        let trimmed = raw.trim().to_string();
        if trimmed.len() < 2 || trimmed.len() > 64 {
            return Err(DomainError::Validation(
                "username length must be between 2 and 64 characters".into(),
            ));
        }
        if !trimmed
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | '.'))
        {
            return Err(DomainError::Validation(
                "username may only contain ASCII letters, digits, '_', '-', or '.'".into(),
            ));
        }
        Ok(trimmed)
    }

    pub fn update_profile(
        &mut self,
        display_name: Option<String>,
        role: Option<UserRole>,
        is_active: Option<bool>,
    ) -> DomainResult<()> {
        if let Some(name) = display_name {
            let trimmed = name.trim();
            if trimmed.is_empty() || trimmed.chars().count() > 128 {
                return Err(DomainError::Validation(
                    "display name must be between 1 and 128 characters".into(),
                ));
            }
            self.display_name = trimmed.to_string();
        }
        if let Some(role) = role {
            self.role = role;
        }
        if let Some(active) = is_active {
            self.is_active = active;
        }
        self.updated_at = Timestamp::now();
        Ok(())
    }

    pub fn update_credentials(
        &mut self,
        new_hash: impl Into<String>,
        new_subsonic_secret_enc: impl Into<String>,
    ) -> DomainResult<()> {
        let new_hash = new_hash.into();
        if new_hash.trim().is_empty() {
            return Err(DomainError::Validation(
                "password hash cannot be empty".into(),
            ));
        }
        self.password_hash = new_hash;
        self.subsonic_secret_enc = new_subsonic_secret_enc.into();
        self.updated_at = Timestamp::now();
        Ok(())
    }

    pub fn rotate_api_key(&mut self, new_api_key: impl Into<String>) -> DomainResult<()> {
        let key = new_api_key.into();
        if key.trim().is_empty() {
            return Err(DomainError::Validation("api key cannot be empty".into()));
        }
        self.api_key = key;
        self.updated_at = Timestamp::now();
        Ok(())
    }

    pub fn update_preferences(&mut self, mut preferences: UserPreferences) {
        preferences.normalize();
        self.preferences = preferences;
        self.updated_at = Timestamp::now();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn user_preferences_normalize_clamps_numeric_bounds() {
        let mut prefs = UserPreferences::default();
        prefs.book.font_family = " ".into();
        prefs.book.font_size = 200;
        prefs.book.line_height = f32::NAN;
        prefs.book.margin_px = 999;
        prefs.book.pdf_scale = 0.01;
        prefs.comic.preload_pages = 99;
        prefs.comic.webtoon_width = 10;
        prefs.music.volume = 3.5;
        prefs.video.playback_rate = 10.0;
        prefs.video.preferred_audio_lang = Some("  ja  ".into());
        prefs.video.preferred_subtitle_lang = Some("   ".into());

        prefs.normalize();

        assert_eq!(prefs.book.font_family, DEFAULT_FONT_FAMILY);
        assert_eq!(prefs.book.font_size, MAX_FONT_SIZE);
        assert_eq!(prefs.book.line_height, DEFAULT_LINE_HEIGHT);
        assert_eq!(prefs.book.margin_px, MAX_MARGIN_PX);
        assert_eq!(prefs.book.pdf_scale, MIN_PDF_SCALE);
        assert_eq!(prefs.comic.preload_pages, MAX_PRELOAD_PAGES);
        assert_eq!(prefs.comic.webtoon_width, MIN_WEBTOON_WIDTH);
        assert_eq!(prefs.music.volume, 1.0);
        assert_eq!(prefs.video.playback_rate, MAX_PLAYBACK_RATE);
        assert_eq!(prefs.video.preferred_audio_lang.as_deref(), Some("ja"));
        assert_eq!(prefs.video.preferred_subtitle_lang, None);
    }

    #[test]
    fn default_preferences_use_system_mode_and_default_seed() {
        let prefs = UserPreferences::default();
        assert_eq!(prefs.theme_mode, ThemeMode::System);
        assert_eq!(prefs.seed_color, None);
    }

    #[test]
    fn seed_color_is_sanitized() {
        let mut prefs = UserPreferences {
            seed_color: Some(" #6750A4 ".into()),
            ..Default::default()
        };
        prefs.normalize();
        assert_eq!(prefs.seed_color.as_deref(), Some("#6750a4"));

        prefs.seed_color = Some("#ABC".into());
        prefs.normalize();
        assert_eq!(prefs.seed_color.as_deref(), Some("#aabbcc"));

        for invalid in ["6750a4", "#6750a4ff", "#zzzzzz", ""] {
            prefs.seed_color = Some(invalid.into());
            prefs.normalize();
            assert_eq!(prefs.seed_color, None, "{invalid} should be rejected");
        }
    }

    #[test]
    fn app_icon_is_sanitized() {
        let mut prefs = UserPreferences {
            app_icon: Some(" Porcelain ".into()),
            ..Default::default()
        };
        prefs.normalize();
        assert_eq!(prefs.app_icon.as_deref(), Some("porcelain"));

        for valid in ["obsidian", "MONOLITH"] {
            prefs.app_icon = Some(valid.into());
            prefs.normalize();
            assert_eq!(
                prefs.app_icon.as_deref(),
                Some(valid.to_ascii_lowercase().as_str())
            );
        }

        for fallback in ["cobalt", "COBALT", "../evil", "", "unknown"] {
            prefs.app_icon = Some(fallback.into());
            prefs.normalize();
            assert_eq!(
                prefs.app_icon, None,
                "{fallback} should normalize to default None"
            );
        }
    }
}

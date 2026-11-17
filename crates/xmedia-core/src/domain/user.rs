use jiff::Timestamp;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::domain::error::{DomainError, DomainResult};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct UserId(pub Uuid);

impl UserId {
    pub fn new() -> Self {
        Self(Uuid::now_v7())
    }
}

impl Default for UserId {
    fn default() -> Self {
        Self::new()
    }
}

impl std::fmt::Display for UserId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.0.fmt(f)
    }
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
#[serde(rename_all = "lowercase")]
pub enum CatppuccinFlavor {
    Latte,
    Frappe,
    Macchiato,
    #[default]
    Mocha,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum CatppuccinAccent {
    Rosewater,
    Flamingo,
    Pink,
    Mauve,
    Red,
    Maroon,
    Peach,
    Yellow,
    Green,
    Teal,
    Sky,
    Sapphire,
    #[default]
    Blue,
    Lavender,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum Locale {
    #[serde(rename = "zh-CN")]
    #[default]
    ZhCn,
    #[serde(rename = "en-US")]
    EnUs,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CustomPaletteTokens {
    pub canvas: String,
    pub shell: String,
    pub surface: String,
    pub text: String,
    pub subtext: String,
}

// --- Book Reader Preferences ---

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum ReadingFlow {
    #[default]
    Paginated,
    Scrolled,
}

fn default_font_family() -> String {
    "system-ui".to_string()
}
fn default_font_size() -> u16 {
    17
}
fn default_line_height() -> f32 {
    1.7
}
fn default_margin_px() -> u16 {
    36
}
fn default_pdf_scale() -> f32 {
    1.25
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

impl Default for BookReaderPreferences {
    fn default() -> Self {
        Self {
            font_family: default_font_family(),
            font_size: default_font_size(),
            line_height: default_line_height(),
            margin_px: default_margin_px(),
            flow: ReadingFlow::default(),
            pdf_scale: default_pdf_scale(),
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
    3
}
fn default_webtoon_width() -> u16 {
    920
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

impl Default for ComicReaderPreferences {
    fn default() -> Self {
        Self {
            default_reader_mode: ComicReaderMode::default(),
            default_fit_mode: ComicFitMode::default(),
            offset_cover_page: true,
            dim_images_in_dark: false,
            preload_pages: default_preload_pages(),
            webtoon_width: default_webtoon_width(),
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
    0.85
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

impl Default for MusicPlaybackPreferences {
    fn default() -> Self {
        Self {
            transcode_profile: AudioTranscodeProfile::Raw,
            volume: default_volume(),
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
    1.0
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

impl Default for VideoPlaybackPreferences {
    fn default() -> Self {
        Self {
            default_quality: VideoQualityProfile::Auto,
            preferred_audio_lang: None,
            preferred_subtitle_lang: None,
            autoplay_next: true,
            playback_rate: default_playback_rate(),
        }
    }
}

fn default_light_preset() -> String {
    "catppuccin-latte".to_string()
}

fn default_dark_preset() -> String {
    "catppuccin-mocha".to_string()
}

fn default_accent_mode() -> String {
    "catppuccin".to_string()
}

// --- Customizable Workspace Layout Preferences (<= 4 items per slot) ---

pub const MAX_LAYOUT_ITEMS: usize = 4;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DomainLayoutConfig {
    #[serde(default)]
    pub home_cards: Vec<String>,
    #[serde(default)]
    pub sidebar_groups: Vec<String>,
}

fn sanitize_slot(
    raw: &[String],
    allowed: &[&str],
    fallback: &[&str],
    allow_empty: bool,
) -> Vec<String> {
    let mut out = Vec::with_capacity(MAX_LAYOUT_ITEMS);
    for item in raw {
        let trimmed = item.trim();
        if allowed.contains(&trimmed) && !out.iter().any(|existing| existing == trimmed) {
            out.push(trimmed.to_string());
            if out.len() >= MAX_LAYOUT_ITEMS {
                break;
            }
        }
    }
    if out.is_empty() && !allow_empty {
        fallback
            .iter()
            .take(MAX_LAYOUT_ITEMS)
            .map(|s| (*s).to_string())
            .collect()
    } else {
        out
    }
}

fn default_book_layout() -> DomainLayoutConfig {
    DomainLayoutConfig {
        home_cards: vec![
            "all".into(),
            "reading".into(),
            "bookmarks".into(),
            "favorites".into(),
        ],
        sidebar_groups: vec![
            "authors".into(),
            "tags".into(),
            "shelves".into(),
            "libraries".into(),
        ],
    }
}

fn default_comic_layout() -> DomainLayoutConfig {
    DomainLayoutConfig {
        home_cards: vec![
            "all".into(),
            "reading".into(),
            "bookmarks".into(),
            "favorites".into(),
        ],
        sidebar_groups: vec![
            "writers".into(),
            "tags".into(),
            "shelves".into(),
            "libraries".into(),
        ],
    }
}

fn default_music_layout() -> DomainLayoutConfig {
    DomainLayoutConfig {
        home_cards: vec![
            "all".into(),
            "playlists".into(),
            "listening".into(),
            "favorites".into(),
        ],
        sidebar_groups: vec![
            "artists".into(),
            "tags".into(),
            "albums".into(),
            "genres".into(),
        ],
    }
}

fn default_video_layout() -> DomainLayoutConfig {
    DomainLayoutConfig {
        home_cards: vec![
            "all".into(),
            "movies".into(),
            "shows".into(),
            "favorites".into(),
        ],
        sidebar_groups: vec![
            "shows".into(),
            "genres".into(),
            "tags".into(),
            "shelves".into(),
        ],
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkspaceLayoutPreferences {
    #[serde(default = "default_book_layout")]
    pub book: DomainLayoutConfig,
    #[serde(default = "default_comic_layout")]
    pub comic: DomainLayoutConfig,
    #[serde(default = "default_music_layout")]
    pub music: DomainLayoutConfig,
    #[serde(default = "default_video_layout")]
    pub video: DomainLayoutConfig,
}

impl Default for WorkspaceLayoutPreferences {
    fn default() -> Self {
        Self {
            book: default_book_layout(),
            comic: default_comic_layout(),
            music: default_music_layout(),
            video: default_video_layout(),
        }
    }
}

impl WorkspaceLayoutPreferences {
    pub fn sanitized(self) -> Self {
        Self {
            book: DomainLayoutConfig {
                home_cards: sanitize_slot(
                    &self.book.home_cards,
                    &[
                        "all",
                        "reading",
                        "bookmarks",
                        "favorites",
                        "shelves",
                        "tags",
                        "authors",
                        "libraries",
                    ],
                    &["all", "reading", "bookmarks", "favorites"],
                    false,
                ),
                sidebar_groups: sanitize_slot(
                    &self.book.sidebar_groups,
                    &["authors", "tags", "shelves", "libraries", "bookmarks"],
                    &["authors", "tags", "shelves", "libraries"],
                    true,
                ),
            },
            comic: DomainLayoutConfig {
                home_cards: sanitize_slot(
                    &self.comic.home_cards,
                    &[
                        "all",
                        "reading",
                        "bookmarks",
                        "favorites",
                        "chapters",
                        "shelves",
                        "tags",
                        "libraries",
                    ],
                    &["all", "reading", "bookmarks", "favorites"],
                    false,
                ),
                sidebar_groups: sanitize_slot(
                    &self.comic.sidebar_groups,
                    &[
                        "writers",
                        "pencillers",
                        "genres",
                        "tags",
                        "shelves",
                        "libraries",
                        "bookmarks",
                    ],
                    &["writers", "tags", "shelves", "libraries"],
                    true,
                ),
            },
            music: DomainLayoutConfig {
                home_cards: sanitize_slot(
                    &self.music.home_cards,
                    &[
                        "all",
                        "playlists",
                        "listening",
                        "favorites",
                        "bookmarks",
                        "albums",
                        "artists",
                        "genres",
                        "tags",
                        "libraries",
                    ],
                    &["all", "playlists", "listening", "favorites"],
                    false,
                ),
                sidebar_groups: sanitize_slot(
                    &self.music.sidebar_groups,
                    &[
                        "artists",
                        "tags",
                        "albums",
                        "genres",
                        "playlists",
                        "libraries",
                    ],
                    &["artists", "tags", "albums", "genres"],
                    true,
                ),
            },
            video: DomainLayoutConfig {
                home_cards: sanitize_slot(
                    &self.video.home_cards,
                    &[
                        "all",
                        "movies",
                        "shows",
                        "favorites",
                        "watching",
                        "shelves",
                        "genres",
                        "tags",
                        "libraries",
                    ],
                    &["all", "movies", "shows", "favorites"],
                    false,
                ),
                sidebar_groups: sanitize_slot(
                    &self.video.sidebar_groups,
                    &["shows", "genres", "tags", "shelves", "libraries"],
                    &["shows", "genres", "tags", "shelves"],
                    true,
                ),
            },
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct UserPreferences {
    #[serde(default)]
    pub theme_mode: ThemeMode,
    #[serde(default = "default_light_preset")]
    pub light_preset: String,
    #[serde(default = "default_dark_preset")]
    pub dark_preset: String,
    #[serde(default)]
    pub dark_flavor: CatppuccinFlavor,
    #[serde(default = "default_accent_mode")]
    pub accent_mode: String,
    #[serde(default)]
    pub accent: CatppuccinAccent,
    #[serde(default)]
    pub custom_accent: Option<String>,
    #[serde(default)]
    pub custom_light: Option<CustomPaletteTokens>,
    #[serde(default)]
    pub custom_dark: Option<CustomPaletteTokens>,
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
    #[serde(default)]
    pub workspace_layout: WorkspaceLayoutPreferences,
}

impl Default for UserPreferences {
    fn default() -> Self {
        Self {
            theme_mode: ThemeMode::default(),
            light_preset: default_light_preset(),
            dark_preset: default_dark_preset(),
            dark_flavor: CatppuccinFlavor::default(),
            accent_mode: default_accent_mode(),
            accent: CatppuccinAccent::default(),
            custom_accent: None,
            custom_light: None,
            custom_dark: None,
            locale: Locale::default(),
            book: BookReaderPreferences::default(),
            comic: ComicReaderPreferences::default(),
            music: MusicPlaybackPreferences::default(),
            video: VideoPlaybackPreferences::default(),
            workspace_layout: WorkspaceLayoutPreferences::default(),
        }
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
            if trimmed.is_empty() {
                return Err(DomainError::Validation(
                    "display name cannot be empty".into(),
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
        preferences.workspace_layout = preferences.workspace_layout.sanitized();
        self.preferences = preferences;
        self.updated_at = Timestamp::now();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn workspace_layout_clamps_to_four_and_deduplicates() {
        let mut prefs = WorkspaceLayoutPreferences::default();
        prefs.book.home_cards = vec![
            "shelves".into(),
            "shelves".into(),
            "tags".into(),
            "all".into(),
            "reading".into(),
            "bookmarks".into(),
            "invalid".into(),
        ];
        prefs.book.sidebar_groups = vec![];
        let clean = prefs.sanitized();
        assert_eq!(
            clean.book.home_cards,
            vec!["shelves", "tags", "all", "reading"]
        );
        assert!(clean.book.sidebar_groups.is_empty());
    }
}

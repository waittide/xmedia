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

fn is_valid_hex_color(raw: &str) -> bool {
    let Some(hex) = raw.strip_prefix('#') else {
        return false;
    };
    matches!(hex.len(), 3 | 6 | 8) && hex.bytes().all(|b| b.is_ascii_hexdigit())
}

fn sanitize_hex_color(raw: &str) -> Option<String> {
    let trimmed = raw.trim();
    if is_valid_hex_color(trimmed) {
        Some(trimmed.to_ascii_lowercase())
    } else {
        None
    }
}

impl CustomPaletteTokens {
    pub fn normalize(&mut self) -> bool {
        let Some(canvas) = sanitize_hex_color(&self.canvas) else {
            return false;
        };
        let Some(shell) = sanitize_hex_color(&self.shell) else {
            return false;
        };
        let Some(surface) = sanitize_hex_color(&self.surface) else {
            return false;
        };
        let Some(text) = sanitize_hex_color(&self.text) else {
            return false;
        };
        let Some(subtext) = sanitize_hex_color(&self.subtext) else {
            return false;
        };
        self.canvas = canvas;
        self.shell = shell;
        self.surface = surface;
        self.text = text;
        self.subtext = subtext;
        true
    }
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
const DEFAULT_LIGHT_PRESET: &str = "catppuccin-latte";
const DEFAULT_DARK_PRESET: &str = "catppuccin-mocha";
const DEFAULT_ACCENT_MODE: &str = "catppuccin";

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

fn default_light_preset() -> String {
    DEFAULT_LIGHT_PRESET.to_string()
}

fn default_dark_preset() -> String {
    DEFAULT_DARK_PRESET.to_string()
}

fn default_accent_mode() -> String {
    DEFAULT_ACCENT_MODE.to_string()
}

// --- Customizable Workspace Layout Preferences (<= 4 items per slot) ---

pub const MAX_LAYOUT_ITEMS: usize = 4;

const BOOK_DEFAULT_HOME_CARDS: &[&str] = &["all", "reading", "bookmarks", "favorites"];
const BOOK_ALLOWED_HOME_CARDS: &[&str] = &[
    "all",
    "reading",
    "bookmarks",
    "favorites",
    "shelves",
    "tags",
    "authors",
    "libraries",
];
const BOOK_DEFAULT_SIDEBAR_GROUPS: &[&str] = &["authors", "tags", "shelves", "libraries"];
const BOOK_ALLOWED_SIDEBAR_GROUPS: &[&str] =
    &["authors", "tags", "shelves", "libraries", "bookmarks"];

const COMIC_DEFAULT_HOME_CARDS: &[&str] = &["all", "reading", "bookmarks", "favorites"];
const COMIC_ALLOWED_HOME_CARDS: &[&str] = &[
    "all",
    "reading",
    "bookmarks",
    "favorites",
    "chapters",
    "shelves",
    "tags",
    "libraries",
];
const COMIC_DEFAULT_SIDEBAR_GROUPS: &[&str] = &["writers", "tags", "shelves", "libraries"];
const COMIC_ALLOWED_SIDEBAR_GROUPS: &[&str] = &[
    "writers",
    "pencillers",
    "genres",
    "tags",
    "shelves",
    "libraries",
    "bookmarks",
];

const MUSIC_DEFAULT_HOME_CARDS: &[&str] = &["all", "playlists", "listening", "favorites"];
const MUSIC_ALLOWED_HOME_CARDS: &[&str] = &[
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
];
const MUSIC_DEFAULT_SIDEBAR_GROUPS: &[&str] = &["artists", "tags", "albums", "genres"];
const MUSIC_ALLOWED_SIDEBAR_GROUPS: &[&str] = &[
    "artists",
    "tags",
    "albums",
    "genres",
    "playlists",
    "libraries",
];

const VIDEO_DEFAULT_HOME_CARDS: &[&str] = &["all", "movies", "shows", "favorites"];
const VIDEO_ALLOWED_HOME_CARDS: &[&str] = &[
    "all",
    "movies",
    "shows",
    "favorites",
    "watching",
    "shelves",
    "genres",
    "tags",
    "libraries",
];
const VIDEO_DEFAULT_SIDEBAR_GROUPS: &[&str] = &["shows", "genres", "tags", "shelves"];
const VIDEO_ALLOWED_SIDEBAR_GROUPS: &[&str] = &["shows", "genres", "tags", "shelves", "libraries"];

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DomainLayoutConfig {
    #[serde(default)]
    pub home_cards: Vec<String>,
    #[serde(default)]
    pub sidebar_groups: Vec<String>,
}

fn to_owned_slot(items: &[&str]) -> Vec<String> {
    items.iter().map(|s| (*s).to_string()).collect()
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
        home_cards: to_owned_slot(BOOK_DEFAULT_HOME_CARDS),
        sidebar_groups: to_owned_slot(BOOK_DEFAULT_SIDEBAR_GROUPS),
    }
}

fn default_comic_layout() -> DomainLayoutConfig {
    DomainLayoutConfig {
        home_cards: to_owned_slot(COMIC_DEFAULT_HOME_CARDS),
        sidebar_groups: to_owned_slot(COMIC_DEFAULT_SIDEBAR_GROUPS),
    }
}

fn default_music_layout() -> DomainLayoutConfig {
    DomainLayoutConfig {
        home_cards: to_owned_slot(MUSIC_DEFAULT_HOME_CARDS),
        sidebar_groups: to_owned_slot(MUSIC_DEFAULT_SIDEBAR_GROUPS),
    }
}

fn default_video_layout() -> DomainLayoutConfig {
    DomainLayoutConfig {
        home_cards: to_owned_slot(VIDEO_DEFAULT_HOME_CARDS),
        sidebar_groups: to_owned_slot(VIDEO_DEFAULT_SIDEBAR_GROUPS),
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
    pub fn sanitize(&mut self) {
        self.book.home_cards = sanitize_slot(
            &self.book.home_cards,
            BOOK_ALLOWED_HOME_CARDS,
            BOOK_DEFAULT_HOME_CARDS,
            false,
        );
        self.book.sidebar_groups = sanitize_slot(
            &self.book.sidebar_groups,
            BOOK_ALLOWED_SIDEBAR_GROUPS,
            BOOK_DEFAULT_SIDEBAR_GROUPS,
            true,
        );
        self.comic.home_cards = sanitize_slot(
            &self.comic.home_cards,
            COMIC_ALLOWED_HOME_CARDS,
            COMIC_DEFAULT_HOME_CARDS,
            false,
        );
        self.comic.sidebar_groups = sanitize_slot(
            &self.comic.sidebar_groups,
            COMIC_ALLOWED_SIDEBAR_GROUPS,
            COMIC_DEFAULT_SIDEBAR_GROUPS,
            true,
        );
        self.music.home_cards = sanitize_slot(
            &self.music.home_cards,
            MUSIC_ALLOWED_HOME_CARDS,
            MUSIC_DEFAULT_HOME_CARDS,
            false,
        );
        self.music.sidebar_groups = sanitize_slot(
            &self.music.sidebar_groups,
            MUSIC_ALLOWED_SIDEBAR_GROUPS,
            MUSIC_DEFAULT_SIDEBAR_GROUPS,
            true,
        );
        self.video.home_cards = sanitize_slot(
            &self.video.home_cards,
            VIDEO_ALLOWED_HOME_CARDS,
            VIDEO_DEFAULT_HOME_CARDS,
            false,
        );
        self.video.sidebar_groups = sanitize_slot(
            &self.video.sidebar_groups,
            VIDEO_ALLOWED_SIDEBAR_GROUPS,
            VIDEO_DEFAULT_SIDEBAR_GROUPS,
            true,
        );
    }

    pub fn sanitized(mut self) -> Self {
        self.sanitize();
        self
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

impl UserPreferences {
    /// Clamps and sanitizes all preference values into safe bounds while preserving JSON schema compatibility.
    pub fn normalize(&mut self) {
        normalize_non_empty(&mut self.light_preset, DEFAULT_LIGHT_PRESET, 64);
        normalize_non_empty(&mut self.dark_preset, DEFAULT_DARK_PRESET, 64);
        normalize_non_empty(&mut self.accent_mode, DEFAULT_ACCENT_MODE, 32);
        self.custom_accent = self.custom_accent.as_deref().and_then(sanitize_hex_color);
        if let Some(ref mut light) = self.custom_light
            && !light.normalize()
        {
            self.custom_light = None;
        }
        if let Some(ref mut dark) = self.custom_dark
            && !dark.normalize()
        {
            self.custom_dark = None;
        }
        self.book.normalize();
        self.comic.normalize();
        self.music.normalize();
        self.video.normalize();
        self.workspace_layout.sanitize();
    }

    pub fn normalized(mut self) -> Self {
        self.normalize();
        self
    }
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

    #[test]
    fn user_preferences_normalize_clamps_numeric_and_layout_bounds() {
        let mut prefs = UserPreferences {
            light_preset: "   ".into(),
            ..Default::default()
        };
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
        prefs.workspace_layout.music.home_cards = vec!["invalid".into()];

        prefs.normalize();

        assert_eq!(prefs.light_preset, DEFAULT_LIGHT_PRESET);
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
        assert_eq!(
            prefs.workspace_layout.music.home_cards,
            vec!["all", "playlists", "listening", "favorites"]
        );
    }
}

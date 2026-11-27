#[cfg(test)]
mod tests;

use serde::{Deserialize, Serialize};

use crate::domain::library::MediaKind;

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

/// Shell/UI appearance and localization preferences.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct UiPreferences {
    #[serde(default)]
    pub theme_mode: ThemeMode,
    /// Single MD3 seed color (`#rrggbb`); `None` means the client default seed.
    #[serde(default)]
    pub seed_color: Option<String>,
    /// Optional icon variant id (`porcelain` | `obsidian` | `monolith`); `None` means default (`cobalt`).
    #[serde(default)]
    pub app_icon: Option<String>,
    /// Optional custom UI font spec (`font:<uuid>` or local family name); `None` means `system-ui`.
    #[serde(default)]
    pub ui_font: Option<String>,
    #[serde(default)]
    pub locale: Locale,
    /// Last selected media domain remembered across sessions.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub active_domain: Option<MediaKind>,
    /// Whether left sidebar / navigation rail items display text labels.
    #[serde(default = "default_true")]
    pub show_sidebar_labels: bool,
    /// Whether bottom and top navigation bars display text labels.
    #[serde(default = "default_true")]
    pub show_nav_labels: bool,
}

impl Default for UiPreferences {
    fn default() -> Self {
        Self {
            theme_mode: ThemeMode::default(),
            seed_color: None,
            app_icon: None,
            ui_font: None,
            locale: Locale::default(),
            active_domain: None,
            show_sidebar_labels: true,
            show_nav_labels: true,
        }
    }
}

impl UiPreferences {
    pub(super) fn normalize(&mut self) {
        self.seed_color = self.seed_color.as_deref().and_then(sanitize_hex_color);
        self.app_icon = self.app_icon.as_deref().and_then(sanitize_app_icon);
        self.ui_font = self.ui_font.as_deref().and_then(sanitize_font_spec);
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

pub(crate) const DEFAULT_FONT_FAMILY: &str = "system-ui";
const DEFAULT_FONT_SIZE: u16 = 17;
const MIN_FONT_SIZE: u16 = 10;
pub(crate) const MAX_FONT_SIZE: u16 = 48;
pub(crate) const DEFAULT_LINE_HEIGHT: f32 = 1.7;
const MIN_LINE_HEIGHT: f32 = 1.0;
const MAX_LINE_HEIGHT: f32 = 3.0;
const DEFAULT_MARGIN_PX: u16 = 36;
const DEFAULT_MARGIN_VERTICAL_PX: u16 = 24;
pub(crate) const MAX_MARGIN_PX: u16 = 240;
pub(crate) const DEFAULT_PARAGRAPH_INDENT_EM: f32 = 2.0;
const MIN_PARAGRAPH_INDENT_EM: f32 = 0.0;
pub(crate) const MAX_PARAGRAPH_INDENT_EM: f32 = 6.0;
pub(crate) const DEFAULT_LETTER_SPACING_EM: f32 = 0.0;
pub(crate) const MIN_LETTER_SPACING_EM: f32 = -0.10;
pub(crate) const MAX_LETTER_SPACING_EM: f32 = 0.50;
const DEFAULT_PARAGRAPH_SPACING_PX: u16 = 16;
pub(crate) const MAX_PARAGRAPH_SPACING_PX: u16 = 80;
const DEFAULT_PDF_SCALE: f32 = 1.25;
pub(crate) const MIN_PDF_SCALE: f32 = 0.25;
const MAX_PDF_SCALE: f32 = 4.0;
const DEFAULT_PRELOAD_PAGES: u8 = 3;
pub(crate) const MAX_PRELOAD_PAGES: u8 = 20;
const DEFAULT_WEBTOON_WIDTH: u16 = 920;
pub(crate) const MIN_WEBTOON_WIDTH: u16 = 320;
const MAX_WEBTOON_WIDTH: u16 = 2400;
const DEFAULT_VOLUME: f32 = 0.85;
const DEFAULT_PLAYBACK_RATE: f32 = 1.0;
const MIN_PLAYBACK_RATE: f32 = 0.25;
pub(crate) const MAX_PLAYBACK_RATE: f32 = 4.0;

/// Normalizes a CSS hex color to lowercase `#rrggbb` (expanding `#rgb`).
fn sanitize_hex_color(raw: &str) -> Option<String> {
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

const MAX_FONT_SPEC_CHARS: usize = 128;

fn sanitize_css_font_chars(raw: &str) -> String {
    let filtered: String = raw
        .chars()
        .filter(|&c| {
            !c.is_control() && !matches!(c, ';' | '{' | '}' | '<' | '>' | '\\' | '"' | '\'')
        })
        .collect();
    let trimmed = filtered.trim();
    let capped: String = trimmed.chars().take(MAX_FONT_SPEC_CHARS).collect();
    capped.trim_end().to_string()
}

/// Sanitizes an optional UI font spec, stripping dangerous CSS/control characters, capping at 128
/// characters, and normalizing empty or `"system-ui"` (case-insensitive) to `None`.
fn sanitize_font_spec(raw: &str) -> Option<String> {
    let cleaned = sanitize_css_font_chars(raw);
    if cleaned.is_empty() || cleaned.eq_ignore_ascii_case(DEFAULT_FONT_FAMILY) {
        None
    } else {
        Some(cleaned)
    }
}

fn clamp_finite(value: f32, min: f32, max: f32, fallback: f32) -> f32 {
    if value.is_finite() {
        value.clamp(min, max)
    } else {
        fallback
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
fn default_margin_vertical_px() -> u16 {
    DEFAULT_MARGIN_VERTICAL_PX
}
fn default_paragraph_indent_em() -> f32 {
    DEFAULT_PARAGRAPH_INDENT_EM
}
fn default_letter_spacing_em() -> f32 {
    DEFAULT_LETTER_SPACING_EM
}
fn default_paragraph_spacing_px() -> u16 {
    DEFAULT_PARAGRAPH_SPACING_PX
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
    #[serde(default = "default_margin_vertical_px")]
    pub margin_top_px: u16,
    #[serde(default = "default_margin_vertical_px")]
    pub margin_bottom_px: u16,
    #[serde(default = "default_margin_px")]
    pub margin_left_px: u16,
    #[serde(default = "default_margin_px")]
    pub margin_right_px: u16,
    #[serde(default = "default_paragraph_indent_em")]
    pub paragraph_indent_em: f32,
    #[serde(default = "default_letter_spacing_em")]
    pub letter_spacing_em: f32,
    #[serde(default = "default_paragraph_spacing_px")]
    pub paragraph_spacing_px: u16,
    #[serde(default)]
    pub text_color: Option<String>,
    #[serde(default)]
    pub bg_color: Option<String>,
    #[serde(default)]
    pub flow: ReadingFlow,
    #[serde(default = "default_pdf_scale")]
    pub pdf_scale: f32,
}

impl BookReaderPreferences {
    pub(super) fn normalize(&mut self) {
        let cleaned_font = sanitize_css_font_chars(&self.font_family);
        self.font_family = if cleaned_font.is_empty() {
            DEFAULT_FONT_FAMILY.to_string()
        } else {
            cleaned_font
        };
        self.font_size = self.font_size.clamp(MIN_FONT_SIZE, MAX_FONT_SIZE);
        self.line_height = clamp_finite(
            self.line_height,
            MIN_LINE_HEIGHT,
            MAX_LINE_HEIGHT,
            DEFAULT_LINE_HEIGHT,
        );
        self.margin_px = self.margin_px.min(MAX_MARGIN_PX);
        self.margin_top_px = self.margin_top_px.min(MAX_MARGIN_PX);
        self.margin_bottom_px = self.margin_bottom_px.min(MAX_MARGIN_PX);
        self.margin_left_px = self.margin_left_px.min(MAX_MARGIN_PX);
        self.margin_right_px = self.margin_right_px.min(MAX_MARGIN_PX);
        self.paragraph_indent_em = clamp_finite(
            self.paragraph_indent_em,
            MIN_PARAGRAPH_INDENT_EM,
            MAX_PARAGRAPH_INDENT_EM,
            DEFAULT_PARAGRAPH_INDENT_EM,
        );
        self.letter_spacing_em = clamp_finite(
            self.letter_spacing_em,
            MIN_LETTER_SPACING_EM,
            MAX_LETTER_SPACING_EM,
            DEFAULT_LETTER_SPACING_EM,
        );
        self.paragraph_spacing_px = self.paragraph_spacing_px.min(MAX_PARAGRAPH_SPACING_PX);
        self.text_color = self.text_color.as_deref().and_then(sanitize_hex_color);
        self.bg_color = self.bg_color.as_deref().and_then(sanitize_hex_color);
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
            margin_top_px: DEFAULT_MARGIN_VERTICAL_PX,
            margin_bottom_px: DEFAULT_MARGIN_VERTICAL_PX,
            margin_left_px: DEFAULT_MARGIN_PX,
            margin_right_px: DEFAULT_MARGIN_PX,
            paragraph_indent_em: DEFAULT_PARAGRAPH_INDENT_EM,
            letter_spacing_em: DEFAULT_LETTER_SPACING_EM,
            paragraph_spacing_px: DEFAULT_PARAGRAPH_SPACING_PX,
            text_color: None,
            bg_color: None,
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ComicSeriesReaderOverride {
    pub reader_mode: ComicReaderMode,
    pub fit_mode: ComicFitMode,
    pub offset_cover_page: bool,
    pub webtoon_width: u16,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ResolvedComicReaderPref {
    pub reader_mode: ComicReaderMode,
    pub fit_mode: ComicFitMode,
    pub offset_cover_page: bool,
    pub dim_images_in_dark: bool,
    pub preload_pages: u8,
    pub webtoon_width: u16,
}

fn resolve_comic_reader_pref(
    prefs: &ComicReaderPreferences,
    is_webtoon_series: bool,
    series_override: Option<ComicSeriesReaderOverride>,
) -> ResolvedComicReaderPref {
    if let Some(sp) = series_override {
        ResolvedComicReaderPref {
            reader_mode: sp.reader_mode,
            fit_mode: sp.fit_mode,
            offset_cover_page: sp.offset_cover_page,
            dim_images_in_dark: prefs.dim_images_in_dark,
            preload_pages: prefs.preload_pages,
            webtoon_width: sp.webtoon_width,
        }
    } else {
        let reader_mode =
            if is_webtoon_series && prefs.default_reader_mode == ComicReaderMode::RtlDouble {
                ComicReaderMode::Webtoon
            } else {
                prefs.default_reader_mode
            };
        ResolvedComicReaderPref {
            reader_mode,
            fit_mode: prefs.default_fit_mode,
            offset_cover_page: prefs.offset_cover_page,
            dim_images_in_dark: prefs.dim_images_in_dark,
            preload_pages: prefs.preload_pages,
            webtoon_width: prefs.webtoon_width,
        }
    }
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
    pub(super) fn normalize(&mut self) {
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

// --- Music Player Preferences ---

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

pub type MusicPlayerPreferences = MusicPlaybackPreferences;

impl MusicPlaybackPreferences {
    pub(super) fn normalize(&mut self) {
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

// --- Video Player Preferences ---

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

pub type VideoPlayerPreferences = VideoPlaybackPreferences;

impl VideoPlaybackPreferences {
    pub(super) fn normalize(&mut self) {
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

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct UserPreferences {
    #[serde(default)]
    theme_mode: ThemeMode,
    /// Single MD3 seed color (`#rrggbb`); `None` means the client default seed.
    #[serde(default)]
    seed_color: Option<String>,
    /// Optional icon variant id (`porcelain` | `obsidian` | `monolith`); `None` means default (`cobalt`).
    #[serde(default)]
    app_icon: Option<String>,
    /// Optional custom UI font spec (`font:<uuid>` or local family name); `None` means `system-ui`.
    #[serde(default)]
    ui_font: Option<String>,
    #[serde(default)]
    locale: Locale,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    active_domain: Option<MediaKind>,
    #[serde(default = "default_true")]
    show_sidebar_labels: bool,
    #[serde(default = "default_true")]
    show_nav_labels: bool,
    #[serde(default)]
    book: BookReaderPreferences,
    #[serde(default)]
    comic: ComicReaderPreferences,
    #[serde(default)]
    music: MusicPlaybackPreferences,
    #[serde(default)]
    video: VideoPlaybackPreferences,
}

impl Default for UserPreferences {
    fn default() -> Self {
        Self {
            theme_mode: ThemeMode::default(),
            seed_color: None,
            app_icon: None,
            ui_font: None,
            locale: Locale::default(),
            active_domain: None,
            show_sidebar_labels: true,
            show_nav_labels: true,
            book: BookReaderPreferences::default(),
            comic: ComicReaderPreferences::default(),
            music: MusicPlaybackPreferences::default(),
            video: VideoPlaybackPreferences::default(),
        }
    }
}

impl UserPreferences {
    pub fn ui(&self) -> UiPreferences {
        UiPreferences {
            theme_mode: self.theme_mode,
            seed_color: self.seed_color.clone(),
            app_icon: self.app_icon.clone(),
            ui_font: self.ui_font.clone(),
            locale: self.locale,
            active_domain: self.active_domain,
            show_sidebar_labels: self.show_sidebar_labels,
            show_nav_labels: self.show_nav_labels,
        }
    }

    pub fn resolve_comic_reader(
        &self,
        is_webtoon_series: bool,
        series_override: Option<ComicSeriesReaderOverride>,
    ) -> ResolvedComicReaderPref {
        resolve_comic_reader_pref(&self.comic, is_webtoon_series, series_override)
    }

    pub fn apply_ui(&mut self, mut ui: UiPreferences) {
        ui.normalize();
        self.theme_mode = ui.theme_mode;
        self.seed_color = ui.seed_color;
        self.app_icon = ui.app_icon;
        self.ui_font = ui.ui_font;
        self.locale = ui.locale;
        self.active_domain = ui.active_domain;
        self.show_sidebar_labels = ui.show_sidebar_labels;
        self.show_nav_labels = ui.show_nav_labels;
    }

    /// Clamps and sanitizes all preference values into safe bounds.
    fn normalize(&mut self) {
        let mut ui = self.ui();
        ui.normalize();
        self.theme_mode = ui.theme_mode;
        self.seed_color = ui.seed_color;
        self.app_icon = ui.app_icon;
        self.ui_font = ui.ui_font;
        self.locale = ui.locale;
        self.active_domain = ui.active_domain;
        self.show_sidebar_labels = ui.show_sidebar_labels;
        self.show_nav_labels = ui.show_nav_labels;
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

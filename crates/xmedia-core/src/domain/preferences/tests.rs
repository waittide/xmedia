use super::*;

#[test]
fn user_preferences_normalize_clamps_numeric_bounds() {
    let mut prefs = UserPreferences::default();
    prefs.book.font_family = " ".into();
    prefs.book.font_size = 200;
    prefs.book.line_height = f32::NAN;
    prefs.book.margin_px = 999;
    prefs.book.margin_top_px = 500;
    prefs.book.margin_bottom_px = 500;
    prefs.book.margin_left_px = 500;
    prefs.book.margin_right_px = 500;
    prefs.book.paragraph_indent_em = 99.0;
    prefs.book.letter_spacing_em = -5.0;
    prefs.book.paragraph_spacing_px = 500;
    prefs.book.text_color = Some(" #2C2218 ".into());
    prefs.book.bg_color = Some("not-a-hex".into());
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
    assert_eq!(prefs.book.margin_top_px, MAX_MARGIN_PX);
    assert_eq!(prefs.book.margin_bottom_px, MAX_MARGIN_PX);
    assert_eq!(prefs.book.margin_left_px, MAX_MARGIN_PX);
    assert_eq!(prefs.book.margin_right_px, MAX_MARGIN_PX);
    assert_eq!(prefs.book.paragraph_indent_em, MAX_PARAGRAPH_INDENT_EM);
    assert_eq!(prefs.book.letter_spacing_em, MIN_LETTER_SPACING_EM);
    assert_eq!(prefs.book.paragraph_spacing_px, MAX_PARAGRAPH_SPACING_PX);
    assert_eq!(prefs.book.text_color.as_deref(), Some("#2c2218"));
    assert_eq!(prefs.book.bg_color, None);
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
    assert_eq!(prefs.ui().locale, Locale::ZhCn);
}

#[test]
fn seed_color_and_app_icon_are_sanitized_via_ui_preferences() {
    let mut prefs = UserPreferences::default();
    prefs.apply_ui(UiPreferences {
        theme_mode: ThemeMode::Dark,
        seed_color: Some(" #6750A4 ".into()),
        app_icon: Some(" Porcelain ".into()),
        ui_font: Some("  LXGW WenKai  ".into()),
        locale: Locale::EnUs,
        active_domain: Some(MediaKind::Music),
    });
    assert_eq!(prefs.theme_mode, ThemeMode::Dark);
    assert_eq!(prefs.seed_color.as_deref(), Some("#6750a4"));
    assert_eq!(prefs.app_icon.as_deref(), Some("porcelain"));
    assert_eq!(prefs.ui_font.as_deref(), Some("LXGW WenKai"));
    assert_eq!(prefs.locale, Locale::EnUs);
    assert_eq!(prefs.active_domain, Some(MediaKind::Music));

    prefs.seed_color = Some("#ABC".into());
    prefs.normalize();
    assert_eq!(prefs.seed_color.as_deref(), Some("#aabbcc"));

    for invalid in ["6750a4", "#6750a4ff", "#zzzzzz", ""] {
        prefs.seed_color = Some(invalid.into());
        prefs.normalize();
        assert_eq!(prefs.seed_color, None, "{invalid} should be rejected");
    }

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

#[test]
fn ui_font_and_book_font_family_strip_dangerous_css_chars_and_normalize_defaults() {
    assert_eq!(sanitize_font_spec("   "), None);
    assert_eq!(sanitize_font_spec("system-ui"), None);
    assert_eq!(sanitize_font_spec("  SYSTEM-UI  "), None);
    assert_eq!(sanitize_font_spec("\";{}<>\\'\0\r\n\t"), None);
    assert_eq!(
        sanitize_font_spec("  \"霞鹜文楷\";{}<>\\'  ").as_deref(),
        Some("霞鹜文楷")
    );
    assert_eq!(
        sanitize_font_spec("font:0195aabb-ccdd-7000-8000-112233445566").as_deref(),
        Some("font:0195aabb-ccdd-7000-8000-112233445566")
    );

    let overlong = "A".repeat(200);
    let capped = sanitize_font_spec(&overlong).unwrap();
    assert_eq!(capped.chars().count(), 128);

    let mut prefs = UserPreferences {
        ui_font: Some("  System-Ui ".into()),
        ..UserPreferences::default()
    };
    prefs.book.font_family = " \";{}<>\\'\r\n ".into();
    prefs.normalize();
    assert_eq!(prefs.ui_font, None);
    assert_eq!(prefs.book.font_family, DEFAULT_FONT_FAMILY);

    prefs.ui_font = Some("Evil\"; } body { display: none".into());
    prefs.book.font_family = "  'LXGW WenKai'; color: red  ".into();
    prefs.normalize();
    assert_eq!(prefs.ui_font.as_deref(), Some("Evil  body  display: none"));
    assert_eq!(prefs.book.font_family, "LXGW WenKai color: red");
    for s in [prefs.ui_font.as_deref().unwrap(), &prefs.book.font_family] {
        assert!(!s.contains([';', '{', '}', '<', '>', '\\', '"', '\'']));
    }
}

use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::{Arc, RwLock};
#[cfg(target_os = "android")]
use std::time::Duration;

use serde::{Deserialize, Serialize};
#[cfg(target_os = "android")]
use tokio::sync::oneshot;

const MAX_SYSTEM_FONT_BYTES: u64 = 48 * 1024 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Default, Serialize, Deserialize)]
pub struct SafeAreaInsetsDto {
    pub top: f32,
    pub right: f32,
    pub bottom: f32,
    pub left: f32,
}

impl SafeAreaInsetsDto {
    pub fn is_non_zero(self) -> bool {
        self.top > 0.0 || self.right > 0.0 || self.bottom > 0.0 || self.left > 0.0
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SystemFontFaceDto {
    pub slot: usize,
    pub family_index: usize,
    pub css_format: String,
    pub variable: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub weight: Option<u16>,
}

#[derive(Debug, Clone)]
pub enum SystemFontSource {
    File(PathBuf),
    Memory(Arc<[u8]>),
}

#[derive(Debug, Clone)]
pub struct SystemFontEntry {
    pub source: SystemFontSource,
    pub mime_type: &'static str,
    pub dto: SystemFontFaceDto,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SfntHeaderInspection {
    pub mime_type: &'static str,
    pub css_format: &'static str,
    pub variable: bool,
}

pub fn inspect_font_header_bytes(buf: &[u8]) -> Option<SfntHeaderInspection> {
    if buf.len() < 12 {
        return None;
    }
    let magic = &buf[0..4];
    let (mime_type, css_format, sfnt_offset) = match magic {
        [0x00, 0x01, 0x00, 0x00] | b"true" | b"typ1" => ("font/ttf", "truetype", Some(0usize)),
        b"OTTO" => ("font/otf", "opentype", Some(0usize)),
        b"ttcf" => {
            if buf.len() < 16 {
                return None;
            }
            let count = u32::from_be_bytes([buf[8], buf[9], buf[10], buf[11]]) as usize;
            let first_offset = if count > 0 {
                u32::from_be_bytes([buf[12], buf[13], buf[14], buf[15]]) as usize
            } else {
                0
            };
            ("font/collection", "truetype", Some(first_offset))
        }
        b"wOFF" => ("font/woff", "woff", None),
        b"wOF2" => ("font/woff2", "woff2", None),
        _ => return None,
    };

    let mut variable = false;
    if let Some(base) = sfnt_offset
        && base.saturating_add(12) <= buf.len()
    {
        let num_tables = u16::from_be_bytes([buf[base + 4], buf[base + 5]]) as usize;
        for i in 0..num_tables.min(128) {
            let rec = base.saturating_add(12).saturating_add(i.saturating_mul(16));
            if rec.saturating_add(4) > buf.len() {
                break;
            }
            if &buf[rec..rec + 4] == b"fvar" {
                variable = true;
                break;
            }
        }
    }

    Some(SfntHeaderInspection {
        mime_type,
        css_format,
        variable,
    })
}

pub fn inspect_system_font_file(path: &Path) -> Option<SfntHeaderInspection> {
    let meta = std::fs::metadata(path).ok()?;
    if !meta.is_file() || !(16..=MAX_SYSTEM_FONT_BYTES).contains(&meta.len()) {
        return None;
    }
    let mut file = std::fs::File::open(path).ok()?;
    let mut header = [0u8; 2048];
    let n = file.read(&mut header).ok()?;
    inspect_font_header_bytes(&header[..n])
}

pub fn should_skip_stock_cjk_collection(path: &Path, ttc_index: i32) -> bool {
    if ttc_index > 0 {
        return true;
    }
    let Some(name) = path.file_name().and_then(|s| s.to_str()) else {
        return false;
    };
    let lower = name.to_ascii_lowercase();
    lower.ends_with(".ttc")
        && (lower.starts_with("notosanscjk") || lower.starts_with("notoserifcjk"))
}

pub fn physical_insets_to_css_px(
    top_px: i32,
    right_px: i32,
    bottom_px: i32,
    left_px: i32,
    density: f32,
) -> SafeAreaInsetsDto {
    let scale = if density.is_finite() && density > 0.0 {
        density
    } else {
        1.0
    };
    let to_css = |px: i32| -> f32 {
        let clamped = px.max(0) as f32 / scale;
        (clamped * 100.0).round() / 100.0
    };
    SafeAreaInsetsDto {
        top: to_css(top_px),
        right: to_css(right_px),
        bottom: to_css(bottom_px),
        left: to_css(left_px),
    }
}

pub fn build_host_safe_area_js(insets: SafeAreaInsetsDto) -> String {
    format!(
        "(function(){{var d=document.documentElement;if(!d||!d.style)return;\
         d.style.setProperty('--host-safe-area-top','{:.2}px');\
         d.style.setProperty('--host-safe-area-right','{:.2}px');\
         d.style.setProperty('--host-safe-area-bottom','{:.2}px');\
         d.style.setProperty('--host-safe-area-left','{:.2}px');}})();",
        insets.top.max(0.0),
        insets.right.max(0.0),
        insets.bottom.max(0.0),
        insets.left.max(0.0),
    )
}

#[cfg(target_os = "android")]
type AndroidSyncFn = Arc<
    dyn Fn(Option<bool>, oneshot::Sender<(SafeAreaInsetsDto, Vec<SystemFontEntry>)>) + Send + Sync,
>;

#[derive(Default)]
struct MobileUiBridgeInner {
    cached_insets: RwLock<Option<SafeAreaInsetsDto>>,
    system_fonts: RwLock<Vec<SystemFontEntry>>,
    last_dark: RwLock<Option<bool>>,
    #[cfg(target_os = "android")]
    android_sync: RwLock<Option<AndroidSyncFn>>,
}

#[derive(Clone, Default)]
pub struct MobileUiBridge {
    inner: Arc<MobileUiBridgeInner>,
}

impl MobileUiBridge {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn cached_safe_area(&self) -> Option<SafeAreaInsetsDto> {
        self.inner.cached_insets.read().ok().and_then(|g| *g)
    }

    pub fn system_font_dtos(&self) -> Vec<SystemFontFaceDto> {
        self.inner
            .system_fonts
            .read()
            .map(|g| g.iter().map(|e| e.dto.clone()).collect())
            .unwrap_or_default()
    }

    pub fn get_system_font(&self, slot: usize) -> Option<SystemFontEntry> {
        self.inner
            .system_fonts
            .read()
            .ok()
            .and_then(|g| g.get(slot).cloned())
    }

    pub async fn query_safe_area(&self) -> Option<SafeAreaInsetsDto> {
        let dark = self.inner.last_dark.read().ok().and_then(|g| *g);
        self.dispatch_sync(dark).await
    }

    pub async fn sync_appearance(&self, dark: bool) -> Option<SafeAreaInsetsDto> {
        if let Ok(mut guard) = self.inner.last_dark.write() {
            *guard = Some(dark);
        }
        self.dispatch_sync(Some(dark)).await
    }

    #[cfg(target_os = "android")]
    async fn dispatch_sync(&self, dark: Option<bool>) -> Option<SafeAreaInsetsDto> {
        let sync_fn = self.inner.android_sync.read().ok().and_then(|g| g.clone());
        let Some(sync_fn) = sync_fn else {
            return self.cached_safe_area();
        };

        let (tx, rx) = oneshot::channel();
        sync_fn(dark, tx);
        match tokio::time::timeout(Duration::from_millis(600), rx).await {
            Ok(Ok((insets, fonts))) => {
                if let Ok(mut guard) = self.inner.cached_insets.write() {
                    *guard = Some(insets);
                }
                if !fonts.is_empty()
                    && let Ok(mut fguard) = self.inner.system_fonts.write()
                {
                    *fguard = fonts;
                }
                Some(insets)
            }
            _ => self.cached_safe_area(),
        }
    }

    #[cfg(not(target_os = "android"))]
    async fn dispatch_sync(&self, _dark: Option<bool>) -> Option<SafeAreaInsetsDto> {
        self.cached_safe_area()
    }

    #[cfg(target_os = "android")]
    pub fn attach_window(&self, window: &tauri::WebviewWindow) {
        let inner = Arc::clone(&self.inner);
        let _ = window.with_webview(move |wv| {
            let jni_handle = wv.jni_handle();
            let initial_dark = inner.last_dark.read().ok().and_then(|g| *g);
            let init_inner = Arc::clone(&inner);
            jni_handle.exec(move |env, activity, webview| {
                let (insets, fonts) =
                    sync_android_system_bars_and_insets(env, activity, webview, initial_dark, true);
                if let Ok(mut guard) = init_inner.cached_insets.write() {
                    *guard = Some(insets);
                }
                if !fonts.is_empty()
                    && let Ok(mut fguard) = init_inner.system_fonts.write()
                {
                    *fguard = fonts;
                }
            });

            let sync_inner = Arc::clone(&inner);
            let sync_fn: AndroidSyncFn = Arc::new(move |dark, reply_tx| {
                let need_fonts = sync_inner
                    .system_fonts
                    .read()
                    .map(|g| g.is_empty())
                    .unwrap_or(true);
                jni_handle.exec(move |env, activity, webview| {
                    let res = sync_android_system_bars_and_insets(
                        env, activity, webview, dark, need_fonts,
                    );
                    let _ = reply_tx.send(res);
                });
            });
            if let Ok(mut guard) = inner.android_sync.write() {
                *guard = Some(sync_fn);
            }
        });
    }

    #[cfg(not(target_os = "android"))]
    pub fn attach_window(&self, _window: &tauri::WebviewWindow) {}
}

#[cfg(target_os = "android")]
fn clear_jni_exception(env: &mut jni::JNIEnv<'_>) {
    if env.exception_check().unwrap_or(false) {
        let _ = env.exception_clear();
    }
}

#[cfg(target_os = "android")]
fn sync_android_system_bars_and_insets(
    env: &mut jni::JNIEnv<'_>,
    activity: &jni::objects::JObject<'_>,
    webview: &jni::objects::JObject<'_>,
    dark: Option<bool>,
    probe_fonts: bool,
) -> (SafeAreaInsetsDto, Vec<SystemFontEntry>) {
    match try_sync_android_system_bars_and_insets(env, activity, webview, dark, probe_fonts) {
        Ok(res) => {
            clear_jni_exception(env);
            res
        }
        Err(_) => {
            clear_jni_exception(env);
            (SafeAreaInsetsDto::default(), Vec::new())
        }
    }
}

#[cfg(target_os = "android")]
enum ShapedAndroidFont {
    File(PathBuf, SfntHeaderInspection),
    Memory(Arc<[u8]>, SfntHeaderInspection),
}

#[cfg(target_os = "android")]
impl ShapedAndroidFont {
    fn info(&self) -> SfntHeaderInspection {
        match self {
            Self::File(_, info) | Self::Memory(_, info) => *info,
        }
    }

    fn into_source(self) -> SystemFontSource {
        match self {
            Self::File(path, _) => SystemFontSource::File(path),
            Self::Memory(bytes, _) => SystemFontSource::Memory(bytes),
        }
    }

    fn clone_source(&self) -> SystemFontSource {
        match self {
            Self::File(path, _) => SystemFontSource::File(path.clone()),
            Self::Memory(bytes, _) => SystemFontSource::Memory(Arc::clone(bytes)),
        }
    }

    fn same_underlying(&self, other: &Self) -> bool {
        match (self, other) {
            (Self::File(a, _), Self::File(b, _)) => a == b,
            (Self::Memory(a, _), Self::Memory(b, _)) => {
                a.len() == b.len() && a.get(..64) == b.get(..64)
            }
            _ => false,
        }
    }
}

#[cfg(target_os = "android")]
fn read_font_direct_buffer(
    env: &mut jni::JNIEnv<'_>,
    font: &jni::objects::JObject<'_>,
) -> Option<(Arc<[u8]>, SfntHeaderInspection)> {
    use jni::objects::JByteBuffer;

    let buf_obj = env
        .call_method(font, "getBuffer", "()Ljava/nio/ByteBuffer;", &[])
        .and_then(|v| v.l())
        .ok()?;
    if buf_obj.is_null() {
        return None;
    }
    let jbuf = JByteBuffer::from(buf_obj);
    let ptr = env.get_direct_buffer_address(&jbuf).ok()?;
    let cap = env.get_direct_buffer_capacity(&jbuf).ok()?;
    if ptr.is_null() || !(16..=MAX_SYSTEM_FONT_BYTES as usize).contains(&cap) {
        return None;
    }
    // SAFETY: `Font.getBuffer()` returns a read-only direct ByteBuffer backed by Android's
    // mmap'd Minikin font asset valid for the lifetime of `font` in this frame.
    let bytes = unsafe { std::slice::from_raw_parts(ptr, cap) };
    let info = inspect_font_header_bytes(&bytes[..cap.min(2048)])?;
    Some((Arc::from(bytes), info))
}

#[cfg(target_os = "android")]
fn shape_typeface_fonts(env: &mut jni::JNIEnv<'_>, typeface_field: &str) -> Vec<ShapedAndroidFont> {
    use jni::objects::{JString, JValue};

    let mut resolved: Vec<ShapedAndroidFont> = Vec::new();
    let step = (|| -> jni::errors::Result<()> {
        let typeface = env
            .get_static_field(
                "android/graphics/Typeface",
                typeface_field,
                "Landroid/graphics/Typeface;",
            )?
            .l()?;
        if typeface.is_null() {
            return Ok(());
        }
        let paint = env.new_object("android/graphics/Paint", "()V", &[])?;
        let _ = env.call_method(
            &paint,
            "setTypeface",
            "(Landroid/graphics/Typeface;)Landroid/graphics/Typeface;",
            &[(&typeface).into()],
        )?;

        let sample = env.new_string("Ag中")?;
        let glyphs = env
            .call_static_method(
                "android/graphics/text/TextRunShaper",
                "shapeTextRun",
                "(Ljava/lang/CharSequence;IIIIFFZLandroid/graphics/Paint;)Landroid/graphics/text/PositionedGlyphs;",
                &[
                    (&sample).into(),
                    JValue::Int(0),
                    JValue::Int(3),
                    JValue::Int(0),
                    JValue::Int(3),
                    JValue::Float(0.0),
                    JValue::Float(0.0),
                    JValue::Bool(0),
                    (&paint).into(),
                ],
            )?
            .l()?;
        if glyphs.is_null() {
            return Ok(());
        }

        let count = env.call_method(&glyphs, "glyphCount", "()I", &[])?.i()?;
        for idx in 0..count.clamp(0, 8) {
            let font = env
                .call_method(
                    &glyphs,
                    "getFont",
                    "(I)Landroid/graphics/fonts/Font;",
                    &[JValue::Int(idx)],
                )?
                .l()?;
            if font.is_null() {
                continue;
            }
            let ttc_index = env
                .call_method(&font, "getTtcIndex", "()I", &[])
                .and_then(|v| v.i())
                .unwrap_or(0);
            clear_jni_exception(env);
            if ttc_index > 0 {
                continue;
            }

            let file_obj = env
                .call_method(&font, "getFile", "()Ljava/io/File;", &[])
                .and_then(|v| v.l())
                .unwrap_or_default();
            clear_jni_exception(env);

            let mut candidate: Option<ShapedAndroidFont> = None;
            if !file_obj.is_null()
                && let Ok(path_obj) = env
                    .call_method(&file_obj, "getAbsolutePath", "()Ljava/lang/String;", &[])
                    .and_then(|v| v.l())
                && !path_obj.is_null()
            {
                let jstr = JString::from(path_obj);
                if let Ok(raw) = env.get_string(&jstr) {
                    let raw_path = raw.to_string_lossy().into_owned();
                    if !raw_path.is_empty() {
                        let pb = PathBuf::from(raw_path);
                        if should_skip_stock_cjk_collection(&pb, ttc_index) {
                            continue;
                        }
                        if let Some(info) = inspect_system_font_file(&pb) {
                            candidate = Some(ShapedAndroidFont::File(pb, info));
                        }
                    }
                }
            }
            clear_jni_exception(env);

            if candidate.is_none() {
                candidate = read_font_direct_buffer(env, &font)
                    .map(|(bytes, info)| ShapedAndroidFont::Memory(bytes, info));
                clear_jni_exception(env);
            }

            if let Some(found) = candidate
                && !resolved
                    .iter()
                    .any(|existing| existing.same_underlying(&found))
            {
                resolved.push(found);
                if resolved.len() >= 3 {
                    break;
                }
            }
        }
        Ok(())
    })();

    if step.is_err() {
        clear_jni_exception(env);
    }
    resolved
}

#[cfg(target_os = "android")]
fn detect_android_default_fonts(env: &mut jni::JNIEnv<'_>, sdk_int: i32) -> Vec<SystemFontEntry> {
    let mut entries: Vec<SystemFontEntry> = Vec::new();

    if sdk_int >= 31 {
        let regular_kept = shape_typeface_fonts(env, "DEFAULT");
        let bold_kept = shape_typeface_fonts(env, "DEFAULT_BOLD");

        for (family_index, reg_font) in regular_kept.into_iter().enumerate() {
            let reg_info = reg_font.info();
            let distinct_bold = if !reg_info.variable {
                bold_kept
                    .get(family_index)
                    .filter(|bf| !bf.same_underlying(&reg_font))
            } else {
                None
            };
            let has_distinct_bold = distinct_bold.is_some();

            let slot = entries.len();
            entries.push(SystemFontEntry {
                source: reg_font.into_source(),
                mime_type: reg_info.mime_type,
                dto: SystemFontFaceDto {
                    slot,
                    family_index,
                    css_format: reg_info.css_format.to_string(),
                    variable: reg_info.variable,
                    weight: has_distinct_bold.then_some(400),
                },
            });

            if let Some(bold_font) = distinct_bold {
                let bold_info = bold_font.info();
                let bold_slot = entries.len();
                entries.push(SystemFontEntry {
                    source: bold_font.clone_source(),
                    mime_type: bold_info.mime_type,
                    dto: SystemFontFaceDto {
                        slot: bold_slot,
                        family_index,
                        css_format: bold_info.css_format.to_string(),
                        variable: bold_info.variable,
                        weight: Some(700),
                    },
                });
            }
        }
    }

    if entries.is_empty() {
        for (family_index, candidate) in [
            "/data/system/theme/fonts/Roboto-Regular.ttf",
            "/data/system/theme/fonts/DroidSansFallback.ttf",
        ]
        .into_iter()
        .enumerate()
        {
            let path = PathBuf::from(candidate);
            if let Some(info) = inspect_system_font_file(&path) {
                let slot = entries.len();
                entries.push(SystemFontEntry {
                    source: SystemFontSource::File(path),
                    mime_type: info.mime_type,
                    dto: SystemFontFaceDto {
                        slot,
                        family_index,
                        css_format: info.css_format.to_string(),
                        variable: info.variable,
                        weight: None,
                    },
                });
            }
        }
    }

    entries
}

#[cfg(target_os = "android")]
fn try_sync_android_system_bars_and_insets(
    env: &mut jni::JNIEnv<'_>,
    activity: &jni::objects::JObject<'_>,
    webview: &jni::objects::JObject<'_>,
    dark: Option<bool>,
    probe_fonts: bool,
) -> jni::errors::Result<(SafeAreaInsetsDto, Vec<SystemFontEntry>)> {
    use jni::objects::{JObject, JValue};

    if activity.is_null() {
        return Ok((SafeAreaInsetsDto::default(), Vec::new()));
    }

    let sdk_int = env
        .get_static_field("android/os/Build$VERSION", "SDK_INT", "I")?
        .i()?;

    let resources = env
        .call_method(
            activity,
            "getResources",
            "()Landroid/content/res/Resources;",
            &[],
        )?
        .l()?;
    let density = if !resources.is_null() {
        let metrics = env
            .call_method(
                &resources,
                "getDisplayMetrics",
                "()Landroid/util/DisplayMetrics;",
                &[],
            )?
            .l()?;
        if !metrics.is_null() {
            env.get_field(&metrics, "density", "F")?.f()?
        } else {
            1.0
        }
    } else {
        1.0
    };

    let window = env
        .call_method(activity, "getWindow", "()Landroid/view/Window;", &[])?
        .l()?;

    if !window.is_null() && (21..35).contains(&sdk_int) {
        let _ = env.call_method(&window, "setStatusBarColor", "(I)V", &[JValue::Int(0)]);
        clear_jni_exception(env);
        let _ = env.call_method(&window, "setNavigationBarColor", "(I)V", &[JValue::Int(0)]);
        clear_jni_exception(env);
    }

    let decor_view = if !window.is_null() {
        env.call_method(&window, "getDecorView", "()Landroid/view/View;", &[])?
            .l()?
    } else {
        JObject::null()
    };

    if let Some(is_dark) = dark {
        if sdk_int >= 30 && !window.is_null() {
            if let Ok(ctrl_val) = env.call_method(
                &window,
                "getInsetsController",
                "()Landroid/view/WindowInsetsController;",
                &[],
            ) && let Ok(controller) = ctrl_val.l()
                && !controller.is_null()
            {
                const APPEARANCE_LIGHT_BARS: i32 = 0x08 | 0x10;
                let appearance = if is_dark { 0 } else { APPEARANCE_LIGHT_BARS };
                let _ = env.call_method(
                    &controller,
                    "setSystemBarsAppearance",
                    "(II)V",
                    &[JValue::Int(appearance), JValue::Int(APPEARANCE_LIGHT_BARS)],
                );
            }
            clear_jni_exception(env);
        } else if sdk_int >= 23 && !decor_view.is_null() {
            if let Ok(flags) = env
                .call_method(&decor_view, "getSystemUiVisibility", "()I", &[])
                .and_then(|v| v.i())
            {
                const SYSTEM_UI_FLAG_LIGHT_STATUS_BAR: i32 = 0x0000_2000;
                let next_flags = if is_dark {
                    flags & !SYSTEM_UI_FLAG_LIGHT_STATUS_BAR
                } else {
                    flags | SYSTEM_UI_FLAG_LIGHT_STATUS_BAR
                };
                let _ = env.call_method(
                    &decor_view,
                    "setSystemUiVisibility",
                    "(I)V",
                    &[JValue::Int(next_flags)],
                );
            }
            clear_jni_exception(env);
        }
    }

    let (mut top_px, mut right_px, mut bottom_px, mut left_px) = (0i32, 0i32, 0i32, 0i32);

    if !decor_view.is_null() {
        let root_insets = env
            .call_method(
                &decor_view,
                "getRootWindowInsets",
                "()Landroid/view/WindowInsets;",
                &[],
            )
            .and_then(|v| v.l())
            .unwrap_or_else(|_| JObject::null());
        clear_jni_exception(env);

        if !root_insets.is_null() {
            if sdk_int >= 30 {
                let status = env
                    .call_static_method("android/view/WindowInsets$Type", "statusBars", "()I", &[])?
                    .i()?;
                let cutout = env
                    .call_static_method(
                        "android/view/WindowInsets$Type",
                        "displayCutout",
                        "()I",
                        &[],
                    )?
                    .i()?;
                let nav = env
                    .call_static_method(
                        "android/view/WindowInsets$Type",
                        "navigationBars",
                        "()I",
                        &[],
                    )?
                    .i()?;
                let insets_obj = env
                    .call_method(
                        &root_insets,
                        "getInsets",
                        "(I)Landroid/graphics/Insets;",
                        &[JValue::Int(status | cutout | nav)],
                    )?
                    .l()?;
                if !insets_obj.is_null() {
                    top_px = env.get_field(&insets_obj, "top", "I")?.i()?;
                    right_px = env.get_field(&insets_obj, "right", "I")?.i()?;
                    bottom_px = env.get_field(&insets_obj, "bottom", "I")?.i()?;
                    left_px = env.get_field(&insets_obj, "left", "I")?.i()?;
                }
            } else if sdk_int >= 23 {
                top_px = env
                    .call_method(&root_insets, "getSystemWindowInsetTop", "()I", &[])?
                    .i()?;
                right_px = env
                    .call_method(&root_insets, "getSystemWindowInsetRight", "()I", &[])?
                    .i()?;
                bottom_px = env
                    .call_method(&root_insets, "getSystemWindowInsetBottom", "()I", &[])?
                    .i()?;
                left_px = env
                    .call_method(&root_insets, "getSystemWindowInsetLeft", "()I", &[])?
                    .i()?;
            }
        }
    }

    if top_px <= 0 && !resources.is_null() {
        let res_name = env.new_string("status_bar_height")?;
        let res_type = env.new_string("dimen")?;
        let res_pkg = env.new_string("android")?;
        if let Ok(res_id) = env
            .call_method(
                &resources,
                "getIdentifier",
                "(Ljava/lang/String;Ljava/lang/String;Ljava/lang/String;)I",
                &[(&res_name).into(), (&res_type).into(), (&res_pkg).into()],
            )
            .and_then(|v| v.i())
            && res_id > 0
            && let Ok(px) = env
                .call_method(
                    &resources,
                    "getDimensionPixelSize",
                    "(I)I",
                    &[JValue::Int(res_id)],
                )
                .and_then(|v| v.i())
        {
            top_px = px.max(0);
        }
        clear_jni_exception(env);
    }

    let dto = physical_insets_to_css_px(top_px, right_px, bottom_px, left_px, density);
    if !webview.is_null() {
        let script = build_host_safe_area_js(dto);
        if let Ok(jscript) = env.new_string(&script) {
            let null_cb = JObject::null();
            let _ = env.call_method(
                webview,
                "evaluateJavascript",
                "(Ljava/lang/String;Landroid/webkit/ValueCallback;)V",
                &[(&jscript).into(), (&null_cb).into()],
            );
            clear_jni_exception(env);
        }
    }

    let fonts = if probe_fonts {
        detect_android_default_fonts(env, sdk_int)
    } else {
        Vec::new()
    };

    Ok((dto, fonts))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn converts_physical_insets_to_css_pixels_and_clamps_invalid_inputs() {
        let dto = physical_insets_to_css_px(84, 0, 42, -10, 2.625);
        assert_eq!(dto.top, 32.0);
        assert_eq!(dto.right, 0.0);
        assert_eq!(dto.bottom, 16.0);
        assert_eq!(dto.left, 0.0);
        assert!(dto.is_non_zero());

        let fallback = physical_insets_to_css_px(24, 0, 0, 0, 0.0);
        assert_eq!(fallback.top, 24.0);
    }

    #[test]
    fn formats_safe_area_css_variable_script() {
        let script = build_host_safe_area_js(SafeAreaInsetsDto {
            top: 28.5,
            right: 0.0,
            bottom: 15.25,
            left: 0.0,
        });
        assert!(script.contains("'--host-safe-area-top','28.50px'"));
        assert!(script.contains("'--host-safe-area-bottom','15.25px'"));
    }

    #[test]
    fn detects_sfnt_variable_table_and_skips_stock_cjk_collections() {
        let mut ttf = vec![0x00, 0x01, 0x00, 0x00, 0x00, 0x01, 0, 0, 0, 0, 0, 0];
        ttf.extend_from_slice(b"fvar");
        ttf.extend_from_slice(&[0u8; 12]);
        let info = inspect_font_header_bytes(&ttf).expect("valid ttf header");
        assert_eq!(info.mime_type, "font/ttf");
        assert_eq!(info.css_format, "truetype");
        assert!(info.variable);

        assert!(should_skip_stock_cjk_collection(
            Path::new("/system/fonts/NotoSansCJK-Regular.ttc"),
            0
        ));
        assert!(should_skip_stock_cjk_collection(
            Path::new("/system/fonts/CustomCollection.ttc"),
            2
        ));
        assert!(!should_skip_stock_cjk_collection(
            Path::new("/system/fonts/MapleMono-NF-CN-Regular.ttf"),
            0
        ));
    }
}

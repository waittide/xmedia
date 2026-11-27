use std::sync::{Arc, RwLock};
#[cfg(target_os = "android")]
use std::time::Duration;

use serde::{Deserialize, Serialize};
#[cfg(target_os = "android")]
use tokio::sync::oneshot;

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
type AndroidSyncFn = Arc<dyn Fn(Option<bool>, oneshot::Sender<SafeAreaInsetsDto>) + Send + Sync>;

#[derive(Default)]
struct MobileUiBridgeInner {
    cached_insets: RwLock<Option<SafeAreaInsetsDto>>,
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
            Ok(Ok(insets)) => {
                if let Ok(mut guard) = self.inner.cached_insets.write() {
                    *guard = Some(insets);
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
                let insets =
                    sync_android_system_bars_and_insets(env, activity, webview, initial_dark);
                if let Ok(mut guard) = init_inner.cached_insets.write() {
                    *guard = Some(insets);
                }
            });

            let sync_fn: AndroidSyncFn = Arc::new(move |dark, reply_tx| {
                jni_handle.exec(move |env, activity, webview| {
                    let insets = sync_android_system_bars_and_insets(env, activity, webview, dark);
                    let _ = reply_tx.send(insets);
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
) -> SafeAreaInsetsDto {
    match try_sync_android_system_bars_and_insets(env, activity, webview, dark) {
        Ok(insets) => {
            clear_jni_exception(env);
            insets
        }
        Err(_) => {
            clear_jni_exception(env);
            SafeAreaInsetsDto::default()
        }
    }
}

#[cfg(target_os = "android")]
fn try_sync_android_system_bars_and_insets(
    env: &mut jni::JNIEnv<'_>,
    activity: &jni::objects::JObject<'_>,
    webview: &jni::objects::JObject<'_>,
    dark: Option<bool>,
) -> jni::errors::Result<SafeAreaInsetsDto> {
    use jni::objects::{JObject, JValue};

    if activity.is_null() {
        return Ok(SafeAreaInsetsDto::default());
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

    Ok(dto)
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
}

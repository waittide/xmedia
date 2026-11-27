#!/usr/bin/env python3
"""Configures Tauri's generated Android project for XMedia:
1. Enables android:usesCleartextTraffic="true" for loopback/LAN servers.
2. Generates full-bleed Adaptive Vector Icons + fallback mipmaps for all 4 app icon variants.
3. Replaces .MainActivity's single LAUNCHER filter with 4 <activity-alias> entries.
4. Enhances MainActivity.kt with runtime TaskDescription + onStop() launcher alias switching.
"""

from __future__ import annotations

import argparse
import pathlib
import re
import shutil
import xml.etree.ElementTree as ET

ANDROID_NS = "http://schemas.android.com/apk/res/android"
TOOLS_NS = "http://schemas.android.com/tools"
ET.register_namespace("android", ANDROID_NS)
ET.register_namespace("tools", TOOLS_NS)

LEFT_WING_PATH = (
    "M 244 268 L 346 268 C 364 268, 381 277, 391 292 L 492 486 "
    "C 500 502, 500 522, 492 538 L 391 732 C 381 747, 364 756, 346 756 "
    "L 244 756 C 217 756, 202 725, 217 703 L 336 512 L 217 321 "
    "C 202 299, 217 268, 244 268 Z"
)
RIGHT_WING_PATH = (
    "M 780 268 C 807 268, 822 299, 807 321 L 688 512 L 807 703 "
    "C 822 725, 807 756, 780 756 L 678 756 C 660 756, 643 747, 633 732 "
    "L 532 538 C 524 522, 524 502, 532 486 L 633 292 "
    "C 643 277, 660 268, 678 268 Z"
)
MONOLITH_BACK_PATH = (
    "M 278 264 L 376 264 C 394 264, 411 273, 422 288 L 764 710 "
    "C 780 730, 766 760, 740 760 L 642 760 C 624 760, 607 751, 596 736 "
    "L 254 314 C 238 294, 252 264, 278 264 Z"
)
MONOLITH_FRONT_PATH = (
    "M 648 264 L 746 264 C 772 264, 786 294, 770 314 L 428 736 "
    "C 417 751, 400 760, 382 760 L 284 760 C 258 760, 244 730, 260 710 "
    "L 602 288 C 613 273, 630 264, 648 264 Z"
)

VARIANTS = [
    ("cobalt", ".LauncherCobalt", True),
    ("porcelain", ".LauncherPorcelain", False),
    ("obsidian", ".LauncherObsidian", False),
    ("monolith", ".LauncherMonolith", False),
]


def build_background_vector_xml(variant: str) -> str:
    if variant in ("cobalt", "monolith"):
        start_x, start_y, end_x, end_y = "153.6", "51.2", "870.4", "972.8"
        start_c, end_c = "#FF2260E6", "#FF0E3085"
    elif variant == "porcelain":
        start_x, start_y, end_x, end_y = "512", "0", "512", "1024"
        start_c, end_c = "#FFFFFFFF", "#FFEBF0F8"
    elif variant == "obsidian":
        start_x, start_y, end_x, end_y = "512", "0", "512", "1024"
        start_c, end_c = "#FF222836", "#FF0E1118"
    else:
        raise ValueError(f"Unknown icon variant: {variant}")

    return f"""<?xml version="1.0" encoding="utf-8"?>
<vector xmlns:android="http://schemas.android.com/apk/res/android"
    xmlns:aapt="http://schemas.android.com/aapt"
    android:width="108dp"
    android:height="108dp"
    android:viewportWidth="1024"
    android:viewportHeight="1024">
    <path android:pathData="M0,0h1024v1024h-1024z">
        <aapt:attr name="android:fillColor">
            <gradient
                android:type="linear"
                android:startX="{start_x}"
                android:startY="{start_y}"
                android:endX="{end_x}"
                android:endY="{end_y}"
                android:startColor="{start_c}"
                android:endColor="{end_c}" />
        </aapt:attr>
    </path>
</vector>
"""


def build_foreground_vector_xml(variant: str) -> str:
    if variant == "cobalt":
        paths = f"""        <path
            android:fillColor="#FFFFFFFF"
            android:pathData="{LEFT_WING_PATH}" />
        <path android:pathData="{RIGHT_WING_PATH}">
            <aapt:attr name="android:fillColor">
                <gradient
                    android:type="linear"
                    android:startX="524"
                    android:startY="268"
                    android:endX="822"
                    android:endY="756"
                    android:startColor="#E6CBE8FF"
                    android:endColor="#9E93C5FD" />
            </aapt:attr>
        </path>"""
    elif variant == "porcelain":
        paths = f"""        <path android:pathData="{LEFT_WING_PATH}">
            <aapt:attr name="android:fillColor">
                <gradient
                    android:type="linear"
                    android:startX="232"
                    android:startY="268"
                    android:endX="470"
                    android:endY="756"
                    android:startColor="#FF2563EB"
                    android:endColor="#FF143EA8" />
            </aapt:attr>
        </path>
        <path android:pathData="{RIGHT_WING_PATH}">
            <aapt:attr name="android:fillColor">
                <gradient
                    android:type="linear"
                    android:startX="524"
                    android:startY="268"
                    android:endX="822"
                    android:endY="756"
                    android:startColor="#FF38BDF8"
                    android:endColor="#FF60A5FA" />
            </aapt:attr>
        </path>"""
    elif variant == "obsidian":
        paths = f"""        <path
            android:fillColor="#FFFFFFFF"
            android:pathData="{LEFT_WING_PATH}" />
        <path android:pathData="{RIGHT_WING_PATH}">
            <aapt:attr name="android:fillColor">
                <gradient
                    android:type="linear"
                    android:startX="524"
                    android:startY="268"
                    android:endX="822"
                    android:endY="756"
                    android:startColor="#FF60A5FA"
                    android:endColor="#FF2563EB" />
            </aapt:attr>
        </path>"""
    elif variant == "monolith":
        paths = f"""        <path android:pathData="{MONOLITH_BACK_PATH}">
            <aapt:attr name="android:fillColor">
                <gradient
                    android:type="linear"
                    android:startX="238"
                    android:startY="264"
                    android:endX="780"
                    android:endY="760"
                    android:startColor="#B8E0F2FE"
                    android:endColor="#7593C5FD" />
            </aapt:attr>
        </path>
        <path
            android:fillColor="#FFFFFFFF"
            android:pathData="{MONOLITH_FRONT_PATH}" />"""
    else:
        raise ValueError(f"Unknown icon variant: {variant}")

    return f"""<?xml version="1.0" encoding="utf-8"?>
<vector xmlns:android="http://schemas.android.com/apk/res/android"
    xmlns:aapt="http://schemas.android.com/aapt"
    android:width="108dp"
    android:height="108dp"
    android:viewportWidth="1536"
    android:viewportHeight="1536">
    <group
        android:translateX="256"
        android:translateY="256">
{paths}
    </group>
</vector>
"""


def build_adaptive_icon_xml(variant: str) -> str:
    return f"""<?xml version="1.0" encoding="utf-8"?>
<adaptive-icon xmlns:android="http://schemas.android.com/apk/res/android">
    <background android:drawable="@drawable/ic_launcher_{variant}_background" />
    <foreground android:drawable="@drawable/ic_launcher_{variant}_foreground" />
</adaptive-icon>
"""


def write_icon_resources(main_dir: pathlib.Path, icons_dir: pathlib.Path) -> None:
    res_dir = main_dir / "res"
    drawable_v24 = res_dir / "drawable-v24"
    anydpi_v26 = res_dir / "mipmap-anydpi-v26"
    mipmap_mdpi = res_dir / "mipmap-mdpi"
    mipmap_xxxhdpi = res_dir / "mipmap-xxxhdpi"

    for d in (drawable_v24, anydpi_v26, mipmap_mdpi, mipmap_xxxhdpi):
        d.mkdir(parents=True, exist_ok=True)

    for variant, _, _ in VARIANTS:
        (drawable_v24 / f"ic_launcher_{variant}_background.xml").write_text(
            build_background_vector_xml(variant), encoding="utf-8"
        )
        (drawable_v24 / f"ic_launcher_{variant}_foreground.xml").write_text(
            build_foreground_vector_xml(variant), encoding="utf-8"
        )
        adaptive_xml = build_adaptive_icon_xml(variant)
        (anydpi_v26 / f"ic_launcher_{variant}.xml").write_text(adaptive_xml, encoding="utf-8")
        (anydpi_v26 / f"ic_launcher_{variant}_round.xml").write_text(
            adaptive_xml, encoding="utf-8"
        )

        png_src = icons_dir / "variants" / f"{variant}-128.png"
        if not png_src.is_file():
            raise FileNotFoundError(f"Missing variant PNG asset: {png_src}")
        for target_dir in (mipmap_mdpi, mipmap_xxxhdpi):
            shutil.copyfile(png_src, target_dir / f"ic_launcher_{variant}.png")
            shutil.copyfile(png_src, target_dir / f"ic_launcher_{variant}_round.png")


def attr_key(name: str) -> str:
    return f"{{{ANDROID_NS}}}{name}"


def is_launcher_intent_filter(elem: ET.Element) -> bool:
    if elem.tag != "intent-filter":
        return False
    categories = {
        c.attrib.get(attr_key("name"), "")
        for c in elem.findall("category")
    }
    return (
        "android.intent.category.LAUNCHER" in categories
        or "android.intent.category.LEANBACK_LAUNCHER" in categories
    )


def patch_android_manifest(manifest_path: pathlib.Path) -> None:
    tree = ET.parse(manifest_path)
    root = tree.getroot()
    app = root.find("application")
    if app is None:
        raise RuntimeError(f"Missing <application> element in {manifest_path}")

    app.set(attr_key("usesCleartextTraffic"), "true")
    app.set(attr_key("icon"), "@mipmap/ic_launcher_cobalt")
    app.set(attr_key("roundIcon"), "@mipmap/ic_launcher_cobalt_round")

    main_activity = None
    main_activity_idx = 0
    children = list(app)
    for idx, child in enumerate(children):
        if child.tag == "activity" and child.attrib.get(attr_key("name"), "").endswith(
            "MainActivity"
        ):
            main_activity = child
            main_activity_idx = idx
            break

    if main_activity is None:
        raise RuntimeError(f"Missing .MainActivity <activity> in {manifest_path}")

    main_target_name = main_activity.attrib[attr_key("name")]
    label_attr = main_activity.attrib.get(attr_key("label"), "@string/app_name")
    main_activity.set(attr_key("exported"), "true")

    for child in list(main_activity):
        if is_launcher_intent_filter(child):
            main_activity.remove(child)

    alias_names = {suffix for _, suffix, _ in VARIANTS}
    for existing in list(app):
        if (
            existing.tag == "activity-alias"
            and existing.attrib.get(attr_key("name")) in alias_names
        ):
            app.remove(existing)

    insert_pos = list(app).index(main_activity) + 1 if main_activity in list(app) else main_activity_idx + 1
    for offset, (variant, alias_suffix, default_enabled) in enumerate(VARIANTS):
        alias_el = ET.Element(
            "activity-alias",
            {
                attr_key("name"): alias_suffix,
                attr_key("targetActivity"): main_target_name,
                attr_key("enabled"): "true" if default_enabled else "false",
                attr_key("exported"): "true",
                attr_key("icon"): f"@mipmap/ic_launcher_{variant}",
                attr_key("roundIcon"): f"@mipmap/ic_launcher_{variant}_round",
                attr_key("label"): label_attr,
            },
        )
        intent_filter = ET.SubElement(alias_el, "intent-filter")
        ET.SubElement(
            intent_filter,
            "action",
            {attr_key("name"): "android.intent.action.MAIN"},
        )
        ET.SubElement(
            intent_filter,
            "category",
            {attr_key("name"): "android.intent.category.LAUNCHER"},
        )
        ET.SubElement(
            intent_filter,
            "category",
            {attr_key("name"): "android.intent.category.LEANBACK_LAUNCHER"},
        )
        app.insert(insert_pos + offset, alias_el)

    ET.indent(tree, space="    ")
    xml_out = ET.tostring(root, encoding="unicode")
    manifest_path.write_text(f'<?xml version="1.0" encoding="utf-8"?>\n{xml_out}\n', encoding="utf-8")


def build_main_activity_kotlin(pkg_name: str, has_edge_to_edge: bool) -> str:
    edge_import = "\nimport androidx.activity.enableEdgeToEdge" if has_edge_to_edge else ""
    edge_call = "\n    enableEdgeToEdge()" if has_edge_to_edge else ""
    return f"""package {pkg_name}

import android.app.ActivityManager
import android.content.ComponentName
import android.content.pm.PackageManager
import android.os.Build
import android.os.Bundle{edge_import}

class MainActivity : TauriActivity() {{
  @Volatile private var pendingIconVariant: String? = null

  override fun onCreate(savedInstanceState: Bundle?) {{{edge_call}
    super.onCreate(savedInstanceState)
  }}

  fun setLauncherIconVariant(variant: String) {{
    val normalized = variant.trim().lowercase()
    if (ALIAS_SPECS.none {{ it.id == normalized }}) {{
      return
    }}
    pendingIconVariant = normalized
    applyTaskDescriptionIcon(normalized)
    if (isFinishing || isDestroyed || !window.decorView.isShown) {{
      applyPendingLauncherAlias()
    }}
  }}

  override fun onStop() {{
    super.onStop()
    applyPendingLauncherAlias()
  }}

  private fun applyTaskDescriptionIcon(variant: String) {{
    try {{
      val resId = resources.getIdentifier("ic_launcher_$variant", "mipmap", packageName)
      if (resId == 0) return
      if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.TIRAMISU) {{
        setTaskDescription(
          ActivityManager.TaskDescription.Builder()
            .setIcon(resId)
            .build()
        )
      }} else if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.P) {{
        @Suppress("DEPRECATION")
        setTaskDescription(ActivityManager.TaskDescription(title?.toString(), resId))
      }}
    }} catch (_: Throwable) {{
      // Ignore non-fatal task description failures across OEM skins.
    }}
  }}

  private fun applyPendingLauncherAlias() {{
    val target = pendingIconVariant ?: return
    pendingIconVariant = null
    try {{
      val pm = packageManager
      val pkg = packageName
      val targetSpec = ALIAS_SPECS.firstOrNull {{ it.id == target }} ?: return
      val targetCn = ComponentName(pkg, "$pkg${{targetSpec.aliasSuffix}}")

      // Enable the newly selected launcher alias before disabling previous ones
      // so the package always exposes at least one enabled LAUNCHER component.
      if (!isAliasEffectivelyEnabled(pm, targetCn, targetSpec.defaultEnabled)) {{
        pm.setComponentEnabledSetting(
          targetCn,
          PackageManager.COMPONENT_ENABLED_STATE_ENABLED,
          PackageManager.DONT_KILL_APP,
        )
      }}

      for (spec in ALIAS_SPECS) {{
        if (spec.id == target) continue
        val otherCn = ComponentName(pkg, "$pkg${{spec.aliasSuffix}}")
        if (isAliasEffectivelyEnabled(pm, otherCn, spec.defaultEnabled)) {{
          pm.setComponentEnabledSetting(
            otherCn,
            PackageManager.COMPONENT_ENABLED_STATE_DISABLED,
            PackageManager.DONT_KILL_APP,
          )
        }}
      }}
    }} catch (_: Throwable) {{
      // Ignore non-fatal PackageManager errors.
    }}
  }}

  private fun isAliasEffectivelyEnabled(
    pm: PackageManager,
    cn: ComponentName,
    defaultEnabled: Boolean,
  ): Boolean {{
    return when (pm.getComponentEnabledSetting(cn)) {{
      PackageManager.COMPONENT_ENABLED_STATE_ENABLED -> true
      PackageManager.COMPONENT_ENABLED_STATE_DISABLED,
      PackageManager.COMPONENT_ENABLED_STATE_DISABLED_USER,
      PackageManager.COMPONENT_ENABLED_STATE_DISABLED_UNTIL_USED -> false
      else -> defaultEnabled
    }}
  }}

  private data class AliasSpec(
    val id: String,
    val aliasSuffix: String,
    val defaultEnabled: Boolean,
  )

  companion object {{
    private val ALIAS_SPECS = listOf(
      AliasSpec("cobalt", ".LauncherCobalt", true),
      AliasSpec("porcelain", ".LauncherPorcelain", false),
      AliasSpec("obsidian", ".LauncherObsidian", false),
      AliasSpec("monolith", ".LauncherMonolith", false),
    )
  }}
}}
"""


def patch_main_activity(main_dir: pathlib.Path) -> None:
    candidates = list(main_dir.glob("**/MainActivity.kt"))
    if not candidates:
        raise FileNotFoundError(f"Could not find MainActivity.kt under {main_dir}")
    kt_path = candidates[0]
    existing = kt_path.read_text(encoding="utf-8")
    m = re.search(r"^\s*package\s+([\w.]+)", existing, re.MULTILINE)
    pkg_name = m.group(1) if m else "app.xmedia.client"
    has_edge_to_edge = "enableEdgeToEdge" in existing or "setLauncherIconVariant" in existing
    kt_path.write_text(
        build_main_activity_kotlin(pkg_name, has_edge_to_edge),
        encoding="utf-8",
    )


def main() -> None:
    app_root = pathlib.Path(__file__).resolve().parent.parent
    parser = argparse.ArgumentParser(description="Configure XMedia Android launcher icons and manifest")
    parser.add_argument(
        "--android-main-dir",
        type=pathlib.Path,
        default=app_root / "gen" / "android" / "app" / "src" / "main",
    )
    parser.add_argument(
        "--icons-dir",
        type=pathlib.Path,
        default=app_root / "icons",
    )
    args = parser.parse_args()

    main_dir: pathlib.Path = args.android_main_dir
    icons_dir: pathlib.Path = args.icons_dir
    manifest_path = main_dir / "AndroidManifest.xml"
    if not manifest_path.is_file():
        raise FileNotFoundError(f"AndroidManifest.xml not found at {manifest_path}")

    write_icon_resources(main_dir, icons_dir)
    patch_android_manifest(manifest_path)
    patch_main_activity(main_dir)


if __name__ == "__main__":
    main()

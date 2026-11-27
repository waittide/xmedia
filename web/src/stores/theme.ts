import { computed, onScopeDispose, ref, watch } from 'vue'
import { defineStore } from 'pinia'
import { appHostApi } from '@/api/appHost'
import {
  appIconUrl,
  applyDocumentFavicon,
  DEFAULT_APP_ICON,
  resolveStoredAppIcon,
  type AppIconId,
} from '@/theme/appIcons'
import {
  ensureCustomFontsLoaded,
  hostSystemFonts,
  normalizeStoredUiFont,
  onCustomFontDeleted,
  resolveFontStack,
  resolveSystemFontStack,
  uploadedFonts,
} from '@/theme/customFonts'
import {
  applyMd3Theme,
  DEFAULT_SEED_COLOR,
  generateMd3Schemes,
  normalizeHex,
  normalizeThemeMode,
  resolveSeedColor,
  type Md3ColorScheme,
  type ThemeMode,
} from '@/theme/md3'
import { i18n, setAppLocale } from '@/i18n'
import { isMediaKind } from '@/sources/routes'
import { useAppClientStore } from '@/stores/appServers'
import { useDomainStore } from '@/stores/domain'
import {
  PREFS_STORAGE_KEY,
  readStoredPreferencesRaw,
  writeStoredVisualPreferences,
  type VisualPreferencesSnapshot,
} from '@/theme/preferencesStorage'
import type { LocaleCode, MediaKind, UserPreferences } from '@/types/api'

export interface ThemeCloudSyncBridge {
  onVisualChange: () => void
  onHydrateUser: (prefs: UserPreferences) => void
}

let cloudSyncBridge: ThemeCloudSyncBridge | null = null

export function registerThemeCloudSync(bridge: ThemeCloudSyncBridge | null): void {
  cloudSyncBridge = bridge
}

function loadInitialVisualPrefs(): VisualPreferencesSnapshot {
  const parsed = readStoredPreferencesRaw()
  return {
    theme_mode: normalizeThemeMode(parsed.theme_mode),
    seed_color: resolveSeedColor(parsed.seed_color),
    app_icon: resolveStoredAppIcon(parsed.app_icon),
    ui_font: normalizeStoredUiFont(parsed.ui_font),
    locale: parsed.locale ?? ((i18n.global.locale.value as LocaleCode) || 'zh-CN'),
    active_domain: isMediaKind(parsed.active_domain) ? parsed.active_domain : null,
    show_sidebar_labels: parsed.show_sidebar_labels !== false,
    show_nav_labels: parsed.show_nav_labels !== false,
  }
}

export const useThemeStore = defineStore('theme', () => {
  const appClientStore = useAppClientStore()
  const initial = loadInitialVisualPrefs()

  const themeMode = ref<ThemeMode>(initial.theme_mode)
  const seedColor = ref<string | null>(initial.seed_color ?? null)
  const appIcon = ref<AppIconId | null>(resolveStoredAppIcon(initial.app_icon))
  const uiFont = ref<string | null>(normalizeStoredUiFont(initial.ui_font))
  const locale = ref<LocaleCode>(initial.locale)
  const activeDomainPref = ref<MediaKind | null>(initial.active_domain ?? null)
  const showSidebarLabels = ref<boolean>(initial.show_sidebar_labels !== false)
  const showNavLabels = ref<boolean>(initial.show_nav_labels !== false)

  function getVisualSnapshot(): VisualPreferencesSnapshot {
    return {
      theme_mode: themeMode.value,
      seed_color: seedColor.value,
      app_icon: appIcon.value,
      ui_font: uiFont.value,
      locale: locale.value,
      active_domain: activeDomainPref.value,
      show_sidebar_labels: showSidebarLabels.value,
      show_nav_labels: showNavLabels.value,
    }
  }

  const mediaQuery = window.matchMedia('(prefers-color-scheme: dark)')
  const prefersDark = ref<boolean>(mediaQuery.matches)
  const onPrefersDarkChange = (ev: MediaQueryListEvent): void => {
    prefersDark.value = ev.matches
  }
  mediaQuery.addEventListener('change', onPrefersDarkChange)
  onScopeDispose(() => {
    mediaQuery.removeEventListener('change', onPrefersDarkChange)
  })

  const isDark = computed<boolean>(() => {
    if (themeMode.value === 'dark') return true
    if (themeMode.value === 'light') return false
    return prefersDark.value
  })

  const effectiveSeedColor = computed<string>(() => seedColor.value ?? DEFAULT_SEED_COLOR)

  const effectiveAppIcon = computed<AppIconId>(() => appIcon.value ?? DEFAULT_APP_ICON)

  const appIconSrc = computed<string>(() => appIconUrl(effectiveAppIcon.value))

  const activeSchemePair = computed(() => generateMd3Schemes(effectiveSeedColor.value))

  const activeScheme = computed<Md3ColorScheme>(() =>
    isDark.value ? activeSchemePair.value.dark : activeSchemePair.value.light,
  )

  let lastAppliedSeed: string | null | undefined = undefined
  let lastAppliedDark: boolean | null = null

  function applyThemeIfChanged(): void {
    const seed = seedColor.value
    const dark = isDark.value
    if (seed === lastAppliedSeed && dark === lastAppliedDark) return
    lastAppliedSeed = seed
    lastAppliedDark = dark
    applyMd3Theme(seed, dark, activeSchemePair.value)
  }

  function persistAndNotifyVisualChange(): void {
    writeStoredVisualPreferences(getVisualSnapshot())
    cloudSyncBridge?.onVisualChange()
  }

  function applyVisualChange(): void {
    applyThemeIfChanged()
    persistAndNotifyVisualChange()
  }

  function applyDocumentUiFont(): void {
    if (typeof document === 'undefined') return
    const normalized = normalizeStoredUiFont(uiFont.value)
    if (normalized) {
      document.documentElement.style.setProperty('--font-sans', resolveFontStack(normalized))
    } else if (hostSystemFonts.value.length > 0) {
      document.documentElement.style.setProperty('--font-sans', resolveSystemFontStack())
    } else {
      document.documentElement.style.removeProperty('--font-sans')
    }
  }

  function hydrateFromUser(prefs: UserPreferences): void {
    themeMode.value = normalizeThemeMode(prefs.theme_mode)
    seedColor.value = resolveSeedColor(prefs.seed_color)
    appIcon.value = resolveStoredAppIcon(prefs.app_icon)
    uiFont.value = normalizeStoredUiFont(prefs.ui_font)
    locale.value = prefs.locale
    activeDomainPref.value = isMediaKind(prefs.active_domain) ? prefs.active_domain : null
    showSidebarLabels.value = prefs.show_sidebar_labels !== false
    showNavLabels.value = prefs.show_nav_labels !== false
    if (activeDomainPref.value) {
      useDomainStore().hydratePreferredDomain(activeDomainPref.value)
    }

    applyThemeIfChanged()
    applyDocumentUiFont()
    setAppLocale(locale.value)
    writeStoredVisualPreferences(getVisualSnapshot())
    syncStandaloneIconIfNeeded()
    cloudSyncBridge?.onHydrateUser(prefs)
    void ensureCustomFontsLoaded(true)
  }

  function rememberActiveDomain(next: MediaKind): void {
    if (activeDomainPref.value === next) return
    activeDomainPref.value = next
    persistAndNotifyVisualChange()
  }

  function setShowSidebarLabels(next: boolean): void {
    if (showSidebarLabels.value === next) return
    showSidebarLabels.value = next
    persistAndNotifyVisualChange()
  }

  function setShowNavLabels(next: boolean): void {
    if (showNavLabels.value === next) return
    showNavLabels.value = next
    persistAndNotifyVisualChange()
  }

  function setThemeMode(mode: ThemeMode): void {
    themeMode.value = mode
    applyVisualChange()
  }

  function setSeedColor(hex: string | null): void {
    if (!hex) {
      seedColor.value = null
    } else {
      const normalized = normalizeHex(hex, '')
      seedColor.value = !normalized || normalized === DEFAULT_SEED_COLOR ? null : normalized
    }
    applyVisualChange()
  }

  function resetSeedColor(): void {
    seedColor.value = null
    applyVisualChange()
  }

  function setAppIcon(next: AppIconId | null): void {
    appIcon.value = resolveStoredAppIcon(next)
    persistAndNotifyVisualChange()
  }

  function resetAppIcon(): void {
    appIcon.value = null
    persistAndNotifyVisualChange()
  }

  function setUiFont(next: string | null): void {
    uiFont.value = normalizeStoredUiFont(next)
    applyDocumentUiFont()
    persistAndNotifyVisualChange()
  }

  function resetUiFont(): void {
    uiFont.value = null
    applyDocumentUiFont()
    persistAndNotifyVisualChange()
  }

  function setLocale(next: LocaleCode): void {
    locale.value = next
    setAppLocale(locale.value)
    persistAndNotifyVisualChange()
  }

  const disposeFontDeleted = onCustomFontDeleted((deletedId) => {
    if (uiFont.value === `font:${deletedId}`) {
      resetUiFont()
    }
  })
  onScopeDispose(disposeFontDeleted)

  if (typeof window !== 'undefined') {
    const onStorageChange = (ev: StorageEvent): void => {
      if (ev.key !== null && ev.key !== PREFS_STORAGE_KEY) return
      const next = loadInitialVisualPrefs()
      themeMode.value = next.theme_mode
      seedColor.value = next.seed_color ?? null
      appIcon.value = resolveStoredAppIcon(next.app_icon)
      uiFont.value = normalizeStoredUiFont(next.ui_font)
      showSidebarLabels.value = next.show_sidebar_labels !== false
      showNavLabels.value = next.show_nav_labels !== false
      if (next.locale && next.locale !== locale.value) {
        locale.value = next.locale
        setAppLocale(locale.value)
      }
      applyThemeIfChanged()
      applyDocumentUiFont()
    }
    window.addEventListener('storage', onStorageChange)
    onScopeDispose(() => {
      window.removeEventListener('storage', onStorageChange)
    })
  }

  watch(
    [seedColor, isDark],
    () => {
      applyThemeIfChanged()
    },
    { immediate: true },
  )

  watch(
    [uiFont, uploadedFonts, hostSystemFonts],
    () => {
      applyDocumentUiFont()
    },
    { immediate: true },
  )

  let lastSyncedStandaloneIcon: AppIconId | null = null
  function syncStandaloneIconIfNeeded(): void {
    const iconId = effectiveAppIcon.value
    if (!appClientStore.isStandaloneApp || iconId === lastSyncedStandaloneIcon) return
    lastSyncedStandaloneIcon = iconId
    void appHostApi.setAppIcon(iconId).catch(() => {
      if (lastSyncedStandaloneIcon === iconId) {
        lastSyncedStandaloneIcon = null
      }
    })
  }

  watch(
    [effectiveAppIcon, () => appClientStore.isStandaloneApp],
    ([iconId]) => {
      applyDocumentFavicon(iconId)
      syncStandaloneIconIfNeeded()
    },
    { immediate: true },
  )

  let lastSyncedStandaloneDark: boolean | null = null
  watch(
    [isDark, () => appClientStore.isStandaloneApp],
    ([dark, isStandalone]) => {
      if (isStandalone && dark !== lastSyncedStandaloneDark) {
        lastSyncedStandaloneDark = dark
        void appHostApi.syncAppearance(dark).catch(() => {
          // Non-fatal on desktop/browser hosts without mobile window insets.
        })
      }
    },
    { immediate: true },
  )

  if (typeof window !== 'undefined') {
    let insetRefreshTimer: ReturnType<typeof setTimeout> | null = null
    const scheduleHostInsetRefresh = (): void => {
      if (!appClientStore.isStandaloneApp) return
      if (insetRefreshTimer) clearTimeout(insetRefreshTimer)
      insetRefreshTimer = setTimeout(() => {
        insetRefreshTimer = null
        if (!appClientStore.isStandaloneApp) return
        void appHostApi.syncAppearance(isDark.value).catch(() => {})
      }, 120)
    }
    window.addEventListener('resize', scheduleHostInsetRefresh, { passive: true })
    window.addEventListener('orientationchange', scheduleHostInsetRefresh, { passive: true })
    onScopeDispose(() => {
      if (insetRefreshTimer) clearTimeout(insetRefreshTimer)
      window.removeEventListener('resize', scheduleHostInsetRefresh)
      window.removeEventListener('orientationchange', scheduleHostInsetRefresh)
    })
  }

  setAppLocale(locale.value)

  return {
    themeMode,
    seedColor,
    effectiveSeedColor,
    appIcon,
    effectiveAppIcon,
    appIconSrc,
    uiFont,
    locale,
    isDark,
    activeScheme,
    getVisualSnapshot,
    hydrateFromUser,
    setThemeMode,
    setSeedColor,
    resetSeedColor,
    setAppIcon,
    resetAppIcon,
    setUiFont,
    resetUiFont,
    setLocale,
    activeDomainPref,
    rememberActiveDomain,
    showSidebarLabels,
    showNavLabels,
    setShowSidebarLabels,
    setShowNavLabels,
  }
})

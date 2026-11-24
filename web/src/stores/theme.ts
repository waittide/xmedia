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
import { useAppClientStore } from '@/stores/appServers'
import {
  readStoredPreferencesRaw,
  usePreferencesStore,
  type VisualPreferencesSnapshot,
} from '@/stores/preferences'
import type { LocaleCode, UserPreferences } from '@/types/api'

function loadInitialVisualPrefs(): VisualPreferencesSnapshot {
  const parsed = readStoredPreferencesRaw()
  return {
    theme_mode: normalizeThemeMode(parsed.theme_mode),
    seed_color: resolveSeedColor(parsed.seed_color),
    app_icon: resolveStoredAppIcon(parsed.app_icon),
    locale: parsed.locale ?? ((i18n.global.locale.value as LocaleCode) || 'zh-CN'),
  }
}

export const useThemeStore = defineStore('theme', () => {
  const preferencesStore = usePreferencesStore()
  const appClientStore = useAppClientStore()
  const initial = loadInitialVisualPrefs()

  const themeMode = ref<ThemeMode>(initial.theme_mode)
  const seedColor = ref<string | null>(initial.seed_color ?? null)
  const appIcon = ref<AppIconId | null>(resolveStoredAppIcon(initial.app_icon))
  const locale = ref<LocaleCode>(initial.locale)

  // Register visual snapshot getter with preferences store so domain/layout changes
  // persist & sync cleanly without touching visual watchers or DOM CSS variables.
  preferencesStore.registerVisualProvider(() => ({
    theme_mode: themeMode.value,
    seed_color: seedColor.value,
    app_icon: appIcon.value,
    locale: locale.value,
  }))

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

  function applyVisualChange(): void {
    applyThemeIfChanged()
    preferencesStore.persistAndSync()
  }

  function hydrateFromUser(prefs: UserPreferences): void {
    themeMode.value = normalizeThemeMode(prefs.theme_mode)
    seedColor.value = resolveSeedColor(prefs.seed_color)
    appIcon.value = resolveStoredAppIcon(prefs.app_icon)
    locale.value = prefs.locale

    preferencesStore.hydrateDomainPreferences(prefs)
    applyThemeIfChanged()
    setAppLocale(locale.value)
    preferencesStore.persistLocal()
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
    preferencesStore.persistAndSync()
  }

  function resetAppIcon(): void {
    appIcon.value = null
    preferencesStore.persistAndSync()
  }

  function setLocale(next: LocaleCode): void {
    locale.value = next
    setAppLocale(locale.value)
    preferencesStore.persistAndSync()
  }

  watch(
    [seedColor, isDark],
    () => {
      applyThemeIfChanged()
    },
    { immediate: true },
  )

  let lastSyncedStandaloneIcon: AppIconId | null = null
  watch(
    [effectiveAppIcon, () => appClientStore.isStandaloneApp],
    ([iconId, isStandalone]) => {
      applyDocumentFavicon(iconId)
      if (isStandalone && iconId !== lastSyncedStandaloneIcon) {
        lastSyncedStandaloneIcon = iconId
        void appHostApi.setAppIcon(iconId).catch(() => {
          // Non-fatal: favicon and in-app brand icon are already updated.
        })
      }
    },
    { immediate: true },
  )

  setAppLocale(locale.value)

  return {
    themeMode,
    seedColor,
    effectiveSeedColor,
    appIcon,
    effectiveAppIcon,
    appIconSrc,
    locale,
    isDark,
    activeScheme,
    hydrateFromUser,
    setThemeMode,
    setSeedColor,
    resetSeedColor,
    setAppIcon,
    resetAppIcon,
    setLocale,
  }
})

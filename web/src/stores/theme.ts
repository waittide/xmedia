import { computed, ref, watch } from 'vue'
import { defineStore, storeToRefs } from 'pinia'
import {
  applyResolvedPaletteToDom,
  DARK_PALETTE_PRESETS,
  DEFAULT_ACCENT_MODE,
  LIGHT_PALETTE_PRESETS,
  resolvePalette,
  type AccentMode,
  type CatppuccinAccent,
  type CustomPaletteTokens,
  type ThemeMode,
} from '@/theme/catppuccin'
import { i18n, setAppLocale } from '@/i18n'
import {
  readStoredPreferencesRaw,
  usePreferencesStore,
  type VisualPreferencesSnapshot,
} from '@/stores/preferences'
import type { LocaleCode, UserPreferences } from '@/types/api'

const DEFAULT_CUSTOM_LIGHT: CustomPaletteTokens = { ...LIGHT_PALETTE_PRESETS[0]!.tokens }
const DEFAULT_CUSTOM_DARK: CustomPaletteTokens = { ...DARK_PALETTE_PRESETS[0]!.tokens }

function loadInitialVisualPrefs(): VisualPreferencesSnapshot {
  const parsed = readStoredPreferencesRaw()
  return {
    theme_mode: parsed.theme_mode ?? 'system',
    light_preset: parsed.light_preset ?? 'material-you-light',
    dark_preset: parsed.dark_preset ?? 'material-you-dark',
    dark_flavor: parsed.dark_flavor ?? 'mocha',
    accent_mode:
      parsed.accent_mode === 'preset' ||
      parsed.accent_mode === 'catppuccin' ||
      parsed.accent_mode === 'custom'
        ? parsed.accent_mode
        : 'preset',
    accent: parsed.accent ?? 'blue',
    custom_accent: parsed.custom_accent ?? '#3b82f6',
    custom_light: parsed.custom_light ?? { ...DEFAULT_CUSTOM_LIGHT },
    custom_dark: parsed.custom_dark ?? { ...DEFAULT_CUSTOM_DARK },
    locale: parsed.locale ?? ((i18n.global.locale.value as LocaleCode) || 'zh-CN'),
  }
}

export const useThemeStore = defineStore('theme', () => {
  const preferencesStore = usePreferencesStore()
  const initial = loadInitialVisualPrefs()

  const themeMode = ref<ThemeMode>(initial.theme_mode)
  const lightPreset = ref<string>(initial.light_preset)
  const darkPreset = ref<string>(initial.dark_preset)
  const darkFlavor = ref(initial.dark_flavor ?? 'mocha')
  const accentMode = ref<AccentMode>(initial.accent_mode)
  const accent = ref<CatppuccinAccent>(initial.accent)
  const customAccent = ref<string>(initial.custom_accent ?? '#3b82f6')
  const customLight = ref<CustomPaletteTokens>(
    initial.custom_light ?? { ...DEFAULT_CUSTOM_LIGHT },
  )
  const customDark = ref<CustomPaletteTokens>(
    initial.custom_dark ?? { ...DEFAULT_CUSTOM_DARK },
  )
  const locale = ref<LocaleCode>(initial.locale)

  // Register visual snapshot getter with preferences store so domain/layout changes
  // persist & sync cleanly without touching visual watchers or DOM CSS variables.
  preferencesStore.registerVisualProvider(() => ({
    theme_mode: themeMode.value,
    light_preset: lightPreset.value,
    dark_preset: darkPreset.value,
    dark_flavor: darkFlavor.value,
    accent_mode: accentMode.value,
    accent: accent.value,
    custom_accent: customAccent.value,
    custom_light: { ...customLight.value },
    custom_dark: { ...customDark.value },
    locale: locale.value,
  }))

  const mediaQuery = window.matchMedia('(prefers-color-scheme: dark)')
  const prefersDark = ref<boolean>(mediaQuery.matches)
  mediaQuery.addEventListener('change', (ev) => {
    prefersDark.value = ev.matches
  })

  const isDark = computed<boolean>(() => {
    if (themeMode.value === 'dark') return true
    if (themeMode.value === 'light') return false
    return prefersDark.value
  })

  const activePresetId = computed<string>(() =>
    isDark.value ? darkPreset.value : lightPreset.value,
  )

  const activePalette = computed(() =>
    resolvePalette({
      isDark: isDark.value,
      lightPresetId: lightPreset.value,
      darkPresetId: darkPreset.value,
      customLight: customLight.value,
      customDark: customDark.value,
      accentMode: accentMode.value,
      catppuccinAccent: accent.value,
      customAccent: customAccent.value,
    }),
  )

  let lastAppliedPalette: ReturnType<typeof resolvePalette> | null = null
  let lastAppliedDark: boolean | null = null
  let lastAppliedPreset: string | null = null

  function applyPaletteIfChanged(): void {
    const pal = activePalette.value
    const dark = isDark.value
    const preset = activePresetId.value
    if (
      pal === lastAppliedPalette &&
      dark === lastAppliedDark &&
      preset === lastAppliedPreset
    ) {
      return
    }
    lastAppliedPalette = pal
    lastAppliedDark = dark
    lastAppliedPreset = preset
    applyResolvedPaletteToDom(pal, dark, preset)
  }

  function applyVisualChange(): void {
    applyPaletteIfChanged()
    preferencesStore.persistAndSync()
  }

  function hydrateFromUser(prefs: UserPreferences): void {
    themeMode.value = prefs.theme_mode
    lightPreset.value = prefs.light_preset || 'material-you-light'
    darkPreset.value = prefs.dark_preset || 'material-you-dark'
    if (prefs.dark_flavor) darkFlavor.value = prefs.dark_flavor
    accentMode.value =
      prefs.accent_mode === 'preset' ||
      prefs.accent_mode === 'catppuccin' ||
      prefs.accent_mode === 'custom'
        ? prefs.accent_mode
        : DEFAULT_ACCENT_MODE
    accent.value = prefs.accent
    if (prefs.custom_accent) customAccent.value = prefs.custom_accent
    if (prefs.custom_light) customLight.value = { ...prefs.custom_light }
    if (prefs.custom_dark) customDark.value = { ...prefs.custom_dark }
    locale.value = prefs.locale

    preferencesStore.hydrateDomainPreferences(prefs)
    applyPaletteIfChanged()
    setAppLocale(locale.value)
    preferencesStore.persistLocal()
  }

  function setThemeMode(mode: ThemeMode): void {
    themeMode.value = mode
    applyVisualChange()
  }

  function setLightPreset(presetId: string): void {
    lightPreset.value = presetId
    applyVisualChange()
  }

  function setDarkPreset(presetId: string): void {
    darkPreset.value = presetId
    applyVisualChange()
  }

  function setCatppuccinAccent(next: CatppuccinAccent): void {
    accentMode.value = 'catppuccin'
    accent.value = next
    applyVisualChange()
  }

  function setAccentMode(mode: AccentMode): void {
    accentMode.value = mode
    applyVisualChange()
  }

  function setCustomAccentHex(hex: string): void {
    accentMode.value = 'custom'
    customAccent.value = hex
    applyVisualChange()
  }

  function updateCustomPalette(
    kind: 'light' | 'dark',
    patch: Partial<CustomPaletteTokens>,
  ): void {
    if (kind === 'light') {
      customLight.value = { ...customLight.value, ...patch }
      lightPreset.value = 'custom'
    } else {
      customDark.value = { ...customDark.value, ...patch }
      darkPreset.value = 'custom'
    }
    applyVisualChange()
  }

  function seedCustomFromActivePreset(kind: 'light' | 'dark'): void {
    if (kind === 'light') {
      const src =
        LIGHT_PALETTE_PRESETS.find((p) => p.id === lightPreset.value) ??
        LIGHT_PALETTE_PRESETS[0]!
      customLight.value = { ...src.tokens }
      lightPreset.value = 'custom'
    } else {
      const src =
        DARK_PALETTE_PRESETS.find((p) => p.id === darkPreset.value) ??
        DARK_PALETTE_PRESETS[0]!
      customDark.value = { ...src.tokens }
      darkPreset.value = 'custom'
    }
    applyVisualChange()
  }

  function resetPalettesToDefault(): void {
    lightPreset.value = 'material-you-light'
    darkPreset.value = 'material-you-dark'
    darkFlavor.value = 'mocha'
    accentMode.value = DEFAULT_ACCENT_MODE
    accent.value = 'blue'
    customAccent.value = '#3b82f6'
    customLight.value = { ...DEFAULT_CUSTOM_LIGHT }
    customDark.value = { ...DEFAULT_CUSTOM_DARK }
    applyVisualChange()
  }

  function setLocale(next: LocaleCode): void {
    locale.value = next
    setAppLocale(locale.value)
    preferencesStore.persistAndSync()
  }

  watch(
    [activePalette, isDark, activePresetId],
    () => {
      applyPaletteIfChanged()
    },
    { immediate: true },
  )

  setAppLocale(locale.value)

  // Lightweight compatibility pass-through proxies to usePreferencesStore()
  const { reader, comic, playback, video, workspaceLayout } = storeToRefs(preferencesStore)

  return {
    themeMode,
    lightPreset,
    darkPreset,
    accentMode,
    accent,
    customAccent,
    customLight,
    customDark,
    locale,
    isDark,
    activePresetId,
    activePalette,
    hydrateFromUser,
    setThemeMode,
    setLightPreset,
    setDarkPreset,
    setCatppuccinAccent,
    setAccentMode,
    setCustomAccentHex,
    updateCustomPalette,
    seedCustomFromActivePreset,
    resetPalettesToDefault,
    setLocale,
    // Compatibility pass-throughs for existing consumers
    reader,
    comic,
    playback,
    video,
    workspaceLayout,
    updateReaderPreferences: preferencesStore.updateReaderPreferences,
    updateComicPreferences: preferencesStore.updateComicPreferences,
    updatePlaybackPreferences: preferencesStore.updatePlaybackPreferences,
    updateVideoPreferences: preferencesStore.updateVideoPreferences,
    updateDomainLayout: preferencesStore.updateDomainLayout,
    resetDomainLayout: preferencesStore.resetDomainLayout,
  }
})

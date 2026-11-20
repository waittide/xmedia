import { computed, ref, watch } from 'vue'
import { defineStore } from 'pinia'
import {
  applyResolvedPaletteToDom,
  DARK_PALETTE_PRESETS,
  LIGHT_PALETTE_PRESETS,
  resolvePalette,
  type AccentMode,
  type CatppuccinAccent,
  type CustomPaletteTokens,
  type ThemeMode,
} from '@/theme/catppuccin'
import { i18n, setAppLocale } from '@/i18n'
import type {
  ComicReaderPreferences,
  DomainLayoutConfig,
  LocaleCode,
  MediaKind,
  PlaybackPreferences,
  ReaderPreferences,
  UserPreferences,
  VideoPlaybackPreferences,
  WorkspaceLayoutPreferences,
} from '@/types/api'
import { DEFAULT_WORKSPACE_LAYOUT, sanitizeDomainLayout, sanitizeWorkspaceLayout } from '@/config/workspaceLayout'
import { api, getStoredToken } from '@/api/client'

const PREFS_STORAGE_KEY = 'xmedia.preferences'

const DEFAULT_CUSTOM_LIGHT: CustomPaletteTokens = { ...LIGHT_PALETTE_PRESETS[0]!.tokens }
const DEFAULT_CUSTOM_DARK: CustomPaletteTokens = { ...DARK_PALETTE_PRESETS[0]!.tokens }

const DEFAULT_BOOK_PREFS: ReaderPreferences = {
  font_family: 'system-ui',
  font_size: 17,
  line_height: 1.7,
  margin_px: 36,
  flow: 'paginated',
  pdf_scale: 1.25,
}

const DEFAULT_COMIC_PREFS: ComicReaderPreferences = {
  default_reader_mode: 'rtl_double',
  default_fit_mode: 'fit_height',
  offset_cover_page: true,
  dim_images_in_dark: false,
  preload_pages: 3,
  webtoon_width: 920,
}

const DEFAULT_MUSIC_PREFS: PlaybackPreferences = {
  transcode_profile: 'raw',
  volume: 0.85,
  muted: false,
  play_mode: 'sequence',
}

const DEFAULT_VIDEO_PREFS: VideoPlaybackPreferences = {
  default_quality: 'auto',
  preferred_audio_lang: null,
  preferred_subtitle_lang: null,
  autoplay_next: true,
  playback_rate: 1.0,
}

function loadLocalPrefs(): UserPreferences {
  const fallback: UserPreferences = {
    theme_mode: 'system',
    light_preset: 'material-you-light',
    dark_preset: 'material-you-dark',
    dark_flavor: 'mocha',
    accent_mode: 'preset',
    accent: 'blue',
    custom_accent: '#3b82f6',
    custom_light: { ...DEFAULT_CUSTOM_LIGHT },
    custom_dark: { ...DEFAULT_CUSTOM_DARK },
    locale: (i18n.global.locale.value as LocaleCode) || 'zh-CN',
    book: { ...DEFAULT_BOOK_PREFS },
    comic: { ...DEFAULT_COMIC_PREFS },
    music: { ...DEFAULT_MUSIC_PREFS },
    video: { ...DEFAULT_VIDEO_PREFS },
    workspace_layout: sanitizeWorkspaceLayout(DEFAULT_WORKSPACE_LAYOUT),
  }
  try {
    const raw = localStorage.getItem(PREFS_STORAGE_KEY)
    if (!raw) return fallback
    const parsed = JSON.parse(raw) as Partial<UserPreferences>
    return {
      theme_mode: parsed.theme_mode ?? fallback.theme_mode,
      light_preset: parsed.light_preset ?? fallback.light_preset,
      dark_preset: parsed.dark_preset ?? fallback.dark_preset,
      dark_flavor: parsed.dark_flavor ?? fallback.dark_flavor,
      accent_mode:
        parsed.accent_mode === 'preset' ||
        parsed.accent_mode === 'catppuccin' ||
        parsed.accent_mode === 'custom'
          ? parsed.accent_mode
          : fallback.accent_mode,
      accent: parsed.accent ?? fallback.accent,
      custom_accent: parsed.custom_accent ?? fallback.custom_accent,
      custom_light: parsed.custom_light ?? fallback.custom_light,
      custom_dark: parsed.custom_dark ?? fallback.custom_dark,
      locale: parsed.locale ?? fallback.locale,
      book: { ...DEFAULT_BOOK_PREFS, ...(parsed.book ?? {}) },
      comic: { ...DEFAULT_COMIC_PREFS, ...(parsed.comic ?? {}) },
      music: { ...DEFAULT_MUSIC_PREFS, ...(parsed.music ?? {}) },
      video: { ...DEFAULT_VIDEO_PREFS, ...(parsed.video ?? {}) },
      workspace_layout: sanitizeWorkspaceLayout(parsed.workspace_layout),
    }
  } catch {
    return fallback
  }
}

export const useThemeStore = defineStore('theme', () => {
  const initial = loadLocalPrefs()
  const themeMode = ref<ThemeMode>(initial.theme_mode)
  const lightPreset = ref<string>(initial.light_preset)
  const darkPreset = ref<string>(initial.dark_preset)
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

  // Expose `reader` for xbook components and `playback` for xmusic components!
  const reader = ref<ReaderPreferences>(initial.book)
  const comic = ref<ComicReaderPreferences>(initial.comic)
  const playback = ref<PlaybackPreferences>(initial.music)
  const video = ref<VideoPlaybackPreferences>(initial.video)
  const workspaceLayout = ref<WorkspaceLayoutPreferences>(
    sanitizeWorkspaceLayout(initial.workspace_layout),
  )

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

  let syncTimer: ReturnType<typeof setTimeout> | null = null
  let lastAppliedPalette: ReturnType<typeof resolvePalette> | null = null
  let lastAppliedDark: boolean | null = null
  let lastAppliedPreset: string | null = null

  function toUserPreferences(): UserPreferences {
    return {
      theme_mode: themeMode.value,
      light_preset: lightPreset.value,
      dark_preset: darkPreset.value,
      dark_flavor: 'mocha',
      accent_mode: accentMode.value,
      accent: accent.value,
      custom_accent: customAccent.value,
      custom_light: { ...customLight.value },
      custom_dark: { ...customDark.value },
      locale: locale.value,
      book: { ...reader.value },
      comic: { ...comic.value },
      music: { ...playback.value },
      video: { ...video.value },
      workspace_layout: sanitizeWorkspaceLayout(workspaceLayout.value),
    }
  }

  function persistLocalPrefs(): void {
    localStorage.setItem(PREFS_STORAGE_KEY, JSON.stringify(toUserPreferences()))
  }

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
    persistLocalPrefs()
  }

  function scheduleServerSync(): void {
    if (!getStoredToken()) return
    if (syncTimer) clearTimeout(syncTimer)
    syncTimer = setTimeout(() => {
      void api.updatePreferences(toUserPreferences()).catch(() => {})
    }, 450)
  }

  function hydrateFromUser(prefs: UserPreferences): void {
    themeMode.value = prefs.theme_mode
    lightPreset.value = prefs.light_preset || 'material-you-light'
    darkPreset.value = prefs.dark_preset || 'material-you-dark'
    accentMode.value =
      prefs.accent_mode === 'preset' ||
      prefs.accent_mode === 'catppuccin' ||
      prefs.accent_mode === 'custom'
        ? prefs.accent_mode
        : 'catppuccin'
    accent.value = prefs.accent
    if (prefs.custom_accent) customAccent.value = prefs.custom_accent
    if (prefs.custom_light) customLight.value = { ...prefs.custom_light }
    if (prefs.custom_dark) customDark.value = { ...prefs.custom_dark }
    locale.value = prefs.locale
    if (prefs.book) reader.value = { ...DEFAULT_BOOK_PREFS, ...prefs.book }
    if (prefs.comic) comic.value = { ...DEFAULT_COMIC_PREFS, ...prefs.comic }
    if (prefs.music) playback.value = { ...DEFAULT_MUSIC_PREFS, ...prefs.music }
    if (prefs.video) video.value = { ...DEFAULT_VIDEO_PREFS, ...prefs.video }
    workspaceLayout.value = sanitizeWorkspaceLayout(prefs.workspace_layout)
    applyPaletteIfChanged()
    setAppLocale(locale.value)
    persistLocalPrefs()
  }

  function setThemeMode(mode: ThemeMode): void {
    themeMode.value = mode
    applyVisualChange()
    scheduleServerSync()
  }

  function setLightPreset(presetId: string): void {
    lightPreset.value = presetId
    applyVisualChange()
    scheduleServerSync()
  }

  function setDarkPreset(presetId: string): void {
    darkPreset.value = presetId
    applyVisualChange()
    scheduleServerSync()
  }

  function setCatppuccinAccent(next: CatppuccinAccent): void {
    accentMode.value = 'catppuccin'
    accent.value = next
    applyVisualChange()
    scheduleServerSync()
  }

  function setAccentMode(mode: AccentMode): void {
    accentMode.value = mode
    applyVisualChange()
    scheduleServerSync()
  }

  function setCustomAccentHex(hex: string): void {
    accentMode.value = 'custom'
    customAccent.value = hex
    applyVisualChange()
    scheduleServerSync()
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
    scheduleServerSync()
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
    scheduleServerSync()
  }

  function resetPalettesToDefault(): void {
    lightPreset.value = 'material-you-light'
    darkPreset.value = 'material-you-dark'
    accentMode.value = 'catppuccin'
    accent.value = 'blue'
    customLight.value = { ...DEFAULT_CUSTOM_LIGHT }
    customDark.value = { ...DEFAULT_CUSTOM_DARK }
    applyVisualChange()
    scheduleServerSync()
  }

  function setLocale(next: LocaleCode): void {
    locale.value = next
    setAppLocale(locale.value)
    persistLocalPrefs()
    scheduleServerSync()
  }

  function updateReaderPreferences(patch: Partial<ReaderPreferences>): void {
    reader.value = { ...reader.value, ...patch }
    persistLocalPrefs()
    scheduleServerSync()
  }

  function updateComicPreferences(patch: Partial<ComicReaderPreferences>): void {
    comic.value = { ...comic.value, ...patch }
    persistLocalPrefs()
    scheduleServerSync()
  }

  function updatePlaybackPreferences(patch: Partial<PlaybackPreferences>): void {
    playback.value = { ...playback.value, ...patch }
    persistLocalPrefs()
    scheduleServerSync()
  }

  function updateVideoPreferences(patch: Partial<VideoPlaybackPreferences>): void {
    video.value = { ...video.value, ...patch }
    persistLocalPrefs()
    scheduleServerSync()
  }

  function updateDomainLayout(domain: MediaKind, patch: Partial<DomainLayoutConfig>): void {
    const current = workspaceLayout.value[domain]
    const next = sanitizeDomainLayout(domain, {
      home_cards: patch.home_cards ?? current.home_cards,
      sidebar_groups:
        patch.sidebar_groups !== undefined ? patch.sidebar_groups : current.sidebar_groups,
    })
    workspaceLayout.value = {
      ...workspaceLayout.value,
      [domain]: next,
    }
    persistLocalPrefs()
    scheduleServerSync()
  }

  function resetDomainLayout(domain: MediaKind): void {
    workspaceLayout.value = {
      ...workspaceLayout.value,
      [domain]: sanitizeDomainLayout(domain, DEFAULT_WORKSPACE_LAYOUT[domain]),
    }
    persistLocalPrefs()
    scheduleServerSync()
  }

  watch(
    [activePalette, isDark, activePresetId],
    () => {
      applyPaletteIfChanged()
    },
    { immediate: true },
  )

  setAppLocale(locale.value)

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
    reader,
    comic,
    playback,
    video,
    workspaceLayout,
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
    updateReaderPreferences,
    updateComicPreferences,
    updatePlaybackPreferences,
    updateVideoPreferences,
    updateDomainLayout,
    resetDomainLayout,
  }
})

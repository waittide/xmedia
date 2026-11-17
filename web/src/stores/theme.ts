import { computed, reactive, ref, watch } from 'vue'
import { defineStore } from 'pinia'
import { i18n } from '@/i18n'
import {
  DARK_PALETTE_PRESETS,
  LIGHT_PALETTE_PRESETS,
  applyResolvedPaletteToDom,
  resolvePalette,
  type AccentMode,
  type CatppuccinAccent,
  type CatppuccinFlavor,
  type CustomPaletteTokens,
  type ThemeMode,
} from '@/theme/catppuccin'
import { useAuthStore } from '@/stores/auth'
import type {
  BookReaderPreferences,
  ComicReaderPreferences,
  Locale,
  MusicPlaybackPreferences,
  UserPreferences,
  VideoPlaybackPreferences,
} from '@/types/api'

export const useThemeStore = defineStore('theme', () => {
  const auth = useAuthStore()

  const themeMode = ref<ThemeMode>(
    (localStorage.getItem('xmedia.theme_mode') as ThemeMode | null) || 'system',
  )
  const lightPreset = ref(localStorage.getItem('xmedia.light_preset') || 'catppuccin-latte')
  const darkPreset = ref(localStorage.getItem('xmedia.dark_preset') || 'catppuccin-mocha')
  const darkFlavor = ref<CatppuccinFlavor>('mocha')
  const accentMode = ref<AccentMode>(
    (localStorage.getItem('xmedia.accent_mode') as AccentMode | null) || 'catppuccin',
  )
  const accent = ref<CatppuccinAccent>(
    (localStorage.getItem('xmedia.accent') as CatppuccinAccent | null) || 'blue',
  )
  const customAccent = ref(localStorage.getItem('xmedia.custom_accent') || '#89b4fa')
  const locale = ref<Locale>(
    (localStorage.getItem('xmedia.locale') as Locale | null) || 'zh-CN',
  )

  const customLight = reactive<CustomPaletteTokens>({
    canvas: '#eff1f5',
    shell: '#e6e9ef',
    surface: '#ccd0da',
    text: '#4c4f69',
    subtext: '#6c6f85',
  })

  const customDark = reactive<CustomPaletteTokens>({
    canvas: '#1e1e2e',
    shell: '#181825',
    surface: '#313244',
    text: '#cdd6f4',
    subtext: '#a6adc8',
  })

  const book = reactive<BookReaderPreferences>({
    font_family: 'system-ui',
    font_size: 17,
    line_height: 1.75,
    margin_px: 36,
    flow: 'paginated',
    pdf_scale: 1.3,
  })

  const comic = reactive<ComicReaderPreferences>({
    default_reader_mode: 'rtl_double',
    default_fit_mode: 'fit_height',
    offset_cover_page: true,
    dim_images_in_dark: false,
    preload_pages: 3,
    webtoon_width: 900,
  })

  const music = reactive<MusicPlaybackPreferences>({
    transcode_profile: 'raw',
    volume: 0.85,
    muted: false,
    play_mode: 'sequence',
  })

  const video = reactive<VideoPlaybackPreferences>({
    default_quality: 'auto',
    preferred_audio_lang: null,
    preferred_subtitle_lang: null,
    autoplay_next: true,
    playback_rate: 1.0,
  })

  const mediaQuery = window.matchMedia('(prefers-color-scheme: dark)')
  const systemDark = ref(mediaQuery.matches)
  mediaQuery.addEventListener('change', (e) => {
    systemDark.value = e.matches
  })

  const isDark = computed(() =>
    themeMode.value === 'system' ? systemDark.value : themeMode.value === 'dark',
  )

  const activePalette = computed(() =>
    resolvePalette({
      isDark: isDark.value,
      lightPresetId: lightPreset.value,
      darkPresetId: darkPreset.value,
      customLight,
      customDark,
      accentMode: accentMode.value,
      catppuccinAccent: accent.value,
      customAccent: customAccent.value,
    }),
  )

  watch(
    [activePalette, isDark, lightPreset, darkPreset],
    ([pal, dark, lp, dp]) => {
      applyResolvedPaletteToDom(pal, dark, dark ? dp : lp)
    },
    { immediate: true },
  )

  watch(
    () => auth.user?.preferences,
    (prefs) => {
      if (!prefs) return
      themeMode.value = prefs.theme_mode
      lightPreset.value = prefs.light_preset
      darkPreset.value = prefs.dark_preset
      darkFlavor.value = prefs.dark_flavor
      accentMode.value = (prefs.accent_mode as AccentMode) || 'catppuccin'
      accent.value = prefs.accent
      if (prefs.custom_accent) customAccent.value = prefs.custom_accent
      if (prefs.custom_light) Object.assign(customLight, prefs.custom_light)
      if (prefs.custom_dark) Object.assign(customDark, prefs.custom_dark)
      locale.value = prefs.locale
      i18n.global.locale.value = prefs.locale
      Object.assign(book, prefs.book)
      Object.assign(comic, prefs.comic)
      Object.assign(music, prefs.music)
      Object.assign(video, prefs.video)
    },
    { immediate: true },
  )

  let syncTimer: ReturnType<typeof setTimeout> | null = null

  function persistPreferences(): void {
    localStorage.setItem('xmedia.theme_mode', themeMode.value)
    localStorage.setItem('xmedia.light_preset', lightPreset.value)
    localStorage.setItem('xmedia.dark_preset', darkPreset.value)
    localStorage.setItem('xmedia.accent_mode', accentMode.value)
    localStorage.setItem('xmedia.accent', accent.value)
    localStorage.setItem('xmedia.custom_accent', customAccent.value)
    localStorage.setItem('xmedia.locale', locale.value)
    i18n.global.locale.value = locale.value

    if (!auth.isAuthenticated) return
    if (syncTimer) clearTimeout(syncTimer)
    syncTimer = setTimeout(() => {
      const payload: UserPreferences = {
        theme_mode: themeMode.value,
        light_preset: lightPreset.value,
        dark_preset: darkPreset.value,
        dark_flavor: darkFlavor.value,
        accent_mode: accentMode.value,
        accent: accent.value,
        custom_accent: customAccent.value,
        custom_light: { ...customLight },
        custom_dark: { ...customDark },
        locale: locale.value,
        book: { ...book },
        comic: { ...comic },
        music: { ...music },
        video: { ...video },
      }
      void auth.savePreferences(payload).catch(() => {})
    }, 350)
  }

  function cycleThemeMode(): void {
    const order: ThemeMode[] = ['system', 'light', 'dark']
    const idx = order.indexOf(themeMode.value)
    themeMode.value = order[(idx + 1) % order.length]!
    persistPreferences()
  }

  function toggleLocale(): void {
    locale.value = locale.value === 'zh-CN' ? 'en-US' : 'zh-CN'
    persistPreferences()
  }

  return {
    themeMode,
    lightPreset,
    darkPreset,
    accentMode,
    accent,
    customAccent,
    locale,
    book,
    comic,
    music,
    video,
    isDark,
    activePalette,
    lightPresets: LIGHT_PALETTE_PRESETS,
    darkPresets: DARK_PALETTE_PRESETS,
    persistPreferences,
    cycleThemeMode,
    toggleLocale,
  }
})

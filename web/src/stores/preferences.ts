import { onScopeDispose, ref } from 'vue'
import { defineStore } from 'pinia'
import { api } from '@/api/client'
import { useAuthStore } from '@/stores/auth'
import { useSnackbarStore } from '@/stores/snackbar'
import { normalizeThemeMode, resolveSeedColor } from '@/theme/md3'
import type {
  ComicReaderPreferences,
  LocaleCode,
  PlaybackPreferences,
  ReaderPreferences,
  UserPreferences,
  VideoPlaybackPreferences,
} from '@/types/api'

export const PREFS_STORAGE_KEY = 'xmedia.preferences'

export type VisualPreferencesSnapshot = Pick<
  UserPreferences,
  'theme_mode' | 'seed_color' | 'locale'
>

export const DEFAULT_BOOK_PREFS: ReaderPreferences = {
  font_family: 'system-ui',
  font_size: 17,
  line_height: 1.7,
  margin_px: 36,
  flow: 'paginated',
  pdf_scale: 1.25,
}

export const DEFAULT_COMIC_PREFS: ComicReaderPreferences = {
  default_reader_mode: 'rtl_double',
  default_fit_mode: 'fit_height',
  offset_cover_page: true,
  dim_images_in_dark: false,
  preload_pages: 3,
  webtoon_width: 920,
}

export const DEFAULT_MUSIC_PREFS: PlaybackPreferences = {
  transcode_profile: 'raw',
  volume: 0.85,
  muted: false,
  play_mode: 'sequence',
}

export const DEFAULT_VIDEO_PREFS: VideoPlaybackPreferences = {
  default_quality: 'auto',
  preferred_audio_lang: null,
  preferred_subtitle_lang: null,
  autoplay_next: true,
  playback_rate: 1.0,
}

export function readStoredPreferencesRaw(): Partial<UserPreferences> {
  try {
    const raw = localStorage.getItem(PREFS_STORAGE_KEY)
    if (!raw) return {}
    const parsed: unknown = JSON.parse(raw)
    if (!parsed || typeof parsed !== 'object' || Array.isArray(parsed)) return {}
    return parsed as Partial<UserPreferences>
  } catch {
    return {}
  }
}

function fallbackVisualSnapshot(): VisualPreferencesSnapshot {
  const parsed = readStoredPreferencesRaw()
  return {
    theme_mode: normalizeThemeMode(parsed.theme_mode),
    seed_color: resolveSeedColor(parsed.seed_color),
    locale: (parsed.locale ?? (localStorage.getItem('xmedia.locale') as LocaleCode | null)) || 'zh-CN',
  }
}

export const usePreferencesStore = defineStore('preferences', () => {
  const stored = readStoredPreferencesRaw()

  const reader = ref<ReaderPreferences>({ ...DEFAULT_BOOK_PREFS, ...(stored.book ?? {}) })
  const comic = ref<ComicReaderPreferences>({ ...DEFAULT_COMIC_PREFS, ...(stored.comic ?? {}) })
  const playback = ref<PlaybackPreferences>({ ...DEFAULT_MUSIC_PREFS, ...(stored.music ?? {}) })
  const video = ref<VideoPlaybackPreferences>({ ...DEFAULT_VIDEO_PREFS, ...(stored.video ?? {}) })

  let visualProvider: (() => VisualPreferencesSnapshot) | null = null
  let syncTimer: ReturnType<typeof setTimeout> | null = null
  let inflightSync: Promise<void> | null = null
  let sessionEpoch = 0
  let dirtyPendingSync = false
  let syncSeq = 0

  function registerVisualProvider(fn: () => VisualPreferencesSnapshot): void {
    visualProvider = fn
  }

  function toUserPreferences(): UserPreferences {
    const visual = visualProvider ? visualProvider() : fallbackVisualSnapshot()
    return {
      ...visual,
      book: { ...reader.value },
      comic: { ...comic.value },
      music: { ...playback.value },
      video: { ...video.value },
    }
  }

  function persistLocal(): void {
    try {
      localStorage.setItem(PREFS_STORAGE_KEY, JSON.stringify(toUserPreferences()))
    } catch {
      // ignore storage quota/privacy errors
    }
  }

  function clearPendingSync(): void {
    if (syncTimer) {
      clearTimeout(syncTimer)
      syncTimer = null
    }
    dirtyPendingSync = false
  }

  async function syncToServerNow(notifyOnError = false): Promise<void> {
    const authStore = useAuthStore()
    if (!authStore.isAuthenticated) return
    clearPendingSync()
    const epoch = sessionEpoch
    const seq = ++syncSeq
    const snapshot = toUserPreferences()
    const req = api
      .updatePreferences(snapshot)
      .then((updatedUser) => {
        if (epoch !== sessionEpoch || seq !== syncSeq) return
        if (authStore.user && authStore.user.id === updatedUser.id) {
          authStore.user = updatedUser
        }
      })
      .catch((err: unknown) => {
        if (epoch !== sessionEpoch) return
        if (notifyOnError) {
          const msg = err instanceof Error ? err.message : String(err)
          useSnackbarStore().show({ message: msg, tone: 'error' })
        }
      })
      .finally(() => {
        if (inflightSync === req) {
          inflightSync = null
        }
      })
    inflightSync = req
    await req
  }

  function scheduleServerSync(options?: { immediate?: boolean; notifyOnError?: boolean }): void {
    if (!useAuthStore().isAuthenticated) return
    if (options?.immediate) {
      void syncToServerNow(options.notifyOnError ?? false)
      return
    }
    if (syncTimer) clearTimeout(syncTimer)
    dirtyPendingSync = true
    const epoch = sessionEpoch
    const notify = options?.notifyOnError ?? false
    syncTimer = setTimeout(() => {
      syncTimer = null
      if (epoch !== sessionEpoch || !useAuthStore().isAuthenticated) {
        dirtyPendingSync = false
        return
      }
      void syncToServerNow(notify)
    }, 450)
  }

  async function flushPendingSync(): Promise<void> {
    if (dirtyPendingSync || syncTimer) {
      await syncToServerNow(false)
      return
    }
    if (inflightSync) {
      await inflightSync
    }
  }

  function flushSyncOnPageHide(): void {
    if (!dirtyPendingSync || !useAuthStore().isAuthenticated) return
    clearPendingSync()
    try {
      void fetch('/api/v1/auth/preferences', {
        method: 'PUT',
        credentials: 'include',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify(toUserPreferences()),
        keepalive: true,
      })
    } catch {
      // ignore unload errors
    }
  }

  if (typeof window !== 'undefined') {
    window.addEventListener('pagehide', flushSyncOnPageHide)
    onScopeDispose(() => {
      window.removeEventListener('pagehide', flushSyncOnPageHide)
    })
  }

  function persistAndSync(options?: { immediate?: boolean; notifyOnError?: boolean }): void {
    persistLocal()
    scheduleServerSync(options)
  }

  function hydrateDomainPreferences(prefs: UserPreferences): void {
    clearPendingSync()
    if (prefs.book) reader.value = { ...DEFAULT_BOOK_PREFS, ...prefs.book }
    if (prefs.comic) comic.value = { ...DEFAULT_COMIC_PREFS, ...prefs.comic }
    if (prefs.music) playback.value = { ...DEFAULT_MUSIC_PREFS, ...prefs.music }
    if (prefs.video) video.value = { ...DEFAULT_VIDEO_PREFS, ...prefs.video }
  }

  function resetState(): void {
    sessionEpoch += 1
    clearPendingSync()
    reader.value = { ...DEFAULT_BOOK_PREFS }
    comic.value = { ...DEFAULT_COMIC_PREFS }
    playback.value = { ...DEFAULT_MUSIC_PREFS }
    video.value = { ...DEFAULT_VIDEO_PREFS }
  }

  function updateReaderPreferences(
    patch: Partial<ReaderPreferences>,
    options?: { immediate?: boolean; notifyOnError?: boolean },
  ): void {
    reader.value = { ...reader.value, ...patch }
    persistAndSync(options)
  }

  function updateComicPreferences(
    patch: Partial<ComicReaderPreferences>,
    options?: { immediate?: boolean; notifyOnError?: boolean },
  ): void {
    comic.value = { ...comic.value, ...patch }
    persistAndSync(options)
  }

  function updatePlaybackPreferences(
    patch: Partial<PlaybackPreferences>,
    options?: { immediate?: boolean; notifyOnError?: boolean },
  ): void {
    playback.value = { ...playback.value, ...patch }
    persistAndSync(options)
  }

  function updateVideoPreferences(
    patch: Partial<VideoPlaybackPreferences>,
    options?: { immediate?: boolean; notifyOnError?: boolean },
  ): void {
    video.value = { ...video.value, ...patch }
    persistAndSync(options)
  }

  return {
    reader,
    comic,
    playback,
    video,
    registerVisualProvider,
    toUserPreferences,
    persistLocal,
    scheduleServerSync,
    flushPendingSync,
    persistAndSync,
    hydrateDomainPreferences,
    updateReaderPreferences,
    updateComicPreferences,
    updatePlaybackPreferences,
    updateVideoPreferences,
    resetState,
  }
})

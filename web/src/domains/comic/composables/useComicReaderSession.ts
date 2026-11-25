import { onBeforeUnmount, onMounted, ref, watch } from 'vue'
import { useComicI18n } from '@/domains/comic/i18n'
import type { ComicReadingDirection } from '@/domains/comic/onlineApi'
import type {
  EffectiveReaderPrefDto,
  ReaderFitMode,
  ReaderMode,
} from '@/domains/comic/types'
import { usePreferencesStore } from '@/stores/preferences'
import { useSnackbarStore } from '@/stores/snackbar'

export interface ComicPageChangeEvent {
  page: number
  highestInSpread: number
  reachedEnd: boolean
}

export interface ComicProgressSnapshot {
  page: number
  highestInSpread: number
  reachedEnd: boolean
}

export interface ComicSeriesReaderPrefPatch {
  reader_mode: ReaderMode
  fit_mode: ReaderFitMode
  offset_cover_page: boolean
  webtoon_width: number
}

export interface UseComicReaderSessionOptions {
  /** Debounce interval (ms) for `onCommitProgress`. Default: `260`. */
  progressDebounceMs?: number
  /** Sync global `dim_images_in_dark` when toggled in reader. Default: `false`. */
  persistGlobalDimImages?: boolean
  /** Persist series-level reader preferences when mode/fit/offset/width changes. */
  onPersistSeriesPref?: (patch: ComicSeriesReaderPrefPatch) => Promise<void> | void
  /** Debounced progress persistence callback. */
  onCommitProgress?: (snapshot: ComicProgressSnapshot, keepalive: boolean) => Promise<void> | void
}

export function useComicReaderSession(options: UseComicReaderSessionOptions = {}) {
  const { t } = useComicI18n()
  const preferences = usePreferencesStore()
  const snackbar = useSnackbarStore()
  const progressDebounceMs = options.progressDebounceMs ?? 260

  const loading = ref(true)
  const loadError = ref<string | null>(null)
  const hasContent = ref(false)

  const currentPage = ref(1)
  const initialPage = ref(1)

  const readerMode = ref<ReaderMode>(preferences.comic.default_reader_mode)
  const fitMode = ref<ReaderFitMode>(preferences.comic.default_fit_mode)
  const offsetCoverPage = ref<boolean>(preferences.comic.offset_cover_page)
  const dimImages = ref<boolean>(preferences.comic.dim_images_in_dark)
  const webtoonWidth = ref<number>(preferences.comic.webtoon_width)
  const userOverrodeMode = ref(false)

  let progressTimer: ReturnType<typeof setTimeout> | null = null
  let pendingSnapshot: ComicProgressSnapshot | null = null
  const lastPageEvent = ref<ComicPageChangeEvent | null>(null)

  watch(
    () => preferences.comic.dim_images_in_dark,
    (val) => {
      dimImages.value = val
    },
  )

  function mapDirectionToReaderMode(dir: ComicReadingDirection | null | undefined): ReaderMode {
    if (dir === 'webtoon' && preferences.comic.default_reader_mode === 'rtl_double') {
      return 'webtoon'
    }
    return preferences.comic.default_reader_mode
  }

  function syncFromGlobalPreferences(dir?: ComicReadingDirection | null): void {
    fitMode.value = preferences.comic.default_fit_mode
    offsetCoverPage.value = preferences.comic.offset_cover_page
    dimImages.value = preferences.comic.dim_images_in_dark
    webtoonWidth.value = preferences.comic.webtoon_width
    if (!userOverrodeMode.value) {
      readerMode.value = mapDirectionToReaderMode(dir)
    }
  }

  function applyDirectionIfNotOverridden(dir: ComicReadingDirection | null | undefined): void {
    if (!userOverrodeMode.value && dir) {
      readerMode.value = mapDirectionToReaderMode(dir)
    }
  }

  function applyEffectivePreferences(pref: EffectiveReaderPrefDto): void {
    readerMode.value = pref.reader_mode
    fitMode.value = pref.fit_mode
    offsetCoverPage.value = pref.offset_cover_page
    dimImages.value = pref.dim_images_in_dark
    webtoonWidth.value = pref.webtoon_width
  }

  async function runPersistSeriesPref(): Promise<void> {
    if (!options.onPersistSeriesPref) return
    try {
      await options.onPersistSeriesPref({
        reader_mode: readerMode.value,
        fit_mode: fitMode.value,
        offset_cover_page: offsetCoverPage.value,
        webtoon_width: webtoonWidth.value,
      })
    } catch (err) {
      snackbar.show({
        message: err instanceof Error ? err.message : String(err),
        tone: 'error',
      })
    }
  }

  function changeMode(mode: ReaderMode): void {
    userOverrodeMode.value = true
    readerMode.value = mode
    void runPersistSeriesPref()
  }

  function changeFit(fit: ReaderFitMode): void {
    fitMode.value = fit
    void runPersistSeriesPref()
  }

  function toggleOffsetCover(val: boolean): void {
    offsetCoverPage.value = val
    void runPersistSeriesPref()
  }

  function toggleDimImages(val: boolean): void {
    dimImages.value = val
    if (options.persistGlobalDimImages) {
      preferences.updateComicPreferences({ dim_images_in_dark: val })
    }
  }

  function setWebtoonWidth(val: number): void {
    webtoonWidth.value = val
  }

  function commitWebtoonWidth(val: number): void {
    webtoonWidth.value = val
    void runPersistSeriesPref()
  }

  function clearProgressTimer(): void {
    if (progressTimer !== null) {
      clearTimeout(progressTimer)
      progressTimer = null
    }
  }

  function flushProgress(keepalive = false): void {
    clearProgressTimer()
    if (!pendingSnapshot) return
    const snap = pendingSnapshot
    pendingSnapshot = null
    if (options.onCommitProgress) {
      void Promise.resolve(options.onCommitProgress(snap, keepalive)).catch(() => {})
    }
  }

  function scheduleProgress(snapshot: ComicProgressSnapshot): void {
    if (!options.onCommitProgress) return
    pendingSnapshot = snapshot
    clearProgressTimer()
    progressTimer = setTimeout(() => {
      progressTimer = null
      flushProgress(false)
    }, progressDebounceMs)
  }

  function handlePageChange(payload: ComicPageChangeEvent): void {
    currentPage.value = payload.page
    lastPageEvent.value = payload
    scheduleProgress(payload)
  }

  function showChapterSwitchedToast(label: string): void {
    if (!label) return
    snackbar.show(t('reader.switchedToChapter', { title: label }))
  }

  function handlePageHide(): void {
    flushProgress(true)
  }

  onMounted(() => {
    if (typeof window !== 'undefined' && options.onCommitProgress) {
      window.addEventListener('pagehide', handlePageHide)
    }
  })

  onBeforeUnmount(() => {
    if (typeof window !== 'undefined' && options.onCommitProgress) {
      window.removeEventListener('pagehide', handlePageHide)
    }
    flushProgress(false)
  })

  return {
    preferences,
    loading,
    loadError,
    hasContent,
    currentPage,
    initialPage,
    lastPageEvent,
    readerMode,
    fitMode,
    offsetCoverPage,
    dimImages,
    webtoonWidth,
    userOverrodeMode,
    mapDirectionToReaderMode,
    syncFromGlobalPreferences,
    applyDirectionIfNotOverridden,
    applyEffectivePreferences,
    changeMode,
    changeFit,
    toggleOffsetCover,
    toggleDimImages,
    setWebtoonWidth,
    commitWebtoonWidth,
    scheduleProgress,
    flushProgress,
    handlePageChange,
    showChapterSwitchedToast,
  }
}

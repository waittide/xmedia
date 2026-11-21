<script setup lang="ts">
import { computed, nextTick, onBeforeUnmount, onMounted, reactive, ref, watch } from 'vue'
import { useRoute, useRouter } from 'vue-router'
import { useI18n as useGlobalI18n } from 'vue-i18n'
import { useComicI18n as useI18n } from '@/domains/comic/i18n'
import {
  LuBookmark,
  LuChevronLeft,
  LuChevronRight,
  LuColumns2,
  LuEye,
  LuFileText,
  LuLayers,
  LuScrollText,
  LuSkipBack,
  LuSkipForward,
  LuSlidersHorizontal,
  LuZoomIn,
  LuZoomOut,
} from 'vue-icons-plus/lu'
import { api, chapterPageUrl } from '@/domains/comic/api'
import { useSidebarStore } from '@/domains/comic/store'
import { useSnackbarStore } from '@/stores/snackbar'
import { useThemeStore } from '@/stores/theme'
import AppSelect from '@/components/ui/AppSelect.vue'
import AppCheckbox from '@/components/ui/AppCheckbox.vue'
import MdSegmentedButton, { type SegmentedOption } from '@/components/ui/MdSegmentedButton.vue'
import type { ComicBookmark, ReaderContextDto, ReaderFitMode, ReaderMode } from '@/domains/comic/types'

const { t } = useI18n()
const { t: globalT } = useGlobalI18n()
const route = useRoute()
const router = useRouter()
const theme = useThemeStore()
const sidebarStore = useSidebarStore()
const snackbar = useSnackbarStore()

const ctx = ref<ReaderContextDto | null>(null)
const chapterBookmarks = ref<ComicBookmark[]>([])
const loading = ref(true)
const loadError = ref<string | null>(null)
const hudVisible = ref(true)
const settingsDrawerOpen = ref(false)
const chaptersDrawerOpen = ref(false)

const currentPage = ref(1)
const readerMode = ref<ReaderMode>('rtl_double')
const fitMode = ref<ReaderFitMode>('fit_height')
const offsetCoverPage = ref(true)
const dimImages = ref(false)
const webtoonWidth = ref(860)
const zoomPercent = ref(100)

const widePages = reactive<Record<number, boolean>>({})
const preloadedSet = new Set<string>()
const pendingNextChapterTap = ref(false)
let progressTimer: ReturnType<typeof setTimeout> | null = null
let pendingProgress: { chapterId: string; page: number; completed?: boolean } | null = null
let webtoonObserver: IntersectionObserver | null = null
const visibleWebtoonPages = new Set<number>()
let chapterLoadSeq = 0
let suppressWebtoonObserverUntil = 0
let bookmarkBusy = false

const webtoonContainerRef = ref<HTMLDivElement | null>(null)

const totalPages = computed(() => ctx.value?.chapter.page_count || 1)

// Group pages into single or double spreads respecting offsetCoverPage & wide landscape spreads
const spreads = computed<number[][]>(() => {
  const total = totalPages.value
  if (readerMode.value === 'ltr_single' || readerMode.value === 'webtoon') {
    return Array.from({ length: total }, (_, i) => [i + 1])
  }

  const groups: number[][] = []
  let p = 1
  while (p <= total) {
    if (p === 1 && offsetCoverPage.value) {
      groups.push([1])
      p += 1
      continue
    }
    if (widePages[p]) {
      groups.push([p])
      p += 1
      continue
    }
    if (p + 1 <= total && !widePages[p + 1]) {
      groups.push([p, p + 1])
      p += 2
    } else {
      groups.push([p])
      p += 1
    }
  }
  return groups
})

const currentSpreadIndex = computed(() => {
  const idx = spreads.value.findIndex((sp) => sp.includes(currentPage.value))
  return idx >= 0 ? idx : 0
})

const currentSpread = computed<number[]>(
  () => spreads.value[currentSpreadIndex.value] || [1]
)

const isRtl = computed(() => readerMode.value === 'rtl_double')

const pageImageStyle = computed(() => {
  const scale = zoomPercent.value / 100
  if (fitMode.value === 'fit_height') {
    return {
      maxHeight: `${Math.round(100 * scale)}vh`,
      width: 'auto',
      maxWidth: currentSpread.value.length > 1 ? '50vw' : '100vw',
    }
  }
  if (fitMode.value === 'fit_width') {
    return {
      width: currentSpread.value.length > 1 ? `${Math.round(50 * scale)}vw` : `${Math.round(96 * scale)}vw`,
      height: 'auto',
    }
  }
  return {
    transform: `scale(${scale})`,
    transformOrigin: 'top center',
  }
})

const eyeCareClass = computed(() =>
  dimImages.value && theme.isDark ? 'brightness-[0.85] contrast-[0.97]' : ''
)

const modeOptions = computed(() => [
  { value: 'rtl_double', label: t('reader.modeRtlDouble') },
  { value: 'ltr_single', label: t('reader.modeLtrSingle') },
  { value: 'ltr_double', label: t('reader.modeLtrDouble') },
  { value: 'webtoon', label: t('reader.modeWebtoon') },
])

const quickModeOptions = computed<SegmentedOption<ReaderMode>[]>(() => [
  { value: 'rtl_double', label: 'RTL', icon: LuColumns2, title: t('reader.modeRtlDouble') },
  { value: 'ltr_single', label: '1P', icon: LuFileText, title: t('reader.modeLtrSingle') },
  { value: 'ltr_double', label: 'LTR 2P', icon: LuColumns2, title: t('reader.modeLtrDouble') },
  { value: 'webtoon', label: 'Webtoon', icon: LuScrollText, title: t('reader.modeWebtoon') },
])

const fitOptions = computed(() => [
  { value: 'fit_height', label: t('reader.fitHeight') },
  { value: 'fit_width', label: t('reader.fitWidth') },
  { value: 'original', label: t('reader.fitOriginal') },
])

async function loadChapter(chapterId: string, queryPage?: number, seamlessToast = false): Promise<void> {
  const seq = ++chapterLoadSeq
  flushPendingProgress()
  teardownWebtoonObserver()
  loading.value = true
  loadError.value = null
  pendingNextChapterTap.value = false
  preloadedSet.clear()
  Object.keys(widePages).forEach((k) => delete widePages[Number(k)])

  try {
    const [data, bms] = await Promise.all([
      api.getReaderContext(chapterId),
      api.listChapterBookmarks(chapterId).catch(() => []),
    ])
    if (seq !== chapterLoadSeq) return
    ctx.value = data
    chapterBookmarks.value = bms

    readerMode.value = data.effective_pref.reader_mode
    fitMode.value = data.effective_pref.fit_mode
    offsetCoverPage.value = data.effective_pref.offset_cover_page
    dimImages.value = data.effective_pref.dim_images_in_dark
    webtoonWidth.value = data.effective_pref.webtoon_width

    const initialPage = queryPage || data.chapter.current_page || 1
    currentPage.value = Math.min(Math.max(1, initialPage), Math.max(1, data.chapter.page_count))

    preloadAround(currentPage.value)
    scheduleSaveProgress()

    if (seamlessToast) {
      snackbar.show(
        t('reader.switchedToChapter', { title: `#${data.chapter.number_label} ${data.chapter.title}` }),
      )
    }

    // Mount stage first (`loading = false`) so `webtoonContainerRef` exists before scrolling.
    suppressWebtoonObserverUntil = Date.now() + 600
    loading.value = false
    if (readerMode.value === 'webtoon') {
      await nextTick()
      if (seq !== chapterLoadSeq) return
      scrollToWebtoonPage(currentPage.value)
      setupWebtoonObserver()
    }
  } catch (err) {
    if (seq !== chapterLoadSeq) return
    loadError.value = err instanceof Error ? err.message : String(err)
    loading.value = false
  }
}

function handleComicPageHide(): void {
  flushPendingProgress(true)
}

onMounted(() => {
  const qPage = route.query.page ? Number(route.query.page) : undefined
  void loadChapter(String(route.params.chapterId), qPage, false)
  window.addEventListener('keydown', onKeydown)
  window.addEventListener('pagehide', handleComicPageHide)
})

onBeforeUnmount(() => {
  chapterLoadSeq += 1
  window.removeEventListener('keydown', onKeydown)
  window.removeEventListener('pagehide', handleComicPageHide)
  flushPendingProgress()
  teardownWebtoonObserver()
  preloadedSet.clear()
})

watch(
  () => route.params.chapterId,
  (newId, oldId) => {
    if (newId && newId !== oldId) {
      const qPage = route.query.page ? Number(route.query.page) : 1
      void loadChapter(String(newId), qPage, true)
    }
  }
)

function onImageLoaded(pageNum: number, ev: Event): void {
  const img = ev.target as HTMLImageElement
  if (img.naturalWidth > 0 && img.naturalHeight > 0) {
    widePages[pageNum] = img.naturalWidth > img.naturalHeight * 1.12
  }
}

function preloadAround(centerPage: number): void {
  if (!ctx.value) return
  const ahead = Math.max(3, ctx.value.effective_pref.preload_pages || 3)
  const chapterId = ctx.value.chapter.id
  const maxPage = ctx.value.chapter.page_count

  const candidates: number[] = []
  for (let offset = 0; offset <= ahead + 1; offset++) {
    if (centerPage + offset <= maxPage) candidates.push(centerPage + offset)
  }
  if (centerPage - 1 >= 1) candidates.push(centerPage - 1)

  for (const p of candidates) {
    const key = `${chapterId}:${p}`
    if (preloadedSet.has(key)) continue
    preloadedSet.add(key)
    const img = new Image()
    img.onload = () => {
      if (img.naturalWidth > 0 && img.naturalHeight > 0 && ctx.value?.chapter.id === chapterId) {
        widePages[p] = img.naturalWidth > img.naturalHeight * 1.12
      }
    }
    img.src = chapterPageUrl(chapterId, p)
  }
}

function flushPendingProgress(useKeepalive = false): void {
  if (progressTimer) {
    clearTimeout(progressTimer)
    progressTimer = null
  }
  if (!pendingProgress) return
  const payload = pendingProgress
  pendingProgress = null
  if (useKeepalive) {
    try {
      void fetch(`/api/v1/comic/chapters/${encodeURIComponent(payload.chapterId)}/progress`, {
        method: 'PUT',
        credentials: 'include',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({
          current_page: payload.page,
          completed: payload.completed,
        }),
        keepalive: true,
      })
    } catch {
      // ignore unload errors
    }
    return
  }
  void api.updateProgress(payload.chapterId, payload.page, payload.completed).catch(() => {})
}

function scheduleSaveProgress(forceCompleted?: boolean): void {
  if (!ctx.value) return
  const chapterId = ctx.value.chapter.id
  const highestInSpread = Math.max(...currentSpread.value, currentPage.value)
  const reachedEnd = highestInSpread >= totalPages.value
  const completed = forceCompleted ?? (reachedEnd ? true : undefined)

  pendingProgress = { chapterId, page: highestInSpread, completed }
  if (progressTimer) clearTimeout(progressTimer)
  progressTimer = setTimeout(() => {
    flushPendingProgress()
  }, 260)
}

watch(currentPage, (p) => {
  pendingNextChapterTap.value = false
  preloadAround(p)
  scheduleSaveProgress()
})

async function persistSeriesReaderPref(): Promise<void> {
  if (!ctx.value) return
  await api.saveSeriesReaderPref(ctx.value.series_id, {
    reader_mode: readerMode.value,
    fit_mode: fitMode.value,
    offset_cover_page: offsetCoverPage.value,
    webtoon_width: webtoonWidth.value,
  })
}

function changeMode(mode: ReaderMode): void {
  readerMode.value = mode
  void persistSeriesReaderPref()
}

function changeFit(fit: ReaderFitMode): void {
  fitMode.value = fit
  void persistSeriesReaderPref()
}

function toggleOffsetCover(val: boolean): void {
  offsetCoverPage.value = val
  void persistSeriesReaderPref()
}

function toggleDimImages(val: boolean): void {
  dimImages.value = val
  theme.updateComicPreferences({ dim_images_in_dark: val })
}

async function advanceForward(): Promise<void> {
  if (!ctx.value) return
  if (readerMode.value === 'webtoon') {
    webtoonContainerRef.value?.scrollBy({ top: window.innerHeight * 0.82, behavior: 'smooth' })
    return
  }

  const nextIdx = currentSpreadIndex.value + 1
  if (nextIdx < spreads.value.length) {
    currentPage.value = spreads.value[nextIdx]![0]!
    return
  }

  // Reached last spread — Next Chapter Seamless Transition!
  scheduleSaveProgress(true)
  if (ctx.value.next_chapter) {
    if (!pendingNextChapterTap.value) {
      pendingNextChapterTap.value = true
      snackbar.show(
        t('reader.endOfChapterToast', {
          next: `#${ctx.value.next_chapter.number_label} ${ctx.value.next_chapter.title}`,
        }),
      )
    } else {
      goToChapter(ctx.value.next_chapter.id)
    }
  } else {
    snackbar.show(t('reader.reachedSeriesEnd'))
  }
}

function stepBackward(): void {
  pendingNextChapterTap.value = false
  if (readerMode.value === 'webtoon') {
    webtoonContainerRef.value?.scrollBy({ top: -window.innerHeight * 0.82, behavior: 'smooth' })
    return
  }

  const prevIdx = currentSpreadIndex.value - 1
  if (prevIdx >= 0) {
    currentPage.value = spreads.value[prevIdx]![0]!
  } else if (ctx.value?.prev_chapter) {
    goToChapter(ctx.value.prev_chapter.id)
  }
}

function goToChapter(chapterId: string): void {
  chaptersDrawerOpen.value = false
  settingsDrawerOpen.value = false
  router.replace('/comic/read/' + chapterId + '?page=1')
}

function onStageClick(ev: MouseEvent): void {
  if (settingsDrawerOpen.value || chaptersDrawerOpen.value) {
    settingsDrawerOpen.value = false
    chaptersDrawerOpen.value = false
    return
  }
  const width = window.innerWidth
  const x = ev.clientX
  if (x < width * 0.32) {
    if (isRtl.value) {
      void advanceForward()
    } else {
      stepBackward()
    }
  } else if (x > width * 0.68) {
    if (isRtl.value) {
      stepBackward()
    } else {
      void advanceForward()
    }
  } else {
    hudVisible.value = !hudVisible.value
  }
}

function onKeydown(e: KeyboardEvent): void {
  const target = e.target as HTMLElement | null
  if (
    target &&
    (target.tagName === 'INPUT' ||
      target.tagName === 'TEXTAREA' ||
      target.tagName === 'SELECT' ||
      target.isContentEditable)
  ) {
    return
  }

  if (e.key === 'Escape') {
    if (settingsDrawerOpen.value || chaptersDrawerOpen.value) {
      settingsDrawerOpen.value = false
      chaptersDrawerOpen.value = false
    } else if (ctx.value) {
      router.push('/comic/series/' + ctx.value.series_id)
    }
    return
  }

  if (e.metaKey || e.ctrlKey || e.altKey) return

  if (e.key === ' ') {
    e.preventDefault()
    void advanceForward()
  } else if (e.key === 'ArrowLeft') {
    e.preventDefault()
    if (isRtl.value) {
      void advanceForward()
    } else {
      stepBackward()
    }
  } else if (e.key === 'ArrowRight') {
    e.preventDefault()
    if (isRtl.value) {
      stepBackward()
    } else {
      void advanceForward()
    }
  } else if (e.key === 'ArrowDown' && readerMode.value === 'webtoon') {
    webtoonContainerRef.value?.scrollBy({ top: 320, behavior: 'smooth' })
  } else if (e.key === 'ArrowUp' && readerMode.value === 'webtoon') {
    webtoonContainerRef.value?.scrollBy({ top: -320, behavior: 'smooth' })
  }
}

function onScrubberInput(ev: Event): void {
  const targetPage = Number((ev.target as HTMLInputElement).value)
  currentPage.value = targetPage
  if (readerMode.value === 'webtoon') {
    scrollToWebtoonPage(targetPage)
  }
}

function scrollToWebtoonPage(pageNum: number): void {
  const container = webtoonContainerRef.value
  if (!container) return
  suppressWebtoonObserverUntil = Date.now() + 450
  const el = container.querySelector(`[data-page="${pageNum}"]`) as HTMLElement | null
  if (el) {
    el.scrollIntoView({ behavior: 'instant', block: 'start' })
  }
}

function teardownWebtoonObserver(): void {
  if (webtoonObserver) {
    webtoonObserver.disconnect()
    webtoonObserver = null
  }
  visibleWebtoonPages.clear()
}

function setupWebtoonObserver(): void {
  teardownWebtoonObserver()
  const container = webtoonContainerRef.value
  if (!container || readerMode.value !== 'webtoon' || loading.value || !ctx.value) return
  webtoonObserver = new IntersectionObserver(
    (entries) => {
      for (const entry of entries) {
        const p = Number((entry.target as HTMLElement).dataset.page)
        if (!p) continue
        if (entry.isIntersecting) visibleWebtoonPages.add(p)
        else visibleWebtoonPages.delete(p)
      }
      if (Date.now() < suppressWebtoonObserverUntil) return
      if (visibleWebtoonPages.size > 0) {
        const minVisible = Math.min(...visibleWebtoonPages)
        if (minVisible !== currentPage.value) {
          currentPage.value = minVisible
        }
      }
    },
    {
      root: container,
      rootMargin: '-35% 0px -55% 0px',
      threshold: 0,
    },
  )
  const nodes = container.querySelectorAll<HTMLElement>('[data-page]')
  for (const node of nodes) {
    webtoonObserver.observe(node)
  }
}

watch(
  () => [readerMode.value, ctx.value?.chapter.id, loading.value] as const,
  async ([mode, chapterId, isLoading]) => {
    teardownWebtoonObserver()
    if (mode !== 'webtoon' || !chapterId || isLoading) return
    await nextTick()
    scrollToWebtoonPage(currentPage.value)
    setupWebtoonObserver()
  },
)

const currentPageBookmark = computed<ComicBookmark | null>(
  () => chapterBookmarks.value.find((b) => b.page_number === currentPage.value) ?? null,
)

async function togglePageBookmark(): Promise<void> {
  if (!ctx.value || bookmarkBusy) return
  bookmarkBusy = true
  try {
    const page = currentPage.value
    const existing = currentPageBookmark.value
    if (existing) {
      await api.deleteBookmark(existing.id)
      chapterBookmarks.value = chapterBookmarks.value.filter((b) => b.id !== existing.id)
      snackbar.show(globalT('bookmarksPage.removedBookmark', { page }))
    } else {
      const created = await api.createBookmark(ctx.value.chapter.id, {
        page_number: page,
        title: `#${ctx.value.chapter.number_label} · ${globalT('bookmarksPage.pageLabel', { page })}`,
      })
      chapterBookmarks.value = [...chapterBookmarks.value, created]
      snackbar.show(globalT('bookmarksPage.addedBookmark', { page }))
    }
    void sidebarStore.refresh()
  } catch (err) {
    snackbar.show({
      message: err instanceof Error ? err.message : String(err),
      tone: 'error',
    })
  } finally {
    bookmarkBusy = false
  }
}
</script>

<template>
  <div
    class="relative flex h-screen w-screen flex-col overflow-hidden bg-[var(--md-sys-color-surface-dim)] text-[var(--md-sys-color-on-surface)] select-none"
  >
    <!-- Top Floating HUD Bar -->
    <header
      v-show="hudVisible"
      class="fixed top-0 right-0 left-0 z-[var(--z-base-overlay)] flex h-14 items-center justify-between gap-4 border-b border-[var(--md-sys-color-outline-variant)]/60 bg-[var(--md-sys-color-surface-container-high)]/90 px-4 text-[var(--md-sys-color-on-surface)] shadow-[var(--md-sys-elevation-2)] backdrop-blur-md"
    >
      <div class="flex min-w-0 items-center gap-3">
        <button
          type="button"
          :title="t('reader.backToSeries')"
          class="md-btn md-btn-sm md-btn-tonal md-state"
          @click="ctx ? router.push('/comic/series/' + ctx.series_id) : router.push('/comic')"
        >
          <LuChevronLeft class="h-4 w-4" />
          <span class="hidden sm:inline">{{ t('reader.backToSeries') }}</span>
        </button>

        <div v-if="ctx" class="min-w-0">
          <div class="truncate text-sm font-bold text-[var(--md-sys-color-on-surface)]">
            {{ ctx.series_title }}
          </div>
          <div class="truncate text-[11px] text-[var(--md-sys-color-on-surface-variant)]">
            #{{ ctx.chapter.number_label }} {{ ctx.chapter.title }}
          </div>
        </div>
      </div>

      <!-- Quick Mode Switcher + Controls -->
      <div class="flex items-center gap-1.5">
        <div class="hidden md:flex">
          <MdSegmentedButton
            :model-value="readerMode"
            :options="quickModeOptions"
            size="sm"
            @update:model-value="changeMode"
          />
        </div>

        <!-- Zoom Controls -->
        <button
          type="button"
          class="md-icon-btn md-icon-btn-sm md-state hidden sm:inline-flex"
          @click="zoomPercent = Math.max(50, zoomPercent - 10)"
        >
          <LuZoomOut class="h-4 w-4" />
        </button>
        <span
          class="hidden w-11 text-center font-mono text-xs text-[var(--md-sys-color-on-surface-variant)] sm:inline"
        >
          {{ zoomPercent }}%
        </span>
        <button
          type="button"
          class="md-icon-btn md-icon-btn-sm md-state hidden sm:inline-flex"
          @click="zoomPercent = Math.min(200, zoomPercent + 10)"
        >
          <LuZoomIn class="h-4 w-4" />
        </button>

        <!-- Page Bookmark Button -->
        <button
          type="button"
          :title="globalT('bookmarksPage.bookmarkPage')"
          class="md-icon-btn md-icon-btn-sm md-state"
          :class="
            currentPageBookmark
              ? '!bg-[var(--md-sys-color-primary-container)] !text-[var(--md-sys-color-on-primary-container)]'
              : ''
          "
          @click="togglePageBookmark"
        >
          <LuBookmark class="h-4 w-4" :class="currentPageBookmark ? 'fill-current' : ''" />
        </button>

        <!-- Chapter List Popover Button -->
        <button
          type="button"
          :title="t('reader.chapterList')"
          class="md-icon-btn md-icon-btn-sm md-state"
          :class="
            chaptersDrawerOpen
              ? '!bg-[var(--md-sys-color-secondary-container)] !text-[var(--md-sys-color-on-secondary-container)]'
              : ''
          "
          @click="
            chaptersDrawerOpen = !chaptersDrawerOpen;
            settingsDrawerOpen = false
          "
        >
          <LuLayers class="h-4 w-4" />
        </button>

        <!-- Reader Settings Popover Button -->
        <button
          type="button"
          class="md-icon-btn md-icon-btn-sm md-state"
          :class="
            settingsDrawerOpen
              ? '!bg-[var(--md-sys-color-secondary-container)] !text-[var(--md-sys-color-on-secondary-container)]'
              : ''
          "
          @click="
            settingsDrawerOpen = !settingsDrawerOpen;
            chaptersDrawerOpen = false
          "
        >
          <LuSlidersHorizontal class="h-4 w-4" />
        </button>
      </div>
    </header>

    <!-- Floating Reader Settings Popover -->
    <div
      v-if="settingsDrawerOpen && ctx"
      class="fixed top-16 right-4 z-[var(--z-sheet)] w-80 space-y-4 rounded-[24px] border border-[var(--md-sys-color-outline-variant)]/60 bg-[var(--md-sys-color-surface-container-high)] p-5 text-[var(--md-sys-color-on-surface)] shadow-[var(--md-sys-elevation-3)]"
    >
      <div>
        <label class="mb-1 block text-xs font-semibold text-[var(--md-sys-color-on-surface)]">
          {{ t('settings.defaultReaderMode') }}
        </label>
        <AppSelect
          size="sm"
          :model-value="readerMode"
          :options="modeOptions"
          @update:model-value="changeMode($event as ReaderMode)"
        />
      </div>

      <div v-if="readerMode !== 'webtoon'">
        <label class="mb-1 block text-xs font-semibold text-[var(--md-sys-color-on-surface)]">
          {{ t('settings.defaultFitMode') }}
        </label>
        <AppSelect
          size="sm"
          :model-value="fitMode"
          :options="fitOptions"
          @update:model-value="changeFit($event as ReaderFitMode)"
        />
      </div>

      <div v-if="readerMode === 'rtl_double' || readerMode === 'ltr_double'">
        <AppCheckbox
          :model-value="offsetCoverPage"
          :label="t('reader.offsetCover')"
          @update:model-value="toggleOffsetCover"
        />
      </div>

      <div v-if="readerMode === 'webtoon'">
        <label class="mb-1 block text-xs font-semibold text-[var(--md-sys-color-on-surface)]">
          {{ t('reader.webtoonWidth') }} ({{ webtoonWidth }}px)
        </label>
        <input
          v-model.number="webtoonWidth"
          type="range"
          min="520"
          max="1400"
          step="20"
          class="w-full accent-[var(--md-sys-color-primary)]"
          @change="persistSeriesReaderPref"
        />
      </div>

      <div>
        <AppCheckbox
          :model-value="dimImages"
          :label="t('reader.eyeCareDim')"
          @update:model-value="toggleDimImages"
        />
      </div>

      <p
        class="border-t border-[var(--md-sys-color-outline-variant)]/55 pt-3 text-[11px] leading-relaxed text-[var(--md-sys-color-on-surface-variant)]"
      >
        {{ t('reader.shortcutsHint') }}
      </p>
    </div>

    <!-- Floating Chapter Switcher Popover -->
    <div
      v-if="chaptersDrawerOpen && ctx"
      class="fixed top-16 right-14 z-[var(--z-sheet)] max-h-[70vh] w-72 overflow-y-auto rounded-[24px] border border-[var(--md-sys-color-outline-variant)]/60 bg-[var(--md-sys-color-surface-container-high)] p-2 text-[var(--md-sys-color-on-surface)] shadow-[var(--md-sys-elevation-3)]"
    >
      <button
        v-for="ch in ctx.all_chapters"
        :key="ch.id"
        type="button"
        class="md-state flex w-full items-center justify-between gap-2 rounded-xl px-3 py-2 text-left text-xs transition"
        :class="
          ch.id === ctx.chapter.id
            ? 'bg-[var(--md-sys-color-secondary-container)] font-semibold text-[var(--md-sys-color-on-secondary-container)]'
            : 'text-[var(--md-sys-color-on-surface)] hover:bg-[var(--md-sys-color-surface-container-highest)]'
        "
        @click="goToChapter(ch.id)"
      >
        <span class="truncate">#{{ ch.number_label }} {{ ch.title }}</span>
        <span class="shrink-0 text-[10px] text-[var(--md-sys-color-on-surface-variant)]">
          {{ ch.page_count }}P
        </span>
      </button>
    </div>

    <!-- Main Comic Stage -->
    <div
      v-if="loadError && !loading"
      class="flex flex-1 flex-col items-center justify-center gap-3 px-6 text-center"
    >
      <p class="text-sm text-[var(--md-sys-color-error)]">{{ loadError }}</p>
      <button
        type="button"
        class="md-btn md-btn-tonal md-state"
        @click="loadChapter(String(route.params.chapterId), undefined, false)"
      >
        {{ globalT('common.refresh') }}
      </button>
    </div>

    <div
      v-else-if="ctx && !loading"
      class="flex-1 overflow-hidden"
      @click="onStageClick"
    >
      <!-- Vertical Webtoon Seamless Scroll Mode -->
      <div
        v-if="readerMode === 'webtoon'"
        ref="webtoonContainerRef"
        class="h-full w-full overflow-y-auto scroll-smooth"
      >
        <div
          class="mx-auto flex flex-col items-center"
          :style="{
            width: '100%',
            maxWidth: Math.round((webtoonWidth * zoomPercent) / 100) + 'px',
          }"
        >
          <img
            v-for="p in totalPages"
            :key="ctx.chapter.id + '-wt-' + p"
            :data-page="p"
            :src="chapterPageUrl(ctx.chapter.id, p)"
            :alt="'Page ' + p"
            loading="lazy"
            class="m-0 block min-h-[40vh] w-full bg-[var(--md-sys-color-surface-container-low)]/40 p-0"
            :class="eyeCareClass"
          />

          <!-- Seamless Next Chapter Card at Bottom of Webtoon Strip -->
          <div class="my-12 flex w-full flex-col items-center gap-3 px-6 text-center" @click.stop>
            <button
              v-if="ctx.next_chapter"
              type="button"
              class="md-btn md-btn-filled md-state !h-11 !px-7 text-sm shadow-[var(--md-sys-elevation-2)]"
              @click="goToChapter(ctx.next_chapter.id)"
            >
              <span>{{ t('reader.nextChapter') }}: #{{ ctx.next_chapter.number_label }} {{ ctx.next_chapter.title }}</span>
              <LuChevronRight class="h-4 w-4" />
            </button>
            <p v-else class="text-sm text-[var(--md-sys-color-on-surface-variant)]">
              {{ t('reader.reachedSeriesEnd') }}
            </p>
          </div>
        </div>
      </div>

      <!-- Paginated Single / Double-Page Spread Mode (RTL & LTR) -->
      <div
        v-else
        class="flex h-full w-full items-center justify-center overflow-auto"
      >
        <div
          class="flex items-center justify-center gap-0"
          :class="isRtl ? 'flex-row-reverse' : 'flex-row'"
        >
          <img
            v-for="p in currentSpread"
            :key="ctx.chapter.id + '-sp-' + p"
            :src="chapterPageUrl(ctx.chapter.id, p)"
            :alt="'Page ' + p"
            :style="pageImageStyle"
            class="m-0 block object-contain p-0 select-none"
            :class="eyeCareClass"
            @load="onImageLoaded(p, $event)"
          />
        </div>
      </div>
    </div>

    <!-- Bottom Floating Page Scrubber HUD -->
    <footer
      v-if="ctx"
      v-show="hudVisible"
      class="fixed right-0 bottom-0 left-0 z-[var(--z-base-overlay)] flex h-14 items-center gap-4 border-t border-[var(--md-sys-color-outline-variant)]/60 bg-[var(--md-sys-color-surface-container-high)]/90 px-4 text-[var(--md-sys-color-on-surface)] shadow-[var(--md-sys-elevation-2)] backdrop-blur-md"
    >
      <button
        type="button"
        :disabled="!ctx.prev_chapter"
        :title="ctx.prev_chapter ? t('reader.prevChapter') : t('reader.noPrevChapter')"
        class="md-btn md-btn-sm md-btn-tonal md-state"
        @click="ctx.prev_chapter && goToChapter(ctx.prev_chapter.id)"
      >
        <LuSkipBack class="h-3.5 w-3.5" />
        <span class="hidden sm:inline">{{ t('reader.prevChapter') }}</span>
      </button>

      <div class="flex flex-1 items-center gap-3">
        <span
          class="w-24 shrink-0 text-center font-mono text-xs text-[var(--md-sys-color-on-surface-variant)]"
        >
          {{
            currentSpread.length > 1
              ? (isRtl
                  ? currentSpread[1] + '-' + currentSpread[0]
                  : currentSpread[0] + '-' + currentSpread[1]) +
                ' / ' +
                totalPages
              : currentPage + ' / ' + totalPages
          }}
        </span>

        <input
          type="range"
          min="1"
          :max="Math.max(1, totalPages)"
          :value="currentPage"
          :dir="isRtl ? 'rtl' : 'ltr'"
          class="h-1.5 flex-1 cursor-pointer accent-[var(--md-sys-color-primary)]"
          @input="onScrubberInput"
        />
      </div>

      <button
        v-if="dimImages"
        type="button"
        :title="t('reader.eyeCareDim')"
        class="md-icon-btn md-icon-btn-sm md-state hidden !bg-[var(--md-sys-color-warning-container)] !text-[var(--md-sys-color-on-warning-container)] sm:inline-flex"
        @click="toggleDimImages(false)"
      >
        <LuEye class="h-4 w-4" />
      </button>

      <button
        type="button"
        :disabled="!ctx.next_chapter"
        :title="ctx.next_chapter ? t('reader.nextChapter') : t('reader.noNextChapter')"
        class="md-btn md-btn-sm md-btn-tonal md-state"
        @click="ctx.next_chapter && goToChapter(ctx.next_chapter.id)"
      >
        <span class="hidden sm:inline">{{ t('reader.nextChapter') }}</span>
        <LuSkipForward class="h-3.5 w-3.5" />
      </button>
    </footer>
  </div>
</template>

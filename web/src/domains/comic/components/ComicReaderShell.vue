<script setup lang="ts">
import { computed, nextTick, onBeforeUnmount, onMounted, reactive, ref, watch } from 'vue'
import { useI18n as useGlobalI18n } from 'vue-i18n'
import {
  LuAlertTriangle,
  LuBookmark,
  LuChevronRight,
  LuColumns2,
  LuEye,
  LuFileText,
  LuLayers,
  LuMaximize,
  LuMinimize,
  LuRotateCw,
  LuScrollText,
  LuSkipBack,
  LuSkipForward,
  LuSlidersHorizontal,
  LuTrash2,
  LuZoomIn,
  LuZoomOut,
} from 'vue-icons-plus/lu'
import { ImmersiveMediaTemplate } from '@/components/domain'
import {
  AppCheckbox,
  AppSlider,
  MdButton,
  MdCircularProgress,
  MdEmptyState,
  MdIconButton,
  MdSegmentedButton,
  MdSwitch,
  MdTabs,
  MdTextField,
  type MdTabItem,
  type SegmentedOption,
} from '@/components/ui'
import { useComicI18n as useI18n } from '@/domains/comic/i18n'
import type { ReaderFitMode, ReaderMode } from '@/domains/comic/types'
import { useSnackbarStore } from '@/stores/snackbar'
import { useThemeStore } from '@/stores/theme'

export interface ComicReaderChapterItem {
  key: string
  label: string
  metaLabel?: string
  disabled?: boolean
}

export interface ComicReaderBookmarkItem {
  id: string
  page: number
  title: string
  note?: string | null
}

type ReaderSheetId = 'chapters' | 'settings'
type ChaptersSheetTab = 'chapters' | 'bookmarks'

const props = withDefaults(
  defineProps<{
    title: string
    subtitle: string
    backLabel: string
    chapterKey: string
    totalPages: number
    initialPage?: number
    getPageSrc: (pageNum: number) => string
    chapters: ComicReaderChapterItem[]
    bookmarks?: ComicReaderBookmarkItem[]
    showBookmarksTab?: boolean
    hasPrevChapter: boolean
    hasNextChapter: boolean
    nextChapterToastLabel?: string
    readerMode: ReaderMode
    fitMode: ReaderFitMode
    offsetCoverPage: boolean
    dimImages: boolean
    webtoonWidth: number
    preloadPages?: number
    loading: boolean
    loadError: string | null
    hasContent: boolean
  }>(),
  {
    initialPage: 1,
    bookmarks: () => [],
    showBookmarksTab: false,
    nextChapterToastLabel: '',
    preloadPages: 3,
  },
)

const emit = defineEmits<{
  'update:readerMode': [mode: ReaderMode]
  'update:fitMode': [fit: ReaderFitMode]
  'update:offsetCoverPage': [val: boolean]
  'update:dimImages': [val: boolean]
  'update:webtoonWidth': [val: number]
  'commit-webtoon-width': [val: number]
  'page-change': [payload: { page: number; highestInSpread: number; reachedEnd: boolean }]
  'prev-chapter': []
  'next-chapter': []
  'select-chapter': [key: string]
  'delete-bookmark': [id: string]
  back: []
  retry: []
}>()

const { t } = useI18n()
const { t: globalT } = useGlobalI18n()
const theme = useThemeStore()
const snackbar = useSnackbarStore()

const immersiveRef = ref<InstanceType<typeof ImmersiveMediaTemplate> | null>(null)
const webtoonContainerRef = ref<HTMLDivElement | null>(null)
const chaptersListRef = ref<HTMLDivElement | null>(null)

const activeSheet = ref<ReaderSheetId | null>(null)
const chaptersSheetTab = ref<ChaptersSheetTab>('chapters')
const chapterFilter = ref('')
const isFullscreen = ref(false)

const currentPage = ref(1)
const zoomPercent = ref(100)
const pendingNextChapterTap = ref(false)

const widePages = reactive<Record<number, boolean>>({})
const naturalWidths = reactive<Record<number, number>>({})
const brokenPages = reactive<Record<number, boolean>>({})
const retryTokens = reactive<Record<number, number>>({})
const preloadedSet = new Set<string>()

let webtoonObserver: IntersectionObserver | null = null
const visibleWebtoonPages = new Set<number>()
let suppressWebtoonObserverUntil = 0
let lastWebtoonScrollTop = 0
let unmounted = false

const safeTotalPages = computed(() => Math.max(1, props.totalPages))
const isSinglePage = computed(() => safeTotalPages.value <= 1)
const isRtl = computed(() => props.readerMode === 'rtl_double')

const spreads = computed<number[][]>(() => {
  const total = safeTotalPages.value
  if (props.readerMode === 'ltr_single' || props.readerMode === 'webtoon') {
    return Array.from({ length: total }, (_, i) => [i + 1])
  }

  const groups: number[][] = []
  let p = 1
  while (p <= total) {
    if (p === 1 && props.offsetCoverPage) {
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
  () => spreads.value[currentSpreadIndex.value] || [1],
)

const pageCounterText = computed(() => {
  if (currentSpread.value.length > 1) {
    return `${currentSpread.value[0]}-${currentSpread.value[1]} / ${props.totalPages}`
  }
  return `${currentPage.value} / ${props.totalPages}`
})

function pageImageStyle(pageNum: number): Record<string, string> {
  const scale = zoomPercent.value / 100
  if (props.fitMode === 'fit_height') {
    return {
      maxHeight: `${Math.round(100 * scale)}dvh`,
      width: 'auto',
      maxWidth:
        currentSpread.value.length > 1
          ? `${Math.round(50 * scale)}vw`
          : `${Math.round(100 * scale)}vw`,
    }
  }
  if (props.fitMode === 'fit_width') {
    return {
      width:
        currentSpread.value.length > 1
          ? `${Math.round(50 * scale)}vw`
          : `${Math.round(96 * scale)}vw`,
      height: 'auto',
    }
  }
  const natW = naturalWidths[pageNum]
  if (natW && natW > 0) {
    return {
      width: `${Math.round(natW * scale)}px`,
      height: 'auto',
      maxWidth: 'none',
    }
  }
  return {
    width: `${Math.round(100 * scale)}%`,
    height: 'auto',
    maxWidth: 'none',
  }
}

// Works visibly in BOTH dark and light themes
const eyeCareClass = computed(() => {
  if (!props.dimImages) return ''
  return theme.isDark
    ? 'brightness-[0.82] contrast-[0.95]'
    : 'brightness-[0.94] sepia-[0.14] contrast-[0.96]'
})

const modeOptions = computed<SegmentedOption<ReaderMode>[]>(() => [
  { value: 'rtl_double', label: t('reader.modeRtlDouble'), icon: LuColumns2 },
  { value: 'ltr_single', label: t('reader.modeLtrSingle'), icon: LuFileText },
  { value: 'ltr_double', label: t('reader.modeLtrDouble'), icon: LuColumns2 },
  { value: 'webtoon', label: t('reader.modeWebtoon'), icon: LuScrollText },
])

const fitOptions = computed<SegmentedOption<ReaderFitMode>[]>(() => [
  { value: 'fit_height', label: t('reader.fitHeight') },
  { value: 'fit_width', label: t('reader.fitWidth') },
  { value: 'original', label: t('reader.fitOriginal') },
])

const sheetTitle = computed(() => {
  if (activeSheet.value === 'settings') return t('reader.settings')
  if (activeSheet.value === 'chapters') return t('reader.chapterList')
  return ''
})

const sheetSubtitle = computed(() => {
  if (activeSheet.value === 'chapters') return props.title
  return ''
})

const chaptersSheetTabs = computed<MdTabItem<ChaptersSheetTab>[]>(() => [
  {
    value: 'chapters',
    label: t('reader.chapterList'),
    badge: props.chapters.length > 0 ? props.chapters.length : undefined,
  },
  {
    value: 'bookmarks',
    label: t('reader.bookmarksTab'),
    badge: props.bookmarks.length > 0 ? props.bookmarks.length : undefined,
  },
])

const filteredChapters = computed(() => {
  const q = chapterFilter.value.trim().toLowerCase()
  if (!q) return props.chapters
  return props.chapters.filter((ch) => ch.label.toLowerCase().includes(q))
})

const sortedBookmarks = computed(() =>
  [...props.bookmarks].sort((a, b) => a.page - b.page),
)

const hasChaptersButton = computed(
  () => props.hasContent && (props.chapters.length > 0 || props.showBookmarksTab),
)

function pageSrc(pageNum: number): string {
  const base = props.getPageSrc(pageNum)
  if (!base) return ''
  const token = retryTokens[pageNum]
  if (!token) return base
  if (base.startsWith('data:')) return base
  const sep = base.includes('?') ? '&' : '?'
  return `${base}${sep}_retry=${token}`
}

function resetStageState(): void {
  teardownWebtoonObserver()
  pendingNextChapterTap.value = false
  preloadedSet.clear()
  for (const k of Object.keys(widePages)) delete widePages[Number(k)]
  for (const k of Object.keys(naturalWidths)) delete naturalWidths[Number(k)]
  for (const k of Object.keys(brokenPages)) delete brokenPages[Number(k)]
  for (const k of Object.keys(retryTokens)) delete retryTokens[Number(k)]
}

function onImageLoaded(pageNum: number, ev: Event): void {
  const img = ev.target as HTMLImageElement
  delete brokenPages[pageNum]
  if (img.naturalWidth > 0 && img.naturalHeight > 0) {
    naturalWidths[pageNum] = img.naturalWidth
    widePages[pageNum] = img.naturalWidth > img.naturalHeight * 1.12
  }
}

function onImageError(pageNum: number): void {
  brokenPages[pageNum] = true
}

function retryPageImage(pageNum: number): void {
  delete brokenPages[pageNum]
  retryTokens[pageNum] = (retryTokens[pageNum] || 0) + 1
}

function preloadAround(centerPage: number): void {
  if (!props.hasContent || props.loading || props.totalPages <= 0) return
  const ahead = Math.max(3, props.preloadPages || 3)
  const keyPrefix = props.chapterKey
  const maxPage = props.totalPages

  const candidates: number[] = []
  for (let offset = 0; offset <= ahead + 1; offset++) {
    if (centerPage + offset <= maxPage) candidates.push(centerPage + offset)
  }
  if (centerPage - 1 >= 1) candidates.push(centerPage - 1)

  for (const p of candidates) {
    const src = props.getPageSrc(p)
    if (!src) continue
    const key = `${keyPrefix}:${p}`
    if (preloadedSet.has(key)) continue
    preloadedSet.add(key)
    const img = new Image()
    img.onload = () => {
      if (
        !unmounted &&
        img.naturalWidth > 0 &&
        img.naturalHeight > 0 &&
        props.chapterKey === keyPrefix
      ) {
        naturalWidths[p] = img.naturalWidth
        widePages[p] = img.naturalWidth > img.naturalHeight * 1.12
      }
    }
    img.onerror = () => {
      // swallow preload failures cleanly
    }
    img.src = src
  }
}

function emitPageChange(pageVal: number, forceReachedEnd = false): void {
  if (!props.hasContent || props.loading) return
  const highestInSpread = Math.max(...currentSpread.value, pageVal)
  const reachedEnd = forceReachedEnd || highestInSpread >= props.totalPages
  emit('page-change', { page: pageVal, highestInSpread, reachedEnd })
}

watch(currentPage, (p) => {
  pendingNextChapterTap.value = false
  preloadAround(p)
  emitPageChange(p)
})

watch(activeSheet, async (sheet) => {
  if (sheet === 'chapters') {
    chapterFilter.value = ''
    await nextTick()
    const activeBtn = chaptersListRef.value?.querySelector<HTMLElement>('[data-active-chapter="true"]')
    activeBtn?.scrollIntoView({ block: 'center' })
  }
})

watch(
  () => [props.readerMode, props.chapterKey, props.loading, props.hasContent] as const,
  async ([mode, newKey, isLoading, ready], oldVal) => {
    const oldMode = oldVal?.[0]
    const oldKey = oldVal?.[1]
    const oldLoading = oldVal?.[2]
    const oldReady = oldVal?.[3]

    const keyChanged = newKey !== oldKey
    const contentJustReady =
      keyChanged || oldVal === undefined || Boolean(oldLoading) || !oldReady

    if (keyChanged) {
      resetStageState()
    } else {
      teardownWebtoonObserver()
    }

    if (isLoading || !ready) {
      return
    }

    if (contentJustReady) {
      const targetInitial = Math.min(
        Math.max(1, props.initialPage || 1),
        Math.max(1, props.totalPages),
      )
      currentPage.value = targetInitial
      preloadAround(targetInitial)
      suppressWebtoonObserverUntil = Date.now() + 600
      // Report initial position so online/local readers capture initial chapter load
      emitPageChange(targetInitial)
    } else if (mode !== oldMode && mode === 'webtoon') {
      suppressWebtoonObserverUntil = Date.now() + 600
    }

    if (mode === 'webtoon' && newKey) {
      await nextTick()
      if (
        unmounted ||
        props.chapterKey !== newKey ||
        props.readerMode !== 'webtoon' ||
        props.loading ||
        !props.hasContent
      ) {
        return
      }
      scrollToWebtoonPage(currentPage.value)
      setupWebtoonObserver()
    }
  },
  { immediate: true },
)

function adjustZoom(delta: number): void {
  const next = Math.max(50, Math.min(200, zoomPercent.value + delta))
  zoomPercent.value = next
  immersiveRef.value?.showOsd(`${next}%`)
}

function resetZoom(): void {
  zoomPercent.value = 100
  immersiveRef.value?.showOsd('100%')
}

function toggleSheet(id: ReaderSheetId): void {
  activeSheet.value = activeSheet.value === id ? null : id
}

function preferredScrollBehavior(): ScrollBehavior {
  if (typeof window !== 'undefined' && window.matchMedia?.('(prefers-reduced-motion: reduce)').matches) {
    return 'auto'
  }
  return 'smooth'
}

function advanceForward(): void {
  if (!props.hasContent || props.loading) return
  if (props.readerMode === 'webtoon') {
    webtoonContainerRef.value?.scrollBy({
      top: window.innerHeight * 0.82,
      behavior: preferredScrollBehavior(),
    })
    return
  }

  const nextIdx = currentSpreadIndex.value + 1
  if (nextIdx < spreads.value.length) {
    currentPage.value = spreads.value[nextIdx]![0]!
    return
  }

  emitPageChange(currentPage.value, true)
  if (props.hasNextChapter) {
    if (!pendingNextChapterTap.value) {
      pendingNextChapterTap.value = true
      snackbar.show(
        t('reader.endOfChapterToast', {
          next: props.nextChapterToastLabel || t('reader.nextChapter'),
        }),
      )
    } else {
      emit('next-chapter')
    }
  } else {
    snackbar.show(t('reader.reachedSeriesEnd'))
  }
}

function stepBackward(): void {
  if (!props.hasContent || props.loading) return
  pendingNextChapterTap.value = false
  if (props.readerMode === 'webtoon') {
    webtoonContainerRef.value?.scrollBy({
      top: -window.innerHeight * 0.82,
      behavior: preferredScrollBehavior(),
    })
    return
  }

  const prevIdx = currentSpreadIndex.value - 1
  if (prevIdx >= 0) {
    currentPage.value = spreads.value[prevIdx]![0]!
  } else if (props.hasPrevChapter) {
    emit('prev-chapter')
  }
}

function handleTapLeft(): void {
  if (isRtl.value) advanceForward()
  else stepBackward()
}

function handleTapRight(): void {
  if (isRtl.value) stepBackward()
  else advanceForward()
}

function handleSelectChapter(key: string): void {
  activeSheet.value = null
  emit('select-chapter', key)
}

function handlePrevChapterClick(): void {
  if (!props.hasPrevChapter) return
  activeSheet.value = null
  emit('prev-chapter')
}

function handleNextChapterClick(): void {
  if (!props.hasNextChapter) return
  activeSheet.value = null
  emit('next-chapter')
}

function jumpToPage(targetPage: number): void {
  const clamped = Math.max(1, Math.min(safeTotalPages.value, Math.round(targetPage)))
  currentPage.value = clamped
  activeSheet.value = null
  if (props.readerMode === 'webtoon') {
    scrollToWebtoonPage(clamped)
  }
}

function onScrubberValue(targetPage: number): void {
  if (isSinglePage.value) return
  const clamped = Math.max(1, Math.min(safeTotalPages.value, Math.round(targetPage)))
  currentPage.value = clamped
  if (props.readerMode === 'webtoon') {
    scrollToWebtoonPage(clamped)
  }
}

function onWebtoonScroll(): void {
  const container = webtoonContainerRef.value
  if (!container) return
  const cur = container.scrollTop
  if (Math.abs(cur - lastWebtoonScrollTop) > 48 && activeSheet.value === null) {
    immersiveRef.value?.hideHud()
    lastWebtoonScrollTop = cur
  }
}

function scrollToWebtoonPage(pageNum: number): void {
  const container = webtoonContainerRef.value
  if (!container) return
  suppressWebtoonObserverUntil = Date.now() + 450
  const el = container.querySelector(`[data-page="${pageNum}"]`) as HTMLElement | null
  if (el) {
    el.scrollIntoView({ behavior: 'instant', block: 'start' })
    lastWebtoonScrollTop = container.scrollTop
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
  if (!container || props.readerMode !== 'webtoon' || props.loading || !props.hasContent) return
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

async function toggleFullscreen(): Promise<void> {
  try {
    if (!document.fullscreenElement) {
      await document.documentElement.requestFullscreen()
    } else {
      await document.exitFullscreen()
    }
  } catch {
    // ignore fullscreen permission restrictions
  }
}

function handleFullscreenChange(): void {
  isFullscreen.value = Boolean(document.fullscreenElement)
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

  // Escape is handled by ImmersiveMediaTemplate via useEscapeStack (sheet -> HUD -> exit)
  if (e.key === 'Escape') return
  if (e.metaKey || e.ctrlKey || e.altKey) return
  if (activeSheet.value !== null) return
  if (target?.closest('[aria-modal="true"]')) return

  if (e.key === ' ') {
    e.preventDefault()
    advanceForward()
  } else if (e.key === 'ArrowLeft') {
    e.preventDefault()
    if (isRtl.value) advanceForward()
    else stepBackward()
  } else if (e.key === 'ArrowRight') {
    e.preventDefault()
    if (isRtl.value) stepBackward()
    else advanceForward()
  } else if (e.key === 'ArrowDown' && props.readerMode === 'webtoon') {
    webtoonContainerRef.value?.scrollBy({ top: 320, behavior: preferredScrollBehavior() })
  } else if (e.key === 'ArrowUp' && props.readerMode === 'webtoon') {
    webtoonContainerRef.value?.scrollBy({ top: -320, behavior: preferredScrollBehavior() })
  } else if (e.key === '+' || e.key === '=') {
    e.preventDefault()
    adjustZoom(10)
  } else if (e.key === '-') {
    e.preventDefault()
    adjustZoom(-10)
  } else if (e.key === '0') {
    e.preventDefault()
    resetZoom()
  } else if (e.key === 'h' || e.key === 'H') {
    e.preventDefault()
    immersiveRef.value?.toggleHud()
  } else if (e.key === 'f' || e.key === 'F') {
    e.preventDefault()
    void toggleFullscreen()
  } else if (e.key === '[') {
    e.preventDefault()
    handlePrevChapterClick()
  } else if (e.key === ']') {
    e.preventDefault()
    handleNextChapterClick()
  }
}

onMounted(() => {
  unmounted = false
  isFullscreen.value = Boolean(document.fullscreenElement)
  window.addEventListener('keydown', onKeydown)
  document.addEventListener('fullscreenchange', handleFullscreenChange)
})

onBeforeUnmount(() => {
  unmounted = true
  window.removeEventListener('keydown', onKeydown)
  document.removeEventListener('fullscreenchange', handleFullscreenChange)
  teardownWebtoonObserver()
  preloadedSet.clear()
})

defineExpose({
  currentPage,
  jumpToPage,
})
</script>

<template>
  <ImmersiveMediaTemplate
    ref="immersiveRef"
    :title="title"
    :subtitle="subtitle"
    :back-label="backLabel"
    :auto-hide="true"
    :auto-hide-delay-ms="2800"
    :lock-hud="loading || Boolean(loadError) || !hasContent"
    :center-tap-mode="readerMode === 'webtoon' ? 'full-stage' : 'center-zone'"
    :active-sheet="activeSheet"
    :sheet-title="sheetTitle"
    :sheet-subtitle="sheetSubtitle"
    sheet-size="sm"
    @update:active-sheet="activeSheet = ($event as ReaderSheetId | null)"
    @back="emit('back')"
    @tap-prev="handleTapLeft"
    @tap-next="handleTapRight"
  >
    <template #header-meta>
      <slot name="header-meta" />
    </template>

    <!-- Strict <= 5 Top HUD Controls (Back + optional Bookmark + Chapters + Settings + Fullscreen) -->
    <template #header-controls>
      <slot name="toolbar-actions" />

      <MdIconButton
        v-if="hasChaptersButton"
        size="sm"
        variant="standard"
        :selected="activeSheet === 'chapters'"
        :icon="LuLayers"
        :aria-label="t('reader.chapterList')"
        :tooltip="t('reader.chapterList')"
        @click="toggleSheet('chapters')"
      />

      <MdIconButton
        v-if="hasContent"
        size="sm"
        variant="standard"
        :selected="activeSheet === 'settings'"
        :icon="LuSlidersHorizontal"
        :aria-label="t('reader.settings')"
        :tooltip="t('reader.settings')"
        @click="toggleSheet('settings')"
      />

      <MdIconButton
        size="sm"
        variant="standard"
        :icon="isFullscreen ? LuMinimize : LuMaximize"
        :aria-label="isFullscreen ? t('reader.fullscreenExit') : t('reader.fullscreenEnter')"
        :tooltip="isFullscreen ? t('reader.fullscreenExit') : t('reader.fullscreenEnter')"
        class="hidden sm:inline-flex"
        @click="toggleFullscreen"
      />
    </template>

    <!-- Main Comic Stage -->
    <div class="flex h-full w-full flex-col overflow-hidden">
      <!-- Loading State -->
      <div
        v-if="loading"
        class="flex flex-1 flex-col items-center justify-center gap-3 px-6 pt-14 text-center text-body-small text-on-surface-variant"
      >
        <MdCircularProgress indeterminate :size="36" :stroke-width="3" />
        <span>{{ t('online.loadingPages') }}</span>
      </div>

      <!-- Error / Empty State -->
      <div
        v-else-if="loadError || !hasContent"
        class="flex flex-1 flex-col items-center justify-center gap-4 px-6 pt-14 text-center"
        data-immersive-ignore
      >
        <p role="alert" class="text-body-medium max-w-md text-error">
          {{ loadError || t('online.emptyPages') }}
        </p>
        <div class="flex flex-wrap items-center justify-center gap-2">
          <MdButton variant="tonal" @click="emit('retry')">
            {{ globalT('common.refresh') }}
          </MdButton>
          <MdButton variant="outlined" @click="emit('back')">
            {{ backLabel }}
          </MdButton>
          <slot name="error-actions" />
        </div>
      </div>

      <!-- Vertical Webtoon Seamless Scroll Mode -->
      <div
        v-else-if="readerMode === 'webtoon'"
        ref="webtoonContainerRef"
        class="h-full w-full overflow-y-auto motion-safe:scroll-smooth"
        @scroll.passive="onWebtoonScroll"
      >
        <div
          class="mx-auto flex flex-col items-center"
          :style="{
            width: '100%',
            maxWidth: `${Math.round((webtoonWidth * zoomPercent) / 100)}px`,
          }"
        >
          <div
            v-for="p in totalPages"
            :key="`${chapterKey}-wt-${p}`"
            :data-page="p"
            class="relative w-full"
          >
            <img
              v-if="pageSrc(p)"
              :src="pageSrc(p)"
              :alt="`Page ${p}`"
              :loading="p <= 4 ? 'eager' : 'lazy'"
              class="m-0 block min-h-[40vh] w-full bg-surface-container-low/40 p-0"
              :class="eyeCareClass"
              @load="onImageLoaded(p, $event)"
              @error="onImageError(p)"
            />
            <div
              v-if="!pageSrc(p) || brokenPages[p]"
              data-immersive-ignore
              class="flex min-h-40 w-full flex-col items-center justify-center gap-2 bg-surface-container-low/95 p-6 text-center text-body-small text-on-surface-variant"
              :class="pageSrc(p) ? 'absolute inset-0' : ''"
              @click.stop
            >
              <LuAlertTriangle class="h-6 w-6 text-error" aria-hidden="true" />
              <span>{{ t('online.imageLoadError', { page: p }) }}</span>
              <MdButton
                v-if="pageSrc(p)"
                variant="tonal"
                size="sm"
                :icon="LuRotateCw"
                @click="retryPageImage(p)"
              >
                {{ t('online.retryImage') }}
              </MdButton>
            </div>
          </div>

          <!-- Seamless Next Chapter Card at Bottom of Webtoon Strip -->
          <div
            data-immersive-ignore
            class="my-12 flex w-full flex-col items-center gap-3 px-6 text-center"
            @click.stop
          >
            <MdButton
              v-if="hasNextChapter"
              variant="filled"
              :aria-label="t('reader.nextChapterAria')"
              @click="handleNextChapterClick"
            >
              <span>
                {{ t('reader.nextChapter') }}<template v-if="nextChapterToastLabel">: {{ nextChapterToastLabel }}</template>
              </span>
              <template #trailing>
                <LuChevronRight class="h-4 w-4" aria-hidden="true" />
              </template>
            </MdButton>
            <p v-else class="text-body-medium text-on-surface-variant">
              {{ t('reader.reachedSeriesEnd') }}
            </p>
          </div>
        </div>
      </div>

      <!-- Paginated Single / Double-Page Spread Mode (RTL & LTR) -->
      <div v-else class="flex h-full w-full overflow-auto">
        <div
          class="m-auto flex items-center justify-center gap-0"
          :class="isRtl ? 'flex-row-reverse' : 'flex-row'"
        >
          <div
            v-for="p in currentSpread"
            :key="`${chapterKey}-sp-${p}`"
            class="relative flex items-center justify-center"
          >
            <img
              v-if="pageSrc(p)"
              :src="pageSrc(p)"
              :alt="`Page ${p}`"
              :style="pageImageStyle(p)"
              class="m-0 block object-contain p-0 select-none"
              :class="eyeCareClass"
              @load="onImageLoaded(p, $event)"
              @error="onImageError(p)"
            />
            <div
              v-if="!pageSrc(p) || brokenPages[p]"
              data-immersive-ignore
              class="flex min-h-60 min-w-52 flex-col items-center justify-center gap-2 rounded-corner-md bg-surface-container-low/95 p-6 text-center text-body-small text-on-surface-variant"
              :class="pageSrc(p) ? 'absolute inset-0' : ''"
              @click.stop
            >
              <LuAlertTriangle class="h-6 w-6 text-error" aria-hidden="true" />
              <span>{{ t('online.imageLoadError', { page: p }) }}</span>
              <MdButton
                v-if="pageSrc(p)"
                variant="tonal"
                size="sm"
                :icon="LuRotateCw"
                @click="retryPageImage(p)"
              >
                {{ t('online.retryImage') }}
              </MdButton>
            </div>
          </div>
        </div>
      </div>
    </div>

    <!-- Bottom Floating Page Scrubber HUD (RTL-aligned direction & buttons) -->
    <template v-if="hasContent && !loading && !loadError" #footer>
      <div
        class="flex items-center gap-3"
        :class="isRtl ? 'flex-row-reverse' : 'flex-row'"
      >
        <MdButton
          variant="tonal"
          size="sm"
          :icon="isRtl ? LuSkipForward : LuSkipBack"
          :disabled="!hasPrevChapter"
          :aria-label="hasPrevChapter ? t('reader.prevChapterAria') : t('reader.noPrevChapter')"
          class="shrink-0"
          @click="handlePrevChapterClick"
        >
          <span class="hidden sm:inline">{{ t('reader.prevChapter') }}</span>
        </MdButton>

        <div
          class="flex min-w-0 flex-1 items-center gap-3"
          :class="isRtl ? 'flex-row-reverse' : 'flex-row'"
        >
          <span class="text-label-medium w-20 shrink-0 text-center font-mono tabular-nums text-on-surface-variant">
            {{ pageCounterText }}
          </span>

          <div class="min-w-0 flex-1">
            <AppSlider
              :model-value="isSinglePage ? 1 : currentPage"
              :min="isSinglePage ? 0 : 1"
              :max="isSinglePage ? 1 : safeTotalPages"
              :step="1"
              :rtl="isRtl"
              :disabled="isSinglePage"
              :aria-label="t('reader.page')"
              @update:model-value="onScrubberValue"
            />
          </div>
        </div>

        <MdButton
          variant="tonal"
          size="sm"
          :disabled="!hasNextChapter"
          :aria-label="hasNextChapter ? t('reader.nextChapterAria') : t('reader.noNextChapter')"
          class="shrink-0"
          @click="handleNextChapterClick"
        >
          <span class="hidden sm:inline">{{ t('reader.nextChapter') }}</span>
          <template #trailing>
            <component :is="isRtl ? LuSkipBack : LuSkipForward" class="h-4 w-4" aria-hidden="true" />
          </template>
        </MdButton>
      </div>
    </template>

    <!-- Sheet Subheader (Chapters/Bookmarks Tabs + Chapter Filter) -->
    <template v-if="activeSheet === 'chapters'" #sheet-subheader>
      <div class="space-y-2.5 pb-2">
        <MdTabs
          v-if="showBookmarksTab || bookmarks.length > 0"
          :model-value="chaptersSheetTab"
          :tabs="chaptersSheetTabs"
          variant="secondary"
          @update:model-value="chaptersSheetTab = $event as ChaptersSheetTab"
        />
        <MdTextField
          v-if="chaptersSheetTab === 'chapters' && chapters.length > 10"
          v-model="chapterFilter"
          type="search"
          size="sm"
          clearable
          :placeholder="t('reader.filterChaptersPlaceholder')"
        />
      </div>
    </template>

    <!-- Single Mutually-Exclusive Responsive MdSheet -->
    <template #sheet>
      <!-- Settings Sheet (Mode / Fit / Zoom / RTL Cover Offset / Webtoon Width / Eye-care) -->
      <div v-if="activeSheet === 'settings'" class="space-y-5">
        <div class="space-y-2">
          <span class="text-label-medium block text-on-surface-variant">
            {{ t('settings.defaultReaderMode') }}
          </span>
          <div class="grid grid-cols-2 gap-2">
            <MdButton
              v-for="opt in modeOptions"
              :key="opt.value"
              :variant="readerMode === opt.value ? 'tonal' : 'outlined'"
              size="sm"
              :icon="opt.icon"
              @click="emit('update:readerMode', opt.value)"
            >
              {{ opt.label }}
            </MdButton>
          </div>
        </div>

        <div v-if="readerMode !== 'webtoon'" class="space-y-2">
          <span class="text-label-medium block text-on-surface-variant">
            {{ t('settings.defaultFitMode') }}
          </span>
          <MdSegmentedButton
            :model-value="fitMode"
            :options="fitOptions"
            size="sm"
            full-width
            :show-check="false"
            @update:model-value="emit('update:fitMode', $event)"
          />
        </div>

        <div class="space-y-2">
          <div class="flex items-center justify-between gap-2">
            <span class="text-label-medium text-on-surface-variant">
              {{ t('reader.zoomLabel') }}
            </span>
            <MdButton
              variant="text"
              size="sm"
              :disabled="zoomPercent === 100"
              @click="resetZoom"
            >
              {{ t('reader.zoomReset') }}
            </MdButton>
          </div>
          <div class="flex items-center justify-between gap-2 rounded-corner-md bg-surface-container-low px-3 py-1.5">
            <MdIconButton
              size="sm"
              variant="standard"
              :icon="LuZoomOut"
              :disabled="zoomPercent <= 50"
              :aria-label="t('reader.zoomOut')"
              @click="adjustZoom(-10)"
            />
            <button
              type="button"
              class="text-label-large cursor-pointer rounded-corner-full px-3 py-1 font-mono tabular-nums text-on-surface hover:bg-surface-container-high"
              :title="t('reader.zoomReset')"
              @click="resetZoom"
            >
              {{ zoomPercent }}%
            </button>
            <MdIconButton
              size="sm"
              variant="standard"
              :icon="LuZoomIn"
              :disabled="zoomPercent >= 200"
              :aria-label="t('reader.zoomIn')"
              @click="adjustZoom(10)"
            />
          </div>
        </div>

        <div v-if="readerMode === 'rtl_double' || readerMode === 'ltr_double'">
          <AppCheckbox
            :model-value="offsetCoverPage"
            :label="t('reader.offsetCover')"
            @update:model-value="emit('update:offsetCoverPage', $event)"
          />
        </div>

        <div v-if="readerMode === 'webtoon'" class="space-y-2">
          <div class="flex items-center justify-between gap-2">
            <span class="text-label-medium text-on-surface-variant">
              {{ t('reader.webtoonWidth') }}
            </span>
            <span class="text-label-medium font-mono tabular-nums text-on-surface">
              {{ webtoonWidth }} px
            </span>
          </div>
          <AppSlider
            :model-value="webtoonWidth"
            :min="520"
            :max="1400"
            :step="20"
            :aria-label="t('reader.webtoonWidth')"
            @update:model-value="emit('update:webtoonWidth', $event)"
            @change="emit('commit-webtoon-width', $event)"
          />
        </div>

        <div class="flex items-center justify-between gap-3 rounded-corner-md bg-surface-container-low px-3.5 py-2.5">
          <div class="flex items-center gap-2 text-body-medium text-on-surface">
            <LuEye class="h-4 w-4 text-primary" aria-hidden="true" />
            <span>{{ t('reader.eyeCareDim') }}</span>
          </div>
          <MdSwitch
            :model-value="dimImages"
            :aria-label="t('reader.dimToggleAria')"
            @update:model-value="emit('update:dimImages', $event)"
          />
        </div>

        <p class="text-body-small border-t border-outline-variant pt-3 text-on-surface-variant">
          {{ t('reader.shortcutsHint') }}
        </p>
      </div>

      <!-- Chapters & Bookmarks Sheet -->
      <div v-else-if="activeSheet === 'chapters'" ref="chaptersListRef" class="space-y-1">
        <template v-if="chaptersSheetTab === 'bookmarks'">
          <MdEmptyState
            v-if="sortedBookmarks.length === 0"
            compact
            :icon="LuBookmark"
            :title="t('reader.noChapterBookmarks')"
          />
          <div
            v-for="bm in sortedBookmarks"
            :key="bm.id"
            class="flex items-center gap-2 rounded-corner-md px-2 py-1 hover:bg-surface-container-high"
          >
            <button
              type="button"
              class="min-w-0 flex-1 cursor-pointer py-1.5 text-left outline-none"
              @click="jumpToPage(bm.page)"
            >
              <div class="text-title-small truncate text-on-surface">
                {{ t('reader.bookmarkPageLabel', { page: bm.page }) }}
              </div>
              <p v-if="bm.note || bm.title" class="text-body-small truncate text-on-surface-variant">
                {{ bm.note || bm.title }}
              </p>
            </button>
            <MdIconButton
              size="sm"
              variant="standard"
              :icon="LuTrash2"
              :aria-label="t('common.delete')"
              @click="emit('delete-bookmark', bm.id)"
            />
          </div>
        </template>

        <template v-else>
          <button
            v-for="ch in filteredChapters"
            :key="ch.key"
            type="button"
            :disabled="ch.disabled"
            :data-active-chapter="ch.key === chapterKey ? 'true' : undefined"
            class="md-state text-body-medium flex w-full cursor-pointer items-center justify-between gap-2 rounded-corner-md px-3 py-2.5 text-left transition-colors duration-150 disabled:cursor-default disabled:opacity-45"
            :class="
              ch.key === chapterKey
                ? 'bg-secondary-container text-on-secondary-container'
                : 'text-on-surface hover:bg-surface-container-high'
            "
            @click="handleSelectChapter(ch.key)"
          >
            <span class="truncate">{{ ch.label }}</span>
            <span
              v-if="ch.metaLabel"
              class="text-label-small shrink-0 text-on-surface-variant"
            >
              {{ ch.metaLabel }}
            </span>
          </button>
        </template>
      </div>
    </template>
  </ImmersiveMediaTemplate>
</template>

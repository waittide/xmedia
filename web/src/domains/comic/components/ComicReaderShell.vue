<script setup lang="ts">
import { computed, nextTick, onBeforeUnmount, onMounted, reactive, ref, watch } from 'vue'
import { useI18n as useGlobalI18n } from 'vue-i18n'
import { useComicI18n as useI18n } from '@/domains/comic/i18n'
import {
  LuAlertTriangle,
  LuChevronLeft,
  LuChevronRight,
  LuColumns2,
  LuEye,
  LuFileText,
  LuLayers,
  LuMaximize2,
  LuRotateCw,
  LuScrollText,
  LuSkipBack,
  LuSkipForward,
  LuSlidersHorizontal,
  LuZoomIn,
  LuZoomOut,
} from 'vue-icons-plus/lu'
import AppCheckbox from '@/components/ui/AppCheckbox.vue'
import AppSelect from '@/components/ui/AppSelect.vue'
import AppSlider from '@/components/ui/AppSlider.vue'
import MdSegmentedButton, { type SegmentedOption } from '@/components/ui/MdSegmentedButton.vue'
import { useSnackbarStore } from '@/stores/snackbar'
import { useThemeStore } from '@/stores/theme'
import type { ReaderFitMode, ReaderMode } from '@/domains/comic/types'

export interface ComicReaderChapterItem {
  key: string
  label: string
  metaLabel?: string
  disabled?: boolean
}

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
  back: []
  retry: []
}>()

const { t } = useI18n()
const { t: globalT } = useGlobalI18n()
const theme = useThemeStore()
const snackbar = useSnackbarStore()

const hudVisible = ref(true)
const settingsDrawerOpen = ref(false)
const chaptersDrawerOpen = ref(false)
const currentPage = ref(1)
const zoomPercent = ref(100)
const pendingNextChapterTap = ref(false)

const widePages = reactive<Record<number, boolean>>({})
const brokenPages = reactive<Record<number, boolean>>({})
const retryTokens = reactive<Record<number, number>>({})
const preloadedSet = new Set<string>()

let webtoonObserver: IntersectionObserver | null = null
const visibleWebtoonPages = new Set<number>()
let suppressWebtoonObserverUntil = 0
let unmounted = false

const webtoonContainerRef = ref<HTMLDivElement | null>(null)

const safeTotalPages = computed(() => Math.max(1, props.totalPages))

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

const isRtl = computed(() => props.readerMode === 'rtl_double')

const pageImageStyle = computed(() => {
  const scale = zoomPercent.value / 100
  if (props.fitMode === 'fit_height') {
    return {
      maxHeight: `${Math.round(100 * scale)}vh`,
      width: 'auto',
      maxWidth: currentSpread.value.length > 1 ? '50vw' : '100vw',
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
  return {
    transform: `scale(${scale})`,
    transformOrigin: 'top center',
  }
})

const eyeCareClass = computed(() =>
  props.dimImages && theme.isDark ? 'brightness-[0.85] contrast-[0.97]' : '',
)

const modeOptions = computed(() => [
  { value: 'rtl_double', label: t('reader.modeRtlDouble') },
  { value: 'ltr_single', label: t('reader.modeLtrSingle') },
  { value: 'ltr_double', label: t('reader.modeLtrDouble') },
  { value: 'webtoon', label: t('reader.modeWebtoon') },
])

const quickModeOptions = computed<SegmentedOption<ReaderMode>[]>(() => [
  { value: 'rtl_double', label: t('reader.modeRtlDouble'), icon: LuColumns2, title: t('reader.modeRtlDouble') },
  { value: 'ltr_single', label: t('reader.modeLtrSingle'), icon: LuFileText, title: t('reader.modeLtrSingle') },
  { value: 'ltr_double', label: t('reader.modeLtrDouble'), icon: LuColumns2, title: t('reader.modeLtrDouble') },
  { value: 'webtoon', label: t('reader.modeWebtoon'), icon: LuScrollText, title: t('reader.modeWebtoon') },
])

const fitOptions = computed(() => [
  { value: 'fit_height', label: t('reader.fitHeight') },
  { value: 'fit_width', label: t('reader.fitWidth') },
  { value: 'original', label: t('reader.fitOriginal') },
])

const currentFitLabel = computed(() => {
  if (props.fitMode === 'fit_width') return t('reader.fitWidth')
  if (props.fitMode === 'original') return t('reader.fitOriginal')
  return t('reader.fitHeight')
})

const fitQuickAria = computed(() =>
  t('reader.quickFitWidthAria', { mode: currentFitLabel.value }),
)

function cycleFitMode(): void {
  const order: ReaderFitMode[] = ['fit_height', 'fit_width', 'original']
  const idx = order.indexOf(props.fitMode)
  const next = order[(idx + 1) % order.length] ?? 'fit_height'
  emit('update:fitMode', next)
}

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
  for (const k of Object.keys(brokenPages)) delete brokenPages[Number(k)]
  for (const k of Object.keys(retryTokens)) delete retryTokens[Number(k)]
}

function onImageLoaded(pageNum: number, ev: Event): void {
  const img = ev.target as HTMLImageElement
  delete brokenPages[pageNum]
  if (img.naturalWidth > 0 && img.naturalHeight > 0) {
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

function advanceForward(): void {
  if (!props.hasContent || props.loading) return
  if (props.readerMode === 'webtoon') {
    webtoonContainerRef.value?.scrollBy({ top: window.innerHeight * 0.82, behavior: 'smooth' })
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
    webtoonContainerRef.value?.scrollBy({ top: -window.innerHeight * 0.82, behavior: 'smooth' })
    return
  }

  const prevIdx = currentSpreadIndex.value - 1
  if (prevIdx >= 0) {
    currentPage.value = spreads.value[prevIdx]![0]!
  } else if (props.hasPrevChapter) {
    emit('prev-chapter')
  }
}

function handleSelectChapter(key: string): void {
  chaptersDrawerOpen.value = false
  settingsDrawerOpen.value = false
  emit('select-chapter', key)
}

function handlePrevChapterClick(): void {
  if (!props.hasPrevChapter) return
  chaptersDrawerOpen.value = false
  settingsDrawerOpen.value = false
  emit('prev-chapter')
}

function handleNextChapterClick(): void {
  if (!props.hasNextChapter) return
  chaptersDrawerOpen.value = false
  settingsDrawerOpen.value = false
  emit('next-chapter')
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
      advanceForward()
    } else {
      stepBackward()
    }
  } else if (x > width * 0.68) {
    if (isRtl.value) {
      stepBackward()
    } else {
      advanceForward()
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
    } else {
      emit('back')
    }
    return
  }

  if (e.metaKey || e.ctrlKey || e.altKey) return
  if (settingsDrawerOpen.value || chaptersDrawerOpen.value) return
  if (target?.closest('[aria-modal="true"]')) return

  if (e.key === ' ') {
    e.preventDefault()
    advanceForward()
  } else if (e.key === 'ArrowLeft') {
    e.preventDefault()
    if (isRtl.value) {
      advanceForward()
    } else {
      stepBackward()
    }
  } else if (e.key === 'ArrowRight') {
    e.preventDefault()
    if (isRtl.value) {
      stepBackward()
    } else {
      advanceForward()
    }
  } else if (e.key === 'ArrowDown' && props.readerMode === 'webtoon') {
    webtoonContainerRef.value?.scrollBy({ top: 320, behavior: 'smooth' })
  } else if (e.key === 'ArrowUp' && props.readerMode === 'webtoon') {
    webtoonContainerRef.value?.scrollBy({ top: -320, behavior: 'smooth' })
  }
}

function onScrubberValue(targetPage: number): void {
  const clamped = Math.max(1, Math.min(safeTotalPages.value, Math.round(targetPage)))
  currentPage.value = clamped
  if (props.readerMode === 'webtoon') {
    scrollToWebtoonPage(clamped)
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

onMounted(() => {
  unmounted = false
  window.addEventListener('keydown', onKeydown)
})

onBeforeUnmount(() => {
  unmounted = true
  window.removeEventListener('keydown', onKeydown)
  teardownWebtoonObserver()
  preloadedSet.clear()
})

defineExpose({
  currentPage,
})
</script>

<template>
  <div
    class="relative flex h-screen w-screen flex-col overflow-hidden bg-[var(--md-sys-color-surface-dim)] text-[var(--md-sys-color-on-surface)] select-none"
  >
    <!-- Top Floating HUD Bar -->
    <header
      v-show="hudVisible"
      class="fixed top-0 right-0 left-0 z-[var(--z-base-overlay)] flex h-14 items-center justify-between gap-3 border-b border-[var(--md-sys-color-outline-variant)]/60 bg-[var(--md-sys-color-surface-container-high)]/90 px-4 text-[var(--md-sys-color-on-surface)] shadow-[var(--md-sys-elevation-2)] backdrop-blur-md"
    >
      <div class="flex min-w-0 items-center gap-3">
        <button
          type="button"
          :title="backLabel"
          :aria-label="backLabel"
          class="md-btn md-btn-sm md-btn-tonal md-state shrink-0"
          @click="emit('back')"
        >
          <LuChevronLeft class="h-4 w-4" />
          <span class="hidden sm:inline">{{ backLabel }}</span>
        </button>

        <div v-if="title || subtitle" class="min-w-0">
          <div class="flex items-center gap-2">
            <span class="truncate text-sm font-bold text-[var(--md-sys-color-on-surface)]">
              {{ title }}
            </span>
            <slot name="header-meta" />
          </div>
          <div
            v-if="subtitle"
            class="truncate text-[11px] text-[var(--md-sys-color-on-surface-variant)]"
          >
            {{ subtitle }}
          </div>
        </div>
      </div>

      <!-- Quick Mode Switcher + Controls -->
      <div class="flex shrink-0 items-center gap-1.5">
        <div v-if="hasContent" class="hidden md:flex">
          <MdSegmentedButton
            :model-value="readerMode"
            :options="quickModeOptions"
            size="sm"
            @update:model-value="emit('update:readerMode', $event)"
          />
        </div>

        <!-- Quick Fit Toggle (directly visible in HUD) -->
        <button
          v-if="hasContent"
          type="button"
          :disabled="readerMode === 'webtoon'"
          :title="fitQuickAria"
          :aria-label="fitQuickAria"
          class="md-icon-btn md-icon-btn-sm md-state"
          :class="
            fitMode === 'fit_width'
              ? '!bg-[var(--md-sys-color-primary-container)] !text-[var(--md-sys-color-on-primary-container)]'
              : ''
          "
          @click="cycleFitMode"
        >
          <LuMaximize2 class="h-4 w-4" />
        </button>

        <!-- Zoom Controls -->
        <button
          v-if="hasContent"
          type="button"
          :title="t('reader.zoomOut')"
          :aria-label="t('reader.zoomOut')"
          class="md-icon-btn md-icon-btn-sm md-state hidden sm:inline-flex"
          @click="zoomPercent = Math.max(50, zoomPercent - 10)"
        >
          <LuZoomOut class="h-4 w-4" />
        </button>
        <span
          v-if="hasContent"
          class="hidden w-11 text-center font-mono text-xs text-[var(--md-sys-color-on-surface-variant)] sm:inline"
        >
          {{ zoomPercent }}%
        </span>
        <button
          v-if="hasContent"
          type="button"
          :title="t('reader.zoomIn')"
          :aria-label="t('reader.zoomIn')"
          class="md-icon-btn md-icon-btn-sm md-state hidden sm:inline-flex"
          @click="zoomPercent = Math.min(200, zoomPercent + 10)"
        >
          <LuZoomIn class="h-4 w-4" />
        </button>

        <!-- Quick Eye-Care Dim Button (directly visible in HUD) -->
        <button
          v-if="hasContent"
          type="button"
          :title="t('reader.dimToggleAria')"
          :aria-label="t('reader.dimToggleAria')"
          :aria-pressed="dimImages"
          class="md-icon-btn md-icon-btn-sm md-state"
          :class="
            dimImages
              ? '!bg-[var(--md-sys-color-warning-container)] !text-[var(--md-sys-color-on-warning-container)]'
              : ''
          "
          @click="emit('update:dimImages', !dimImages)"
        >
          <LuEye class="h-4 w-4" />
        </button>

        <!-- Domain-specific actions (e.g. local page bookmark) -->
        <slot name="toolbar-actions" />

        <!-- Chapter List Popover Button -->
        <button
          v-if="hasContent && chapters.length > 0"
          type="button"
          :title="t('reader.chapterList')"
          :aria-label="t('reader.chapterList')"
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
          v-if="hasContent"
          type="button"
          :title="t('reader.settings')"
          :aria-label="t('reader.settings')"
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
      v-if="settingsDrawerOpen && hasContent"
      class="fixed top-16 right-4 z-[var(--z-sheet)] w-80 max-w-[calc(100vw-2rem)] space-y-4 rounded-[24px] border border-[var(--md-sys-color-outline-variant)]/60 bg-[var(--md-sys-color-surface-container-high)] p-5 text-[var(--md-sys-color-on-surface)] shadow-[var(--md-sys-elevation-3)]"
    >
      <div>
        <label class="mb-1 block text-xs font-semibold text-[var(--md-sys-color-on-surface)]">
          {{ t('settings.defaultReaderMode') }}
        </label>
        <AppSelect
          size="sm"
          :model-value="readerMode"
          :options="modeOptions"
          @update:model-value="emit('update:readerMode', $event as ReaderMode)"
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
          @update:model-value="emit('update:fitMode', $event as ReaderFitMode)"
        />
      </div>

      <div v-if="readerMode === 'rtl_double' || readerMode === 'ltr_double'">
        <AppCheckbox
          :model-value="offsetCoverPage"
          :label="t('reader.offsetCover')"
          @update:model-value="emit('update:offsetCoverPage', $event)"
        />
      </div>

      <div v-if="readerMode === 'webtoon'">
        <div class="mb-1 flex items-center justify-between text-xs">
          <span class="font-semibold text-[var(--md-sys-color-on-surface)]">
            {{ t('reader.webtoonWidth') }}
          </span>
          <span class="font-mono tabular-nums text-[var(--md-sys-color-on-surface-variant)]">
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

      <div>
        <AppCheckbox
          :model-value="dimImages"
          :label="t('reader.eyeCareDim')"
          @update:model-value="emit('update:dimImages', $event)"
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
      v-if="chaptersDrawerOpen && hasContent && chapters.length > 0"
      class="fixed top-16 right-14 z-[var(--z-sheet)] max-h-[70vh] w-72 overflow-y-auto rounded-[24px] border border-[var(--md-sys-color-outline-variant)]/60 bg-[var(--md-sys-color-surface-container-high)] p-2 text-[var(--md-sys-color-on-surface)] shadow-[var(--md-sys-elevation-3)]"
    >
      <button
        v-for="ch in chapters"
        :key="ch.key"
        type="button"
        :disabled="ch.disabled"
        class="md-state flex w-full items-center justify-between gap-2 rounded-xl px-3 py-2 text-left text-xs transition disabled:opacity-50"
        :class="
          ch.key === chapterKey
            ? 'bg-[var(--md-sys-color-secondary-container)] font-semibold text-[var(--md-sys-color-on-secondary-container)]'
            : 'text-[var(--md-sys-color-on-surface)] hover:bg-[var(--md-sys-color-surface-container-highest)]'
        "
        @click="handleSelectChapter(ch.key)"
      >
        <span class="truncate">{{ ch.label }}</span>
        <span
          v-if="ch.metaLabel"
          class="shrink-0 text-[10px] text-[var(--md-sys-color-on-surface-variant)]"
        >
          {{ ch.metaLabel }}
        </span>
      </button>
    </div>

    <!-- Loading State -->
    <div
      v-if="loading"
      class="flex flex-1 flex-col items-center justify-center gap-3 px-6 pt-14 text-center text-xs text-[var(--md-sys-color-on-surface-variant)]"
    >
      <div
        class="h-7 w-7 animate-spin rounded-full border-2 border-[var(--md-sys-color-primary)] border-t-transparent"
      />
      <span>{{ t('online.loadingPages') }}</span>
    </div>

    <!-- Error / Empty State -->
    <div
      v-else-if="loadError || !hasContent"
      class="flex flex-1 flex-col items-center justify-center gap-3 px-6 pt-14 text-center"
    >
      <p class="max-w-md text-sm text-[var(--md-sys-color-error)]">
        {{ loadError || t('online.emptyPages') }}
      </p>
      <div class="flex flex-wrap items-center justify-center gap-2">
        <button
          type="button"
          class="md-btn md-btn-tonal md-state"
          @click="emit('retry')"
        >
          {{ globalT('common.refresh') }}
        </button>
        <button
          type="button"
          class="md-btn md-btn-outlined md-state"
          @click="emit('back')"
        >
          {{ backLabel }}
        </button>
        <slot name="error-actions" />
      </div>
    </div>

    <!-- Main Comic Stage -->
    <div
      v-else
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
          <div
            v-for="p in totalPages"
            :key="chapterKey + '-wt-' + p"
            :data-page="p"
            class="relative w-full"
          >
            <img
              v-if="pageSrc(p)"
              :src="pageSrc(p)"
              :alt="'Page ' + p"
              :loading="p <= 4 ? 'eager' : 'lazy'"
              class="m-0 block min-h-[40vh] w-full bg-[var(--md-sys-color-surface-container-low)]/40 p-0"
              :class="eyeCareClass"
              @load="onImageLoaded(p, $event)"
              @error="onImageError(p)"
            />
            <div
              v-if="!pageSrc(p) || brokenPages[p]"
              class="flex min-h-[160px] w-full flex-col items-center justify-center gap-2 bg-[var(--md-sys-color-surface-container-low)] p-6 text-center text-xs text-[var(--md-sys-color-on-surface-variant)]"
              :class="pageSrc(p) ? 'absolute inset-0 bg-[var(--md-sys-color-surface-container-low)]/90' : ''"
              @click.stop
            >
              <LuAlertTriangle class="h-6 w-6 text-[var(--md-sys-color-error)]" />
              <span>{{ t('online.imageLoadError', { page: p }) }}</span>
              <button
                v-if="pageSrc(p)"
                type="button"
                class="md-btn md-btn-sm md-btn-tonal md-state"
                @click="retryPageImage(p)"
              >
                <LuRotateCw class="h-3.5 w-3.5" />
                <span>{{ t('online.retryImage') }}</span>
              </button>
            </div>
          </div>

          <!-- Seamless Next Chapter Card at Bottom of Webtoon Strip -->
          <div class="my-12 flex w-full flex-col items-center gap-3 px-6 text-center" @click.stop>
            <button
              v-if="hasNextChapter"
              type="button"
              :aria-label="t('reader.nextChapterAria')"
              class="md-btn md-btn-filled md-state !h-11 !px-7 text-sm shadow-[var(--md-sys-elevation-2)]"
              @click="handleNextChapterClick"
            >
              <span>
                {{ t('reader.nextChapter')
                }}<template v-if="nextChapterToastLabel">: {{ nextChapterToastLabel }}</template>
              </span>
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
        class="flex h-full w-full overflow-auto"
      >
        <div
          class="m-auto flex items-center justify-center gap-0"
          :class="isRtl ? 'flex-row-reverse' : 'flex-row'"
        >
          <div
            v-for="p in currentSpread"
            :key="chapterKey + '-sp-' + p"
            class="relative flex items-center justify-center"
          >
            <img
              v-if="pageSrc(p)"
              :src="pageSrc(p)"
              :alt="'Page ' + p"
              :style="pageImageStyle"
              class="m-0 block object-contain p-0 select-none"
              :class="eyeCareClass"
              @load="onImageLoaded(p, $event)"
              @error="onImageError(p)"
            />
            <div
              v-if="!pageSrc(p) || brokenPages[p]"
              class="flex min-h-[240px] min-w-[200px] flex-col items-center justify-center gap-2 rounded-xl bg-[var(--md-sys-color-surface-container-low)]/95 p-6 text-center text-xs text-[var(--md-sys-color-on-surface-variant)]"
              :class="pageSrc(p) ? 'absolute inset-0' : ''"
              @click.stop
            >
              <LuAlertTriangle class="h-6 w-6 text-[var(--md-sys-color-error)]" />
              <span>{{ t('online.imageLoadError', { page: p }) }}</span>
              <button
                v-if="pageSrc(p)"
                type="button"
                class="md-btn md-btn-sm md-btn-tonal md-state"
                @click="retryPageImage(p)"
              >
                <LuRotateCw class="h-3.5 w-3.5" />
                <span>{{ t('online.retryImage') }}</span>
              </button>
            </div>
          </div>
        </div>
      </div>
    </div>

    <!-- Bottom Floating Page Scrubber HUD -->
    <footer
      v-if="hasContent && !loading && !loadError"
      v-show="hudVisible"
      class="fixed right-0 bottom-0 left-0 z-[var(--z-base-overlay)] flex h-14 items-center gap-4 border-t border-[var(--md-sys-color-outline-variant)]/60 bg-[var(--md-sys-color-surface-container-high)]/90 px-4 text-[var(--md-sys-color-on-surface)] shadow-[var(--md-sys-elevation-2)] backdrop-blur-md"
    >
      <button
        type="button"
        :disabled="!hasPrevChapter"
        :title="hasPrevChapter ? t('reader.prevChapter') : t('reader.noPrevChapter')"
        :aria-label="hasPrevChapter ? t('reader.prevChapterAria') : t('reader.noPrevChapter')"
        class="md-btn md-btn-sm md-btn-tonal md-state"
        @click="handlePrevChapterClick"
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

        <div class="min-w-0 flex-1">
          <AppSlider
            :model-value="currentPage"
            :min="1"
            :max="safeTotalPages"
            :step="1"
            :rtl="isRtl"
            :aria-label="t('reader.page')"
            @update:model-value="onScrubberValue"
          />
        </div>
      </div>

      <button
        type="button"
        :disabled="!hasNextChapter"
        :title="hasNextChapter ? t('reader.nextChapter') : t('reader.noNextChapter')"
        :aria-label="hasNextChapter ? t('reader.nextChapterAria') : t('reader.noNextChapter')"
        class="md-btn md-btn-sm md-btn-tonal md-state"
        @click="handleNextChapterClick"
      >
        <span class="hidden sm:inline">{{ t('reader.nextChapter') }}</span>
        <LuSkipForward class="h-3.5 w-3.5" />
      </button>
    </footer>
  </div>
</template>

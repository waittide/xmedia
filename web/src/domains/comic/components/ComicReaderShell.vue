<script setup lang="ts">
import { computed, onBeforeUnmount, onMounted, ref, watch } from 'vue'
import {
  LuLayers,
  LuMaximize,
  LuMinimize,
  LuSkipBack,
  LuSkipForward,
  LuSlidersHorizontal,
} from 'vue-icons-plus/lu'
import { ImmersiveMediaTemplate } from '@/components/domain'
import {
  AppSlider,
  MdButton,
  MdIconButton,
  MdTabs,
  MdTextField,
  type MdTabItem,
} from '@/components/ui'
import ComicReaderChaptersSheet from '@/domains/comic/components/reader/ComicReaderChaptersSheet.vue'
import ComicReaderSettingsSheet from '@/domains/comic/components/reader/ComicReaderSettingsSheet.vue'
import ComicReaderStage from '@/domains/comic/components/reader/ComicReaderStage.vue'
import type {
  ChaptersSheetTab,
  ComicReaderBookmarkItem,
  ComicReaderChapterItem,
  ReaderSheetId,
} from '@/domains/comic/components/reader/types'
import {
  preferredComicScrollBehavior,
  useComicStageController,
} from '@/domains/comic/composables/useComicStageController'
import { useComicI18n as useI18n } from '@/domains/comic/i18n'
import type { ReaderFitMode, ReaderMode } from '@/domains/comic/types'
import { useSnackbarStore } from '@/stores/snackbar'

export type { ComicReaderBookmarkItem, ComicReaderChapterItem }

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
const snackbar = useSnackbarStore()

const immersiveRef = ref<InstanceType<typeof ImmersiveMediaTemplate> | null>(null)
const stageRef = ref<InstanceType<typeof ComicReaderStage> | null>(null)

const activeSheet = ref<ReaderSheetId | null>(null)
const chaptersSheetTab = ref<ChaptersSheetTab>('chapters')
const chapterFilter = ref('')

function getWebtoonContainer(): HTMLDivElement | null {
  return stageRef.value?.getWebtoonContainer() ?? null
}

const {
  currentPage,
  zoomPercent,
  pendingNextChapterTap,
  isFullscreen,
  brokenPages,
  safeTotalPages,
  isSinglePage,
  isRtl,
  spreads,
  currentSpreadIndex,
  currentSpread,
  pageCounterText,
  eyeCareClass,
  pageImageStyle,
  pageSrc,
  onImageLoaded,
  onImageError,
  retryPageImage,
  emitPageChange,
  scrollToWebtoonPage,
  onWebtoonScroll,
  adjustZoom,
  resetZoom,
  toggleFullscreen,
} = useComicStageController({
  getChapterKey: () => props.chapterKey,
  getTotalPages: () => props.totalPages,
  getInitialPage: () => props.initialPage,
  getReaderMode: () => props.readerMode,
  getFitMode: () => props.fitMode,
  getOffsetCoverPage: () => props.offsetCoverPage,
  getDimImages: () => props.dimImages,
  getPreloadPages: () => props.preloadPages,
  getLoading: () => props.loading,
  getHasContent: () => props.hasContent,
  getPageSrc: (p) => props.getPageSrc(p),
  getWebtoonContainer,
  onHideHud: () => immersiveRef.value?.hideHud(),
  onShowOsd: (text) => immersiveRef.value?.showOsd(text),
  canAutoHideHud: () => activeSheet.value === null,
  onPageChange: (payload) => emit('page-change', payload),
})

const sheetTitle = computed(() => {
  if (activeSheet.value === 'settings') return t('reader.settings')
  if (activeSheet.value === 'chapters') return t('reader.chapterList')
  return ''
})

const sheetSubtitle = computed(() => (activeSheet.value === 'chapters' ? props.title : ''))

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

const hasChaptersButton = computed(
  () => props.hasContent && (props.chapters.length > 0 || props.showBookmarksTab),
)

watch(activeSheet, (sheet) => {
  if (sheet === 'chapters') {
    chapterFilter.value = ''
  }
})

function handleSheetModelUpdate(next: string | null): void {
  activeSheet.value = next === 'chapters' || next === 'settings' ? next : null
}

function handleChaptersTabUpdate(next: string): void {
  chaptersSheetTab.value = next === 'bookmarks' ? 'bookmarks' : 'chapters'
}

function toggleSheet(id: ReaderSheetId): void {
  activeSheet.value = activeSheet.value === id ? null : id
}

function advanceForward(): void {
  if (!props.hasContent || props.loading) return
  if (props.readerMode === 'webtoon') {
    getWebtoonContainer()?.scrollBy({
      top: window.innerHeight * 0.82,
      behavior: preferredComicScrollBehavior(),
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
    getWebtoonContainer()?.scrollBy({
      top: -window.innerHeight * 0.82,
      behavior: preferredComicScrollBehavior(),
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

function onKeydown(e: KeyboardEvent): void {
  const target = e.target
  if (
    target instanceof HTMLElement &&
    (target.tagName === 'INPUT' ||
      target.tagName === 'TEXTAREA' ||
      target.tagName === 'SELECT' ||
      target.isContentEditable)
  ) {
    return
  }

  if (e.key === 'Escape') return
  if (e.metaKey || e.ctrlKey || e.altKey) return
  if (activeSheet.value !== null) return
  if (target instanceof HTMLElement && target.closest('[aria-modal="true"]')) return

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
    getWebtoonContainer()?.scrollBy({ top: 320, behavior: preferredComicScrollBehavior() })
  } else if (e.key === 'ArrowUp' && props.readerMode === 'webtoon') {
    getWebtoonContainer()?.scrollBy({ top: -320, behavior: preferredComicScrollBehavior() })
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
  window.addEventListener('keydown', onKeydown)
})

onBeforeUnmount(() => {
  window.removeEventListener('keydown', onKeydown)
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
    @update:active-sheet="handleSheetModelUpdate"
    @back="emit('back')"
    @tap-prev="handleTapLeft"
    @tap-next="handleTapRight"
  >
    <template #header-meta>
      <slot name="header-meta" />
    </template>

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

    <ComicReaderStage
      ref="stageRef"
      :loading="loading"
      :load-error="loadError"
      :has-content="hasContent"
      :back-label="backLabel"
      :reader-mode="readerMode"
      :chapter-key="chapterKey"
      :total-pages="totalPages"
      :webtoon-width="webtoonWidth"
      :zoom-percent="zoomPercent"
      :is-rtl="isRtl"
      :current-spread="currentSpread"
      :eye-care-class="eyeCareClass"
      :broken-pages="brokenPages"
      :has-next-chapter="hasNextChapter"
      :next-chapter-toast-label="nextChapterToastLabel"
      :get-display-page-src="pageSrc"
      :get-page-image-style="pageImageStyle"
      @retry="emit('retry')"
      @back="emit('back')"
      @webtoon-scroll="onWebtoonScroll"
      @image-loaded="onImageLoaded"
      @image-error="onImageError"
      @retry-page="retryPageImage"
      @next-chapter="handleNextChapterClick"
    >
      <template #error-actions>
        <slot name="error-actions" />
      </template>
    </ComicReaderStage>

    <template v-if="hasContent && !loading && !loadError" #footer>
      <div
        class="flex items-center gap-3"
        :class="isRtl ? 'flex-row-reverse' : 'flex-row'"
      >
        <MdButton
          variant="tonal"
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

    <template v-if="activeSheet === 'chapters'" #sheet-subheader>
      <div class="space-y-2.5 pb-2">
        <MdTabs
          v-if="showBookmarksTab || bookmarks.length > 0"
          :model-value="chaptersSheetTab"
          :tabs="chaptersSheetTabs"
          variant="secondary"
          @update:model-value="handleChaptersTabUpdate"
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

    <template #sheet>
      <ComicReaderSettingsSheet
        v-if="activeSheet === 'settings'"
        :reader-mode="readerMode"
        :fit-mode="fitMode"
        :zoom-percent="zoomPercent"
        :offset-cover-page="offsetCoverPage"
        :webtoon-width="webtoonWidth"
        :dim-images="dimImages"
        @update:reader-mode="emit('update:readerMode', $event)"
        @update:fit-mode="emit('update:fitMode', $event)"
        @adjust-zoom="adjustZoom"
        @reset-zoom="resetZoom"
        @update:offset-cover-page="emit('update:offsetCoverPage', $event)"
        @update:webtoon-width="emit('update:webtoonWidth', $event)"
        @commit-webtoon-width="emit('commit-webtoon-width', $event)"
        @update:dim-images="emit('update:dimImages', $event)"
      />

      <ComicReaderChaptersSheet
        v-else-if="activeSheet === 'chapters'"
        :tab="chaptersSheetTab"
        :chapter-key="chapterKey"
        :chapters="chapters"
        :bookmarks="bookmarks"
        :filter-query="chapterFilter"
        @select-chapter="handleSelectChapter"
        @jump-to-page="jumpToPage"
        @delete-bookmark="emit('delete-bookmark', $event)"
      />
    </template>
  </ImmersiveMediaTemplate>
</template>

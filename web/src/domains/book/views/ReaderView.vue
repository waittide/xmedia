<script setup lang="ts">
import { computed, defineAsyncComponent, onBeforeUnmount, onMounted, ref, watch } from 'vue'
import { useRoute, useRouter } from 'vue-router'
import {
  LuBookmark,
  LuChevronLeft,
  LuChevronRight,
  LuList,
  LuMaximize2,
  LuMoveHorizontal,
  LuRepeat,
  LuSlidersHorizontal,
  LuZoomIn,
  LuZoomOut,
} from 'vue-icons-plus/lu'
import {
  ImmersiveMediaTemplate,
  MediaErrorState,
  type ImmersiveTapMode,
} from '@/components/domain'
import {
  MdButton,
  MdCircularProgress,
  MdIconButton,
  MdSegmentedButton,
  MdTextField,
  type SegmentedOption,
} from '@/components/ui'
import { api } from '@/domains/book/api'
import BookSourceSwitchDialog from '@/domains/book/components/BookSourceSwitchDialog.vue'
import BookBookmarksSheet from '@/domains/book/components/reader/BookBookmarksSheet.vue'
import BookTocDrawer from '@/domains/book/components/reader/BookTocDrawer.vue'
import BookTypographySheet from '@/domains/book/components/reader/BookTypographySheet.vue'
import { useBookI18n as useI18n } from '@/domains/book/i18n'
import { onlineBookApi, type AggregatedSearchItemDto } from '@/domains/book/onlineApi'
import type { PdfFitMode, PdfReaderState } from '@/domains/book/readers/PdfReader.vue'
import {
  isTextChapterBookFormat,
  type Bookmark,
  type BookSummaryDto,
  type ReadingLocation,
  type TocItem,
} from '@/domains/book/types'
import { formatProgressPercentValue } from '@/domains/book/utils/bookVm'
import { sendKeepalivePutJson } from '@/domains/shared/keepalive'
import { itemRoute, libraryRoute } from '@/sources'
import { useAuthStore } from '@/stores/auth'
import { useSnackbarStore } from '@/stores/snackbar'
import { usePreferencesStore } from '@/stores/preferences'

const EpubReader = defineAsyncComponent(() => import('@/domains/book/readers/EpubReader.vue'))
const TxtReader = defineAsyncComponent(() => import('@/domains/book/readers/TxtReader.vue'))
const PdfReader = defineAsyncComponent(() => import('@/domains/book/readers/PdfReader.vue'))
const TrackedOnlineReader = defineAsyncComponent(
  () => import('@/domains/book/readers/TrackedOnlineReader.vue'),
)

interface BasicReaderHandle {
  navigateTo: (target: string, scrollRatio?: number) => Promise<void>
}

interface PagedReaderHandle extends BasicReaderHandle {
  prevPage: () => Promise<void>
  nextPage: () => Promise<void>
}

interface PdfReaderHandle extends PagedReaderHandle {
  goToPage: (page: number) => Promise<void>
  zoomIn: () => Promise<void>
  zoomOut: () => Promise<void>
  applyFitMode: (mode: 'width' | 'page') => Promise<void>
}

interface TrackedReaderHandle extends BasicReaderHandle {
  reload: () => Promise<void>
}

type ReaderSheet = 'toc' | 'bookmarks' | 'typography' | null

const { t } = useI18n()
const route = useRoute()
const router = useRouter()
const authStore = useAuthStore()
const preferencesStore = usePreferencesStore()
const snackbarStore = useSnackbarStore()

const bookId = computed(() => String(route.params.id ?? ''))
const book = ref<BookSummaryDto | null>(null)
const toc = ref<TocItem[]>([])
const bookmarks = ref<Bookmark[]>([])
const initialLocation = ref<ReadingLocation | null>(null)
const currentLocation = ref<ReadingLocation | null>(null)
const loading = ref(true)
const loadError = ref<string | null>(null)

const hudVisible = ref(true)
const activeSheet = ref<ReaderSheet>(null)
const switchSourceOpen = ref(false)
const switchingSourceKey = ref<string | null>(null)

const pdfState = ref<PdfReaderState>({
  currentPage: 1,
  totalPages: 1,
  scale: 1,
  fitMode: 'width',
})
const pdfPageInput = ref('1')

const immersiveRef = ref<InstanceType<typeof ImmersiveMediaTemplate> | null>(null)
const epubRef = ref<PagedReaderHandle | null>(null)
const txtRef = ref<BasicReaderHandle | null>(null)
const pdfRef = ref<PdfReaderHandle | null>(null)
const trackedRef = ref<TrackedReaderHandle | null>(null)

const isPdf = computed(() => Boolean(book.value && !book.value.online_tracking && book.value.format === 'pdf'))
const isPaginatedEpub = computed(
  () =>
    Boolean(book.value && !book.value.online_tracking && book.value.format === 'epub') &&
    preferencesStore.reader.flow === 'paginated',
)

const centerTapMode = computed<ImmersiveTapMode>(() =>
  isPdf.value || isPaginatedEpub.value ? 'center-zone' : 'full-stage',
)

const progressSubtitle = computed(() => {
  if (!currentLocation.value) return ''
  const pctText = `${formatProgressPercentValue(currentLocation.value.progress_percent)}%`
  if (isPdf.value && pdfState.value.totalPages > 0) {
    return `${pdfState.value.currentPage} / ${pdfState.value.totalPages} · ${pctText}`
  }
  return pctText
})

const sheetTitle = computed(() => {
  if (activeSheet.value === 'toc') return `${t('reader.toc')} (${toc.value.length})`
  if (activeSheet.value === 'bookmarks') return `${t('reader.bookmarks')} (${bookmarks.value.length})`
  if (activeSheet.value === 'typography') return t('reader.typography')
  return ''
})

const sheetSubtitle = computed(() =>
  activeSheet.value === 'typography' ? t('reader.typographySubtitle') : '',
)

const pdfFitOptions = computed<SegmentedOption<'width' | 'page'>[]>(() => [
  { value: 'width', label: t('reader.fitWidth'), icon: LuMoveHorizontal },
  { value: 'page', label: t('reader.fitPage'), icon: LuMaximize2 },
])

interface PendingProgressPayload {
  progress_percent: number
  locator: string
  chapter_index?: number | null
  page_number?: number | null
  scroll_ratio?: number | null
}

let saveTimer: ReturnType<typeof setTimeout> | null = null
let pendingProgress: { bookId: string; payload: PendingProgressPayload } | null = null
let disposed = false
let loadSeq = 0
let lastSaveErrorToastAt = 0

function reportProgressSaveError(err: unknown): void {
  const now = Date.now()
  if (now - lastSaveErrorToastAt < 8000) return
  lastSaveErrorToastAt = now
  snackbarStore.show({
    message: err instanceof Error ? err.message : String(err),
    tone: 'error',
  })
}

function parseFiniteQueryNumber(raw: unknown): number | null {
  if (typeof raw !== 'string' || !raw.trim()) return null
  const num = Number(raw)
  return Number.isFinite(num) ? num : null
}

async function flushPendingProgress(useKeepalive = false): Promise<void> {
  if (saveTimer) {
    clearTimeout(saveTimer)
    saveTimer = null
  }
  if (!pendingProgress) return
  const { bookId: targetBookId, payload } = pendingProgress
  pendingProgress = null
  if (!targetBookId) return
  if (useKeepalive) {
    sendKeepalivePutJson(`/api/v1/book/books/${encodeURIComponent(targetBookId)}/progress`, payload)
    return
  }
  await api.saveProgress(targetBookId, payload).catch((err) => {
    reportProgressSaveError(err)
  })
}

function handlePageHide(): void {
  void flushPendingProgress(true)
}

async function loadBook(id: string): Promise<void> {
  const seq = ++loadSeq
  loading.value = true
  loadError.value = null
  book.value = null
  toc.value = []
  bookmarks.value = []
  initialLocation.value = null
  currentLocation.value = null
  activeSheet.value = null

  if (!id) {
    loadError.value = t('detail.notFound')
    loading.value = false
    return
  }

  try {
    const [b, progress, bms] = await Promise.all([
      api.getBook(id),
      api.getProgress(id),
      api.listBookmarks(id),
    ])
    if (disposed || seq !== loadSeq) return

    book.value = b
    bookmarks.value = bms
    if (!b.online_tracking) {
      const loadedToc = await api.getToc(id).catch(() => [])
      if (disposed || seq !== loadSeq) return
      toc.value = loadedToc
    }

    const queryLocator = typeof route.query.locator === 'string' ? route.query.locator : null
    if (queryLocator) {
      initialLocation.value = {
        progress_percent: progress?.location.progress_percent ?? 0,
        locator: queryLocator,
        chapter_index: parseFiniteQueryNumber(route.query.chapter),
        page_number: parseFiniteQueryNumber(route.query.page),
        scroll_ratio: parseFiniteQueryNumber(route.query.ratio),
      }
    } else {
      initialLocation.value = progress?.location ?? b.progress ?? null
    }
    currentLocation.value = initialLocation.value
  } catch (err) {
    if (disposed || seq !== loadSeq) return
    loadError.value = err instanceof Error ? err.message : String(err)
  } finally {
    if (!disposed && seq === loadSeq) {
      loading.value = false
    }
  }
}

function toggleSheet(sheet: Exclude<ReaderSheet, null>): void {
  activeSheet.value = activeSheet.value === sheet ? null : sheet
}

function handleReaderKeydown(ev: KeyboardEvent): void {
  const target = ev.target
  const isTyping =
    target instanceof HTMLElement &&
    (target.tagName === 'INPUT' ||
      target.tagName === 'TEXTAREA' ||
      target.tagName === 'SELECT' ||
      target.isContentEditable)

  if (isTyping || ev.metaKey || ev.ctrlKey || ev.altKey) return
  if (document.querySelector('[aria-modal="true"]')) return
  if (ev.key.toLowerCase() === 'b') {
    ev.preventDefault()
    toggleSheet('bookmarks')
  } else if (ev.key.toLowerCase() === 't') {
    ev.preventDefault()
    toggleSheet('toc')
  }
}

async function exitReader(): Promise<void> {
  await flushPendingProgress()
  const currentId = bookId.value
  const prev = (window.history.state as { back?: string | null } | null)?.back
  if (
    typeof prev === 'string' &&
    (prev.startsWith(`/book/item/${currentId}`) || prev.startsWith(`/book/items/${currentId}`))
  ) {
    router.back()
  } else if (currentId) {
    await router.replace(itemRoute({ scope: 'local', domain: 'book', id: currentId }))
  } else {
    await router.replace(libraryRoute('book'))
  }
}

function handleTapPrev(): void {
  if (isPdf.value) {
    void pdfRef.value?.prevPage()
  } else if (isPaginatedEpub.value) {
    void epubRef.value?.prevPage()
  }
}

function handleTapNext(): void {
  if (isPdf.value) {
    void pdfRef.value?.nextPage()
  } else if (isPaginatedEpub.value) {
    void epubRef.value?.nextPage()
  }
}

function handlePdfStateChange(state: PdfReaderState): void {
  pdfState.value = state
  pdfPageInput.value = String(state.currentPage)
}

function handlePdfPageSubmit(): void {
  const parsed = Number.parseInt(pdfPageInput.value, 10)
  if (!Number.isFinite(parsed) || parsed < 1) {
    pdfPageInput.value = String(pdfState.value.currentPage)
    return
  }
  void pdfRef.value?.goToPage(parsed)
}

function handlePdfFitChange(mode: PdfFitMode): void {
  if (mode === 'width' || mode === 'page') {
    void pdfRef.value?.applyFitMode(mode)
  }
}

function handleProgressUpdate(payload: PendingProgressPayload): void {
  const currentId = bookId.value
  if (!currentId) return
  currentLocation.value = {
    progress_percent: payload.progress_percent,
    locator: payload.locator,
    chapter_index: payload.chapter_index ?? null,
    page_number: payload.page_number ?? null,
    scroll_ratio: payload.scroll_ratio ?? null,
  }

  pendingProgress = { bookId: currentId, payload }
  if (saveTimer) clearTimeout(saveTimer)
  saveTimer = setTimeout(() => {
    saveTimer = null
    const toSave = pendingProgress
    pendingProgress = null
    if (toSave) {
      void api.saveProgress(toSave.bookId, toSave.payload).catch((err) => {
        reportProgressSaveError(err)
      })
    }
  }, 600)
}

async function jumpToTarget(hrefOrLocator: string, scrollRatio = 0): Promise<void> {
  activeSheet.value = null
  if (book.value?.online_tracking) {
    await trackedRef.value?.navigateTo(hrefOrLocator, scrollRatio)
  } else if (book.value?.format === 'epub') {
    await epubRef.value?.navigateTo(hrefOrLocator)
  } else if (book.value && isTextChapterBookFormat(book.value.format)) {
    await txtRef.value?.navigateTo(hrefOrLocator, scrollRatio)
  } else if (book.value?.format === 'pdf') {
    await pdfRef.value?.navigateTo(hrefOrLocator)
  }
}

async function handleAddBookmark(label: string): Promise<void> {
  if (!currentLocation.value || !bookId.value) return
  const loc = currentLocation.value
  const defaultTitle = `${formatProgressPercentValue(loc.progress_percent)}% · ${book.value?.metadata.title ?? ''}`
  try {
    const created = await api.createBookmark(bookId.value, {
      title: label.trim() || defaultTitle,
      progress_percent: loc.progress_percent,
      locator: loc.locator,
      chapter_index: loc.chapter_index,
      page_number: loc.page_number,
      scroll_ratio: loc.scroll_ratio,
    })
    bookmarks.value = [created, ...bookmarks.value]
    snackbarStore.show(t('reader.bookmarkAdded'))
  } catch (err) {
    snackbarStore.show({
      message: err instanceof Error ? err.message : String(err),
      tone: 'error',
    })
  }
}

async function handleRemoveBookmark(bmId: string): Promise<void> {
  try {
    await api.deleteBookmark(bmId)
    bookmarks.value = bookmarks.value.filter((b) => b.id !== bmId)
  } catch (err) {
    snackbarStore.show({
      message: err instanceof Error ? err.message : String(err),
      tone: 'error',
    })
  }
}

async function handleSwitchTrackedSource(
  item: AggregatedSearchItemDto,
  clearCache: boolean,
): Promise<void> {
  const targetBook = book.value
  if (!targetBook?.online_tracking || switchingSourceKey.value !== null) return
  const targetId = targetBook.id
  const key = `${item.source_id}:${item.book.book_url}`
  switchingSourceKey.value = key
  try {
    const detailRes = await onlineBookApi.fetchBookInfo(item.source_id, item.book.book_url, {
      name: item.book.name,
      author: item.book.author,
      book_url: item.book.book_url,
      toc_url: '',
      cover_url: item.book.cover_url,
      intro: item.book.intro,
    })
    const tocUrl = detailRes.detail.toc_url?.trim() || ''
    if (!tocUrl) {
      throw new Error(t('online.switchSourceNoToc'))
    }
    const res = await onlineBookApi.switchTrackedSource(targetId, {
      source_id: item.source_id,
      book_url: item.book.book_url,
      toc_url: tocUrl,
      cover_url: detailRes.detail.cover_url ?? item.book.cover_url,
      intro: detailRes.detail.intro ?? item.book.intro,
      last_chapter: detailRes.detail.last_chapter ?? item.book.last_chapter,
      clear_cache: clearCache,
    })
    switchSourceOpen.value = false
    activeSheet.value = null
    snackbarStore.show(
      t('online.trackedSwitchSuccess', {
        source: res.source_name || item.source_name,
        count: res.total_chapters,
      }),
    )
    await trackedRef.value?.reload()
  } catch (err) {
    snackbarStore.show({
      message: err instanceof Error ? err.message : String(err),
      tone: 'error',
    })
  } finally {
    switchingSourceKey.value = null
  }
}

onMounted(() => {
  disposed = false
  window.addEventListener('keydown', handleReaderKeydown)
  window.addEventListener('pagehide', handlePageHide)
  void loadBook(bookId.value)
})

watch(bookId, (nextId, prevId) => {
  if (nextId === prevId) return
  void flushPendingProgress().finally(() => {
    if (!disposed) {
      void loadBook(nextId)
    }
  })
})

onBeforeUnmount(() => {
  disposed = true
  loadSeq += 1
  window.removeEventListener('keydown', handleReaderKeydown)
  window.removeEventListener('pagehide', handlePageHide)
  void flushPendingProgress()
})
</script>

<template>
  <ImmersiveMediaTemplate
    ref="immersiveRef"
    v-model:hud-visible="hudVisible"
    v-model:active-sheet="activeSheet"
    :title="book?.metadata.title || ''"
    :subtitle="progressSubtitle"
    stage-bg-class="bg-surface text-on-surface"
    :auto-hide="true"
    :lock-hud="loading || Boolean(loadError) || switchSourceOpen"
    :center-tap-mode="centerTapMode"
    :back-label="t('reader.backToBook')"
    :sheet-title="sheetTitle"
    :sheet-subtitle="sheetSubtitle"
    sheet-size="sm"
    :escape-to-exit="true"
    @back="exitReader"
    @tap-prev="handleTapPrev"
    @tap-next="handleTapNext"
  >
    <!-- Top HUD Right Controls (<= 4 controls + built-in Back = <= 5 total) -->
    <template #header-controls>
      <MdIconButton
        size="sm"
        variant="standard"
        :selected="activeSheet === 'toc'"
        :icon="LuList"
        :aria-label="t('reader.toc')"
        :tooltip="t('reader.toc')"
        @click="toggleSheet('toc')"
      />

      <MdIconButton
        size="sm"
        variant="standard"
        :selected="activeSheet === 'bookmarks'"
        :icon="LuBookmark"
        :aria-label="t('reader.bookmarks')"
        :tooltip="t('reader.bookmarks')"
        @click="toggleSheet('bookmarks')"
      />

      <MdIconButton
        v-if="!isPdf"
        size="sm"
        variant="standard"
        :selected="activeSheet === 'typography'"
        :icon="LuSlidersHorizontal"
        :aria-label="t('reader.typography')"
        :tooltip="t('reader.typography')"
        @click="toggleSheet('typography')"
      />
    </template>

    <!-- Main Reader Stage -->
    <div v-if="loading" class="flex h-full flex-col items-center justify-center gap-3 text-on-surface-variant">
      <MdCircularProgress indeterminate :size="32" :stroke-width="3" />
      <span class="text-body-small">{{ t('reader.loadingBook') }}</span>
    </div>

    <div v-else-if="loadError" class="flex h-full items-center justify-center px-6">
      <MediaErrorState
        :title="t('reader.loadFailedTitle')"
        :message="loadError"
        :retry-label="t('reader.retry')"
        class="max-w-md"
        @retry="loadBook(bookId)"
      >
        <template #action>
          <MdButton variant="tonal" @click="exitReader">
            {{ t('reader.backToBook') }}
          </MdButton>
        </template>
      </MediaErrorState>
    </div>

    <template v-else-if="book">
      <TrackedOnlineReader
        v-if="book.online_tracking"
        :key="`tracked-${bookId}`"
        ref="trackedRef"
        :book-id="bookId"
        :initial-location="initialLocation"
        @progress="handleProgressUpdate"
        @toc-loaded="toc = $event"
        @switch-source="switchSourceOpen = true"
      />
      <EpubReader
        v-else-if="book.format === 'epub'"
        :key="`epub-${bookId}`"
        ref="epubRef"
        :book-id="bookId"
        :initial-location="initialLocation"
        :hud-visible="hudVisible"
        @progress="handleProgressUpdate"
        @toggle-hud="immersiveRef?.toggleHud()"
      />
      <TxtReader
        v-else-if="isTextChapterBookFormat(book.format)"
        :key="`txt-${bookId}`"
        ref="txtRef"
        :book-id="bookId"
        :initial-location="initialLocation"
        @progress="handleProgressUpdate"
      />
      <PdfReader
        v-else
        :key="`pdf-${bookId}`"
        ref="pdfRef"
        :book-id="bookId"
        :initial-location="initialLocation"
        @progress="handleProgressUpdate"
        @state-change="handlePdfStateChange"
      />
    </template>

    <!-- Floating Auto-Hide Bottom Control Bar (PDF only) -->
    <template v-if="isPdf" #footer>
      <div class="mx-auto flex max-w-3xl flex-wrap items-center justify-between gap-2">
        <div class="flex items-center gap-1">
          <MdIconButton
            size="sm"
            variant="standard"
            :icon="LuChevronLeft"
            :disabled="pdfState.currentPage <= 1"
            :aria-label="t('reader.prevPage')"
            @click="pdfRef?.prevPage()"
          />
          <form class="flex items-center gap-1.5" @submit.prevent="handlePdfPageSubmit">
            <div class="w-16">
              <MdTextField
                v-model="pdfPageInput"
                type="number"
                size="sm"
                min="1"
                :max="pdfState.totalPages"
                :aria-label="t('reader.page')"
              />
            </div>
            <span class="text-label-medium tabular-nums text-on-surface-variant">
              / {{ pdfState.totalPages }}
            </span>
          </form>
          <MdIconButton
            size="sm"
            variant="standard"
            :icon="LuChevronRight"
            :disabled="pdfState.currentPage >= pdfState.totalPages"
            :aria-label="t('reader.nextPage')"
            @click="pdfRef?.nextPage()"
          />
        </div>

        <div class="flex items-center gap-1">
          <MdIconButton
            size="sm"
            variant="standard"
            :icon="LuZoomOut"
            :aria-label="t('reader.zoomOut')"
            @click="pdfRef?.zoomOut()"
          />
          <span class="text-label-medium w-12 text-center font-mono tabular-nums text-on-surface">
            {{ Math.round(pdfState.scale * 100) }}%
          </span>
          <MdIconButton
            size="sm"
            variant="standard"
            :icon="LuZoomIn"
            :aria-label="t('reader.zoomIn')"
            @click="pdfRef?.zoomIn()"
          />
        </div>

        <MdSegmentedButton
          :model-value="pdfState.fitMode === 'page' ? 'page' : 'width'"
          :options="pdfFitOptions"
          size="sm"
          :show-check="false"
          @update:model-value="handlePdfFitChange"
        />
      </div>
    </template>

    <!-- Single Mutually-Exclusive Sheet (TOC / Bookmarks / Typography) -->
    <template
      v-if="activeSheet === 'toc' && book?.online_tracking && authStore.isAdmin"
      #sheet-header-actions
    >
      <MdButton
        variant="tonal"
        :icon="LuRepeat"
        @click="
          activeSheet = null;
          switchSourceOpen = true
        "
      >
        {{ t('online.switchSource') }}
      </MdButton>
    </template>

    <template #sheet>
      <BookTocDrawer
        v-if="activeSheet === 'toc'"
        :items="toc"
        :active-href="currentLocation?.locator"
        searchable
        @select="jumpToTarget"
      />

      <BookBookmarksSheet
        v-else-if="activeSheet === 'bookmarks'"
        :bookmarks="bookmarks"
        @add="handleAddBookmark"
        @select="jumpToTarget"
        @remove="handleRemoveBookmark"
      />

      <BookTypographySheet
        v-else-if="activeSheet === 'typography'"
        :show-flow-mode="Boolean(book && book.format === 'epub' && !book.online_tracking)"
      />
    </template>

    <template #overlay>
      <BookSourceSwitchDialog
        v-if="book?.online_tracking"
        :open="switchSourceOpen"
        :book-title="book.metadata.title"
        :current-source-id="book.online_tracking.source_id"
        :current-book-url="book.online_tracking.book_url"
        :switching-key="switchingSourceKey"
        show-clear-cache-option
        @close="switchSourceOpen = false"
        @select="handleSwitchTrackedSource"
      />
    </template>
  </ImmersiveMediaTemplate>
</template>

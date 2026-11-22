<script setup lang="ts">
import { computed, defineAsyncComponent, onBeforeUnmount, onMounted, ref, watch } from 'vue'
import { useRoute, useRouter } from 'vue-router'
import { useBookI18n as useI18n } from '@/domains/book/i18n'
import {
  LuChevronLeft,
  LuList,
  LuBookmark,
  LuSlidersHorizontal,
  LuTrash2,
  LuPlus,
  LuX,
} from 'vue-icons-plus/lu'
import { api } from '@/domains/book/api'
import { pushEscapeHandler } from '@/composables/useEscapeStack'
import { useSnackbarStore } from '@/stores/snackbar'
import { useThemeStore } from '@/stores/theme'
import type { Bookmark, BookSummaryDto, ReadingLocation, TocItem } from '@/domains/book/types'
import ThemePopover from '@/components/ThemePopover.vue'
import AppSlider from '@/components/ui/AppSlider.vue'
import MdSegmentedButton, { type SegmentedOption } from '@/components/ui/MdSegmentedButton.vue'
import TocTree from '@/domains/book/components/TocTree.vue'
const EpubReader = defineAsyncComponent(() => import('@/domains/book/readers/EpubReader.vue'))
const TxtReader = defineAsyncComponent(() => import('@/domains/book/readers/TxtReader.vue'))
const PdfReader = defineAsyncComponent(() => import('@/domains/book/readers/PdfReader.vue'))
const TrackedOnlineReader = defineAsyncComponent(
  () => import('@/domains/book/readers/TrackedOnlineReader.vue'),
)

interface ReaderHandle {
  navigateTo: (target: string, scrollRatio?: number) => Promise<void>
}

const { t } = useI18n()
const route = useRoute()
const router = useRouter()
const themeStore = useThemeStore()
const snackbarStore = useSnackbarStore()

const bookId = computed(() => String(route.params.id ?? ''))
const book = ref<BookSummaryDto | null>(null)
const toc = ref<TocItem[]>([])
const bookmarks = ref<Bookmark[]>([])
const initialLocation = ref<ReadingLocation | null>(null)
const currentLocation = ref<ReadingLocation | null>(null)
const loading = ref(true)
const loadError = ref<string | null>(null)

const activePanel = ref<'none' | 'toc' | 'bookmarks' | 'typography'>('none')
const bookmarkLabel = ref('')

const epubRef = ref<ReaderHandle | null>(null)
const txtRef = ref<ReaderHandle | null>(null)
const pdfRef = ref<ReaderHandle | null>(null)
const trackedRef = ref<ReaderHandle | null>(null)

const flowOptions = computed<SegmentedOption<'paginated' | 'scrolled'>[]>(() => [
  { value: 'paginated', label: t('reader.paginated') },
  { value: 'scrolled', label: t('reader.scrolled') },
])

const fontOptions = computed<SegmentedOption<string>[]>(() => [
  { value: 'system-ui', label: t('reader.fontSystem') },
  { value: 'serif', label: t('reader.fontSerif') },
  { value: 'mono', label: t('reader.fontMono') },
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
    try {
      void fetch(`/api/v1/book/books/${encodeURIComponent(targetBookId)}/progress`, {
        method: 'PUT',
        credentials: 'include',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify(payload),
        keepalive: true,
      })
    } catch {
      // ignore unload error
    }
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
  activePanel.value = 'none'

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

// Reader-level Escape is the bottom layer of the shared LIFO escape stack, so popovers/sheets
// opened later (theme popover, panels) are dismissed first instead of exiting the reader.
let removeReaderEscape: (() => void) | null = null

function handleReaderEscape(): void {
  if (activePanel.value !== 'none') {
    activePanel.value = 'none'
  } else {
    void exitReader()
  }
}

onMounted(() => {
  disposed = false
  removeReaderEscape = pushEscapeHandler(handleReaderEscape)
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
  removeReaderEscape?.()
  removeReaderEscape = null
  window.removeEventListener('keydown', handleReaderKeydown)
  window.removeEventListener('pagehide', handlePageHide)
  void flushPendingProgress()
})

function handleReaderKeydown(ev: KeyboardEvent): void {
  const target = ev.target
  const isTyping =
    target instanceof HTMLElement &&
    (target.tagName === 'INPUT' ||
      target.tagName === 'TEXTAREA' ||
      target.tagName === 'SELECT' ||
      target.isContentEditable)

  if (isTyping || ev.metaKey || ev.ctrlKey || ev.altKey) return
  if (ev.key.toLowerCase() === 'b') {
    ev.preventDefault()
    togglePanel('bookmarks')
  } else if (ev.key.toLowerCase() === 't') {
    ev.preventDefault()
    togglePanel('toc')
  }
}

async function exitReader(): Promise<void> {
  await flushPendingProgress()
  const currentId = bookId.value
  const prev = (window.history.state as { back?: string | null } | null)?.back
  if (typeof prev === 'string' && prev.startsWith(`/book/items/${currentId}`)) {
    router.back()
  } else if (currentId) {
    await router.replace(`/book/items/${currentId}`)
  } else {
    await router.replace('/book/catalog')
  }
}

function togglePanel(panel: 'toc' | 'bookmarks' | 'typography'): void {
  activePanel.value = activePanel.value === panel ? 'none' : panel
}

function handleProgressUpdate(payload: {
  progress_percent: number
  locator: string
  chapter_index?: number | null
  page_number?: number | null
  scroll_ratio?: number | null
}): void {
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
  activePanel.value = 'none'
  if (book.value?.online_tracking) {
    await trackedRef.value?.navigateTo(hrefOrLocator, scrollRatio)
  } else if (book.value?.format === 'epub') {
    await epubRef.value?.navigateTo(hrefOrLocator)
  } else if (book.value?.format === 'txt') {
    await txtRef.value?.navigateTo(hrefOrLocator, scrollRatio)
  } else if (book.value?.format === 'pdf') {
    await pdfRef.value?.navigateTo(hrefOrLocator)
  }
}

async function handleAddBookmark(): Promise<void> {
  if (!currentLocation.value || !bookId.value) return
  const loc = currentLocation.value
  const defaultTitle = `${Math.round(loc.progress_percent)}% · ${book.value?.metadata.title ?? ''}`
  try {
    const created = await api.createBookmark(bookId.value, {
      title: bookmarkLabel.value.trim() || defaultTitle,
      progress_percent: loc.progress_percent,
      locator: loc.locator,
      chapter_index: loc.chapter_index,
      page_number: loc.page_number,
      scroll_ratio: loc.scroll_ratio,
    })
    bookmarkLabel.value = ''
    bookmarks.value = [created, ...bookmarks.value]
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
</script>

<template>
  <div class="flex h-screen flex-col overflow-hidden bg-[var(--md-sys-color-surface-container)] text-[var(--md-sys-color-on-surface)]">
    <!-- Distraction-free Top Reader Control Bar -->
    <header class="flex h-14 shrink-0 items-center justify-between gap-3 border-b border-[var(--md-sys-color-outline-variant)]/55 bg-[var(--md-sys-color-surface)] px-4">
      <div class="flex min-w-0 items-center gap-2.5">
        <button
          type="button"
          class="md-btn md-btn-sm md-btn-text md-state !text-[var(--md-sys-color-on-surface-variant)] hover:!text-[var(--md-sys-color-on-surface)]"
          @click="exitReader"
        >
          <LuChevronLeft class="h-4 w-4" />
          <span class="hidden sm:inline">{{ t('reader.backToBook') }}</span>
        </button>

        <div class="h-4 w-px bg-[var(--md-sys-color-outline-variant)]" />

        <div class="min-w-0">
          <h1 class="truncate text-sm font-bold">{{ book?.metadata.title }}</h1>
        </div>
      </div>

      <div class="flex items-center gap-1.5">
        <span
          v-if="currentLocation"
          class="hidden rounded-full bg-[var(--md-sys-color-primary-container)] px-2.5 py-1 text-xs font-semibold text-[var(--md-sys-color-on-primary-container)] sm:inline-block"
        >
          {{ Math.round(currentLocation.progress_percent) }}%
        </span>

        <button
          type="button"
          class="md-icon-btn md-icon-btn-sm md-state !h-9 !w-9"
          :class="
            activePanel === 'toc'
              ? '!bg-[var(--md-sys-color-secondary-container)] !text-[var(--md-sys-color-on-secondary-container)]'
              : ''
          "
          :title="t('reader.toc')"
          @click="togglePanel('toc')"
        >
          <LuList class="h-4 w-4" />
        </button>

        <button
          type="button"
          class="md-icon-btn md-icon-btn-sm md-state !h-9 !w-9"
          :class="
            activePanel === 'bookmarks'
              ? '!bg-[var(--md-sys-color-secondary-container)] !text-[var(--md-sys-color-on-secondary-container)]'
              : ''
          "
          :title="t('reader.bookmarks')"
          @click="togglePanel('bookmarks')"
        >
          <LuBookmark class="h-4 w-4" />
        </button>

        <button
          v-if="book?.format !== 'pdf'"
          type="button"
          class="md-icon-btn md-icon-btn-sm md-state !h-9 !w-9"
          :class="
            activePanel === 'typography'
              ? '!bg-[var(--md-sys-color-secondary-container)] !text-[var(--md-sys-color-on-secondary-container)]'
              : ''
          "
          :title="t('reader.typography')"
          @click="togglePanel('typography')"
        >
          <LuSlidersHorizontal class="h-4 w-4" />
        </button>

        <ThemePopover />
      </div>
    </header>

    <!-- Main Reader Area + Slide-Out Drawer -->
    <div class="relative flex min-h-0 flex-1 overflow-hidden">
      <div v-if="loading" class="flex flex-1 items-center justify-center text-sm text-[var(--md-sys-color-on-surface-variant)]">
        {{ t('reader.loadingBook') }}
      </div>

      <div
        v-else-if="loadError"
        class="flex flex-1 flex-col items-center justify-center gap-3 px-6 text-center"
      >
        <p class="max-w-md rounded-2xl bg-[var(--md-sys-color-error-container)] px-4 py-3 text-xs text-[var(--md-sys-color-on-error-container)]">
          {{ loadError }}
        </p>
        <div class="flex items-center gap-2">
          <button
            type="button"
            class="md-btn md-btn-sm md-btn-tonal md-state"
            @click="exitReader"
          >
            {{ t('reader.backToBook') }}
          </button>
          <button
            type="button"
            class="md-btn md-btn-sm md-btn-filled md-state"
            @click="loadBook(bookId)"
          >
            重试
          </button>
        </div>
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
        />
        <EpubReader
          v-else-if="book.format === 'epub'"
          :key="`epub-${bookId}`"
          ref="epubRef"
          :book-id="bookId"
          :initial-location="initialLocation"
          @progress="handleProgressUpdate"
        />
        <TxtReader
          v-else-if="book.format === 'txt'"
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
        />
      </template>

      <!-- Drawer for TOC / Bookmarks / Typography -->
      <aside
        v-if="activePanel !== 'none'"
        class="absolute bottom-0 right-0 top-0 z-30 flex w-80 max-w-full flex-col border-l border-[var(--md-sys-color-outline-variant)]/55 bg-[var(--md-sys-color-surface-container-low)] shadow-[var(--md-sys-elevation-3)] sm:w-[336px]"
      >
        <div class="flex items-center justify-between border-b border-[var(--md-sys-color-outline-variant)]/55 px-4 py-3">
          <h2 class="text-sm font-bold">
            {{
              activePanel === 'toc'
                ? t('reader.toc')
                : activePanel === 'bookmarks'
                  ? t('reader.bookmarks')
                  : t('reader.typography')
            }}
          </h2>
          <button
            type="button"
            class="md-icon-btn md-icon-btn-sm md-state"
            @click="activePanel = 'none'"
          >
            <LuX class="h-4 w-4" />
          </button>
        </div>

        <!-- TOC Panel (Volume -> Chapter collapsible tree) -->
        <div v-if="activePanel === 'toc'" class="flex-1 overflow-y-auto p-3">
          <TocTree
            :items="toc"
            :active-href="currentLocation?.locator"
            @select="jumpToTarget"
          />
        </div>

        <!-- Bookmarks Panel -->
        <div v-else-if="activePanel === 'bookmarks'" class="flex flex-1 flex-col overflow-hidden p-4">
          <form class="mb-4 space-y-2" @submit.prevent="handleAddBookmark">
            <input
              v-model="bookmarkLabel"
              type="text"
              :placeholder="t('reader.bookmarkTitlePlaceholder')"
              class="md-input h-9 w-full text-xs"
            />
            <button
              type="submit"
              class="md-btn md-btn-sm md-btn-filled md-state w-full"
            >
              <LuPlus class="h-3.5 w-3.5" />
              <span>{{ t('reader.addBookmark') }}</span>
            </button>
          </form>

          <div class="flex-1 space-y-2 overflow-y-auto">
            <p v-if="bookmarks.length === 0" class="py-6 text-center text-xs text-[var(--md-sys-color-on-surface-variant)]">
              {{ t('reader.noBookmarks') }}
            </p>
            <div
              v-for="bm in bookmarks"
              :key="bm.id"
              class="group flex items-center justify-between gap-2 rounded-2xl bg-[var(--md-sys-color-surface-container)] px-3 py-2.5 text-xs"
            >
              <button
                type="button"
                class="min-w-0 flex-1 text-left"
                @click="jumpToTarget(bm.location.locator, bm.location.scroll_ratio ?? 0)"
              >
                <div class="truncate font-semibold text-[var(--md-sys-color-on-surface)]">{{ bm.title }}</div>
                <div class="mt-0.5 text-[11px] text-[var(--md-sys-color-primary)]">
                  {{ Math.round(bm.location.progress_percent) }}%
                </div>
              </button>
              <button
                type="button"
                class="md-icon-btn md-icon-btn-sm md-state hover:!text-[var(--md-sys-color-error)]"
                @click="handleRemoveBookmark(bm.id)"
              >
                <LuTrash2 class="h-3.5 w-3.5" />
              </button>
            </div>
          </div>
        </div>

        <!-- Typography & Flow Panel -->
        <div v-else class="flex-1 space-y-5 overflow-x-hidden overflow-y-auto p-4 text-xs">
          <div v-if="book?.format === 'epub' && !book.online_tracking">
            <label class="mb-2 block font-semibold text-[var(--md-sys-color-on-surface-variant)]">
              {{ t('reader.flowMode') }}
            </label>
            <MdSegmentedButton
              :model-value="themeStore.reader.flow"
              :options="flowOptions"
              size="sm"
              full-width
              @update:model-value="(v) => themeStore.updateReaderPreferences({ flow: v })"
            />
          </div>

          <div>
            <label class="mb-2 block font-semibold text-[var(--md-sys-color-on-surface-variant)]">
              {{ t('reader.fontFamily') }}
            </label>
            <MdSegmentedButton
              :model-value="themeStore.reader.font_family"
              :options="fontOptions"
              size="sm"
              full-width
              @update:model-value="(v) => themeStore.updateReaderPreferences({ font_family: v })"
            />
          </div>

          <div>
            <div class="mb-1.5 flex justify-between">
              <span class="font-semibold text-[var(--md-sys-color-on-surface-variant)]">{{ t('reader.fontSize') }}</span>
              <span class="font-mono tabular-nums">{{ themeStore.reader.font_size }} px</span>
            </div>
            <AppSlider
              :model-value="themeStore.reader.font_size"
              :min="14"
              :max="30"
              :step="1"
              :aria-label="t('reader.fontSize')"
              @update:model-value="(v) => themeStore.updateReaderPreferences({ font_size: v })"
            />
          </div>

          <div>
            <div class="mb-1.5 flex justify-between">
              <span class="font-semibold text-[var(--md-sys-color-on-surface-variant)]">{{ t('reader.lineHeight') }}</span>
              <span class="font-mono tabular-nums">{{ themeStore.reader.line_height.toFixed(2) }}</span>
            </div>
            <AppSlider
              :model-value="themeStore.reader.line_height"
              :min="1.3"
              :max="2.4"
              :step="0.05"
              :aria-label="t('reader.lineHeight')"
              @update:model-value="(v) => themeStore.updateReaderPreferences({ line_height: v })"
            />
          </div>

          <div>
            <div class="mb-1.5 flex justify-between">
              <span class="font-semibold text-[var(--md-sys-color-on-surface-variant)]">{{ t('reader.margin') }}</span>
              <span class="font-mono tabular-nums">{{ themeStore.reader.margin_px }} px</span>
            </div>
            <AppSlider
              :model-value="themeStore.reader.margin_px"
              :min="16"
              :max="120"
              :step="8"
              :aria-label="t('reader.margin')"
              @update:model-value="(v) => themeStore.updateReaderPreferences({ margin_px: v })"
            />
          </div>
        </div>
      </aside>
    </div>
  </div>
</template>

<script setup lang="ts">
import { computed, defineAsyncComponent, onBeforeUnmount, onMounted, ref } from 'vue'
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
import { useThemeStore } from '@/stores/theme'
import type { Bookmark, BookSummaryDto, ReadingLocation, TocItem } from '@/domains/book/types'
import ThemePopover from '@/components/ThemePopover.vue'
import MdSegmentedButton, { type SegmentedOption } from '@/components/ui/MdSegmentedButton.vue'
import TocTree from '@/domains/book/components/TocTree.vue'
const EpubReader = defineAsyncComponent(() => import('@/domains/book/readers/EpubReader.vue'))
const TxtReader = defineAsyncComponent(() => import('@/domains/book/readers/TxtReader.vue'))
const PdfReader = defineAsyncComponent(() => import('@/domains/book/readers/PdfReader.vue'))

interface ReaderHandle {
  navigateTo: (target: string, scrollRatio?: number) => Promise<void>
}

const { t } = useI18n()
const route = useRoute()
const router = useRouter()
const themeStore = useThemeStore()

const bookId = String(route.params.id)
const book = ref<BookSummaryDto | null>(null)
const toc = ref<TocItem[]>([])
const bookmarks = ref<Bookmark[]>([])
const initialLocation = ref<ReadingLocation | null>(null)
const currentLocation = ref<ReadingLocation | null>(null)
const loading = ref(true)

const activePanel = ref<'none' | 'toc' | 'bookmarks' | 'typography'>('none')
const bookmarkLabel = ref('')

const epubRef = ref<ReaderHandle | null>(null)
const txtRef = ref<ReaderHandle | null>(null)
const pdfRef = ref<ReaderHandle | null>(null)

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
let pendingProgress: PendingProgressPayload | null = null

async function flushPendingProgress(): Promise<void> {
  if (saveTimer) {
    clearTimeout(saveTimer)
    saveTimer = null
  }
  if (!pendingProgress) return
  const payload = pendingProgress
  pendingProgress = null
  await api.saveProgress(bookId, payload).catch(() => {})
}

onMounted(async () => {
  const [b, tocItems, progress, bms] = await Promise.all([
    api.getBook(bookId),
    api.getToc(bookId).catch(() => []),
    api.getProgress(bookId),
    api.listBookmarks(bookId),
  ])
  book.value = b
  toc.value = tocItems
  bookmarks.value = bms
  const queryLocator = typeof route.query.locator === 'string' ? route.query.locator : null
  if (queryLocator) {
    initialLocation.value = {
      progress_percent: progress?.location.progress_percent ?? 0,
      locator: queryLocator,
      chapter_index: route.query.chapter ? Number(route.query.chapter) : null,
      page_number: route.query.page ? Number(route.query.page) : null,
      scroll_ratio: route.query.ratio ? Number(route.query.ratio) : null,
    }
  } else {
    initialLocation.value = progress?.location ?? b.progress ?? null
  }
  currentLocation.value = initialLocation.value
  loading.value = false
  window.addEventListener('keydown', handleReaderKeydown)
})

onBeforeUnmount(() => {
  window.removeEventListener('keydown', handleReaderKeydown)
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

  if (ev.key === 'Escape') {
    ev.preventDefault()
    if (activePanel.value !== 'none') {
      activePanel.value = 'none'
    } else {
      void exitReader()
    }
    return
  }
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
  const prev = (window.history.state as { back?: string | null } | null)?.back
  if (typeof prev === 'string' && prev.startsWith(`/book/items/${bookId}`)) {
    router.back()
  } else {
    await router.replace(`/book/items/${bookId}`)
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
  currentLocation.value = {
    progress_percent: payload.progress_percent,
    locator: payload.locator,
    chapter_index: payload.chapter_index ?? null,
    page_number: payload.page_number ?? null,
    scroll_ratio: payload.scroll_ratio ?? null,
  }

  pendingProgress = payload
  if (saveTimer) clearTimeout(saveTimer)
  saveTimer = setTimeout(() => {
    saveTimer = null
    const toSave = pendingProgress
    pendingProgress = null
    if (toSave) {
      void api.saveProgress(bookId, toSave).catch(() => {})
    }
  }, 600)
}

async function jumpToTarget(hrefOrLocator: string, scrollRatio = 0): Promise<void> {
  activePanel.value = 'none'
  if (book.value?.format === 'epub') {
    await epubRef.value?.navigateTo(hrefOrLocator)
  } else if (book.value?.format === 'txt') {
    await txtRef.value?.navigateTo(hrefOrLocator, scrollRatio)
  } else if (book.value?.format === 'pdf') {
    await pdfRef.value?.navigateTo(hrefOrLocator)
  }
}

async function handleAddBookmark(): Promise<void> {
  if (!currentLocation.value) return
  const loc = currentLocation.value
  const defaultTitle = `${Math.round(loc.progress_percent)}% · ${book.value?.metadata.title ?? ''}`
  const created = await api.createBookmark(bookId, {
    title: bookmarkLabel.value.trim() || defaultTitle,
    progress_percent: loc.progress_percent,
    locator: loc.locator,
    chapter_index: loc.chapter_index,
    page_number: loc.page_number,
    scroll_ratio: loc.scroll_ratio,
  })
  bookmarkLabel.value = ''
  bookmarks.value = [created, ...bookmarks.value]
}

async function handleRemoveBookmark(bmId: string): Promise<void> {
  await api.deleteBookmark(bmId)
  bookmarks.value = bookmarks.value.filter((b) => b.id !== bmId)
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

      <template v-else-if="book">
        <EpubReader
          v-if="book.format === 'epub'"
          ref="epubRef"
          :book-id="bookId"
          :initial-location="initialLocation"
          @progress="handleProgressUpdate"
        />
        <TxtReader
          v-else-if="book.format === 'txt'"
          ref="txtRef"
          :book-id="bookId"
          :initial-location="initialLocation"
          @progress="handleProgressUpdate"
        />
        <PdfReader
          v-else
          ref="pdfRef"
          :book-id="bookId"
          :initial-location="initialLocation"
          @progress="handleProgressUpdate"
        />
      </template>

      <!-- Drawer for TOC / Bookmarks / Typography -->
      <aside
        v-if="activePanel !== 'none'"
        class="absolute bottom-0 right-0 top-0 z-30 flex w-80 flex-col border-l border-[var(--md-sys-color-outline-variant)]/55 bg-[var(--md-sys-color-surface-container-low)] shadow-[var(--md-sys-elevation-3)]"
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
        <div v-else class="flex-1 space-y-5 overflow-y-auto p-5 text-xs">
          <div v-if="book?.format === 'epub'">
            <label class="mb-2 block font-semibold text-[var(--md-sys-color-on-surface-variant)]">
              {{ t('reader.flowMode') }}
            </label>
            <MdSegmentedButton
              :model-value="themeStore.reader.flow"
              :options="flowOptions"
              class="w-full"
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
              @update:model-value="(v) => themeStore.updateReaderPreferences({ font_family: v })"
            />
          </div>

          <div>
            <div class="mb-1.5 flex justify-between">
              <span class="font-semibold text-[var(--md-sys-color-on-surface-variant)]">{{ t('reader.fontSize') }}</span>
              <span>{{ themeStore.reader.font_size }} px</span>
            </div>
            <input
              type="range"
              min="14"
              max="30"
              step="1"
              :value="themeStore.reader.font_size"
              class="w-full accent-[var(--md-sys-color-primary)]"
              @input="
                themeStore.updateReaderPreferences({
                  font_size: Number(($event.target as HTMLInputElement).value),
                })
              "
            />
          </div>

          <div>
            <div class="mb-1.5 flex justify-between">
              <span class="font-semibold text-[var(--md-sys-color-on-surface-variant)]">{{ t('reader.lineHeight') }}</span>
              <span>{{ themeStore.reader.line_height.toFixed(2) }}</span>
            </div>
            <input
              type="range"
              min="1.3"
              max="2.4"
              step="0.05"
              :value="themeStore.reader.line_height"
              class="w-full accent-[var(--md-sys-color-primary)]"
              @input="
                themeStore.updateReaderPreferences({
                  line_height: Number(($event.target as HTMLInputElement).value),
                })
              "
            />
          </div>

          <div>
            <div class="mb-1.5 flex justify-between">
              <span class="font-semibold text-[var(--md-sys-color-on-surface-variant)]">{{ t('reader.margin') }}</span>
              <span>{{ themeStore.reader.margin_px }} px</span>
            </div>
            <input
              type="range"
              min="16"
              max="120"
              step="8"
              :value="themeStore.reader.margin_px"
              class="w-full accent-[var(--md-sys-color-primary)]"
              @input="
                themeStore.updateReaderPreferences({
                  margin_px: Number(($event.target as HTMLInputElement).value),
                })
              "
            />
          </div>
        </div>
      </aside>
    </div>
  </div>
</template>

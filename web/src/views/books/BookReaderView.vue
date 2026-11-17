<script setup lang="ts">
import { onMounted, ref } from 'vue'
import { useRoute, useRouter } from 'vue-router'
import { useI18n } from 'vue-i18n'
import { LuArrowLeft, LuBookmark, LuList, LuMinus, LuPlus } from 'vue-icons-plus/lu'
import { api } from '@/api/client'
import { useThemeStore } from '@/stores/theme'
import EpubReader from '@/components/readers/EpubReader.vue'
import PdfReader from '@/components/readers/PdfReader.vue'
import TxtReader from '@/components/readers/TxtReader.vue'
import type { BookBookmark, BookSummaryDto, TocItem } from '@/types/api'

const { t } = useI18n()
const route = useRoute()
const router = useRouter()
const theme = useThemeStore()

const bookId = String(route.params.id)
const book = ref<BookSummaryDto | null>(null)
const toc = ref<TocItem[]>([])
const bookmarks = ref<BookBookmark[]>([])
const tocOpen = ref(false)
const bookmarksOpen = ref(false)
const latestLoc = ref<{
  progress_percent: number
  locator: string
  chapter_index?: number | null
  page_number?: number | null
  scroll_ratio?: number | null
} | null>(null)

const epubRef = ref<InstanceType<typeof EpubReader> | null>(null)
const pdfRef = ref<InstanceType<typeof PdfReader> | null>(null)
const txtRef = ref<InstanceType<typeof TxtReader> | null>(null)

let saveTimer: ReturnType<typeof setTimeout> | null = null

onMounted(async () => {
  book.value = await api.getBook(bookId)
  api.getBookToc(bookId).then((items) => (toc.value = items)).catch(() => {})
  api.listBookBookmarks(bookId).then((bms) => (bookmarks.value = bms)).catch(() => {})
})

function onProgress(payload: {
  progress_percent: number
  locator: string
  chapter_index?: number | null
  page_number?: number | null
  scroll_ratio?: number | null
}): void {
  latestLoc.value = payload
  if (saveTimer) clearTimeout(saveTimer)
  saveTimer = setTimeout(() => {
    void api.saveBookProgress(bookId, payload).catch(() => {})
  }, 800)
}

function jumpToHref(href: string): void {
  tocOpen.value = false
  bookmarksOpen.value = false
  if (book.value?.format === 'epub') void epubRef.value?.navigateTo(href)
  else if (book.value?.format === 'pdf') void pdfRef.value?.navigateTo(href)
  else if (book.value?.format === 'txt') void txtRef.value?.navigateTo(href)
}

async function addBookmark(): Promise<void> {
  if (!latestLoc.value) return
  const created = await api.createBookBookmark(bookId, {
    title: `${Math.round(latestLoc.value.progress_percent)}%`,
    ...latestLoc.value,
  })
  bookmarks.value.unshift(created)
}

function adjustFontSize(delta: number): void {
  theme.book.font_size = Math.min(32, Math.max(12, theme.book.font_size + delta))
  theme.persistPreferences()
}
</script>

<template>
  <div class="relative flex h-full w-full flex-col overflow-hidden">
    <!-- Reader Toolbar -->
    <div class="flex h-12 shrink-0 items-center justify-between border-b border-[var(--ctp-surface0)] bg-[var(--ctp-mantle)] px-4 text-xs">
      <div class="flex items-center gap-3 min-w-0">
        <button
          type="button"
          class="inline-flex items-center gap-1 rounded-lg px-2 py-1 hover:bg-[var(--ctp-surface0)]"
          @click="router.push('/books')"
        >
          <LuArrowLeft class="h-4 w-4" /> {{ t('reader.back') }}
        </button>
        <span class="truncate font-semibold">{{ book?.metadata.title }}</span>
      </div>

      <div class="flex items-center gap-2">
        <template v-if="book && book.format !== 'pdf'">
          <button type="button" class="rounded p-1 hover:bg-[var(--ctp-surface0)]" @click="adjustFontSize(-1)">
            <LuMinus class="h-3.5 w-3.5" />
          </button>
          <span class="tabular-nums">{{ theme.book.font_size }}px</span>
          <button type="button" class="rounded p-1 hover:bg-[var(--ctp-surface0)]" @click="adjustFontSize(1)">
            <LuPlus class="h-3.5 w-3.5" />
          </button>
        </template>

        <button
          type="button"
          class="inline-flex items-center gap-1 rounded-lg border border-[var(--ctp-surface0)] px-2.5 py-1 hover:border-[var(--ctp-accent)]"
          @click="tocOpen = !tocOpen; bookmarksOpen = false"
        >
          <LuList class="h-3.5 w-3.5" /> {{ t('reader.toc') }}
        </button>

        <button
          type="button"
          class="inline-flex items-center gap-1 rounded-lg border border-[var(--ctp-surface0)] px-2.5 py-1 hover:border-[var(--ctp-accent)]"
          @click="bookmarksOpen = !bookmarksOpen; tocOpen = false"
        >
          <LuBookmark class="h-3.5 w-3.5" /> ({{ bookmarks.length }})
        </button>

        <button
          type="button"
          class="rounded-lg bg-[var(--ctp-accent)] px-2.5 py-1 font-semibold text-[var(--ctp-on-accent)]"
          @click="addBookmark"
        >
          + {{ t('reader.addBookmark') }}
        </button>
      </div>
    </div>

    <!-- TOC Drawer -->
    <div
      v-if="tocOpen"
      class="absolute left-0 top-12 bottom-0 z-20 w-72 overflow-y-auto border-r border-[var(--ctp-surface0)] bg-[var(--ctp-mantle)] p-3 shadow-xl space-y-1"
    >
      <button
        v-for="item in toc"
        :key="item.index"
        type="button"
        class="block w-full truncate rounded-lg px-3 py-2 text-left text-xs hover:bg-[var(--ctp-surface0)]"
        :style="{ paddingLeft: `${12 + item.level * 12}px` }"
        @click="jumpToHref(item.href)"
      >
        {{ item.title }}
      </button>
    </div>

    <!-- Bookmarks Drawer -->
    <div
      v-if="bookmarksOpen"
      class="absolute right-0 top-12 bottom-0 z-20 w-72 overflow-y-auto border-l border-[var(--ctp-surface0)] bg-[var(--ctp-mantle)] p-3 shadow-xl space-y-1"
    >
      <div
        v-for="bm in bookmarks"
        :key="bm.id"
        class="flex items-center justify-between rounded-lg px-3 py-2 text-xs hover:bg-[var(--ctp-surface0)]"
      >
        <button type="button" class="flex-1 truncate text-left" @click="jumpToHref(bm.location.locator)">
          {{ bm.title }} · {{ Math.round(bm.location.progress_percent) }}%
        </button>
      </div>
    </div>

    <!-- Reader Viewport -->
    <div v-if="book" class="flex-1 min-h-0">
      <EpubReader
        v-if="book.format === 'epub'"
        ref="epubRef"
        :book-id="book.id"
        :initial-location="book.progress"
        @progress="onProgress"
      />
      <PdfReader
        v-else-if="book.format === 'pdf'"
        ref="pdfRef"
        :book-id="book.id"
        :initial-location="book.progress"
        @progress="onProgress"
      />
      <TxtReader
        v-else
        ref="txtRef"
        :book-id="book.id"
        :initial-location="book.progress"
        @progress="onProgress"
      />
    </div>
  </div>
</template>

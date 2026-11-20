<script setup lang="ts">
import { computed, onMounted, ref, watch } from 'vue'
import { useRouter } from 'vue-router'
import {
  LuBookMarked,
  LuBookOpen,
  LuChevronLeft,
  LuChevronRight,
  LuDownload,
  LuGlobe,
  LuInfo,
  LuLayoutGrid,
  LuList,
  LuLoader2,
  LuPlus,
  LuSearch,
  LuSettings2,
  LuTrash2,
  LuUpload,
  LuX,
} from 'vue-icons-plus/lu'
import AppSelect, { type SelectOption } from '@/components/ui/AppSelect.vue'
import MdSegmentedButton, { type SegmentedOption } from '@/components/ui/MdSegmentedButton.vue'
import { useAuthStore } from '@/stores/auth'
import { useDialogStore } from '@/stores/dialog'
import { useOnlineDownloadsStore } from '@/stores/onlineDownloads'
import { useSnackbarStore } from '@/stores/snackbar'
import OnlineBookDetailSheet from '../components/OnlineBookDetailSheet.vue'
import OnlineDownloadModal, {
  type OnlineDownloadTarget,
} from '../components/OnlineDownloadModal.vue'
import OnlineShelfCard from '../components/OnlineShelfCard.vue'
import OnlineSourceManagerPanel from '../components/OnlineSourceManagerPanel.vue'
import {
  buildOnlineCoverUrl,
  onlineBookApi,
  type AggregatedSearchItemDto,
  type BookSourceSummaryDto,
  type OnlineDownloadJobDto,
  type OnlineShelfBookDto,
  type SourceSearchErrorDto,
} from '../onlineApi'

const SHELF_VIEW_STORAGE_KEY = 'xmedia_online_shelf_view_mode'

const router = useRouter()
const authStore = useAuthStore()
const snackbar = useSnackbarStore()
const dialogStore = useDialogStore()
const onlineDownloadsStore = useOnlineDownloadsStore()

type ActiveTab = 'shelf' | 'search' | 'sources'
type ShelfViewMode = 'grid' | 'list'
type ShelfSortMode = 'recent' | 'title' | 'progress'

const activeTab = ref<ActiveTab>('shelf')
const sources = ref<BookSourceSummaryDto[]>([])
const shelfBooks = ref<OnlineShelfBookDto[]>([])
const loadingInit = ref(false)

// Shelf toolbar & detail sheet state
const shelfQuery = ref('')
const shelfSourceFilter = ref<string>('all')
const shelfSort = ref<ShelfSortMode>('recent')
const shelfViewMode = ref<ShelfViewMode>(
  typeof window !== 'undefined' && localStorage.getItem(SHELF_VIEW_STORAGE_KEY) === 'list'
    ? 'list'
    : 'grid',
)
const detailBook = ref<OnlineShelfBookDto | null>(null)
const listCoverFailedIds = ref<Record<string, boolean>>({})

watch(shelfViewMode, (mode) => {
  try {
    localStorage.setItem(SHELF_VIEW_STORAGE_KEY, mode)
  } catch {
    // ignore storage quota errors
  }
})

// Search state with pagination
const searchKeyword = ref('')
const selectedSourceIds = ref<string[]>([])
const searching = ref(false)
const loadingMore = ref(false)
const searchPage = ref(1)
const hasMoreResults = ref(false)
const lastSearchKeyword = ref('')
const lastSearchSourceIds = ref<string[]>([])
const searchResults = ref<AggregatedSearchItemDto[]>([])
const searchErrors = ref<SourceSearchErrorDto[]>([])
const searchToasts = ref<string[]>([])
const hasSearched = ref(false)
const addingShelfKey = ref<string | null>(null)
const openingKey = ref<string | null>(null)

// Download modal bound to global onlineDownloads store
const downloadTarget = ref<OnlineDownloadTarget | null>(null)
const modalTrackedJobId = ref<string | null>(null)

const enabledSources = computed(() => sources.value.filter((s) => s.enabled))

const modalTrackedJob = computed<OnlineDownloadJobDto | null>(() => {
  if (!modalTrackedJobId.value) return null
  return onlineDownloadsStore.jobs.find((j) => j.id === modalTrackedJobId.value) ?? null
})

const tabOptions = computed<SegmentedOption<ActiveTab>[]>(() => {
  const opts: SegmentedOption<ActiveTab>[] = [
    {
      value: 'shelf',
      label: shelfBooks.value.length > 0 ? `书架 ${shelfBooks.value.length}` : '书架',
      icon: LuBookMarked,
    },
    {
      value: 'search',
      label: '搜索',
      icon: LuSearch,
    },
  ]
  if (authStore.isAdmin) {
    opts.push({
      value: 'sources',
      label: '管理',
      icon: LuSettings2,
    })
  }
  return opts
})

const viewModeOptions: readonly SegmentedOption<ShelfViewMode>[] = [
  { value: 'grid', label: '封面', icon: LuLayoutGrid, title: '封面网格' },
  { value: 'list', label: '列表', icon: LuList, title: '紧凑列表' },
]

const sortOptions: SelectOption<ShelfSortMode>[] = [
  { value: 'recent', label: '最近阅读' },
  { value: 'title', label: '书名' },
  { value: 'progress', label: '进度' },
]

const shelfSourceOptions = computed<SelectOption[]>(() => {
  const counts = new Map<string, { name: string; count: number }>()
  for (const book of shelfBooks.value) {
    const existing = counts.get(book.source_id)
    if (existing) {
      existing.count += 1
    } else {
      counts.set(book.source_id, { name: book.source_name, count: 1 })
    }
  }
  const opts: SelectOption[] = [
    { value: 'all', label: '全部书源', badge: shelfBooks.value.length },
  ]
  for (const [id, info] of counts.entries()) {
    opts.push({ value: id, label: info.name, badge: info.count })
  }
  return opts
})

const hasMultipleShelfSources = computed(() => shelfSourceOptions.value.length > 2)

watch(shelfSourceOptions, (opts) => {
  if (shelfSourceFilter.value !== 'all' && !opts.some((o) => o.value === shelfSourceFilter.value)) {
    shelfSourceFilter.value = 'all'
  }
})

function computeProgressRatio(book: OnlineShelfBookDto): number {
  if (book.total_chapters <= 0) return 0
  return Math.min(1, Math.max(0, (book.current_chapter_index + 1) / book.total_chapters))
}

function computeProgressPercent(book: OnlineShelfBookDto): number {
  return Math.round(computeProgressRatio(book) * 100)
}

const filteredShelfBooks = computed(() => {
  const q = shelfQuery.value.trim().toLowerCase()
  const sourceId = shelfSourceFilter.value

  const filtered = shelfBooks.value.filter((book) => {
    if (sourceId !== 'all' && book.source_id !== sourceId) return false
    if (q) {
      const nameMatch = book.name.toLowerCase().includes(q)
      const authorMatch = (book.author || '').toLowerCase().includes(q)
      if (!nameMatch && !authorMatch) return false
    }
    return true
  })

  const sorted = [...filtered]
  if (shelfSort.value === 'title') {
    sorted.sort((a, b) => a.name.localeCompare(b.name, 'zh-CN'))
  } else if (shelfSort.value === 'progress') {
    sorted.sort((a, b) => {
      const diff = computeProgressRatio(b) - computeProgressRatio(a)
      if (Math.abs(diff) > 0.0001) return diff
      return b.current_chapter_index - a.current_chapter_index
    })
  } else {
    sorted.sort((a, b) => {
      const ta = Date.parse(a.updated_at) || 0
      const tb = Date.parse(b.updated_at) || 0
      return tb - ta
    })
  }
  return sorted
})

function resetShelfFilters(): void {
  shelfQuery.value = ''
  shelfSourceFilter.value = 'all'
}

async function loadAll(): Promise<void> {
  loadingInit.value = true
  try {
    const [srcList, shelfList] = await Promise.all([
      onlineBookApi.listSources(),
      onlineBookApi.listShelf(),
    ])
    sources.value = srcList
    shelfBooks.value = shelfList
    if (shelfList.length === 0 && srcList.some((s) => s.enabled) && activeTab.value === 'shelf') {
      activeTab.value = 'search'
    } else if (srcList.length === 0 && authStore.isAdmin) {
      activeTab.value = 'sources'
    }
    void onlineDownloadsStore.refresh()
  } catch (err) {
    snackbar.show({
      message: err instanceof Error ? err.message : String(err),
      tone: 'error',
    })
  } finally {
    loadingInit.value = false
  }
}

async function refreshSources(): Promise<void> {
  try {
    sources.value = await onlineBookApi.listSources()
  } catch {
    // ignore
  }
}

onMounted(() => {
  void loadAll()
})

function toggleSourceFilter(id: string): void {
  if (selectedSourceIds.value.includes(id)) {
    selectedSourceIds.value = selectedSourceIds.value.filter((s) => s !== id)
  } else {
    selectedSourceIds.value = [...selectedSourceIds.value, id]
  }
}

async function runSearch(page: number, mode: 'replace' | 'append'): Promise<void> {
  const kw = (
    mode === 'replace' && page === 1
      ? searchKeyword.value
      : lastSearchKeyword.value || searchKeyword.value
  ).trim()
  if (!kw || searching.value || loadingMore.value) return
  const srcIds =
    mode === 'replace' && page === 1 ? [...selectedSourceIds.value] : [...lastSearchSourceIds.value]

  if (mode === 'append') {
    loadingMore.value = true
  } else {
    searching.value = true
    hasSearched.value = true
    searchErrors.value = []
    searchToasts.value = []
  }

  try {
    const res = await onlineBookApi.search(kw, srcIds, page)
    lastSearchKeyword.value = kw
    lastSearchSourceIds.value = srcIds
    searchPage.value = res.page || page

    if (mode === 'append') {
      const seen = new Set(searchResults.value.map((i) => `${i.source_id}:${i.book.book_url}`))
      const appended = res.items.filter((i) => !seen.has(`${i.source_id}:${i.book.book_url}`))
      searchResults.value = [...searchResults.value, ...appended]
      hasMoreResults.value = appended.length > 0
      if (appended.length === 0) {
        snackbar.show({ message: '没有更多新结果了' })
      }
    } else {
      searchResults.value = res.items
      hasMoreResults.value = res.items.length > 0
    }
    searchErrors.value = res.errors
    searchToasts.value = res.toasts
  } catch (err) {
    snackbar.show({
      message: err instanceof Error ? err.message : String(err),
      tone: 'error',
    })
  } finally {
    searching.value = false
    loadingMore.value = false
  }
}

async function handleSearch(): Promise<void> {
  await runSearch(1, 'replace')
}

async function handleLoadMore(): Promise<void> {
  if (!hasMoreResults.value) return
  await runSearch(searchPage.value + 1, 'append')
}

async function handleSwitchPage(nextPage: number): Promise<void> {
  if (nextPage < 1) return
  await runSearch(nextPage, 'replace')
}

function formatKindTags(raw: string | null | undefined): string {
  if (!raw) return ''
  return raw
    .split(/[,，|/]/)
    .map((s) => s.trim())
    .filter(Boolean)
    .join(' · ')
}

function isInShelf(item: AggregatedSearchItemDto): boolean {
  return shelfBooks.value.some(
    (b) => b.source_id === item.source_id && b.book_url === item.book.book_url,
  )
}

async function handleAddSearchResultToShelf(
  item: AggregatedSearchItemDto,
): Promise<OnlineShelfBookDto | null> {
  const key = `${item.source_id}:${item.book.book_url}`
  addingShelfKey.value = key
  try {
    const detailRes = await onlineBookApi.fetchBookInfo(item.source_id, item.book.book_url, {
      name: item.book.name,
      author: item.book.author,
      book_url: item.book.book_url,
      toc_url: '',
      cover_url: item.book.cover_url,
      intro: item.book.intro,
    })
    const saved = await onlineBookApi.addToShelf({
      source_id: item.source_id,
      name: detailRes.detail.name || item.book.name,
      author: detailRes.detail.author || item.book.author,
      book_url: item.book.book_url,
      toc_url: detailRes.detail.toc_url,
      cover_url: detailRes.detail.cover_url ?? item.book.cover_url,
      intro: detailRes.detail.intro ?? item.book.intro,
      kind: detailRes.detail.kind ?? item.book.kind,
      last_chapter: detailRes.detail.last_chapter ?? item.book.last_chapter,
    })
    shelfBooks.value = [saved, ...shelfBooks.value.filter((b) => b.id !== saved.id)]
    snackbar.show({ message: `已将《${saved.name}》加入在线书架` })
    return saved
  } catch (err) {
    snackbar.show({
      message: err instanceof Error ? err.message : String(err),
      tone: 'error',
    })
    return null
  } finally {
    addingShelfKey.value = null
  }
}

async function openSearchResultReader(item: AggregatedSearchItemDto): Promise<void> {
  const existing = shelfBooks.value.find(
    (b) => b.source_id === item.source_id && b.book_url === item.book.book_url,
  )
  if (existing) {
    openShelfReader(existing)
    return
  }
  const key = `${item.source_id}:${item.book.book_url}`
  openingKey.value = key
  try {
    const detailRes = await onlineBookApi.fetchBookInfo(item.source_id, item.book.book_url, {
      name: item.book.name,
      author: item.book.author,
      book_url: item.book.book_url,
      toc_url: '',
      cover_url: item.book.cover_url,
      intro: item.book.intro,
    })
    const bookName = detailRes.detail.name || item.book.name
    const bookAuthor = detailRes.detail.author || item.book.author
    const coverUrl = detailRes.detail.cover_url ?? item.book.cover_url
    const intro = detailRes.detail.intro ?? item.book.intro
    const tocUrl = detailRes.detail.toc_url

    sessionStorage.setItem(
      'xmedia_online_reader_ctx',
      JSON.stringify({
        sourceId: item.source_id,
        sourceName: item.source_name,
        name: bookName,
        author: bookAuthor,
        bookUrl: item.book.book_url,
        tocUrl,
        coverUrl,
        intro,
        currentChapterIndex: 0,
      }),
    )

    const query: Record<string, string> = {
      sourceId: item.source_id,
      sourceName: item.source_name,
      bookUrl: item.book.book_url,
      tocUrl,
      name: bookName,
      author: bookAuthor,
      chapter: '0',
    }
    if (coverUrl) {
      query.coverUrl = coverUrl
    }
    void router.push({ path: '/book/online/read', query })
  } catch (err) {
    snackbar.show({
      message: err instanceof Error ? err.message : String(err),
      tone: 'error',
    })
  } finally {
    openingKey.value = null
  }
}

function openShelfReader(book: OnlineShelfBookDto): void {
  sessionStorage.setItem(
    'xmedia_online_reader_ctx',
    JSON.stringify({
      shelfId: book.id,
      sourceId: book.source_id,
      sourceName: book.source_name,
      name: book.name,
      author: book.author,
      bookUrl: book.book_url,
      tocUrl: book.toc_url,
      coverUrl: book.cover_url,
      intro: book.intro,
      currentChapterIndex: book.current_chapter_index,
    }),
  )
  const query: Record<string, string> = {
    shelfId: book.id,
    sourceId: book.source_id,
    sourceName: book.source_name,
    bookUrl: book.book_url,
    tocUrl: book.toc_url,
    name: book.name,
    author: book.author,
    chapter: String(book.current_chapter_index || 0),
  }
  if (book.cover_url) {
    query.coverUrl = book.cover_url
  }
  void router.push({ path: '/book/online/read', query })
}

async function openDownloadForSearchItem(item: AggregatedSearchItemDto): Promise<void> {
  const key = `dl:${item.source_id}:${item.book.book_url}`
  openingKey.value = key
  try {
    const detailRes = await onlineBookApi.fetchBookInfo(item.source_id, item.book.book_url, {
      name: item.book.name,
      author: item.book.author,
      book_url: item.book.book_url,
      toc_url: '',
      cover_url: item.book.cover_url,
      intro: item.book.intro,
    })
    downloadTarget.value = {
      sourceId: item.source_id,
      sourceName: item.source_name,
      bookName: detailRes.detail.name || item.book.name,
      author: detailRes.detail.author || item.book.author,
      bookUrl: item.book.book_url,
      tocUrl: detailRes.detail.toc_url,
      coverUrl: detailRes.detail.cover_url ?? item.book.cover_url,
      intro: detailRes.detail.intro ?? item.book.intro,
    }
  } catch (err) {
    snackbar.show({
      message: err instanceof Error ? err.message : String(err),
      tone: 'error',
    })
  } finally {
    openingKey.value = null
  }
}

function openDownloadForShelf(book: OnlineShelfBookDto): void {
  downloadTarget.value = {
    sourceId: book.source_id,
    sourceName: book.source_name,
    bookName: book.name,
    author: book.author,
    bookUrl: book.book_url,
    tocUrl: book.toc_url,
    coverUrl: book.cover_url,
    intro: book.intro,
    totalChapters: book.total_chapters || undefined,
  }
}

function openShelfDetail(book: OnlineShelfBookDto): void {
  detailBook.value = book
}

function handleDetailRead(book: OnlineShelfBookDto): void {
  detailBook.value = null
  openShelfReader(book)
}

function handleDetailDownload(book: OnlineShelfBookDto): void {
  detailBook.value = null
  openDownloadForShelf(book)
}

async function handleRemoveShelf(book: OnlineShelfBookDto): Promise<void> {
  const ok = await dialogStore.confirm({
    title: '移出书架',
    message: `确定将《${book.name}》移出在线书架吗？`,
    confirmText: '移出',
    cancelText: '取消',
    danger: true,
  })
  if (!ok) return
  try {
    await onlineBookApi.removeFromShelf(book.id)
    shelfBooks.value = shelfBooks.value.filter((b) => b.id !== book.id)
    if (detailBook.value?.id === book.id) {
      detailBook.value = null
    }
  } catch (err) {
    snackbar.show({
      message: err instanceof Error ? err.message : String(err),
      tone: 'error',
    })
  }
}

function handleJobUpdated(job: OnlineDownloadJobDto): void {
  modalTrackedJobId.value = job.id
  onlineDownloadsStore.trackJob(job)
}

function closeDownloadModal(): void {
  downloadTarget.value = null
  modalTrackedJobId.value = null
}
</script>

<template>
  <div class="mx-auto max-w-6xl space-y-5 px-4 py-6 sm:px-6">
    <!-- Header + MD3 Segmented Tabs -->
    <div class="flex flex-wrap items-center justify-between gap-4">
      <h1 class="flex items-center gap-2 text-2xl font-bold tracking-tight text-[var(--md-sys-color-on-surface)]">
        <LuGlobe class="h-6 w-6 text-[var(--md-sys-color-primary)]" />
        <span>在线书源</span>
      </h1>

      <MdSegmentedButton
        v-model="activeTab"
        :options="tabOptions"
        size="sm"
        :show-check="false"
      />
    </div>

    <!-- TAB 1: Online Bookshelf -->
    <template v-if="activeTab === 'shelf'">
      <div
        v-if="loadingInit"
        class="flex items-center justify-center py-16 text-xs text-[var(--md-sys-color-on-surface-variant)]"
      >
        <LuLoader2 class="mr-2 h-4 w-4 animate-spin" />
        <span>正在加载书架…</span>
      </div>

      <div
        v-else-if="shelfBooks.length === 0"
        class="rounded-[24px] bg-[var(--md-sys-color-surface-container-low)] py-16 text-center"
      >
        <p class="text-sm font-semibold text-[var(--md-sys-color-on-surface)]">书架为空</p>
        <p class="mt-1 text-xs text-[var(--md-sys-color-on-surface-variant)]">
          前往「搜索」找书加入书架，或在「管理」导入书源
        </p>
        <button
          type="button"
          class="md-btn md-btn-sm md-btn-filled md-state mt-4"
          @click="activeTab = enabledSources.length > 0 || !authStore.isAdmin ? 'search' : 'sources'"
        >
          <LuSearch v-if="enabledSources.length > 0 || !authStore.isAdmin" class="h-3.5 w-3.5" />
          <LuUpload v-else class="h-3.5 w-3.5" />
          <span>{{ enabledSources.length > 0 || !authStore.isAdmin ? '去搜索' : '去导入' }}</span>
        </button>
      </div>

      <template v-else>
        <!-- Single-row Bookshelf Toolbar -->
        <div class="flex flex-wrap items-center gap-2.5">
          <div class="relative min-w-[12rem] flex-1 sm:max-w-xs">
            <LuSearch
              class="pointer-events-none absolute top-1/2 left-3 h-3.5 w-3.5 -translate-y-1/2 text-[var(--md-sys-color-on-surface-variant)]"
            />
            <input
              v-model="shelfQuery"
              type="text"
              placeholder="筛选书名或作者…"
              class="md-input h-9 w-full !pr-8 !pl-9 text-xs"
            />
            <button
              v-if="shelfQuery"
              type="button"
              aria-label="清空筛选"
              class="absolute top-1/2 right-2 flex h-5 w-5 -translate-y-1/2 cursor-pointer items-center justify-center rounded-full text-[var(--md-sys-color-on-surface-variant)] hover:bg-[var(--md-sys-color-surface-container-highest)] hover:text-[var(--md-sys-color-on-surface)]"
              @click="shelfQuery = ''"
            >
              <LuX class="h-3.5 w-3.5" />
            </button>
          </div>

          <div class="ml-auto flex flex-wrap items-center gap-2">
            <div v-if="hasMultipleShelfSources" class="min-w-[8rem]">
              <AppSelect
                v-model="shelfSourceFilter"
                :options="shelfSourceOptions"
                size="sm"
              />
            </div>

            <div class="min-w-[6.75rem]">
              <AppSelect
                :model-value="shelfSort"
                :options="sortOptions"
                size="sm"
                @update:model-value="shelfSort = $event as ShelfSortMode"
              />
            </div>

            <MdSegmentedButton
              v-model="shelfViewMode"
              :options="viewModeOptions"
              size="sm"
              :show-check="false"
            />
          </div>
        </div>

        <!-- Empty Filtered Result -->
        <div
          v-if="filteredShelfBooks.length === 0"
          class="rounded-[24px] bg-[var(--md-sys-color-surface-container-low)] py-14 text-center"
        >
          <p class="text-xs text-[var(--md-sys-color-on-surface-variant)]">
            没有匹配的书籍
          </p>
          <button
            type="button"
            class="md-btn md-btn-sm md-btn-tonal md-state mt-3"
            @click="resetShelfFilters"
          >
            重置筛选
          </button>
        </div>

        <!-- Portrait 2:3 Cover Grid (Default) -->
        <div
          v-else-if="shelfViewMode === 'grid'"
          class="grid grid-cols-2 gap-4 sm:grid-cols-3 sm:gap-5 md:grid-cols-4 lg:grid-cols-5 xl:grid-cols-6"
        >
          <OnlineShelfCard
            v-for="book in filteredShelfBooks"
            :key="book.id"
            :book="book"
            @read="openShelfReader"
            @download="openDownloadForShelf"
            @detail="openShelfDetail"
          />
        </div>

        <!-- Compact List View -->
        <div v-else class="space-y-2">
          <div
            v-for="book in filteredShelfBooks"
            :key="book.id"
            class="md-card-elevated flex items-center gap-3.5 p-3 transition hover:bg-[var(--md-sys-color-surface-container)]"
          >
            <div
              role="button"
              tabindex="0"
              :aria-label="`阅读《${book.name}》`"
              class="aspect-[2/3] w-11 shrink-0 cursor-pointer overflow-hidden rounded-lg bg-[var(--md-sys-color-surface-container-high)]"
              @click="openShelfReader(book)"
              @keydown.enter.prevent="openShelfReader(book)"
            >
              <img
                v-if="book.cover_url && !listCoverFailedIds[book.id]"
                :src="buildOnlineCoverUrl(book.cover_url)"
                :alt="book.name"
                loading="lazy"
                class="h-full w-full object-cover"
                @error="listCoverFailedIds[book.id] = true"
              />
              <div
                v-else
                class="flex h-full w-full items-center justify-center bg-[var(--md-sys-color-surface-container-high)] text-[var(--md-sys-color-on-surface-variant)]"
              >
                <LuBookOpen class="h-4 w-4 opacity-50" />
              </div>
            </div>

            <div class="min-w-0 flex-1 space-y-0.5">
              <div class="flex items-center gap-2">
                <button
                  type="button"
                  class="cursor-pointer truncate text-left text-sm font-semibold text-[var(--md-sys-color-on-surface)] hover:text-[var(--md-sys-color-primary)]"
                  @click="openShelfReader(book)"
                >
                  {{ book.name }}
                </button>
                <span
                  class="shrink-0 rounded-full bg-[var(--md-sys-color-primary-container)] px-2 py-0.5 text-[10px] font-medium text-[var(--md-sys-color-on-primary-container)]"
                >
                  {{ book.source_name }}
                </span>
              </div>

              <div class="flex flex-wrap items-center gap-x-2.5 gap-y-0.5 text-xs text-[var(--md-sys-color-on-surface-variant)]">
                <span class="truncate">{{ book.author || '佚名' }}</span>
                <span class="font-mono text-[11px]">
                  {{ book.current_chapter_title || `第 ${book.current_chapter_index + 1} 章` }}
                  <template v-if="book.total_chapters > 0">
                    ({{ book.current_chapter_index + 1 }} / {{ book.total_chapters }} · {{ computeProgressPercent(book) }}%)
                  </template>
                </span>
              </div>

              <p
                v-if="book.last_chapter"
                class="truncate text-[11px] text-[var(--md-sys-color-on-surface-variant)] opacity-80"
              >
                最新：{{ book.last_chapter }}
              </p>
            </div>

            <div class="flex shrink-0 items-center gap-1.5">
              <button
                type="button"
                class="md-btn md-btn-sm md-btn-filled md-state"
                @click="openShelfReader(book)"
              >
                <LuBookOpen class="h-3.5 w-3.5" />
                <span>阅读</span>
              </button>
              <button
                type="button"
                class="md-btn md-btn-sm md-btn-tonal md-state"
                @click="openDownloadForShelf(book)"
              >
                <LuDownload class="h-3.5 w-3.5" />
                <span>下载</span>
              </button>
              <button
                type="button"
                class="md-icon-btn md-icon-btn-sm md-state"
                title="详情"
                aria-label="书籍详情"
                @click="openShelfDetail(book)"
              >
                <LuInfo class="h-4 w-4" />
              </button>
              <button
                type="button"
                class="md-icon-btn md-icon-btn-sm md-state !text-[var(--md-sys-color-error)] hover:bg-[var(--md-sys-color-error-container)]/50"
                title="移出"
                aria-label="移出书架"
                @click="handleRemoveShelf(book)"
              >
                <LuTrash2 class="h-4 w-4" />
              </button>
            </div>
          </div>
        </div>
      </template>
    </template>

    <!-- TAB 2: Aggregated Online Search -->
    <template v-else-if="activeTab === 'search'">
      <div class="md-card-filled space-y-3 p-4">
        <form class="flex flex-wrap items-center gap-2" @submit.prevent="handleSearch">
          <div class="relative min-w-0 flex-1">
            <LuSearch class="pointer-events-none absolute top-1/2 left-3.5 h-4 w-4 -translate-y-1/2 text-[var(--md-sys-color-on-surface-variant)]" />
            <input
              v-model="searchKeyword"
              type="text"
              placeholder="输入书名或作者，搜索已启用的书源…"
              class="md-input h-10 w-full !pl-10 text-xs"
            />
          </div>
          <button
            type="submit"
            :disabled="!searchKeyword.trim() || searching || enabledSources.length === 0"
            class="md-btn md-btn-filled md-state h-10 px-5"
          >
            <LuLoader2 v-if="searching" class="h-4 w-4 animate-spin" />
            <LuSearch v-else class="h-4 w-4" />
            <span>{{ searching ? '搜索中…' : '搜索' }}</span>
          </button>
        </form>

        <!-- Source Filter Chips -->
        <div v-if="enabledSources.length > 0" class="flex flex-wrap items-center gap-1.5">
          <span class="text-[11px] text-[var(--md-sys-color-on-surface-variant)]">书源范围：</span>
          <button
            type="button"
            class="md-chip md-state !h-7 !rounded-full !px-2.5 !text-[11px]"
            :class="selectedSourceIds.length === 0 ? 'md-chip-selected' : ''"
            @click="selectedSourceIds = []"
          >
            全部 ({{ enabledSources.length }})
          </button>
          <button
            v-for="src in enabledSources"
            :key="src.id"
            type="button"
            class="md-chip md-state !h-7 !rounded-full !px-2.5 !text-[11px]"
            :class="selectedSourceIds.includes(src.id) ? 'md-chip-selected' : ''"
            @click="toggleSourceFilter(src.id)"
          >
            {{ src.name }}
          </button>
        </div>

        <p
          v-else
          class="text-xs text-[var(--md-sys-color-warning)]"
        >
          当前没有已启用的书源，请先在「管理」导入并启用至少一条书源。
        </p>
      </div>

      <!-- Toasts / Per-Source Errors -->
      <div
        v-if="searchErrors.length > 0 || searchToasts.length > 0"
        class="space-y-1.5 text-xs"
      >
        <div
          v-for="(t, i) in searchToasts"
          :key="`st-${i}`"
          class="rounded-xl bg-[var(--md-sys-color-surface-container)] px-3.5 py-2 text-[var(--md-sys-color-on-surface-variant)]"
        >
          {{ t }}
        </div>
        <div
          v-for="e in searchErrors"
          :key="e.source_id"
          class="rounded-xl bg-[var(--md-sys-color-error-container)]/70 px-3.5 py-2 text-[var(--md-sys-color-on-error-container)]"
        >
          「{{ e.source_name }}」搜索异常：{{ e.error }}
        </div>
      </div>

      <!-- Search Results -->
      <div
        v-if="hasSearched && !searching && searchResults.length === 0"
        class="rounded-[24px] bg-[var(--md-sys-color-surface-container-low)] py-14 text-center text-xs text-[var(--md-sys-color-on-surface-variant)]"
      >
        未找到与「{{ lastSearchKeyword || searchKeyword }}」匹配的书籍，可检查书源是否需要先登录或切换搜索关键词。
      </div>

      <template v-else-if="searchResults.length > 0">
        <div class="grid grid-cols-1 gap-3.5 md:grid-cols-2">
          <div
            v-for="(item, idx) in searchResults"
            :key="`${item.source_id}-${idx}-${item.book.book_url}`"
            class="md-card-filled flex gap-3.5 p-3.5"
          >
            <div class="h-28 w-20 shrink-0 overflow-hidden rounded-xl bg-[var(--md-sys-color-surface-container-high)]">
              <img
                v-if="item.book.cover_url"
                :src="buildOnlineCoverUrl(item.book.cover_url)"
                :alt="item.book.name"
                class="h-full w-full object-cover"
                loading="lazy"
              />
              <div
                v-else
                class="flex h-full w-full items-center justify-center text-[var(--md-sys-color-on-surface-variant)]"
              >
                <LuBookOpen class="h-6 w-6 opacity-50" />
              </div>
            </div>

            <div class="flex min-w-0 flex-1 flex-col justify-between">
              <div>
                <div class="flex items-start justify-between gap-2">
                  <h3 class="truncate text-sm font-semibold text-[var(--md-sys-color-on-surface)]">
                    {{ item.book.name }}
                  </h3>
                  <span class="shrink-0 rounded-full bg-[var(--md-sys-color-primary-container)] px-2 py-0.5 text-[10px] font-medium text-[var(--md-sys-color-on-primary-container)]">
                    {{ item.source_name }}
                  </span>
                </div>
                <p class="mt-0.5 truncate text-xs text-[var(--md-sys-color-on-surface-variant)]">
                  {{ item.book.author || '佚名' }}
                  <span v-if="item.book.word_count"> · {{ item.book.word_count }}</span>
                  <span v-if="formatKindTags(item.book.kind)"> · {{ formatKindTags(item.book.kind) }}</span>
                </p>
                <p
                  v-if="item.book.intro"
                  class="mt-1 line-clamp-2 text-[11px] leading-relaxed text-[var(--md-sys-color-on-surface-variant)]"
                >
                  {{ item.book.intro }}
                </p>
                <p
                  v-if="item.book.last_chapter"
                  class="mt-1 truncate text-[10px] text-[var(--md-sys-color-on-surface-variant)] opacity-80"
                >
                  最新：{{ item.book.last_chapter }}
                </p>
              </div>

              <div class="mt-2.5 flex flex-wrap items-center gap-1.5">
                <button
                  type="button"
                  :disabled="openingKey === `${item.source_id}:${item.book.book_url}`"
                  class="md-btn md-btn-sm md-btn-filled md-state"
                  @click="openSearchResultReader(item)"
                >
                  <LuLoader2
                    v-if="openingKey === `${item.source_id}:${item.book.book_url}`"
                    class="h-3.5 w-3.5 animate-spin"
                  />
                  <LuBookOpen v-else class="h-3.5 w-3.5" />
                  <span>阅读</span>
                </button>

                <button
                  type="button"
                  :disabled="isInShelf(item) || addingShelfKey === `${item.source_id}:${item.book.book_url}`"
                  class="md-btn md-btn-sm md-btn-tonal md-state"
                  @click="handleAddSearchResultToShelf(item)"
                >
                  <LuLoader2
                    v-if="addingShelfKey === `${item.source_id}:${item.book.book_url}`"
                    class="h-3.5 w-3.5 animate-spin"
                  />
                  <LuPlus v-else class="h-3.5 w-3.5" />
                  <span>{{ isInShelf(item) ? '已加' : '加书架' }}</span>
                </button>

                <button
                  type="button"
                  :disabled="openingKey === `dl:${item.source_id}:${item.book.book_url}`"
                  class="md-btn md-btn-sm md-btn-outlined md-state"
                  @click="openDownloadForSearchItem(item)"
                >
                  <LuLoader2
                    v-if="openingKey === `dl:${item.source_id}:${item.book.book_url}`"
                    class="h-3.5 w-3.5 animate-spin"
                  />
                  <LuDownload v-else class="h-3.5 w-3.5" />
                  <span>下载</span>
                </button>
              </div>
            </div>
          </div>
        </div>

        <!-- Search Pagination & Load More Controls -->
        <div class="flex flex-wrap items-center justify-between gap-3 rounded-2xl bg-[var(--md-sys-color-surface-container-low)] px-4 py-3 text-xs">
          <div class="flex items-center gap-2">
            <button
              type="button"
              :disabled="searchPage <= 1 || searching || loadingMore"
              class="md-btn md-btn-sm md-btn-outlined md-state"
              @click="handleSwitchPage(searchPage - 1)"
            >
              <LuChevronLeft class="h-3.5 w-3.5" />
              <span>上一页</span>
            </button>
            <span class="px-2 font-mono text-[var(--md-sys-color-on-surface-variant)]">
              第 {{ searchPage }} 页 · 共 {{ searchResults.length }} 条
            </span>
            <button
              type="button"
              :disabled="!hasMoreResults || searching || loadingMore"
              class="md-btn md-btn-sm md-btn-outlined md-state"
              @click="handleSwitchPage(searchPage + 1)"
            >
              <span>下一页</span>
              <LuChevronRight class="h-3.5 w-3.5" />
            </button>
          </div>

          <button
            type="button"
            :disabled="!hasMoreResults || searching || loadingMore"
            class="md-btn md-btn-sm md-btn-tonal md-state"
            @click="handleLoadMore"
          >
            <LuLoader2 v-if="loadingMore" class="h-3.5 w-3.5 animate-spin" />
            <LuPlus v-else class="h-3.5 w-3.5" />
            <span>{{ loadingMore ? '加载中…' : hasMoreResults ? '加载更多' : '已无更多' }}</span>
          </button>
        </div>
      </template>
    </template>

    <!-- TAB 3: Book Source Manager (Admin) -->
    <OnlineSourceManagerPanel
      v-else-if="activeTab === 'sources' && authStore.isAdmin"
      :sources="sources"
      @refresh="refreshSources"
    />

    <!-- Right-side Book Detail Sheet -->
    <OnlineBookDetailSheet
      :open="detailBook !== null"
      :book="detailBook"
      @close="detailBook = null"
      @read="handleDetailRead"
      @download="handleDetailDownload"
      @remove="handleRemoveShelf"
    />

    <!-- Online Download Modal -->
    <OnlineDownloadModal
      :open="downloadTarget !== null"
      :target="downloadTarget"
      :external-job="modalTrackedJob"
      :disable-polling="true"
      @close="closeDownloadModal"
      @job-updated="handleJobUpdated"
    />
  </div>
</template>

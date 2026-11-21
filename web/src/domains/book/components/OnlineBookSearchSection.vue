<script setup lang="ts">
import { computed, onMounted, ref, watch } from 'vue'
import { useRouter } from 'vue-router'
import {
  LuBookOpen,
  LuChevronLeft,
  LuChevronRight,
  LuDownload,
  LuLoader2,
  LuPlus,
  LuSearch,
  LuSettings2,
} from 'vue-icons-plus/lu'
import { useAuthStore } from '@/stores/auth'
import { useOnlineDownloadsStore } from '@/stores/onlineDownloads'
import { useSnackbarStore } from '@/stores/snackbar'
import { api as bookApi } from '../api'
import { useCatalogStore } from '../store'
import OnlineDownloadModal, { type OnlineDownloadTarget } from './OnlineDownloadModal.vue'
import {
  buildOnlineCoverUrl,
  onlineBookApi,
  type AggregatedSearchItemDto,
  type BookSourceSummaryDto,
  type OnlineDownloadJobDto,
  type SourceSearchErrorDto,
} from '../onlineApi'

const props = defineProps<{
  keyword: string
  initialSourceId?: string | null
}>()

const emit = defineEmits<{
  'update:keyword': [value: string]
}>()

const router = useRouter()
const authStore = useAuthStore()
const catalogStore = useCatalogStore()
const snackbar = useSnackbarStore()
const onlineDownloadsStore = useOnlineDownloadsStore()

const loadingInit = ref(false)
const sources = ref<BookSourceSummaryDto[]>([])
const trackedBookIdByKey = ref<Record<string, string>>({})
const searchKeyword = ref(props.keyword)
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
const downloadTarget = ref<OnlineDownloadTarget | null>(null)
const modalTrackedJobId = ref<string | null>(null)

const enabledSources = computed(() => sources.value.filter((s) => s.enabled))

const modalTrackedJob = computed<OnlineDownloadJobDto | null>(() => {
  if (!modalTrackedJobId.value) return null
  return onlineDownloadsStore.jobs.find((j) => j.id === modalTrackedJobId.value) ?? null
})

watch(
  () => props.initialSourceId,
  (nextId) => {
    if (nextId && sources.value.some((s) => s.id === nextId)) {
      selectedSourceIds.value = [nextId]
    }
  },
)

watch(
  () => props.keyword,
  (next) => {
    const trimmed = next.trim()
    searchKeyword.value = next
    if (
      trimmed &&
      trimmed !== lastSearchKeyword.value &&
      enabledSources.value.length > 0 &&
      !loadingInit.value
    ) {
      void runSearch(1, 'replace')
    }
  },
)

async function initData(): Promise<void> {
  loadingInit.value = true
  try {
    const [srcList, existingPage] = await Promise.all([
      onlineBookApi.listSources().catch(() => []),
      bookApi.searchBooks({ page: 1, page_size: 200 }).catch(() => null),
    ])
    sources.value = srcList
    if (props.initialSourceId && srcList.some((s) => s.id === props.initialSourceId)) {
      selectedSourceIds.value = [props.initialSourceId]
    }
    if (existingPage) {
      const map: Record<string, string> = {}
      for (const b of existingPage.items) {
        if (b.online_tracking) {
          map[`${b.online_tracking.source_id}:${b.online_tracking.book_url}`] = b.id
        }
      }
      trackedBookIdByKey.value = map
    }
  } finally {
    loadingInit.value = false
  }
  if (searchKeyword.value.trim() && enabledSources.value.length > 0) {
    void runSearch(1, 'replace')
  }
}

onMounted(() => {
  void initData()
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
  if (!kw || searching.value || loadingMore.value || enabledSources.value.length === 0) return

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

async function handleSearchSubmit(): Promise<void> {
  emit('update:keyword', searchKeyword.value)
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

function trackedBookIdFor(item: AggregatedSearchItemDto): string | undefined {
  return trackedBookIdByKey.value[`${item.source_id}:${item.book.book_url}`]
}

function isInShelf(item: AggregatedSearchItemDto): boolean {
  return Boolean(trackedBookIdFor(item))
}

async function handleAddSearchResultToShelf(item: AggregatedSearchItemDto): Promise<void> {
  const existingId = trackedBookIdFor(item)
  if (existingId) {
    void router.push(`/book/items/${existingId}`)
    return
  }
  const key = `${item.source_id}:${item.book.book_url}`
  addingShelfKey.value = key
  try {
    const detailRes = await onlineBookApi
      .fetchBookInfo(item.source_id, item.book.book_url, {
        name: item.book.name,
        author: item.book.author,
        book_url: item.book.book_url,
        toc_url: '',
        cover_url: item.book.cover_url,
        intro: item.book.intro,
      })
      .catch(() => null)
    const created = await onlineBookApi.trackOnlineBook({
      source_id: item.source_id,
      name: detailRes?.detail.name || item.book.name,
      author: detailRes?.detail.author || item.book.author,
      book_url: item.book.book_url,
      toc_url: detailRes?.detail.toc_url || '',
      cover_url: detailRes?.detail.cover_url ?? item.book.cover_url,
      intro: detailRes?.detail.intro ?? item.book.intro,
      kind: detailRes?.detail.kind ?? item.book.kind,
      last_chapter: detailRes?.detail.last_chapter ?? item.book.last_chapter,
    })
    trackedBookIdByKey.value = { ...trackedBookIdByKey.value, [key]: created.id }
    void catalogStore.refreshSidebarData()
    snackbar.show({
      message: `已将《${created.metadata.title}》加入书库（标准 EPUB 3.0）`,
      actionLabel: '查看',
      onAction: () => {
        void router.push(`/book/items/${created.id}`)
      },
    })
  } catch (err) {
    snackbar.show({
      message: err instanceof Error ? err.message : String(err),
      tone: 'error',
    })
  } finally {
    addingShelfKey.value = null
  }
}

async function openSearchResultReader(item: AggregatedSearchItemDto): Promise<void> {
  const existingId = trackedBookIdFor(item)
  if (existingId) {
    void router.push(`/book/read/${existingId}`)
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
    if (coverUrl) query.coverUrl = coverUrl
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
      bookId: trackedBookIdFor(item),
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

function handleJobUpdated(job: OnlineDownloadJobDto): void {
  modalTrackedJobId.value = job.id
  onlineDownloadsStore.trackJob(job)
}

function closeDownloadModal(): void {
  downloadTarget.value = null
  modalTrackedJobId.value = null
}

function goManageDataSources(): void {
  void router.push({ path: '/settings', query: { tab: 'domain-datasources', domain: 'book' } })
}
</script>

<template>
  <div class="space-y-4">
    <div class="md-card-filled space-y-3 p-4">
      <form class="flex flex-wrap items-center gap-2" @submit.prevent="handleSearchSubmit">
        <div class="relative min-w-0 flex-1">
          <LuSearch
            class="pointer-events-none absolute top-1/2 left-3.5 h-4 w-4 -translate-y-1/2 text-[var(--md-sys-color-on-surface-variant)]"
          />
          <input
            v-model="searchKeyword"
            type="text"
            placeholder="输入书名或作者，聚合搜索已启用的书源…"
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

      <div
        v-else-if="!loadingInit"
        class="flex flex-wrap items-center justify-between gap-2 text-xs text-[var(--md-sys-color-on-surface-variant)]"
      >
        <span>当前没有已启用的书源，请先在「设置 → 数据源 → 书籍」导入并启用书源。</span>
        <button
          v-if="authStore.isAdmin"
          type="button"
          class="md-btn md-btn-sm md-btn-tonal md-state"
          @click="goManageDataSources"
        >
          <LuSettings2 class="h-3.5 w-3.5" />
          <span>前往设置</span>
        </button>
      </div>
    </div>

    <!-- Toasts / Per-Source Errors -->
    <div
      v-if="searchErrors.length > 0 || searchToasts.length > 0"
      class="space-y-1.5 text-xs"
    >
      <div
        v-for="(toast, i) in searchToasts"
        :key="`st-${i}`"
        class="rounded-xl bg-[var(--md-sys-color-surface-container)] px-3.5 py-2 text-[var(--md-sys-color-on-surface-variant)]"
      >
        {{ toast }}
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
      未找到与「{{ lastSearchKeyword || searchKeyword }}」匹配的书籍
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
                :disabled="addingShelfKey === `${item.source_id}:${item.book.book_url}`"
                class="md-btn md-btn-sm md-btn-tonal md-state"
                @click="handleAddSearchResultToShelf(item)"
              >
                <LuLoader2
                  v-if="addingShelfKey === `${item.source_id}:${item.book.book_url}`"
                  class="h-3.5 w-3.5 animate-spin"
                />
                <LuPlus v-else class="h-3.5 w-3.5" />
                <span>{{ isInShelf(item) ? '已入库' : '加书架' }}</span>
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

      <!-- Pagination & Load More -->
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

    <!-- Online Download Modal -->
    <OnlineDownloadModal
      :open="downloadTarget !== null"
      :target="downloadTarget"
      :external-job="modalTrackedJob"
      @close="closeDownloadModal"
      @job-updated="handleJobUpdated"
    />
  </div>
</template>

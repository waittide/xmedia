<script setup lang="ts">
import { computed, onBeforeUnmount, onMounted, ref } from 'vue'
import { useRouter } from 'vue-router'
import {
  LuBan,
  LuBookMarked,
  LuBookOpen,
  LuDownload,
  LuGlobe,
  LuLoader2,
  LuPlus,
  LuSearch,
  LuSettings2,
  LuTrash2,
} from 'vue-icons-plus/lu'
import { useAuthStore } from '@/stores/auth'
import { useDialogStore } from '@/stores/dialog'
import { useSnackbarStore } from '@/stores/snackbar'
import OnlineDownloadModal, {
  type OnlineDownloadTarget,
} from '../components/OnlineDownloadModal.vue'
import OnlineSourceManagerPanel from '../components/OnlineSourceManagerPanel.vue'
import {
  buildOnlineCoverUrl,
  buildOnlineDownloadFileUrl,
  onlineBookApi,
  type AggregatedSearchItemDto,
  type BookSourceSummaryDto,
  type OnlineDownloadJobDto,
  type OnlineShelfBookDto,
  type SourceSearchErrorDto,
} from '../onlineApi'

const router = useRouter()
const authStore = useAuthStore()
const snackbar = useSnackbarStore()
const dialogStore = useDialogStore()

type ActiveTab = 'shelf' | 'search' | 'sources'
const activeTab = ref<ActiveTab>('shelf')

const sources = ref<BookSourceSummaryDto[]>([])
const shelfBooks = ref<OnlineShelfBookDto[]>([])
const downloads = ref<OnlineDownloadJobDto[]>([])
const loadingInit = ref(false)

// Search state
const searchKeyword = ref('')
const selectedSourceIds = ref<string[]>([])
const searching = ref(false)
const searchResults = ref<AggregatedSearchItemDto[]>([])
const searchErrors = ref<SourceSearchErrorDto[]>([])
const searchToasts = ref<string[]>([])
const hasSearched = ref(false)
const addingShelfKey = ref<string | null>(null)
const openingKey = ref<string | null>(null)

// Download modal & unified single polling loop
const downloadTarget = ref<OnlineDownloadTarget | null>(null)
const modalTrackedJobId = ref<string | null>(null)
const busyDownloadId = ref<string | null>(null)
let downloadsPollTimer: ReturnType<typeof setTimeout> | null = null
let downloadsPollInFlight = false
let isUnmounted = false

const enabledSources = computed(() => sources.value.filter((s) => s.enabled))
const hasActiveDownloads = computed(() =>
  downloads.value.some((j) => j.status === 'pending' || j.status === 'running'),
)
const modalTrackedJob = computed<OnlineDownloadJobDto | null>(() => {
  if (!modalTrackedJobId.value) return null
  return downloads.value.find((j) => j.id === modalTrackedJobId.value) ?? null
})

async function loadAll(): Promise<void> {
  loadingInit.value = true
  try {
    const [srcList, shelfList, dlList] = await Promise.all([
      onlineBookApi.listSources(),
      onlineBookApi.listShelf(),
      onlineBookApi.listDownloads(),
    ])
    sources.value = srcList
    shelfBooks.value = shelfList
    downloads.value = dlList
    if (shelfList.length === 0 && srcList.some((s) => s.enabled) && activeTab.value === 'shelf') {
      activeTab.value = 'search'
    } else if (srcList.length === 0 && authStore.isAdmin) {
      activeTab.value = 'sources'
    }
    ensureDownloadPolling()
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

function stopDownloadPolling(): void {
  if (downloadsPollTimer) {
    clearTimeout(downloadsPollTimer)
    downloadsPollTimer = null
  }
}

function ensureDownloadPolling(): void {
  if (isUnmounted) return
  if (!hasActiveDownloads.value) {
    stopDownloadPolling()
    return
  }
  if (downloadsPollTimer || downloadsPollInFlight) return
  downloadsPollTimer = setTimeout(async () => {
    downloadsPollTimer = null
    if (isUnmounted || !hasActiveDownloads.value) return
    downloadsPollInFlight = true
    try {
      downloads.value = await onlineBookApi.listDownloads()
    } catch {
      // ignore transient network errors
    } finally {
      downloadsPollInFlight = false
      if (!isUnmounted && hasActiveDownloads.value) {
        ensureDownloadPolling()
      }
    }
  }, 1200)
}

onMounted(() => {
  isUnmounted = false
  void loadAll()
})

onBeforeUnmount(() => {
  isUnmounted = true
  stopDownloadPolling()
})

function toggleSourceFilter(id: string): void {
  if (selectedSourceIds.value.includes(id)) {
    selectedSourceIds.value = selectedSourceIds.value.filter((s) => s !== id)
  } else {
    selectedSourceIds.value = [...selectedSourceIds.value, id]
  }
}

async function handleSearch(): Promise<void> {
  const kw = searchKeyword.value.trim()
  if (!kw || searching.value) return
  searching.value = true
  hasSearched.value = true
  searchErrors.value = []
  searchToasts.value = []
  try {
    const res = await onlineBookApi.search(kw, selectedSourceIds.value, 1)
    searchResults.value = res.items
    searchErrors.value = res.errors
    searchToasts.value = res.toasts
  } catch (err) {
    snackbar.show({
      message: err instanceof Error ? err.message : String(err),
      tone: 'error',
    })
  } finally {
    searching.value = false
  }
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

async function handleAddSearchResultToShelf(item: AggregatedSearchItemDto): Promise<OnlineShelfBookDto | null> {
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
    sessionStorage.setItem(
      'xmedia_online_reader_ctx',
      JSON.stringify({
        sourceId: item.source_id,
        sourceName: item.source_name,
        name: detailRes.detail.name || item.book.name,
        author: detailRes.detail.author || item.book.author,
        bookUrl: item.book.book_url,
        tocUrl: detailRes.detail.toc_url,
        coverUrl: detailRes.detail.cover_url ?? item.book.cover_url,
        intro: detailRes.detail.intro ?? item.book.intro,
        currentChapterIndex: 0,
      }),
    )
    void router.push('/book/online/read')
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
  void router.push({ path: '/book/online/read', query: { shelfId: book.id } })
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

async function handleRemoveShelf(book: OnlineShelfBookDto): Promise<void> {
  const ok = await dialogStore.confirm({
    title: '移出在线书架',
    message: `确定将《${book.name}》移出在线书架吗？`,
    confirmText: '移出',
    cancelText: '取消',
    danger: true,
  })
  if (!ok) return
  await onlineBookApi.removeFromShelf(book.id)
  shelfBooks.value = shelfBooks.value.filter((b) => b.id !== book.id)
}

function handleJobUpdated(job: OnlineDownloadJobDto): void {
  modalTrackedJobId.value = job.id
  downloads.value = [job, ...downloads.value.filter((j) => j.id !== job.id)]
  ensureDownloadPolling()
}

function closeDownloadModal(): void {
  downloadTarget.value = null
  modalTrackedJobId.value = null
}

function formatDownloadSpeed(cps?: number): string {
  if (typeof cps !== 'number' || !Number.isFinite(cps) || cps <= 0) return ''
  return `${cps.toFixed(1)} 章/秒`
}

async function handleCancelDownload(job: OnlineDownloadJobDto): Promise<void> {
  if (busyDownloadId.value === job.id) return
  busyDownloadId.value = job.id
  try {
    const updated = await onlineBookApi.cancelDownload(job.id)
    downloads.value = downloads.value.map((j) => (j.id === updated.id ? updated : j))
    if (!hasActiveDownloads.value) {
      stopDownloadPolling()
    }
  } catch (err) {
    snackbar.show({
      message: err instanceof Error ? err.message : String(err),
      tone: 'error',
    })
  } finally {
    busyDownloadId.value = null
  }
}

async function handleDeleteDownload(job: OnlineDownloadJobDto): Promise<void> {
  if (busyDownloadId.value === job.id) return
  busyDownloadId.value = job.id
  try {
    await onlineBookApi.deleteDownload(job.id)
    downloads.value = downloads.value.filter((j) => j.id !== job.id)
    if (modalTrackedJobId.value === job.id) {
      modalTrackedJobId.value = null
    }
    if (!hasActiveDownloads.value) {
      stopDownloadPolling()
    }
  } catch (err) {
    snackbar.show({
      message: err instanceof Error ? err.message : String(err),
      tone: 'error',
    })
  } finally {
    busyDownloadId.value = null
  }
}
</script>

<template>
  <div class="mx-auto max-w-6xl space-y-6 px-4 py-6 sm:px-6">
    <!-- Header + Tabs -->
    <div class="flex flex-wrap items-center justify-between gap-4">
      <div>
        <h1 class="flex items-center gap-2 text-2xl font-bold tracking-tight text-[var(--md-sys-color-on-surface)]">
          <LuGlobe class="h-6 w-6 text-[var(--md-sys-color-primary)]" />
          <span>在线书源</span>
        </h1>
        <p class="mt-1 text-xs text-[var(--md-sys-color-on-surface-variant)]">
          聚合搜书、追更与打包
        </p>
      </div>

      <div class="flex items-center gap-1 rounded-full bg-[var(--md-sys-color-surface-container)] p-1">
        <button
          type="button"
          class="md-state inline-flex items-center gap-1.5 rounded-full px-3.5 py-1.5 text-xs font-medium transition"
          :class="
            activeTab === 'shelf'
              ? 'bg-[var(--md-sys-color-secondary-container)] font-semibold text-[var(--md-sys-color-on-secondary-container)]'
              : 'text-[var(--md-sys-color-on-surface-variant)]'
          "
          @click="activeTab = 'shelf'"
        >
          <LuBookMarked class="h-3.5 w-3.5" />
          <span>在线书架 ({{ shelfBooks.length }})</span>
        </button>

        <button
          type="button"
          class="md-state inline-flex items-center gap-1.5 rounded-full px-3.5 py-1.5 text-xs font-medium transition"
          :class="
            activeTab === 'search'
              ? 'bg-[var(--md-sys-color-secondary-container)] font-semibold text-[var(--md-sys-color-on-secondary-container)]'
              : 'text-[var(--md-sys-color-on-surface-variant)]'
          "
          @click="activeTab = 'search'"
        >
          <LuSearch class="h-3.5 w-3.5" />
          <span>聚合搜书</span>
        </button>

        <button
          v-if="authStore.isAdmin"
          type="button"
          class="md-state inline-flex items-center gap-1.5 rounded-full px-3.5 py-1.5 text-xs font-medium transition"
          :class="
            activeTab === 'sources'
              ? 'bg-[var(--md-sys-color-secondary-container)] font-semibold text-[var(--md-sys-color-on-secondary-container)]'
              : 'text-[var(--md-sys-color-on-surface-variant)]'
          "
          @click="activeTab = 'sources'"
        >
          <LuSettings2 class="h-3.5 w-3.5" />
          <span>书源管理 ({{ sources.length }})</span>
        </button>
      </div>
    </div>

    <!-- Active / Recent Download Jobs Banner -->
    <div
      v-if="downloads.length > 0"
      class="md-card-filled space-y-2 p-3.5"
    >
      <div class="flex items-center justify-between text-xs font-semibold text-[var(--md-sys-color-on-surface)]">
        <span class="flex items-center gap-1.5">
          <LuDownload class="h-3.5 w-3.5 text-[var(--md-sys-color-primary)]" />
          <span>下载与打包任务 ({{ downloads.length }})</span>
        </span>
      </div>
      <div class="space-y-1.5">
        <div
          v-for="job in downloads.slice(0, 4)"
          :key="job.id"
          class="flex flex-wrap items-center justify-between gap-2 rounded-xl bg-[var(--md-sys-color-surface-container-low)] px-3 py-2 text-xs"
        >
          <div class="min-w-0 flex-1">
            <span class="font-medium text-[var(--md-sys-color-on-surface)]">{{ job.book_name }}</span>
            <span class="ml-2 text-[11px] text-[var(--md-sys-color-on-surface-variant)]">
              {{ job.format.toUpperCase() }} ·
              <template v-if="job.status === 'running' || job.status === 'pending'">
                {{ job.processed_chapters }} / {{ job.total_chapters || '?' }} 章
                <template v-if="formatDownloadSpeed(job.chapters_per_sec)">
                  · {{ formatDownloadSpeed(job.chapters_per_sec) }}
                </template>
              </template>
              <template v-else-if="job.status === 'completed'">已完成</template>
              <template v-else-if="job.status === 'cancelled'">已取消</template>
              <template v-else>失败：{{ job.error }}</template>
            </span>
          </div>
          <div class="flex shrink-0 items-center gap-1.5">
            <LuLoader2
              v-if="job.status === 'running' || job.status === 'pending'"
              class="h-3.5 w-3.5 animate-spin text-[var(--md-sys-color-primary)]"
            />
            <button
              v-if="job.status === 'running' || job.status === 'pending'"
              type="button"
              :disabled="busyDownloadId === job.id"
              class="md-btn md-btn-sm md-btn-outlined md-state"
              @click="handleCancelDownload(job)"
            >
              <LuBan class="h-3.5 w-3.5" />
              <span>取消</span>
            </button>
            <a
              v-if="job.status === 'completed'"
              :href="buildOnlineDownloadFileUrl(job.id)"
              :download="job.output_file_name || undefined"
              class="md-btn md-btn-sm md-btn-tonal md-state"
            >
              <LuDownload class="h-3.5 w-3.5" />
              <span>下载</span>
            </a>
            <button
              v-if="job.imported_book_id"
              type="button"
              class="md-btn md-btn-sm md-btn-filled md-state"
              @click="router.push(`/book/items/${job.imported_book_id}`)"
            >
              <LuBookOpen class="h-3.5 w-3.5" />
              <span>查看入库书</span>
            </button>
            <button
              v-if="job.status === 'completed' || job.status === 'failed' || job.status === 'cancelled'"
              type="button"
              :disabled="busyDownloadId === job.id"
              class="md-icon-btn md-icon-btn-sm md-state text-[var(--md-sys-color-on-surface-variant)] hover:text-[var(--md-sys-color-error)]"
              title="删除记录与文件"
              @click="handleDeleteDownload(job)"
            >
              <LuTrash2 class="h-3.5 w-3.5" />
            </button>
          </div>
        </div>
      </div>
    </div>

    <!-- TAB 1: Online Bookshelf -->
    <template v-if="activeTab === 'shelf'">
      <div
        v-if="loadingInit"
        class="flex items-center justify-center py-16 text-xs text-[var(--md-sys-color-on-surface-variant)]"
      >
        <LuLoader2 class="mr-2 h-4 w-4 animate-spin" />
        <span>正在加载在线书架…</span>
      </div>

      <div
        v-else-if="shelfBooks.length === 0"
        class="rounded-[24px] bg-[var(--md-sys-color-surface-container-low)] py-16 text-center"
      >
        <p class="text-sm font-semibold text-[var(--md-sys-color-on-surface)]">在线书架为空</p>
        <p class="mt-1 text-xs text-[var(--md-sys-color-on-surface-variant)]">
          前往「聚合搜书」搜索小说并加入书架，或让管理员先在「书源管理」导入书源
        </p>
        <button
          type="button"
          class="md-btn md-btn-sm md-btn-filled md-state mt-4"
          @click="activeTab = enabledSources.length > 0 ? 'search' : 'sources'"
        >
          <LuSearch class="h-3.5 w-3.5" />
          <span>{{ enabledSources.length > 0 ? '去搜书' : '去导入书源' }}</span>
        </button>
      </div>

      <div v-else class="grid grid-cols-1 gap-3.5 md:grid-cols-2">
        <div
          v-for="book in shelfBooks"
          :key="book.id"
          class="md-card-filled flex gap-3.5 p-3.5 transition hover:brightness-[1.02]"
        >
          <div
            class="h-28 w-20 shrink-0 cursor-pointer overflow-hidden rounded-xl bg-[var(--md-sys-color-surface-container-high)]"
            @click="openShelfReader(book)"
          >
            <img
              v-if="book.cover_url"
              :src="buildOnlineCoverUrl(book.cover_url)"
              :alt="book.name"
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
                <button
                  type="button"
                  class="truncate text-left text-sm font-semibold text-[var(--md-sys-color-on-surface)] hover:text-[var(--md-sys-color-primary)]"
                  @click="openShelfReader(book)"
                >
                  {{ book.name }}
                </button>
                <span class="shrink-0 rounded-full bg-[var(--md-sys-color-primary-container)] px-2 py-0.5 text-[10px] font-medium text-[var(--md-sys-color-on-primary-container)]">
                  {{ book.source_name }}
                </span>
              </div>
              <p class="mt-0.5 truncate text-xs text-[var(--md-sys-color-on-surface-variant)]">
                {{ book.author || '佚名' }}
              </p>
              <p class="mt-1 truncate text-[11px] text-[var(--md-sys-color-on-surface-variant)]">
                已读：{{ book.current_chapter_title || `第 ${book.current_chapter_index + 1} 章` }}
                <span v-if="book.total_chapters > 0">
                  ({{ book.current_chapter_index + 1 }} / {{ book.total_chapters }})
                </span>
              </p>
              <p
                v-if="book.last_chapter"
                class="mt-0.5 truncate text-[11px] text-[var(--md-sys-color-on-surface-variant)] opacity-80"
              >
                最新：{{ book.last_chapter }}
              </p>
            </div>

            <div class="mt-2 flex flex-wrap items-center gap-1.5">
              <button
                type="button"
                class="md-btn md-btn-sm md-btn-filled md-state"
                @click="openShelfReader(book)"
              >
                <LuBookOpen class="h-3.5 w-3.5" />
                <span>继续阅读</span>
              </button>
              <button
                type="button"
                class="md-btn md-btn-sm md-btn-tonal md-state"
                @click="openDownloadForShelf(book)"
              >
                <LuDownload class="h-3.5 w-3.5" />
                <span>下载 / 入库</span>
              </button>
              <button
                type="button"
                class="md-icon-btn md-icon-btn-sm md-state ml-auto text-red-400 hover:bg-red-500/10"
                title="移出在线书架"
                @click="handleRemoveShelf(book)"
              >
                <LuTrash2 class="h-4 w-4" />
              </button>
            </div>
          </div>
        </div>
      </div>
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
            全部已启用 ({{ enabledSources.length }})
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
          class="text-xs text-amber-500"
        >
          当前没有已启用的书源，请先在「书源管理」导入并启用至少一条书源。
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
        未找到与「{{ searchKeyword }}」匹配的书籍，可检查书源是否需要先登录或切换搜索关键词。
      </div>

      <div v-else-if="searchResults.length > 0" class="grid grid-cols-1 gap-3.5 md:grid-cols-2">
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
                <span>在线阅读</span>
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
                <span>{{ isInShelf(item) ? '已在书架' : '加入书架' }}</span>
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
                <span>下载 / 入库</span>
              </button>
            </div>
          </div>
        </div>
      </div>
    </template>

    <!-- TAB 3: Book Source Manager (Admin) -->
    <OnlineSourceManagerPanel
      v-else-if="activeTab === 'sources' && authStore.isAdmin"
      :sources="sources"
      @refresh="refreshSources"
    />

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

<script setup lang="ts">
import { computed, onMounted, ref, watch } from 'vue'
import { useRouter } from 'vue-router'
import {
  LuBookOpen,
  LuDownload,
  LuInfo,
  LuLayoutGrid,
  LuList,
  LuLoader2,
  LuSearch,
  LuSettings2,
  LuTrash2,
  LuX,
} from 'vue-icons-plus/lu'
import AppSelect, { type SelectOption } from '@/components/ui/AppSelect.vue'
import MdSegmentedButton, { type SegmentedOption } from '@/components/ui/MdSegmentedButton.vue'
import { useAuthStore } from '@/stores/auth'
import { useDialogStore } from '@/stores/dialog'
import { useOnlineDownloadsStore } from '@/stores/onlineDownloads'
import { useSnackbarStore } from '@/stores/snackbar'
import OnlineBookDetailSheet from './OnlineBookDetailSheet.vue'
import OnlineDownloadModal, { type OnlineDownloadTarget } from './OnlineDownloadModal.vue'
import OnlineShelfCard from './OnlineShelfCard.vue'
import {
  buildOnlineCoverUrl,
  onlineBookApi,
  type BookSourceSummaryDto,
  type OnlineDownloadJobDto,
  type OnlineShelfBookDto,
} from '../onlineApi'

const SHELF_VIEW_STORAGE_KEY = 'xmedia_online_shelf_view_mode'

const emit = defineEmits<{
  'count-change': [count: number]
}>()

const router = useRouter()
const authStore = useAuthStore()
const snackbar = useSnackbarStore()
const dialogStore = useDialogStore()
const onlineDownloadsStore = useOnlineDownloadsStore()

type ShelfViewMode = 'grid' | 'list'
type ShelfSortMode = 'recent' | 'title' | 'progress'

const loadingInit = ref(false)
const sources = ref<BookSourceSummaryDto[]>([])
const shelfBooks = ref<OnlineShelfBookDto[]>([])
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
const downloadTarget = ref<OnlineDownloadTarget | null>(null)
const modalTrackedJobId = ref<string | null>(null)

watch(shelfViewMode, (mode) => {
  try {
    localStorage.setItem(SHELF_VIEW_STORAGE_KEY, mode)
  } catch {
    // ignore storage quota errors
  }
})

watch(
  () => shelfBooks.value.length,
  (len) => emit('count-change', len),
  { immediate: true },
)

const enabledSources = computed(() => sources.value.filter((s) => s.enabled))

const modalTrackedJob = computed<OnlineDownloadJobDto | null>(() => {
  if (!modalTrackedJobId.value) return null
  return onlineDownloadsStore.jobs.find((j) => j.id === modalTrackedJobId.value) ?? null
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
  const opts: SelectOption[] = [{ value: 'all', label: '全部书源', badge: shelfBooks.value.length }]
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

async function loadShelf(): Promise<void> {
  loadingInit.value = true
  try {
    const [srcList, shelfList] = await Promise.all([
      onlineBookApi.listSources().catch(() => []),
      onlineBookApi.listShelf(),
    ])
    sources.value = srcList
    shelfBooks.value = shelfList
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

onMounted(() => {
  void loadShelf()
})

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

function goSearchOnline(): void {
  void router.push({ path: '/book/catalog', query: { scope: 'online' } })
}

function goManageDataSources(): void {
  void router.push({ path: '/settings', query: { tab: 'domain-datasources', domain: 'book' } })
}
</script>

<template>
  <div class="space-y-4">
    <div
      v-if="loadingInit"
      class="flex items-center justify-center py-16 text-xs text-[var(--md-sys-color-on-surface-variant)]"
    >
      <LuLoader2 class="mr-2 h-4 w-4 animate-spin" />
      <span>正在加载在线书架…</span>
    </div>

    <div
      v-else-if="shelfBooks.length === 0"
      class="rounded-[24px] bg-[var(--md-sys-color-surface-container-low)] px-6 py-16 text-center"
    >
      <p class="text-sm font-semibold text-[var(--md-sys-color-on-surface)]">在线书架为空</p>
      <p class="mt-1 text-xs text-[var(--md-sys-color-on-surface-variant)]">
        通过顶栏搜索在线书源加入书架，或在「设置 → 数据源」导入书源
      </p>
      <div class="mt-4 flex flex-wrap justify-center gap-2">
        <button
          v-if="enabledSources.length > 0 || !authStore.isAdmin"
          type="button"
          class="md-btn md-btn-sm md-btn-filled md-state"
          @click="goSearchOnline"
        >
          <LuSearch class="h-3.5 w-3.5" />
          <span>去搜书</span>
        </button>
        <button
          v-if="authStore.isAdmin"
          type="button"
          class="md-btn md-btn-sm md-state"
          :class="enabledSources.length === 0 ? 'md-btn-filled' : 'md-btn-tonal'"
          @click="goManageDataSources"
        >
          <LuSettings2 class="h-3.5 w-3.5" />
          <span>管理书源</span>
        </button>
      </div>
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
            <AppSelect v-model="shelfSourceFilter" :options="shelfSourceOptions" size="sm" />
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
        <p class="text-xs text-[var(--md-sys-color-on-surface-variant)]">没有匹配的书籍</p>
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

<script setup lang="ts">
import { computed, onMounted, ref, watch } from 'vue'
import { useRoute, useRouter } from 'vue-router'
import { useBookI18n as useI18n } from '@/domains/book/i18n'
import {
  LuLayoutGrid,
  LuList,
  LuSlidersHorizontal,
  LuArrowUpDown,
  LuHeart,
  LuTag,
  LuBookOpen,
  LuChevronLeft,
  LuChevronRight,
  LuGlobe,
  LuX,
} from 'vue-icons-plus/lu'
import { api } from '@/domains/book/api'
import { useAuthStore } from '@/stores/auth'
import { useCatalogStore, type NavSection } from '@/domains/book/store'
import { useDialogStore } from '@/stores/dialog'
import { useSnackbarStore } from '@/stores/snackbar'
import type { BookFormat, BookSortField, BookSummaryDto } from '@/domains/book/types'
import BookCard from '@/domains/book/components/BookCard.vue'
import OnlineBookSearchSection from '@/domains/book/components/OnlineBookSearchSection.vue'
import AppSelect from '@/components/ui/AppSelect.vue'
import AppMultiSelect from '@/components/ui/AppMultiSelect.vue'
import AppCheckbox from '@/components/ui/AppCheckbox.vue'
import MdSegmentedButton, { type SegmentedOption } from '@/components/ui/MdSegmentedButton.vue'
import RefreshButton from '@/components/ui/RefreshButton.vue'
import { formatBytes } from '@/utils/format'

type CatalogSearchScope = 'local' | 'online'

const { t } = useI18n()
const route = useRoute()
const router = useRouter()
const authStore = useAuthStore()
const catalogStore = useCatalogStore()
const dialogStore = useDialogStore()
const snackbarStore = useSnackbarStore()

const showFilters = ref(false)
const searchScope = ref<CatalogSearchScope>(route.query.scope === 'online' ? 'online' : 'local')
const onlineSourceId = computed(() =>
  typeof route.query.source === 'string' && route.query.source.trim()
    ? route.query.source.trim()
    : null,
)
let syncingRouteQuery = false

const showSearchScopeSwitch = computed(
  () => catalogStore.keyword.trim().length > 0 || searchScope.value === 'online',
)

const searchScopeOptions = computed<SegmentedOption<CatalogSearchScope>[]>(() => [
  { value: 'local', label: t('catalog.scopeLocal'), icon: LuBookOpen },
  { value: 'online', label: t('catalog.scopeOnline'), icon: LuGlobe },
])

function buildQueryFromStore(): Record<string, string> {
  const q: Record<string, string> = {}
  if (searchScope.value === 'online') {
    q.scope = 'online'
    if (onlineSourceId.value) {
      q.source = onlineSourceId.value
    }
  }
  if (catalogStore.activeSection !== 'all') {
    q.section = catalogStore.activeSection
  }
  if (catalogStore.activeSection === 'library' && catalogStore.activeLibraryId) {
    q.library = catalogStore.activeLibraryId
  }
  if (catalogStore.activeSection === 'shelf' && catalogStore.activeShelfId) {
    q.shelf = catalogStore.activeShelfId
  }
  if (catalogStore.keyword.trim()) {
    q.q = catalogStore.keyword.trim()
  }
  if (catalogStore.filterAuthors.length === 1) {
    q.author = catalogStore.filterAuthors[0]!
  }
  if (catalogStore.filterTags.length === 1) {
    q.tag = catalogStore.filterTags[0]!
  }
  if (catalogStore.filterFormats.length === 1) {
    q.format = catalogStore.filterFormats[0]!
  }
  if (catalogStore.sortBy !== 'created_at') {
    q.sort_by = catalogStore.sortBy
  }
  if (catalogStore.sortOrder !== 'desc') {
    q.sort_order = catalogStore.sortOrder
  }
  if (catalogStore.page > 1) {
    q.page = String(catalogStore.page)
  }
  return q
}

function syncStoreToRouteQuery(): void {
  const next = buildQueryFromStore()
  const cur = route.query
  const same =
    Object.keys(next).length === Object.keys(cur).length &&
    Object.entries(next).every(([k, v]) => cur[k] === v)
  if (same) return
  syncingRouteQuery = true
  void router.replace({ path: '/book/catalog', query: next }).finally(() => {
    syncingRouteQuery = false
  })
}

function applyQueryParamsFromRoute(): void {
  searchScope.value = route.query.scope === 'online' ? 'online' : 'local'
  const qSection = typeof route.query.section === 'string' ? route.query.section : ''
  const qLibrary = typeof route.query.library === 'string' ? route.query.library : ''
  const qShelf = typeof route.query.shelf === 'string' ? route.query.shelf : ''
  const qKeyword = typeof route.query.q === 'string' ? route.query.q : ''
  const qAuthor = typeof route.query.author === 'string' ? route.query.author : ''
  const qTag = typeof route.query.tag === 'string' ? route.query.tag : ''
  const qFormat = typeof route.query.format === 'string' ? route.query.format : ''
  const qSortBy = typeof route.query.sort_by === 'string' ? route.query.sort_by : ''
  const qSortOrder = typeof route.query.sort_order === 'string' ? route.query.sort_order : ''
  const qPage = typeof route.query.page === 'string' ? Number.parseInt(route.query.page, 10) : NaN

  if (
    qSection === 'all' ||
    qSection === 'recent' ||
    qSection === 'favorites' ||
    qSection === 'library' ||
    qSection === 'shelf' ||
    qSection === 'tag' ||
    qSection === 'author'
  ) {
    catalogStore.activeSection = qSection as NavSection
  } else if (qLibrary) {
    catalogStore.activeSection = 'library'
  } else if (qShelf) {
    catalogStore.activeSection = 'shelf'
  } else {
    catalogStore.activeSection = 'all'
  }

  catalogStore.activeLibraryId = qLibrary
  catalogStore.activeShelfId = qShelf
  catalogStore.keyword = qKeyword
  catalogStore.filterAuthors = qAuthor ? [qAuthor] : []
  catalogStore.filterTags = qTag ? [qTag] : []
  catalogStore.filterFormats =
    qFormat === 'epub' || qFormat === 'txt' || qFormat === 'pdf' ? [qFormat] : []
  catalogStore.sortBy =
    qSortBy === 'created_at' ||
    qSortBy === 'updated_at' ||
    qSortBy === 'title' ||
    qSortBy === 'file_size'
      ? qSortBy
      : 'created_at'
  catalogStore.sortOrder = qSortOrder === 'asc' || qSortOrder === 'desc' ? qSortOrder : 'desc'
  catalogStore.page = Number.isFinite(qPage) && qPage >= 1 ? qPage : 1
}

onMounted(() => {
  applyQueryParamsFromRoute()
  if (searchScope.value !== 'online') {
    void catalogStore.fetchBooks()
  }
})

watch(
  () => route.query,
  () => {
    if (syncingRouteQuery) return
    applyQueryParamsFromRoute()
    if (searchScope.value !== 'online') {
      void catalogStore.fetchBooks()
    }
  },
)

const totalPages = computed(() =>
  Math.max(1, Math.ceil(catalogStore.total / catalogStore.pageSize)),
)

const allCurrentSelected = computed(
  () =>
    catalogStore.books.length > 0 &&
    catalogStore.books.every((b) => catalogStore.selectedBookIds.has(b.id)),
)

const sortOptions = computed(() => [
  { value: 'created_at', label: t('catalog.sortCreatedAt') },
  { value: 'updated_at', label: t('catalog.sortUpdatedAt') },
  { value: 'title', label: t('catalog.sortTitle') },
  { value: 'file_size', label: t('catalog.sortFileSize') },
])

const formatOptions = computed(() => [
  { value: 'epub', label: 'EPUB' },
  { value: 'txt', label: 'TXT' },
  { value: 'pdf', label: 'PDF' },
])

const authorOptions = computed(() =>
  catalogStore.authors.map((a) => ({
    value: a.name,
    label: a.name,
    badge: a.count,
  })),
)

const tagOptions = computed(() =>
  catalogStore.tags.map((tg) => ({
    value: tg.name,
    label: tg.name,
    badge: tg.count,
  })),
)

const libraryOptions = computed(() =>
  catalogStore.libraries.map((lib) => ({
    value: lib.id,
    label: lib.name,
    badge: lib.book_count,
  })),
)

const hasFilterPills = computed(
  () =>
    catalogStore.filterFormats.length > 0 ||
    catalogStore.filterAuthors.length > 0 ||
    catalogStore.filterLibraryIds.length > 0 ||
    catalogStore.filterTags.length > 0 ||
    catalogStore.includeMissing,
)

function removeFormatChip(fmt: BookFormat): void {
  catalogStore.filterFormats = catalogStore.filterFormats.filter((f) => f !== fmt)
  triggerRefetch()
}

function removeAuthorChip(author: string): void {
  catalogStore.filterAuthors = catalogStore.filterAuthors.filter((a) => a !== author)
  triggerRefetch()
}

function removeTagChip(tag: string): void {
  catalogStore.filterTags = catalogStore.filterTags.filter((tg) => tg !== tag)
  triggerRefetch()
}

function removeLibraryChip(libId: string): void {
  catalogStore.filterLibraryIds = catalogStore.filterLibraryIds.filter((id) => id !== libId)
  triggerRefetch()
}

function resolveLibraryName(libId: string): string {
  return catalogStore.libraries.find((l) => l.id === libId)?.name ?? libId
}

function toggleSelectAll(): void {
  if (allCurrentSelected.value) {
    catalogStore.clearSelection()
  } else {
    catalogStore.selectedBookIds = new Set(catalogStore.books.map((b) => b.id))
  }
}

async function toggleBookFavorite(book: BookSummaryDto): Promise<void> {
  const next = !book.is_favorite
  try {
    await api.setFavorite(book.id, next)
    catalogStore.updateBookInList({ ...book, is_favorite: next })
  } catch (err) {
    snackbarStore.show({
      message: err instanceof Error ? err.message : String(err),
      tone: 'error',
    })
  }
}

async function batchFavorite(fav: boolean): Promise<void> {
  const ids = Array.from(catalogStore.selectedBookIds)
  if (ids.length === 0) return
  try {
    await api.batchBooks({ book_ids: ids, set_favorite: fav })
    catalogStore.clearSelection()
    await catalogStore.fetchBooks()
  } catch (err) {
    snackbarStore.show({
      message: err instanceof Error ? err.message : String(err),
      tone: 'error',
    })
  }
}

async function batchAddTag(): Promise<void> {
  const ids = Array.from(catalogStore.selectedBookIds)
  if (ids.length === 0) return
  const raw = await dialogStore.prompt({
    title: t('catalog.batchAddTag'),
    message: t('catalog.batchTagPrompt'),
  })
  if (!raw) return
  const addTags = raw
    .split(/[,，]/)
    .map((s) => s.trim())
    .filter(Boolean)
  if (addTags.length === 0) return
  try {
    await api.batchBooks({ book_ids: ids, add_tags: addTags })
    catalogStore.clearSelection()
    await Promise.all([catalogStore.refreshSidebarData(), catalogStore.fetchBooks()])
  } catch (err) {
    snackbarStore.show({
      message: err instanceof Error ? err.message : String(err),
      tone: 'error',
    })
  }
}

function handleResetFilters(): void {
  catalogStore.resetFilters()
  syncStoreToRouteQuery()
}

function triggerRefetch(): void {
  catalogStore.page = 1
  syncStoreToRouteQuery()
  void catalogStore.fetchBooks()
}

function goPage(nextPage: number): void {
  if (nextPage < 1 || nextPage > totalPages.value) return
  catalogStore.page = nextPage
  syncStoreToRouteQuery()
  void catalogStore.fetchBooks()
}

function switchSearchScope(next: CatalogSearchScope): void {
  searchScope.value = next
  syncStoreToRouteQuery()
  if (next === 'local') {
    void catalogStore.fetchBooks()
  }
}

function handleOnlineKeywordUpdate(nextKw: string): void {
  catalogStore.keyword = nextKw
  syncStoreToRouteQuery()
}

async function refreshPage(): Promise<void> {
  await Promise.all([catalogStore.refreshSidebarData(), catalogStore.fetchBooks()])
}
</script>

<template>
  <div class="mx-auto max-w-7xl px-6 py-7 sm:px-8">
    <!-- Local vs Online Book Source Search Switcher -->
    <div
      v-if="showSearchScopeSwitch"
      class="mb-5 flex flex-wrap items-center justify-between gap-3"
    >
      <MdSegmentedButton
        :model-value="searchScope"
        :options="searchScopeOptions"
        size="sm"
        :show-check="false"
        @update:model-value="switchSearchScope($event)"
      />
    </div>

    <OnlineBookSearchSection
      v-if="searchScope === 'online'"
      :keyword="catalogStore.keyword"
      :initial-source-id="onlineSourceId"
      @update:keyword="handleOnlineKeywordUpdate"
    />

    <template v-else>
    <!-- Top Filter & View Controls Bar -->
    <div class="mb-6 space-y-3">
      <div class="flex flex-wrap items-center justify-between gap-3">
        <!-- Left: Sort & Filter Pill Triggers + Active Filter Chips -->
        <div class="flex flex-wrap items-center gap-2">
          <div class="inline-flex h-8 items-center gap-1.5 rounded-full bg-[var(--md-sys-color-surface-container)] px-3 text-xs font-medium text-[var(--md-sys-color-on-surface-variant)]">
            <LuArrowUpDown class="h-3.5 w-3.5 text-[var(--md-sys-color-on-surface-variant)]" />
            <AppSelect
              :model-value="catalogStore.sortBy"
              :options="sortOptions"
              variant="inline"
              @update:model-value="(val) => { catalogStore.sortBy = val as BookSortField; triggerRefetch() }"
            />
            <button
              type="button"
              class="ml-1 rounded px-1 text-[11px] font-semibold text-[var(--md-sys-color-primary)] hover:bg-[var(--md-sys-color-surface-container-highest)]"
              @click="
                catalogStore.sortOrder = catalogStore.sortOrder === 'desc' ? 'asc' : 'desc';
                triggerRefetch()
              "
            >
              {{ catalogStore.sortOrder === 'desc' ? '↓' : '↑' }}
            </button>
          </div>

          <button
            type="button"
            class="md-chip md-state !rounded-full"
            :class="showFilters || hasFilterPills ? 'md-chip-selected' : ''"
            @click="showFilters = !showFilters"
          >
            <LuSlidersHorizontal class="h-3.5 w-3.5" />
            <span>{{ t('search.filters') }}</span>
          </button>

          <!-- Active Removable Multi-Select Filter Chips -->
          <button
            v-for="fmt in catalogStore.filterFormats"
            :key="`fmt-${fmt}`"
            type="button"
            class="md-chip md-chip-selected md-state !h-7 !rounded-full !px-2.5"
            @click="removeFormatChip(fmt)"
          >
            <span>{{ fmt.toUpperCase() }}</span>
            <LuX class="h-3 w-3" />
          </button>

          <button
            v-for="libId in catalogStore.filterLibraryIds"
            :key="`lib-${libId}`"
            type="button"
            class="md-chip md-chip-selected md-state !h-7 !rounded-full !px-2.5"
            @click="removeLibraryChip(libId)"
          >
            <span>{{ resolveLibraryName(libId) }}</span>
            <LuX class="h-3 w-3" />
          </button>

          <button
            v-for="author in catalogStore.filterAuthors"
            :key="`au-${author}`"
            type="button"
            class="md-chip md-chip-selected md-state !h-7 !rounded-full !px-2.5"
            @click="removeAuthorChip(author)"
          >
            <span>{{ author }}</span>
            <LuX class="h-3 w-3" />
          </button>

          <button
            v-if="catalogStore.filterTags.length > 1"
            type="button"
            class="md-chip md-state !h-7 !rounded-full !border-[var(--md-sys-color-primary)]/50 !px-2 !text-[11px] font-bold !text-[var(--md-sys-color-primary)]"
            @click="
              catalogStore.filterTagMode = catalogStore.filterTagMode === 'any' ? 'all' : 'any';
              triggerRefetch()
            "
          >
            {{ catalogStore.filterTagMode === 'all' ? t('search.tagModeAll') : t('search.tagModeAny') }}
          </button>

          <button
            v-for="tg in catalogStore.filterTags"
            :key="`tg-${tg}`"
            type="button"
            class="md-chip md-chip-selected md-state !h-7 !rounded-full !px-2.5"
            @click="removeTagChip(tg)"
          >
            <span>#{{ tg }}</span>
            <LuX class="h-3 w-3" />
          </button>
        </div>

        <!-- Right: Count + Refresh + Grid/Table Switcher -->
        <div class="flex items-center gap-2.5">
          <span class="text-xs text-[var(--md-sys-color-on-surface-variant)]">
            {{ t('catalog.totalBooks', { count: catalogStore.total }) }}
          </span>

          <RefreshButton :action="refreshPage" :busy="catalogStore.loading" />

          <div class="flex items-center rounded-full bg-[var(--md-sys-color-surface-container)] p-0.5">
            <button
              type="button"
              class="md-state flex h-7 w-8 items-center justify-center rounded-full transition"
              :class="
                catalogStore.viewMode === 'grid'
                  ? 'bg-[var(--md-sys-color-secondary-container)] text-[var(--md-sys-color-on-secondary-container)]'
                  : 'text-[var(--md-sys-color-on-surface-variant)] hover:text-[var(--md-sys-color-on-surface)]'
              "
              :title="t('catalog.gridView')"
              @click="catalogStore.setViewMode('grid')"
            >
              <LuLayoutGrid class="h-4 w-4" />
            </button>
            <button
              type="button"
              class="md-state flex h-7 w-8 items-center justify-center rounded-full transition"
              :class="
                catalogStore.viewMode === 'list'
                  ? 'bg-[var(--md-sys-color-secondary-container)] text-[var(--md-sys-color-on-secondary-container)]'
                  : 'text-[var(--md-sys-color-on-surface-variant)] hover:text-[var(--md-sys-color-on-surface)]'
              "
              :title="t('catalog.tableView')"
              @click="catalogStore.setViewMode('list')"
            >
              <LuList class="h-4 w-4" />
            </button>
          </div>
        </div>
      </div>

      <!-- Expandable Multi-Select Filter Drawer Bar -->
      <div
        v-if="showFilters"
        class="md-card-filled space-y-3 p-4"
      >
        <div
          class="grid grid-cols-1 gap-3"
          :class="catalogStore.activeSection === 'library' ? 'sm:grid-cols-3' : 'sm:grid-cols-4'"
        >
          <AppMultiSelect
            :model-value="catalogStore.filterFormats"
            :options="formatOptions"
            :label="t('search.format')"
            :all-placeholder="t('search.allFormats')"
            @update:model-value="(vals) => { catalogStore.filterFormats = vals as BookFormat[]; triggerRefetch() }"
          />

          <AppMultiSelect
            :model-value="catalogStore.filterAuthors"
            :options="authorOptions"
            :label="t('search.author')"
            :all-placeholder="t('search.allAuthors')"
            @update:model-value="(vals) => { catalogStore.filterAuthors = vals; triggerRefetch() }"
          />

          <AppMultiSelect
            :model-value="catalogStore.filterTags"
            :options="tagOptions"
            :label="t('search.tag')"
            :all-placeholder="t('search.allTags')"
            @update:model-value="(vals) => { catalogStore.filterTags = vals; triggerRefetch() }"
          >
            <template #header>
              <div class="grid grid-cols-2 gap-1 rounded-full bg-[var(--md-sys-color-surface-container)] p-0.5 text-[11px] font-semibold">
                <button
                  type="button"
                  class="rounded-full py-1 transition"
                  :class="
                    catalogStore.filterTagMode === 'any'
                      ? 'bg-[var(--md-sys-color-secondary-container)] text-[var(--md-sys-color-on-secondary-container)]'
                      : 'text-[var(--md-sys-color-on-surface-variant)] hover:text-[var(--md-sys-color-on-surface)]'
                  "
                  @click="catalogStore.filterTagMode = 'any'; triggerRefetch()"
                >
                  {{ t('search.tagModeAny') }}
                </button>
                <button
                  type="button"
                  class="rounded-full py-1 transition"
                  :class="
                    catalogStore.filterTagMode === 'all'
                      ? 'bg-[var(--md-sys-color-secondary-container)] text-[var(--md-sys-color-on-secondary-container)]'
                      : 'text-[var(--md-sys-color-on-surface-variant)] hover:text-[var(--md-sys-color-on-surface)]'
                  "
                  @click="catalogStore.filterTagMode = 'all'; triggerRefetch()"
                >
                  {{ t('search.tagModeAll') }}
                </button>
              </div>
            </template>
          </AppMultiSelect>

          <AppMultiSelect
            v-if="catalogStore.activeSection !== 'library'"
            :model-value="catalogStore.filterLibraryIds"
            :options="libraryOptions"
            :label="t('search.library')"
            :all-placeholder="t('search.allLibraries')"
            @update:model-value="(vals) => { catalogStore.filterLibraryIds = vals; triggerRefetch() }"
          />
        </div>

        <div class="flex items-center justify-between border-t border-[var(--md-sys-color-outline-variant)]/55 pt-2.5">
          <AppCheckbox
            v-model="catalogStore.includeMissing"
            :label="t('search.includeMissing')"
            @change="triggerRefetch"
          />
          <button
            v-if="hasFilterPills"
            type="button"
            class="text-xs font-semibold text-[var(--md-sys-color-primary)] hover:underline"
            @click="handleResetFilters"
          >
            {{ t('search.reset') }}
          </button>
        </div>
      </div>

      <!-- Selection Bulk Toolbar -->
      <div
        v-if="catalogStore.selectedBookIds.size > 0"
        class="flex flex-wrap items-center justify-between gap-2 rounded-2xl bg-[var(--md-sys-color-secondary-container)] px-4 py-2 text-xs text-[var(--md-sys-color-on-secondary-container)]"
      >
        <span class="font-semibold">
          {{ t('catalog.selectedCount', { count: catalogStore.selectedBookIds.size }) }}
        </span>
        <div class="flex flex-wrap items-center gap-1.5">
          <button
            type="button"
            class="md-btn md-btn-sm md-state bg-[var(--md-sys-color-surface)] text-[var(--md-sys-color-on-surface)] shadow-sm"
            @click="batchFavorite(true)"
          >
            <LuHeart class="h-3.5 w-3.5 text-[var(--md-sys-color-error)]" />
            <span>{{ t('catalog.batchStar') }}</span>
          </button>
          <button
            type="button"
            class="md-btn md-btn-sm md-state bg-[var(--md-sys-color-surface)] text-[var(--md-sys-color-on-surface-variant)] shadow-sm hover:text-[var(--md-sys-color-on-surface)]"
            @click="batchFavorite(false)"
          >
            <span>{{ t('catalog.batchUnstar') }}</span>
          </button>
          <button
            v-if="authStore.isAdmin"
            type="button"
            class="md-btn md-btn-sm md-state bg-[var(--md-sys-color-surface)] text-[var(--md-sys-color-on-surface)] shadow-sm"
            @click="batchAddTag"
          >
            <LuTag class="h-3.5 w-3.5 text-[var(--md-sys-color-primary)]" />
            <span>{{ t('catalog.batchAddTag') }}</span>
          </button>
          <button
            type="button"
            class="md-btn md-btn-sm md-btn-text md-state !text-[var(--md-sys-color-on-secondary-container)]"
            @click="catalogStore.clearSelection()"
          >
            {{ t('catalog.clearSelection') }}
          </button>
        </div>
      </div>
    </div>

    <!-- Empty State -->
    <div
      v-if="!catalogStore.loading && catalogStore.books.length === 0"
      class="md-card-filled flex flex-col items-center justify-center rounded-3xl py-20 text-center"
    >
      <div class="mb-4 flex h-14 w-14 items-center justify-center rounded-2xl bg-[var(--md-sys-color-primary-container)] text-[var(--md-sys-color-on-primary-container)]">
        <LuBookOpen class="h-7 w-7" />
      </div>
      <h3 class="text-base font-bold">{{ t('catalog.emptyTitle') }}</h3>
      <p class="mt-1 max-w-sm text-xs text-[var(--md-sys-color-on-surface-variant)]">{{ t('catalog.emptyHint') }}</p>
    </div>

    <!-- Cover Grid View -->
    <div
      v-else-if="catalogStore.viewMode === 'grid'"
      class="grid grid-cols-2 gap-6 sm:grid-cols-3 md:grid-cols-4 lg:grid-cols-5 xl:grid-cols-6"
    >
      <BookCard
        v-for="book in catalogStore.books"
        :key="book.id"
        :book="book"
      />
    </div>

    <!-- Clean Data Table View -->
    <div
      v-else
      class="md-card-filled overflow-hidden"
    >
      <table class="w-full border-collapse text-left text-sm">
        <thead>
          <tr class="border-b border-[var(--md-sys-color-outline-variant)]/55 text-xs text-[var(--md-sys-color-on-surface-variant)]">
            <th class="w-10 py-3 pl-4">
              <AppCheckbox
                :model-value="allCurrentSelected"
                @update:model-value="toggleSelectAll"
              />
            </th>
            <th class="w-8 py-3" />
            <th class="py-3 pr-4 font-medium">{{ t('catalog.colTitle') }}</th>
            <th class="hidden py-3 pr-4 font-medium md:table-cell">{{ t('catalog.colAuthor') }}</th>
            <th class="hidden py-3 pr-4 font-medium sm:table-cell">{{ t('catalog.colProgress') }}</th>
            <th class="py-3 pr-4 font-medium">{{ t('catalog.colFormat') }}</th>
            <th class="hidden py-3 pr-4 text-right font-medium sm:table-cell">{{ t('catalog.colSize') }}</th>
          </tr>
        </thead>
        <tbody class="divide-y divide-[var(--md-sys-color-outline-variant)]/40">
          <tr
            v-for="book in catalogStore.books"
            :key="book.id"
            class="group cursor-pointer transition hover:bg-[var(--md-sys-color-surface-container-high)]/65"
            @click="router.push(`/book/items/${book.id}`)"
          >
            <td class="py-2.5 pl-4" @click.stop>
              <AppCheckbox
                :model-value="catalogStore.selectedBookIds.has(book.id)"
                @update:model-value="catalogStore.toggleSelectBook(book.id)"
              />
            </td>
            <td class="py-2.5 pr-2" @click.stop>
              <button
                type="button"
                class="text-[var(--md-sys-color-on-surface-variant)] transition hover:scale-110"
                :class="book.is_favorite ? '!text-[var(--md-sys-color-error)]' : ''"
                @click="toggleBookFavorite(book)"
              >
                <LuHeart class="h-4 w-4" :class="book.is_favorite ? 'fill-current' : ''" />
              </button>
            </td>
            <td class="py-2.5 pr-4">
              <div class="flex items-center gap-3">
                <img
                  v-if="book.has_cover"
                  :src="api.coverUrl(book.id, book.updated_at)"
                  alt=""
                  class="h-10 w-7 shrink-0 rounded object-cover shadow-sm"
                />
                <div class="min-w-0">
                  <p class="truncate font-semibold text-[var(--md-sys-color-on-surface)] group-hover:text-[var(--md-sys-color-primary)]">
                    {{ book.metadata.title }}
                  </p>
                  <div v-if="book.metadata.tags.length > 0" class="mt-0.5 flex gap-1">
                    <span
                      v-for="tg in book.metadata.tags.slice(0, 2)"
                      :key="tg"
                      class="rounded bg-[var(--md-sys-color-surface-container-high)] px-1.5 py-0.5 text-[10px] text-[var(--md-sys-color-on-surface-variant)]"
                    >
                      {{ tg }}
                    </span>
                  </div>
                </div>
              </div>
            </td>
            <td class="hidden py-2.5 pr-4 text-xs text-[var(--md-sys-color-on-surface-variant)] md:table-cell">
              {{ book.metadata.authors.join(', ') || t('catalog.unknownAuthor') }}
            </td>
            <td class="hidden py-2.5 pr-4 text-xs sm:table-cell">
              <span v-if="book.progress" class="font-medium text-[var(--md-sys-color-primary)]">
                {{ Math.round(book.progress.progress_percent) }}%
              </span>
              <span v-else class="text-[var(--md-sys-color-on-surface-variant)]">—</span>
            </td>
            <td class="py-2.5 pr-4">
              <span class="rounded-md bg-[var(--md-sys-color-surface-container-high)] px-2 py-0.5 text-[10px] font-bold uppercase text-[var(--md-sys-color-on-surface-variant)]">
                {{ book.format }}
              </span>
            </td>
            <td class="hidden py-2.5 pr-4 text-right text-xs text-[var(--md-sys-color-on-surface-variant)] sm:table-cell">
              {{ formatBytes(book.file_size) }}
            </td>
          </tr>
        </tbody>
      </table>
    </div>

    <!-- Bottom Pagination -->
    <div
      v-if="catalogStore.activeSection !== 'recent' && totalPages > 1"
      class="mt-8 flex items-center justify-center gap-3"
    >
      <button
        type="button"
        :disabled="catalogStore.page <= 1"
        class="md-btn md-btn-sm md-btn-outlined md-state"
        @click="goPage(catalogStore.page - 1)"
      >
        <LuChevronLeft class="h-4 w-4" />
        <span>{{ t('catalog.prevPage') }}</span>
      </button>
      <span class="text-xs text-[var(--md-sys-color-on-surface-variant)]">
        {{ t('catalog.pageInfo', { page: catalogStore.page, totalPages }) }}
      </span>
      <button
        type="button"
        :disabled="catalogStore.page >= totalPages"
        class="md-btn md-btn-sm md-btn-outlined md-state"
        @click="goPage(catalogStore.page + 1)"
      >
        <span>{{ t('catalog.nextPage') }}</span>
        <LuChevronRight class="h-4 w-4" />
      </button>
    </div>
    </template>
  </div>
</template>

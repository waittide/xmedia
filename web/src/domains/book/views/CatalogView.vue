<script setup lang="ts">
import { computed, onMounted, ref, watch } from 'vue'
import { useI18n as useGlobalI18n } from 'vue-i18n'
import { useRoute, useRouter } from 'vue-router'
import {
  LuArrowDown,
  LuArrowUp,
  LuLayoutGrid,
  LuList,
  LuPlus,
  LuRefreshCw,
  LuTag,
  LuUpload,
  LuUser,
} from 'vue-icons-plus/lu'
import { DomainLibraryTemplate, DomainPoolSection, MediaItemCard } from '@/components/domain'
import RemoteOpdsBrowser from '@/components/remote/RemoteOpdsBrowser.vue'
import {
  MdButton,
  MdChip,
  MdIconButton,
  MdSegmentedButton,
  MdSelect,
  type MdTabItem,
  type SegmentedOption,
} from '@/components/ui'
import { api } from '@/domains/book/api'
import BookCatalogBooksGrid from '@/domains/book/components/catalog/BookCatalogBooksGrid.vue'
import BookCatalogContinueSection from '@/domains/book/components/catalog/BookCatalogContinueSection.vue'
import BookCatalogFilterSheet from '@/domains/book/components/catalog/BookCatalogFilterSheet.vue'
import BookCatalogListTable from '@/domains/book/components/catalog/BookCatalogListTable.vue'
import BookCatalogSelectionBar from '@/domains/book/components/catalog/BookCatalogSelectionBar.vue'
import BookCatalogShelvesSection from '@/domains/book/components/catalog/BookCatalogShelvesSection.vue'
import {
  parseBookTab,
  useBookCatalogQuerySync,
  type BookBrowseView,
} from '@/domains/book/components/catalog/useBookCatalogQuerySync'
import { useBookShelfActions } from '@/domains/book/components/catalog/useBookShelfActions'
import OnlineBookSearchSection from '@/domains/book/components/OnlineBookSearchSection.vue'
import { useBookI18n as useI18n } from '@/domains/book/i18n'
import { useCatalogStore } from '@/domains/book/store'
import type {
  BookFormat,
  BookSortField,
  BookSummaryDto,
  ShelfDto,
} from '@/domains/book/types'
import {
  useMergedShelf,
  useSourceFilter,
  type LibraryTab,
  type SourceScope,
} from '@/sources'
import { useAppClientStore } from '@/stores/appServers'
import { useAuthStore } from '@/stores/auth'
import { useSnackbarStore } from '@/stores/snackbar'
import { useUploadStore } from '@/stores/uploads'

const { t } = useI18n()
const { t: globalT } = useGlobalI18n()
const route = useRoute()
const router = useRouter()
const appClientStore = useAppClientStore()
const authStore = useAuthStore()
const catalogStore = useCatalogStore()
const snackbarStore = useSnackbarStore()
const uploadStore = useUploadStore()

const sourceFilter = useSourceFilter('book')
const activeScope = computed<SourceScope>(() => sourceFilter.filter.value.scope)
const onlineSourceId = computed(() =>
  activeScope.value === 'online' ? sourceFilter.filter.value.sourceId : null,
)

const activeTab = ref<LibraryTab>('all')
const activeView = ref<BookBrowseView>('items')
const activeCollectionId = ref('')
const searchInput = ref('')
const filterSheetOpen = ref(false)
const manualRefreshing = ref(false)
const localError = ref<string | null>(null)

const uploadInputRef = ref<HTMLInputElement | null>(null)
const onlineSearchRef = ref<InstanceType<typeof OnlineBookSearchSection> | null>(null)

const isLocalScope = computed(() => activeScope.value === 'local')
const isPoolScope = computed(() => activeScope.value === 'pool')
const isOnlineScope = computed(() => activeScope.value === 'online')
const isRemoteScope = computed(() => activeScope.value === 'remote')

const { isSyncing, syncRouteQuery, applyRouteQuery } = useBookCatalogQuerySync({
  activeScope,
  getSourceId: () => sourceFilter.filter.value.sourceId,
  activeTab,
  activeView,
  activeCollectionId,
  searchInput,
})

const continueShelf = useMergedShelf('book', 'continue', {
  scope: 'all',
  query: () => (activeTab.value === 'continue' ? searchInput.value : ''),
  autoLoad: false,
})

const favoritesShelf = useMergedShelf('book', 'favorites', {
  scope: 'all',
  query: () => (activeTab.value === 'favorites' ? searchInput.value : ''),
  autoLoad: false,
})

const selectedRemoteSourceId = computed(() => {
  const fromQuery =
    activeScope.value === 'remote' ? sourceFilter.filter.value.sourceId : null
  if (fromQuery && appClientStore.opdsSources.some((s) => s.id === fromQuery)) {
    return fromQuery
  }
  return appClientStore.opdsSources[0]?.id ?? ''
})

async function loadActiveLocalData(): Promise<void> {
  if (!isLocalScope.value) return
  localError.value = null
  try {
    if (activeTab.value === 'continue') {
      await Promise.all([continueShelf.refresh(), catalogStore.refreshSidebarData()])
      return
    }
    if (activeTab.value === 'favorites') {
      await favoritesShelf.refresh()
      return
    }
    if (activeTab.value === 'collections' && !activeCollectionId.value) {
      await catalogStore.refreshSidebarData()
      return
    }
    if (activeView.value === 'authors' || activeView.value === 'tags') {
      await catalogStore.refreshSidebarData()
      return
    }
    await catalogStore.fetchBooks()
  } catch (err) {
    localError.value = err instanceof Error ? err.message : String(err)
  }
}

onMounted(() => {
  applyRouteQuery()
  void catalogStore.refreshSidebarData().catch(() => {})
  if (isLocalScope.value) {
    void loadActiveLocalData()
  }
})

watch(
  () => route.query,
  () => {
    if (isSyncing()) return
    applyRouteQuery()
    if (isLocalScope.value) {
      void loadActiveLocalData()
    }
  },
)

watch(
  () => [uploadStore.versionCounter, catalogStore.scanRefreshVersion],
  () => {
    if (isLocalScope.value) {
      void loadActiveLocalData()
    }
  },
)

const {
  createShelf: handleCreateShelf,
  renameShelf: handleRenameShelf,
  deleteShelf: handleDeleteShelf,
} = useBookShelfActions({
  onShelfDeleted: (shelfId) => {
    if (activeCollectionId.value === shelfId) {
      activeCollectionId.value = ''
      catalogStore.activeSection = 'all'
      catalogStore.activeShelfId = ''
      syncRouteQuery()
    }
  },
})

const libraryTabs = computed<MdTabItem<LibraryTab>[]>(() =>
  isLocalScope.value
    ? [
        { value: 'all', label: t('catalog.tabAll') },
        { value: 'continue', label: t('catalog.tabContinue') },
        { value: 'favorites', label: t('catalog.tabFavorites') },
        { value: 'collections', label: t('catalog.tabCollections') },
      ]
    : [],
)

const viewModeOptions = computed<SegmentedOption<BookBrowseView>[]>(() => [
  { value: 'items', label: t('catalog.viewItems') },
  { value: 'authors', label: t('catalog.viewAuthors') },
  { value: 'tags', label: t('catalog.viewTags') },
])

const layoutOptions = computed(() => [
  {
    value: 'grid' as const,
    label: '',
    icon: LuLayoutGrid,
    title: t('catalog.gridView'),
    ariaLabel: t('catalog.gridView'),
  },
  {
    value: 'list' as const,
    label: '',
    icon: LuList,
    title: t('catalog.tableView'),
    ariaLabel: t('catalog.tableView'),
  },
])

function handleLayoutKeydown(event: KeyboardEvent, currentIdx: number): void {
  if (
    event.key !== 'ArrowLeft' &&
    event.key !== 'ArrowRight' &&
    event.key !== 'ArrowUp' &&
    event.key !== 'ArrowDown'
  ) {
    return
  }
  event.preventDefault()
  const opts = layoutOptions.value
  const delta = event.key === 'ArrowRight' || event.key === 'ArrowDown' ? 1 : -1
  const nextIdx = (currentIdx + delta + opts.length) % opts.length
  const nextOpt = opts[nextIdx]
  if (!nextOpt) return
  handleLayoutModeChange(nextOpt.value)
  const targetEl = event.currentTarget
  const group = targetEl instanceof HTMLElement ? targetEl.parentElement : null
  const buttons = group?.querySelectorAll<HTMLButtonElement>('[role="radio"]')
  buttons?.[nextIdx]?.focus()
}

const sortOptions = computed(() => [
  { value: 'created_at', label: t('catalog.sortCreatedAt') },
  { value: 'updated_at', label: t('catalog.sortUpdatedAt') },
  { value: 'title', label: t('catalog.sortTitle') },
  { value: 'file_size', label: t('catalog.sortFileSize') },
])

const activeShelf = computed<ShelfDto | null>(() =>
  activeCollectionId.value
    ? (catalogStore.shelves.find((sh) => sh.id === activeCollectionId.value) ?? null)
    : null,
)

const isShowingBooksGrid = computed(
  () =>
    isLocalScope.value &&
    activeTab.value === 'all' &&
    (Boolean(activeCollectionId.value) || activeView.value === 'items'),
)

const activeFilterCount = computed(() => {
  if (!isShowingBooksGrid.value) return 0
  return (
    catalogStore.filterFormats.length +
    catalogStore.filterLibraryIds.length +
    catalogStore.filterAuthors.length +
    catalogStore.filterTags.length +
    (activeCollectionId.value ? 1 : 0) +
    (catalogStore.includeMissing ? 1 : 0)
  )
})

const totalPages = computed(() =>
  Math.max(1, Math.ceil(catalogStore.total / catalogStore.pageSize)),
)

const filteredAuthors = computed(() => {
  const q = searchInput.value.trim().toLowerCase()
  if (!q) return catalogStore.authors
  return catalogStore.authors.filter((a) => a.name.toLowerCase().includes(q))
})

const filteredTags = computed(() => {
  const q = searchInput.value.trim().toLowerCase()
  if (!q) return catalogStore.tags
  return catalogStore.tags.filter((tg) => tg.name.toLowerCase().includes(q))
})

const filteredShelves = computed(() => {
  const q = searchInput.value.trim().toLowerCase()
  if (!q) return catalogStore.shelves
  return catalogStore.shelves.filter((sh) => sh.name.toLowerCase().includes(q))
})

const filteredBookmarks = computed(() => {
  const q = searchInput.value.trim().toLowerCase()
  if (!q) return catalogStore.bookmarks
  return catalogStore.bookmarks.filter(
    (entry) =>
      entry.bookmark.title.toLowerCase().includes(q) ||
      entry.book_title.toLowerCase().includes(q) ||
      entry.book_authors.some((a) => a.toLowerCase().includes(q)),
  )
})

const headerCount = computed<number | null>(() => {
  if (!isLocalScope.value) return null
  if (activeTab.value === 'continue') return continueShelf.items.value.length
  if (activeTab.value === 'favorites') return favoritesShelf.items.value.length
  if (activeTab.value === 'collections' && !activeCollectionId.value) {
    return filteredShelves.value.length
  }
  if (activeView.value === 'authors') return filteredAuthors.value.length
  if (activeView.value === 'tags') return filteredTags.value.length
  return catalogStore.total
})

const templateLoading = computed(() => {
  if (!isLocalScope.value) return false
  if (activeTab.value === 'continue') return continueShelf.loading.value
  if (activeTab.value === 'favorites') return favoritesShelf.loading.value
  if (isShowingBooksGrid.value) return catalogStore.loading
  return false
})

const templateError = computed<string | null>(() => {
  if (!isLocalScope.value) return null
  if (activeTab.value === 'continue') return continueShelf.error.value
  if (activeTab.value === 'favorites') return favoritesShelf.error.value
  return localError.value
})

const templateEmpty = computed(() => {
  if (!isLocalScope.value || templateLoading.value || Boolean(templateError.value)) return false
  if (activeTab.value === 'continue') {
    return continueShelf.items.value.length === 0 && filteredBookmarks.value.length === 0
  }
  if (activeTab.value === 'favorites') return favoritesShelf.items.value.length === 0
  if (activeTab.value === 'collections' && !activeCollectionId.value) {
    return filteredShelves.value.length === 0
  }
  if (activeView.value === 'authors') return filteredAuthors.value.length === 0
  if (activeView.value === 'tags') return filteredTags.value.length === 0
  return catalogStore.books.length === 0
})

const emptyTitle = computed(() => {
  if (activeTab.value === 'continue') return t('catalog.emptyContinueTitle')
  if (activeTab.value === 'favorites') return t('catalog.emptyFavoritesTitle')
  if (activeTab.value === 'collections') return t('collections.emptyShelvesTitle')
  if (activeView.value === 'authors' || activeView.value === 'tags') {
    return t('collections.emptyResult')
  }
  return t('catalog.emptyTitle')
})

const emptyDescription = computed(() => {
  if (activeTab.value === 'continue') return t('catalog.emptyContinueHint')
  if (activeTab.value === 'favorites') return t('catalog.emptyFavoritesHint')
  if (activeTab.value === 'collections') return t('collections.emptyShelvesHint')
  if (activeView.value === 'items') return t('catalog.emptyHint')
  return ''
})

function resolveLibraryName(libId: string): string {
  return catalogStore.libraries.find((l) => l.id === libId)?.name ?? libId
}

function handleScopeChange(nextScope: SourceScope): void {
  catalogStore.clearSelection()
  filterSheetOpen.value = false
  void sourceFilter.setScope(nextScope)
}

function handleTabChange(nextTab: string): void {
  const parsed = parseBookTab(nextTab)
  activeTab.value = parsed
  if (parsed !== 'all') {
    activeView.value = 'items'
  }
  if (parsed === 'collections') {
    activeCollectionId.value = ''
    catalogStore.activeSection = 'all'
    catalogStore.activeShelfId = ''
  }
  catalogStore.clearSelection()
  catalogStore.page = 1
  syncRouteQuery()
  void loadActiveLocalData()
}

function handleViewChange(nextView: BookBrowseView): void {
  activeView.value = nextView
  catalogStore.clearSelection()
  syncRouteQuery()
  void loadActiveLocalData()
}

function handleLayoutModeChange(nextMode: 'grid' | 'list'): void {
  if (catalogStore.viewMode === nextMode) return
  catalogStore.clearSelection()
  catalogStore.setViewMode(nextMode)
}

function handleSearchSubmit(query: string): void {
  const trimmed = query.trim()
  searchInput.value = query
  catalogStore.keyword = trimmed
  catalogStore.page = 1
  syncRouteQuery()
  if (isOnlineScope.value) {
    void onlineSearchRef.value?.submitSearch(trimmed)
  } else if (isLocalScope.value) {
    void loadActiveLocalData()
  }
}

function handleSearchClear(): void {
  searchInput.value = ''
  catalogStore.keyword = ''
  catalogStore.page = 1
  syncRouteQuery()
  if (isOnlineScope.value) {
    onlineSearchRef.value?.clearSearch()
  } else if (isLocalScope.value) {
    void loadActiveLocalData()
  }
}

function handleOnlineKeywordSync(nextKw: string): void {
  searchInput.value = nextKw
  catalogStore.keyword = nextKw.trim()
  syncRouteQuery()
}

function handleOnlineSourceSync(nextSourceId: string | null): void {
  void sourceFilter.setSource(nextSourceId)
}

function handleRemoteSourceChange(nextSourceId: string): void {
  void sourceFilter.setSource(nextSourceId || null)
}

function goRemoteSettings(): void {
  void router.push({ path: '/book/settings', query: { tab: 'sources', kind: 'remote' } })
}

function triggerRefetch(): void {
  catalogStore.page = 1
  syncRouteQuery()
  void loadActiveLocalData()
}

function handleSortByChange(val: string | number): void {
  const raw = String(val)
  if (raw === 'created_at' || raw === 'updated_at' || raw === 'title' || raw === 'file_size') {
    catalogStore.sortBy = raw as BookSortField
    triggerRefetch()
  }
}

function handlePageChange(nextPage: number): void {
  if (nextPage < 1 || nextPage > totalPages.value) return
  catalogStore.page = nextPage
  syncRouteQuery()
  void loadActiveLocalData()
}

function handleResetFilters(): void {
  activeCollectionId.value = ''
  catalogStore.activeSection = 'all'
  catalogStore.activeShelfId = ''
  catalogStore.resetFilters()
  searchInput.value = ''
  syncRouteQuery()
  void loadActiveLocalData()
}

function selectDimensionAuthor(author: string): void {
  activeTab.value = 'all'
  activeView.value = 'items'
  catalogStore.filterAuthors = [author]
  catalogStore.page = 1
  syncRouteQuery()
  void loadActiveLocalData()
}

function selectDimensionTag(tag: string): void {
  activeTab.value = 'all'
  activeView.value = 'items'
  catalogStore.filterTags = [tag]
  catalogStore.page = 1
  syncRouteQuery()
  void loadActiveLocalData()
}

function selectShelfCollection(shelfId: string): void {
  activeCollectionId.value = shelfId
  catalogStore.activeSection = shelfId ? 'shelf' : 'all'
  catalogStore.activeShelfId = shelfId
  if (shelfId) {
    activeTab.value = 'all'
    activeView.value = 'items'
  }
  catalogStore.page = 1
  syncRouteQuery()
  void loadActiveLocalData()
}

function handleBackToShelves(): void {
  activeCollectionId.value = ''
  activeTab.value = 'collections'
  catalogStore.activeSection = 'all'
  catalogStore.activeShelfId = ''
  syncRouteQuery()
  void loadActiveLocalData()
}

function removeFormatChip(fmt: BookFormat): void {
  catalogStore.filterFormats = catalogStore.filterFormats.filter((f) => f !== fmt)
  triggerRefetch()
}

function removeLibraryChip(libId: string): void {
  catalogStore.filterLibraryIds = catalogStore.filterLibraryIds.filter((id) => id !== libId)
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

function toggleSortOrder(): void {
  catalogStore.sortOrder = catalogStore.sortOrder === 'desc' ? 'asc' : 'desc'
  triggerRefetch()
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

async function handleCardMutated(): Promise<void> {
  await Promise.all([catalogStore.refreshSidebarData(), loadActiveLocalData()])
}

function handlePoolChanged(): void {
  void catalogStore.refreshSidebarData()
}

async function handleSelectionBatchChanged(refreshSidebar: boolean): Promise<void> {
  if (refreshSidebar) {
    await Promise.all([catalogStore.refreshSidebarData(), loadActiveLocalData()])
  } else {
    await loadActiveLocalData()
  }
}

async function handleRefresh(): Promise<void> {
  if (manualRefreshing.value) return
  manualRefreshing.value = true
  try {
    await Promise.allSettled([catalogStore.refreshSidebarData(), loadActiveLocalData()])
  } finally {
    manualRefreshing.value = false
  }
}

function triggerUpload(): void {
  if (!authStore.isAdmin) return
  uploadInputRef.value?.click()
}

function onUploadFilesChange(ev: Event): void {
  const input = ev.target
  if (!(input instanceof HTMLInputElement)) return
  if (input.files && input.files.length > 0) {
    uploadStore.enqueueFiles(input.files)
  }
  input.value = ''
}
</script>

<template>
  <div class="px-4 py-5 sm:px-6 sm:py-6">
    <DomainLibraryTemplate
      domain="book"
      :title="t('catalog.title')"
      :count="headerCount"
      :scope="activeScope"
      :sync-scope-url="false"
      :show-scope-bar="true"
      :show-remote-scope="sourceFilter.hasRemoteSources.value || isRemoteScope"
      :search-query="searchInput"
      :search-placeholder="
        isOnlineScope
          ? t('online.searchPlaceholder')
          : isPoolScope
            ? globalT('pool.searchPlaceholder', { domain: globalT('app.domains.book') })
            : undefined
      "
      :show-search="!isRemoteScope"
      :tabs="libraryTabs"
      :active-tab="activeTab"
      :loading="templateLoading"
      :skeleton-mode="isShowingBooksGrid && catalogStore.viewMode === 'list' ? 'list' : 'grid'"
      skeleton-aspect="poster"
      :error="templateError"
      :empty="templateEmpty"
      :empty-title="emptyTitle"
      :empty-description="emptyDescription"
      :page="isShowingBooksGrid ? catalogStore.page : 1"
      :page-size="catalogStore.pageSize"
      :total="isShowingBooksGrid ? catalogStore.total : null"
      :total-pages="isShowingBooksGrid ? totalPages : null"
      v-model:filter-sheet-open="filterSheetOpen"
      :active-filter-count="activeFilterCount"
      @scope-change="handleScopeChange"
      @update:search-query="searchInput = $event"
      @search-submit="handleSearchSubmit"
      @search-clear="handleSearchClear"
      @update:active-tab="handleTabChange"
      @update:page="handlePageChange"
      @retry="handleRefresh"
      @reset-filters="handleResetFilters"
    >
      <template v-if="isLocalScope && authStore.isAdmin" #header-trailing>
        <MdButton variant="tonal" size="sm" :icon="LuUpload" @click="triggerUpload">
          {{ t('catalog.uploadBooks') }}
        </MdButton>
      </template>

      <template v-if="isLocalScope && activeTab === 'all'" #toolbar-chips>
        <MdSegmentedButton
          v-if="!activeCollectionId"
          :model-value="activeView"
          :options="viewModeOptions"
          size="sm"
          :show-check="false"
          @update:model-value="handleViewChange"
        />

        <template v-if="isShowingBooksGrid">
          <MdChip
            v-if="activeShelf"
            variant="input"
            selected
            :label="activeShelf.name"
            @remove="selectShelfCollection('')"
          />
          <MdChip
            v-for="fmt in catalogStore.filterFormats"
            :key="`fmt-${fmt}`"
            variant="input"
            selected
            :label="fmt.toUpperCase()"
            @remove="removeFormatChip(fmt)"
          />
          <MdChip
            v-for="libId in catalogStore.filterLibraryIds"
            :key="`lib-${libId}`"
            variant="input"
            selected
            :label="resolveLibraryName(libId)"
            @remove="removeLibraryChip(libId)"
          />
          <MdChip
            v-for="author in catalogStore.filterAuthors"
            :key="`au-${author}`"
            variant="input"
            selected
            :label="author"
            @remove="removeAuthorChip(author)"
          />
          <MdChip
            v-if="catalogStore.filterTags.length > 1"
            variant="filter"
            selected
            hide-check
            :label="
              catalogStore.filterTagMode === 'all'
                ? t('search.tagModeAll')
                : t('search.tagModeAny')
            "
            @click="
              catalogStore.filterTagMode = catalogStore.filterTagMode === 'any' ? 'all' : 'any';
              triggerRefetch()
            "
          />
          <MdChip
            v-for="tg in catalogStore.filterTags"
            :key="`tg-${tg}`"
            variant="input"
            selected
            :label="`#${tg}`"
            @remove="removeTagChip(tg)"
          />
        </template>
      </template>

      <template v-if="isLocalScope" #toolbar-trailing>
        <MdButton
          v-if="activeTab === 'collections' && !activeCollectionId"
          variant="tonal"
          size="sm"
          :icon="LuPlus"
          @click="handleCreateShelf"
        >
          {{ t('collections.createShelfBtn') }}
        </MdButton>

        <template v-if="isShowingBooksGrid">
          <div class="w-32 sm:w-36">
            <MdSelect
              :model-value="catalogStore.sortBy"
              :options="sortOptions"
              size="sm"
              @update:model-value="handleSortByChange"
            />
          </div>

          <MdIconButton
            variant="standard"
            size="sm"
            :icon="catalogStore.sortOrder === 'desc' ? LuArrowDown : LuArrowUp"
            :aria-label="
              catalogStore.sortOrder === 'desc' ? t('catalog.sortDesc') : t('catalog.sortAsc')
            "
            :tooltip="
              catalogStore.sortOrder === 'desc' ? t('catalog.sortDesc') : t('catalog.sortAsc')
            "
            @click="toggleSortOrder"
          />

          <div
            role="radiogroup"
            :aria-label="t('catalog.gridView')"
            class="inline-flex h-9 items-stretch overflow-hidden rounded-corner-full border border-outline"
          >
            <button
              v-for="(opt, idx) in layoutOptions"
              :key="opt.value"
              type="button"
              role="radio"
              :aria-checked="catalogStore.viewMode === opt.value"
              :aria-label="opt.ariaLabel"
              :title="opt.title"
              :tabindex="catalogStore.viewMode === opt.value ? 0 : -1"
              class="md-state md-touch-target inline-flex cursor-pointer items-center justify-center px-3 transition-colors duration-150 ease-standard focus-visible:outline-3 focus-visible:-outline-offset-2 focus-visible:outline-secondary"
              :class="[
                idx > 0 ? 'border-l border-outline' : '',
                catalogStore.viewMode === opt.value
                  ? 'bg-secondary-container text-on-secondary-container'
                  : 'bg-transparent text-on-surface hover:bg-surface-container-high/50',
              ]"
              @click="handleLayoutModeChange(opt.value)"
              @keydown="handleLayoutKeydown($event, idx)"
            >
              <component :is="opt.icon" class="h-4 w-4" aria-hidden="true" />
            </button>
          </div>
        </template>

        <MdIconButton
          variant="standard"
          size="sm"
          :icon="LuRefreshCw"
          :loading="manualRefreshing"
          :aria-label="globalT('common.refresh')"
          :tooltip="globalT('common.refresh')"
          @click="handleRefresh"
        />
      </template>

      <template v-if="isShowingBooksGrid && catalogStore.selectedBookIds.size > 0" #selection-bar>
        <BookCatalogSelectionBar @changed="handleSelectionBatchChanged" />
      </template>

      <template v-if="isLocalScope && activeShelf" #subheader>
        <BookCatalogShelvesSection
          mode="subheader"
          :active-shelf="activeShelf"
          @back="handleBackToShelves"
          @rename="handleRenameShelf"
          @delete="handleDeleteShelf"
        />
      </template>

      <template v-if="isShowingBooksGrid" #filter-sheet>
        <BookCatalogFilterSheet
          :active-collection-id="activeCollectionId"
          @refetch="triggerRefetch"
          @select-shelf="selectShelfCollection"
        />
      </template>

      <template #empty-actions>
        <MdButton
          v-if="isLocalScope && activeTab === 'collections'"
          variant="filled"
          :icon="LuPlus"
          @click="handleCreateShelf"
        >
          {{ t('collections.createShelfBtn') }}
        </MdButton>
        <template v-else-if="isShowingBooksGrid">
          <MdButton
            v-if="activeFilterCount > 0 || searchInput.trim()"
            variant="tonal"
            @click="handleResetFilters"
          >
            {{ t('search.reset') }}
          </MdButton>
          <MdButton
            v-if="authStore.isAdmin"
            variant="filled"
            :icon="LuUpload"
            @click="triggerUpload"
          >
            {{ t('catalog.uploadBooks') }}
          </MdButton>
        </template>
      </template>

      <DomainPoolSection
        v-if="isPoolScope"
        domain="book"
        :search-query="searchInput"
        @changed="handlePoolChanged"
      />

      <OnlineBookSearchSection
        v-else-if="isOnlineScope"
        ref="onlineSearchRef"
        :keyword="searchInput"
        :initial-source-id="onlineSourceId"
        :show-search-input="false"
        @update:keyword="handleOnlineKeywordSync"
        @update:source-id="handleOnlineSourceSync"
      />

      <RemoteOpdsBrowser
        v-else-if="isRemoteScope"
        :selected-source-id="selectedRemoteSourceId"
        :active="true"
        @update:selected-source-id="handleRemoteSourceChange"
        @add-source="goRemoteSettings"
      />

      <template v-else>
        <BookCatalogContinueSection
          v-if="activeTab === 'continue'"
          :items="continueShelf.items.value"
          :bookmarks="filteredBookmarks"
        />

        <div
          v-else-if="activeTab === 'favorites'"
          class="grid grid-cols-2 gap-5 sm:grid-cols-3 md:grid-cols-4 lg:grid-cols-5 xl:grid-cols-6"
        >
          <MediaItemCard
            v-for="(item, idx) in favoritesShelf.items.value"
            :key="`fav-${idx}-${item.title}`"
            :item="item"
            aspect="poster"
            cover-style="book"
          />
        </div>

        <BookCatalogShelvesSection
          v-else-if="activeTab === 'collections' && !activeCollectionId"
          mode="grid"
          :shelves="filteredShelves"
          @select="selectShelfCollection"
          @rename="handleRenameShelf"
          @delete="handleDeleteShelf"
        />

        <div v-else-if="activeView === 'authors'" class="flex flex-wrap gap-2">
          <MdChip
            v-for="author in filteredAuthors"
            :key="author.name"
            variant="assist"
            :icon="LuUser"
            :label="`${author.name} (${author.count})`"
            @click="selectDimensionAuthor(author.name)"
          />
        </div>

        <div v-else-if="activeView === 'tags'" class="flex flex-wrap gap-2">
          <MdChip
            v-for="tag in filteredTags"
            :key="tag.name"
            variant="assist"
            :icon="LuTag"
            :label="`#${tag.name} (${tag.count})`"
            @click="selectDimensionTag(tag.name)"
          />
        </div>

        <BookCatalogBooksGrid
          v-else-if="catalogStore.viewMode === 'grid'"
          :books="catalogStore.books"
          @toggle-favorite="toggleBookFavorite"
          @mutated="handleCardMutated"
        />

        <BookCatalogListTable
          v-else
          :books="catalogStore.books"
          @toggle-favorite="toggleBookFavorite"
        />
      </template>
    </DomainLibraryTemplate>

    <input
      v-if="authStore.isAdmin"
      ref="uploadInputRef"
      type="file"
      multiple
      accept=".epub,.txt,.pdf,.mobi,.azw3,.fb2"
      class="hidden"
      @change="onUploadFilesChange"
    />
  </div>
</template>

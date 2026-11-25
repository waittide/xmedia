import { computed, onMounted, ref, watch } from 'vue'
import { useRoute, useRouter } from 'vue-router'
import type { MdTabItem, SegmentedOption } from '@/components/ui'
import { api } from '@/domains/book/api'
import {
  parseBookTab,
  useBookCatalogQuerySync,
  type BookBrowseView,
} from '@/domains/book/components/catalog/useBookCatalogQuerySync'
import { useBookShelfActions } from '@/domains/book/components/catalog/useBookShelfActions'
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
import { useSnackbarStore } from '@/stores/snackbar'
import { useUploadStore } from '@/stores/uploads'

export interface UseBookLibraryCatalogOptions {
  submitOnlineSearch: (query: string) => void
  clearOnlineSearch: () => void
}

export function useBookLibraryCatalog(options: UseBookLibraryCatalogOptions) {
  const { t } = useI18n()
  const route = useRoute()
  const router = useRouter()
  const appClientStore = useAppClientStore()
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
      options.submitOnlineSearch(trimmed)
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
      options.clearOnlineSearch()
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

  return {
    sourceFilter,
    activeScope,
    onlineSourceId,
    activeTab,
    activeView,
    activeCollectionId,
    searchInput,
    filterSheetOpen,
    manualRefreshing,
    isLocalScope,
    isPoolScope,
    isOnlineScope,
    isRemoteScope,
    continueShelf,
    favoritesShelf,
    selectedRemoteSourceId,
    libraryTabs,
    viewModeOptions,
    sortOptions,
    activeShelf,
    isShowingBooksGrid,
    activeFilterCount,
    totalPages,
    filteredAuthors,
    filteredTags,
    filteredShelves,
    filteredBookmarks,
    headerCount,
    templateLoading,
    templateError,
    templateEmpty,
    emptyTitle,
    emptyDescription,
    resolveLibraryName,
    handleCreateShelf,
    handleRenameShelf,
    handleDeleteShelf,
    handleScopeChange,
    handleTabChange,
    handleViewChange,
    handleLayoutModeChange,
    handleSearchSubmit,
    handleSearchClear,
    handleOnlineKeywordSync,
    handleOnlineSourceSync,
    handleRemoteSourceChange,
    goRemoteSettings,
    triggerRefetch,
    handleSortByChange,
    handlePageChange,
    handleResetFilters,
    selectDimensionAuthor,
    selectDimensionTag,
    selectShelfCollection,
    handleBackToShelves,
    removeFormatChip,
    removeLibraryChip,
    removeAuthorChip,
    removeTagChip,
    toggleSortOrder,
    toggleBookFavorite,
    handleCardMutated,
    handlePoolChanged,
    handleSelectionBatchChanged,
    handleRefresh,
  }
}

<script setup lang="ts">
import { computed, onMounted, ref, watch } from 'vue'
import { useI18n as useGlobalI18n } from 'vue-i18n'
import { useRoute, useRouter, type LocationQueryRaw } from 'vue-router'
import {
  LuArrowDown,
  LuArrowUp,
  LuBookOpen,
  LuLayoutGrid,
  LuList,
  LuPlus,
  LuRefreshCw,
  LuTag,
  LuTrash2,
  LuUpload,
  LuUser,
  LuX,
} from 'vue-icons-plus/lu'
import { DomainLibraryTemplate, DomainPoolSection, MediaItemCard } from '@/components/domain'
import RemoteOpdsBrowser from '@/components/remote/RemoteOpdsBrowser.vue'
import {
  MdButton,
  MdChip,
  MdIconButton,
  MdMenuItem,
  MdSegmentedButton,
  MdSelect,
  type MdTabItem,
  type SegmentedOption,
} from '@/components/ui'
import { api } from '@/domains/book/api'
import BookCatalogContinueSection from '@/domains/book/components/catalog/BookCatalogContinueSection.vue'
import BookCatalogFilterSheet from '@/domains/book/components/catalog/BookCatalogFilterSheet.vue'
import BookCatalogListTable from '@/domains/book/components/catalog/BookCatalogListTable.vue'
import BookCatalogSelectionBar from '@/domains/book/components/catalog/BookCatalogSelectionBar.vue'
import BookCatalogShelvesSection from '@/domains/book/components/catalog/BookCatalogShelvesSection.vue'
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
import { bookSummaryToVM, hasBookProgress } from '@/domains/book/utils/bookVm'
import {
  itemRoute,
  playRoute,
  useMergedShelf,
  useSourceFilter,
  type LibraryTab,
  type MediaItemVM,
  type SourceScope,
} from '@/sources'
import { useAppClientStore } from '@/stores/appServers'
import { useAuthStore } from '@/stores/auth'
import { useDialogStore } from '@/stores/dialog'
import { useSnackbarStore } from '@/stores/snackbar'
import { useUploadStore } from '@/stores/uploads'

type BookBrowseView = 'items' | 'authors' | 'tags'

const { t } = useI18n()
const { t: globalT } = useGlobalI18n()
const route = useRoute()
const router = useRouter()
const appClientStore = useAppClientStore()
const authStore = useAuthStore()
const catalogStore = useCatalogStore()
const dialogStore = useDialogStore()
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
let syncingRouteQuery = false

const isLocalScope = computed(() => activeScope.value === 'local')
const isPoolScope = computed(() => activeScope.value === 'pool')
const isOnlineScope = computed(() => activeScope.value === 'online')
const isRemoteScope = computed(() => activeScope.value === 'remote')

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

function readQueryString(val: unknown): string {
  if (typeof val === 'string') return val.trim()
  if (Array.isArray(val)) {
    const first = val.find((v): v is string => typeof v === 'string' && v.trim().length > 0)
    return first ? first.trim() : ''
  }
  return ''
}

function readQueryList(val: unknown): string[] {
  if (typeof val === 'string') return val.trim() ? [val.trim()] : []
  if (Array.isArray(val)) {
    return val
      .filter((v): v is string => typeof v === 'string' && v.trim().length > 0)
      .map((v) => v.trim())
  }
  return []
}

function parseTab(raw: string): LibraryTab {
  if (raw === 'continue' || raw === 'favorites' || raw === 'collections') return raw
  return 'all'
}

function parseView(raw: string): BookBrowseView {
  if (raw === 'authors' || raw === 'tags') return raw
  return 'items'
}

function buildCanonicalQuery(): LocationQueryRaw {
  const q: LocationQueryRaw = {}
  const scope = activeScope.value

  if (scope !== 'local') {
    q.scope = scope
    const srcId = sourceFilter.filter.value.sourceId
    if (srcId) q.source = srcId
    if (searchInput.value.trim()) q.q = searchInput.value.trim()
    return q
  }

  if (activeTab.value !== 'all') q.tab = activeTab.value
  if (activeCollectionId.value) q.collection = activeCollectionId.value
  if (activeTab.value === 'all' && !activeCollectionId.value && activeView.value !== 'items') {
    q.view = activeView.value
  }
  if (searchInput.value.trim()) q.q = searchInput.value.trim()

  if (catalogStore.filterLibraryIds.length === 1) {
    q.library = catalogStore.filterLibraryIds[0]!
  } else if (catalogStore.filterLibraryIds.length > 1) {
    q.library = [...catalogStore.filterLibraryIds]
  }
  if (catalogStore.filterAuthors.length === 1) {
    q.author = catalogStore.filterAuthors[0]!
  } else if (catalogStore.filterAuthors.length > 1) {
    q.author = [...catalogStore.filterAuthors]
  }
  if (catalogStore.filterTags.length === 1) {
    q.tag = catalogStore.filterTags[0]!
  } else if (catalogStore.filterTags.length > 1) {
    q.tag = [...catalogStore.filterTags]
  }
  if (catalogStore.filterTags.length > 1 && catalogStore.filterTagMode !== 'any') {
    q.tag_mode = catalogStore.filterTagMode
  }
  if (catalogStore.filterFormats.length === 1) {
    q.format = catalogStore.filterFormats[0]!
  } else if (catalogStore.filterFormats.length > 1) {
    q.format = [...catalogStore.filterFormats]
  }
  if (catalogStore.includeMissing) q.missing = '1'
  if (catalogStore.sortBy !== 'created_at') q.sort = catalogStore.sortBy
  if (catalogStore.sortOrder !== 'desc') q.order = catalogStore.sortOrder
  if (catalogStore.page > 1) q.page = String(catalogStore.page)

  return q
}

function queryValuesEqual(a: unknown, b: unknown): boolean {
  if (Array.isArray(a) || Array.isArray(b)) {
    const la = readQueryList(a)
    const lb = readQueryList(b)
    return la.length === lb.length && la.every((v, i) => v === lb[i])
  }
  return a === b
}

function syncRouteQuery(): void {
  const next = buildCanonicalQuery()
  const cur = route.query
  const same =
    route.path === '/book/library' &&
    Object.keys(next).length === Object.keys(cur).length &&
    Object.entries(next).every(([k, v]) => queryValuesEqual(cur[k], v))
  if (same) return
  syncingRouteQuery = true
  void router.replace({ path: '/book/library', query: next }).finally(() => {
    syncingRouteQuery = false
  })
}

function applyRouteQuery(): void {
  const qTab = readQueryString(route.query.tab)
  const qCollection = readQueryString(route.query.collection) || readQueryString(route.query.shelf)
  const qView = readQueryString(route.query.view)
  const qKeyword = readQueryString(route.query.q)
  const qLibraries = readQueryList(route.query.library)
  const qAuthors = readQueryList(route.query.author)
  const qTags = readQueryList(route.query.tag)
  const qTagMode = readQueryString(route.query.tag_mode)
  const qFormats = readQueryList(route.query.format).filter(
    (f): f is BookFormat => f === 'epub' || f === 'txt' || f === 'pdf',
  )
  const qMissing = route.query.missing === '1' || route.query.missing === 'true'
  const qSort = readQueryString(route.query.sort) || readQueryString(route.query.sort_by)
  const qOrder = readQueryString(route.query.order) || readQueryString(route.query.sort_order)
  const qPage = Number.parseInt(readQueryString(route.query.page), 10)

  activeTab.value = qCollection && qTab === 'collections' ? 'all' : parseTab(qTab)
  activeCollectionId.value = qCollection
  activeView.value = qCollection ? 'items' : parseView(qView)
  searchInput.value = qKeyword

  catalogStore.activeSection = qCollection ? 'shelf' : 'all'
  catalogStore.activeLibraryId = ''
  catalogStore.activeShelfId = qCollection
  catalogStore.keyword = qKeyword
  catalogStore.filterLibraryIds = qLibraries
  catalogStore.filterAuthors = qAuthors
  catalogStore.filterTags = qTags
  catalogStore.filterTagMode = qTagMode === 'all' ? 'all' : 'any'
  catalogStore.filterFormats = qFormats
  catalogStore.includeMissing = qMissing
  catalogStore.sortBy =
    qSort === 'created_at' || qSort === 'updated_at' || qSort === 'title' || qSort === 'file_size'
      ? qSort
      : 'created_at'
  catalogStore.sortOrder = qOrder === 'asc' || qOrder === 'desc' ? qOrder : 'desc'
  catalogStore.page = Number.isFinite(qPage) && qPage >= 1 ? qPage : 1
}

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
    if (syncingRouteQuery) return
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
  const group = (event.currentTarget as HTMLElement | null)?.parentElement
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

const localBookVMs = computed<Array<{ book: BookSummaryDto; vm: MediaItemVM }>>(() =>
  catalogStore.books.map((book) => ({
    book,
    vm: bookSummaryToVM(book, {
      unknownAuthor: t('catalog.unknownAuthor'),
      missingLabel: t('catalog.missingBadge'),
    }),
  })),
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
  const parsed = parseTab(nextTab)
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

async function handleDeleteBookFromCard(book: BookSummaryDto): Promise<void> {
  const confirmed = await dialogStore.confirm({
    title: t('detail.deleteBook'),
    message: t('detail.confirmDeleteBook', { title: book.metadata.title }),
    danger: true,
  })
  if (!confirmed) return
  try {
    await api.deleteBook(book.id)
    await Promise.all([catalogStore.refreshSidebarData(), loadActiveLocalData()])
  } catch (err) {
    snackbarStore.show({
      message: err instanceof Error ? err.message : String(err),
      tone: 'error',
    })
  }
}

async function handleUnbindBookFromCard(book: BookSummaryDto): Promise<void> {
  const confirmed = await dialogStore.confirm({
    title: globalT('pool.unbindConfirmTitle'),
    message: book.online_tracking
      ? globalT('pool.unbindOnlineConfirmMessage', { title: book.metadata.title })
      : globalT('pool.unbindConfirmMessage', { title: book.metadata.title }),
    danger: true,
    confirmText: globalT('pool.unbindBtn'),
  })
  if (!confirmed) return
  try {
    await api.unbindItem(book.id)
    snackbarStore.show(globalT('pool.unboundToast'))
    await Promise.all([catalogStore.refreshSidebarData(), loadActiveLocalData()])
  } catch (err) {
    snackbarStore.show({
      message: err instanceof Error ? err.message : String(err),
      tone: 'error',
    })
  }
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
  const input = ev.target as HTMLInputElement
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

        <div
          v-else-if="catalogStore.viewMode === 'grid'"
          class="grid grid-cols-2 gap-5 sm:grid-cols-3 md:grid-cols-4 lg:grid-cols-5 xl:grid-cols-6"
        >
          <MediaItemCard
            v-for="entry in localBookVMs"
            :key="entry.book.id"
            :item="entry.vm"
            aspect="poster"
            cover-style="book"
            show-favorite
            :favorite="entry.book.is_favorite"
            @toggle-favorite="toggleBookFavorite(entry.book)"
          >
            <template #menu>
              <MdMenuItem
                v-if="!entry.book.is_missing"
                :icon="LuBookOpen"
                :label="
                  hasBookProgress(entry.book.progress)
                    ? t('catalog.continueReading')
                    : t('catalog.startReading')
                "
                :to="playRoute({ scope: 'local', domain: 'book', id: entry.book.id })"
              />
              <MdMenuItem
                :icon="LuBookOpen"
                :label="t('catalog.openDetail')"
                :to="itemRoute({ scope: 'local', domain: 'book', id: entry.book.id })"
              />
              <MdMenuItem
                :icon="LuX"
                :label="globalT('pool.unbindBtn')"
                destructive
                @select="handleUnbindBookFromCard(entry.book)"
              />
              <MdMenuItem
                v-if="authStore.isAdmin"
                :icon="LuTrash2"
                :label="t('detail.deleteBook')"
                destructive
                @select="handleDeleteBookFromCard(entry.book)"
              />
            </template>
          </MediaItemCard>
        </div>

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
      accept=".epub,.txt,.pdf"
      class="hidden"
      @change="onUploadFilesChange"
    />
  </div>
</template>

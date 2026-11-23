<script setup lang="ts">
import { computed, onMounted, ref, watch } from 'vue'
import { useI18n as useGlobalI18n } from 'vue-i18n'
import { useRoute, useRouter, type LocationQueryRaw } from 'vue-router'
import {
  LuArrowDown,
  LuArrowLeft,
  LuArrowUp,
  LuBookmark,
  LuBookOpen,
  LuFolder,
  LuHeart,
  LuLayoutGrid,
  LuList,
  LuMoreVertical,
  LuPencil,
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
  AppCheckbox,
  AppMultiSelect,
  MdButton,
  MdChip,
  MdIconButton,
  MdMenu,
  MdMenuItem,
  MdSegmentedButton,
  MdSelect,
  MediaCoverImage,
  type MdTabItem,
  type SegmentedOption,
} from '@/components/ui'
import { api } from '@/domains/book/api'
import OnlineBookSearchSection from '@/domains/book/components/OnlineBookSearchSection.vue'
import { useBookI18n as useI18n } from '@/domains/book/i18n'
import { useCatalogStore } from '@/domains/book/store'
import type {
  BookBookmarkItemDto,
  BookFormat,
  BookSortField,
  BookSummaryDto,
  ShelfDto,
  TagMatchMode,
} from '@/domains/book/types'
import {
  bookSummaryToVM,
  formatProgressPercentValue,
  hasBookProgress,
  normalizeBookProgressPercent,
} from '@/domains/book/utils/bookVm'
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
import { formatBytes } from '@/utils/format'

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
const deletingBookmarkIds = ref<Set<string>>(new Set())

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

// Tabs & options
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

const layoutOptions = computed<SegmentedOption<'grid' | 'list'>[]>(() => [
  { value: 'grid', label: '', icon: LuLayoutGrid, title: t('catalog.gridView') },
  { value: 'list', label: '', icon: LuList, title: t('catalog.tableView') },
])

const tagModeOptions = computed<SegmentedOption<TagMatchMode>[]>(() => [
  { value: 'any', label: t('search.tagModeAny') },
  { value: 'all', label: t('search.tagModeAll') },
])

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

const shelfOptions = computed(() => [
  { value: '', label: t('search.allShelves') },
  ...catalogStore.shelves.map((sh) => ({
    value: sh.id,
    label: `${sh.name} (${sh.book_count})`,
  })),
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

const allCurrentSelected = computed(
  () =>
    catalogStore.books.length > 0 &&
    catalogStore.books.every((b) => catalogStore.selectedBookIds.has(b.id)),
)

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

function clearShelfChip(): void {
  selectShelfCollection('')
}

function toggleSortOrder(): void {
  catalogStore.sortOrder = catalogStore.sortOrder === 'desc' ? 'asc' : 'desc'
  triggerRefetch()
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

async function batchFavorite(fav: boolean): Promise<void> {
  const ids = Array.from(catalogStore.selectedBookIds)
  if (ids.length === 0) return
  try {
    await api.batchBooks({ book_ids: ids, set_favorite: fav })
    catalogStore.clearSelection()
    await loadActiveLocalData()
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
    await Promise.all([catalogStore.refreshSidebarData(), loadActiveLocalData()])
  } catch (err) {
    snackbarStore.show({
      message: err instanceof Error ? err.message : String(err),
      tone: 'error',
    })
  }
}

async function handleCreateShelf(): Promise<void> {
  const name = await dialogStore.prompt({
    title: t('nav.newShelf'),
    placeholder: t('nav.shelfNamePlaceholder'),
  })
  const trimmed = name?.trim()
  if (!trimmed) return
  try {
    await api.createShelf(trimmed)
    await catalogStore.refreshSidebarData()
    snackbarStore.show(t('collections.shelfCreated', { name: trimmed }))
  } catch (err) {
    snackbarStore.show({
      message: err instanceof Error ? err.message : String(err),
      tone: 'error',
    })
  }
}

async function handleRenameShelf(shelf: ShelfDto): Promise<void> {
  const nextName = await dialogStore.prompt({
    title: t('collections.renameShelf'),
    message: t('collections.renameShelfPrompt'),
    defaultValue: shelf.name,
  })
  const trimmed = nextName?.trim()
  if (!trimmed || trimmed === shelf.name) return
  try {
    await api.renameShelf(shelf.id, trimmed)
    await catalogStore.refreshSidebarData()
    snackbarStore.show(t('collections.shelfRenamed'))
  } catch (err) {
    snackbarStore.show({
      message: err instanceof Error ? err.message : String(err),
      tone: 'error',
    })
  }
}

async function handleDeleteShelf(shelf: ShelfDto): Promise<void> {
  const confirmed = await dialogStore.confirm({
    title: t('collections.deleteShelf'),
    message: t('collections.confirmDeleteShelf', { name: shelf.name }),
    danger: true,
  })
  if (!confirmed) return
  try {
    await api.deleteShelf(shelf.id)
    if (activeCollectionId.value === shelf.id) {
      activeCollectionId.value = ''
      catalogStore.activeSection = 'all'
      catalogStore.activeShelfId = ''
      syncRouteQuery()
    }
    await catalogStore.refreshSidebarData()
    snackbarStore.show(t('collections.shelfDeleted'))
  } catch (err) {
    snackbarStore.show({
      message: err instanceof Error ? err.message : String(err),
      tone: 'error',
    })
  }
}

function openBookmarkInReader(entry: BookBookmarkItemDto): void {
  const bm = entry.bookmark
  void router.push({
    path: `/book/play/${encodeURIComponent(bm.book_id)}`,
    query: {
      locator: bm.location.locator,
      chapter: bm.location.chapter_index !== null ? String(bm.location.chapter_index) : undefined,
      page: bm.location.page_number !== null ? String(bm.location.page_number) : undefined,
      ratio: bm.location.scroll_ratio !== null ? String(bm.location.scroll_ratio) : undefined,
    },
  })
}

async function handleDeleteBookmark(entry: BookBookmarkItemDto): Promise<void> {
  const bm = entry.bookmark
  if (deletingBookmarkIds.value.has(bm.id)) return
  const next = new Set(deletingBookmarkIds.value)
  next.add(bm.id)
  deletingBookmarkIds.value = next
  try {
    await api.deleteBookmark(bm.id)
    catalogStore.bookmarks = catalogStore.bookmarks.filter((item) => item.bookmark.id !== bm.id)
    snackbarStore.show({
      message: t('catalog.bookmarkDeleted'),
      actionLabel: t('catalog.undo'),
      onAction: () => {
        void api
          .createBookmark(bm.book_id, {
            title: bm.title,
            note: bm.note ?? undefined,
            progress_percent: bm.location.progress_percent,
            locator: bm.location.locator,
            chapter_index: bm.location.chapter_index,
            page_number: bm.location.page_number,
            scroll_ratio: bm.location.scroll_ratio,
          })
          .then(() => catalogStore.refreshSidebarData())
          .then(() => {
            snackbarStore.show(t('catalog.bookRestored'))
          })
          .catch(() => {})
      },
    })
  } catch (err) {
    snackbarStore.show({
      message: err instanceof Error ? err.message : String(err),
      tone: 'error',
    })
  } finally {
    const done = new Set(deletingBookmarkIds.value)
    done.delete(bm.id)
    deletingBookmarkIds.value = done
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

      <!-- Inline Dimension Switcher + Active Filter Chips -->
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
            @remove="clearShelfChip"
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

      <!-- Trailing Sort / Layout / Create Shelf Controls -->
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
              @update:model-value="
                (val) => {
                  catalogStore.sortBy = val as BookSortField
                  triggerRefetch()
                }
              "
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

          <MdSegmentedButton
            :model-value="catalogStore.viewMode"
            :options="layoutOptions"
            size="sm"
            :show-check="false"
            @update:model-value="handleLayoutModeChange"
          />
        </template>

        <MdIconButton
          variant="standard"
          size="sm"
          :icon="LuRefreshCw"
          :loading="manualRefreshing"
          :aria-label="t('common.refresh')"
          :tooltip="t('common.refresh')"
          @click="handleRefresh"
        />
      </template>

      <!-- Bulk Selection Toolbar (List Mode) -->
      <template v-if="isShowingBooksGrid && catalogStore.selectedBookIds.size > 0" #selection-bar>
        <div
          class="flex flex-wrap items-center justify-between gap-2 rounded-corner-md bg-secondary-container px-4 py-2.5 text-on-secondary-container"
        >
          <span class="text-title-small">
            {{ t('catalog.selectedCount', { count: catalogStore.selectedBookIds.size }) }}
          </span>
          <div class="flex flex-wrap items-center gap-1.5">
            <MdButton variant="elevated" size="sm" :icon="LuHeart" @click="batchFavorite(true)">
              {{ t('catalog.batchStar') }}
            </MdButton>
            <MdButton variant="elevated" size="sm" @click="batchFavorite(false)">
              {{ t('catalog.batchUnstar') }}
            </MdButton>
            <MdButton
              v-if="authStore.isAdmin"
              variant="elevated"
              size="sm"
              :icon="LuTag"
              @click="batchAddTag"
            >
              {{ t('catalog.batchAddTag') }}
            </MdButton>
            <MdButton variant="text" size="sm" @click="catalogStore.clearSelection()">
              {{ t('catalog.clearSelection') }}
            </MdButton>
          </div>
        </div>
      </template>

      <!-- Active Shelf Subheader -->
      <template v-if="isLocalScope && activeShelf" #subheader>
        <div
          class="flex flex-wrap items-center justify-between gap-3 rounded-corner-md bg-surface-container-low px-4 py-3"
        >
          <div class="flex min-w-0 items-center gap-2.5">
            <MdButton
              variant="text"
              size="sm"
              :icon="LuArrowLeft"
              @click="
                activeCollectionId = '';
                activeTab = 'collections';
                catalogStore.activeSection = 'all';
                catalogStore.activeShelfId = '';
                syncRouteQuery();
                loadActiveLocalData()
              "
            >
              {{ t('collections.backToShelves') }}
            </MdButton>
            <span class="text-title-medium truncate text-on-surface">
              {{ activeShelf.name }}
            </span>
            <span class="text-body-small shrink-0 tabular-nums text-on-surface-variant">
              {{ t('collections.bookCountUnit', { count: activeShelf.book_count }) }}
            </span>
          </div>

          <div v-if="!activeShelf.is_favorite" class="flex items-center gap-1">
            <MdButton
              variant="text"
              size="sm"
              :icon="LuPencil"
              @click="handleRenameShelf(activeShelf)"
            >
              {{ t('collections.renameShelf') }}
            </MdButton>
            <MdButton
              variant="text"
              tone="error"
              size="sm"
              :icon="LuTrash2"
              @click="handleDeleteShelf(activeShelf)"
            >
              {{ t('collections.deleteShelf') }}
            </MdButton>
          </div>
        </div>
      </template>

      <!-- Filter Sheet (Local Items) -->
      <template v-if="isShowingBooksGrid" #filter-sheet>
        <div class="space-y-4">
          <AppMultiSelect
            :model-value="catalogStore.filterFormats"
            :options="formatOptions"
            :label="t('search.format')"
            :all-placeholder="t('search.allFormats')"
            @update:model-value="
              (vals) => {
                catalogStore.filterFormats = vals as BookFormat[]
                triggerRefetch()
              }
            "
          />

          <AppMultiSelect
            :model-value="catalogStore.filterLibraryIds"
            :options="libraryOptions"
            :label="t('search.library')"
            :all-placeholder="t('search.allLibraries')"
            @update:model-value="
              (vals) => {
                catalogStore.filterLibraryIds = vals
                triggerRefetch()
              }
            "
          />

          <div class="space-y-1.5">
            <label class="text-label-medium block text-on-surface-variant">
              {{ t('search.shelf') }}
            </label>
            <MdSelect
              :model-value="activeCollectionId"
              :options="shelfOptions"
              size="sm"
              @update:model-value="selectShelfCollection(String($event))"
            />
          </div>

          <AppMultiSelect
            :model-value="catalogStore.filterAuthors"
            :options="authorOptions"
            :label="t('search.author')"
            :all-placeholder="t('search.allAuthors')"
            @update:model-value="
              (vals) => {
                catalogStore.filterAuthors = vals
                triggerRefetch()
              }
            "
          />

          <AppMultiSelect
            :model-value="catalogStore.filterTags"
            :options="tagOptions"
            :label="t('search.tag')"
            :all-placeholder="t('search.allTags')"
            @update:model-value="
              (vals) => {
                catalogStore.filterTags = vals
                triggerRefetch()
              }
            "
          >
            <template #header>
              <MdSegmentedButton
                :model-value="catalogStore.filterTagMode"
                :options="tagModeOptions"
                size="sm"
                :show-check="false"
                full-width
                @update:model-value="
                  (v) => {
                    catalogStore.filterTagMode = v
                    triggerRefetch()
                  }
                "
              />
            </template>
          </AppMultiSelect>

          <div class="border-t border-outline-variant pt-3">
            <AppCheckbox
              v-model="catalogStore.includeMissing"
              :label="t('search.includeMissing')"
              @change="triggerRefetch"
            />
          </div>
        </div>
      </template>

      <!-- Empty State Actions -->
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

      <!-- SCOPE 0: POOL -->
      <DomainPoolSection
        v-if="isPoolScope"
        domain="book"
        :search-query="searchInput"
        @changed="handlePoolChanged"
      />

      <!-- SCOPE 1: ONLINE -->
      <OnlineBookSearchSection
        v-else-if="isOnlineScope"
        ref="onlineSearchRef"
        :keyword="searchInput"
        :initial-source-id="onlineSourceId"
        :show-search-input="false"
        @update:keyword="handleOnlineKeywordSync"
        @update:source-id="handleOnlineSourceSync"
      />

      <!-- SCOPE 2: REMOTE (Standalone OPDS) -->
      <RemoteOpdsBrowser
        v-else-if="isRemoteScope"
        :selected-source-id="selectedRemoteSourceId"
        :active="true"
        @update:selected-source-id="handleRemoteSourceChange"
        @add-source="goRemoteSettings"
      />

      <!-- SCOPE 3: LOCAL -->
      <template v-else>
        <!-- TAB: CONTINUE (Merged Shelf + Bookmarks Section) -->
        <div v-if="activeTab === 'continue'" class="space-y-8">
          <div
            v-if="continueShelf.items.value.length > 0"
            class="grid grid-cols-2 gap-5 sm:grid-cols-3 md:grid-cols-4 lg:grid-cols-5 xl:grid-cols-6"
          >
            <MediaItemCard
              v-for="(item, idx) in continueShelf.items.value"
              :key="`cont-${idx}-${item.title}`"
              :item="item"
              aspect="poster"
              cover-style="book"
              prefer-play-route
            />
          </div>

          <section v-if="filteredBookmarks.length > 0" class="space-y-3">
            <h2 class="text-title-large flex items-center gap-2 text-on-surface">
              <LuBookmark class="h-5 w-5 text-primary" aria-hidden="true" />
              <span>{{ t('catalog.bookmarksSectionTitle') }}</span>
              <span class="text-body-small tabular-nums text-on-surface-variant">
                {{ filteredBookmarks.length }}
              </span>
            </h2>

            <div class="grid grid-cols-1 gap-3 md:grid-cols-2">
              <div
                v-for="entry in filteredBookmarks"
                :key="entry.bookmark.id"
                class="flex items-center gap-3 rounded-corner-md bg-surface-container-low p-3"
              >
                <button
                  type="button"
                  class="md-state flex min-w-0 flex-1 cursor-pointer items-center gap-3 rounded-corner-sm text-left"
                  @click="openBookmarkInReader(entry)"
                >
                  <div class="h-14 w-10 shrink-0 overflow-hidden rounded-corner-xs">
                    <MediaCoverImage
                      :src="
                        entry.has_cover
                          ? api.coverUrl(entry.bookmark.book_id, entry.book_updated_at)
                          : null
                      "
                      :alt="entry.book_title"
                      aspect-class="aspect-[2/3]"
                      rounded="rounded-corner-xs"
                      bg-class="bg-surface-container-high"
                      :fallback-icon="LuBookOpen"
                      fallback-icon-class="h-4 w-4 text-on-surface-variant/60"
                    />
                  </div>
                  <div class="min-w-0 flex-1">
                    <p class="text-title-small truncate text-on-surface">
                      {{ entry.bookmark.title }}
                    </p>
                    <p class="text-body-small mt-0.5 truncate text-on-surface-variant">
                      {{ entry.book_title }}
                    </p>
                    <p class="text-label-small mt-0.5 tabular-nums text-primary">
                      {{ Math.round(entry.bookmark.location.progress_percent) }}%
                    </p>
                  </div>
                </button>

                <MdIconButton
                  size="sm"
                  variant="standard"
                  :icon="LuTrash2"
                  :disabled="deletingBookmarkIds.has(entry.bookmark.id)"
                  :aria-label="t('common.delete')"
                  @click="handleDeleteBookmark(entry)"
                />
              </div>
            </div>
          </section>
        </div>

        <!-- TAB: FAVORITES -->
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

        <!-- TAB: COLLECTIONS (Shelves) -->
        <div
          v-else-if="activeTab === 'collections' && !activeCollectionId"
          class="grid grid-cols-1 gap-3 sm:grid-cols-2 lg:grid-cols-3"
        >
          <div
            v-for="shelf in filteredShelves"
            :key="shelf.id"
            class="flex items-center justify-between gap-2 rounded-corner-md bg-surface-container-low px-4 py-3.5 transition-colors duration-150 ease-standard hover:bg-surface-container"
          >
            <button
              type="button"
              class="md-state flex min-w-0 flex-1 cursor-pointer items-center gap-3 rounded-corner-sm text-left"
              @click="selectShelfCollection(shelf.id)"
            >
              <div
                class="flex h-10 w-10 shrink-0 items-center justify-center rounded-corner-md bg-secondary-container text-on-secondary-container"
              >
                <LuFolder class="h-5 w-5" aria-hidden="true" />
              </div>
              <div class="min-w-0 flex-1">
                <div class="flex items-center gap-2">
                  <span class="text-title-small truncate text-on-surface">
                    {{ shelf.name }}
                  </span>
                  <span
                    v-if="shelf.is_favorite"
                    class="text-label-small shrink-0 rounded-corner-full bg-primary-container px-2 py-0.5 text-on-primary-container"
                  >
                    {{ t('collections.defaultFavoriteBadge') }}
                  </span>
                </div>
                <p class="text-body-small mt-0.5 tabular-nums text-on-surface-variant">
                  {{ t('collections.bookCountUnit', { count: shelf.book_count }) }}
                </p>
              </div>
            </button>

            <MdMenu
              v-if="!shelf.is_favorite"
              placement="bottom-end"
              :aria-label="t('detail.moreActions')"
            >
              <template #trigger="{ toggle, triggerAttrs }">
                <MdIconButton
                  v-bind="triggerAttrs"
                  size="sm"
                  variant="standard"
                  :icon="LuMoreVertical"
                  :aria-label="t('detail.moreActions')"
                  @click="toggle"
                />
              </template>
              <MdMenuItem
                :icon="LuPencil"
                :label="t('collections.renameShelf')"
                @select="handleRenameShelf(shelf)"
              />
              <MdMenuItem
                :icon="LuTrash2"
                :label="t('collections.deleteShelf')"
                destructive
                @select="handleDeleteShelf(shelf)"
              />
            </MdMenu>
          </div>
        </div>

        <!-- VIEW: AUTHORS -->
        <div
          v-else-if="activeView === 'authors'"
          class="flex flex-wrap gap-2"
        >
          <MdChip
            v-for="author in filteredAuthors"
            :key="author.name"
            variant="assist"
            :icon="LuUser"
            :label="`${author.name} (${author.count})`"
            @click="selectDimensionAuthor(author.name)"
          />
        </div>

        <!-- VIEW: TAGS -->
        <div
          v-else-if="activeView === 'tags'"
          class="flex flex-wrap gap-2"
        >
          <MdChip
            v-for="tag in filteredTags"
            :key="tag.name"
            variant="assist"
            :icon="LuTag"
            :label="`#${tag.name} (${tag.count})`"
            @click="selectDimensionTag(tag.name)"
          />
        </div>

        <!-- VIEW: ITEMS (Grid or List) -->
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

        <!-- Clean MD3 Table View -->
        <div
          v-else
          class="overflow-x-auto rounded-corner-md bg-surface-container-low"
        >
          <table class="w-full border-collapse text-left">
            <thead>
              <tr class="text-label-medium border-b border-outline-variant text-on-surface-variant">
                <th class="w-10 py-3 pl-4">
                  <AppCheckbox
                    :model-value="allCurrentSelected"
                    @update:model-value="toggleSelectAll"
                  />
                </th>
                <th class="w-10 py-3" />
                <th class="py-3 pr-4">{{ t('catalog.colTitle') }}</th>
                <th class="hidden py-3 pr-4 md:table-cell">{{ t('catalog.colAuthor') }}</th>
                <th class="hidden py-3 pr-4 sm:table-cell">{{ t('catalog.colProgress') }}</th>
                <th class="py-3 pr-4">{{ t('catalog.colFormat') }}</th>
                <th class="hidden py-3 pr-4 text-right sm:table-cell">
                  {{ t('catalog.colSize') }}
                </th>
              </tr>
            </thead>
            <tbody class="divide-y divide-outline-variant/50">
              <tr
                v-for="book in catalogStore.books"
                :key="book.id"
                tabindex="0"
                class="group cursor-pointer transition-colors duration-150 ease-standard hover:bg-surface-container focus-visible:outline-2 focus-visible:-outline-offset-2 focus-visible:outline-secondary"
                @click="router.push(itemRoute({ scope: 'local', domain: 'book', id: book.id }))"
                @keydown.enter.self="router.push(itemRoute({ scope: 'local', domain: 'book', id: book.id }))"
                @keydown.space.self.prevent="router.push(itemRoute({ scope: 'local', domain: 'book', id: book.id }))"
              >
                <td class="py-2.5 pl-4" @click.stop>
                  <AppCheckbox
                    :model-value="catalogStore.selectedBookIds.has(book.id)"
                    @update:model-value="catalogStore.toggleSelectBook(book.id)"
                  />
                </td>
                <td class="py-2.5 pr-1" @click.stop>
                  <MdIconButton
                    size="sm"
                    variant="standard"
                    :selected="book.is_favorite"
                    :aria-label="
                      book.is_favorite ? t('detail.favoriteRemove') : t('detail.favoriteAdd')
                    "
                    @click="toggleBookFavorite(book)"
                  >
                    <LuHeart
                      class="h-4 w-4"
                      :class="book.is_favorite ? 'fill-current text-tertiary' : ''"
                    />
                  </MdIconButton>
                </td>
                <td class="py-2.5 pr-4">
                  <div class="flex items-center gap-3">
                    <div class="h-10 w-7 shrink-0 overflow-hidden rounded-corner-xs">
                      <MediaCoverImage
                        :src="book.has_cover ? api.coverUrl(book.id, book.updated_at) : null"
                        :alt="book.metadata.title"
                        aspect-class="aspect-[2/3]"
                        rounded="rounded-corner-xs"
                        bg-class="bg-surface-container-high"
                        :fallback-icon="LuBookOpen"
                        fallback-icon-class="h-3.5 w-3.5 text-on-surface-variant/60"
                      />
                    </div>
                    <div class="min-w-0">
                      <p class="text-title-small truncate text-on-surface group-hover:text-primary">
                        {{ book.metadata.title }}
                      </p>
                      <div v-if="book.metadata.tags.length > 0" class="mt-0.5 flex flex-wrap gap-1">
                        <span
                          v-for="tg in book.metadata.tags.slice(0, 2)"
                          :key="tg"
                          class="text-label-small rounded-corner-xs bg-surface-container-high px-1.5 py-0.5 text-on-surface-variant"
                        >
                          {{ tg }}
                        </span>
                      </div>
                    </div>
                  </div>
                </td>
                <td class="text-body-small hidden py-2.5 pr-4 text-on-surface-variant md:table-cell">
                  {{ book.metadata.authors.join(', ') || t('catalog.unknownAuthor') }}
                </td>
                <td class="text-label-medium hidden py-2.5 pr-4 tabular-nums sm:table-cell">
                  <span v-if="hasBookProgress(book.progress)" class="text-primary">
                    {{ formatProgressPercentValue(normalizeBookProgressPercent(book.progress) ?? 0) }}%
                  </span>
                  <span v-else class="text-on-surface-variant">—</span>
                </td>
                <td class="py-2.5 pr-4">
                  <span
                    class="text-label-small rounded-corner-xs bg-surface-container-high px-2 py-0.5 font-mono uppercase text-on-surface-variant"
                  >
                    {{ book.format }}
                  </span>
                </td>
                <td
                  class="text-body-small hidden py-2.5 pr-4 text-right tabular-nums text-on-surface-variant sm:table-cell"
                >
                  {{ formatBytes(book.file_size) }}
                </td>
              </tr>
            </tbody>
          </table>
        </div>
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

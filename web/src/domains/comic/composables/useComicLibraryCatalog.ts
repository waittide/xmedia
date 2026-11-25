import { computed, onMounted, ref, watch } from 'vue'
import { useI18n as useGlobalI18n } from 'vue-i18n'
import { useRoute, useRouter, type LocationQueryRaw } from 'vue-router'
import type { MdTabItem } from '@/components/ui'
import { api } from '@/domains/comic/api'
import { useComicI18n as useI18n } from '@/domains/comic/i18n'
import { useSidebarStore } from '@/domains/comic/store'
import type {
  ComicBookmarkItemDto,
  SeriesStatus,
  SeriesSummaryDto,
  ShelfDetailDto,
  TaxonomyCountDto,
} from '@/domains/comic/types'
import { seriesToMediaItemVM } from '@/domains/comic/utils/mappers'
import {
  libraryRoute,
  useMergedShelf,
  useSourceFilter,
  type LibraryTab,
  type MediaItemVM,
  type SourceScope,
} from '@/sources'
import { useAppClientStore } from '@/stores/appServers'
import { useDialogStore } from '@/stores/dialog'
import { useSnackbarStore } from '@/stores/snackbar'
import { useUploadStore } from '@/stores/uploads'

export type ComicBrowseView = 'items' | 'genres' | 'writers' | 'pencillers' | 'tags'
export type ComicSortKey = 'updated_at' | 'created_at' | 'title' | 'chapter_count'
export type ComicSortOrder = 'desc' | 'asc'
export type ComicReadFilter = '' | 'unread' | 'in_progress' | 'completed'

const VALID_TABS: readonly LibraryTab[] = ['all', 'continue', 'favorites', 'collections']
const VALID_VIEWS: readonly ComicBrowseView[] = ['items', 'genres', 'writers', 'pencillers', 'tags']
const VALID_SORTS: readonly ComicSortKey[] = ['updated_at', 'created_at', 'title', 'chapter_count']

export interface UseComicLibraryCatalogOptions {
  presetMode: () => 'all' | 'continue' | 'favorites' | undefined
  submitOnlineSearch: (query: string) => void
  clearOnlineSearch: () => void
  refreshOnlineSearch: () => Promise<void>
}

export function useComicLibraryCatalog(options: UseComicLibraryCatalogOptions) {
  const { t } = useI18n()
  const { t: globalT } = useGlobalI18n()
  const route = useRoute()
  const router = useRouter()
  const appClientStore = useAppClientStore()
  const upload = useUploadStore()
  const sidebarStore = useSidebarStore()
  const snackbarStore = useSnackbarStore()
  const dialogStore = useDialogStore()

  const sourceFilter = useSourceFilter('comic')
  const filterSheetOpen = ref(false)

  function readQueryStr(key: string): string {
    const raw = route.query[key]
    if (typeof raw === 'string' && raw.trim()) return raw.trim()
    if (Array.isArray(raw)) {
      const hit = raw.find((v): v is string => typeof v === 'string' && v.trim().length > 0)
      if (hit) return hit.trim()
    }
    return ''
  }

  const scope = computed<SourceScope>(() => sourceFilter.filter.value.scope)
  const activeSourceId = computed<string | null>(() => sourceFilter.filter.value.sourceId)

  const showRemoteScope = computed(
    () =>
      appClientStore.isStandaloneApp &&
      (sourceFilter.hasRemoteSources.value ||
        appClientStore.opdsSources.length > 0 ||
        scope.value === 'remote'),
  )

  const activeTab = computed<LibraryTab>(() => {
    if (scope.value !== 'local') return 'all'
    const raw = readQueryStr('tab')
    if (raw && (VALID_TABS as readonly string[]).includes(raw)) {
      return raw as LibraryTab
    }
    const preset = options.presetMode()
    if (preset === 'continue') return 'continue'
    if (preset === 'favorites') return 'favorites'
    return 'all'
  })

  const collectionId = computed(() => readQueryStr('collection'))

  const activeView = computed<ComicBrowseView>(() => {
    if (scope.value !== 'local' || activeTab.value !== 'all') return 'items'
    if (collectionId.value) return 'items'
    const raw = readQueryStr('view')
    if (raw && (VALID_VIEWS as readonly string[]).includes(raw)) {
      return raw as ComicBrowseView
    }
    return 'items'
  })

  const queryFromRoute = computed(() => readQueryStr('q'))
  const searchInput = ref(queryFromRoute.value)

  const libraryId = computed(() => readQueryStr('library'))
  const writerFilter = computed(() => readQueryStr('writer'))
  const pencillerFilter = computed(() => readQueryStr('penciller'))
  const genreFilter = computed(() => readQueryStr('genre'))
  const tagFilter = computed(() => readQueryStr('tag'))

  const statusFilter = computed<SeriesStatus | ''>(() => {
    const raw = readQueryStr('status')
    if (raw === 'ongoing' || raw === 'completed' || raw === 'hiatus') return raw
    return ''
  })

  const readStatusFilter = computed<ComicReadFilter>(() => {
    const raw = readQueryStr('read_status')
    if (raw === 'unread' || raw === 'in_progress' || raw === 'completed') return raw
    return ''
  })

  const sortBy = computed<ComicSortKey>(() => {
    const raw = readQueryStr('sort')
    if (raw && (VALID_SORTS as readonly string[]).includes(raw)) {
      return raw as ComicSortKey
    }
    return 'updated_at'
  })

  const sortOrder = computed<ComicSortOrder>(() => {
    const raw = readQueryStr('order')
    return raw === 'asc' ? 'asc' : 'desc'
  })

  const activeCollection = computed<ShelfDetailDto | null>(() => {
    if (!collectionId.value) return null
    return sidebarStore.shelves.find((s) => s.shelf.id === collectionId.value) ?? null
  })

  const isLocalItemsView = computed(
    () => scope.value === 'local' && activeTab.value === 'all' && activeView.value === 'items',
  )
  const isTaxonomyView = computed(
    () => scope.value === 'local' && activeTab.value === 'all' && activeView.value !== 'items',
  )
  const isCollectionsView = computed(
    () => scope.value === 'local' && activeTab.value === 'collections',
  )
  const isMergedTab = computed(
    () =>
      scope.value === 'local' &&
      (activeTab.value === 'continue' || activeTab.value === 'favorites'),
  )
  const isPoolBrowseView = computed(() => scope.value === 'pool')
  const isOnlineBrowseView = computed(() => scope.value === 'online')
  const isRemoteBrowseView = computed(() => scope.value === 'remote')

  const hasLocalFilterOverrides = computed(() =>
    Boolean(
      libraryId.value ||
        collectionId.value ||
        writerFilter.value ||
        pencillerFilter.value ||
        genreFilter.value ||
        tagFilter.value ||
        statusFilter.value ||
        readStatusFilter.value,
    ),
  )

  const mergedScope = computed<SourceScope | 'all'>(() => {
    if (scope.value === 'online') return 'online'
    if (scope.value === 'remote') return 'remote'
    if (readQueryStr('scope') === 'local' || hasLocalFilterOverrides.value) return 'local'
    return 'all'
  })

  const mergedView = computed<'continue' | 'favorites'>(() =>
    activeTab.value === 'favorites' ? 'favorites' : 'continue',
  )

  const mergedShelf = useMergedShelf('comic', mergedView, {
    scope: mergedScope,
    sourceId: activeSourceId,
    query: queryFromRoute,
    limit: 100,
    autoLoad: false,
  })

  const seriesList = ref<SeriesSummaryDto[]>([])
  const localLoading = ref(false)
  const manualRefreshing = ref(false)
  const localError = ref<string | null>(null)
  let fetchSeq = 0

  const selectedRemoteSourceId = computed(() => {
    const fromUrl = activeSourceId.value
    if (fromUrl && appClientStore.opdsSources.some((s) => s.id === fromUrl)) {
      return fromUrl
    }
    return appClientStore.opdsSources[0]?.id ?? ''
  })

  const shelfDialogOpen = ref(false)
  const editingShelf = ref<ShelfDetailDto | null>(null)

  const tabs = computed<MdTabItem<LibraryTab>[]>(() =>
    scope.value === 'local'
      ? [
          { value: 'all', label: t('library.tabAll') },
          { value: 'continue', label: t('library.tabContinue') },
          { value: 'favorites', label: t('library.tabFavorites') },
          { value: 'collections', label: t('library.tabCollections') },
        ]
      : [],
  )

  const viewOptions = computed<Array<{ value: ComicBrowseView; label: string }>>(() => [
    { value: 'items', label: t('library.viewItems') },
    { value: 'genres', label: t('library.viewGenres') },
    { value: 'writers', label: t('library.viewWriters') },
    { value: 'pencillers', label: t('library.viewPencillers') },
    { value: 'tags', label: t('library.viewTags') },
  ])

  const activeFilterCount = computed(() => {
    let count = 0
    if (libraryId.value) count += 1
    if (statusFilter.value) count += 1
    if (readStatusFilter.value) count += 1
    if (sortBy.value !== 'updated_at' || sortOrder.value !== 'desc') count += 1
    return count
  })

  const activeFilterChips = computed<Array<{ key: string; label: string }>>(() => {
    const chips: Array<{ key: string; label: string }> = []
    if (libraryId.value) {
      const libName =
        sidebarStore.libraries.find((l) => l.id === libraryId.value)?.name || libraryId.value
      chips.push({ key: 'library', label: `${t('series.filterLibrary')}: ${libName}` })
    }
    if (statusFilter.value) {
      const label =
        statusFilter.value === 'completed'
          ? t('series.statusCompleted')
          : statusFilter.value === 'hiatus'
            ? t('series.statusHiatus')
            : t('series.statusOngoing')
      chips.push({ key: 'status', label })
    }
    if (readStatusFilter.value) {
      const label =
        readStatusFilter.value === 'completed'
          ? t('series.readCompleted')
          : readStatusFilter.value === 'in_progress'
            ? t('series.readInProgress')
            : t('series.readUnread')
      chips.push({ key: 'read_status', label })
    }
    if (writerFilter.value) {
      chips.push({ key: 'writer', label: `${t('series.writers')}: ${writerFilter.value}` })
    }
    if (pencillerFilter.value) {
      chips.push({ key: 'penciller', label: `${t('series.pencillers')}: ${pencillerFilter.value}` })
    }
    if (genreFilter.value) {
      chips.push({ key: 'genre', label: `${t('series.genres')}: ${genreFilter.value}` })
    }
    if (tagFilter.value) {
      chips.push({ key: 'tag', label: `#${tagFilter.value}` })
    }
    return chips
  })

  const localSeriesVMs = computed<MediaItemVM[]>(() =>
    seriesList.value.map((series) =>
      seriesToMediaItemVM(series, {
        chaptersCountLabel: t('home.chaptersCount', { count: series.chapter_count }),
        statusLabel:
          series.status === 'completed'
            ? t('series.statusCompleted')
            : series.status === 'hiatus'
              ? t('series.statusHiatus')
              : t('series.statusOngoing'),
      }),
    ),
  )

  const taxonomyItems = computed<TaxonomyCountDto[]>(() => {
    let raw: TaxonomyCountDto[] = []
    if (activeView.value === 'genres') raw = sidebarStore.genres
    else if (activeView.value === 'writers') raw = sidebarStore.writers
    else if (activeView.value === 'pencillers') raw = sidebarStore.pencillers
    else if (activeView.value === 'tags') raw = sidebarStore.tags

    const q = queryFromRoute.value.toLowerCase()
    const filtered = q ? raw.filter((item) => item.name.toLowerCase().includes(q)) : raw
    return [...filtered].sort((a, b) => b.count - a.count || a.name.localeCompare(b.name))
  })

  const filteredShelves = computed<ShelfDetailDto[]>(() => {
    const q = queryFromRoute.value.toLowerCase()
    if (!q) return sidebarStore.shelves
    return sidebarStore.shelves.filter(
      (s) =>
        s.shelf.name.toLowerCase().includes(q) || s.shelf.description.toLowerCase().includes(q),
    )
  })

  const filteredBookmarks = computed<ComicBookmarkItemDto[]>(() => {
    if (scope.value !== 'local' || activeTab.value !== 'continue') return []
    const q = queryFromRoute.value.toLowerCase()
    if (!q) return sidebarStore.bookmarks
    return sidebarStore.bookmarks.filter(
      (bm) =>
        bm.series_title.toLowerCase().includes(q) ||
        bm.chapter_title.toLowerCase().includes(q) ||
        bm.bookmark.title.toLowerCase().includes(q) ||
        (bm.bookmark.note?.toLowerCase().includes(q) ?? false),
    )
  })

  const headerTitle = computed(() => {
    if (activeCollection.value) return activeCollection.value.shelf.name
    if (activeTab.value === 'continue') return t('library.tabContinue')
    if (activeTab.value === 'favorites') return t('library.tabFavorites')
    if (activeTab.value === 'collections') return t('library.tabCollections')
    return t('library.title')
  })

  const headerCount = computed<number | null>(() => {
    if (isPoolBrowseView.value || isOnlineBrowseView.value || isRemoteBrowseView.value) return null
    if (isMergedTab.value) return mergedShelf.items.value.length
    if (isCollectionsView.value) return filteredShelves.value.length
    if (isTaxonomyView.value) return taxonomyItems.value.length
    return seriesList.value.length
  })

  const templateLoading = computed(() => {
    if (isPoolBrowseView.value || isOnlineBrowseView.value || isRemoteBrowseView.value) return false
    if (isMergedTab.value) return mergedShelf.loading.value && mergedShelf.items.value.length === 0
    if (isCollectionsView.value || isTaxonomyView.value) return sidebarStore.loading
    return localLoading.value
  })

  const templateError = computed(() => {
    if (isPoolBrowseView.value || isOnlineBrowseView.value || isRemoteBrowseView.value) return null
    if (isMergedTab.value) return mergedShelf.error.value
    if (isLocalItemsView.value) return localError.value
    return null
  })

  const templateEmpty = computed(() => {
    if (isPoolBrowseView.value || isOnlineBrowseView.value || isRemoteBrowseView.value) return false
    if (isMergedTab.value) {
      return mergedShelf.items.value.length === 0 && filteredBookmarks.value.length === 0
    }
    if (isCollectionsView.value) return filteredShelves.value.length === 0
    if (isTaxonomyView.value) return taxonomyItems.value.length === 0
    return seriesList.value.length === 0
  })

  const templateEmptyTitle = computed(() => {
    if (activeTab.value === 'continue') return t('library.emptyContinue')
    if (activeTab.value === 'favorites') return t('library.emptyFavorites')
    if (activeTab.value === 'collections') return t('shelves.emptyShelves')
    if (isTaxonomyView.value) return t('library.emptyTaxonomy')
    return t('series.emptyFiltered')
  })

  function updateQuery(patch: Record<string, string | null | undefined>): void {
    const next: LocationQueryRaw = {}
    for (const [k, v] of Object.entries(route.query)) {
      if (v !== undefined && v !== null) {
        next[k] = v
      }
    }
    for (const [k, v] of Object.entries(patch)) {
      if (v === null || v === undefined || v === '') {
        delete next[k]
      } else {
        next[k] = v
      }
    }
    void router.replace({ path: '/comic/library', query: next })
  }

  async function handleScopeChange(nextScope: SourceScope): Promise<void> {
    await sourceFilter.setScope(nextScope)
  }

  function handleTabChange(nextTab: string): void {
    const tab: LibraryTab = (VALID_TABS as readonly string[]).includes(nextTab)
      ? (nextTab as LibraryTab)
      : 'all'
    updateQuery({
      tab: tab === 'all' ? null : tab,
      collection: null,
      view: null,
      page: null,
    })
  }

  function handleViewChange(nextView: ComicBrowseView): void {
    updateQuery({
      view: nextView === 'items' ? null : nextView,
      collection: null,
      page: null,
    })
  }

  function selectTaxonomyValue(name: string): void {
    const patch: Record<string, string | null> = { view: null, page: null }
    if (activeView.value === 'genres') patch.genre = name
    else if (activeView.value === 'writers') patch.writer = name
    else if (activeView.value === 'pencillers') patch.penciller = name
    else if (activeView.value === 'tags') patch.tag = name
    updateQuery(patch)
  }

  function handleSearchSubmit(nextQuery: string): void {
    const trimmed = nextQuery.trim()
    updateQuery({ q: trimmed || null, page: null })
    if (isOnlineBrowseView.value) {
      options.submitOnlineSearch(trimmed)
    }
  }

  function handleSearchClear(): void {
    searchInput.value = ''
    updateQuery({ q: null, page: null })
    if (isOnlineBrowseView.value) {
      options.clearOnlineSearch()
    }
  }

  function handleOnlineKeywordSync(nextKw: string): void {
    searchInput.value = nextKw
    updateQuery({ q: nextKw.trim() || null })
  }

  function handleOnlineSourceSync(nextSourceId: string | null): void {
    void sourceFilter.setSource(nextSourceId)
  }

  function removeFilterChip(key: string): void {
    updateQuery({ [key]: null, page: null })
  }

  function resetLocalFilters(): void {
    updateQuery({
      library: null,
      collection: null,
      writer: null,
      penciller: null,
      genre: null,
      tag: null,
      status: null,
      read_status: null,
      sort: null,
      order: null,
      page: null,
    })
  }

  async function fetchLocalSeries(): Promise<void> {
    if (!isLocalItemsView.value) {
      localLoading.value = false
      return
    }
    const seq = ++fetchSeq
    localLoading.value = true
    localError.value = null
    try {
      const list = await api.listSeries({
        search: queryFromRoute.value || undefined,
        library_id: libraryId.value || undefined,
        shelf_id: collectionId.value || undefined,
        status: statusFilter.value || undefined,
        read_status: readStatusFilter.value || undefined,
        writer: writerFilter.value || undefined,
        penciller: pencillerFilter.value || undefined,
        tag: tagFilter.value || undefined,
        genre: genreFilter.value || undefined,
        sort_by: sortBy.value,
        sort_dir: sortOrder.value,
      })
      if (seq !== fetchSeq) return
      seriesList.value = list
    } catch (err) {
      if (seq !== fetchSeq) return
      localError.value = err instanceof Error ? err.message : String(err)
    } finally {
      if (seq === fetchSeq) {
        localLoading.value = false
      }
    }
  }

  async function refreshActiveView(): Promise<void> {
    if (manualRefreshing.value) return
    manualRefreshing.value = true
    try {
      if (isOnlineBrowseView.value) {
        await options.refreshOnlineSearch()
        return
      }
      if (isMergedTab.value) {
        await Promise.allSettled([mergedShelf.refresh(), sidebarStore.refresh()])
        return
      }
      await Promise.allSettled([sidebarStore.refresh(), fetchLocalSeries()])
    } finally {
      manualRefreshing.value = false
    }
  }

  async function toggleLocalSeriesFavorite(series: SeriesSummaryDto): Promise<void> {
    const next = !series.is_favorite
    try {
      await api.setFavorite(series.id, next)
      series.is_favorite = next
    } catch (err) {
      snackbarStore.show({
        message: err instanceof Error ? err.message : String(err),
        tone: 'error',
      })
    }
  }

  async function handleUnbindLocalSeries(series: SeriesSummaryDto): Promise<void> {
    const ok = await dialogStore.confirm({
      title: globalT('pool.unbindConfirmTitle'),
      message: series.online_tracking
        ? globalT('pool.unbindOnlineConfirmMessage', { title: series.title })
        : globalT('pool.unbindConfirmMessage', { title: series.title }),
      danger: true,
      confirmText: globalT('pool.unbindBtn'),
    })
    if (!ok) return
    try {
      await api.unbindItem(series.id)
      snackbarStore.show(globalT('pool.unboundToast'))
      upload.notifyCatalogChanged()
      await Promise.allSettled([sidebarStore.refresh(), fetchLocalSeries()])
    } catch (err) {
      snackbarStore.show({
        message: err instanceof Error ? err.message : String(err),
        tone: 'error',
      })
    }
  }

  function handlePoolChanged(): void {
    upload.notifyCatalogChanged()
    void sidebarStore.refresh()
  }

  function openCreateShelf(): void {
    editingShelf.value = null
    shelfDialogOpen.value = true
  }

  function openEditShelf(sh: ShelfDetailDto): void {
    editingShelf.value = sh
    shelfDialogOpen.value = true
  }

  async function handleDeleteShelf(sh: ShelfDetailDto): Promise<void> {
    const ok = await dialogStore.confirm({
      title: t('library.deleteShelfConfirmTitle'),
      description: t('library.deleteShelfConfirmDesc', { name: sh.shelf.name }),
      danger: true,
      confirmText: t('common.delete'),
    })
    if (!ok) return
    try {
      await api.deleteShelf(sh.shelf.id)
      if (collectionId.value === sh.shelf.id) {
        updateQuery({ collection: null, tab: 'collections' })
      }
      await sidebarStore.refresh()
    } catch (err) {
      snackbarStore.show({
        message: err instanceof Error ? err.message : String(err),
        tone: 'error',
      })
    }
  }

  function openCollection(shelfId: string): void {
    void router.push(libraryRoute('comic', { collection: shelfId }))
  }

  function onRemoteSourceSelect(nextId: string): void {
    void sourceFilter.setSource(nextId || null)
  }

  function goRemoteSettings(): void {
    void router.push({ path: '/comic/settings', query: { tab: 'sources', kind: 'remote' } })
  }

  watch(queryFromRoute, (next) => {
    if (next !== searchInput.value) {
      searchInput.value = next
    }
  })

  watch(
    [
      isLocalItemsView,
      queryFromRoute,
      libraryId,
      collectionId,
      statusFilter,
      readStatusFilter,
      writerFilter,
      pencillerFilter,
      genreFilter,
      tagFilter,
      sortBy,
      sortOrder,
      () => upload.catalogVersion,
    ],
    () => {
      if (isLocalItemsView.value) {
        void fetchLocalSeries()
      }
    },
  )

  watch(
    [
      isMergedTab,
      mergedView,
      mergedScope,
      activeSourceId,
      queryFromRoute,
      () => upload.catalogVersion,
    ],
    () => {
      if (isMergedTab.value) {
        void mergedShelf.refresh()
      }
    },
  )

  onMounted(async () => {
    if (sidebarStore.libraries.length === 0 && sidebarStore.shelves.length === 0) {
      await sidebarStore.refresh().catch(() => {})
    }
    if (isLocalItemsView.value) {
      await fetchLocalSeries()
    } else if (isMergedTab.value) {
      await mergedShelf.refresh()
    }
  })

  return {
    scope,
    activeSourceId,
    showRemoteScope,
    activeTab,
    activeView,
    queryFromRoute,
    searchInput,
    libraryId,
    collectionId,
    statusFilter,
    readStatusFilter,
    sortBy,
    sortOrder,
    activeCollection,
    isLocalItemsView,
    isTaxonomyView,
    isCollectionsView,
    isMergedTab,
    isPoolBrowseView,
    isOnlineBrowseView,
    isRemoteBrowseView,
    mergedShelf,
    seriesList,
    manualRefreshing,
    selectedRemoteSourceId,
    filterSheetOpen,
    shelfDialogOpen,
    editingShelf,
    tabs,
    viewOptions,
    activeFilterCount,
    activeFilterChips,
    localSeriesVMs,
    taxonomyItems,
    filteredShelves,
    filteredBookmarks,
    headerTitle,
    headerCount,
    templateLoading,
    templateError,
    templateEmpty,
    templateEmptyTitle,
    updateQuery,
    handleScopeChange,
    handleTabChange,
    handleViewChange,
    selectTaxonomyValue,
    handleSearchSubmit,
    handleSearchClear,
    handleOnlineKeywordSync,
    handleOnlineSourceSync,
    removeFilterChip,
    resetLocalFilters,
    refreshActiveView,
    toggleLocalSeriesFavorite,
    handleUnbindLocalSeries,
    handlePoolChanged,
    openCreateShelf,
    openEditShelf,
    handleDeleteShelf,
    openCollection,
    onRemoteSourceSelect,
    goRemoteSettings,
  }
}

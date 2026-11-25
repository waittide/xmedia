import { computed, onBeforeUnmount, onMounted, ref, watch } from 'vue'
import { useI18n as useGlobalI18n } from 'vue-i18n'
import { useRoute, useRouter, type LocationQueryRaw } from 'vue-router'
import type { MdTabItem, SelectOption } from '@/components/ui'
import { api } from '@/domains/video/api'
import { useVideoI18n as useI18n } from '@/domains/video/i18n'
import { useVideoStore } from '@/domains/video/store'
import type {
  ShowSummaryDto,
  VideoKindFilter,
  VideoShelfDetailDto,
  VideoSortBy,
  VideoSummaryDto,
} from '@/domains/video/types'
import {
  useMergedShelf,
  useSourceFilter,
  type LibraryTab,
  type MediaItemVM,
  type SourceScope,
} from '@/sources'
import { useDialogStore } from '@/stores/dialog'
import { useSnackbarStore } from '@/stores/snackbar'
import { useUploadStore } from '@/stores/uploads'

export type VideoLibraryView = 'items' | 'shows' | 'movies' | 'genres' | 'tags'

const VALID_TABS: readonly LibraryTab[] = ['all', 'continue', 'favorites', 'collections']
const VALID_VIEWS: readonly VideoLibraryView[] = ['items', 'shows', 'movies', 'genres', 'tags']

function isValidTab(raw: string): raw is LibraryTab {
  return (VALID_TABS as readonly string[]).includes(raw)
}

function isValidView(raw: string): raw is VideoLibraryView {
  return (VALID_VIEWS as readonly string[]).includes(raw)
}

function isVideoSortBy(raw: string): raw is VideoSortBy {
  return raw === 'title' || raw === 'year' || raw === 'duration' || raw === 'created_at'
}

function isMovieItemRoute(to: MediaItemVM['to']): boolean {
  if (typeof to !== 'object' || to === null || !('query' in to)) return false
  const q = to.query
  return typeof q === 'object' && q !== null && 'kind' in q && q.kind === 'movie'
}

function normalizeMergedItem(item: MediaItemVM): MediaItemVM {
  if (item.ref.scope !== 'local') return item
  const target = item.to
  if (typeof target === 'object' && target && 'path' in target && typeof target.path === 'string') {
    if (target.path.startsWith('/video/play/')) {
      return {
        ...item,
        to: { path: `/video/item/${encodeURIComponent(item.ref.id)}`, query: { kind: 'movie' } },
      }
    }
  }
  return item
}

export interface UseVideoLibraryCatalogOptions {
  submitOnlineKeyword: (keyword: string) => void
  clearOnlineKeyword: () => void
  refreshOnlineSearch: () => Promise<void>
}

export function useVideoLibraryCatalog(options: UseVideoLibraryCatalogOptions) {
  const { t } = useI18n()
  const { t: globalT } = useGlobalI18n()
  const route = useRoute()
  const router = useRouter()
  const videoStore = useVideoStore()
  const uploadStore = useUploadStore()
  const dialogStore = useDialogStore()
  const snackbar = useSnackbarStore()

  const sourceFilter = useSourceFilter('video')

  const filterSheetOpen = ref(false)
  const editingVideo = ref<VideoSummaryDto | null>(null)

  const videos = ref<VideoSummaryDto[]>([])
  const shows = ref<ShowSummaryDto[]>([])
  const shelfDetail = ref<VideoShelfDetailDto | null>(null)
  const localLoading = ref(false)
  const localError = ref<string | null>(null)
  const manualRefreshing = ref(false)

  let fetchSeq = 0
  let searchDebounceTimer: ReturnType<typeof setTimeout> | null = null

  function readQueryStr(key: string): string {
    const raw = route.query[key]
    if (typeof raw === 'string') return raw.trim()
    if (Array.isArray(raw)) {
      const first = raw.find((v): v is string => typeof v === 'string' && v.trim().length > 0)
      return first?.trim() ?? ''
    }
    return ''
  }

  const activeScope = computed<SourceScope>(() => {
    const scope = sourceFilter.filter.value.scope
    return scope === 'online' || scope === 'pool' ? scope : 'local'
  })
  const onlineSourceId = computed<string | null>(() => sourceFilter.filter.value.sourceId)

  const activeCollectionId = computed(() =>
    activeScope.value === 'local' ? readQueryStr('collection') : '',
  )

  const activeTab = computed<LibraryTab>(() => {
    if (activeScope.value !== 'local') return 'all'
    const raw = readQueryStr('tab')
    return isValidTab(raw) ? raw : 'all'
  })

  const activeView = computed<VideoLibraryView>(() => {
    const raw = readQueryStr('view')
    return isValidView(raw) ? raw : 'items'
  })

  const selectedLibrary = computed(() => readQueryStr('library'))
  const selectedGenre = computed(() => readQueryStr('genre'))
  const selectedTag = computed(() => readQueryStr('tag'))
  const kindFilter = computed<VideoKindFilter>(() => {
    if (activeView.value === 'movies') return 'standalone'
    const raw = readQueryStr('kind')
    if (raw === 'standalone' || raw === 'episodes') return raw
    return 'all'
  })

  const sortBy = computed<VideoSortBy>(() => {
    const raw = readQueryStr('sort')
    return isVideoSortBy(raw) ? raw : 'created_at'
  })
  const sortOrder = computed<'asc' | 'desc'>(() => {
    const raw = readQueryStr('order')
    if (raw === 'asc' || raw === 'desc') return raw
    return sortBy.value === 'title' ? 'asc' : 'desc'
  })
  const sortKey = computed(() => `${sortBy.value}:${sortOrder.value}`)

  const routeQueryQ = computed(() => readQueryStr('q'))
  const searchInput = ref(routeQueryQ.value)

  watch(routeQueryQ, (next) => {
    if (next !== searchInput.value) {
      searchInput.value = next
    }
  })

  const isShelfTab = computed(
    () =>
      activeScope.value === 'local' &&
      !activeCollectionId.value &&
      (activeTab.value === 'continue' || activeTab.value === 'favorites'),
  )
  const mergedShelfView = computed<'continue' | 'favorites'>(() =>
    activeTab.value === 'favorites' ? 'favorites' : 'continue',
  )

  const mergedShelf = useMergedShelf('video', mergedShelfView, {
    scope: 'local',
    query: routeQueryQ,
    limit: 100,
    autoLoad: false,
  })

  const mergedShelfItems = computed<MediaItemVM[]>(() =>
    mergedShelf.items.value.map(normalizeMergedItem),
  )

  function updateLibraryQuery(patch: Record<string, string | null | undefined>): void {
    const next: LocationQueryRaw = {}
    for (const [k, v] of Object.entries(route.query)) {
      if (v !== undefined && v !== null) {
        next[k] = v
      }
    }
    for (const [k, v] of Object.entries(patch)) {
      if (v === undefined || v === null || v === '') {
        delete next[k]
      } else {
        next[k] = v
      }
    }
    if (next.scope === 'local') delete next.scope
    if (next.tab === 'all') delete next.tab
    if (next.view === 'items') delete next.view
    if (next.kind === 'all') delete next.kind
    if (next.sort === 'created_at' && (!next.order || next.order === 'desc')) {
      delete next.sort
      delete next.order
    }
    void router.replace({ path: '/video/library', query: next })
  }

  async function handleScopeChange(nextScope: SourceScope): Promise<void> {
    if (searchDebounceTimer) {
      clearTimeout(searchDebounceTimer)
      searchDebounceTimer = null
    }
    await sourceFilter.setScope(nextScope)
  }

  function handleTabChange(nextTab: string): void {
    const tab: LibraryTab = isValidTab(nextTab) ? nextTab : 'all'
    updateLibraryQuery({
      tab: tab === 'all' ? null : tab,
      collection: null,
      view: tab === 'all' ? (activeView.value === 'items' ? null : activeView.value) : null,
    })
  }

  function handleViewChange(nextView: VideoLibraryView): void {
    updateLibraryQuery({
      tab: null,
      collection: null,
      view: nextView === 'items' ? null : nextView,
      kind: nextView === 'movies' ? null : kindFilter.value === 'all' ? null : kindFilter.value,
    })
  }

  function handleSortChange(val: string): void {
    const [rawSort = '', rawOrder = ''] = val.split(':')
    const nextSort: VideoSortBy = isVideoSortBy(rawSort) ? rawSort : 'created_at'
    const nextOrder: 'asc' | 'desc' = rawOrder === 'asc' ? 'asc' : 'desc'
    updateLibraryQuery({
      sort: nextSort === 'created_at' && nextOrder === 'desc' ? null : nextSort,
      order: nextSort === 'created_at' && nextOrder === 'desc' ? null : nextOrder,
    })
  }

  function handleResetFilters(): void {
    updateLibraryQuery({
      library: null,
      genre: null,
      tag: null,
      kind: null,
      sort: null,
      order: null,
    })
  }

  function handleSearchUpdate(val: string): void {
    searchInput.value = val
    if (activeScope.value === 'online' && activeTab.value === 'all') {
      return
    }
    if (searchDebounceTimer) clearTimeout(searchDebounceTimer)
    searchDebounceTimer = setTimeout(() => {
      searchDebounceTimer = null
      updateLibraryQuery({ q: searchInput.value.trim() || null })
    }, 220)
  }

  function handleSearchSubmit(val: string): void {
    if (searchDebounceTimer) {
      clearTimeout(searchDebounceTimer)
      searchDebounceTimer = null
    }
    const trimmed = val.trim()
    updateLibraryQuery({ q: trimmed || null })
    if (activeScope.value === 'online' && activeTab.value === 'all') {
      options.submitOnlineKeyword(trimmed)
    }
  }

  function handleSearchClear(): void {
    if (searchDebounceTimer) {
      clearTimeout(searchDebounceTimer)
      searchDebounceTimer = null
    }
    searchInput.value = ''
    updateLibraryQuery({ q: null })
    if (activeScope.value === 'online' && activeTab.value === 'all') {
      options.clearOnlineKeyword()
    }
  }

  function handleOnlineKeywordSync(nextKw: string): void {
    const trimmed = nextKw.trim()
    if (trimmed !== routeQueryQ.value) {
      updateLibraryQuery({ q: trimmed || null })
    }
  }

  function handleOnlineSourceSync(nextSourceId: string | null): void {
    void sourceFilter.setSource(nextSourceId)
  }

  const primaryTabs = computed<MdTabItem<string>[]>(() => {
    if (activeCollectionId.value || activeScope.value !== 'local') return []
    return [
      { value: 'all', label: t('nav.all') },
      { value: 'continue', label: t('nav.continueWatching') },
      { value: 'favorites', label: t('nav.favorites') },
      { value: 'collections', label: t('nav.collections') },
    ]
  })

  const viewChipOptions = computed<Array<{ value: VideoLibraryView; label: string }>>(() => [
    { value: 'items', label: t('catalog.allViews') },
    { value: 'movies', label: t('catalog.moviesView') },
    { value: 'shows', label: t('catalog.showsView') },
    { value: 'genres', label: t('catalog.genresView') },
    { value: 'tags', label: t('catalog.tagsView') },
  ])

  const sortOptions = computed<SelectOption[]>(() => [
    { value: 'created_at:desc', label: t('catalog.sortCreatedDesc') },
    { value: 'title:asc', label: t('catalog.sortTitleAsc') },
    { value: 'year:desc', label: t('catalog.sortYearDesc') },
    { value: 'duration:desc', label: t('catalog.sortDurationDesc') },
  ])

  const activeLibraryName = computed(
    () => videoStore.libraries.find((l) => l.id === selectedLibrary.value)?.name ?? '',
  )

  const activeFilterCount = computed(() => {
    if (activeScope.value !== 'local' || activeTab.value !== 'all' || activeCollectionId.value) {
      return 0
    }
    let count = 0
    if (selectedLibrary.value) count += 1
    if (selectedGenre.value) count += 1
    if (selectedTag.value) count += 1
    if (activeView.value === 'items' && kindFilter.value !== 'all') count += 1
    return count
  })

  const showLocalFilterControls = computed(
    () =>
      activeScope.value === 'local' &&
      activeTab.value === 'all' &&
      !activeCollectionId.value &&
      activeView.value !== 'genres' &&
      activeView.value !== 'tags',
  )

  const showVideoSort = computed(
    () =>
      showLocalFilterControls.value &&
      (activeView.value === 'items' || activeView.value === 'movies'),
  )

  const filteredGenres = computed(() => {
    const q = routeQueryQ.value.toLowerCase()
    const list = [...videoStore.genres].sort((a, b) => b.count - a.count)
    return q ? list.filter((g) => g.name.toLowerCase().includes(q)) : list
  })

  const filteredTags = computed(() => {
    const q = routeQueryQ.value.toLowerCase()
    const list = [...videoStore.tags].sort((a, b) => b.count - a.count)
    return q ? list.filter((tg) => tg.name.toLowerCase().includes(q)) : list
  })

  const filteredShelves = computed(() => {
    const q = routeQueryQ.value.toLowerCase()
    const list = videoStore.shelves
    return q
      ? list.filter(
          (sh) =>
            sh.name.toLowerCase().includes(q) || sh.description.toLowerCase().includes(q),
        )
      : list
  })

  const filteredShelfVideos = computed(() => {
    const list = shelfDetail.value?.videos ?? []
    const q = routeQueryQ.value.toLowerCase()
    return q ? list.filter((v) => v.title.toLowerCase().includes(q)) : list
  })

  const pageTitle = computed(() => {
    if (activeCollectionId.value) {
      return shelfDetail.value?.shelf.name || t('nav.collections')
    }
    if (activeScope.value === 'pool') return globalT('pool.title')
    if (activeScope.value === 'online') return t('online.scopeOnline')
    if (activeTab.value === 'continue') return t('continueView.title')
    if (activeTab.value === 'favorites') return t('favorites.title')
    if (activeTab.value === 'collections') return t('collections.shelvesTitle')
    if (activeLibraryName.value) return activeLibraryName.value
    if (selectedGenre.value) return selectedGenre.value
    if (selectedTag.value) return `#${selectedTag.value}`
    if (activeView.value === 'shows') return t('catalog.showsView')
    if (activeView.value === 'movies') return t('catalog.moviesView')
    if (activeView.value === 'genres') return t('catalog.genresView')
    if (activeView.value === 'tags') return t('catalog.tagsView')
    return t('nav.library')
  })

  const isPoolBrowseMode = computed(() => activeScope.value === 'pool')
  const isOnlineSearchMode = computed(
    () => activeScope.value === 'online' && activeTab.value === 'all',
  )

  const pageCount = computed<number | null>(() => {
    if (isPoolBrowseMode.value || isOnlineSearchMode.value) return null
    if (isShelfTab.value) return mergedShelfItems.value.length
    if (activeCollectionId.value) return filteredShelfVideos.value.length
    if (activeTab.value === 'collections') return filteredShelves.value.length
    if (activeView.value === 'genres') return filteredGenres.value.length
    if (activeView.value === 'tags') return filteredTags.value.length
    if (activeView.value === 'shows') return shows.value.length
    return videos.value.length
  })

  const templateLoading = computed(() => {
    if (isPoolBrowseMode.value || isOnlineSearchMode.value) return false
    if (isShelfTab.value) return mergedShelf.loading.value
    return localLoading.value
  })

  const templateError = computed<string | null>(() => {
    if (isPoolBrowseMode.value || isOnlineSearchMode.value) return null
    if (isShelfTab.value) return mergedShelf.error.value
    return localError.value
  })

  const templateEmpty = computed<boolean>(() => {
    if (
      isPoolBrowseMode.value ||
      isOnlineSearchMode.value ||
      templateLoading.value ||
      templateError.value
    ) {
      return false
    }
    if (isShelfTab.value) return mergedShelfItems.value.length === 0
    if (activeCollectionId.value) return filteredShelfVideos.value.length === 0
    if (activeTab.value === 'collections') return filteredShelves.value.length === 0
    if (activeView.value === 'genres') return filteredGenres.value.length === 0
    if (activeView.value === 'tags') return filteredTags.value.length === 0
    if (activeView.value === 'shows') return shows.value.length === 0
    return videos.value.length === 0
  })

  const emptyTitle = computed(() => {
    if (activeTab.value === 'continue') return t('continueView.emptyTitle')
    if (activeTab.value === 'favorites') return t('favorites.empty')
    if (activeCollectionId.value) return t('shelf.emptyVideos')
    if (activeTab.value === 'collections') return t('shelf.emptyShelves')
    return t('common.noResults')
  })

  const emptyDescription = computed(() => {
    if (activeTab.value === 'continue') return t('continueView.emptyDesc')
    if (activeTab.value === 'collections' && !activeCollectionId.value) {
      return t('shelf.emptyShelvesDesc')
    }
    return ''
  })

  const skeletonAspect = computed<'video' | 'poster'>(() =>
    activeView.value === 'shows' ? 'poster' : 'video',
  )

  async function fetchActiveData(): Promise<void> {
    if (isPoolBrowseMode.value || isOnlineSearchMode.value) {
      localLoading.value = false
      localError.value = null
      return
    }

    if (isShelfTab.value) {
      localLoading.value = false
      localError.value = null
      await mergedShelf.refresh()
      return
    }

    const seq = ++fetchSeq
    localLoading.value = true
    localError.value = null

    try {
      if (activeCollectionId.value) {
        const detail = await api.getShelf(activeCollectionId.value)
        if (seq !== fetchSeq) return
        shelfDetail.value = detail
        return
      }

      shelfDetail.value = null

      if (
        activeTab.value === 'collections' ||
        activeView.value === 'genres' ||
        activeView.value === 'tags'
      ) {
        await videoStore.refreshSidebar(false)
        return
      }

      if (activeView.value === 'shows') {
        const list = await api.listShows({
          library_id: selectedLibrary.value || undefined,
          genre: selectedGenre.value || undefined,
          tag: selectedTag.value || undefined,
          search: routeQueryQ.value || undefined,
        })
        if (seq !== fetchSeq) return
        shows.value = list
        return
      }

      const list = await api.listVideos({
        library_id: selectedLibrary.value || undefined,
        kind: kindFilter.value,
        genre: selectedGenre.value || undefined,
        tag: selectedTag.value || undefined,
        search: routeQueryQ.value || undefined,
        sort_by: sortBy.value,
        sort_desc: sortOrder.value === 'desc',
      })
      if (seq !== fetchSeq) return
      videos.value = list
    } catch (err) {
      if (seq !== fetchSeq) return
      localError.value = err instanceof Error ? err.message : String(err)
    } finally {
      if (seq === fetchSeq) {
        localLoading.value = false
      }
    }
  }

  async function handleRefresh(): Promise<void> {
    if (manualRefreshing.value) return
    manualRefreshing.value = true
    try {
      if (isOnlineSearchMode.value) {
        await options.refreshOnlineSearch()
        return
      }
      await Promise.allSettled([videoStore.refreshSidebar(true), fetchActiveData()])
    } finally {
      manualRefreshing.value = false
    }
  }

  async function handleRenameShelf(shelfId: string, currentName: string): Promise<void> {
    const next = await dialogStore.prompt({
      title: t('shelf.renameTitle'),
      message: t('shelf.renamePrompt'),
      initialValue: currentName,
    })
    if (!next || !next.trim() || next.trim() === currentName) return
    try {
      const detail =
        shelfDetail.value && shelfDetail.value.shelf.id === shelfId
          ? shelfDetail.value
          : await api.getShelf(shelfId)
      const updated = await api.updateShelf(shelfId, {
        name: next.trim(),
        description: detail.shelf.description,
        video_ids: detail.videos.map((v) => v.id),
      })
      if (shelfDetail.value?.shelf.id === shelfId) {
        shelfDetail.value = updated
      }
      await videoStore.refreshSidebar(true)
    } catch (err) {
      snackbar.show({
        message: err instanceof Error ? err.message : String(err),
        tone: 'error',
      })
    }
  }

  async function handleDeleteShelf(shelfId: string, name: string): Promise<void> {
    const ok = await dialogStore.confirm({
      title: t('shelf.deleteTitle'),
      message: t('shelf.confirmDelete', { name }),
      danger: true,
    })
    if (!ok) return
    try {
      await api.deleteShelf(shelfId)
      await videoStore.refreshSidebar(true)
      if (activeCollectionId.value === shelfId) {
        updateLibraryQuery({ collection: null, tab: 'collections' })
      }
    } catch (err) {
      snackbar.show({
        message: err instanceof Error ? err.message : String(err),
        tone: 'error',
      })
    }
  }

  async function handleRemoveVideoFromShelf(videoId: string): Promise<void> {
    if (!shelfDetail.value) return
    try {
      const remaining = shelfDetail.value.videos.filter((v) => v.id !== videoId).map((v) => v.id)
      shelfDetail.value = await api.updateShelf(shelfDetail.value.shelf.id, {
        name: shelfDetail.value.shelf.name,
        description: shelfDetail.value.shelf.description,
        video_ids: remaining,
      })
      await videoStore.refreshSidebar(true)
    } catch (err) {
      snackbar.show({
        message: err instanceof Error ? err.message : String(err),
        tone: 'error',
      })
    }
  }

  function handlePoolChanged(): void {
    videoStore.bumpCatalogVersion()
    void videoStore.refreshSidebar(true)
  }

  async function handleMergedRemoveFavorite(item: MediaItemVM): Promise<void> {
    if (item.ref.scope !== 'local') return
    try {
      const isShow =
        typeof item.to === 'object' &&
        item.to !== null &&
        'path' in item.to &&
        typeof item.to.path === 'string' &&
        item.to.path.startsWith('/video/item/') &&
        !isMovieItemRoute(item.to)
      await api.setFavorite(isShow ? 'show' : 'video', item.ref.id, false)
      await mergedShelf.refresh()
      snackbar.show({
        message: globalT('common.removedFromFavorites'),
      })
    } catch (err) {
      snackbar.show({
        message: err instanceof Error ? err.message : String(err),
        tone: 'error',
      })
    }
  }

  async function handleMergedClearContinue(item: MediaItemVM, close: () => void): Promise<void> {
    close()
    if (item.ref.scope !== 'local') return
    try {
      await api.setWatched(item.ref.id, true)
      await mergedShelf.refresh()
    } catch (err) {
      snackbar.show({
        message: err instanceof Error ? err.message : String(err),
        tone: 'error',
      })
    }
  }

  watch(
    () => [
      activeScope.value,
      onlineSourceId.value,
      activeTab.value,
      activeCollectionId.value,
      activeView.value,
      selectedLibrary.value,
      selectedGenre.value,
      selectedTag.value,
      kindFilter.value,
      sortBy.value,
      sortOrder.value,
      routeQueryQ.value,
    ],
    () => {
      void fetchActiveData()
    },
  )

  watch(
    () => [videoStore.catalogVersion, uploadStore.versionCounter],
    () => {
      if (!isPoolBrowseMode.value && !isOnlineSearchMode.value) {
        void fetchActiveData()
      }
    },
  )

  onMounted(() => {
    void videoStore.refreshSidebar(false)
    void fetchActiveData()
  })

  onBeforeUnmount(() => {
    if (searchDebounceTimer) {
      clearTimeout(searchDebounceTimer)
      searchDebounceTimer = null
    }
  })

  return {
    filterSheetOpen,
    editingVideo,
    videos,
    shows,
    shelfDetail,
    manualRefreshing,
    activeScope,
    onlineSourceId,
    activeCollectionId,
    activeTab,
    activeView,
    selectedLibrary,
    selectedGenre,
    selectedTag,
    kindFilter,
    sortKey,
    routeQueryQ,
    searchInput,
    isShelfTab,
    mergedShelfItems,
    primaryTabs,
    viewChipOptions,
    sortOptions,
    activeLibraryName,
    activeFilterCount,
    showLocalFilterControls,
    showVideoSort,
    filteredGenres,
    filteredTags,
    filteredShelves,
    filteredShelfVideos,
    pageTitle,
    isPoolBrowseMode,
    isOnlineSearchMode,
    pageCount,
    templateLoading,
    templateError,
    templateEmpty,
    emptyTitle,
    emptyDescription,
    skeletonAspect,
    updateLibraryQuery,
    handleScopeChange,
    handleTabChange,
    handleViewChange,
    handleSortChange,
    handleResetFilters,
    handleSearchUpdate,
    handleSearchSubmit,
    handleSearchClear,
    handleOnlineKeywordSync,
    handleOnlineSourceSync,
    fetchActiveData,
    handleRefresh,
    handleRenameShelf,
    handleDeleteShelf,
    handleRemoveVideoFromShelf,
    handlePoolChanged,
    handleMergedRemoveFavorite,
    handleMergedClearContinue,
  }
}

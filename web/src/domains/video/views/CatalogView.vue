<script setup lang="ts">
import { computed, onBeforeUnmount, onMounted, ref, watch } from 'vue'
import { useRoute, useRouter, type LocationQueryRaw } from 'vue-router'
import { useI18n as useGlobalI18n } from 'vue-i18n'
import {
  LuArrowLeft,
  LuCheck,
  LuFilm,
  LuFolderHeart,
  LuMoreVertical,
  LuPencil,
  LuPlay,
  LuPlus,
  LuRefreshCw,
  LuSparkles,
  LuTag,
  LuTrash2,
  LuX,
} from 'vue-icons-plus/lu'
import { DomainLibraryTemplate, DomainPoolSection, MediaItemCard } from '@/components/domain'
import {
  MdButton,
  MdCard,
  MdChip,
  MdIconButton,
  MdMenu,
  MdMenuItem,
  MdSelect,
  MdTextField,
  MediaCoverImage,
  type MdTabItem,
  type SelectOption,
} from '@/components/ui'
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
import { api, buildPosterUrl } from '../api'
import OnlineVideoSearchSection from '../components/OnlineVideoSearchSection.vue'
import ShowCard from '../components/ShowCard.vue'
import VideoCard from '../components/VideoCard.vue'
import VideoMetadataModal from '../components/VideoMetadataModal.vue'
import { useVideoI18n as useI18n } from '../i18n'
import { useVideoStore } from '../store'
import type {
  ShowSummaryDto,
  VideoKindFilter,
  VideoShelfDetailDto,
  VideoSortBy,
  VideoSummaryDto,
} from '../types'

type VideoLibraryView = 'items' | 'shows' | 'movies' | 'genres' | 'tags'

const VALID_TABS = new Set<LibraryTab>(['all', 'continue', 'favorites', 'collections'])
const VALID_VIEWS = new Set<VideoLibraryView>(['items', 'shows', 'movies', 'genres', 'tags'])

const { t } = useI18n()
const { t: globalT } = useGlobalI18n()
const route = useRoute()
const router = useRouter()
const videoStore = useVideoStore()
const uploadStore = useUploadStore()
const dialogStore = useDialogStore()
const snackbar = useSnackbarStore()

const sourceFilter = useSourceFilter('video')

const onlineSearchRef = ref<InstanceType<typeof OnlineVideoSearchSection> | null>(null)
const filterSheetOpen = ref(false)
const editingVideo = ref<VideoSummaryDto | null>(null)
const newShelfName = ref('')
const creatingShelf = ref(false)

// Local data states
const videos = ref<VideoSummaryDto[]>([])
const shows = ref<ShowSummaryDto[]>([])
const shelfDetail = ref<VideoShelfDetailDto | null>(null)
const localLoading = ref(false)
const localError = ref<string | null>(null)

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
  const raw = readQueryStr('tab') as LibraryTab
  if (VALID_TABS.has(raw)) {
    return raw
  }
  return 'all'
})

const activeView = computed<VideoLibraryView>(() => {
  const raw = readQueryStr('view') as VideoLibraryView
  if (VALID_VIEWS.has(raw)) return raw
  return 'items'
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
  if (raw === 'title' || raw === 'year' || raw === 'duration' || raw === 'created_at') return raw
  return 'created_at'
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

// Local Continue & Favorites via useMergedShelf
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

const mergedShelfItems = computed<MediaItemVM[]>(() =>
  mergedShelf.items.value.map(normalizeMergedItem),
)

// Query mutation helper (always canonical `/video/library`)
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
  // Drop redundant defaults
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
  const tab = (VALID_TABS.has(nextTab as LibraryTab) ? nextTab : 'all') as LibraryTab
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
  const [nextSort, nextOrder] = val.split(':') as [VideoSortBy, 'asc' | 'desc']
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
    onlineSearchRef.value?.submitKeyword(trimmed)
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
    onlineSearchRef.value?.clearKeyword()
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

// Tabs & options
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

const libraryOptions = computed<SelectOption[]>(() => [
  { value: '', label: t('catalog.allLibraries') },
  ...videoStore.libraries.map((l) => ({ value: l.id, label: l.name, badge: l.video_count })),
])

const genreOptions = computed<SelectOption[]>(() => [
  { value: '', label: t('catalog.allGenres') },
  ...videoStore.genres.map((g) => ({ value: g.name, label: g.name, badge: g.count })),
])

const tagOptions = computed<SelectOption[]>(() => [
  { value: '', label: t('catalog.allTags') },
  ...videoStore.tags.map((tg) => ({ value: tg.name, label: `#${tg.name}`, badge: tg.count })),
])

const kindOptions = computed<SelectOption[]>(() => [
  { value: 'all', label: t('catalog.kindAll') },
  { value: 'standalone', label: t('catalog.kindStandalone') },
  { value: 'episodes', label: t('catalog.kindEpisodes') },
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
  () => showLocalFilterControls.value && (activeView.value === 'items' || activeView.value === 'movies'),
)

// Filtered dimensions & collections
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

const firstPlayableShelfVideo = computed<VideoSummaryDto | null>(() => {
  const list = shelfDetail.value?.videos ?? []
  if (list.length === 0) return null
  return list.find((v) => !v.progress?.completed) ?? list[0] ?? null
})

// Page title & count
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

const templateEmpty = computed(() => {
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
  if (activeTab.value === 'collections' && !activeCollectionId.value) return t('shelf.emptyShelvesDesc')
  return ''
})

const skeletonAspect = computed<'video' | 'poster'>(() =>
  activeView.value === 'shows' ? 'poster' : 'video',
)

// Data fetching
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

    if (activeTab.value === 'collections' || activeView.value === 'genres' || activeView.value === 'tags') {
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

const manualRefreshing = ref(false)

async function handleRefresh(): Promise<void> {
  if (manualRefreshing.value) return
  manualRefreshing.value = true
  try {
    if (isOnlineSearchMode.value) {
      await onlineSearchRef.value?.refresh()
      return
    }
    await Promise.allSettled([videoStore.refreshSidebar(true), fetchActiveData()])
  } finally {
    manualRefreshing.value = false
  }
}

// Collection (Shelf) actions
async function handleCreateShelf(): Promise<void> {
  const trimmed = newShelfName.value.trim()
  if (!trimmed || creatingShelf.value) return
  creatingShelf.value = true
  try {
    await api.createShelf({ name: trimmed })
    newShelfName.value = ''
    await videoStore.refreshSidebar(true)
  } catch (err) {
    snackbar.show({
      message: err instanceof Error ? err.message : String(err),
      tone: 'error',
    })
  } finally {
    creatingShelf.value = false
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

// Local continue/favorites item quick actions
async function handleMergedRemoveFavorite(item: MediaItemVM): Promise<void> {
  if (item.ref.scope !== 'local') return
  try {
    const isShow =
      typeof item.to === 'object' &&
      item.to &&
      'path' in item.to &&
      typeof item.to.path === 'string' &&
      item.to.path.startsWith('/video/item/') &&
      !('query' in item.to && (item.to.query as Record<string, unknown>)?.kind === 'movie')
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
</script>

<template>
  <div class="px-4 py-6 sm:px-6 lg:px-8">
    <DomainLibraryTemplate
      domain="video"
      :title="pageTitle"
      :count="pageCount"
      :scope="activeScope"
      :sync-scope-url="false"
      :show-scope-bar="true"
      :search-query="searchInput"
      :search-placeholder="
        activeScope === 'online' ? t('online.searchPlaceholder') : t('common.filterPlaceholder')
      "
      :tabs="primaryTabs"
      :active-tab="activeTab"
      :loading="templateLoading"
      :skeleton-aspect="skeletonAspect"
      :error="templateError"
      :empty="templateEmpty"
      :empty-title="emptyTitle"
      :empty-description="emptyDescription"
      :filter-sheet-open="filterSheetOpen"
      :active-filter-count="activeFilterCount"
      @scope-change="handleScopeChange"
      @update:active-tab="handleTabChange"
      @update:search-query="handleSearchUpdate"
      @search-submit="handleSearchSubmit"
      @search-clear="handleSearchClear"
      @retry="fetchActiveData"
      @update:filter-sheet-open="filterSheetOpen = $event"
      @reset-filters="handleResetFilters"
    >
      <template #header-trailing>
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

      <!-- Single compact row of View chips + removable active filter chips -->
      <template
        v-if="activeScope === 'local' && activeTab === 'all' && !activeCollectionId"
        #toolbar-chips
      >
        <MdChip
          v-for="opt in viewChipOptions"
          :key="opt.value"
          variant="filter"
          :label="opt.label"
          :selected="activeView === opt.value"
          hide-check
          @click="handleViewChange(opt.value)"
        />

        <MdChip
          v-if="activeLibraryName"
          variant="input"
          :label="activeLibraryName"
          removable
          @remove="updateLibraryQuery({ library: null })"
        />
        <MdChip
          v-if="selectedGenre"
          variant="input"
          :label="selectedGenre"
          removable
          @remove="updateLibraryQuery({ genre: null })"
        />
        <MdChip
          v-if="selectedTag"
          variant="input"
          :label="`#${selectedTag}`"
          removable
          @remove="updateLibraryQuery({ tag: null })"
        />
      </template>

      <template v-if="showVideoSort" #toolbar-trailing>
        <div class="w-40">
          <MdSelect
            :model-value="sortKey"
            :options="sortOptions"
            variant="compact"
            :aria-label="t('catalog.sortLabel')"
            @change="handleSortChange"
          />
        </div>
      </template>

      <!-- Responsive Filter Sheet (only registered when local browsing) -->
      <template v-if="showLocalFilterControls" #filter-sheet>
        <div class="space-y-4">
          <div class="space-y-1.5">
            <span class="text-label-medium block text-on-surface-variant">
              {{ t('catalog.libraryLabel') }}
            </span>
            <MdSelect
              :model-value="selectedLibrary"
              :options="libraryOptions"
              size="sm"
              @change="updateLibraryQuery({ library: $event || null })"
            />
          </div>

          <div class="space-y-1.5">
            <span class="text-label-medium block text-on-surface-variant">
              {{ t('catalog.genreLabel') }}
            </span>
            <MdSelect
              :model-value="selectedGenre"
              :options="genreOptions"
              size="sm"
              searchable
              @change="updateLibraryQuery({ genre: $event || null })"
            />
          </div>

          <div v-if="videoStore.tags.length > 0 || selectedTag" class="space-y-1.5">
            <span class="text-label-medium block text-on-surface-variant">
              {{ t('catalog.tagLabel') }}
            </span>
            <MdSelect
              :model-value="selectedTag"
              :options="tagOptions"
              size="sm"
              searchable
              @change="updateLibraryQuery({ tag: $event || null })"
            />
          </div>

          <div v-if="activeView === 'items'" class="space-y-1.5">
            <span class="text-label-medium block text-on-surface-variant">
              {{ t('catalog.kindLabel') }}
            </span>
            <MdSelect
              :model-value="kindFilter"
              :options="kindOptions"
              size="sm"
              @change="updateLibraryQuery({ kind: $event === 'all' ? null : $event })"
            />
          </div>

          <div v-if="showVideoSort" class="space-y-1.5">
            <span class="text-label-medium block text-on-surface-variant">
              {{ t('catalog.sortLabel') }}
            </span>
            <MdSelect
              :model-value="sortKey"
              :options="sortOptions"
              size="sm"
              @change="handleSortChange"
            />
          </div>
        </div>
      </template>

      <!-- Collection Detail Subheader OR New Watchlist Subheader -->
      <template #subheader>
        <!-- Viewing a specific Watchlist (?collection=<id>) -->
        <section
          v-if="activeCollectionId && shelfDetail"
          class="space-y-4 rounded-corner-lg bg-surface-container-low p-4 sm:p-5"
        >
          <div class="flex items-center">
            <MdButton
              variant="text"
              size="sm"
              :icon="LuArrowLeft"
              @click="updateLibraryQuery({ collection: null, tab: 'collections' })"
            >
              {{ t('shelf.backToCollections') }}
            </MdButton>
          </div>

          <div class="flex flex-col gap-4 sm:flex-row sm:items-center">
            <div class="w-40 shrink-0 overflow-hidden rounded-corner-md bg-surface-container-high">
              <MediaCoverImage
                :src="buildPosterUrl(shelfDetail.shelf.poster_key, 400)"
                :alt="shelfDetail.shelf.name"
                aspect-class="aspect-video"
                rounded="rounded-corner-md"
                :fallback-icon="LuFolderHeart"
                fallback-icon-class="h-8 w-8 text-on-surface-variant/60"
              />
            </div>

            <div class="min-w-0 flex-1 space-y-2">
              <div class="space-y-0.5">
                <h2 class="text-title-large truncate text-on-surface">
                  {{ shelfDetail.shelf.name }}
                </h2>
                <p class="text-body-small text-on-surface-variant">
                  {{ t('common.videosCount', { count: shelfDetail.shelf.video_count }) }}
                </p>
                <p
                  v-if="shelfDetail.shelf.description"
                  class="text-body-medium line-clamp-2 text-on-surface-variant"
                >
                  {{ shelfDetail.shelf.description }}
                </p>
              </div>

              <div class="flex flex-wrap items-center gap-2 pt-1">
                <MdButton
                  v-if="firstPlayableShelfVideo"
                  variant="filled"
                  size="sm"
                  :icon="LuPlay"
                  :to="{
                    path: `/video/play/${encodeURIComponent(firstPlayableShelfVideo.id)}`,
                    query: { shelf: shelfDetail.shelf.id },
                  }"
                >
                  {{ t('common.play') }}
                </MdButton>

                <MdButton
                  variant="tonal"
                  size="sm"
                  :icon="LuPencil"
                  @click="handleRenameShelf(shelfDetail.shelf.id, shelfDetail.shelf.name)"
                >
                  {{ t('common.edit') }}
                </MdButton>

                <MdMenu placement="bottom-end" :aria-label="t('common.moreActions')">
                  <template #trigger="{ toggle, triggerAttrs }">
                    <MdIconButton
                      v-bind="triggerAttrs"
                      variant="standard"
                      size="sm"
                      :icon="LuMoreVertical"
                      :aria-label="t('common.moreActions')"
                      @click="toggle"
                    />
                  </template>
                  <template #default="{ close }">
                    <MdMenuItem
                      :icon="LuTrash2"
                      :label="t('common.delete')"
                      destructive
                      @select="
                        close();
                        handleDeleteShelf(shelfDetail.shelf.id, shelfDetail.shelf.name)
                      "
                    />
                  </template>
                </MdMenu>
              </div>
            </div>
          </div>
        </section>

        <!-- Collections Overview Inline Create Bar -->
        <form
          v-else-if="activeScope === 'local' && activeTab === 'collections' && !activeCollectionId"
          class="flex flex-wrap items-center gap-2"
          @submit.prevent="handleCreateShelf"
        >
          <div class="min-w-52 flex-1 sm:max-w-xs">
            <MdTextField
              v-model="newShelfName"
              size="sm"
              :placeholder="t('nav.shelfNamePlaceholder')"
            />
          </div>
          <MdButton
            type="submit"
            variant="tonal"
            size="sm"
            :icon="LuPlus"
            :loading="creatingShelf"
            :disabled="!newShelfName.trim()"
          >
            {{ t('nav.newShelf') }}
          </MdButton>
        </form>
      </template>

      <!-- Mode 0: Resource Pool Browse -->
      <DomainPoolSection
        v-if="isPoolBrowseMode"
        domain="video"
        :search-query="searchInput"
        @changed="handlePoolChanged"
      />

      <!-- Mode 1: Online Search & Explore Shell (single search bar lives in DomainLibraryTemplate) -->
      <OnlineVideoSearchSection
        v-else-if="isOnlineSearchMode"
        ref="onlineSearchRef"
        :keyword="routeQueryQ"
        :initial-source-id="onlineSourceId"
        :show-search-input="false"
        @update:keyword="handleOnlineKeywordSync"
        @update:source-id="handleOnlineSourceSync"
      />

      <!-- Mode 2: Merged Continue / Favorites (Cross-Source Local + Online) -->
      <div
        v-else-if="isShelfTab"
        class="grid grid-cols-1 gap-5 sm:grid-cols-2 lg:grid-cols-3 xl:grid-cols-4"
      >
        <MediaItemCard
          v-for="(item, idx) in mergedShelfItems"
          :key="
            item.ref.scope === 'local'
              ? `shelf-local-${item.ref.id}-${idx}`
              : `shelf-online-${item.ref.sourceId}-${item.ref.ref}-${idx}`
          "
          :item="item"
          aspect="video"
          cover-style="plain"
          :prefer-play-route="activeTab === 'continue'"
          :show-favorite="activeTab === 'favorites'"
          :favorite="activeTab === 'favorites' ? true : undefined"
          @toggle-favorite="handleMergedRemoveFavorite"
        >
          <template v-if="activeTab === 'continue'" #menu="{ close }">
            <MdMenuItem
              :icon="LuCheck"
              :label="t('detail.clearProgress')"
              @select="handleMergedClearContinue(item, close)"
            />
          </template>
        </MediaItemCard>
      </div>

      <!-- Mode 3: Watchlist Detail Grid (?collection=<id>) -->
      <div
        v-else-if="activeCollectionId"
        class="grid grid-cols-1 gap-5 sm:grid-cols-2 lg:grid-cols-3 xl:grid-cols-4"
      >
        <VideoCard
          v-for="v in filteredShelfVideos"
          :key="v.id"
          :video="v"
          removable-from-shelf
          @remove-from-shelf="handleRemoveVideoFromShelf"
          @edit="editingVideo = $event"
          @deleted="fetchActiveData"
        />
      </div>

      <!-- Mode 4: Watchlists Overview Grid (tab=collections) -->
      <div
        v-else-if="activeTab === 'collections'"
        class="grid grid-cols-1 gap-4 sm:grid-cols-2 lg:grid-cols-3"
      >
        <MdCard
          v-for="sh in filteredShelves"
          :key="sh.id"
          variant="filled"
          padding="sm"
          clickable
          class="flex items-center gap-3.5"
          @click="updateLibraryQuery({ collection: sh.id, tab: null })"
        >
          <div class="h-12 w-20 shrink-0 overflow-hidden rounded-corner-sm bg-surface-container-high">
            <MediaCoverImage
              :src="buildPosterUrl(sh.poster_key, 240)"
              :alt="sh.name"
              aspect-class="aspect-video"
              rounded="rounded-corner-sm"
              :fallback-icon="LuFolderHeart"
              fallback-icon-class="h-5 w-5 text-on-surface-variant/60"
            />
          </div>

          <div class="min-w-0 flex-1">
            <h3 class="text-title-small truncate text-on-surface">
              {{ sh.name }}
            </h3>
            <p class="text-body-small mt-0.5 truncate text-on-surface-variant">
              {{ t('common.videosCount', { count: sh.video_count }) }}
            </p>
          </div>

          <div class="shrink-0" @click.stop>
            <MdMenu placement="bottom-end" :aria-label="t('common.moreActions')">
              <template #trigger="{ toggle, triggerAttrs }">
                <MdIconButton
                  v-bind="triggerAttrs"
                  variant="standard"
                  size="sm"
                  :icon="LuMoreVertical"
                  :aria-label="t('common.moreActions')"
                  @click="toggle"
                />
              </template>
              <template #default="{ close }">
                <MdMenuItem
                  :icon="LuPencil"
                  :label="t('common.edit')"
                  @select="
                    close();
                    handleRenameShelf(sh.id, sh.name)
                  "
                />
                <div class="my-1 border-t border-outline-variant pt-1" role="separator">
                  <MdMenuItem
                    :icon="LuTrash2"
                    :label="t('common.delete')"
                    destructive
                    @select="
                      close();
                      handleDeleteShelf(sh.id, sh.name)
                    "
                  />
                </div>
              </template>
            </MdMenu>
          </div>
        </MdCard>
      </div>

      <!-- Mode 5: Genres / Tags Dimension Browser (view=genres | view=tags) -->
      <div
        v-else-if="activeView === 'genres' || activeView === 'tags'"
        class="flex flex-wrap gap-2"
      >
        <MdChip
          v-for="item in activeView === 'genres' ? filteredGenres : filteredTags"
          :key="item.name"
          variant="assist"
          :icon="activeView === 'genres' ? LuSparkles : LuTag"
          :label="`${activeView === 'tags' ? '#' : ''}${item.name} (${item.count})`"
          @click="
            updateLibraryQuery(
              activeView === 'genres'
                ? { genre: item.name, view: null }
                : { tag: item.name, view: null },
            )
          "
        />
      </div>

      <!-- Mode 6: TV & Anime Shows Grid (view=shows) -->
      <div
        v-else-if="activeView === 'shows'"
        class="grid grid-cols-2 gap-5 sm:grid-cols-3 md:grid-cols-4 lg:grid-cols-5 xl:grid-cols-6"
      >
        <ShowCard v-for="sh in shows" :key="sh.id" :show="sh" @unbound="fetchActiveData" />
      </div>

      <!-- Mode 7: Videos / Movies Grid (view=items | view=movies) -->
      <div
        v-else
        class="grid grid-cols-1 gap-5 sm:grid-cols-2 lg:grid-cols-3 xl:grid-cols-4"
      >
        <VideoCard
          v-for="v in videos"
          :key="v.id"
          :video="v"
          @edit="editingVideo = $event"
          @deleted="fetchActiveData"
          @updated="fetchActiveData"
        />
      </div>

      <template v-if="activeFilterCount > 0 || routeQueryQ" #empty-actions>
        <MdButton
          variant="tonal"
          :icon="LuX"
          @click="
            handleSearchClear();
            handleResetFilters()
          "
        >
          {{ globalT('common.clearFilters') }}
        </MdButton>
      </template>

      <template v-else-if="activeTab === 'continue'" #empty-actions>
        <MdButton variant="tonal" :icon="LuFilm" @click="handleTabChange('all')">
          {{ t('continueView.browseCatalog') }}
        </MdButton>
      </template>
    </DomainLibraryTemplate>

    <VideoMetadataModal
      :open="editingVideo !== null"
      :video="editingVideo"
      @close="editingVideo = null"
      @saved="fetchActiveData"
    />
  </div>
</template>

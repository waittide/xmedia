<script setup lang="ts">
import { computed, onMounted, ref, watch } from 'vue'
import { useI18n as useGlobalI18n } from 'vue-i18n'
import { useRoute, useRouter, type LocationQueryRaw } from 'vue-router'
import {
  LuBookOpen,
  LuPlus,
  LuRefreshCw,
  LuSettings2,
  LuTag,
  LuX,
} from 'vue-icons-plus/lu'
import { DomainLibraryTemplate, DomainPoolSection, MediaItemCard } from '@/components/domain'
import RemoteOpdsBrowser from '@/components/remote/RemoteOpdsBrowser.vue'
import {
  MdButton,
  MdChip,
  MdEmptyState,
  MdIconButton,
  MdMenuItem,
  type MdTabItem,
} from '@/components/ui'
import { api } from '@/domains/comic/api'
import ComicCatalogFilterSheet from '@/domains/comic/components/catalog/ComicCatalogFilterSheet.vue'
import ComicCollectionsSection from '@/domains/comic/components/catalog/ComicCollectionsSection.vue'
import ComicMergedShelfSection from '@/domains/comic/components/catalog/ComicMergedShelfSection.vue'
import ComicShelfFormDialog from '@/domains/comic/components/catalog/ComicShelfFormDialog.vue'
import OnlineComicSearchSection from '@/domains/comic/components/OnlineComicSearchSection.vue'
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
import { useAuthStore } from '@/stores/auth'
import { useDialogStore } from '@/stores/dialog'
import { useSnackbarStore } from '@/stores/snackbar'
import { useUploadStore } from '@/stores/uploads'

type ComicBrowseView = 'items' | 'genres' | 'writers' | 'pencillers' | 'tags'
type ComicSortKey = 'updated_at' | 'created_at' | 'title' | 'chapter_count'
type ComicSortOrder = 'desc' | 'asc'
type ComicReadFilter = '' | 'unread' | 'in_progress' | 'completed'

const VALID_TABS: readonly LibraryTab[] = ['all', 'continue', 'favorites', 'collections']
const VALID_VIEWS: readonly ComicBrowseView[] = ['items', 'genres', 'writers', 'pencillers', 'tags']
const VALID_SORTS: readonly ComicSortKey[] = ['updated_at', 'created_at', 'title', 'chapter_count']

const props = defineProps<{
  presetMode?: 'all' | 'continue' | 'favorites'
}>()

const { t } = useI18n()
const { t: globalT } = useGlobalI18n()
const route = useRoute()
const router = useRouter()
const authStore = useAuthStore()
const appClientStore = useAppClientStore()
const upload = useUploadStore()
const sidebarStore = useSidebarStore()
const snackbarStore = useSnackbarStore()
const dialogStore = useDialogStore()

const sourceFilter = useSourceFilter('comic')
const onlineSearchRef = ref<InstanceType<typeof OnlineComicSearchSection> | null>(null)
const filterSheetOpen = ref(false)

function firstQuery(keys: readonly string[]): string {
  for (const key of keys) {
    const raw = route.query[key]
    if (typeof raw === 'string' && raw.trim()) return raw.trim()
    if (Array.isArray(raw)) {
      const hit = raw.find((v): v is string => typeof v === 'string' && v.trim().length > 0)
      if (hit) return hit.trim()
    }
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
  const raw = firstQuery(['tab'])
  if (raw && (VALID_TABS as readonly string[]).includes(raw)) {
    return raw as LibraryTab
  }
  if (props.presetMode === 'continue') return 'continue'
  if (props.presetMode === 'favorites') return 'favorites'
  return 'all'
})

const activeView = computed<ComicBrowseView>(() => {
  if (scope.value !== 'local' || activeTab.value !== 'all') return 'items'
  if (collectionId.value) return 'items'
  const raw = firstQuery(['view'])
  if (raw && (VALID_VIEWS as readonly string[]).includes(raw)) {
    return raw as ComicBrowseView
  }
  return 'items'
})

const queryFromRoute = computed(() => firstQuery(['q']))
const searchInput = ref(queryFromRoute.value)

const libraryId = computed(() => firstQuery(['library', 'library_id']))
const collectionId = computed(() => firstQuery(['collection', 'shelf_id', 'shelf']))
const writerFilter = computed(() => firstQuery(['writer']))
const pencillerFilter = computed(() => firstQuery(['penciller']))
const genreFilter = computed(() => firstQuery(['genre']))
const tagFilter = computed(() => firstQuery(['tag']))

const statusFilter = computed<SeriesStatus | ''>(() => {
  const raw = firstQuery(['status'])
  if (raw === 'ongoing' || raw === 'completed' || raw === 'hiatus') return raw
  return ''
})

const readStatusFilter = computed<ComicReadFilter>(() => {
  const raw = firstQuery(['read_status'])
  if (raw === 'unread' || raw === 'in_progress' || raw === 'completed') return raw
  return ''
})

const sortBy = computed<ComicSortKey>(() => {
  const raw = firstQuery(['sort', 'sort_by'])
  if (raw && (VALID_SORTS as readonly string[]).includes(raw)) {
    return raw as ComicSortKey
  }
  return 'updated_at'
})

const sortOrder = computed<ComicSortOrder>(() => {
  const raw = firstQuery(['order', 'sort_order'])
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
  () => scope.value === 'local' && (activeTab.value === 'continue' || activeTab.value === 'favorites'),
)
const isPoolBrowseView = computed(() => scope.value === 'pool')
const isOnlineBrowseView = computed(() => scope.value === 'online')
const isRemoteBrowseView = computed(() => scope.value === 'remote')

const hasLocalFilterOverrides = computed(
  () =>
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
  if (firstQuery(['scope']) === 'local' || hasLocalFilterOverrides.value) return 'local'
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
    if (
      k === 'library_id' ||
      k === 'shelf_id' ||
      k === 'shelf' ||
      k === 'sort_by' ||
      k === 'sort_order'
    ) {
      continue
    }
    if (v !== undefined && v !== null) {
      next[k] = v
    }
  }
  if (libraryId.value && !('library' in patch)) next.library = libraryId.value
  if (collectionId.value && !('collection' in patch)) next.collection = collectionId.value

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
    void onlineSearchRef.value?.submitSearch(trimmed)
  }
}

function handleSearchClear(): void {
  searchInput.value = ''
  updateQuery({ q: null, page: null })
  if (isOnlineBrowseView.value) {
    onlineSearchRef.value?.clearSearch()
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
      await onlineSearchRef.value?.refresh()
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
</script>

<template>
  <div class="px-4 py-6 sm:px-6 md:px-8">
    <DomainLibraryTemplate
      domain="comic"
      :title="headerTitle"
      :count="headerCount"
      :scope="scope"
      :sync-scope-url="false"
      :show-scope-bar="true"
      :show-remote-scope="showRemoteScope"
      :search-query="searchInput"
      :search-placeholder="
        isOnlineBrowseView
          ? t('online.searchPlaceholder')
          : isPoolBrowseView
            ? globalT('pool.searchPlaceholder', { domain: globalT('app.domains.comic') })
            : globalT('common.searchComic')
      "
      :show-search="!isRemoteBrowseView"
      :tabs="tabs"
      :active-tab="activeTab"
      :loading="templateLoading"
      skeleton-aspect="poster"
      :error="templateError"
      :empty="templateEmpty"
      :empty-title="templateEmptyTitle"
      :filter-sheet-open="filterSheetOpen"
      :filter-sheet-title="t('library.filterSheetTitle')"
      :active-filter-count="activeFilterCount"
      @scope-change="handleScopeChange"
      @update:active-tab="handleTabChange"
      @update:search-query="searchInput = $event"
      @search-submit="handleSearchSubmit"
      @search-clear="handleSearchClear"
      @update:filter-sheet-open="filterSheetOpen = $event"
      @reset-filters="resetLocalFilters"
      @retry="refreshActiveView"
    >
      <template #header-trailing>
        <MdIconButton
          v-if="!isRemoteBrowseView"
          variant="standard"
          size="sm"
          :icon="LuRefreshCw"
          :loading="manualRefreshing"
          :aria-label="globalT('common.refresh')"
          :tooltip="globalT('common.refresh')"
          @click="refreshActiveView"
        />
      </template>

      <template v-if="scope === 'local' && activeTab === 'all'" #toolbar-chips>
        <template v-if="!collectionId">
          <MdChip
            v-for="opt in viewOptions"
            :key="opt.value"
            variant="filter"
            :label="opt.label"
            :selected="activeView === opt.value"
            hide-check
            @click="handleViewChange(opt.value)"
          />
        </template>

        <MdChip
          v-for="chip in activeFilterChips"
          :key="chip.key"
          variant="input"
          :label="chip.label"
          selected
          removable
          hide-check
          @remove="removeFilterChip(chip.key)"
        />

        <MdButton
          v-if="activeFilterChips.length > 0"
          variant="text"
          size="sm"
          @click="resetLocalFilters"
        >
          {{ t('series.clearFilters') }}
        </MdButton>
      </template>

      <template v-if="isCollectionsView" #toolbar-trailing>
        <MdButton variant="tonal" size="sm" :icon="LuPlus" @click="openCreateShelf">
          {{ t('shelves.newShelf') }}
        </MdButton>
      </template>

      <template v-if="activeCollection && scope === 'local'" #subheader>
        <ComicCollectionsSection
          mode="subheader"
          :active-collection="activeCollection"
          @back="updateQuery({ collection: null, tab: 'collections' })"
          @edit="openEditShelf"
          @delete="handleDeleteShelf"
        />
      </template>

      <template v-if="isLocalItemsView" #filter-sheet>
        <ComicCatalogFilterSheet
          :library-id="libraryId"
          :status-filter="statusFilter"
          :read-status-filter="readStatusFilter"
          :sort-by="sortBy"
          :sort-order="sortOrder"
          @patch-query="updateQuery"
        />
      </template>

      <template v-if="isCollectionsView" #empty-actions>
        <MdButton variant="tonal" :icon="LuPlus" @click="openCreateShelf">
          {{ t('shelves.newShelf') }}
        </MdButton>
      </template>
      <template v-else-if="isLocalItemsView && activeFilterChips.length > 0" #empty-actions>
        <MdButton variant="tonal" :icon="LuX" @click="resetLocalFilters">
          {{ t('series.clearFilters') }}
        </MdButton>
      </template>

      <DomainPoolSection
        v-if="isPoolBrowseView"
        domain="comic"
        :search-query="searchInput"
        @changed="handlePoolChanged"
      />

      <OnlineComicSearchSection
        v-else-if="isOnlineBrowseView"
        ref="onlineSearchRef"
        :keyword="queryFromRoute"
        :initial-source-id="activeSourceId"
        :show-search-input="false"
        @update:keyword="handleOnlineKeywordSync"
        @update:source-id="handleOnlineSourceSync"
      />

      <template v-else-if="isRemoteBrowseView">
        <RemoteOpdsBrowser
          v-if="appClientStore.isStandaloneApp"
          :selected-source-id="selectedRemoteSourceId"
          :active="true"
          @update:selected-source-id="onRemoteSourceSelect"
          @add-source="goRemoteSettings"
        />
        <MdEmptyState
          v-else
          :icon="LuSettings2"
          :title="t('library.remoteEmptyTitle')"
          :description="t('library.remoteEmptyDesc')"
        >
          <template v-if="authStore.isAdmin" #action>
            <MdButton variant="tonal" :icon="LuSettings2" @click="goRemoteSettings">
              {{ t('library.remoteManageBtn') }}
            </MdButton>
          </template>
        </MdEmptyState>
      </template>

      <ComicMergedShelfSection
        v-else-if="isMergedTab"
        :items="mergedShelf.items.value"
        :bookmarks="filteredBookmarks"
        :prefer-play-route="activeTab === 'continue'"
      />

      <ComicCollectionsSection
        v-else-if="isCollectionsView"
        mode="grid"
        :shelves="filteredShelves"
        @select="openCollection"
        @edit="openEditShelf"
        @delete="handleDeleteShelf"
      />

      <div
        v-else-if="isTaxonomyView"
        class="grid grid-cols-2 gap-3 sm:grid-cols-3 md:grid-cols-4 lg:grid-cols-5"
      >
        <button
          v-for="item in taxonomyItems"
          :key="item.name"
          type="button"
          class="md-state flex cursor-pointer items-center justify-between gap-2 rounded-corner-md bg-surface-container-low px-4 py-3 text-left transition-colors duration-150 ease-standard hover:bg-surface-container-high"
          @click="selectTaxonomyValue(item.name)"
        >
          <span class="text-title-small flex min-w-0 items-center gap-2 text-on-surface">
            <LuTag class="h-4 w-4 shrink-0 text-primary" aria-hidden="true" />
            <span class="truncate">{{ activeView === 'tags' ? `#${item.name}` : item.name }}</span>
          </span>
          <span
            class="text-label-small shrink-0 rounded-corner-full bg-surface-container-highest px-2 py-0.5 tabular-nums text-on-surface-variant"
          >
            {{ item.count }}
          </span>
        </button>
      </div>

      <div
        v-else
        class="grid grid-cols-2 gap-5 sm:grid-cols-3 md:grid-cols-4 lg:grid-cols-5 xl:grid-cols-6"
      >
        <MediaItemCard
          v-for="(vm, idx) in localSeriesVMs"
          :key="vm.ref.scope === 'local' ? vm.ref.id : idx"
          :item="vm"
          aspect="poster"
          cover-style="book"
          :fallback-icon="LuBookOpen"
          :show-favorite="seriesList[idx]?.is_favorite"
          :favorite="seriesList[idx]?.is_favorite ? true : undefined"
          @toggle-favorite="seriesList[idx] && toggleLocalSeriesFavorite(seriesList[idx]!)"
        >
          <template v-if="seriesList[idx]" #menu="{ close }">
            <MdMenuItem
              :icon="LuX"
              :label="globalT('pool.unbindBtn')"
              destructive
              @select="
                close();
                handleUnbindLocalSeries(seriesList[idx]!)
              "
            />
          </template>
        </MediaItemCard>
      </div>
    </DomainLibraryTemplate>

    <ComicShelfFormDialog
      :open="shelfDialogOpen"
      :editing-shelf="editingShelf"
      @close="shelfDialogOpen = false"
    />
  </div>
</template>

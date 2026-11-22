<script setup lang="ts">
import { computed, onBeforeUnmount, onMounted, ref, watch } from 'vue'
import { useRoute, useRouter, type LocationQueryRaw } from 'vue-router'
import { useI18n as useGlobalI18n } from 'vue-i18n'
import {
  LuClock,
  LuDisc3,
  LuHeart,
  LuListMusic,
  LuMic,
  LuMusic,
  LuPlay,
  LuRadio,
  LuRefreshCw,
  LuShuffle,
  LuTag,
} from 'vue-icons-plus/lu'
import { DomainLibraryTemplate } from '@/components/domain'
import {
  MdButton,
  MdChip,
  MdIconButton,
  MdSelect,
  type MdTabItem,
  type SelectOption,
} from '@/components/ui'
import { useSourceFilter, type LibraryTab, type SourceScope } from '@/sources'
import { useUploadStore } from '@/stores/uploads'
import { api } from '../api'
import AlbumCard from '../components/AlbumCard.vue'
import ArtistCard from '../components/ArtistCard.vue'
import LibraryArtistPanel from '../components/LibraryArtistPanel.vue'
import LibraryCollectionsPanel from '../components/LibraryCollectionsPanel.vue'
import LibraryContinuePanel from '../components/LibraryContinuePanel.vue'
import LibraryFavoritesPanel from '../components/LibraryFavoritesPanel.vue'
import LibraryPlaylistPanel from '../components/LibraryPlaylistPanel.vue'
import OnlineMusicSearchSection from '../components/OnlineMusicSearchSection.vue'
import RemoteSubsonicSection from '../components/RemoteSubsonicSection.vue'
import TrackTable from '../components/TrackTable.vue'
import { useMusicI18n as useI18n } from '../i18n'
import { usePlayerStore } from '../player'
import { useMusicStore } from '../store'
import type {
  AlbumDto,
  AlbumListType,
  ArtistDto,
  ArtistRoleFilter,
  SortOrder,
  TrackDto,
  TrackSortField,
} from '../types'

type MusicViewMode = 'items' | 'tracks' | 'artists' | 'genres' | 'tags'

const VALID_TABS: readonly LibraryTab[] = ['all', 'continue', 'favorites', 'collections']
const VALID_VIEWS: readonly MusicViewMode[] = ['items', 'tracks', 'artists', 'genres', 'tags']
const TRACKS_PAGE_SIZE = 50
const ALBUMS_PAGE_SIZE = 36
const ARTISTS_PAGE_SIZE = 48

const { t } = useI18n()
const { t: globalT } = useGlobalI18n()
const route = useRoute()
const router = useRouter()
const musicStore = useMusicStore()
const playerStore = usePlayerStore()
const uploadStore = useUploadStore()
const sourceFilter = useSourceFilter('music')

const onlineSearchRef = ref<InstanceType<typeof OnlineMusicSearchSection> | null>(null)
const remoteSearchRef = ref<InstanceType<typeof RemoteSubsonicSection> | null>(null)
const continuePanelRef = ref<InstanceType<typeof LibraryContinuePanel> | null>(null)
const favoritesPanelRef = ref<InstanceType<typeof LibraryFavoritesPanel> | null>(null)

function readSingleQuery(key: string): string {
  const raw = route.query[key]
  if (typeof raw === 'string') return raw.trim()
  if (Array.isArray(raw)) {
    const first = raw.find((v): v is string => typeof v === 'string' && v.trim().length > 0)
    return first ? first.trim() : ''
  }
  return ''
}

const activeScope = computed<SourceScope>(() => sourceFilter.filter.value.scope)
const activeSourceId = computed<string | null>(() => sourceFilter.filter.value.sourceId)

const activeArtistId = computed(() =>
  activeScope.value === 'local' ? readSingleQuery('artist') : '',
)
const activeCollectionId = computed(() =>
  activeScope.value === 'local' && !activeArtistId.value ? readSingleQuery('collection') : '',
)
const isSubDetailView = computed(
  () => Boolean(activeArtistId.value || activeCollectionId.value),
)

const activeTab = computed<LibraryTab>(() => {
  const raw = readSingleQuery('tab') as LibraryTab
  if (VALID_TABS.includes(raw)) {
    if (activeScope.value !== 'local' && raw === 'collections') return 'all'
    return raw
  }
  return 'all'
})

const activeView = computed<MusicViewMode>(() => {
  const raw = readSingleQuery('view') as MusicViewMode
  if (VALID_VIEWS.includes(raw)) return raw
  if (readSingleQuery('tag')) return 'tracks'
  return 'items'
})

const selectedLibrary = computed(() => readSingleQuery('library'))
const selectedGenre = computed(() => readSingleQuery('genre'))
const selectedTag = computed(() => readSingleQuery('tag'))
const committedQuery = computed(() => readSingleQuery('q'))

const currentPage = computed(() => {
  const n = Number(readSingleQuery('page'))
  return Number.isFinite(n) && n >= 1 ? Math.floor(n) : 1
})

const tracksSortKey = computed(() => {
  const sort = readSingleQuery('sort')
  const order = readSingleQuery('order')
  if (sort) {
    const validOrder = order === 'asc' || order === 'desc' ? order : 'asc'
    return `${sort}:${validOrder}`
  }
  return 'created_at:desc'
})

const albumListType = computed<AlbumListType>(() => {
  const sort = readSingleQuery('sort') as AlbumListType
  const allowed: AlbumListType[] = [
    'newest',
    'recent',
    'frequent',
    'random',
    'starred',
    'alphabeticalByName',
    'alphabeticalByArtist',
    'byYear',
    'byGenre',
  ]
  return allowed.includes(sort) ? sort : 'newest'
})

const artistRoleFilter = ref<ArtistRoleFilter>('all')

// Local search input synced with URL `q`
const searchInput = ref(committedQuery.value)
const filterSheetOpen = ref(false)
let searchDebounceTimer: ReturnType<typeof setTimeout> | null = null
let fetchSeq = 0

// Local view data state
const loading = ref(false)
const fetchError = ref<string | null>(null)
const tracks = ref<TrackDto[]>([])
const tracksTotal = ref(0)
const albums = ref<AlbumDto[]>([])
const albumsTotal = ref(0)
const artists = ref<ArtistDto[]>([])
const artistsTotal = ref(0)

const isLocalCatalogMode = computed(
  () => activeScope.value === 'local' && activeTab.value === 'all' && !isSubDetailView.value,
)

const tabs = computed<MdTabItem<string>[]>(() => {
  if (isSubDetailView.value) return []
  const base: MdTabItem<string>[] = [
    { value: 'all', label: t('nav.all'), icon: LuMusic },
    { value: 'continue', label: t('nav.continueListening'), icon: LuClock },
    { value: 'favorites', label: t('nav.favorites'), icon: LuHeart },
  ]
  if (activeScope.value === 'local') {
    base.push({ value: 'collections', label: t('nav.collections'), icon: LuListMusic })
  }
  return base
})

const viewChips = computed<Array<{ value: MusicViewMode; label: string; icon: typeof LuDisc3 }>>(
  () => [
    { value: 'items', label: t('nav.albums'), icon: LuDisc3 },
    { value: 'tracks', label: t('nav.songs'), icon: LuMusic },
    { value: 'artists', label: t('nav.artists'), icon: LuMic },
    { value: 'genres', label: t('nav.genres'), icon: LuRadio },
    { value: 'tags', label: t('nav.tags'), icon: LuTag },
  ],
)

const activeLibraryName = computed(() => {
  if (!selectedLibrary.value) return ''
  return musicStore.libraries.find((l) => l.id === selectedLibrary.value)?.name ?? ''
})

const pageTitle = computed(() => {
  if (activeScope.value === 'online') return t('online.scopeOnline')
  if (activeScope.value === 'remote') return globalT('remote.subsonicTab')
  if (activeLibraryName.value) return activeLibraryName.value
  if (selectedTag.value) return `#${selectedTag.value}`
  if (selectedGenre.value) return selectedGenre.value
  return t('nav.library')
})

const headerCount = computed<number | null>(() => {
  if (!isLocalCatalogMode.value) return null
  if (activeView.value === 'tracks') return tracksTotal.value
  if (activeView.value === 'items') return albumsTotal.value
  if (activeView.value === 'artists') return artistsTotal.value
  if (activeView.value === 'genres') return filteredGenres.value.length
  if (activeView.value === 'tags') return filteredTags.value.length
  return null
})

const activeFilterCount = computed(() => {
  if (!isLocalCatalogMode.value) return 0
  let count = 0
  if (selectedLibrary.value) count += 1
  if (selectedGenre.value) count += 1
  if (selectedTag.value) count += 1
  return count
})

const filteredGenres = computed(() => {
  const needle = searchInput.value.trim().toLowerCase()
  if (!needle) return musicStore.genres
  return musicStore.genres.filter((g) => g.name.toLowerCase().includes(needle))
})

const filteredTags = computed(() => {
  const needle = searchInput.value.trim().toLowerCase()
  if (!needle) return musicStore.tags
  return musicStore.tags.filter((tg) => tg.name.toLowerCase().includes(needle))
})

const libraryOptions = computed<SelectOption[]>(() => [
  { value: '', label: t('catalog.allLibraries') },
  ...musicStore.libraries.map((l) => ({
    value: l.id,
    label: l.name,
    badge: l.track_count,
  })),
])

const genreOptions = computed<SelectOption[]>(() => [
  { value: '', label: t('catalog.allGenres') },
  ...musicStore.genres.map((g) => ({
    value: g.name,
    label: g.name,
    badge: activeView.value === 'items' ? g.album_count : g.song_count,
  })),
])

const tagOptions = computed<SelectOption[]>(() => [
  { value: '', label: globalT('musicTags.allTagsFilter') },
  ...musicStore.tags.map((tg) => ({
    value: tg.name,
    label: `#${tg.name}`,
    badge: tg.song_count,
  })),
])

const trackSortOptions = computed<SelectOption[]>(() => [
  { value: 'created_at:desc', label: t('catalog.sortCreatedDesc') },
  { value: 'title:asc', label: t('catalog.sortTitleAsc') },
  { value: 'artist:asc', label: t('catalog.sortArtistAsc') },
  { value: 'album:asc', label: t('catalog.sortAlbumAsc') },
  { value: 'year:desc', label: t('catalog.sortYearDesc') },
  { value: 'duration:desc', label: t('catalog.sortDurationDesc') },
])

const albumTypeOptions = computed<SelectOption[]>(() => [
  { value: 'newest', label: t('catalog.albumSortNewest') },
  { value: 'recent', label: t('catalog.albumSortRecent') },
  { value: 'frequent', label: t('catalog.albumSortFrequent') },
  { value: 'random', label: t('catalog.albumSortRandom') },
  { value: 'alphabeticalByName', label: t('catalog.albumSortAlphaName') },
  { value: 'alphabeticalByArtist', label: t('catalog.albumSortAlphaArtist') },
  { value: 'starred', label: t('catalog.albumSortStarred') },
])

const artistRoleOptions = computed<SelectOption[]>(() => [
  { value: 'all', label: t('catalog.artistRoleAll') },
  { value: 'album_artist', label: t('catalog.artistRoleAlbum') },
  { value: 'track_artist', label: t('catalog.artistRoleTrack') },
])

const activePagination = computed(() => {
  if (!isLocalCatalogMode.value) {
    return { page: 1, pageSize: 24, total: null as number | null, totalPages: 1 }
  }
  if (activeView.value === 'tracks') {
    return {
      page: currentPage.value,
      pageSize: TRACKS_PAGE_SIZE,
      total: tracksTotal.value,
      totalPages: Math.max(1, Math.ceil(tracksTotal.value / TRACKS_PAGE_SIZE)),
    }
  }
  if (activeView.value === 'items') {
    return {
      page: currentPage.value,
      pageSize: ALBUMS_PAGE_SIZE,
      total: albumsTotal.value,
      totalPages: Math.max(1, Math.ceil(albumsTotal.value / ALBUMS_PAGE_SIZE)),
    }
  }
  if (activeView.value === 'artists') {
    return {
      page: currentPage.value,
      pageSize: ARTISTS_PAGE_SIZE,
      total: artistsTotal.value,
      totalPages: Math.max(1, Math.ceil(artistsTotal.value / ARTISTS_PAGE_SIZE)),
    }
  }
  return { page: 1, pageSize: 24, total: null as number | null, totalPages: 1 }
})

const isLocalCatalogEmpty = computed(() => {
  if (!isLocalCatalogMode.value || loading.value || fetchError.value) return false
  if (activeView.value === 'tracks') return tracks.value.length === 0
  if (activeView.value === 'items') return albums.value.length === 0
  if (activeView.value === 'artists') return artists.value.length === 0
  if (activeView.value === 'genres') return filteredGenres.value.length === 0
  if (activeView.value === 'tags') return filteredTags.value.length === 0
  return false
})

const searchPlaceholder = computed(() => {
  if (activeScope.value === 'online') return t('online.searchPlaceholder')
  if (activeScope.value === 'remote') return globalT('remote.subsonicSearchPlaceholder')
  return t('common.filterPlaceholder')
})

function updateQuery(patch: Record<string, string | null | undefined>, resetPage = false): void {
  const next: LocationQueryRaw = {}
  for (const [k, v] of Object.entries(route.query)) {
    if (v !== undefined && v !== null) next[k] = v
  }
  for (const [k, v] of Object.entries(patch)) {
    if (v === null || v === undefined || v === '') {
      delete next[k]
    } else {
      next[k] = v
    }
  }
  if (resetPage) {
    delete next.page
  }
  void router.replace({ path: '/music/library', query: next })
}

function handleTabChange(nextTab: string): void {
  const tab = nextTab as LibraryTab
  updateQuery(
    {
      tab: tab === 'all' ? null : tab,
      artist: null,
      collection: null,
    },
    true,
  )
}

function handleViewChange(nextView: MusicViewMode): void {
  const patch: Record<string, string | null> = {
    view: nextView === 'items' ? null : nextView,
    sort: null,
    order: null,
  }
  if (nextView === 'artists' || nextView === 'genres' || nextView === 'tags') {
    patch.library = null
    patch.genre = null
    patch.tag = null
  } else if (nextView === 'items') {
    patch.tag = null
  }
  updateQuery(patch, true)
}

function handleSelectGenre(name: string): void {
  updateQuery({ genre: name, view: 'items' }, true)
}

function handleSelectTag(name: string): void {
  updateQuery({ tag: name, view: 'tracks' }, true)
}

function handleLibraryFilterChange(val: string): void {
  updateQuery({ library: val || null }, true)
}

function handleGenreFilterChange(val: string): void {
  updateQuery({ genre: val || null }, true)
}

function handleTagFilterChange(val: string): void {
  updateQuery({ tag: val || null }, true)
}

function handleTrackSortChange(val: string): void {
  const [sort, order] = val.split(':')
  updateQuery(
    {
      sort: sort === 'created_at' && order === 'desc' ? null : sort || null,
      order: sort === 'created_at' && order === 'desc' ? null : order || null,
    },
    true,
  )
}

function handleAlbumSortChange(val: string): void {
  updateQuery({ sort: val === 'newest' ? null : val, order: null }, true)
}

function handleArtistRoleChange(val: string): void {
  artistRoleFilter.value = val as ArtistRoleFilter
  updateQuery({}, true)
  void fetchLocalCatalog()
}

function resetAllFilters(): void {
  updateQuery({ library: null, genre: null, tag: null, sort: null, order: null }, true)
}

function clearDebounce(): void {
  if (searchDebounceTimer !== null) {
    clearTimeout(searchDebounceTimer)
    searchDebounceTimer = null
  }
}

function handleSearchInputUpdate(val: string): void {
  searchInput.value = val
  clearDebounce()
  searchDebounceTimer = setTimeout(() => {
    searchDebounceTimer = null
    const trimmed = searchInput.value.trim()
    if (trimmed !== committedQuery.value) {
      updateQuery({ q: trimmed || null }, true)
    }
  }, 320)
}

function handleSearchSubmit(val: string): void {
  clearDebounce()
  const trimmed = val.trim()
  if (activeScope.value === 'online' && activeTab.value === 'all') {
    updateQuery({ q: trimmed || null }, true)
    void onlineSearchRef.value?.submitSearch(trimmed)
  } else if (trimmed !== committedQuery.value) {
    updateQuery({ q: trimmed || null }, true)
  } else if (activeScope.value === 'remote' && activeTab.value === 'all') {
    remoteSearchRef.value?.reload()
  } else if (isLocalCatalogMode.value) {
    void fetchLocalCatalog()
  }
}

function handleSearchClear(): void {
  clearDebounce()
  searchInput.value = ''
  updateQuery({ q: null }, true)
  if (activeScope.value === 'online' && activeTab.value === 'all') {
    onlineSearchRef.value?.clearSearch()
  }
}

function handlePageChange(nextPage: number): void {
  updateQuery({ page: nextPage > 1 ? String(nextPage) : null })
}

function handleOnlineKeywordUpdate(kw: string): void {
  searchInput.value = kw
  if (kw.trim() !== committedQuery.value) {
    updateQuery({ q: kw.trim() || null })
  }
}

function handleSourceIdUpdate(sourceId: string | null): void {
  void sourceFilter.setSource(sourceId)
}

function clearArtist(): void {
  updateQuery({ artist: null, view: 'artists' }, true)
}

function clearCollection(): void {
  updateQuery({ collection: null, tab: 'collections' }, true)
}

async function fetchLocalCatalog(): Promise<void> {
  if (!isLocalCatalogMode.value) return
  if (activeView.value === 'genres' || activeView.value === 'tags') {
    fetchError.value = null
    loading.value = false
    if (musicStore.genres.length === 0 && musicStore.tags.length === 0) {
      await musicStore.refreshSidebar()
    }
    return
  }

  const seq = ++fetchSeq
  loading.value = true
  fetchError.value = null
  try {
    if (activeView.value === 'tracks') {
      const [field, order] = tracksSortKey.value.split(':') as [TrackSortField, SortOrder]
      const res = await api.queryTracks({
        library_id: selectedLibrary.value || undefined,
        genre: selectedGenre.value || undefined,
        tag: selectedTag.value || undefined,
        search: committedQuery.value || undefined,
        sort_by: field,
        sort_order: order,
        page: currentPage.value,
        page_size: TRACKS_PAGE_SIZE,
      })
      if (seq !== fetchSeq) return
      tracks.value = res.items
      tracksTotal.value = res.total
    } else if (activeView.value === 'items') {
      const res = await api.listAlbums({
        type: albumListType.value,
        library_id: selectedLibrary.value || undefined,
        genre: selectedGenre.value || undefined,
        search: committedQuery.value || undefined,
        offset: (currentPage.value - 1) * ALBUMS_PAGE_SIZE,
        size: ALBUMS_PAGE_SIZE,
      })
      if (seq !== fetchSeq) return
      albums.value = res.items
      albumsTotal.value = res.total
    } else if (activeView.value === 'artists') {
      const res = await api.listArtists({
        search: committedQuery.value || undefined,
        role: artistRoleFilter.value,
        offset: (currentPage.value - 1) * ARTISTS_PAGE_SIZE,
        size: ARTISTS_PAGE_SIZE,
      })
      if (seq !== fetchSeq) return
      artists.value = res.items
      artistsTotal.value = res.total
    }
  } catch (err) {
    if (seq !== fetchSeq) return
    fetchError.value = err instanceof Error ? err.message : String(err)
  } finally {
    if (seq === fetchSeq) {
      loading.value = false
    }
  }
}

async function handleRefresh(): Promise<void> {
  if (activeTab.value === 'continue') {
    await continuePanelRef.value?.refresh()
    return
  }
  if (activeTab.value === 'favorites') {
    await favoritesPanelRef.value?.refresh()
    return
  }
  if (activeScope.value === 'online') {
    await onlineSearchRef.value?.retryInit()
    return
  }
  if (activeScope.value === 'remote') {
    remoteSearchRef.value?.reload()
    return
  }
  await Promise.allSettled([musicStore.refreshSidebar(), fetchLocalCatalog()])
}

function handleTagsUpdated(payload: { trackId: string; tags: string[] }): void {
  tracks.value = tracks.value.map((tr) =>
    tr.id === payload.trackId ? { ...tr, tags: payload.tags } : tr,
  )
}

watch(committedQuery, (next) => {
  if (next !== searchInput.value.trim()) {
    searchInput.value = next
  }
})

watch(
  () => [
    activeScope.value,
    activeTab.value,
    activeView.value,
    activeArtistId.value,
    activeCollectionId.value,
    selectedLibrary.value,
    selectedGenre.value,
    selectedTag.value,
    committedQuery.value,
    tracksSortKey.value,
    albumListType.value,
    currentPage.value,
  ],
  () => {
    if (isLocalCatalogMode.value) {
      void fetchLocalCatalog()
    }
  },
  { immediate: true },
)

watch(
  () => [musicStore.catalogVersion, uploadStore.versionCounter],
  () => {
    if (isLocalCatalogMode.value) {
      void fetchLocalCatalog()
    }
  },
)

onMounted(() => {
  if (musicStore.libraries.length === 0 && musicStore.genres.length === 0) {
    void musicStore.refreshSidebar()
  }
})

onBeforeUnmount(clearDebounce)
</script>

<template>
  <div class="px-4 py-6 sm:px-6 lg:px-8">
    <DomainLibraryTemplate
      domain="music"
      :title="pageTitle"
      :count="headerCount"
      :show-scope-bar="true"
      :show-search="!isSubDetailView"
      :search-query="searchInput"
      :search-placeholder="searchPlaceholder"
      :tabs="tabs"
      :active-tab="activeTab"
      :loading="isLocalCatalogMode ? loading : false"
      :skeleton-mode="activeView === 'tracks' ? 'list' : 'grid'"
      skeleton-aspect="square"
      :error="isLocalCatalogMode ? fetchError : null"
      :empty="isLocalCatalogEmpty"
      :page="activePagination.page"
      :page-size="activePagination.pageSize"
      :total="activePagination.total"
      :total-pages="activePagination.totalPages"
      :filter-sheet-open="filterSheetOpen"
      :active-filter-count="activeFilterCount"
      @update:search-query="handleSearchInputUpdate"
      @search-submit="handleSearchSubmit"
      @search-clear="handleSearchClear"
      @update:active-tab="handleTabChange"
      @update:page="handlePageChange"
      @update:filter-sheet-open="filterSheetOpen = $event"
      @reset-filters="resetAllFilters"
      @retry="fetchLocalCatalog"
    >
      <!-- Local Catalog Dimension Chips + Active Filter Chips -->
      <template v-if="isLocalCatalogMode" #toolbar-chips>
        <MdChip
          v-for="chip in viewChips"
          :key="chip.value"
          variant="filter"
          :icon="chip.icon"
          :label="chip.label"
          :selected="activeView === chip.value"
          hide-check
          @click="handleViewChange(chip.value)"
        />
        <MdChip
          v-if="selectedLibrary && activeLibraryName"
          variant="input"
          :label="activeLibraryName"
          selected
          removable
          hide-check
          @remove="handleLibraryFilterChange('')"
        />
        <MdChip
          v-if="selectedGenre"
          variant="input"
          :label="selectedGenre"
          selected
          removable
          hide-check
          @remove="handleGenreFilterChange('')"
        />
        <MdChip
          v-if="selectedTag"
          variant="input"
          :label="`#${selectedTag}`"
          selected
          removable
          hide-check
          @remove="handleTagFilterChange('')"
        />
      </template>

      <!-- Compact Trailing Controls -->
      <template v-if="isLocalCatalogMode" #toolbar-trailing>
        <template v-if="activeView === 'tracks' && tracks.length > 0">
          <MdButton
            variant="tonal"
            size="sm"
            :icon="LuPlay"
            @click="playerStore.playQueue(tracks, 0)"
          >
            {{ t('common.playAll') }}
          </MdButton>
          <MdIconButton
            size="sm"
            variant="standard"
            :icon="LuShuffle"
            :aria-label="t('common.shufflePlay')"
            :tooltip="t('common.shufflePlay')"
            @click="playerStore.shuffleAndPlay(tracks)"
          />
        </template>
        <MdIconButton
          size="sm"
          variant="standard"
          :icon="LuRefreshCw"
          :loading="loading"
          :aria-label="globalT('common.refresh')"
          :tooltip="globalT('common.refresh')"
          @click="handleRefresh"
        />
      </template>

      <!-- Responsive Filter Sheet (only for local catalog) -->
      <template v-if="isLocalCatalogMode && (activeView === 'items' || activeView === 'tracks' || activeView === 'artists')" #filter-sheet>
        <div class="space-y-4">
          <div v-if="activeView === 'items' || activeView === 'tracks'" class="space-y-1.5">
            <label class="text-label-medium text-on-surface-variant">
              {{ t('catalog.allLibraries') }}
            </label>
            <MdSelect
              :model-value="selectedLibrary"
              :options="libraryOptions"
              @update:model-value="handleLibraryFilterChange"
            />
          </div>

          <div v-if="activeView === 'items' || activeView === 'tracks'" class="space-y-1.5">
            <label class="text-label-medium text-on-surface-variant">
              {{ t('nav.genres') }}
            </label>
            <MdSelect
              :model-value="selectedGenre"
              :options="genreOptions"
              searchable
              @update:model-value="handleGenreFilterChange"
            />
          </div>

          <div v-if="activeView === 'tracks'" class="space-y-1.5">
            <label class="text-label-medium text-on-surface-variant">
              {{ t('nav.tags') }}
            </label>
            <MdSelect
              :model-value="selectedTag"
              :options="tagOptions"
              searchable
              @update:model-value="handleTagFilterChange"
            />
          </div>

          <div v-if="activeView === 'tracks'" class="space-y-1.5">
            <label class="text-label-medium text-on-surface-variant">
              {{ globalT('common.sort') }}
            </label>
            <MdSelect
              :model-value="tracksSortKey"
              :options="trackSortOptions"
              @update:model-value="handleTrackSortChange"
            />
          </div>

          <div v-else-if="activeView === 'items'" class="space-y-1.5">
            <label class="text-label-medium text-on-surface-variant">
              {{ globalT('common.sort') }}
            </label>
            <MdSelect
              :model-value="albumListType"
              :options="albumTypeOptions"
              @update:model-value="handleAlbumSortChange"
            />
          </div>

          <div v-else-if="activeView === 'artists'" class="space-y-1.5">
            <label class="text-label-medium text-on-surface-variant">
              {{ t('nav.artists') }}
            </label>
            <MdSelect
              :model-value="artistRoleFilter"
              :options="artistRoleOptions"
              @update:model-value="handleArtistRoleChange"
            />
          </div>
        </div>
      </template>

      <!-- Main Body Routing by Sub-Detail / Tab / Scope / View -->
      <LibraryArtistPanel
        v-if="activeArtistId"
        :artist-id="activeArtistId"
        @back="clearArtist"
      />

      <LibraryPlaylistPanel
        v-else-if="activeCollectionId"
        :playlist-id="activeCollectionId"
        @back="clearCollection"
        @deleted="clearCollection"
      />

      <LibraryContinuePanel
        v-else-if="activeTab === 'continue'"
        ref="continuePanelRef"
        :scope="activeScope"
        :source-id="activeSourceId"
        :query="searchInput"
      />

      <LibraryFavoritesPanel
        v-else-if="activeTab === 'favorites'"
        ref="favoritesPanelRef"
        :scope="activeScope"
        :source-id="activeSourceId"
        :query="searchInput"
      />

      <LibraryCollectionsPanel
        v-else-if="activeScope === 'local' && activeTab === 'collections'"
        :query="searchInput"
      />

      <OnlineMusicSearchSection
        v-else-if="activeScope === 'online'"
        ref="onlineSearchRef"
        :keyword="committedQuery"
        :initial-source-id="activeSourceId"
        :show-search-input="false"
        @update:keyword="handleOnlineKeywordUpdate"
        @update:source-id="handleSourceIdUpdate"
      />

      <RemoteSubsonicSection
        v-else-if="activeScope === 'remote'"
        ref="remoteSearchRef"
        :keyword="committedQuery"
        :selected-source-id="activeSourceId"
        @update:source-id="handleSourceIdUpdate"
      />

      <!-- Local Catalog Views (items=Albums | tracks | artists | genres | tags) -->
      <template v-else>
        <TrackTable
          v-if="activeView === 'tracks'"
          :tracks="tracks"
          @tags-updated="handleTagsUpdated"
        />

        <div
          v-else-if="activeView === 'items'"
          class="grid grid-cols-2 gap-5 sm:grid-cols-3 md:grid-cols-4 lg:grid-cols-5 xl:grid-cols-6"
        >
          <AlbumCard v-for="al in albums" :key="al.id" :album="al" />
        </div>

        <div
          v-else-if="activeView === 'artists'"
          class="grid grid-cols-2 gap-4 sm:grid-cols-3 md:grid-cols-4 lg:grid-cols-5 xl:grid-cols-6"
        >
          <ArtistCard v-for="ar in artists" :key="ar.id" :artist="ar" />
        </div>

        <div
          v-else-if="activeView === 'genres'"
          class="flex flex-wrap gap-2"
        >
          <MdChip
            v-for="genre in filteredGenres"
            :key="genre.name"
            variant="assist"
            :icon="LuRadio"
            :label="`${genre.name} (${genre.song_count})`"
            @click="handleSelectGenre(genre.name)"
          />
        </div>

        <div
          v-else-if="activeView === 'tags'"
          class="flex flex-wrap gap-2"
        >
          <MdChip
            v-for="tg in filteredTags"
            :key="tg.name"
            variant="assist"
            :icon="LuTag"
            :label="`#${tg.name} (${tg.song_count})`"
            @click="handleSelectTag(tg.name)"
          />
        </div>
      </template>
    </DomainLibraryTemplate>
  </div>
</template>

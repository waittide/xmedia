import { computed, onBeforeUnmount, onMounted, ref, watch } from 'vue'
import { useI18n as useGlobalI18n } from 'vue-i18n'
import { useRoute, useRouter, type LocationQueryRaw } from 'vue-router'
import {
  LuClock,
  LuDisc3,
  LuHeart,
  LuListMusic,
  LuMic,
  LuMusic,
  LuRadio,
  LuTag,
} from 'vue-icons-plus/lu'
import type { MdTabItem, SelectOption } from '@/components/ui'
import { useSourceFilter, type LibraryTab, type SourceScope } from '@/sources'
import { useUploadStore } from '@/stores/uploads'
import { api } from '../api'
import { useMusicI18n as useI18n } from '../i18n'
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

export type MusicViewMode = 'items' | 'tracks' | 'artists' | 'genres' | 'tags'

const VALID_TABS: readonly LibraryTab[] = ['all', 'continue', 'favorites', 'collections']
const VALID_VIEWS: readonly MusicViewMode[] = ['items', 'tracks', 'artists', 'genres', 'tags']
const TRACKS_PAGE_SIZE = 50
const ALBUMS_PAGE_SIZE = 36
const ARTISTS_PAGE_SIZE = 48

export function useMusicLibraryCatalog() {
  const { t } = useI18n()
  const { t: globalT } = useGlobalI18n()
  const route = useRoute()
  const router = useRouter()
  const musicStore = useMusicStore()
  const uploadStore = useUploadStore()
  const sourceFilter = useSourceFilter('music')

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
    if (activeScope.value !== 'local') return 'all'
    const raw = readSingleQuery('tab') as LibraryTab
    return VALID_TABS.includes(raw) ? raw : 'all'
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
  const searchInput = ref(committedQuery.value)
  const filterSheetOpen = ref(false)
  let searchDebounceTimer: ReturnType<typeof setTimeout> | null = null
  let fetchSeq = 0

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
    if (isSubDetailView.value || activeScope.value !== 'local') return []
    return [
      { value: 'all', label: t('nav.all'), icon: LuMusic },
      { value: 'continue', label: t('nav.continueListening'), icon: LuClock },
      { value: 'favorites', label: t('nav.favorites'), icon: LuHeart },
      { value: 'collections', label: t('nav.collections'), icon: LuListMusic },
    ]
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

  const filteredGenres = computed(() => {
    const needle = searchInput.value.trim().toLowerCase()
    return needle
      ? musicStore.genres.filter((g) => g.name.toLowerCase().includes(needle))
      : musicStore.genres
  })

  const filteredTags = computed(() => {
    const needle = searchInput.value.trim().toLowerCase()
    return needle
      ? musicStore.tags.filter((tg) => tg.name.toLowerCase().includes(needle))
      : musicStore.tags
  })

  const pageTitle = computed(() => {
    if (activeScope.value === 'pool') return globalT('pool.title')
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
    return (
      (selectedLibrary.value ? 1 : 0) +
      (selectedGenre.value ? 1 : 0) +
      (selectedTag.value ? 1 : 0)
    )
  })

  const libraryOptions = computed<SelectOption[]>(() => [
    { value: '', label: t('catalog.allLibraries') },
    ...musicStore.libraries.map((l) => ({ value: l.id, label: l.name, badge: l.track_count })),
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
    if (resetPage) delete next.page
    void router.replace({ path: '/music/library', query: next })
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
      if (isLocalCatalogMode.value) void fetchLocalCatalog()
    },
    { immediate: true },
  )

  watch(
    () => [musicStore.catalogVersion, uploadStore.versionCounter],
    () => {
      if (isLocalCatalogMode.value) void fetchLocalCatalog()
    },
  )

  onMounted(() => {
    if (musicStore.libraries.length === 0 && musicStore.genres.length === 0) {
      void musicStore.refreshSidebar()
    }
  })

  onBeforeUnmount(clearDebounce)

  return {
    musicStore,
    sourceFilter,
    activeScope,
    activeSourceId,
    activeArtistId,
    activeCollectionId,
    isSubDetailView,
    activeTab,
    activeView,
    selectedLibrary,
    selectedGenre,
    selectedTag,
    committedQuery,
    tracksSortKey,
    albumListType,
    artistRoleFilter,
    searchInput,
    filterSheetOpen,
    loading,
    fetchError,
    tracks,
    albums,
    artists,
    isLocalCatalogMode,
    tabs,
    viewChips,
    activeLibraryName,
    filteredGenres,
    filteredTags,
    pageTitle,
    headerCount,
    activeFilterCount,
    libraryOptions,
    genreOptions,
    tagOptions,
    trackSortOptions,
    albumTypeOptions,
    artistRoleOptions,
    activePagination,
    isLocalCatalogEmpty,
    searchPlaceholder,
    updateQuery,
    clearDebounce,
    handleSearchInputUpdate,
    fetchLocalCatalog,
  }
}

<script setup lang="ts">
import { computed, nextTick, onBeforeUnmount, onMounted, ref, watch, type Component } from 'vue'
import { useRoute, useRouter } from 'vue-router'
import { useI18n } from 'vue-i18n'
import {
  LuBookOpen,
  LuChevronDown,
  LuChevronRight,
  LuClapperboard,
  LuClock,
  LuDisc3,
  LuFilm,
  LuFolder,
  LuFolderHeart,
  LuFolderPlus,
  LuHeart,
  LuHome,
  LuLayers,
  LuLibrary,
  LuListMusic,
  LuLogOut,
  LuMonitor,
  LuMoon,
  LuMusic,
  LuPanelLeftClose,
  LuPanelLeftOpen,
  LuRefreshCw,
  LuSearch,
  LuSettings,
  LuSun,
  LuTag,
  LuTrash2,
  LuUpload,
  LuX,
} from 'vue-icons-plus/lu'
import { useAuthStore } from '@/stores/auth'
import { useDialogStore } from '@/stores/dialog'
import { useDomainStore } from '@/stores/domain'
import { useEventsStore } from '@/stores/events'
import { useThemeStore } from '@/stores/theme'
import { useUploadStore } from '@/stores/uploads'
import type { MediaKind } from '@/types/api'

import { api as bookApi } from '@/domains/book/api'
import { useBookI18n } from '@/domains/book/i18n'
import { useCatalogStore, type NavSection as BookNavSection } from '@/domains/book/store'

import { api as comicApi, withAuthUrl } from '@/domains/comic/api'
import { useComicI18n } from '@/domains/comic/i18n'
import { resolveLibrarySeriesCount, useSidebarStore as useComicSidebarStore } from '@/domains/comic/store'
import type { SeriesSummaryDto } from '@/domains/comic/types'

import { api as musicApi } from '@/domains/music/api'
import { useMusicI18n } from '@/domains/music/i18n'
import { usePlayerStore } from '@/domains/music/player'
import { useMusicStore } from '@/domains/music/store'
import type { PlaylistSummaryDto } from '@/domains/music/types'
import PlayerBar from '@/domains/music/components/PlayerBar.vue'
import QueueDrawer from '@/domains/music/components/QueueDrawer.vue'
import LyricsOverlay from '@/domains/music/components/LyricsOverlay.vue'

import { api as videoApi } from '@/domains/video/api'
import { useVideoI18n } from '@/domains/video/i18n'
import { useVideoStore } from '@/domains/video/store'
import type { VideoShelfDto } from '@/domains/video/types'

import UploadShelf from '@/components/UploadShelf.vue'

const SIDEBAR_PREVIEW_LIMIT = 5

const { t } = useI18n()
const { t: bookT } = useBookI18n()
const { t: comicT } = useComicI18n()
const { t: musicT } = useMusicI18n()
const { t: videoT } = useVideoI18n()

const route = useRoute()
const router = useRouter()
const authStore = useAuthStore()
const dialogStore = useDialogStore()
const domainStore = useDomainStore()
const eventsStore = useEventsStore()
const themeStore = useThemeStore()
const uploadStore = useUploadStore()

const bookStore = useCatalogStore()
const comicSidebarStore = useComicSidebarStore()
const musicStore = useMusicStore()
const playerStore = usePlayerStore()
const videoStore = useVideoStore()

const collapsed = ref(localStorage.getItem('xmedia.sidebar_collapsed') === '1')
const mobileMenuOpen = ref(false)
const accountMenuOpen = ref(false)
const dragDepth = ref(0)

const filePickerRef = ref<HTMLInputElement | null>(null)
const searchWrapRef = ref<HTMLFormElement | null>(null)
const searchInputRef = ref<HTMLInputElement | null>(null)
const comicSearchDropdownRef = ref<HTMLDivElement | null>(null)
const accountMenuRef = ref<HTMLElement | null>(null)

// Domain-specific search inputs (book & music already live in their stores)
const comicSearchQuery = ref('')
const videoSearchQuery = ref('')
const comicSearchFocused = ref(false)
const comicSearchResults = ref<SeriesSummaryDto[]>([])
const comicSearchLoading = ref(false)
const searchRect = ref({ top: 56, left: 0, width: 480 })
let comicSearchTimer: ReturnType<typeof setTimeout> | null = null

// Inline creation state in sidebar
const creatingCollection = ref(false)
const newCollectionName = ref('')

// Sidebar group fold & expand states
const librariesFolded = ref(localStorage.getItem('xmedia.fold_libraries') === '1')
const collectionsFolded = ref(localStorage.getItem('xmedia.fold_collections') === '1')
const tagsFolded = ref(localStorage.getItem('xmedia.fold_tags') === '1')

const librariesExpanded = ref(false)
const collectionsExpanded = ref(false)
const tagsExpanded = ref(false)

const loadedDomains = new Set<MediaKind>()

async function ensureDomainSidebarLoaded(kind: MediaKind): Promise<void> {
  if (!authStore.hasModule(kind) || loadedDomains.has(kind)) return
  loadedDomains.add(kind)
  try {
    if (kind === 'book') await bookStore.refreshSidebarData()
    else if (kind === 'comic') await comicSidebarStore.refresh()
    else if (kind === 'music') await musicStore.refreshSidebar()
    else if (kind === 'video') await videoStore.refreshSidebar()
  } catch {
    loadedDomains.delete(kind)
  }
}

watch(
  () => domainStore.activeDomain,
  (next) => {
    creatingCollection.value = false
    newCollectionName.value = ''
    librariesExpanded.value = false
    collectionsExpanded.value = false
    tagsExpanded.value = false
    comicSearchFocused.value = false
    void ensureDomainSidebarLoaded(next)
  },
  { immediate: true },
)

const domainTabs = computed<Array<{ kind: MediaKind; label: string; icon: Component }>>(() => {
  const all: Array<{ kind: MediaKind; label: string; icon: Component }> = [
    { kind: 'book', label: t('app.domains.book'), icon: LuBookOpen },
    { kind: 'comic', label: t('app.domains.comic'), icon: LuLayers },
    { kind: 'music', label: t('app.domains.music'), icon: LuDisc3 },
    { kind: 'video', label: t('app.domains.video'), icon: LuClapperboard },
  ]
  return all.filter((item) => authStore.hasModule(item.kind))
})

const acceptedExtensions = computed(() => {
  const exts: string[] = []
  if (authStore.hasModule('book')) exts.push('.epub', '.txt', '.pdf')
  if (authStore.hasModule('comic')) exts.push('.cbz', '.zip')
  if (authStore.hasModule('music')) {
    exts.push('.flac', '.mp3', '.m4a', '.aac', '.alac', '.ogg', '.opus', '.wav', '.lrc')
  }
  if (authStore.hasModule('video')) {
    exts.push('.mp4', '.mkv', '.webm', '.mov', '.avi', '.ts', '.m4v', '.flv', '.srt', '.vtt', '.ass', '.ssa')
  }
  return exts.join(',')
})

const showMusicBar = computed(
  () =>
    authStore.hasModule('music') &&
    (domainStore.activeDomain === 'music' || playerStore.currentTrack !== null),
)

const isCompact = computed(() => collapsed.value && !mobileMenuOpen.value)

async function switchDomain(kind: MediaKind): Promise<void> {
  domainStore.setActiveDomain(kind)
  mobileMenuOpen.value = false
  await router.push(`/${kind}`)
}

function handleSidebarButtonClick(): void {
  if (window.innerWidth < 768) {
    mobileMenuOpen.value = !mobileMenuOpen.value
    return
  }
  collapsed.value = !collapsed.value
  localStorage.setItem('xmedia.sidebar_collapsed', collapsed.value ? '1' : '0')
}

function toggleGroupFold(group: 'libraries' | 'collections' | 'tags'): void {
  if (group === 'libraries') {
    librariesFolded.value = !librariesFolded.value
    localStorage.setItem('xmedia.fold_libraries', librariesFolded.value ? '1' : '0')
  } else if (group === 'collections') {
    collectionsFolded.value = !collectionsFolded.value
    localStorage.setItem('xmedia.fold_collections', collectionsFolded.value ? '1' : '0')
  } else {
    tagsFolded.value = !tagsFolded.value
    localStorage.setItem('xmedia.fold_tags', tagsFolded.value ? '1' : '0')
  }
}

// Search logic across 4 domains
const activeSearchValue = computed({
  get(): string {
    const d = domainStore.activeDomain
    if (d === 'book') return bookStore.keyword
    if (d === 'comic') return comicSearchQuery.value
    if (d === 'music') return musicStore.globalSearchQuery
    return videoSearchQuery.value
  },
  set(val: string): void {
    const d = domainStore.activeDomain
    if (d === 'book') bookStore.keyword = val
    else if (d === 'comic') comicSearchQuery.value = val
    else if (d === 'music') musicStore.globalSearchQuery = val
    else videoSearchQuery.value = val
  },
})

const searchPlaceholder = computed(() => {
  const d = domainStore.activeDomain
  if (d === 'book') return t('common.searchBook')
  if (d === 'comic') return t('common.searchComic')
  if (d === 'music') return t('common.searchMusic')
  return t('common.searchVideo')
})

function updateSearchDropdownPosition(): void {
  const el = searchWrapRef.value
  if (!el) return
  const rect = el.getBoundingClientRect()
  searchRect.value = {
    top: rect.bottom + 8,
    left: rect.left,
    width: rect.width,
  }
}

async function runComicLiveSearch(q: string): Promise<void> {
  const trimmed = q.trim()
  if (!trimmed) {
    comicSearchResults.value = []
    return
  }
  comicSearchLoading.value = true
  try {
    const list = await comicApi.listSeries({ search: trimmed })
    comicSearchResults.value = list.slice(0, 8)
  } finally {
    comicSearchLoading.value = false
  }
}

watch(comicSearchQuery, (q) => {
  if (comicSearchTimer) clearTimeout(comicSearchTimer)
  if (!q.trim()) {
    comicSearchResults.value = []
    return
  }
  comicSearchTimer = setTimeout(() => {
    void runComicLiveSearch(q)
  }, 160)
})

async function onSearchFocus(): Promise<void> {
  if (domainStore.activeDomain !== 'comic') return
  comicSearchFocused.value = true
  await nextTick()
  updateSearchDropdownPosition()
  if (comicSearchQuery.value.trim()) {
    void runComicLiveSearch(comicSearchQuery.value)
  }
}

function selectComicSearchResult(id: string): void {
  comicSearchFocused.value = false
  comicSearchQuery.value = ''
  void router.push(`/comic/series/${id}`)
}

async function submitGlobalSearch(): Promise<void> {
  const d = domainStore.activeDomain
  comicSearchFocused.value = false
  if (d === 'book') {
    bookStore.page = 1
    if (route.name !== 'book-catalog') {
      bookStore.activeSection = 'all'
      await router.push('/book/catalog')
    }
    await bookStore.fetchBooks()
  } else if (d === 'comic') {
    const q = comicSearchQuery.value.trim()
    searchInputRef.value?.blur()
    await router.push(q ? { path: '/comic/series', query: { q } } : '/comic/series')
  } else if (d === 'music') {
    const q = musicStore.globalSearchQuery.trim()
    if (!q) return
    await router.push({ path: '/music/search', query: { q } })
  } else {
    const q = videoSearchQuery.value.trim()
    await router.push(q ? { path: '/video/catalog', query: { q } } : '/video/catalog')
  }
}

function clearGlobalSearch(): void {
  const d = domainStore.activeDomain
  if (d === 'book') {
    bookStore.resetFilters()
  } else if (d === 'comic') {
    comicSearchQuery.value = ''
    comicSearchResults.value = []
  } else if (d === 'music') {
    musicStore.globalSearchQuery = ''
  } else {
    videoSearchQuery.value = ''
    if (route.name === 'video-catalog' && route.query.q) {
      const nextQuery = { ...route.query }
      delete nextQuery.q
      void router.replace({ path: '/video/catalog', query: nextQuery })
    }
  }
}

// Book sidebar navigation helpers
function isBookSectionActive(section: BookNavSection, id = ''): boolean {
  if (route.name === 'book-detail' && section === 'all') return true
  if (route.name !== 'book-catalog') return false
  if (bookStore.activeSection !== section) return false
  if (section === 'library') return bookStore.activeLibraryId === id
  if (section === 'shelf') return bookStore.activeShelfId === id
  if (section === 'tag') {
    return bookStore.filterTags.length === 1 && bookStore.filterTags[0] === id
  }
  return true
}

async function openBookSection(section: BookNavSection, id = ''): Promise<void> {
  mobileMenuOpen.value = false
  bookStore.selectSection(section, id)
  if (route.name !== 'book-catalog') {
    await router.push('/book/catalog')
  }
}

const visibleBookLibraries = computed(() => {
  const all = bookStore.libraries
  if (librariesExpanded.value || all.length <= SIDEBAR_PREVIEW_LIMIT) return all
  const head = all.slice(0, SIDEBAR_PREVIEW_LIMIT)
  if (
    route.name === 'book-catalog' &&
    bookStore.activeSection === 'library' &&
    bookStore.activeLibraryId &&
    !head.some((l) => l.id === bookStore.activeLibraryId)
  ) {
    const active = all.find((l) => l.id === bookStore.activeLibraryId)
    if (active) return [...head.slice(0, SIDEBAR_PREVIEW_LIMIT - 1), active]
  }
  return head
})

const visibleBookShelves = computed(() => {
  const all = bookStore.shelves
  if (collectionsExpanded.value || all.length <= SIDEBAR_PREVIEW_LIMIT) return all
  const head = all.slice(0, SIDEBAR_PREVIEW_LIMIT)
  if (
    route.name === 'book-catalog' &&
    bookStore.activeSection === 'shelf' &&
    bookStore.activeShelfId &&
    !head.some((s) => s.id === bookStore.activeShelfId)
  ) {
    const active = all.find((s) => s.id === bookStore.activeShelfId)
    if (active) return [...head.slice(0, SIDEBAR_PREVIEW_LIMIT - 1), active]
  }
  return head
})

const visibleBookTags = computed(() => {
  const all = bookStore.tags
  if (tagsExpanded.value || all.length <= SIDEBAR_PREVIEW_LIMIT) return all
  const head = all.slice(0, SIDEBAR_PREVIEW_LIMIT)
  const activeTag = bookStore.filterTags[0]
  if (
    route.name === 'book-catalog' &&
    bookStore.activeSection === 'tag' &&
    activeTag &&
    !head.some((tg) => tg.name === activeTag)
  ) {
    const active = all.find((tg) => tg.name === activeTag)
    if (active) return [...head.slice(0, SIDEBAR_PREVIEW_LIMIT - 1), active]
  }
  return head
})

// Comic sidebar navigation helpers
const activeComicLibraryId = computed(() =>
  route.name === 'comic-series-list' && typeof route.query.library_id === 'string'
    ? route.query.library_id
    : '',
)
const activeComicShelfId = computed(() =>
  route.name === 'comic-series-list' && typeof route.query.shelf_id === 'string'
    ? route.query.shelf_id
    : '',
)
const activeComicTag = computed(() =>
  route.name === 'comic-series-list' && typeof route.query.tag === 'string'
    ? route.query.tag
    : '',
)
const isAllComicSeriesActive = computed(() => {
  if (route.name === 'comic-series-detail') return true
  if (route.name !== 'comic-series-list') return false
  return !activeComicLibraryId.value && !activeComicShelfId.value && !activeComicTag.value
})

const visibleComicLibraries = computed(() => {
  const all = comicSidebarStore.libraries
  if (librariesExpanded.value || all.length <= SIDEBAR_PREVIEW_LIMIT) return all
  const head = all.slice(0, SIDEBAR_PREVIEW_LIMIT)
  if (activeComicLibraryId.value && !head.some((l) => l.id === activeComicLibraryId.value)) {
    const active = all.find((l) => l.id === activeComicLibraryId.value)
    if (active) return [...head.slice(0, SIDEBAR_PREVIEW_LIMIT - 1), active]
  }
  return head
})

const visibleComicShelves = computed(() => {
  const all = comicSidebarStore.shelves
  if (collectionsExpanded.value || all.length <= SIDEBAR_PREVIEW_LIMIT) return all
  const head = all.slice(0, SIDEBAR_PREVIEW_LIMIT)
  if (activeComicShelfId.value && !head.some((s) => s.shelf.id === activeComicShelfId.value)) {
    const active = all.find((s) => s.shelf.id === activeComicShelfId.value)
    if (active) return [...head.slice(0, SIDEBAR_PREVIEW_LIMIT - 1), active]
  }
  return head
})

const visibleComicTags = computed(() => {
  const all = comicSidebarStore.tags
  if (tagsExpanded.value || all.length <= SIDEBAR_PREVIEW_LIMIT) return all
  const head = all.slice(0, SIDEBAR_PREVIEW_LIMIT)
  if (activeComicTag.value && !head.some((tg) => tg.name === activeComicTag.value)) {
    const active = all.find((tg) => tg.name === activeComicTag.value)
    if (active) return [...head.slice(0, SIDEBAR_PREVIEW_LIMIT - 1), active]
  }
  return head
})

// Music sidebar navigation helpers
const isAllMusicActive = computed(() => {
  if (route.name === 'music-album-detail' || route.name === 'music-artist-detail') return true
  if (route.name !== 'music-songs') return false
  return !route.query.library && !route.query.genre
})

function isMusicLibraryActive(id: string): boolean {
  return route.name === 'music-songs' && route.query.library === id
}

function isMusicGenreActive(name: string): boolean {
  return route.name === 'music-songs' && route.query.genre === name
}

async function openMusicCatalog(extraQuery: Record<string, string> = {}): Promise<void> {
  mobileMenuOpen.value = false
  const tab =
    route.name === 'music-songs' && typeof route.query.tab === 'string'
      ? route.query.tab
      : undefined
  await router.push({
    path: '/music/songs',
    query: tab ? { tab, ...extraQuery } : extraQuery,
  })
}

function canEditPlaylist(pl: PlaylistSummaryDto): boolean {
  return pl.owner_id === authStore.user?.id || authStore.isAdmin
}

const visibleMusicLibraries = computed(() => {
  const all = musicStore.libraries
  if (librariesExpanded.value || all.length <= SIDEBAR_PREVIEW_LIMIT) return all
  const head = all.slice(0, SIDEBAR_PREVIEW_LIMIT)
  const activeId = typeof route.query.library === 'string' ? route.query.library : ''
  if (route.name === 'music-songs' && activeId && !head.some((l) => l.id === activeId)) {
    const active = all.find((l) => l.id === activeId)
    if (active) return [...head.slice(0, SIDEBAR_PREVIEW_LIMIT - 1), active]
  }
  return head
})

const visibleMusicPlaylists = computed(() => {
  const all = musicStore.playlists
  if (collectionsExpanded.value || all.length <= SIDEBAR_PREVIEW_LIMIT) return all
  const head = all.slice(0, SIDEBAR_PREVIEW_LIMIT)
  const activeId = route.name === 'music-playlist-detail' ? String(route.params.id || '') : ''
  if (activeId && !head.some((pl) => pl.id === activeId)) {
    const active = all.find((pl) => pl.id === activeId)
    if (active) return [...head.slice(0, SIDEBAR_PREVIEW_LIMIT - 1), active]
  }
  return head
})

const visibleMusicGenres = computed(() => {
  const all = musicStore.genres
  if (tagsExpanded.value || all.length <= SIDEBAR_PREVIEW_LIMIT) return all
  const head = all.slice(0, SIDEBAR_PREVIEW_LIMIT)
  const activeGenre = typeof route.query.genre === 'string' ? route.query.genre : ''
  if (route.name === 'music-songs' && activeGenre && !head.some((g) => g.name === activeGenre)) {
    const active = all.find((g) => g.name === activeGenre)
    if (active) return [...head.slice(0, SIDEBAR_PREVIEW_LIMIT - 1), active]
  }
  return head
})

// Video sidebar navigation helpers
const isAllVideoActive = computed(() => {
  if (route.name === 'video-show-detail') return true
  if (route.name !== 'video-catalog') return false
  return !route.query.library && !route.query.tag
})

function isVideoLibraryActive(id: string): boolean {
  return route.name === 'video-catalog' && route.query.library === id
}

function isVideoTagActive(name: string): boolean {
  return route.name === 'video-catalog' && route.query.tag === name
}

async function openVideoCatalog(extraQuery: Record<string, string> = {}): Promise<void> {
  mobileMenuOpen.value = false
  const tab =
    route.name === 'video-catalog' && typeof route.query.tab === 'string'
      ? route.query.tab
      : undefined
  await router.push({
    path: '/video/catalog',
    query: tab ? { tab, ...extraQuery } : extraQuery,
  })
}

const visibleVideoLibraries = computed(() => {
  const all = videoStore.libraries
  if (librariesExpanded.value || all.length <= SIDEBAR_PREVIEW_LIMIT) return all
  const head = all.slice(0, SIDEBAR_PREVIEW_LIMIT)
  const activeId = typeof route.query.library === 'string' ? route.query.library : ''
  if (route.name === 'video-catalog' && activeId && !head.some((l) => l.id === activeId)) {
    const active = all.find((l) => l.id === activeId)
    if (active) return [...head.slice(0, SIDEBAR_PREVIEW_LIMIT - 1), active]
  }
  return head
})

const visibleVideoShelves = computed(() => {
  const all = videoStore.shelves
  if (collectionsExpanded.value || all.length <= SIDEBAR_PREVIEW_LIMIT) return all
  const head = all.slice(0, SIDEBAR_PREVIEW_LIMIT)
  const activeId = route.name === 'video-shelf-detail' ? String(route.params.id || '') : ''
  if (activeId && !head.some((sh) => sh.id === activeId)) {
    const active = all.find((sh) => sh.id === activeId)
    if (active) return [...head.slice(0, SIDEBAR_PREVIEW_LIMIT - 1), active]
  }
  return head
})

const visibleVideoTags = computed(() => {
  const all = videoStore.tags
  if (tagsExpanded.value || all.length <= SIDEBAR_PREVIEW_LIMIT) return all
  const head = all.slice(0, SIDEBAR_PREVIEW_LIMIT)
  const activeTag = typeof route.query.tag === 'string' ? route.query.tag : ''
  if (route.name === 'video-catalog' && activeTag && !head.some((tg) => tg.name === activeTag)) {
    const active = all.find((tg) => tg.name === activeTag)
    if (active) return [...head.slice(0, SIDEBAR_PREVIEW_LIMIT - 1), active]
  }
  return head
})

// Inline shelf/playlist create & delete handlers
async function handleCreateCollection(): Promise<void> {
  const name = newCollectionName.value.trim()
  if (!name) return
  const d = domainStore.activeDomain
  newCollectionName.value = ''
  creatingCollection.value = false

  if (d === 'book') {
    const created = await bookApi.createShelf(name)
    await bookStore.refreshSidebarData()
    await openBookSection('shelf', created.id)
  } else if (d === 'comic') {
    const created = await comicApi.createShelf({ name })
    await comicSidebarStore.refresh()
    mobileMenuOpen.value = false
    await router.push({ path: '/comic/series', query: { shelf_id: created.id } })
  } else if (d === 'music') {
    const created = await musicApi.createPlaylist({ name })
    await musicStore.refreshSidebar()
    mobileMenuOpen.value = false
    await router.push(`/music/playlists/${created.playlist.id}`)
  } else {
    const created = await videoApi.createShelf({ name })
    await videoStore.refreshSidebar()
    mobileMenuOpen.value = false
    await router.push(`/video/shelves/${created.shelf.id}`)
  }
}

async function handleDeleteBookShelf(id: string, name: string): Promise<void> {
  const ok = await dialogStore.confirm({
    title: t('common.delete'),
    message: name,
    danger: true,
  })
  if (!ok) return
  await bookApi.deleteShelf(id)
  await bookStore.refreshSidebarData()
  if (bookStore.activeSection === 'shelf' && bookStore.activeShelfId === id) {
    await openBookSection('all')
  }
}

async function handleDeleteComicShelf(id: string, name: string): Promise<void> {
  const ok = await dialogStore.confirm({
    title: comicT('shelves.deleteShelf'),
    message: name,
    danger: true,
  })
  if (!ok) return
  await comicApi.deleteShelf(id)
  await comicSidebarStore.refresh()
  if (activeComicShelfId.value === id) {
    await router.push('/comic/series')
  }
}

async function handleDeleteMusicPlaylist(pl: PlaylistSummaryDto): Promise<void> {
  const ok = await dialogStore.confirm({
    title: musicT('playlist.deleteTitle'),
    message: musicT('playlist.confirmDelete', { name: pl.name }),
    danger: true,
  })
  if (!ok) return
  await musicApi.deletePlaylist(pl.id)
  await musicStore.refreshSidebar()
  if (route.path === `/music/playlists/${pl.id}`) {
    await router.push('/music/songs')
  }
}

async function handleDeleteVideoShelf(sh: VideoShelfDto): Promise<void> {
  const ok = await dialogStore.confirm({
    title: videoT('shelf.deleteTitle'),
    message: videoT('shelf.confirmDelete', { name: sh.name }),
    danger: true,
  })
  if (!ok) return
  await videoApi.deleteShelf(sh.id)
  await videoStore.refreshSidebar()
  if (route.path === `/video/shelves/${sh.id}`) {
    await router.push('/video/catalog')
  }
}

// Upload & Drag-and-Drop
function triggerUploadPicker(): void {
  filePickerRef.value?.click()
}

function handlePickedFiles(ev: Event): void {
  const input = ev.target as HTMLInputElement
  if (input.files && input.files.length > 0) {
    uploadStore.enqueueFiles(input.files)
    input.value = ''
  }
}

function onDragEnter(ev: DragEvent): void {
  if (!authStore.isAdmin) return
  if (!ev.dataTransfer?.types.includes('Files')) return
  dragDepth.value += 1
}

function onDragLeave(): void {
  dragDepth.value = Math.max(0, dragDepth.value - 1)
}

function onDropFiles(ev: DragEvent): void {
  dragDepth.value = 0
  if (!authStore.isAdmin) return
  if (ev.dataTransfer?.files && ev.dataTransfer.files.length > 0) {
    uploadStore.enqueueFiles(ev.dataTransfer.files)
  }
}

// Theme & Account
function cycleThemeMode(): void {
  const order = ['system', 'light', 'dark'] as const
  const idx = order.indexOf(themeStore.themeMode)
  const next = order[(idx + 1) % order.length]!
  themeStore.setThemeMode(next)
}

const themeModeTooltip = computed(() => {
  if (themeStore.themeMode === 'light') return `${t('theme.modeTitle')}: ${t('theme.light')}`
  if (themeStore.themeMode === 'dark') return `${t('theme.modeTitle')}: ${t('theme.dark')}`
  return `${t('theme.modeTitle')}: ${t('theme.system')}`
})

const topScanLabel = computed(() => {
  const active = eventsStore.firstActiveScan
  if (!active) return ''
  return t('admin.scanning', { scanned: active.scanned })
})

const userInitials = computed(() => {
  const label = authStore.user?.display_name || authStore.user?.username || 'U'
  return label.slice(0, 1).toUpperCase()
})

async function handleSignOut(): Promise<void> {
  accountMenuOpen.value = false
  playerStore.pause()
  eventsStore.disconnect()
  await authStore.logout()
  await router.replace('/login')
}

function handleOutsideClick(ev: MouseEvent): void {
  const target = ev.target as Node
  if (accountMenuOpen.value && accountMenuRef.value && !accountMenuRef.value.contains(target)) {
    accountMenuOpen.value = false
  }
  if (
    comicSearchFocused.value &&
    !searchWrapRef.value?.contains(target) &&
    !comicSearchDropdownRef.value?.contains(target)
  ) {
    comicSearchFocused.value = false
  }
}

function handleGlobalKeydown(ev: KeyboardEvent): void {
  if ((ev.metaKey || ev.ctrlKey) && ev.key.toLowerCase() === 'k') {
    ev.preventDefault()
    searchInputRef.value?.focus()
    searchInputRef.value?.select()
  } else if ((ev.metaKey || ev.ctrlKey) && ev.key.toLowerCase() === 'b') {
    ev.preventDefault()
    handleSidebarButtonClick()
  } else if (ev.key === 'Escape') {
    mobileMenuOpen.value = false
    accountMenuOpen.value = false
    comicSearchFocused.value = false
    playerStore.lyricsOpen = false
    playerStore.queueOpen = false
  }
}

onMounted(() => {
  document.addEventListener('mousedown', handleOutsideClick)
  window.addEventListener('keydown', handleGlobalKeydown)
  window.addEventListener('resize', updateSearchDropdownPosition)
  eventsStore.connect()
  if (authStore.hasModule('music')) {
    void playerStore.restoreRemoteQueue(false)
  }
})

onBeforeUnmount(() => {
  document.removeEventListener('mousedown', handleOutsideClick)
  window.removeEventListener('keydown', handleGlobalKeydown)
  window.removeEventListener('resize', updateSearchDropdownPosition)
  eventsStore.disconnect()
  if (comicSearchTimer) clearTimeout(comicSearchTimer)
})
</script>

<template>
  <div
    class="relative flex h-screen w-screen flex-col overflow-hidden bg-[var(--ctp-shell)] text-[var(--ctp-text)]"
    @dragenter.prevent="onDragEnter"
    @dragover.prevent
    @dragleave.prevent="onDragLeave"
    @drop.prevent="onDropFiles"
  >
    <!-- Hidden Global Multi-Domain File Picker -->
    <input
      ref="filePickerRef"
      type="file"
      multiple
      :accept="acceptedExtensions"
      class="hidden"
      @change="handlePickedFiles"
    />

    <!-- Full-Window Drag & Drop Overlay -->
    <div
      v-if="dragDepth > 0"
      class="pointer-events-none fixed inset-3 z-50 flex flex-col items-center justify-center rounded-3xl border-2 border-dashed border-[var(--ctp-accent)] bg-[var(--ctp-canvas)]/90 backdrop-blur-md"
    >
      <div
        class="mb-3 flex h-14 w-14 items-center justify-center rounded-2xl bg-[var(--ctp-accent-soft)] text-[var(--ctp-accent)]"
      >
        <LuUpload class="h-7 w-7" />
      </div>
      <p class="text-base font-bold text-[var(--ctp-text)]">{{ t('transfers.dropOverlayTitle') }}</p>
      <p class="mt-1 px-4 text-center text-xs text-[var(--ctp-subtext0)]">
        {{ t('transfers.dropOverlayHint') }}
      </p>
    </div>

    <!-- Full-Width Unified Top Bar (xbook / xcomic / xmusic style) -->
    <header
      class="flex h-16 shrink-0 items-center justify-between gap-3 bg-[var(--ctp-shell)] px-4 select-none sm:px-5"
    >
      <!-- Left: Sidebar Toggle + Rounded Accent Brand Icon + Domain Capsule Switcher -->
      <div class="flex shrink-0 items-center gap-2.5">
        <button
          type="button"
          class="flex h-9 w-9 items-center justify-center rounded-xl text-[var(--ctp-subtext1)] transition hover:bg-[var(--ctp-surface0)]/60 hover:text-[var(--ctp-text)]"
          :title="collapsed ? t('nav.expandSidebar') : t('nav.collapseSidebar')"
          @click="handleSidebarButtonClick"
        >
          <LuPanelLeftOpen v-if="isCompact" class="h-[18px] w-[18px]" />
          <LuPanelLeftClose v-else class="h-[18px] w-[18px]" />
        </button>

        <RouterLink
          :to="`/${domainStore.activeDomain}`"
          class="flex items-center gap-2.5 outline-none transition hover:opacity-90"
          @click="mobileMenuOpen = false"
        >
          <div
            class="flex h-9 w-9 items-center justify-center rounded-xl bg-[var(--ctp-accent)] text-[var(--ctp-on-accent)] shadow-sm"
          >
            <LuLayers class="h-[18px] w-[18px]" />
          </div>
          <span class="hidden text-base font-bold tracking-tight lg:inline">
            {{ t('app.name') }}
          </span>
        </RouterLink>

        <!-- Unified 4-Domain Capsule Switcher -->
        <div
          v-if="domainTabs.length > 1"
          class="ml-1 hidden items-center gap-0.5 rounded-xl border border-[var(--ctp-surface0)]/75 bg-[var(--ctp-mantle)]/70 p-1 md:flex"
        >
          <button
            v-for="tab in domainTabs"
            :key="tab.kind"
            type="button"
            class="flex items-center gap-1.5 rounded-lg px-2.5 py-1.5 text-xs font-semibold transition"
            :class="
              domainStore.activeDomain === tab.kind
                ? 'bg-[var(--ctp-accent)] text-[var(--ctp-on-accent)] shadow-xs'
                : 'text-[var(--ctp-subtext1)] hover:bg-[var(--ctp-surface0)]/60 hover:text-[var(--ctp-text)]'
            "
            @click="switchDomain(tab.kind)"
          >
            <component :is="tab.icon" class="h-3.5 w-3.5 shrink-0" />
            <span>{{ tab.label }}</span>
          </button>
        </div>
      </div>

      <!-- Center: Pill-Shaped Global Search Bar with ⌘K Badge -->
      <form
        ref="searchWrapRef"
        class="relative mx-auto max-w-md flex-1 lg:max-w-lg"
        @submit.prevent="submitGlobalSearch"
      >
        <LuSearch
          class="pointer-events-none absolute top-1/2 left-4 h-4 w-4 -translate-y-1/2 text-[var(--ctp-subtext0)]"
        />
        <input
          ref="searchInputRef"
          v-model="activeSearchValue"
          type="text"
          :placeholder="searchPlaceholder"
          class="h-10 w-full rounded-full border border-[var(--ctp-surface0)]/90 bg-[var(--ctp-mantle)]/70 pr-14 pl-10 text-xs text-[var(--ctp-text)] placeholder-[var(--ctp-subtext0)] outline-none transition focus:border-[var(--ctp-accent)] focus:bg-[var(--ctp-canvas)]"
          @focus="onSearchFocus"
        />
        <kbd
          v-if="!activeSearchValue"
          class="pointer-events-none absolute top-1/2 right-3 hidden -translate-y-1/2 rounded-md border border-[var(--ctp-surface0)] bg-[var(--ctp-shell)]/80 px-1.5 py-0.5 font-mono text-[10px] text-[var(--ctp-subtext0)] sm:inline"
        >
          ⌘K
        </kbd>
        <button
          v-else
          type="button"
          class="absolute top-1/2 right-3 -translate-y-1/2 rounded-full p-0.5 text-[var(--ctp-subtext0)] hover:text-[var(--ctp-text)]"
          @click="clearGlobalSearch"
        >
          <LuX class="h-3.5 w-3.5" />
        </button>
      </form>

      <!-- Right: Scan Indicator + Upload Button + 3-State Theme Button + Avatar Menu -->
      <div class="flex shrink-0 items-center gap-1.5">
        <span
          v-if="eventsStore.anyScanning"
          class="mr-1 hidden items-center gap-1.5 rounded-full bg-[var(--ctp-accent-soft)] px-2.5 py-1 text-xs font-medium text-[var(--ctp-accent)] xl:inline-flex"
        >
          <LuRefreshCw class="h-3.5 w-3.5 animate-spin" />
          <span>{{ topScanLabel }}</span>
        </span>

        <button
          v-if="authStore.isAdmin"
          type="button"
          class="relative flex h-9 w-9 items-center justify-center rounded-xl text-[var(--ctp-subtext1)] transition hover:bg-[var(--ctp-surface0)]/60 hover:text-[var(--ctp-text)]"
          :title="t('common.uploadMedia')"
          @click="triggerUploadPicker"
        >
          <LuUpload class="h-[18px] w-[18px]" />
          <span
            v-if="uploadStore.activeCount > 0"
            class="absolute top-1.5 right-1.5 h-2 w-2 rounded-full bg-[var(--ctp-accent)]"
          />
        </button>

        <button
          type="button"
          class="flex h-9 w-9 items-center justify-center rounded-xl transition hover:bg-[var(--ctp-surface0)]/60 hover:text-[var(--ctp-text)]"
          :class="
            themeStore.themeMode === 'system'
              ? 'bg-[var(--ctp-surface0)]/55 text-[var(--ctp-text)]'
              : 'text-[var(--ctp-subtext1)]'
          "
          :title="themeModeTooltip"
          @click="cycleThemeMode"
        >
          <LuMonitor v-if="themeStore.themeMode === 'system'" class="h-[18px] w-[18px]" />
          <LuSun v-else-if="themeStore.themeMode === 'light'" class="h-[18px] w-[18px]" />
          <LuMoon v-else class="h-[18px] w-[18px]" />
        </button>

        <div ref="accountMenuRef" class="relative ml-1">
          <button
            type="button"
            class="flex h-9 w-9 items-center justify-center rounded-full border border-[var(--ctp-surface0)] bg-[var(--ctp-surface0)]/65 text-xs font-bold text-[var(--ctp-accent)] transition hover:border-[var(--ctp-accent)]"
            @click="accountMenuOpen = !accountMenuOpen"
          >
            {{ userInitials }}
          </button>

          <div
            v-if="accountMenuOpen"
            class="absolute top-11 right-0 z-50 w-56 rounded-2xl border border-[var(--ctp-surface0)] bg-[var(--ctp-canvas)] p-1.5 shadow-2xl"
          >
            <div class="px-3 py-2">
              <p class="truncate text-xs font-semibold text-[var(--ctp-text)]">
                {{ authStore.user?.display_name || authStore.user?.username }}
              </p>
              <p class="truncate text-[11px] text-[var(--ctp-subtext0)]">
                @{{ authStore.user?.username }} ·
                {{ authStore.isAdmin ? t('auth.roleAdmin') : t('auth.roleMember') }}
              </p>
            </div>

            <div class="my-1 h-px bg-[var(--ctp-surface0)]" />

            <RouterLink
              to="/settings"
              class="flex items-center gap-2.5 rounded-xl px-3 py-2 text-xs font-medium text-[var(--ctp-text)] transition hover:bg-[var(--ctp-mantle)]"
              @click="accountMenuOpen = false; mobileMenuOpen = false"
            >
              <LuSettings class="h-4 w-4 text-[var(--ctp-subtext0)]" />
              <span>{{ t('nav.settings') }}</span>
            </RouterLink>

            <button
              type="button"
              class="flex w-full items-center gap-2.5 rounded-xl px-3 py-2 text-left text-xs font-medium text-[var(--ctp-red)] transition hover:bg-[var(--ctp-red)]/10"
              @click="handleSignOut"
            >
              <LuLogOut class="h-4 w-4" />
              <span>{{ t('auth.signOut') }}</span>
            </button>
          </div>
        </div>
      </div>

      <!-- Comic Live Search Dropdown (xcomic style) -->
      <Teleport to="body">
        <div
          v-if="
            domainStore.activeDomain === 'comic' &&
            comicSearchFocused &&
            comicSearchQuery.trim().length > 0
          "
          ref="comicSearchDropdownRef"
          class="fixed z-[9985] max-h-96 overflow-y-auto rounded-2xl border border-[var(--ctp-surface0)] bg-[var(--ctp-canvas)] p-2 shadow-2xl"
          :style="{
            top: searchRect.top + 'px',
            left: searchRect.left + 'px',
            width: searchRect.width + 'px',
          }"
        >
          <div
            v-if="!comicSearchLoading && comicSearchResults.length === 0"
            class="py-6 text-center text-xs text-[var(--ctp-subtext0)]"
          >
            {{ t('common.noResults') }}
          </div>
          <button
            v-for="item in comicSearchResults"
            :key="item.id"
            type="button"
            class="flex w-full items-center gap-3 rounded-xl p-2 text-left transition hover:bg-[var(--ctp-row-hover)]"
            @click="selectComicSearchResult(item.id)"
          >
            <div class="h-12 w-9 shrink-0 overflow-hidden rounded-md bg-[var(--ctp-mantle)]">
              <img
                v-if="item.cover_url"
                :src="withAuthUrl(item.cover_url)"
                :alt="item.title"
                class="h-full w-full object-cover"
              />
              <div
                v-else
                class="flex h-full w-full items-center justify-center text-[var(--ctp-subtext0)]"
              >
                <LuBookOpen class="h-3.5 w-3.5" />
              </div>
            </div>
            <div class="min-w-0 flex-1">
              <div class="truncate text-xs font-semibold text-[var(--ctp-text)]">
                {{ item.title }}
              </div>
              <div class="mt-0.5 truncate text-[11px] text-[var(--ctp-subtext0)]">
                {{
                  [...item.writers, ...item.pencillers].join(' · ') ||
                  comicT('home.chaptersCount', { count: item.chapter_count })
                }}
              </div>
            </div>
            <span
              class="rounded-lg bg-[var(--ctp-mantle)] px-2 py-0.5 text-[11px] text-[var(--ctp-subtext0)]"
            >
              {{ comicT('home.chaptersCount', { count: item.chapter_count }) }}
            </span>
          </button>
        </div>
      </Teleport>
    </header>

    <!-- Middle Body: Collapsible Domain Sidebar + Flush Rounded Main Canvas -->
    <div class="relative flex min-h-0 flex-1 overflow-hidden">
      <div
        v-if="mobileMenuOpen"
        class="fixed inset-0 z-30 bg-black/45 md:hidden"
        @click="mobileMenuOpen = false"
      />

      <aside
        class="fixed top-16 left-0 z-40 flex shrink-0 flex-col border-r border-[var(--ctp-surface0)]/65 bg-[var(--ctp-shell)] transition-all duration-200 select-none md:static md:top-0 md:bottom-0 md:border-r-0"
        :class="[
          showMusicBar ? 'bottom-20' : 'bottom-0',
          mobileMenuOpen ? 'w-64 translate-x-0' : '-translate-x-full md:translate-x-0',
          collapsed ? 'md:w-16' : 'md:w-60',
        ]"
      >
        <!-- Mobile Domain Switcher Grid -->
        <div
          v-if="domainTabs.length > 1"
          class="grid grid-cols-2 gap-1.5 border-b border-[var(--ctp-surface0)]/60 px-3 py-2.5 md:hidden"
        >
          <button
            v-for="tab in domainTabs"
            :key="`mob-${tab.kind}`"
            type="button"
            class="flex items-center justify-center gap-1.5 rounded-xl px-2.5 py-2 text-xs font-semibold transition"
            :class="
              domainStore.activeDomain === tab.kind
                ? 'bg-[var(--ctp-accent)] text-[var(--ctp-on-accent)]'
                : 'bg-[var(--ctp-mantle)] text-[var(--ctp-subtext1)]'
            "
            @click="switchDomain(tab.kind)"
          >
            <component :is="tab.icon" class="h-3.5 w-3.5 shrink-0" />
            <span class="truncate">{{ tab.label }}</span>
          </button>
        </div>

        <nav class="flex-1 space-y-0.5 overflow-y-auto px-3 pt-1 pb-3">
          <!-- ==================== 1. BOOK SIDEBAR ==================== -->
          <template v-if="domainStore.activeDomain === 'book'">
            <RouterLink
              to="/book"
              class="flex items-center gap-3 rounded-xl px-3 py-2 text-sm font-medium transition"
              :class="[
                isCompact ? 'md:justify-center md:px-0' : '',
                route.name === 'book-home'
                  ? 'bg-[var(--ctp-accent-soft)] font-semibold text-[var(--ctp-accent)]'
                  : 'text-[var(--ctp-subtext1)] hover:bg-[var(--ctp-surface0)]/60 hover:text-[var(--ctp-text)]',
              ]"
              :title="isCompact ? bookT('nav.home') : undefined"
              @click="mobileMenuOpen = false"
            >
              <LuHome class="h-4 w-4 shrink-0" />
              <span v-if="!isCompact" class="truncate">{{ bookT('nav.home') }}</span>
            </RouterLink>

            <button
              type="button"
              class="flex w-full items-center gap-3 rounded-xl px-3 py-2 text-sm font-medium transition"
              :class="[
                isCompact ? 'md:justify-center md:px-0' : '',
                isBookSectionActive('all')
                  ? 'bg-[var(--ctp-accent-soft)] font-semibold text-[var(--ctp-accent)]'
                  : 'text-[var(--ctp-subtext1)] hover:bg-[var(--ctp-surface0)]/60 hover:text-[var(--ctp-text)]',
              ]"
              :title="isCompact ? bookT('nav.allBooks') : undefined"
              @click="openBookSection('all')"
            >
              <LuBookOpen class="h-4 w-4 shrink-0" />
              <span v-if="!isCompact" class="truncate">{{ bookT('nav.allBooks') }}</span>
            </button>

            <button
              type="button"
              class="flex w-full items-center gap-3 rounded-xl px-3 py-2 text-sm font-medium transition"
              :class="[
                isCompact ? 'md:justify-center md:px-0' : '',
                isBookSectionActive('recent')
                  ? 'bg-[var(--ctp-accent-soft)] font-semibold text-[var(--ctp-accent)]'
                  : 'text-[var(--ctp-subtext1)] hover:bg-[var(--ctp-surface0)]/60 hover:text-[var(--ctp-text)]',
              ]"
              :title="isCompact ? bookT('nav.recentReading') : undefined"
              @click="openBookSection('recent')"
            >
              <LuClock class="h-4 w-4 shrink-0" />
              <span v-if="!isCompact" class="truncate">{{ bookT('nav.recentReading') }}</span>
            </button>

            <button
              type="button"
              class="flex w-full items-center gap-3 rounded-xl px-3 py-2 text-sm font-medium transition"
              :class="[
                isCompact ? 'md:justify-center md:px-0' : '',
                isBookSectionActive('favorites')
                  ? 'bg-[var(--ctp-accent-soft)] font-semibold text-[var(--ctp-accent)]'
                  : 'text-[var(--ctp-subtext1)] hover:bg-[var(--ctp-surface0)]/60 hover:text-[var(--ctp-text)]',
              ]"
              :title="isCompact ? bookT('nav.favorites') : undefined"
              @click="openBookSection('favorites')"
            >
              <LuHeart class="h-4 w-4 shrink-0" />
              <span v-if="!isCompact" class="truncate">{{ bookT('nav.favorites') }}</span>
            </button>

            <div
              v-if="isCompact"
              class="mt-3 hidden space-y-0.5 border-t border-[var(--ctp-surface0)]/55 pt-3 md:block"
            >
              <RouterLink
                to="/book/libraries"
                class="flex items-center justify-center rounded-xl py-2 transition"
                :class="
                  route.name === 'book-libraries-overview'
                    ? 'bg-[var(--ctp-accent-soft)] text-[var(--ctp-accent)]'
                    : 'text-[var(--ctp-subtext1)] hover:bg-[var(--ctp-surface0)]/60 hover:text-[var(--ctp-text)]'
                "
                :title="bookT('nav.libraries')"
              >
                <LuLibrary class="h-4 w-4 shrink-0" />
              </RouterLink>
              <RouterLink
                to="/book/shelves"
                class="flex items-center justify-center rounded-xl py-2 transition"
                :class="
                  route.name === 'book-shelves-overview'
                    ? 'bg-[var(--ctp-accent-soft)] text-[var(--ctp-accent)]'
                    : 'text-[var(--ctp-subtext1)] hover:bg-[var(--ctp-surface0)]/60 hover:text-[var(--ctp-text)]'
                "
                :title="bookT('nav.shelves')"
              >
                <LuFolder class="h-4 w-4 shrink-0" />
              </RouterLink>
              <RouterLink
                to="/book/tags"
                class="flex items-center justify-center rounded-xl py-2 transition"
                :class="
                  route.name === 'book-tags-overview'
                    ? 'bg-[var(--ctp-accent-soft)] text-[var(--ctp-accent)]'
                    : 'text-[var(--ctp-subtext1)] hover:bg-[var(--ctp-surface0)]/60 hover:text-[var(--ctp-text)]'
                "
                :title="bookT('nav.tags')"
              >
                <LuTag class="h-4 w-4 shrink-0" />
              </RouterLink>
            </div>

            <!-- Book Libraries -->
            <div v-if="!isCompact && bookStore.libraries.length > 0" class="pt-5">
              <div class="mb-1 flex items-center justify-between px-2">
                <div class="flex min-w-0 flex-1 items-center gap-0.5">
                  <button
                    type="button"
                    class="rounded-md p-1 text-[var(--ctp-subtext0)] transition hover:bg-[var(--ctp-surface0)]/60 hover:text-[var(--ctp-text)]"
                    :title="bookT('nav.toggleGroup')"
                    @click="toggleGroupFold('libraries')"
                  >
                    <LuChevronRight v-if="librariesFolded" class="h-3.5 w-3.5" />
                    <LuChevronDown v-else class="h-3.5 w-3.5" />
                  </button>
                  <RouterLink
                    to="/book/libraries"
                    class="truncate rounded-md px-1 py-0.5 text-[11px] font-bold tracking-wider uppercase transition"
                    :class="
                      route.name === 'book-libraries-overview'
                        ? 'text-[var(--ctp-accent)]'
                        : 'text-[var(--ctp-subtext0)] hover:text-[var(--ctp-text)]'
                    "
                    @click="mobileMenuOpen = false"
                  >
                    {{ bookT('nav.libraries') }}
                  </RouterLink>
                </div>
                <span class="px-1.5 text-[11px] font-medium tabular-nums text-[var(--ctp-subtext0)]">
                  {{ bookStore.libraries.length }}
                </span>
              </div>

              <div v-if="!librariesFolded" class="space-y-0.5">
                <button
                  v-for="lib in visibleBookLibraries"
                  :key="lib.id"
                  type="button"
                  class="flex w-full items-center justify-between rounded-xl px-3 py-1.5 text-sm transition"
                  :class="
                    isBookSectionActive('library', lib.id)
                      ? 'bg-[var(--ctp-accent-soft)] font-semibold text-[var(--ctp-accent)]'
                      : 'text-[var(--ctp-subtext1)] hover:bg-[var(--ctp-surface0)]/60 hover:text-[var(--ctp-text)]'
                  "
                  :title="lib.name"
                  @click="openBookSection('library', lib.id)"
                >
                  <span class="flex min-w-0 items-center gap-2.5">
                    <LuLibrary class="h-4 w-4 shrink-0" />
                    <span class="truncate">{{ lib.name }}</span>
                  </span>
                  <span class="ml-2 shrink-0 text-xs tabular-nums text-[var(--ctp-subtext0)]">
                    {{ lib.book_count }}
                  </span>
                </button>

                <button
                  v-if="bookStore.libraries.length > SIDEBAR_PREVIEW_LIMIT"
                  type="button"
                  class="flex w-full items-center gap-1.5 rounded-xl px-3 py-1.5 text-xs font-medium text-[var(--ctp-subtext0)] transition hover:bg-[var(--ctp-surface0)]/50 hover:text-[var(--ctp-text)]"
                  @click="librariesExpanded = !librariesExpanded"
                >
                  <span>
                    {{
                      librariesExpanded
                        ? bookT('nav.showLess')
                        : bookT('nav.showMore', {
                            count: bookStore.libraries.length - SIDEBAR_PREVIEW_LIMIT,
                          })
                    }}
                  </span>
                </button>
              </div>
            </div>

            <!-- Book Shelves -->
            <div v-if="!isCompact" class="pt-5">
              <div class="mb-1 flex items-center justify-between px-2">
                <div class="flex min-w-0 flex-1 items-center gap-0.5">
                  <button
                    type="button"
                    class="rounded-md p-1 text-[var(--ctp-subtext0)] transition hover:bg-[var(--ctp-surface0)]/60 hover:text-[var(--ctp-text)]"
                    :title="bookT('nav.toggleGroup')"
                    @click="toggleGroupFold('collections')"
                  >
                    <LuChevronRight v-if="collectionsFolded" class="h-3.5 w-3.5" />
                    <LuChevronDown v-else class="h-3.5 w-3.5" />
                  </button>
                  <RouterLink
                    to="/book/shelves"
                    class="truncate rounded-md px-1 py-0.5 text-[11px] font-bold tracking-wider uppercase transition"
                    :class="
                      route.name === 'book-shelves-overview'
                        ? 'text-[var(--ctp-accent)]'
                        : 'text-[var(--ctp-subtext0)] hover:text-[var(--ctp-text)]'
                    "
                    @click="mobileMenuOpen = false"
                  >
                    {{ bookT('nav.shelves') }}
                  </RouterLink>
                </div>

                <button
                  type="button"
                  class="rounded-md p-1 text-[var(--ctp-subtext0)] hover:bg-[var(--ctp-surface0)] hover:text-[var(--ctp-text)]"
                  :title="bookT('nav.newShelf')"
                  @click="collectionsFolded = false; creatingCollection = !creatingCollection"
                >
                  <LuFolderPlus class="h-3.5 w-3.5" />
                </button>
              </div>

              <div v-if="!collectionsFolded" class="space-y-0.5">
                <form
                  v-if="creatingCollection"
                  class="mb-2 px-2"
                  @submit.prevent="handleCreateCollection"
                >
                  <input
                    v-model="newCollectionName"
                    type="text"
                    :placeholder="bookT('nav.shelfNamePlaceholder')"
                    class="w-full rounded-xl border border-[var(--ctp-surface0)] bg-[var(--ctp-canvas)] px-2.5 py-1.5 text-xs outline-none focus:border-[var(--ctp-accent)]"
                  />
                </form>

                <div
                  v-for="shelf in visibleBookShelves"
                  :key="shelf.id"
                  class="group flex items-center justify-between rounded-xl px-3 py-1.5 text-sm transition"
                  :class="
                    isBookSectionActive('shelf', shelf.id)
                      ? 'bg-[var(--ctp-accent-soft)] font-semibold text-[var(--ctp-accent)]'
                      : 'text-[var(--ctp-subtext1)] hover:bg-[var(--ctp-surface0)]/60 hover:text-[var(--ctp-text)]'
                  "
                  :title="shelf.name"
                >
                  <button
                    type="button"
                    class="flex min-w-0 flex-1 items-center gap-2.5 text-left"
                    @click="openBookSection('shelf', shelf.id)"
                  >
                    <LuFolder class="h-4 w-4 shrink-0" />
                    <span class="truncate">{{ shelf.name }}</span>
                  </button>
                  <div class="ml-2 flex shrink-0 items-center gap-1">
                    <span class="text-xs tabular-nums text-[var(--ctp-subtext0)]">
                      {{ shelf.book_count }}
                    </span>
                    <button
                      v-if="!shelf.is_favorite"
                      type="button"
                      class="hidden rounded p-0.5 text-[var(--ctp-subtext0)] group-hover:inline-flex hover:text-[var(--ctp-red)]"
                      @click.stop="handleDeleteBookShelf(shelf.id, shelf.name)"
                    >
                      <LuTrash2 class="h-3.5 w-3.5" />
                    </button>
                  </div>
                </div>

                <button
                  v-if="bookStore.shelves.length > SIDEBAR_PREVIEW_LIMIT"
                  type="button"
                  class="flex w-full items-center gap-1.5 rounded-xl px-3 py-1.5 text-xs font-medium text-[var(--ctp-subtext0)] transition hover:bg-[var(--ctp-surface0)]/50 hover:text-[var(--ctp-text)]"
                  @click="collectionsExpanded = !collectionsExpanded"
                >
                  <span>
                    {{
                      collectionsExpanded
                        ? bookT('nav.showLess')
                        : bookT('nav.showMore', {
                            count: bookStore.shelves.length - SIDEBAR_PREVIEW_LIMIT,
                          })
                    }}
                  </span>
                </button>
              </div>
            </div>

            <!-- Book Tags -->
            <div v-if="!isCompact && bookStore.tags.length > 0" class="pt-5">
              <div class="mb-1 flex items-center justify-between px-2">
                <div class="flex min-w-0 flex-1 items-center gap-0.5">
                  <button
                    type="button"
                    class="rounded-md p-1 text-[var(--ctp-subtext0)] transition hover:bg-[var(--ctp-surface0)]/60 hover:text-[var(--ctp-text)]"
                    :title="bookT('nav.toggleGroup')"
                    @click="toggleGroupFold('tags')"
                  >
                    <LuChevronRight v-if="tagsFolded" class="h-3.5 w-3.5" />
                    <LuChevronDown v-else class="h-3.5 w-3.5" />
                  </button>
                  <RouterLink
                    to="/book/tags"
                    class="truncate rounded-md px-1 py-0.5 text-[11px] font-bold tracking-wider uppercase transition"
                    :class="
                      route.name === 'book-tags-overview'
                        ? 'text-[var(--ctp-accent)]'
                        : 'text-[var(--ctp-subtext0)] hover:text-[var(--ctp-text)]'
                    "
                    @click="mobileMenuOpen = false"
                  >
                    {{ bookT('nav.tags') }}
                  </RouterLink>
                </div>
                <span class="px-1.5 text-[11px] font-medium tabular-nums text-[var(--ctp-subtext0)]">
                  {{ bookStore.tags.length }}
                </span>
              </div>

              <div v-if="!tagsFolded" class="space-y-0.5">
                <button
                  v-for="tg in visibleBookTags"
                  :key="tg.name"
                  type="button"
                  class="flex w-full items-center justify-between rounded-xl px-3 py-1.5 text-sm transition"
                  :class="
                    isBookSectionActive('tag', tg.name)
                      ? 'bg-[var(--ctp-accent-soft)] font-semibold text-[var(--ctp-accent)]'
                      : 'text-[var(--ctp-subtext1)] hover:bg-[var(--ctp-surface0)]/60 hover:text-[var(--ctp-text)]'
                  "
                  :title="tg.name"
                  @click="openBookSection('tag', tg.name)"
                >
                  <span class="flex min-w-0 items-center gap-2.5">
                    <LuTag class="h-3.5 w-3.5 shrink-0" />
                    <span class="truncate">{{ tg.name }}</span>
                  </span>
                  <span class="ml-2 shrink-0 text-xs tabular-nums text-[var(--ctp-subtext0)]">
                    {{ tg.count }}
                  </span>
                </button>

                <button
                  v-if="bookStore.tags.length > SIDEBAR_PREVIEW_LIMIT"
                  type="button"
                  class="flex w-full items-center gap-1.5 rounded-xl px-3 py-1.5 text-xs font-medium text-[var(--ctp-subtext0)] transition hover:bg-[var(--ctp-surface0)]/50 hover:text-[var(--ctp-text)]"
                  @click="tagsExpanded = !tagsExpanded"
                >
                  <span>
                    {{
                      tagsExpanded
                        ? bookT('nav.showLess')
                        : bookT('nav.showMore', {
                            count: bookStore.tags.length - SIDEBAR_PREVIEW_LIMIT,
                          })
                    }}
                  </span>
                </button>
              </div>
            </div>
          </template>

          <!-- ==================== 2. COMIC SIDEBAR ==================== -->
          <template v-else-if="domainStore.activeDomain === 'comic'">
            <RouterLink
              to="/comic"
              class="flex items-center gap-3 rounded-xl px-3 py-2 text-sm font-medium transition"
              :class="[
                isCompact ? 'md:justify-center md:px-0' : '',
                route.name === 'comic-home'
                  ? 'bg-[var(--ctp-accent-soft)] font-semibold text-[var(--ctp-accent)]'
                  : 'text-[var(--ctp-subtext1)] hover:bg-[var(--ctp-surface0)]/60 hover:text-[var(--ctp-text)]',
              ]"
              :title="isCompact ? comicT('nav.home') : undefined"
              @click="mobileMenuOpen = false"
            >
              <LuHome class="h-4 w-4 shrink-0" />
              <span v-if="!isCompact" class="truncate">{{ comicT('nav.home') }}</span>
            </RouterLink>

            <button
              type="button"
              class="flex w-full items-center gap-3 rounded-xl px-3 py-2 text-sm font-medium transition"
              :class="[
                isCompact ? 'md:justify-center md:px-0' : '',
                isAllComicSeriesActive
                  ? 'bg-[var(--ctp-accent-soft)] font-semibold text-[var(--ctp-accent)]'
                  : 'text-[var(--ctp-subtext1)] hover:bg-[var(--ctp-surface0)]/60 hover:text-[var(--ctp-text)]',
              ]"
              :title="isCompact ? comicT('nav.allSeries') : undefined"
              @click="mobileMenuOpen = false; router.push({ path: '/comic/series', query: {} })"
            >
              <LuLayers class="h-4 w-4 shrink-0" />
              <span v-if="!isCompact" class="truncate">{{ comicT('nav.allSeries') }}</span>
            </button>

            <RouterLink
              to="/comic/continue"
              class="flex items-center gap-3 rounded-xl px-3 py-2 text-sm font-medium transition"
              :class="[
                isCompact ? 'md:justify-center md:px-0' : '',
                route.name === 'comic-continue'
                  ? 'bg-[var(--ctp-accent-soft)] font-semibold text-[var(--ctp-accent)]'
                  : 'text-[var(--ctp-subtext1)] hover:bg-[var(--ctp-surface0)]/60 hover:text-[var(--ctp-text)]',
              ]"
              :title="isCompact ? comicT('nav.continueReading') : undefined"
              @click="mobileMenuOpen = false"
            >
              <LuClock class="h-4 w-4 shrink-0" />
              <span v-if="!isCompact" class="truncate">{{ comicT('nav.continueReading') }}</span>
            </RouterLink>

            <RouterLink
              to="/comic/favorites"
              class="flex items-center gap-3 rounded-xl px-3 py-2 text-sm font-medium transition"
              :class="[
                isCompact ? 'md:justify-center md:px-0' : '',
                route.name === 'comic-favorites'
                  ? 'bg-[var(--ctp-accent-soft)] font-semibold text-[var(--ctp-accent)]'
                  : 'text-[var(--ctp-subtext1)] hover:bg-[var(--ctp-surface0)]/60 hover:text-[var(--ctp-text)]',
              ]"
              :title="isCompact ? comicT('nav.favorites') : undefined"
              @click="mobileMenuOpen = false"
            >
              <LuHeart class="h-4 w-4 shrink-0" />
              <span v-if="!isCompact" class="truncate">{{ comicT('nav.favorites') }}</span>
            </RouterLink>

            <div
              v-if="isCompact"
              class="mt-3 hidden space-y-0.5 border-t border-[var(--ctp-surface0)]/55 pt-3 md:block"
            >
              <RouterLink
                to="/comic/libraries"
                class="flex items-center justify-center rounded-xl py-2 transition"
                :class="
                  route.name === 'comic-libraries'
                    ? 'bg-[var(--ctp-accent-soft)] text-[var(--ctp-accent)]'
                    : 'text-[var(--ctp-subtext1)] hover:bg-[var(--ctp-surface0)]/60 hover:text-[var(--ctp-text)]'
                "
                :title="comicT('nav.libraries')"
              >
                <LuLibrary class="h-4 w-4 shrink-0" />
              </RouterLink>
              <RouterLink
                to="/comic/shelves"
                class="flex items-center justify-center rounded-xl py-2 transition"
                :class="
                  route.name === 'comic-shelves'
                    ? 'bg-[var(--ctp-accent-soft)] text-[var(--ctp-accent)]'
                    : 'text-[var(--ctp-subtext1)] hover:bg-[var(--ctp-surface0)]/60 hover:text-[var(--ctp-text)]'
                "
                :title="comicT('nav.shelves')"
              >
                <LuFolder class="h-4 w-4 shrink-0" />
              </RouterLink>
              <RouterLink
                to="/comic/tags"
                class="flex items-center justify-center rounded-xl py-2 transition"
                :class="
                  route.name === 'comic-tags'
                    ? 'bg-[var(--ctp-accent-soft)] text-[var(--ctp-accent)]'
                    : 'text-[var(--ctp-subtext1)] hover:bg-[var(--ctp-surface0)]/60 hover:text-[var(--ctp-text)]'
                "
                :title="comicT('nav.tags')"
              >
                <LuTag class="h-4 w-4 shrink-0" />
              </RouterLink>
            </div>

            <!-- Comic Libraries -->
            <div v-if="!isCompact" class="pt-5">
              <div class="mb-1 flex items-center justify-between px-2">
                <div class="flex min-w-0 flex-1 items-center gap-0.5">
                  <button
                    type="button"
                    class="rounded-md p-1 text-[var(--ctp-subtext0)] transition hover:bg-[var(--ctp-surface0)]/60 hover:text-[var(--ctp-text)]"
                    :title="comicT('nav.toggleGroup')"
                    @click="toggleGroupFold('libraries')"
                  >
                    <LuChevronRight v-if="librariesFolded" class="h-3.5 w-3.5" />
                    <LuChevronDown v-else class="h-3.5 w-3.5" />
                  </button>
                  <RouterLink
                    to="/comic/libraries"
                    class="truncate rounded-md px-1 py-0.5 text-[11px] font-bold tracking-wider uppercase transition"
                    :class="
                      route.name === 'comic-libraries'
                        ? 'text-[var(--ctp-accent)]'
                        : 'text-[var(--ctp-subtext0)] hover:text-[var(--ctp-text)]'
                    "
                    @click="mobileMenuOpen = false"
                  >
                    {{ comicT('nav.libraries') }}
                  </RouterLink>
                </div>
                <span class="px-1.5 text-[11px] font-medium tabular-nums text-[var(--ctp-subtext0)]">
                  {{ comicSidebarStore.libraries.length }}
                </span>
              </div>

              <div v-if="!librariesFolded && comicSidebarStore.libraries.length > 0" class="space-y-0.5">
                <button
                  v-for="lib in visibleComicLibraries"
                  :key="lib.id"
                  type="button"
                  class="flex w-full items-center justify-between rounded-xl px-3 py-1.5 text-sm transition"
                  :class="
                    activeComicLibraryId === lib.id
                      ? 'bg-[var(--ctp-accent-soft)] font-semibold text-[var(--ctp-accent)]'
                      : 'text-[var(--ctp-subtext1)] hover:bg-[var(--ctp-surface0)]/60 hover:text-[var(--ctp-text)]'
                  "
                  :title="lib.name"
                  @click="mobileMenuOpen = false; router.push({ path: '/comic/series', query: { library_id: lib.id } })"
                >
                  <span class="flex min-w-0 items-center gap-2.5">
                    <LuLibrary class="h-4 w-4 shrink-0" />
                    <span class="truncate">{{ lib.name }}</span>
                  </span>
                  <span class="ml-2 shrink-0 text-xs tabular-nums text-[var(--ctp-subtext0)]">
                    {{ resolveLibrarySeriesCount(lib) }}
                  </span>
                </button>

                <button
                  v-if="comicSidebarStore.libraries.length > SIDEBAR_PREVIEW_LIMIT"
                  type="button"
                  class="flex w-full items-center gap-1.5 rounded-xl px-3 py-1.5 text-xs font-medium text-[var(--ctp-subtext0)] transition hover:bg-[var(--ctp-surface0)]/50 hover:text-[var(--ctp-text)]"
                  @click="librariesExpanded = !librariesExpanded"
                >
                  <span>
                    {{
                      librariesExpanded
                        ? comicT('nav.showLess')
                        : comicT('nav.showMore', {
                            count: comicSidebarStore.libraries.length - SIDEBAR_PREVIEW_LIMIT,
                          })
                    }}
                  </span>
                </button>
              </div>
            </div>

            <!-- Comic Shelves -->
            <div v-if="!isCompact" class="pt-5">
              <div class="mb-1 flex items-center justify-between px-2">
                <div class="flex min-w-0 flex-1 items-center gap-0.5">
                  <button
                    type="button"
                    class="rounded-md p-1 text-[var(--ctp-subtext0)] transition hover:bg-[var(--ctp-surface0)]/60 hover:text-[var(--ctp-text)]"
                    :title="comicT('nav.toggleGroup')"
                    @click="toggleGroupFold('collections')"
                  >
                    <LuChevronRight v-if="collectionsFolded" class="h-3.5 w-3.5" />
                    <LuChevronDown v-else class="h-3.5 w-3.5" />
                  </button>
                  <RouterLink
                    to="/comic/shelves"
                    class="truncate rounded-md px-1 py-0.5 text-[11px] font-bold tracking-wider uppercase transition"
                    :class="
                      route.name === 'comic-shelves'
                        ? 'text-[var(--ctp-accent)]'
                        : 'text-[var(--ctp-subtext0)] hover:text-[var(--ctp-text)]'
                    "
                    @click="mobileMenuOpen = false"
                  >
                    {{ comicT('nav.shelves') }}
                  </RouterLink>
                </div>

                <button
                  type="button"
                  class="rounded-md p-1 text-[var(--ctp-subtext0)] hover:bg-[var(--ctp-surface0)] hover:text-[var(--ctp-text)]"
                  :title="comicT('nav.newShelf')"
                  @click="collectionsFolded = false; creatingCollection = !creatingCollection"
                >
                  <LuFolderPlus class="h-3.5 w-3.5" />
                </button>
              </div>

              <div v-if="!collectionsFolded" class="space-y-0.5">
                <form
                  v-if="creatingCollection"
                  class="mb-2 px-2"
                  @submit.prevent="handleCreateCollection"
                >
                  <input
                    v-model="newCollectionName"
                    type="text"
                    :placeholder="comicT('nav.shelfNamePlaceholder')"
                    class="w-full rounded-xl border border-[var(--ctp-surface0)] bg-[var(--ctp-canvas)] px-2.5 py-1.5 text-xs outline-none focus:border-[var(--ctp-accent)]"
                  />
                </form>

                <div
                  v-for="item in visibleComicShelves"
                  :key="item.shelf.id"
                  class="group flex items-center justify-between rounded-xl px-3 py-1.5 text-sm transition"
                  :class="
                    activeComicShelfId === item.shelf.id
                      ? 'bg-[var(--ctp-accent-soft)] font-semibold text-[var(--ctp-accent)]'
                      : 'text-[var(--ctp-subtext1)] hover:bg-[var(--ctp-surface0)]/60 hover:text-[var(--ctp-text)]'
                  "
                  :title="item.shelf.name"
                >
                  <button
                    type="button"
                    class="flex min-w-0 flex-1 items-center gap-2.5 text-left"
                    @click="mobileMenuOpen = false; router.push({ path: '/comic/series', query: { shelf_id: item.shelf.id } })"
                  >
                    <LuFolder class="h-4 w-4 shrink-0" />
                    <span class="truncate">{{ item.shelf.name }}</span>
                  </button>
                  <div class="ml-2 flex shrink-0 items-center gap-1">
                    <span class="text-xs tabular-nums text-[var(--ctp-subtext0)]">
                      {{ item.series.length }}
                    </span>
                    <button
                      type="button"
                      class="hidden rounded p-0.5 text-[var(--ctp-subtext0)] group-hover:inline-flex hover:text-[var(--ctp-red)]"
                      @click.stop="handleDeleteComicShelf(item.shelf.id, item.shelf.name)"
                    >
                      <LuTrash2 class="h-3.5 w-3.5" />
                    </button>
                  </div>
                </div>

                <button
                  v-if="comicSidebarStore.shelves.length > SIDEBAR_PREVIEW_LIMIT"
                  type="button"
                  class="flex w-full items-center gap-1.5 rounded-xl px-3 py-1.5 text-xs font-medium text-[var(--ctp-subtext0)] transition hover:bg-[var(--ctp-surface0)]/50 hover:text-[var(--ctp-text)]"
                  @click="collectionsExpanded = !collectionsExpanded"
                >
                  <span>
                    {{
                      collectionsExpanded
                        ? comicT('nav.showLess')
                        : comicT('nav.showMore', {
                            count: comicSidebarStore.shelves.length - SIDEBAR_PREVIEW_LIMIT,
                          })
                    }}
                  </span>
                </button>
              </div>
            </div>

            <!-- Comic Tags -->
            <div v-if="!isCompact && comicSidebarStore.tags.length > 0" class="pt-5">
              <div class="mb-1 flex items-center justify-between px-2">
                <div class="flex min-w-0 flex-1 items-center gap-0.5">
                  <button
                    type="button"
                    class="rounded-md p-1 text-[var(--ctp-subtext0)] transition hover:bg-[var(--ctp-surface0)]/60 hover:text-[var(--ctp-text)]"
                    :title="comicT('nav.toggleGroup')"
                    @click="toggleGroupFold('tags')"
                  >
                    <LuChevronRight v-if="tagsFolded" class="h-3.5 w-3.5" />
                    <LuChevronDown v-else class="h-3.5 w-3.5" />
                  </button>
                  <RouterLink
                    to="/comic/tags"
                    class="truncate rounded-md px-1 py-0.5 text-[11px] font-bold tracking-wider uppercase transition"
                    :class="
                      route.name === 'comic-tags'
                        ? 'text-[var(--ctp-accent)]'
                        : 'text-[var(--ctp-subtext0)] hover:text-[var(--ctp-text)]'
                    "
                    @click="mobileMenuOpen = false"
                  >
                    {{ comicT('nav.tags') }}
                  </RouterLink>
                </div>
                <span class="px-1.5 text-[11px] font-medium tabular-nums text-[var(--ctp-subtext0)]">
                  {{ comicSidebarStore.tags.length }}
                </span>
              </div>

              <div v-if="!tagsFolded" class="space-y-0.5">
                <button
                  v-for="tg in visibleComicTags"
                  :key="tg.name"
                  type="button"
                  class="flex w-full items-center justify-between rounded-xl px-3 py-1.5 text-sm transition"
                  :class="
                    activeComicTag === tg.name
                      ? 'bg-[var(--ctp-accent-soft)] font-semibold text-[var(--ctp-accent)]'
                      : 'text-[var(--ctp-subtext1)] hover:bg-[var(--ctp-surface0)]/60 hover:text-[var(--ctp-text)]'
                  "
                  :title="tg.name"
                  @click="mobileMenuOpen = false; router.push({ path: '/comic/series', query: { tag: tg.name } })"
                >
                  <span class="flex min-w-0 items-center gap-2.5">
                    <LuTag class="h-3.5 w-3.5 shrink-0" />
                    <span class="truncate">{{ tg.name }}</span>
                  </span>
                  <span class="ml-2 shrink-0 text-xs tabular-nums text-[var(--ctp-subtext0)]">
                    {{ tg.count }}
                  </span>
                </button>

                <button
                  v-if="comicSidebarStore.tags.length > SIDEBAR_PREVIEW_LIMIT"
                  type="button"
                  class="flex w-full items-center gap-1.5 rounded-xl px-3 py-1.5 text-xs font-medium text-[var(--ctp-subtext0)] transition hover:bg-[var(--ctp-surface0)]/50 hover:text-[var(--ctp-text)]"
                  @click="tagsExpanded = !tagsExpanded"
                >
                  <span>
                    {{
                      tagsExpanded
                        ? comicT('nav.showLess')
                        : comicT('nav.showMore', {
                            count: comicSidebarStore.tags.length - SIDEBAR_PREVIEW_LIMIT,
                          })
                    }}
                  </span>
                </button>
              </div>
            </div>
          </template>

          <!-- ==================== 3. MUSIC SIDEBAR ==================== -->
          <template v-else-if="domainStore.activeDomain === 'music'">
            <RouterLink
              to="/music"
              class="flex items-center gap-3 rounded-xl px-3 py-2 text-sm font-medium transition"
              :class="[
                isCompact ? 'md:justify-center md:px-0' : '',
                route.name === 'music-home'
                  ? 'bg-[var(--ctp-accent-soft)] font-semibold text-[var(--ctp-accent)]'
                  : 'text-[var(--ctp-subtext1)] hover:bg-[var(--ctp-surface0)]/60 hover:text-[var(--ctp-text)]',
              ]"
              :title="isCompact ? musicT('nav.home') : undefined"
              @click="mobileMenuOpen = false"
            >
              <LuHome class="h-4 w-4 shrink-0" />
              <span v-if="!isCompact" class="truncate">{{ musicT('nav.home') }}</span>
            </RouterLink>

            <button
              type="button"
              class="flex w-full items-center gap-3 rounded-xl px-3 py-2 text-sm font-medium transition"
              :class="[
                isCompact ? 'md:justify-center md:px-0' : '',
                isAllMusicActive
                  ? 'bg-[var(--ctp-accent-soft)] font-semibold text-[var(--ctp-accent)]'
                  : 'text-[var(--ctp-subtext1)] hover:bg-[var(--ctp-surface0)]/60 hover:text-[var(--ctp-text)]',
              ]"
              :title="isCompact ? musicT('nav.all') : undefined"
              @click="openMusicCatalog()"
            >
              <LuMusic class="h-4 w-4 shrink-0" />
              <span v-if="!isCompact" class="truncate">{{ musicT('nav.all') }}</span>
            </button>

            <RouterLink
              to="/music/continue"
              class="flex items-center gap-3 rounded-xl px-3 py-2 text-sm font-medium transition"
              :class="[
                isCompact ? 'md:justify-center md:px-0' : '',
                route.name === 'music-continue'
                  ? 'bg-[var(--ctp-accent-soft)] font-semibold text-[var(--ctp-accent)]'
                  : 'text-[var(--ctp-subtext1)] hover:bg-[var(--ctp-surface0)]/60 hover:text-[var(--ctp-text)]',
              ]"
              :title="isCompact ? musicT('nav.continueListening') : undefined"
              @click="mobileMenuOpen = false"
            >
              <LuClock class="h-4 w-4 shrink-0" />
              <span v-if="!isCompact" class="truncate">{{ musicT('nav.continueListening') }}</span>
            </RouterLink>

            <RouterLink
              to="/music/favorites"
              class="flex items-center gap-3 rounded-xl px-3 py-2 text-sm font-medium transition"
              :class="[
                isCompact ? 'md:justify-center md:px-0' : '',
                route.name === 'music-favorites'
                  ? 'bg-[var(--ctp-accent-soft)] font-semibold text-[var(--ctp-accent)]'
                  : 'text-[var(--ctp-subtext1)] hover:bg-[var(--ctp-surface0)]/60 hover:text-[var(--ctp-text)]',
              ]"
              :title="isCompact ? musicT('nav.favorites') : undefined"
              @click="mobileMenuOpen = false"
            >
              <LuHeart class="h-4 w-4 shrink-0" />
              <span v-if="!isCompact" class="truncate">{{ musicT('nav.favorites') }}</span>
            </RouterLink>

            <div
              v-if="isCompact"
              class="mt-3 hidden space-y-0.5 border-t border-[var(--ctp-surface0)]/55 pt-3 md:block"
            >
              <RouterLink
                to="/music/libraries"
                class="flex items-center justify-center rounded-xl py-2 transition"
                :class="
                  route.name === 'music-libraries-overview'
                    ? 'bg-[var(--ctp-accent-soft)] text-[var(--ctp-accent)]'
                    : 'text-[var(--ctp-subtext1)] hover:bg-[var(--ctp-surface0)]/60 hover:text-[var(--ctp-text)]'
                "
                :title="musicT('nav.libraries')"
              >
                <LuLibrary class="h-4 w-4 shrink-0" />
              </RouterLink>
              <RouterLink
                to="/music/playlists"
                class="flex items-center justify-center rounded-xl py-2 transition"
                :class="
                  route.name === 'music-playlists-overview'
                    ? 'bg-[var(--ctp-accent-soft)] text-[var(--ctp-accent)]'
                    : 'text-[var(--ctp-subtext1)] hover:bg-[var(--ctp-surface0)]/60 hover:text-[var(--ctp-text)]'
                "
                :title="musicT('nav.playlists')"
              >
                <LuListMusic class="h-4 w-4 shrink-0" />
              </RouterLink>
              <RouterLink
                to="/music/genres"
                class="flex items-center justify-center rounded-xl py-2 transition"
                :class="
                  route.name === 'music-genres-overview'
                    ? 'bg-[var(--ctp-accent-soft)] text-[var(--ctp-accent)]'
                    : 'text-[var(--ctp-subtext1)] hover:bg-[var(--ctp-surface0)]/60 hover:text-[var(--ctp-text)]'
                "
                :title="musicT('nav.genres')"
              >
                <LuTag class="h-4 w-4 shrink-0" />
              </RouterLink>
            </div>

            <!-- Music Libraries -->
            <div v-if="!isCompact && musicStore.libraries.length > 0" class="pt-5">
              <div class="mb-1 flex items-center justify-between px-2">
                <div class="flex min-w-0 flex-1 items-center gap-0.5">
                  <button
                    type="button"
                    class="rounded-md p-1 text-[var(--ctp-subtext0)] transition hover:bg-[var(--ctp-surface0)]/60 hover:text-[var(--ctp-text)]"
                    :title="musicT('nav.toggleGroup')"
                    @click="toggleGroupFold('libraries')"
                  >
                    <LuChevronRight v-if="librariesFolded" class="h-3.5 w-3.5" />
                    <LuChevronDown v-else class="h-3.5 w-3.5" />
                  </button>
                  <RouterLink
                    to="/music/libraries"
                    class="truncate rounded-md px-1 py-0.5 text-[11px] font-bold tracking-wider uppercase transition"
                    :class="
                      route.name === 'music-libraries-overview'
                        ? 'text-[var(--ctp-accent)]'
                        : 'text-[var(--ctp-subtext0)] hover:text-[var(--ctp-text)]'
                    "
                    @click="mobileMenuOpen = false"
                  >
                    {{ musicT('nav.libraries') }}
                  </RouterLink>
                </div>
                <span class="px-1.5 text-[11px] font-medium tabular-nums text-[var(--ctp-subtext0)]">
                  {{ musicStore.libraries.length }}
                </span>
              </div>

              <div v-if="!librariesFolded" class="space-y-0.5">
                <button
                  v-for="lib in visibleMusicLibraries"
                  :key="lib.id"
                  type="button"
                  class="flex w-full items-center justify-between rounded-xl px-3 py-1.5 text-sm transition"
                  :class="
                    isMusicLibraryActive(lib.id)
                      ? 'bg-[var(--ctp-accent-soft)] font-semibold text-[var(--ctp-accent)]'
                      : 'text-[var(--ctp-subtext1)] hover:bg-[var(--ctp-surface0)]/60 hover:text-[var(--ctp-text)]'
                  "
                  :title="lib.name"
                  @click="openMusicCatalog({ library: lib.id })"
                >
                  <span class="flex min-w-0 items-center gap-2.5">
                    <LuLibrary class="h-4 w-4 shrink-0" />
                    <span class="truncate">{{ lib.name }}</span>
                  </span>
                  <span class="ml-2 shrink-0 text-xs tabular-nums text-[var(--ctp-subtext0)]">
                    {{ lib.track_count }}
                  </span>
                </button>

                <button
                  v-if="musicStore.libraries.length > SIDEBAR_PREVIEW_LIMIT"
                  type="button"
                  class="flex w-full items-center gap-1.5 rounded-xl px-3 py-1.5 text-xs font-medium text-[var(--ctp-subtext0)] transition hover:bg-[var(--ctp-surface0)]/50 hover:text-[var(--ctp-text)]"
                  @click="librariesExpanded = !librariesExpanded"
                >
                  <span>
                    {{
                      librariesExpanded
                        ? musicT('nav.showLess')
                        : musicT('nav.showMore', {
                            count: musicStore.libraries.length - SIDEBAR_PREVIEW_LIMIT,
                          })
                    }}
                  </span>
                </button>
              </div>
            </div>

            <!-- Music Playlists -->
            <div v-if="!isCompact" class="pt-5">
              <div class="mb-1 flex items-center justify-between px-2">
                <div class="flex min-w-0 flex-1 items-center gap-0.5">
                  <button
                    type="button"
                    class="rounded-md p-1 text-[var(--ctp-subtext0)] transition hover:bg-[var(--ctp-surface0)]/60 hover:text-[var(--ctp-text)]"
                    :title="musicT('nav.toggleGroup')"
                    @click="toggleGroupFold('collections')"
                  >
                    <LuChevronRight v-if="collectionsFolded" class="h-3.5 w-3.5" />
                    <LuChevronDown v-else class="h-3.5 w-3.5" />
                  </button>
                  <RouterLink
                    to="/music/playlists"
                    class="truncate rounded-md px-1 py-0.5 text-[11px] font-bold tracking-wider uppercase transition"
                    :class="
                      route.name === 'music-playlists-overview'
                        ? 'text-[var(--ctp-accent)]'
                        : 'text-[var(--ctp-subtext0)] hover:text-[var(--ctp-text)]'
                    "
                    @click="mobileMenuOpen = false"
                  >
                    {{ musicT('nav.playlists') }}
                  </RouterLink>
                </div>

                <button
                  type="button"
                  class="rounded-md p-1 text-[var(--ctp-subtext0)] hover:bg-[var(--ctp-surface0)] hover:text-[var(--ctp-text)]"
                  :title="musicT('nav.newPlaylist')"
                  @click="collectionsFolded = false; creatingCollection = !creatingCollection"
                >
                  <LuFolderPlus class="h-3.5 w-3.5" />
                </button>
              </div>

              <div v-if="!collectionsFolded" class="space-y-0.5">
                <form
                  v-if="creatingCollection"
                  class="mb-2 px-2"
                  @submit.prevent="handleCreateCollection"
                >
                  <input
                    v-model="newCollectionName"
                    type="text"
                    :placeholder="musicT('nav.playlistNamePlaceholder')"
                    class="w-full rounded-xl border border-[var(--ctp-surface0)] bg-[var(--ctp-canvas)] px-2.5 py-1.5 text-xs outline-none focus:border-[var(--ctp-accent)]"
                  />
                </form>

                <div
                  v-for="pl in visibleMusicPlaylists"
                  :key="pl.id"
                  class="group flex items-center justify-between rounded-xl px-3 py-1.5 text-sm transition"
                  :class="
                    route.path === `/music/playlists/${pl.id}`
                      ? 'bg-[var(--ctp-accent-soft)] font-semibold text-[var(--ctp-accent)]'
                      : 'text-[var(--ctp-subtext1)] hover:bg-[var(--ctp-surface0)]/60 hover:text-[var(--ctp-text)]'
                  "
                  :title="pl.name"
                >
                  <RouterLink
                    :to="`/music/playlists/${pl.id}`"
                    class="flex min-w-0 flex-1 items-center gap-2.5"
                    @click="mobileMenuOpen = false"
                  >
                    <LuListMusic class="h-4 w-4 shrink-0" />
                    <span class="truncate">{{ pl.name }}</span>
                  </RouterLink>

                  <div class="ml-2 flex shrink-0 items-center gap-1">
                    <span class="text-xs tabular-nums text-[var(--ctp-subtext0)]">
                      {{ pl.track_count }}
                    </span>
                    <button
                      v-if="canEditPlaylist(pl)"
                      type="button"
                      class="hidden rounded p-0.5 text-[var(--ctp-subtext0)] group-hover:inline-flex hover:text-[var(--ctp-red)]"
                      @click.stop="handleDeleteMusicPlaylist(pl)"
                    >
                      <LuTrash2 class="h-3.5 w-3.5" />
                    </button>
                  </div>
                </div>

                <button
                  v-if="musicStore.playlists.length > SIDEBAR_PREVIEW_LIMIT"
                  type="button"
                  class="flex w-full items-center gap-1.5 rounded-xl px-3 py-1.5 text-xs font-medium text-[var(--ctp-subtext0)] transition hover:bg-[var(--ctp-surface0)]/50 hover:text-[var(--ctp-text)]"
                  @click="collectionsExpanded = !collectionsExpanded"
                >
                  <span>
                    {{
                      collectionsExpanded
                        ? musicT('nav.showLess')
                        : musicT('nav.showMore', {
                            count: musicStore.playlists.length - SIDEBAR_PREVIEW_LIMIT,
                          })
                    }}
                  </span>
                </button>
              </div>
            </div>

            <!-- Music Genres -->
            <div v-if="!isCompact && musicStore.genres.length > 0" class="pt-5">
              <div class="mb-1 flex items-center justify-between px-2">
                <div class="flex min-w-0 flex-1 items-center gap-0.5">
                  <button
                    type="button"
                    class="rounded-md p-1 text-[var(--ctp-subtext0)] transition hover:bg-[var(--ctp-surface0)]/60 hover:text-[var(--ctp-text)]"
                    :title="musicT('nav.toggleGroup')"
                    @click="toggleGroupFold('tags')"
                  >
                    <LuChevronRight v-if="tagsFolded" class="h-3.5 w-3.5" />
                    <LuChevronDown v-else class="h-3.5 w-3.5" />
                  </button>
                  <RouterLink
                    to="/music/genres"
                    class="truncate rounded-md px-1 py-0.5 text-[11px] font-bold tracking-wider uppercase transition"
                    :class="
                      route.name === 'music-genres-overview'
                        ? 'text-[var(--ctp-accent)]'
                        : 'text-[var(--ctp-subtext0)] hover:text-[var(--ctp-text)]'
                    "
                    @click="mobileMenuOpen = false"
                  >
                    {{ musicT('nav.genres') }}
                  </RouterLink>
                </div>
                <span class="px-1.5 text-[11px] font-medium tabular-nums text-[var(--ctp-subtext0)]">
                  {{ musicStore.genres.length }}
                </span>
              </div>

              <div v-if="!tagsFolded" class="space-y-0.5">
                <button
                  v-for="g in visibleMusicGenres"
                  :key="g.name"
                  type="button"
                  class="flex w-full items-center justify-between rounded-xl px-3 py-1.5 text-sm transition"
                  :class="
                    isMusicGenreActive(g.name)
                      ? 'bg-[var(--ctp-accent-soft)] font-semibold text-[var(--ctp-accent)]'
                      : 'text-[var(--ctp-subtext1)] hover:bg-[var(--ctp-surface0)]/60 hover:text-[var(--ctp-text)]'
                  "
                  :title="g.name"
                  @click="openMusicCatalog({ genre: g.name })"
                >
                  <span class="flex min-w-0 items-center gap-2.5">
                    <LuTag class="h-3.5 w-3.5 shrink-0" />
                    <span class="truncate">{{ g.name }}</span>
                  </span>
                  <span class="ml-2 shrink-0 text-xs tabular-nums text-[var(--ctp-subtext0)]">
                    {{ g.song_count }}
                  </span>
                </button>

                <button
                  v-if="musicStore.genres.length > SIDEBAR_PREVIEW_LIMIT"
                  type="button"
                  class="flex w-full items-center gap-1.5 rounded-xl px-3 py-1.5 text-xs font-medium text-[var(--ctp-subtext0)] transition hover:bg-[var(--ctp-surface0)]/50 hover:text-[var(--ctp-text)]"
                  @click="tagsExpanded = !tagsExpanded"
                >
                  <span>
                    {{
                      tagsExpanded
                        ? musicT('nav.showLess')
                        : musicT('nav.showMore', {
                            count: musicStore.genres.length - SIDEBAR_PREVIEW_LIMIT,
                          })
                    }}
                  </span>
                </button>
              </div>
            </div>
          </template>

          <!-- ==================== 4. VIDEO SIDEBAR ==================== -->
          <template v-else>
            <RouterLink
              to="/video"
              class="flex items-center gap-3 rounded-xl px-3 py-2 text-sm font-medium transition"
              :class="[
                isCompact ? 'md:justify-center md:px-0' : '',
                route.name === 'video-home'
                  ? 'bg-[var(--ctp-accent-soft)] font-semibold text-[var(--ctp-accent)]'
                  : 'text-[var(--ctp-subtext1)] hover:bg-[var(--ctp-surface0)]/60 hover:text-[var(--ctp-text)]',
              ]"
              :title="isCompact ? videoT('nav.home') : undefined"
              @click="mobileMenuOpen = false"
            >
              <LuHome class="h-4 w-4 shrink-0" />
              <span v-if="!isCompact" class="truncate">{{ videoT('nav.home') }}</span>
            </RouterLink>

            <button
              type="button"
              class="flex w-full items-center gap-3 rounded-xl px-3 py-2 text-sm font-medium transition"
              :class="[
                isCompact ? 'md:justify-center md:px-0' : '',
                isAllVideoActive
                  ? 'bg-[var(--ctp-accent-soft)] font-semibold text-[var(--ctp-accent)]'
                  : 'text-[var(--ctp-subtext1)] hover:bg-[var(--ctp-surface0)]/60 hover:text-[var(--ctp-text)]',
              ]"
              :title="isCompact ? videoT('nav.all') : undefined"
              @click="openVideoCatalog()"
            >
              <LuFilm class="h-4 w-4 shrink-0" />
              <span v-if="!isCompact" class="truncate">{{ videoT('nav.all') }}</span>
            </button>

            <RouterLink
              to="/video/continue"
              class="flex items-center gap-3 rounded-xl px-3 py-2 text-sm font-medium transition"
              :class="[
                isCompact ? 'md:justify-center md:px-0' : '',
                route.name === 'video-continue'
                  ? 'bg-[var(--ctp-accent-soft)] font-semibold text-[var(--ctp-accent)]'
                  : 'text-[var(--ctp-subtext1)] hover:bg-[var(--ctp-surface0)]/60 hover:text-[var(--ctp-text)]',
              ]"
              :title="isCompact ? videoT('nav.continueWatching') : undefined"
              @click="mobileMenuOpen = false"
            >
              <LuClock class="h-4 w-4 shrink-0" />
              <span v-if="!isCompact" class="truncate">{{ videoT('nav.continueWatching') }}</span>
            </RouterLink>

            <RouterLink
              to="/video/favorites"
              class="flex items-center gap-3 rounded-xl px-3 py-2 text-sm font-medium transition"
              :class="[
                isCompact ? 'md:justify-center md:px-0' : '',
                route.name === 'video-favorites'
                  ? 'bg-[var(--ctp-accent-soft)] font-semibold text-[var(--ctp-accent)]'
                  : 'text-[var(--ctp-subtext1)] hover:bg-[var(--ctp-surface0)]/60 hover:text-[var(--ctp-text)]',
              ]"
              :title="isCompact ? videoT('nav.favorites') : undefined"
              @click="mobileMenuOpen = false"
            >
              <LuHeart class="h-4 w-4 shrink-0" />
              <span v-if="!isCompact" class="truncate">{{ videoT('nav.favorites') }}</span>
            </RouterLink>

            <div
              v-if="isCompact"
              class="mt-3 hidden space-y-0.5 border-t border-[var(--ctp-surface0)]/55 pt-3 md:block"
            >
              <RouterLink
                to="/video/libraries"
                class="flex items-center justify-center rounded-xl py-2 transition"
                :class="
                  route.name === 'video-libraries-overview'
                    ? 'bg-[var(--ctp-accent-soft)] text-[var(--ctp-accent)]'
                    : 'text-[var(--ctp-subtext1)] hover:bg-[var(--ctp-surface0)]/60 hover:text-[var(--ctp-text)]'
                "
                :title="videoT('nav.libraries')"
              >
                <LuLibrary class="h-4 w-4 shrink-0" />
              </RouterLink>
              <RouterLink
                to="/video/shelves"
                class="flex items-center justify-center rounded-xl py-2 transition"
                :class="
                  route.name === 'video-shelves-overview'
                    ? 'bg-[var(--ctp-accent-soft)] text-[var(--ctp-accent)]'
                    : 'text-[var(--ctp-subtext1)] hover:bg-[var(--ctp-surface0)]/60 hover:text-[var(--ctp-text)]'
                "
                :title="videoT('nav.shelves')"
              >
                <LuFolderHeart class="h-4 w-4 shrink-0" />
              </RouterLink>
              <RouterLink
                to="/video/tags"
                class="flex items-center justify-center rounded-xl py-2 transition"
                :class="
                  route.name === 'video-tags-overview'
                    ? 'bg-[var(--ctp-accent-soft)] text-[var(--ctp-accent)]'
                    : 'text-[var(--ctp-subtext1)] hover:bg-[var(--ctp-surface0)]/60 hover:text-[var(--ctp-text)]'
                "
                :title="videoT('nav.tags')"
              >
                <LuTag class="h-4 w-4 shrink-0" />
              </RouterLink>
            </div>

            <!-- Video Libraries -->
            <div v-if="!isCompact && videoStore.libraries.length > 0" class="pt-5">
              <div class="mb-1 flex items-center justify-between px-2">
                <div class="flex min-w-0 flex-1 items-center gap-0.5">
                  <button
                    type="button"
                    class="rounded-md p-1 text-[var(--ctp-subtext0)] transition hover:bg-[var(--ctp-surface0)]/60 hover:text-[var(--ctp-text)]"
                    :title="videoT('nav.toggleGroup')"
                    @click="toggleGroupFold('libraries')"
                  >
                    <LuChevronRight v-if="librariesFolded" class="h-3.5 w-3.5" />
                    <LuChevronDown v-else class="h-3.5 w-3.5" />
                  </button>
                  <RouterLink
                    to="/video/libraries"
                    class="truncate rounded-md px-1 py-0.5 text-[11px] font-bold tracking-wider uppercase transition"
                    :class="
                      route.name === 'video-libraries-overview'
                        ? 'text-[var(--ctp-accent)]'
                        : 'text-[var(--ctp-subtext0)] hover:text-[var(--ctp-text)]'
                    "
                    @click="mobileMenuOpen = false"
                  >
                    {{ videoT('nav.libraries') }}
                  </RouterLink>
                </div>
                <span class="px-1.5 text-[11px] font-medium tabular-nums text-[var(--ctp-subtext0)]">
                  {{ videoStore.libraries.length }}
                </span>
              </div>

              <div v-if="!librariesFolded" class="space-y-0.5">
                <button
                  v-for="lib in visibleVideoLibraries"
                  :key="lib.id"
                  type="button"
                  class="flex w-full items-center justify-between rounded-xl px-3 py-1.5 text-sm transition"
                  :class="
                    isVideoLibraryActive(lib.id)
                      ? 'bg-[var(--ctp-accent-soft)] font-semibold text-[var(--ctp-accent)]'
                      : 'text-[var(--ctp-subtext1)] hover:bg-[var(--ctp-surface0)]/60 hover:text-[var(--ctp-text)]'
                  "
                  :title="lib.name"
                  @click="openVideoCatalog({ library: lib.id })"
                >
                  <span class="flex min-w-0 items-center gap-2.5">
                    <LuLibrary class="h-4 w-4 shrink-0" />
                    <span class="truncate">{{ lib.name }}</span>
                  </span>
                  <span class="ml-2 shrink-0 text-xs tabular-nums text-[var(--ctp-subtext0)]">
                    {{ lib.video_count }}
                  </span>
                </button>

                <button
                  v-if="videoStore.libraries.length > SIDEBAR_PREVIEW_LIMIT"
                  type="button"
                  class="flex w-full items-center gap-1.5 rounded-xl px-3 py-1.5 text-xs font-medium text-[var(--ctp-subtext0)] transition hover:bg-[var(--ctp-surface0)]/50 hover:text-[var(--ctp-text)]"
                  @click="librariesExpanded = !librariesExpanded"
                >
                  <span>
                    {{
                      librariesExpanded
                        ? videoT('nav.showLess')
                        : videoT('nav.showMore', {
                            count: videoStore.libraries.length - SIDEBAR_PREVIEW_LIMIT,
                          })
                    }}
                  </span>
                </button>
              </div>
            </div>

            <!-- Video Shelves -->
            <div v-if="!isCompact" class="pt-5">
              <div class="mb-1 flex items-center justify-between px-2">
                <div class="flex min-w-0 flex-1 items-center gap-0.5">
                  <button
                    type="button"
                    class="rounded-md p-1 text-[var(--ctp-subtext0)] transition hover:bg-[var(--ctp-surface0)]/60 hover:text-[var(--ctp-text)]"
                    :title="videoT('nav.toggleGroup')"
                    @click="toggleGroupFold('collections')"
                  >
                    <LuChevronRight v-if="collectionsFolded" class="h-3.5 w-3.5" />
                    <LuChevronDown v-else class="h-3.5 w-3.5" />
                  </button>
                  <RouterLink
                    to="/video/shelves"
                    class="truncate rounded-md px-1 py-0.5 text-[11px] font-bold tracking-wider uppercase transition"
                    :class="
                      route.name === 'video-shelves-overview'
                        ? 'text-[var(--ctp-accent)]'
                        : 'text-[var(--ctp-subtext0)] hover:text-[var(--ctp-text)]'
                    "
                    @click="mobileMenuOpen = false"
                  >
                    {{ videoT('nav.shelves') }}
                  </RouterLink>
                </div>

                <button
                  type="button"
                  class="rounded-md p-1 text-[var(--ctp-subtext0)] hover:bg-[var(--ctp-surface0)] hover:text-[var(--ctp-text)]"
                  :title="videoT('nav.newShelf')"
                  @click="collectionsFolded = false; creatingCollection = !creatingCollection"
                >
                  <LuFolderPlus class="h-3.5 w-3.5" />
                </button>
              </div>

              <div v-if="!collectionsFolded" class="space-y-0.5">
                <form
                  v-if="creatingCollection"
                  class="mb-2 px-2"
                  @submit.prevent="handleCreateCollection"
                >
                  <input
                    v-model="newCollectionName"
                    type="text"
                    :placeholder="videoT('nav.shelfNamePlaceholder')"
                    class="w-full rounded-xl border border-[var(--ctp-surface0)] bg-[var(--ctp-canvas)] px-2.5 py-1.5 text-xs outline-none focus:border-[var(--ctp-accent)]"
                  />
                </form>

                <div
                  v-for="sh in visibleVideoShelves"
                  :key="sh.id"
                  class="group flex items-center justify-between rounded-xl px-3 py-1.5 text-sm transition"
                  :class="
                    route.path === `/video/shelves/${sh.id}`
                      ? 'bg-[var(--ctp-accent-soft)] font-semibold text-[var(--ctp-accent)]'
                      : 'text-[var(--ctp-subtext1)] hover:bg-[var(--ctp-surface0)]/60 hover:text-[var(--ctp-text)]'
                  "
                  :title="sh.name"
                >
                  <RouterLink
                    :to="`/video/shelves/${sh.id}`"
                    class="flex min-w-0 flex-1 items-center gap-2.5"
                    @click="mobileMenuOpen = false"
                  >
                    <LuFolderHeart class="h-4 w-4 shrink-0" />
                    <span class="truncate">{{ sh.name }}</span>
                  </RouterLink>
                  <div class="ml-2 flex shrink-0 items-center gap-1">
                    <span class="text-xs tabular-nums text-[var(--ctp-subtext0)]">
                      {{ sh.video_count }}
                    </span>
                    <button
                      type="button"
                      class="hidden rounded p-0.5 text-[var(--ctp-subtext0)] group-hover:inline-flex hover:text-[var(--ctp-red)]"
                      @click.stop="handleDeleteVideoShelf(sh)"
                    >
                      <LuTrash2 class="h-3.5 w-3.5" />
                    </button>
                  </div>
                </div>

                <button
                  v-if="videoStore.shelves.length > SIDEBAR_PREVIEW_LIMIT"
                  type="button"
                  class="flex w-full items-center gap-1.5 rounded-xl px-3 py-1.5 text-xs font-medium text-[var(--ctp-subtext0)] transition hover:bg-[var(--ctp-surface0)]/50 hover:text-[var(--ctp-text)]"
                  @click="collectionsExpanded = !collectionsExpanded"
                >
                  <span>
                    {{
                      collectionsExpanded
                        ? videoT('nav.showLess')
                        : videoT('nav.showMore', {
                            count: videoStore.shelves.length - SIDEBAR_PREVIEW_LIMIT,
                          })
                    }}
                  </span>
                </button>
              </div>
            </div>

            <!-- Video Tags -->
            <div v-if="!isCompact && videoStore.tags.length > 0" class="pt-5">
              <div class="mb-1 flex items-center justify-between px-2">
                <div class="flex min-w-0 flex-1 items-center gap-0.5">
                  <button
                    type="button"
                    class="rounded-md p-1 text-[var(--ctp-subtext0)] transition hover:bg-[var(--ctp-surface0)]/60 hover:text-[var(--ctp-text)]"
                    :title="videoT('nav.toggleGroup')"
                    @click="toggleGroupFold('tags')"
                  >
                    <LuChevronRight v-if="tagsFolded" class="h-3.5 w-3.5" />
                    <LuChevronDown v-else class="h-3.5 w-3.5" />
                  </button>
                  <RouterLink
                    to="/video/tags"
                    class="truncate rounded-md px-1 py-0.5 text-[11px] font-bold tracking-wider uppercase transition"
                    :class="
                      route.name === 'video-tags-overview'
                        ? 'text-[var(--ctp-accent)]'
                        : 'text-[var(--ctp-subtext0)] hover:text-[var(--ctp-text)]'
                    "
                    @click="mobileMenuOpen = false"
                  >
                    {{ videoT('nav.tags') }}
                  </RouterLink>
                </div>
                <span class="px-1.5 text-[11px] font-medium tabular-nums text-[var(--ctp-subtext0)]">
                  {{ videoStore.tags.length }}
                </span>
              </div>

              <div v-if="!tagsFolded" class="space-y-0.5">
                <button
                  v-for="tg in visibleVideoTags"
                  :key="tg.name"
                  type="button"
                  class="flex w-full items-center justify-between rounded-xl px-3 py-1.5 text-sm transition"
                  :class="
                    isVideoTagActive(tg.name)
                      ? 'bg-[var(--ctp-accent-soft)] font-semibold text-[var(--ctp-accent)]'
                      : 'text-[var(--ctp-subtext1)] hover:bg-[var(--ctp-surface0)]/60 hover:text-[var(--ctp-text)]'
                  "
                  :title="tg.name"
                  @click="openVideoCatalog({ tag: tg.name })"
                >
                  <span class="flex min-w-0 items-center gap-2.5">
                    <LuTag class="h-3.5 w-3.5 shrink-0" />
                    <span class="truncate">{{ tg.name }}</span>
                  </span>
                  <span class="ml-2 shrink-0 text-xs tabular-nums text-[var(--ctp-subtext0)]">
                    {{ tg.count }}
                  </span>
                </button>

                <button
                  v-if="videoStore.tags.length > SIDEBAR_PREVIEW_LIMIT"
                  type="button"
                  class="flex w-full items-center gap-1.5 rounded-xl px-3 py-1.5 text-xs font-medium text-[var(--ctp-subtext0)] transition hover:bg-[var(--ctp-surface0)]/50 hover:text-[var(--ctp-text)]"
                  @click="tagsExpanded = !tagsExpanded"
                >
                  <span>
                    {{
                      tagsExpanded
                        ? videoT('nav.showLess')
                        : videoT('nav.showMore', {
                            count: videoStore.tags.length - SIDEBAR_PREVIEW_LIMIT,
                          })
                    }}
                  </span>
                </button>
              </div>
            </div>
          </template>
        </nav>
      </aside>

      <!-- Main Route Viewport (Flush Rounded Top-Left Canvas) -->
      <main
        class="min-w-0 flex-1 overflow-y-auto border-t border-[var(--ctp-surface0)]/65 bg-[var(--ctp-canvas)] md:rounded-tl-3xl md:border-l"
      >
        <RouterView />
      </main>
    </div>

    <!-- Persistent Bottom Audio Player (Active in Music Domain or When Playing Audio) -->
    <template v-if="showMusicBar">
      <PlayerBar />
      <QueueDrawer />
      <LyricsOverlay />
    </template>

    <!-- Non-Blocking Multi-Domain Upload Queue Shelf -->
    <UploadShelf />
  </div>
</template>

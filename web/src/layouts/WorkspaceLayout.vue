<script setup lang="ts">
import { computed, nextTick, onBeforeUnmount, onMounted, ref, watch, type Component } from 'vue'
import { useRoute, useRouter } from 'vue-router'
import { useI18n } from 'vue-i18n'
import {
  LuBookmark,
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
  LuMic,
  LuMonitor,
  LuMoon,
  LuMusic,
  LuPalette,
  LuPanelLeftClose,
  LuPanelLeftOpen,
  LuRadio,
  LuRefreshCw,
  LuSearch,
  LuSettings,
  LuSlidersHorizontal,
  LuSparkles,
  LuSun,
  LuTag,
  LuTrash2,
  LuTv,
  LuUpload,
  LuUser,
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
import type { BookBookmarkItemDto } from '@/domains/book/types'

import { api as comicApi, withAuthUrl } from '@/domains/comic/api'
import { useComicI18n } from '@/domains/comic/i18n'
import {
  resolveLibrarySeriesCount,
  useSidebarStore as useComicSidebarStore,
} from '@/domains/comic/store'
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

import { fetchHomeOverview } from '@/config/homeCache'
import LayoutCustomizerModal from '@/components/LayoutCustomizerModal.vue'
import UploadShelf from '@/components/UploadShelf.vue'

const SIDEBAR_PREVIEW_LIMIT = 5
const FOLDED_STORAGE_KEY = 'xmedia.sidebar_folded_groups'

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
const layoutCustomizerOpen = ref(false)
const dragDepth = ref(0)

const filePickerRef = ref<HTMLInputElement | null>(null)
const mainScrollRef = ref<HTMLElement | null>(null)
const searchWrapRef = ref<HTMLFormElement | null>(null)
const searchInputRef = ref<HTMLInputElement | null>(null)
const comicSearchDropdownRef = ref<HTMLDivElement | null>(null)
const accountMenuRef = ref<HTMLElement | null>(null)

const comicSearchQuery = ref('')
const videoSearchQuery = ref('')
const comicSearchFocused = ref(false)
const comicSearchResults = ref<SeriesSummaryDto[]>([])
const comicSearchLoading = ref(false)
const searchRect = ref({ top: 56, left: 0, width: 480 })
let comicSearchTimer: ReturnType<typeof setTimeout> | null = null

const creatingGroupId = ref<string | null>(null)
const newCollectionName = ref('')

function loadInitialFolded(): Record<string, boolean> {
  try {
    const raw = localStorage.getItem(FOLDED_STORAGE_KEY)
    return raw ? (JSON.parse(raw) as Record<string, boolean>) : {}
  } catch {
    return {}
  }
}

const foldedMap = ref<Record<string, boolean>>(loadInitialFolded())
const expandedMap = ref<Record<string, boolean>>({})

function isGroupFolded(id: string): boolean {
  return Boolean(foldedMap.value[id])
}

function toggleGroupFold(id: string): void {
  const next = !foldedMap.value[id]
  foldedMap.value = { ...foldedMap.value, [id]: next }
  localStorage.setItem(FOLDED_STORAGE_KEY, JSON.stringify(foldedMap.value))
}

function isGroupExpanded(id: string): boolean {
  return Boolean(expandedMap.value[id])
}

function toggleGroupExpanded(id: string): void {
  expandedMap.value = { ...expandedMap.value, [id]: !expandedMap.value[id] }
}

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

let sidebarLoadTimer: ReturnType<typeof setTimeout> | null = null

watch(
  () => domainStore.activeDomain,
  (next) => {
    creatingGroupId.value = null
    newCollectionName.value = ''
    comicSearchFocused.value = false
    if (mainScrollRef.value) {
      mainScrollRef.value.scrollTop = 0
    }
    if (sidebarLoadTimer) clearTimeout(sidebarLoadTimer)
    if (loadedDomains.has(next)) {
      void ensureDomainSidebarLoaded(next)
    } else {
      sidebarLoadTimer = setTimeout(() => {
        void ensureDomainSidebarLoaded(next)
      }, 60)
    }
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
    exts.push(
      '.mp4',
      '.mkv',
      '.webm',
      '.mov',
      '.avi',
      '.ts',
      '.m4v',
      '.flv',
      '.srt',
      '.vtt',
      '.ass',
      '.ssa',
    )
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

// Domain Navigation Helpers
function isBookSectionActive(section: BookNavSection, id = ''): boolean {
  if (route.name === 'book-detail' && section === 'all') return true
  if (route.name !== 'book-catalog') return false
  if (bookStore.activeSection !== section) return false
  if (section === 'library') return bookStore.activeLibraryId === id
  if (section === 'shelf') return bookStore.activeShelfId === id
  if (section === 'tag') {
    return bookStore.filterTags.length === 1 && bookStore.filterTags[0] === id
  }
  if (section === 'author') {
    return bookStore.filterAuthors.length === 1 && bookStore.filterAuthors[0] === id
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

function openBookBookmark(item: BookBookmarkItemDto): void {
  mobileMenuOpen.value = false
  const loc = item.bookmark.location
  const query: Record<string, string> = { locator: loc.locator }
  if (loc.chapter_index !== null) query.chapter = String(loc.chapter_index)
  if (loc.page_number !== null) query.page = String(loc.page_number)
  if (loc.scroll_ratio !== null) query.ratio = String(loc.scroll_ratio)
  void router.push({ path: `/book/read/${item.bookmark.book_id}`, query })
}

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
const activeComicGenre = computed(() =>
  route.name === 'comic-series-list' && typeof route.query.genre === 'string'
    ? route.query.genre
    : '',
)
const activeComicWriter = computed(() =>
  route.name === 'comic-series-list' && typeof route.query.writer === 'string'
    ? route.query.writer
    : '',
)
const activeComicPenciller = computed(() =>
  route.name === 'comic-series-list' && typeof route.query.penciller === 'string'
    ? route.query.penciller
    : '',
)

const isAllComicSeriesActive = computed(() => {
  if (route.name === 'comic-series-detail') return true
  if (route.name !== 'comic-series-list') return false
  return (
    !activeComicLibraryId.value &&
    !activeComicShelfId.value &&
    !activeComicTag.value &&
    !activeComicGenre.value &&
    !activeComicWriter.value &&
    !activeComicPenciller.value
  )
})

const isAllMusicActive = computed(() => {
  if (route.name === 'music-album-detail' || route.name === 'music-artist-detail') return true
  if (route.name !== 'music-songs') return false
  return !route.query.library && !route.query.genre && !route.query.tag
})

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

const isAllVideoActive = computed(() => {
  if (route.name === 'video-show-detail') return true
  if (route.name !== 'video-catalog') return false
  return !route.query.library && !route.query.genre && !route.query.tag
})

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

// Inline shelf/playlist create & delete handlers
async function handleCreateCollection(): Promise<void> {
  const name = newCollectionName.value.trim()
  if (!name) return
  const d = domainStore.activeDomain
  newCollectionName.value = ''
  creatingGroupId.value = null

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

// Unified Primary Navigation & Dynamic Customizable Sidebar Groups
interface PrimaryNavItem {
  key: string
  label: string
  icon: Component
  active: boolean
  onClick: () => void
}

const primaryNavItems = computed<PrimaryNavItem[]>(() => {
  const d = domainStore.activeDomain
  const go = (path: string) => {
    mobileMenuOpen.value = false
    void router.push(path)
  }

  if (d === 'book') {
    return [
      {
        key: 'home',
        label: bookT('nav.home'),
        icon: LuHome,
        active: route.name === 'book-home',
        onClick: () => go('/book'),
      },
      {
        key: 'all',
        label: bookT('nav.allBooks'),
        icon: LuBookOpen,
        active: isBookSectionActive('all'),
        onClick: () => void openBookSection('all'),
      },
      {
        key: 'continue',
        label: bookT('nav.recentReading'),
        icon: LuClock,
        active: isBookSectionActive('recent'),
        onClick: () => void openBookSection('recent'),
      },
      {
        key: 'favorites',
        label: bookT('nav.favorites'),
        icon: LuHeart,
        active: isBookSectionActive('favorites'),
        onClick: () => void openBookSection('favorites'),
      },
    ]
  }

  if (d === 'comic') {
    return [
      {
        key: 'home',
        label: comicT('nav.home'),
        icon: LuHome,
        active: route.name === 'comic-home',
        onClick: () => go('/comic'),
      },
      {
        key: 'all',
        label: comicT('nav.allSeries'),
        icon: LuLayers,
        active: isAllComicSeriesActive.value,
        onClick: () => go('/comic/series'),
      },
      {
        key: 'continue',
        label: comicT('nav.continueReading'),
        icon: LuClock,
        active: route.name === 'comic-continue',
        onClick: () => go('/comic/continue'),
      },
      {
        key: 'favorites',
        label: comicT('nav.favorites'),
        icon: LuHeart,
        active: route.name === 'comic-favorites',
        onClick: () => go('/comic/favorites'),
      },
    ]
  }

  if (d === 'music') {
    return [
      {
        key: 'home',
        label: musicT('nav.home'),
        icon: LuHome,
        active: route.name === 'music-home',
        onClick: () => go('/music'),
      },
      {
        key: 'all',
        label: musicT('nav.all'),
        icon: LuMusic,
        active: isAllMusicActive.value,
        onClick: () => void openMusicCatalog(),
      },
      {
        key: 'continue',
        label: musicT('nav.continueListening'),
        icon: LuClock,
        active: route.name === 'music-continue',
        onClick: () => go('/music/continue'),
      },
      {
        key: 'favorites',
        label: musicT('nav.favorites'),
        icon: LuHeart,
        active: route.name === 'music-favorites',
        onClick: () => go('/music/favorites'),
      },
    ]
  }

  return [
    {
      key: 'home',
      label: videoT('nav.home'),
      icon: LuHome,
      active: route.name === 'video-home',
      onClick: () => go('/video'),
    },
    {
      key: 'all',
      label: videoT('nav.all'),
      icon: LuFilm,
      active: isAllVideoActive.value,
      onClick: () => void openVideoCatalog(),
    },
    {
      key: 'continue',
      label: videoT('nav.continueWatching'),
      icon: LuClock,
      active: route.name === 'video-continue',
      onClick: () => go('/video/continue'),
    },
    {
      key: 'favorites',
      label: videoT('nav.favorites'),
      icon: LuHeart,
      active: route.name === 'video-favorites',
      onClick: () => go('/video/favorites'),
    },
  ]
})

interface SidebarSubItem {
  id: string
  label: string
  count?: number | string
  icon: Component
  active: boolean
  scanning?: boolean
  onClick: () => void
  onDelete?: () => void
}

interface SidebarGroupDescriptor {
  id: string
  key: string
  label: string
  icon: Component
  overviewActive: boolean
  onOpenOverview: () => void
  canCreate?: boolean
  createTitle?: string
  createPlaceholder?: string
  items: SidebarSubItem[]
}

function visibleGroupItems(group: SidebarGroupDescriptor): SidebarSubItem[] {
  const all = group.items
  if (isGroupExpanded(group.id) || all.length <= SIDEBAR_PREVIEW_LIMIT) return all
  const head = all.slice(0, SIDEBAR_PREVIEW_LIMIT)
  if (!head.some((it) => it.active)) {
    const activeItem = all.find((it) => it.active)
    if (activeItem) return [...head.slice(0, SIDEBAR_PREVIEW_LIMIT - 1), activeItem]
  }
  return head
}

function buildDomainSidebarGroup(domain: MediaKind, key: string): SidebarGroupDescriptor | null {
  const id = `${domain}:${key}`
  const label = t(`layoutCustomizer.sidebar.${domain}.${key}`)
  const go = (target: string | { path: string; query?: Record<string, string> }) => {
    mobileMenuOpen.value = false
    void router.push(target)
  }

  if (domain === 'book') {
    switch (key) {
      case 'authors':
        return {
          id,
          key,
          label,
          icon: LuUser,
          overviewActive: route.name === 'book-authors-overview',
          onOpenOverview: () => go('/book/authors'),
          items: bookStore.authors.map((au) => ({
            id: au.name,
            label: au.name,
            count: au.count,
            icon: LuUser,
            active: isBookSectionActive('author', au.name),
            onClick: () => void openBookSection('author', au.name),
          })),
        }
      case 'tags':
        return {
          id,
          key,
          label,
          icon: LuTag,
          overviewActive: route.name === 'book-tags-overview',
          onOpenOverview: () => go('/book/tags'),
          items: bookStore.tags.map((tg) => ({
            id: tg.name,
            label: tg.name,
            count: tg.count,
            icon: LuTag,
            active: isBookSectionActive('tag', tg.name),
            onClick: () => void openBookSection('tag', tg.name),
          })),
        }
      case 'shelves':
        return {
          id,
          key,
          label,
          icon: LuFolder,
          overviewActive: route.name === 'book-shelves-overview',
          onOpenOverview: () => go('/book/shelves'),
          canCreate: true,
          createTitle: bookT('nav.newShelf'),
          createPlaceholder: bookT('nav.shelfNamePlaceholder'),
          items: bookStore.shelves.map((sh) => ({
            id: sh.id,
            label: sh.name,
            count: sh.book_count,
            icon: LuFolder,
            active: isBookSectionActive('shelf', sh.id),
            onClick: () => void openBookSection('shelf', sh.id),
            onDelete: sh.is_favorite ? undefined : () => void handleDeleteBookShelf(sh.id, sh.name),
          })),
        }
      case 'libraries':
        return {
          id,
          key,
          label,
          icon: LuLibrary,
          overviewActive: route.name === 'book-libraries-overview',
          onOpenOverview: () => go('/book/libraries'),
          items: bookStore.libraries.map((lib) => ({
            id: lib.id,
            label: lib.name,
            count: lib.book_count,
            icon: LuLibrary,
            active: isBookSectionActive('library', lib.id),
            scanning: lib.scan_status.state === 'scanning',
            onClick: () => void openBookSection('library', lib.id),
          })),
        }
      case 'bookmarks':
        return {
          id,
          key,
          label,
          icon: LuBookmark,
          overviewActive: route.name === 'book-bookmarks',
          onOpenOverview: () => go('/book/bookmarks'),
          items: bookStore.bookmarks.map((bm) => ({
            id: bm.bookmark.id,
            label: `${bm.book_title} · ${bm.bookmark.title}`,
            count: `${Math.round(bm.bookmark.location.progress_percent)}%`,
            icon: LuBookmark,
            active: false,
            onClick: () => openBookBookmark(bm),
            onDelete: () =>
              void bookApi
                .deleteBookmark(bm.bookmark.id)
                .then(() => bookStore.refreshSidebarData()),
          })),
        }
      default:
        return null
    }
  }

  if (domain === 'comic') {
    switch (key) {
      case 'writers':
        return {
          id,
          key,
          label,
          icon: LuUser,
          overviewActive: route.name === 'comic-tags',
          onOpenOverview: () => go('/comic/tags'),
          items: comicSidebarStore.writers.map((w) => ({
            id: w.name,
            label: w.name,
            count: w.count,
            icon: LuUser,
            active: activeComicWriter.value === w.name,
            onClick: () => go({ path: '/comic/series', query: { writer: w.name } }),
          })),
        }
      case 'pencillers':
        return {
          id,
          key,
          label,
          icon: LuPalette,
          overviewActive: route.name === 'comic-tags',
          onOpenOverview: () => go('/comic/tags'),
          items: comicSidebarStore.pencillers.map((p) => ({
            id: p.name,
            label: p.name,
            count: p.count,
            icon: LuPalette,
            active: activeComicPenciller.value === p.name,
            onClick: () => go({ path: '/comic/series', query: { penciller: p.name } }),
          })),
        }
      case 'genres':
        return {
          id,
          key,
          label,
          icon: LuSparkles,
          overviewActive: route.name === 'comic-tags',
          onOpenOverview: () => go('/comic/tags'),
          items: comicSidebarStore.genres.map((g) => ({
            id: g.name,
            label: g.name,
            count: g.count,
            icon: LuSparkles,
            active: activeComicGenre.value === g.name,
            onClick: () => go({ path: '/comic/series', query: { genre: g.name } }),
          })),
        }
      case 'tags':
        return {
          id,
          key,
          label,
          icon: LuTag,
          overviewActive: route.name === 'comic-tags',
          onOpenOverview: () => go('/comic/tags'),
          items: comicSidebarStore.tags.map((tg) => ({
            id: tg.name,
            label: `#${tg.name}`,
            count: tg.count,
            icon: LuTag,
            active: activeComicTag.value === tg.name,
            onClick: () => go({ path: '/comic/series', query: { tag: tg.name } }),
          })),
        }
      case 'shelves':
        return {
          id,
          key,
          label,
          icon: LuFolder,
          overviewActive: route.name === 'comic-shelves',
          onOpenOverview: () => go('/comic/shelves'),
          canCreate: true,
          createTitle: comicT('shelves.newShelf'),
          createPlaceholder: comicT('shelves.shelfNamePlaceholder'),
          items: comicSidebarStore.shelves.map((s) => ({
            id: s.shelf.id,
            label: s.shelf.name,
            count: s.series.length,
            icon: LuFolder,
            active: activeComicShelfId.value === s.shelf.id,
            onClick: () => go({ path: '/comic/series', query: { shelf_id: s.shelf.id } }),
            onDelete: () => void handleDeleteComicShelf(s.shelf.id, s.shelf.name),
          })),
        }
      case 'libraries':
        return {
          id,
          key,
          label,
          icon: LuLibrary,
          overviewActive: route.name === 'comic-libraries',
          onOpenOverview: () => go('/comic/libraries'),
          items: comicSidebarStore.libraries.map((lib) => ({
            id: lib.id,
            label: lib.name,
            count: resolveLibrarySeriesCount(lib),
            icon: LuLibrary,
            active: activeComicLibraryId.value === lib.id,
            scanning: lib.scan_state.state === 'scanning',
            onClick: () => go({ path: '/comic/series', query: { library_id: lib.id } }),
          })),
        }
      case 'bookmarks':
        return {
          id,
          key,
          label,
          icon: LuBookmark,
          overviewActive: route.name === 'comic-bookmarks',
          onOpenOverview: () => go('/comic/bookmarks'),
          items: comicSidebarStore.bookmarks.map((bm) => ({
            id: bm.bookmark.id,
            label: `${bm.series_title} #${bm.chapter_number}`,
            count: `P${bm.bookmark.page_number}`,
            icon: LuBookmark,
            active: false,
            onClick: () =>
              go(
                `/comic/read/${encodeURIComponent(bm.bookmark.chapter_id)}?page=${bm.bookmark.page_number}`,
              ),
            onDelete: () =>
              void comicApi
                .deleteBookmark(bm.bookmark.id)
                .then(() => comicSidebarStore.refresh()),
          })),
        }
      default:
        return null
    }
  }

  if (domain === 'music') {
    switch (key) {
      case 'artists':
        return {
          id,
          key,
          label,
          icon: LuMic,
          overviewActive: route.name === 'music-songs' && route.query.tab === 'artists',
          onOpenOverview: () => go({ path: '/music/songs', query: { tab: 'artists' } }),
          items: musicStore.artists.map((ar) => ({
            id: ar.id,
            label: ar.name,
            count: ar.track_count,
            icon: LuMic,
            active: route.name === 'music-artist-detail' && route.params.id === ar.id,
            onClick: () => go(`/music/artists/${ar.id}`),
          })),
        }
      case 'tags':
        return {
          id,
          key,
          label,
          icon: LuTag,
          overviewActive: route.name === 'music-tags-overview',
          onOpenOverview: () => go('/music/tags'),
          items: musicStore.tags.map((tg) => ({
            id: tg.name,
            label: `#${tg.name}`,
            count: tg.song_count,
            icon: LuTag,
            active: route.name === 'music-songs' && route.query.tag === tg.name,
            onClick: () => void openMusicCatalog({ tag: tg.name }),
          })),
        }
      case 'albums':
        return {
          id,
          key,
          label,
          icon: LuDisc3,
          overviewActive: route.name === 'music-songs' && route.query.tab === 'albums',
          onOpenOverview: () => go({ path: '/music/songs', query: { tab: 'albums' } }),
          items: musicStore.albums.map((al) => ({
            id: al.id,
            label: al.title,
            count: al.track_count,
            icon: LuDisc3,
            active: route.name === 'music-album-detail' && route.params.id === al.id,
            onClick: () => go(`/music/albums/${al.id}`),
          })),
        }
      case 'genres':
        return {
          id,
          key,
          label,
          icon: LuRadio,
          overviewActive: route.name === 'music-genres-overview',
          onOpenOverview: () => go('/music/genres'),
          items: musicStore.genres.map((g) => ({
            id: g.name,
            label: g.name,
            count: g.song_count,
            icon: LuRadio,
            active: route.name === 'music-songs' && route.query.genre === g.name,
            onClick: () => void openMusicCatalog({ genre: g.name }),
          })),
        }
      case 'playlists':
        return {
          id,
          key,
          label,
          icon: LuListMusic,
          overviewActive: route.name === 'music-playlists-overview',
          onOpenOverview: () => go('/music/playlists'),
          canCreate: true,
          createTitle: musicT('nav.newPlaylist'),
          createPlaceholder: musicT('nav.playlistNamePlaceholder'),
          items: musicStore.playlists.map((pl) => ({
            id: pl.id,
            label: pl.name,
            count: pl.track_count,
            icon: LuListMusic,
            active: route.path === `/music/playlists/${pl.id}`,
            onClick: () => go(`/music/playlists/${pl.id}`),
            onDelete: canEditPlaylist(pl) ? () => void handleDeleteMusicPlaylist(pl) : undefined,
          })),
        }
      case 'libraries':
        return {
          id,
          key,
          label,
          icon: LuLibrary,
          overviewActive: route.name === 'music-libraries-overview',
          onOpenOverview: () => go('/music/libraries'),
          items: musicStore.libraries.map((lib) => ({
            id: lib.id,
            label: lib.name,
            count: lib.track_count,
            icon: LuLibrary,
            active: route.name === 'music-songs' && route.query.library === lib.id,
            scanning: lib.scan_status.state === 'scanning',
            onClick: () => void openMusicCatalog({ library: lib.id }),
          })),
        }
      default:
        return null
    }
  }

  // Video domain
  switch (key) {
    case 'shows':
      return {
        id,
        key,
        label,
        icon: LuTv,
        overviewActive: route.name === 'video-catalog' && route.query.tab === 'shows',
        onOpenOverview: () => go({ path: '/video/catalog', query: { tab: 'shows' } }),
        items: videoStore.shows.map((sh) => ({
          id: sh.id,
          label: sh.title,
          count: sh.episode_count,
          icon: LuTv,
          active: route.name === 'video-show-detail' && route.params.id === sh.id,
          onClick: () => go(`/video/shows/${sh.id}`),
        })),
      }
    case 'genres':
      return {
        id,
        key,
        label,
        icon: LuSparkles,
        overviewActive: route.name === 'video-genres-overview',
        onOpenOverview: () => go('/video/genres'),
        items: videoStore.genres.map((g) => ({
          id: g.name,
          label: g.name,
          count: g.count,
          icon: LuSparkles,
          active: route.name === 'video-catalog' && route.query.genre === g.name,
          onClick: () => void openVideoCatalog({ genre: g.name }),
        })),
      }
    case 'tags':
      return {
        id,
        key,
        label,
        icon: LuTag,
        overviewActive: route.name === 'video-tags-overview',
        onOpenOverview: () => go('/video/tags'),
        items: videoStore.tags.map((tg) => ({
          id: tg.name,
          label: `#${tg.name}`,
          count: tg.count,
          icon: LuTag,
          active: route.name === 'video-catalog' && route.query.tag === tg.name,
          onClick: () => void openVideoCatalog({ tag: tg.name }),
        })),
      }
    case 'shelves':
      return {
        id,
        key,
        label,
        icon: LuFolderHeart,
        overviewActive: route.name === 'video-shelves-overview',
        onOpenOverview: () => go('/video/shelves'),
        canCreate: true,
        createTitle: videoT('nav.newShelf'),
        createPlaceholder: videoT('nav.shelfNamePlaceholder'),
        items: videoStore.shelves.map((sh) => ({
          id: sh.id,
          label: sh.name,
          count: sh.video_count,
          icon: LuFolderHeart,
          active: route.path === `/video/shelves/${sh.id}`,
          onClick: () => go(`/video/shelves/${sh.id}`),
          onDelete: () => void handleDeleteVideoShelf(sh),
        })),
      }
    case 'libraries':
      return {
        id,
        key,
        label,
        icon: LuLibrary,
        overviewActive: route.name === 'video-libraries-overview',
        onOpenOverview: () => go('/video/libraries'),
        items: videoStore.libraries.map((lib) => ({
          id: lib.id,
          label: lib.name,
          count: lib.video_count,
          icon: LuLibrary,
          active: route.name === 'video-catalog' && route.query.library === lib.id,
          scanning: lib.scan_status.state === 'scanning',
          onClick: () => void openVideoCatalog({ library: lib.id }),
        })),
      }
    default:
      return null
  }
}

const activeSidebarGroups = computed<SidebarGroupDescriptor[]>(() => {
  const dom = domainStore.activeDomain
  const configuredKeys = themeStore.workspaceLayout[dom].sidebar_groups
  return configuredKeys
    .map((key) => buildDomainSidebarGroup(dom, key))
    .filter((g): g is SidebarGroupDescriptor => g !== null)
})

function toggleCreateForGroup(group: SidebarGroupDescriptor): void {
  if (isGroupFolded(group.id)) {
    toggleGroupFold(group.id)
  }
  creatingGroupId.value = creatingGroupId.value === group.id ? null : group.id
  newCollectionName.value = ''
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
    layoutCustomizerOpen.value = false
    comicSearchFocused.value = false
    playerStore.lyricsOpen = false
    playerStore.queueOpen = false
  }
}

let idlePrewarmTimer: ReturnType<typeof setTimeout> | null = null

async function prewarmOtherDomains(): Promise<void> {
  const loaders: Record<MediaKind, () => Promise<unknown>> = {
    book: () => import('@/domains/book/views/HomeView.vue'),
    comic: () => import('@/domains/comic/views/HomeView.vue'),
    music: () => import('@/domains/music/views/HomeView.vue'),
    video: () => import('@/domains/video/views/HomeView.vue'),
  }
  const others = authStore.enabledModules.filter((k) => k !== domainStore.activeDomain)
  await Promise.allSettled(others.map((k) => loaders[k]()))
  for (const kind of others) {
    await fetchHomeOverview(kind).catch(() => {})
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
  idlePrewarmTimer = setTimeout(() => {
    void prewarmOtherDomains()
  }, 350)
})

onBeforeUnmount(() => {
  document.removeEventListener('mousedown', handleOutsideClick)
  window.removeEventListener('keydown', handleGlobalKeydown)
  window.removeEventListener('resize', updateSearchDropdownPosition)
  eventsStore.disconnect()
  if (comicSearchTimer) clearTimeout(comicSearchTimer)
  if (sidebarLoadTimer) clearTimeout(sidebarLoadTimer)
  if (idlePrewarmTimer) clearTimeout(idlePrewarmTimer)
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

    <!-- Full-Width Unified Top Bar -->
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

      <!-- Comic Live Search Dropdown -->
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
          <!-- Fixed Top Primary Navigation (Home / All / Continue / Favorites) -->
          <button
            v-for="item in primaryNavItems"
            :key="item.key"
            type="button"
            class="flex w-full items-center gap-3 rounded-xl px-3 py-2 text-sm font-medium transition"
            :class="[
              isCompact ? 'md:justify-center md:px-0' : '',
              item.active
                ? 'bg-[var(--ctp-accent-soft)] font-semibold text-[var(--ctp-accent)]'
                : 'text-[var(--ctp-subtext1)] hover:bg-[var(--ctp-surface0)]/60 hover:text-[var(--ctp-text)]',
            ]"
            :title="isCompact ? item.label : undefined"
            @click="item.onClick()"
          >
            <component :is="item.icon" class="h-4 w-4 shrink-0" />
            <span v-if="!isCompact" class="truncate">{{ item.label }}</span>
          </button>

          <!-- Compact Rail Icons for Configured Sidebar Groups -->
          <div
            v-if="isCompact"
            class="mt-3 hidden space-y-0.5 border-t border-[var(--ctp-surface0)]/55 pt-3 md:block"
          >
            <button
              v-for="group in activeSidebarGroups"
              :key="group.id"
              type="button"
              class="flex w-full items-center justify-center rounded-xl py-2 transition"
              :class="
                group.overviewActive || group.items.some((it) => it.active)
                  ? 'bg-[var(--ctp-accent-soft)] text-[var(--ctp-accent)]'
                  : 'text-[var(--ctp-subtext1)] hover:bg-[var(--ctp-surface0)]/60 hover:text-[var(--ctp-text)]'
              "
              :title="group.label"
              @click="group.onOpenOverview()"
            >
              <component :is="group.icon" class="h-4 w-4 shrink-0" />
            </button>

            <button
              type="button"
              class="flex w-full items-center justify-center rounded-xl py-2 text-[var(--ctp-subtext0)] transition hover:bg-[var(--ctp-surface0)]/60 hover:text-[var(--ctp-text)]"
              :title="t('layoutCustomizer.customizeSidebar')"
              @click="layoutCustomizerOpen = true"
            >
              <LuSlidersHorizontal class="h-4 w-4 shrink-0" />
            </button>
          </div>

          <!-- Expanded Customizable Foldable Groups (0..=4) -->
          <template v-if="!isCompact">
            <div class="mt-4 flex items-center justify-between border-t border-[var(--ctp-surface0)]/50 px-2 pt-3">
              <span class="text-[10px] font-semibold tracking-wider text-[var(--ctp-subtext0)]/80 uppercase">
                {{ t(`app.domains.${domainStore.activeDomain}`) }}
              </span>
              <button
                type="button"
                class="inline-flex items-center gap-1 rounded-md px-1.5 py-0.5 text-[11px] font-medium text-[var(--ctp-subtext0)] transition hover:bg-[var(--ctp-surface0)]/60 hover:text-[var(--ctp-text)]"
                :title="t('layoutCustomizer.customizeSidebar')"
                @click="layoutCustomizerOpen = true"
              >
                <LuSlidersHorizontal class="h-3 w-3" />
                <span>{{ t('layoutCustomizer.customizeSidebar') }}</span>
              </button>
            </div>

            <div
              v-for="group in activeSidebarGroups"
              :key="group.id"
              class="pt-3.5"
            >
              <div class="mb-1 flex items-center justify-between px-2">
                <div class="flex min-w-0 flex-1 items-center gap-0.5">
                  <button
                    type="button"
                    class="rounded-md p-1 text-[var(--ctp-subtext0)] transition hover:bg-[var(--ctp-surface0)]/60 hover:text-[var(--ctp-text)]"
                    :title="t('nav.toggleGroup')"
                    @click="toggleGroupFold(group.id)"
                  >
                    <LuChevronRight v-if="isGroupFolded(group.id)" class="h-3.5 w-3.5" />
                    <LuChevronDown v-else class="h-3.5 w-3.5" />
                  </button>

                  <button
                    type="button"
                    class="truncate rounded-md px-1 py-0.5 text-left text-[11px] font-bold tracking-wider uppercase transition"
                    :class="
                      group.overviewActive
                        ? 'text-[var(--ctp-accent)]'
                        : 'text-[var(--ctp-subtext0)] hover:text-[var(--ctp-text)]'
                    "
                    @click="group.onOpenOverview()"
                  >
                    {{ group.label }}
                  </button>
                </div>

                <button
                  v-if="group.canCreate"
                  type="button"
                  class="rounded-md p-1 text-[var(--ctp-subtext0)] transition hover:bg-[var(--ctp-surface0)] hover:text-[var(--ctp-text)]"
                  :title="group.createTitle"
                  @click="toggleCreateForGroup(group)"
                >
                  <LuFolderPlus class="h-3.5 w-3.5" />
                </button>
                <span
                  v-else
                  class="px-1.5 text-[11px] font-medium tabular-nums text-[var(--ctp-subtext0)]"
                >
                  {{ group.items.length }}
                </span>
              </div>

              <div v-if="!isGroupFolded(group.id)" class="space-y-0.5">
                <form
                  v-if="group.canCreate && creatingGroupId === group.id"
                  class="mb-2 px-2"
                  @submit.prevent="handleCreateCollection"
                >
                  <input
                    v-model="newCollectionName"
                    type="text"
                    :placeholder="group.createPlaceholder"
                    class="w-full rounded-xl border border-[var(--ctp-surface0)] bg-[var(--ctp-canvas)] px-2.5 py-1.5 text-xs outline-none focus:border-[var(--ctp-accent)]"
                  />
                </form>

                <div
                  v-for="sub in visibleGroupItems(group)"
                  :key="sub.id"
                  class="group flex items-center justify-between rounded-xl px-3 py-1.5 text-sm transition"
                  :class="
                    sub.active
                      ? 'bg-[var(--ctp-accent-soft)] font-semibold text-[var(--ctp-accent)]'
                      : 'text-[var(--ctp-subtext1)] hover:bg-[var(--ctp-surface0)]/60 hover:text-[var(--ctp-text)]'
                  "
                  :title="sub.label"
                >
                  <button
                    type="button"
                    class="flex min-w-0 flex-1 items-center gap-2.5 text-left"
                    @click="sub.onClick()"
                  >
                    <component :is="sub.icon" class="h-3.5 w-3.5 shrink-0" />
                    <span class="truncate">{{ sub.label }}</span>
                  </button>

                  <div class="ml-2 flex shrink-0 items-center gap-1">
                    <LuRefreshCw
                      v-if="sub.scanning"
                      class="h-3.5 w-3.5 animate-spin text-[var(--ctp-accent)]"
                    />
                    <span
                      v-else-if="sub.count !== undefined"
                      class="text-xs tabular-nums text-[var(--ctp-subtext0)]"
                    >
                      {{ sub.count }}
                    </span>

                    <button
                      v-if="sub.onDelete"
                      type="button"
                      class="hidden rounded p-0.5 text-[var(--ctp-subtext0)] group-hover:inline-flex hover:text-[var(--ctp-red)]"
                      :title="t('common.delete')"
                      @click.stop="sub.onDelete()"
                    >
                      <LuTrash2 class="h-3.5 w-3.5" />
                    </button>
                  </div>
                </div>

                <button
                  v-if="group.items.length > SIDEBAR_PREVIEW_LIMIT"
                  type="button"
                  class="flex w-full items-center gap-1.5 rounded-xl px-3 py-1.5 text-xs font-medium text-[var(--ctp-subtext0)] transition hover:bg-[var(--ctp-surface0)]/50 hover:text-[var(--ctp-text)]"
                  @click="toggleGroupExpanded(group.id)"
                >
                  <span>
                    {{
                      isGroupExpanded(group.id)
                        ? t('nav.showLess')
                        : t('nav.showMore', {
                            count: group.items.length - SIDEBAR_PREVIEW_LIMIT,
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
        ref="mainScrollRef"
        class="min-w-0 flex-1 overflow-y-auto border-t border-[var(--ctp-surface0)]/65 bg-[var(--ctp-canvas)] md:rounded-tl-3xl md:border-l"
      >
        <RouterView v-slot="{ Component }">
          <KeepAlive :include="['BookHomeView', 'ComicHomeView', 'MusicHomeView', 'VideoHomeView']">
            <component :is="Component" />
          </KeepAlive>
        </RouterView>
      </main>
    </div>

    <!-- Persistent Bottom Audio Player -->
    <template v-if="showMusicBar">
      <PlayerBar />
      <QueueDrawer />
      <LyricsOverlay />
    </template>

    <!-- Non-Blocking Multi-Domain Upload Queue Shelf -->
    <UploadShelf />

    <!-- Sidebar Layout Customizer Modal -->
    <LayoutCustomizerModal
      :open="layoutCustomizerOpen"
      :domain="domainStore.activeDomain"
      initial-tab="sidebar"
      @close="layoutCustomizerOpen = false"
    />
  </div>
</template>

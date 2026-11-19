<script setup lang="ts">
import { computed, ref, watch, type Component } from 'vue'
import { useRoute, useRouter } from 'vue-router'
import { useI18n } from 'vue-i18n'
import {
  LuBookmark,
  LuBookOpen,
  LuChevronDown,
  LuChevronRight,
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
  LuMic,
  LuMusic,
  LuPalette,
  LuRadio,
  LuRefreshCw,
  LuSlidersHorizontal,
  LuSparkles,
  LuTag,
  LuTrash2,
  LuTv,
  LuUser,
} from 'vue-icons-plus/lu'
import { useAuthStore } from '@/stores/auth'
import { useDialogStore } from '@/stores/dialog'
import { useDomainStore } from '@/stores/domain'
import { useThemeStore } from '@/stores/theme'
import { useBookI18n } from '@/domains/book/i18n'
import { useCatalogStore, type NavSection as BookNavSection } from '@/domains/book/store'
import { api as bookApi } from '@/domains/book/api'
import type { BookBookmarkItemDto } from '@/domains/book/types'
import { useComicI18n } from '@/domains/comic/i18n'
import {
  resolveLibrarySeriesCount,
  useSidebarStore as useComicSidebarStore,
} from '@/domains/comic/store'
import { api as comicApi } from '@/domains/comic/api'
import { useMusicI18n } from '@/domains/music/i18n'
import { useMusicStore } from '@/domains/music/store'
import { api as musicApi } from '@/domains/music/api'
import type { PlaylistSummaryDto } from '@/domains/music/types'
import { useVideoI18n } from '@/domains/video/i18n'
import { useVideoStore } from '@/domains/video/store'
import { api as videoApi } from '@/domains/video/api'
import type { VideoShelfDto } from '@/domains/video/types'
import type { MediaKind } from '@/types/api'

const SIDEBAR_PREVIEW_LIMIT = 5
const FOLDED_STORAGE_KEY = 'xmedia.sidebar_folded_groups'

defineProps<{
  mobileMenuOpen: boolean
  collapsed: boolean
  isCompact: boolean
  showMusicBar: boolean
  domainTabs: Array<{ kind: MediaKind; label: string; icon: Component }>
}>()

const emit = defineEmits<{
  closeMobile: []
  switchDomain: [kind: MediaKind]
  openCustomizer: []
}>()

const route = useRoute()
const router = useRouter()
const { t } = useI18n()
const { t: bookT } = useBookI18n()
const { t: comicT } = useComicI18n()
const { t: musicT } = useMusicI18n()
const { t: videoT } = useVideoI18n()

const authStore = useAuthStore()
const dialogStore = useDialogStore()
const domainStore = useDomainStore()
const themeStore = useThemeStore()

const bookStore = useCatalogStore()
const comicSidebarStore = useComicSidebarStore()
const musicStore = useMusicStore()
const videoStore = useVideoStore()

const creatingGroupId = ref<string | null>(null)
const newCollectionName = ref('')

watch(
  () => domainStore.activeDomain,
  () => {
    creatingGroupId.value = null
    newCollectionName.value = ''
  },
)

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
  emit('closeMobile')
  bookStore.selectSection(section, id)
  const query: Record<string, string> = {}
  if (section !== 'all') query.section = section
  if (section === 'library' && id) query.library = id
  if (section === 'shelf' && id) query.shelf = id
  if (section === 'tag' && id) query.tag = id
  if (section === 'author' && id) query.author = id
  await router.push({ path: '/book/catalog', query })
}

function openBookBookmark(item: BookBookmarkItemDto): void {
  emit('closeMobile')
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
  emit('closeMobile')
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
  emit('closeMobile')
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
    emit('closeMobile')
    await router.push({ path: '/comic/series', query: { shelf_id: created.id } })
  } else if (d === 'music') {
    const created = await musicApi.createPlaylist({ name })
    await musicStore.refreshSidebar()
    emit('closeMobile')
    await router.push(`/music/playlists/${created.playlist.id}`)
  } else {
    const created = await videoApi.createShelf({ name })
    await videoStore.refreshSidebar()
    emit('closeMobile')
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
    emit('closeMobile')
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
    emit('closeMobile')
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
</script>

<template>
<div
  v-if="mobileMenuOpen"
  class="fixed inset-0 z-30 bg-black/45 md:hidden"
  @click="emit('closeMobile')"
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
      @click="emit('switchDomain', tab.kind)"
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
        @click="emit('openCustomizer')"
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
          @click="emit('openCustomizer')"
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
</template>

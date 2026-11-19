import { computed, ref, watch, type Component } from 'vue'
import { useRoute, useRouter } from 'vue-router'
import { useI18n } from 'vue-i18n'
import {
  LuBookmark,
  LuBookOpen,
  LuClock,
  LuDisc3,
  LuFilm,
  LuFolder,
  LuFolderHeart,
  LuHeart,
  LuHome,
  LuLayers,
  LuLibrary,
  LuListMusic,
  LuMic,
  LuMusic,
  LuPalette,
  LuRadio,
  LuSparkles,
  LuTag,
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

export function useWorkspaceSidebar(onCloseMobile: () => void) {
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
    onCloseMobile()
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
    onCloseMobile()
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
    onCloseMobile()
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
    onCloseMobile()
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
      onCloseMobile()
      await router.push({ path: '/comic/series', query: { shelf_id: created.id } })
    } else if (d === 'music') {
      const created = await musicApi.createPlaylist({ name })
      await musicStore.refreshSidebar()
      onCloseMobile()
      await router.push(`/music/playlists/${created.playlist.id}`)
    } else {
      const created = await videoApi.createShelf({ name })
      await videoStore.refreshSidebar()
      onCloseMobile()
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
      onCloseMobile()
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
      onCloseMobile()
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

  return {
    t,
    domainStore,
    primaryNavItems,
    activeSidebarGroups,
    creatingGroupId,
    newCollectionName,
    SIDEBAR_PREVIEW_LIMIT,
    isGroupFolded,
    toggleGroupFold,
    isGroupExpanded,
    toggleGroupExpanded,
    visibleGroupItems,
    toggleCreateForGroup,
    handleCreateCollection,
  }
}

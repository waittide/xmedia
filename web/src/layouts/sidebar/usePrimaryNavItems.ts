import { computed } from 'vue'
import { useRoute, useRouter } from 'vue-router'
import {
  LuBookOpen,
  LuClock,
  LuFilm,
  LuHeart,
  LuHome,
  LuLayers,
  LuMusic,
} from 'vue-icons-plus/lu'
import { useDomainStore } from '@/stores/domain'
import { useBookI18n } from '@/domains/book/i18n'
import { useCatalogStore, type NavSection as BookNavSection } from '@/domains/book/store'
import { useComicI18n } from '@/domains/comic/i18n'
import { useMusicI18n } from '@/domains/music/i18n'
import { useVideoI18n } from '@/domains/video/i18n'
import type { PrimaryNavItem } from './types'

export function usePrimaryNavItems(onNavigate?: () => void) {
  const route = useRoute()
  const router = useRouter()
  const { t: bookT } = useBookI18n()
  const { t: comicT } = useComicI18n()
  const { t: musicT } = useMusicI18n()
  const { t: videoT } = useVideoI18n()

  const domainStore = useDomainStore()
  const bookStore = useCatalogStore()

  function isBookSectionActive(section: BookNavSection, id = ''): boolean {
    if (route.name === 'book-detail' && section === 'all') return true
    if (route.name !== 'book-catalog') return false
    if (route.query.scope === 'online') return false
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
    onNavigate?.()
    bookStore.selectSection(section, id)
    const query: Record<string, string> = {}
    if (section !== 'all') query.section = section
    if (section === 'library' && id) query.library = id
    if (section === 'shelf' && id) query.shelf = id
    if (section === 'tag' && id) query.tag = id
    if (section === 'author' && id) query.author = id
    await router.push({ path: '/book/catalog', query })
  }

  const isAllComicSeriesActive = computed(() => {
    if (route.name === 'comic-series-detail') return true
    if (route.name !== 'comic-series-list') return false
    return (
      !route.query.library_id &&
      !route.query.shelf_id &&
      !route.query.tag &&
      !route.query.genre &&
      !route.query.writer &&
      !route.query.penciller
    )
  })

  const isAllMusicActive = computed(() => {
    if (route.name !== 'music-songs') return false
    if (route.query.tab === 'albums' || route.query.tab === 'artists') return false
    return !route.query.library && !route.query.genre && !route.query.tag
  })

  async function openMusicCatalog(extraQuery: Record<string, string> = {}): Promise<void> {
    onNavigate?.()
    const currentTab =
      route.name === 'music-songs' && typeof route.query.tab === 'string'
        ? route.query.tab
        : undefined
    const hasFilter = Boolean(extraQuery.library || extraQuery.genre || extraQuery.tag)
    let nextTab: string | undefined = currentTab
    if (!hasFilter) {
      nextTab = undefined
    } else if (currentTab === 'artists') {
      nextTab = undefined
    } else if (currentTab === 'albums' && extraQuery.tag) {
      nextTab = undefined
    }
    await router.push({
      path: '/music/songs',
      query: nextTab ? { tab: nextTab, ...extraQuery } : extraQuery,
    })
  }

  const isAllVideoActive = computed(() => {
    if (route.name !== 'video-catalog') return false
    if (route.query.tab === 'shows') return false
    return !route.query.library && !route.query.genre && !route.query.tag
  })

  async function openVideoCatalog(extraQuery: Record<string, string> = {}): Promise<void> {
    onNavigate?.()
    const hasFilter = Boolean(extraQuery.library || extraQuery.genre || extraQuery.tag)
    const tab =
      hasFilter && route.name === 'video-catalog' && typeof route.query.tab === 'string'
        ? route.query.tab
        : undefined
    await router.push({
      path: '/video/catalog',
      query: tab ? { tab, ...extraQuery } : extraQuery,
    })
  }

  const primaryNavItems = computed<PrimaryNavItem[]>(() => {
    const d = domainStore.activeDomain
    const go = (path: string) => {
      onNavigate?.()
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

  return {
    primaryNavItems,
    isBookSectionActive,
    openBookSection,
    openMusicCatalog,
    openVideoCatalog,
  }
}

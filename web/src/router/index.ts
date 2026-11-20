import { createRouter, createWebHistory } from 'vue-router'
import { setUnauthorizedHandler } from '@/api/client'
import { ensureBookLocale } from '@/domains/book/i18n'
import { ensureComicLocale } from '@/domains/comic/i18n'
import { ensureMusicLocale } from '@/domains/music/i18n'
import { ensureVideoLocale } from '@/domains/video/i18n'
import { useAuthStore } from '@/stores/auth'
import { useDomainStore } from '@/stores/domain'
import type { MediaKind } from '@/types/api'

function ensureDomainLocale(domain: MediaKind): Promise<void> {
  switch (domain) {
    case 'book':
      return ensureBookLocale()
    case 'comic':
      return ensureComicLocale()
    case 'music':
      return ensureMusicLocale()
    case 'video':
      return ensureVideoLocale()
  }
}

function resolvePreferredDomain(
  preferred: MediaKind,
  enabled: MediaKind[],
): MediaKind {
  if (enabled.includes(preferred)) return preferred
  return enabled[0] ?? 'book'
}

export const router = createRouter({
  history: createWebHistory(),
  routes: [
    {
      path: '/setup',
      name: 'setup',
      component: () => import('@/views/SetupView.vue'),
      meta: { public: true, fullscreen: true },
    },
    {
      path: '/login',
      name: 'login',
      component: () => import('@/views/LoginView.vue'),
      meta: { public: true, fullscreen: true },
    },
    // Standalone Fullscreen Readers & Cinema Player
    {
      path: '/book/read/:id',
      name: 'book-reader',
      component: () => import('@/domains/book/views/ReaderView.vue'),
      meta: { domain: 'book' as MediaKind, fullscreen: true },
    },
    {
      path: '/book/online/read',
      name: 'book-online-reader',
      component: () => import('@/domains/book/views/OnlineReaderView.vue'),
      meta: { domain: 'book' as MediaKind, fullscreen: true },
    },
    {
      path: '/comic/read/:chapterId',
      name: 'comic-reader',
      component: () => import('@/domains/comic/views/ReaderView.vue'),
      meta: { domain: 'comic' as MediaKind, fullscreen: true },
    },
    {
      path: '/video/watch/:id',
      name: 'video-player',
      component: () => import('@/domains/video/views/VideoPlayerView.vue'),
      meta: { domain: 'video' as MediaKind, fullscreen: true },
    },
    // Unified Workspace Shell
    {
      path: '/',
      component: () => import('@/layouts/WorkspaceLayout.vue'),
      children: [
        {
          path: '',
          name: 'root',
          redirect: () => {
            const auth = useAuthStore()
            const domainStore = useDomainStore()
            const target = resolvePreferredDomain(
              domainStore.activeDomain,
              auth.enabledModules,
            )
            return `/${target}`
          },
        },
        // 1. Ebooks Domain (/book)
        {
          path: 'book',
          name: 'book-home',
          component: () => import('@/domains/book/views/HomeView.vue'),
          meta: { domain: 'book' as MediaKind },
        },
        {
          path: 'book/catalog',
          name: 'book-catalog',
          component: () => import('@/domains/book/views/CatalogView.vue'),
          meta: { domain: 'book' as MediaKind },
        },
        {
          path: 'book/items/:id',
          name: 'book-detail',
          component: () => import('@/domains/book/views/BookDetailView.vue'),
          meta: { domain: 'book' as MediaKind },
        },
        {
          path: 'book/libraries',
          name: 'book-libraries-overview',
          component: () => import('@/domains/book/views/CollectionsOverviewView.vue'),
          meta: { domain: 'book' as MediaKind },
        },
        {
          path: 'book/shelves',
          name: 'book-shelves-overview',
          component: () => import('@/domains/book/views/CollectionsOverviewView.vue'),
          meta: { domain: 'book' as MediaKind },
        },
        {
          path: 'book/tags',
          name: 'book-tags-overview',
          component: () => import('@/domains/book/views/CollectionsOverviewView.vue'),
          meta: { domain: 'book' as MediaKind },
        },
        {
          path: 'book/authors',
          name: 'book-authors-overview',
          component: () => import('@/domains/book/views/CollectionsOverviewView.vue'),
          meta: { domain: 'book' as MediaKind },
        },
        {
          path: 'book/bookmarks',
          name: 'book-bookmarks',
          component: () => import('@/domains/book/views/BookmarksView.vue'),
          meta: { domain: 'book' as MediaKind },
        },
        {
          path: 'book/online',
          name: 'book-online',
          component: () => import('@/domains/book/views/OnlineBookSourcesView.vue'),
          meta: { domain: 'book' as MediaKind },
        },

        // 2. Comics Domain (/comic)
        {
          path: 'comic',
          name: 'comic-home',
          component: () => import('@/domains/comic/views/HomeView.vue'),
          meta: { domain: 'comic' as MediaKind },
        },
        {
          path: 'comic/series',
          name: 'comic-series-list',
          component: () => import('@/domains/comic/views/SeriesListView.vue'),
          props: { presetMode: 'all' },
          meta: { domain: 'comic' as MediaKind },
        },
        {
          path: 'comic/series/:id',
          name: 'comic-series-detail',
          component: () => import('@/domains/comic/views/SeriesDetailView.vue'),
          meta: { domain: 'comic' as MediaKind },
        },
        {
          path: 'comic/continue',
          name: 'comic-continue',
          component: () => import('@/domains/comic/views/SeriesListView.vue'),
          props: { presetMode: 'continue' },
          meta: { domain: 'comic' as MediaKind },
        },
        {
          path: 'comic/favorites',
          name: 'comic-favorites',
          component: () => import('@/domains/comic/views/SeriesListView.vue'),
          props: { presetMode: 'favorites' },
          meta: { domain: 'comic' as MediaKind },
        },
        {
          path: 'comic/libraries',
          name: 'comic-libraries',
          component: () => import('@/domains/comic/views/LibrariesView.vue'),
          meta: { domain: 'comic' as MediaKind },
        },
        {
          path: 'comic/shelves',
          name: 'comic-shelves',
          component: () => import('@/domains/comic/views/ShelvesView.vue'),
          meta: { domain: 'comic' as MediaKind },
        },
        {
          path: 'comic/tags',
          name: 'comic-tags',
          component: () => import('@/domains/comic/views/TagsView.vue'),
          meta: { domain: 'comic' as MediaKind },
        },
        {
          path: 'comic/bookmarks',
          name: 'comic-bookmarks',
          component: () => import('@/domains/comic/views/BookmarksView.vue'),
          meta: { domain: 'comic' as MediaKind },
        },

        // 3. Music Domain (/music)
        {
          path: 'music',
          name: 'music-home',
          component: () => import('@/domains/music/views/HomeView.vue'),
          meta: { domain: 'music' as MediaKind },
        },
        {
          path: 'music/songs',
          name: 'music-songs',
          component: () => import('@/domains/music/views/SongsView.vue'),
          meta: { domain: 'music' as MediaKind },
        },
        {
          path: 'music/albums',
          redirect: { path: '/music/songs', query: { tab: 'albums' } },
        },
        {
          path: 'music/albums/:id',
          name: 'music-album-detail',
          component: () => import('@/domains/music/views/AlbumDetailView.vue'),
          meta: { domain: 'music' as MediaKind },
        },
        {
          path: 'music/artists',
          redirect: { path: '/music/songs', query: { tab: 'artists' } },
        },
        {
          path: 'music/artists/:id',
          name: 'music-artist-detail',
          component: () => import('@/domains/music/views/ArtistDetailView.vue'),
          meta: { domain: 'music' as MediaKind },
        },
        {
          path: 'music/continue',
          name: 'music-continue',
          component: () => import('@/domains/music/views/ContinueView.vue'),
          meta: { domain: 'music' as MediaKind },
        },
        {
          path: 'music/favorites',
          name: 'music-favorites',
          component: () => import('@/domains/music/views/FavoritesView.vue'),
          meta: { domain: 'music' as MediaKind },
        },
        {
          path: 'music/libraries',
          name: 'music-libraries-overview',
          component: () => import('@/domains/music/views/CollectionsOverviewView.vue'),
          meta: { domain: 'music' as MediaKind },
        },
        {
          path: 'music/playlists',
          name: 'music-playlists-overview',
          component: () => import('@/domains/music/views/CollectionsOverviewView.vue'),
          meta: { domain: 'music' as MediaKind },
        },
        {
          path: 'music/playlists/:id',
          name: 'music-playlist-detail',
          component: () => import('@/domains/music/views/PlaylistDetailView.vue'),
          meta: { domain: 'music' as MediaKind },
        },
        {
          path: 'music/genres',
          name: 'music-genres-overview',
          component: () => import('@/domains/music/views/CollectionsOverviewView.vue'),
          meta: { domain: 'music' as MediaKind },
        },
        {
          path: 'music/tags',
          name: 'music-tags-overview',
          component: () => import('@/domains/music/views/CollectionsOverviewView.vue'),
          meta: { domain: 'music' as MediaKind },
        },
        {
          path: 'music/search',
          name: 'music-search',
          component: () => import('@/domains/music/views/SearchView.vue'),
          meta: { domain: 'music' as MediaKind },
        },

        // 4. Video Domain (/video)
        {
          path: 'video',
          name: 'video-home',
          component: () => import('@/domains/video/views/HomeView.vue'),
          meta: { domain: 'video' as MediaKind },
        },
        {
          path: 'video/catalog',
          name: 'video-catalog',
          component: () => import('@/domains/video/views/CatalogView.vue'),
          meta: { domain: 'video' as MediaKind },
        },
        {
          path: 'video/continue',
          name: 'video-continue',
          component: () => import('@/domains/video/views/ContinueView.vue'),
          meta: { domain: 'video' as MediaKind },
        },
        {
          path: 'video/favorites',
          name: 'video-favorites',
          component: () => import('@/domains/video/views/FavoritesView.vue'),
          meta: { domain: 'video' as MediaKind },
        },
        {
          path: 'video/libraries',
          name: 'video-libraries-overview',
          component: () => import('@/domains/video/views/CollectionsOverviewView.vue'),
          meta: { domain: 'video' as MediaKind },
        },
        {
          path: 'video/shelves',
          name: 'video-shelves-overview',
          component: () => import('@/domains/video/views/CollectionsOverviewView.vue'),
          meta: { domain: 'video' as MediaKind },
        },
        {
          path: 'video/shelves/:id',
          name: 'video-shelf-detail',
          component: () => import('@/domains/video/views/ShelfDetailView.vue'),
          meta: { domain: 'video' as MediaKind },
        },
        {
          path: 'video/tags',
          name: 'video-tags-overview',
          component: () => import('@/domains/video/views/CollectionsOverviewView.vue'),
          meta: { domain: 'video' as MediaKind },
        },
        {
          path: 'video/genres',
          name: 'video-genres-overview',
          component: () => import('@/domains/video/views/CollectionsOverviewView.vue'),
          meta: { domain: 'video' as MediaKind },
        },
        {
          path: 'video/shows',
          redirect: { path: '/video/catalog', query: { tab: 'shows' } },
        },
        {
          path: 'video/shows/:id',
          name: 'video-show-detail',
          component: () => import('@/domains/video/views/ShowDetailView.vue'),
          meta: { domain: 'video' as MediaKind },
        },

        // Unified Settings
        {
          path: 'settings',
          name: 'settings',
          component: () => import('@/views/SettingsView.vue'),
        },
        // Legacy path aliases
        { path: 'books', redirect: '/book' },
        { path: 'comics', redirect: '/comic' },
        { path: 'videos', redirect: '/video' },
      ],
    },
    {
      path: '/:pathMatch(.*)*',
      redirect: '/',
    },
  ],
})

setUnauthorizedHandler(() => {
  const auth = useAuthStore()
  auth.clearLocalSession()
  if (router.currentRoute.value.name !== 'login' && router.currentRoute.value.name !== 'setup') {
    void router.replace('/login')
  }
})

router.beforeEach(async (to) => {
  const auth = useAuthStore()
  const domainStore = useDomainStore()
  await auth.bootstrap()

  if (auth.needsSetup) {
    if (to.name !== 'setup') return { name: 'setup' }
    return true
  }

  if (to.name === 'setup') {
    return auth.isAuthenticated ? '/' : { name: 'login' }
  }

  if (!auth.isAuthenticated && !to.meta.public) {
    return { name: 'login' }
  }

  if (auth.isAuthenticated && to.name === 'login') {
    return '/'
  }

  const routeDomain = to.meta.domain as MediaKind | undefined
  if (routeDomain && !auth.hasModule(routeDomain)) {
    const fallback = resolvePreferredDomain(domainStore.activeDomain, auth.enabledModules)
    if (fallback !== routeDomain) {
      return `/${fallback}`
    }
  }

  domainStore.syncFromPath(to.fullPath)

  if (routeDomain) {
    await ensureDomainLocale(routeDomain)
  } else if (to.name === 'settings') {
    const domains = new Set<MediaKind>(['book', 'comic', domainStore.activeDomain])
    await Promise.all([...domains].map((d) => ensureDomainLocale(d)))
  }

  return true
})

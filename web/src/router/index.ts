import {
  createRouter,
  createWebHistory,
  type LocationQuery,
  type LocationQueryRaw,
} from 'vue-router'
import { setUnauthorizedHandler } from '@/api/client'
import { ensureBookLocale } from '@/domains/book/i18n'
import { ensureComicLocale } from '@/domains/comic/i18n'
import { ensureMusicLocale } from '@/domains/music/i18n'
import { ensureVideoLocale } from '@/domains/video/i18n'
import {
  legacyRedirectRoutes,
  migrateBookOnlineReadQuery,
  resolveLegacySettingsRedirect,
} from '@/router/legacyRedirects'
import { useAppClientStore } from '@/stores/appServers'
import { useAuthStore } from '@/stores/auth'
import { useDomainStore } from '@/stores/domain'
import type { MediaKind } from '@/types/api'

const DESKTOP_SETTINGS_MEDIA_QUERY = '(min-width: 840px)'

export function ensureDomainLocale(domain: MediaKind): Promise<void> {
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
): MediaKind | null {
  if (enabled.includes(preferred)) return preferred
  return enabled[0] ?? null
}

function firstQueryString(query: LocationQuery, keys: readonly string[]): string {
  for (const key of keys) {
    const raw = query[key]
    if (typeof raw === 'string' && raw.trim()) return raw.trim()
    if (Array.isArray(raw)) {
      const hit = raw.find((v): v is string => typeof v === 'string' && v.trim().length > 0)
      if (hit) return hit.trim()
    }
  }
  return ''
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

    // Standalone Fullscreen Readers & Players (static `/play/online` MUST precede `/play/:id`)
    {
      path: '/book/play/online',
      name: 'book-online-reader',
      component: () => import('@/domains/book/views/OnlineReaderView.vue'),
      meta: { domain: 'book' as MediaKind, fullscreen: true },
    },
    {
      path: '/book/play/:id',
      name: 'book-reader',
      component: () => import('@/domains/book/views/ReaderView.vue'),
      meta: { domain: 'book' as MediaKind, fullscreen: true },
    },
    {
      path: '/comic/play/online',
      name: 'comic-online-reader',
      component: () => import('@/domains/comic/views/OnlineComicReaderView.vue'),
      meta: { domain: 'comic' as MediaKind, fullscreen: true },
    },
    {
      path: '/comic/play/:id',
      name: 'comic-reader',
      component: () => import('@/domains/comic/views/ReaderView.vue'),
      meta: { domain: 'comic' as MediaKind, fullscreen: true },
    },
    {
      path: '/video/play/online',
      name: 'video-online-player',
      component: () => import('@/domains/video/views/OnlineVideoPlayerView.vue'),
      meta: { domain: 'video' as MediaKind, fullscreen: true },
    },
    {
      path: '/video/play/:id',
      name: 'video-player',
      component: () => import('@/domains/video/views/VideoPlayerView.vue'),
      meta: { domain: 'video' as MediaKind, fullscreen: true },
    },

    // Unified Workspace Shell — Canonical Isomorphic IA (§1.1)
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
            return target ? `/${target}` : '/settings'
          },
        },

        // 1. Book Domain (/book)
        {
          path: 'book',
          name: 'book-home',
          component: () => import('@/domains/book/views/HomeView.vue'),
          meta: { domain: 'book' as MediaKind },
        },
        {
          path: 'book/library',
          name: 'book-library',
          component: () => import('@/domains/book/views/CatalogView.vue'),
          meta: { domain: 'book' as MediaKind },
        },
        {
          path: 'book/history',
          name: 'book-history',
          component: () => import('@/views/DomainHistoryView.vue'),
          meta: { domain: 'book' as MediaKind },
        },
        {
          path: 'book/settings',
          name: 'book-settings',
          component: () => import('@/views/DomainSettingsView.vue'),
          meta: { domain: 'book' as MediaKind },
        },
        {
          // Placeholder until Wave 4B introduces a dedicated online book detail view
          path: 'book/item/online',
          name: 'book-online-detail',
          redirect: (to) => {
            const query = migrateBookOnlineReadQuery(to.query)
            return Object.keys(query).length > 0
              ? { path: '/book/play/online', query }
              : { path: '/book/play/online' }
          },
          meta: { domain: 'book' as MediaKind },
        },
        {
          path: 'book/item/:id',
          name: 'book-detail',
          component: () => import('@/domains/book/views/BookDetailView.vue'),
          meta: { domain: 'book' as MediaKind },
        },

        // 2. Comic Domain (/comic)
        {
          path: 'comic',
          name: 'comic-home',
          component: () => import('@/domains/comic/views/HomeView.vue'),
          meta: { domain: 'comic' as MediaKind },
        },
        {
          path: 'comic/library',
          name: 'comic-library',
          component: () => import('@/domains/comic/views/SeriesListView.vue'),
          props: (route) => {
            if (route.query.scope === 'online') return { presetMode: 'all' }
            if (route.query.tab === 'continue') return { presetMode: 'continue' }
            if (route.query.tab === 'favorites') return { presetMode: 'favorites' }
            return { presetMode: 'all' }
          },
          meta: { domain: 'comic' as MediaKind },
        },
        {
          path: 'comic/history',
          name: 'comic-history',
          component: () => import('@/views/DomainHistoryView.vue'),
          meta: { domain: 'comic' as MediaKind },
        },
        {
          path: 'comic/settings',
          name: 'comic-settings',
          component: () => import('@/views/DomainSettingsView.vue'),
          meta: { domain: 'comic' as MediaKind },
        },
        {
          path: 'comic/item/online',
          name: 'comic-online-detail',
          component: () => import('@/domains/comic/views/OnlineComicDetailView.vue'),
          meta: { domain: 'comic' as MediaKind },
        },
        {
          path: 'comic/item/:id',
          name: 'comic-series-detail',
          component: () => import('@/domains/comic/views/SeriesDetailView.vue'),
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
          path: 'music/library',
          name: 'music-library',
          component: () => import('@/domains/music/views/SongsView.vue'),
          meta: { domain: 'music' as MediaKind },
        },
        {
          path: 'music/history',
          name: 'music-history',
          component: () => import('@/views/DomainHistoryView.vue'),
          meta: { domain: 'music' as MediaKind },
        },
        {
          path: 'music/settings',
          name: 'music-settings',
          component: () => import('@/views/DomainSettingsView.vue'),
          meta: { domain: 'music' as MediaKind },
        },
        {
          // Placeholder until Wave 4B introduces online track/album detail resolution
          path: 'music/item/online',
          name: 'music-online-detail',
          redirect: (to) => {
            const query: LocationQueryRaw = { scope: 'online' }
            const source = firstQueryString(to.query, ['source', 'sourceId', 'source_id'])
            const q = firstQueryString(to.query, ['q', 'title', 'name'])
            if (source) query.source = source
            if (q) query.q = q
            return { path: '/music/library', query }
          },
          meta: { domain: 'music' as MediaKind },
        },
        {
          path: 'music/item/:id',
          name: 'music-album-detail',
          component: () => import('@/domains/music/views/AlbumDetailView.vue'),
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
          path: 'video/library',
          name: 'video-library',
          component: () => import('@/domains/video/views/CatalogView.vue'),
          meta: { domain: 'video' as MediaKind },
        },
        {
          path: 'video/history',
          name: 'video-history',
          component: () => import('@/views/DomainHistoryView.vue'),
          meta: { domain: 'video' as MediaKind },
        },
        {
          path: 'video/settings',
          name: 'video-settings',
          component: () => import('@/views/DomainSettingsView.vue'),
          meta: { domain: 'video' as MediaKind },
        },
        {
          path: 'video/item/online',
          name: 'video-online-detail',
          component: () => import('@/domains/video/views/OnlineVideoDetailView.vue'),
          meta: { domain: 'video' as MediaKind },
        },
        {
          path: 'video/item/:id',
          name: 'video-show-detail',
          component: () => import('@/domains/video/views/ShowDetailView.vue'),
          meta: { domain: 'video' as MediaKind },
        },

        // 5. Unified Single-Level Settings (/settings & /settings/:section)
        {
          path: 'settings',
          name: 'settings',
          component: () => import('@/views/SettingsView.vue'),
          beforeEnter: (to) => {
            const legacyTarget = resolveLegacySettingsRedirect(to)
            if (legacyTarget) return legacyTarget
            if (
              typeof window !== 'undefined' &&
              typeof window.matchMedia === 'function' &&
              window.matchMedia(DESKTOP_SETTINGS_MEDIA_QUERY).matches
            ) {
              return { path: '/settings/appearance', replace: true }
            }
            return true
          },
        },
        {
          path: 'settings/:section(appearance|account|about)',
          name: 'settings-section',
          component: () => import('@/views/SettingsView.vue'),
        },
      ],
    },

    // Centralized legacy redirects & `/:domain/discover` alias (§1.4)
    ...legacyRedirectRoutes,

    {
      path: '/:pathMatch(.*)*',
      redirect: '/',
    },
  ],
})

setUnauthorizedHandler(() => {
  const appClient = useAppClientStore()
  const auth = useAuthStore()
  auth.clearLocalSession()
  if (appClient.isStandaloneApp) {
    void appClient
      .fetchAutoSession()
      .then((session) => {
        auth.applySession(session)
      })
      .catch(() => {
        // ignore transient auto-session errors
      })
    return
  }
  if (
    router.currentRoute.value.name !== 'login' &&
    router.currentRoute.value.name !== 'setup'
  ) {
    void router.replace('/login')
  }
})

router.beforeEach(async (to) => {
  const auth = useAuthStore()
  const domainStore = useDomainStore()
  const appClient = useAppClientStore()

  await appClient.bootstrap()

  if (appClient.isStandaloneApp) {
    try {
      await auth.bootstrap()
    } catch {
      // transient startup error; allow auto-session attempt below
    }

    if (!auth.isAuthenticated) {
      try {
        const session = await appClient.fetchAutoSession()
        auth.applySession(session)
        if (!auth.initialized) {
          await auth.bootstrap().catch(() => {})
        }
      } catch {
        if (!to.meta.public) {
          return { name: 'login' }
        }
      }
    }

    if (to.name === 'login' || to.name === 'setup') {
      return '/'
    }
  } else {
    try {
      await auth.bootstrap()
    } catch {
      if (!auth.isAuthenticated && !to.meta.public) {
        return { name: 'login' }
      }
    }

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
  }

  const isSettingsTarget = to.name === 'settings' || to.name === 'settings-section'
  const routeDomain = to.meta.domain as MediaKind | undefined
  if (routeDomain && !auth.hasModule(routeDomain)) {
    const fallback = resolvePreferredDomain(domainStore.activeDomain, auth.enabledModules)
    if (fallback && fallback !== routeDomain) {
      return `/${fallback}`
    }
    if (!fallback && !isSettingsTarget) {
      return { name: 'settings' }
    }
  }

  if (routeDomain) {
    await ensureDomainLocale(routeDomain)
  } else if (isSettingsTarget) {
    const domains = new Set<MediaKind>([
      'book',
      'comic',
      domainStore.activeDomain,
      ...auth.enabledModules,
    ])
    await Promise.all([...domains].map((d) => ensureDomainLocale(d)))
  }

  domainStore.syncFromPath(to.fullPath)

  return true
})

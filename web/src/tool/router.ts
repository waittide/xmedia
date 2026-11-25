import {
  createRouter,
  createWebHashHistory,
  type LocationQueryRaw,
  type RouteLocationNormalizedLoaded,
  type RouteLocationRaw,
} from 'vue-router'
import type { MediaKind } from '@/types/api'
import type { ToolCompletenessFilter } from './completeness'

export type PrimaryNavKey = 'home' | 'items' | 'settings'
export type ToolSettingsTab = 'appearance' | 'scrapers' | 'roots' | 'templates'

export const VALID_DOMAINS: MediaKind[] = ['book', 'comic', 'music', 'video']
export const VALID_SETTINGS_TABS: ToolSettingsTab[] = [
  'appearance',
  'scrapers',
  'roots',
  'templates',
]
export const VALID_FILTERS: ToolCompletenessFilter[] = [
  'all',
  'complete',
  'incomplete',
  'missing_cover',
  'missing_summary',
  'missing_tag',
]

export function loadSavedDomain(): MediaKind {
  if (typeof window === 'undefined') return 'book'
  const saved = localStorage.getItem('xmedia_tool.active_domain') as MediaKind | null
  if (saved && VALID_DOMAINS.includes(saved)) return saved
  return 'book'
}

export function loadSavedNav(): 'home' | 'items' {
  if (typeof window === 'undefined') return 'home'
  return localStorage.getItem('xmedia_tool.active_nav') === 'items' ? 'items' : 'home'
}

export interface ParsedToolRoute {
  domain?: MediaKind
  nav: PrimaryNavKey
  workbench: boolean
  settingsTab: ToolSettingsTab
  rootId: string
  relPath: string
  filter: ToolCompletenessFilter
  q: string
  item: string | null
}

export function parseToolRoute(route: RouteLocationNormalizedLoaded): ParsedToolRoute {
  const rawDomain = typeof route.params.domain === 'string' ? route.params.domain : ''
  const domain = VALID_DOMAINS.includes(rawDomain as MediaKind)
    ? (rawDomain as MediaKind)
    : undefined

  const workbench = route.name === 'tool-scrape'
  const nav: PrimaryNavKey =
    route.name === 'tool-settings'
      ? 'settings'
      : route.name === 'tool-items' || workbench
        ? 'items'
        : 'home'

  const rawTab = typeof route.params.tab === 'string' ? route.params.tab : ''
  const settingsTab: ToolSettingsTab = VALID_SETTINGS_TABS.includes(rawTab as ToolSettingsTab)
    ? (rawTab as ToolSettingsTab)
    : 'appearance'

  const rootId = typeof route.query.root === 'string' ? route.query.root : ''
  const relPath = typeof route.query.path === 'string' ? route.query.path : ''

  const rawFilter = typeof route.query.filter === 'string' ? route.query.filter : ''
  const filter: ToolCompletenessFilter =
    route.name === 'tool-items' && VALID_FILTERS.includes(rawFilter as ToolCompletenessFilter)
      ? (rawFilter as ToolCompletenessFilter)
      : 'all'

  const q = route.name === 'tool-items' && typeof route.query.q === 'string' ? route.query.q : ''
  const item =
    typeof route.query.item === 'string' && route.query.item.length > 0 ? route.query.item : null

  return {
    domain,
    nav,
    workbench,
    settingsTab,
    rootId,
    relPath,
    filter,
    q,
    item,
  }
}

export interface BuildToolRouteInput {
  domain: MediaKind
  nav: PrimaryNavKey
  workbench: boolean
  rootId: string
  relPath: string
  filter: ToolCompletenessFilter
  q: string
  item: string | null
  settingsTab: ToolSettingsTab
}

export function buildToolRoute(state: BuildToolRouteInput): RouteLocationRaw {
  if (state.nav === 'settings') {
    return {
      name: 'tool-settings',
      params: { tab: state.settingsTab },
    }
  }

  if (state.workbench) {
    const query: LocationQueryRaw = {}
    if (state.rootId) query.root = state.rootId
    if (state.relPath) query.path = state.relPath
    if (state.item) query.item = state.item
    return {
      name: 'tool-scrape',
      params: { domain: state.domain },
      query,
    }
  }

  if (state.nav === 'items') {
    const query: LocationQueryRaw = {}
    if (state.rootId) query.root = state.rootId
    if (state.relPath) query.path = state.relPath
    if (state.filter !== 'all') query.filter = state.filter
    const trimmedQ = state.q.trim()
    if (trimmedQ) query.q = trimmedQ
    if (state.item) query.item = state.item
    return {
      name: 'tool-items',
      params: { domain: state.domain },
      query,
    }
  }

  const query: LocationQueryRaw = {}
  if (state.rootId) query.root = state.rootId
  if (state.relPath) query.path = state.relPath
  if (state.item) query.item = state.item
  return {
    name: 'tool-home',
    params: { domain: state.domain },
    query,
  }
}

export const toolRouter = createRouter({
  history: createWebHashHistory(),
  routes: [
    {
      path: '/',
      redirect: () => `/${loadSavedDomain()}/${loadSavedNav()}`,
    },
    {
      path: '/:domain(book|comic|music|video)',
      redirect: (to) => `/${String(to.params.domain)}/home`,
    },
    {
      path: '/:domain(book|comic|music|video)/home',
      name: 'tool-home',
      component: () => import('./components/ToolHomeView.vue'),
    },
    {
      path: '/:domain(book|comic|music|video)/items',
      name: 'tool-items',
      component: () => import('./components/ToolItemsBrowser.vue'),
    },
    {
      path: '/:domain(book|comic|music|video)/scrape',
      name: 'tool-scrape',
      component: () => import('./components/scrape/ScrapeWorkbenchView.vue'),
    },
    {
      path: '/settings/:tab?',
      name: 'tool-settings',
      component: () => import('./components/ToolSettingsView.vue'),
    },
    {
      path: '/:pathMatch(.*)*',
      redirect: () => `/${loadSavedDomain()}/home`,
    },
  ],
})

import type { Component } from 'vue'
import type {
  LocationQuery,
  RouteLocationNormalizedLoaded,
  RouteLocationRaw,
  Router,
} from 'vue-router'
import {
  LuBookOpen,
  LuClapperboard,
  LuDisc3,
  LuInfo,
  LuKeyRound,
  LuLayers,
  LuPalette,
} from 'vue-icons-plus/lu'
import { isMediaKind, libraryRoute, type LibraryRouteParams } from '@/sources'
import type { MediaKind } from '@/types/api'

export type SettingsSection = 'appearance' | 'account' | 'about'

export interface SettingsSectionDef {
  id: SettingsSection
  labelKey: string
  summaryKey: string
  icon: Component
  adminOnly?: boolean
  /** `data-settings-tab` attribute value */
  testId: string
}

export const SETTINGS_SECTIONS: readonly SettingsSectionDef[] = [
  {
    id: 'appearance',
    labelKey: 'settings.navAppearance',
    summaryKey: 'settings.summaryAppearance',
    icon: LuPalette,
    testId: 'appearance',
  },
  {
    id: 'account',
    labelKey: 'settings.navAccountSecurity',
    summaryKey: 'settings.summaryAccount',
    icon: LuKeyRound,
    testId: 'account',
  },
  {
    id: 'about',
    labelKey: 'settings.navAbout',
    summaryKey: 'settings.summaryAbout',
    icon: LuInfo,
    testId: 'about',
  },
]

const LEGACY_DOMAIN_CATALOG_PATHS: Record<MediaKind, string> = {
  book: '/book/library',
  comic: '/comic/series',
  music: '/music/songs',
  video: '/video/catalog',
}

export function resolveLibraryRoute(
  router: Router,
  domain: MediaKind,
  params?: LibraryRouteParams,
): RouteLocationRaw {
  const hasCanonicalLibrary = router
    .getRoutes()
    .some(
      (r) =>
        r.name === `${domain}-library` ||
        r.path === `/${domain}/library` ||
        (r.path.includes(':domain') && r.path.endsWith('/library')),
    )

  if (hasCanonicalLibrary) {
    return libraryRoute(domain, params)
  }

  const query: Record<string, string> = {}
  if (params?.scope && params.scope !== 'local') {
    query.scope = params.scope
    if (params.sourceId) query.source = params.sourceId
  }
  if (params?.library) {
    if (domain === 'comic') {
      query.library_id = params.library
    } else {
      query.library = params.library
    }
  }
  if (params?.q?.trim()) {
    query.q = params.q.trim()
  }

  const path = LEGACY_DOMAIN_CATALOG_PATHS[domain]
  return Object.keys(query).length > 0 ? { path, query } : { path }
}

export const DOMAIN_ICONS: Record<MediaKind, Component> = {
  book: LuBookOpen,
  comic: LuLayers,
  music: LuDisc3,
  video: LuClapperboard,
}

export const SOURCES_ADVANCED_STORAGE_KEY = 'xmedia.sources.advanced'

const SECTION_SET = new Set<string>(SETTINGS_SECTIONS.map((s) => s.id))

export function isSettingsSection(value: unknown): value is SettingsSection {
  return typeof value === 'string' && SECTION_SET.has(value)
}

export function readSingleQuery(query: LocationQuery, key: string): string {
  const raw = query[key]
  if (typeof raw === 'string') return raw.trim()
  if (Array.isArray(raw)) {
    const first = raw.find((v): v is string => typeof v === 'string' && v.trim().length > 0)
    return first ? first.trim() : ''
  }
  return ''
}

export function hasExplicitSectionInRoute(route: RouteLocationNormalizedLoaded): boolean {
  const paramSection = typeof route.params.section === 'string' ? route.params.section : ''
  if (isSettingsSection(paramSection)) return true
  const rawTab = readSingleQuery(route.query, 'tab')
  return rawTab.length > 0
}

export function resolveSectionFromRoute(
  route: RouteLocationNormalizedLoaded,
): SettingsSection {
  const paramSection = typeof route.params.section === 'string' ? route.params.section : ''
  if (isSettingsSection(paramSection)) {
    return paramSection
  }

  const rawTab = readSingleQuery(route.query, 'tab')
  if (rawTab === 'general' || rawTab === 'about') {
    return 'about'
  }
  if (rawTab === 'account' || rawTab === 'profile' || rawTab === 'users') {
    return 'account'
  }
  return 'appearance'
}

export function resolveDomainFromQuery(
  route: RouteLocationNormalizedLoaded,
  enabledModules: MediaKind[],
  fallbackDomain: MediaKind,
): MediaKind {
  const rawDomain = readSingleQuery(route.query, 'domain')
  if (isMediaKind(rawDomain) && enabledModules.includes(rawDomain)) {
    return rawDomain
  }
  if (enabledModules.includes(fallbackDomain)) {
    return fallbackDomain
  }
  return enabledModules[0] ?? 'book'
}

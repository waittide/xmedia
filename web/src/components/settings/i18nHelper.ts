import { useI18n } from 'vue-i18n'
import type { RouteLocationRaw, Router } from 'vue-router'
import { libraryRoute, type LibraryRouteParams } from '@/sources'
import type { MediaKind } from '@/types/api'

const LEGACY_DOMAIN_CATALOG_PATHS: Record<MediaKind, string> = {
  book: '/book/catalog',
  comic: '/comic/series',
  music: '/music/songs',
  video: '/video/catalog',
}

export function useSettingsI18n() {
  const { t, te, locale } = useI18n()

  function tr(
    key: string,
    zhFallback: string,
    enFallback: string,
    params?: Record<string, unknown>,
  ): string {
    if (te(key)) {
      return params ? t(key, params) : t(key)
    }
    const template = locale.value === 'zh-CN' ? zhFallback : enFallback
    if (!params) return template
    return template.replace(/\{(\w+)\}/g, (_, k: string) =>
      params[k] !== undefined && params[k] !== null ? String(params[k]) : `{${k}}`,
    )
  }

  return { t, te, locale, tr }
}

/**
 * Resolves a domain library route that works seamlessly both before and after
 * canonical `/:domain/library` routes are mounted in `router/index.ts`.
 */
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
        r.path.includes(':domain') && r.path.endsWith('/library'),
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

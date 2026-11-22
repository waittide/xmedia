import type {
  LocationQuery,
  LocationQueryRaw,
  LocationQueryValue,
  RouteLocationNormalized,
  RouteLocationRaw,
  RouteRecordRaw,
} from 'vue-router'
import { useAppClientStore } from '@/stores/appServers'
import { isMediaKind } from '@/sources/routes'
import type { MediaKind } from '@/types/api'

/**
 * Planned removal milestone for legacy redirect entries (§1.4).
 * Keep all legacy path/query compatibility mappings centralized here so they can
 * be audited and retired cleanly once external bookmarks & clients migrate.
 */
export const LEGACY_REDIRECT_REMOVAL_TARGET = 'v2.0'

const BASE_ALLOWED_QUERY_KEYS = [
  'scope',
  'source',
  'q',
  'page',
  'tab',
  'collection',
  'view',
  'library',
  'sort',
  'order',
] as const

const QUERY_KEY_RENAMES: ReadonlyArray<readonly [legacyKey: string, canonicalKey: string]> = [
  ['sort_by', 'sort'],
  ['sort_order', 'order'],
  ['library_id', 'library'],
  ['shelf_id', 'collection'],
  ['shelf', 'collection'],
  ['source_id', 'source'],
  ['sourceId', 'source'],
]

const BOOK_FILTER_KEYS = ['author', 'tag', 'format', 'missing'] as const
const COMIC_FILTER_KEYS = ['writer', 'penciller', 'genre', 'tag'] as const
const MUSIC_FILTER_KEYS = ['artist', 'genre', 'tag'] as const
const VIDEO_FILTER_KEYS = ['genre', 'tag', 'kind'] as const

const VALID_COMIC_TAG_SECTIONS = new Set(['genres', 'writers', 'pencillers', 'tags'])
const VALID_LIBRARY_TABS = new Set(['all', 'continue', 'favorites', 'collections'])

export interface MigrateQueryOptions {
  /** Additional domain-specific filter keys to preserve (e.g. `author`, `genre`, `tag`). */
  extraKeys?: readonly string[]
}

function readSingle(query: LocationQuery, keys: readonly string[]): string {
  for (const key of keys) {
    const raw = query[key]
    if (typeof raw === 'string' && raw.trim()) {
      return raw.trim()
    }
    if (Array.isArray(raw)) {
      for (const item of raw) {
        if (typeof item === 'string' && item.trim()) {
          return item.trim()
        }
      }
    }
  }
  return ''
}

function readParam(raw: string | string[] | undefined): string {
  if (typeof raw === 'string') return raw
  if (Array.isArray(raw)) return raw[0] ?? ''
  return ''
}

function normalizeQueryValue(
  raw: LocationQueryValue | LocationQueryValue[] | undefined,
): LocationQueryValue | LocationQueryValue[] | undefined {
  if (raw === undefined || raw === null) return undefined
  if (Array.isArray(raw)) {
    const clean = raw
      .filter((v): v is string => typeof v === 'string' && v.trim().length > 0)
      .map((v) => v.trim())
    if (clean.length === 0) return undefined
    return clean.length === 1 ? clean[0] : clean
  }
  const trimmed = raw.trim()
  return trimmed.length > 0 ? trimmed : undefined
}

function compactQuery(query: LocationQueryRaw): LocationQueryRaw | undefined {
  return Object.keys(query).length > 0 ? query : undefined
}

/**
 * Normalizes legacy query parameters to the canonical §1.2 schema:
 * - `sort_by` -> `sort`
 * - `sort_order` -> `order`
 * - `library_id` -> `library`
 * - `shelf_id` | `shelf` -> `collection`
 * - `source_id` | `sourceId` -> `source`
 * Preserves canonical keys (`scope`, `source`, `q`, `page`, `tab`, `collection`,
 * `view`, `library`, `sort`, `order`) plus `options.extraKeys`, dropping unknown keys.
 */
export function migrateQuery(
  query: LocationQuery,
  options: MigrateQueryOptions = {},
): LocationQueryRaw {
  const allowed = new Set<string>(BASE_ALLOWED_QUERY_KEYS)
  if (options.extraKeys) {
    for (const k of options.extraKeys) {
      allowed.add(k)
    }
  }

  const next: LocationQueryRaw = {}

  // 1. Copy already-canonical keys first (highest precedence)
  for (const key of allowed) {
    const normalized = normalizeQueryValue(query[key])
    if (normalized !== undefined) {
      next[key] = normalized
    }
  }

  // 2. Apply legacy renames without overriding already-present canonical keys
  for (const [legacyKey, canonicalKey] of QUERY_KEY_RENAMES) {
    if (!allowed.has(canonicalKey) || next[canonicalKey] !== undefined) continue
    const normalized = normalizeQueryValue(query[legacyKey])
    if (normalized !== undefined) {
      next[canonicalKey] = normalized
    }
  }

  return next
}

/** Migrates `/book/online/read` query parameters to `/book/play/online` (§1.3). */
export function migrateBookOnlineReadQuery(query: LocationQuery): LocationQueryRaw {
  const next: LocationQueryRaw = {}
  const source = readSingle(query, ['source', 'sourceId', 'source_id'])
  const ref = readSingle(query, ['ref', 'bookUrl', 'book_url'])
  const toc = readSingle(query, ['toc', 'tocUrl', 'toc_url'])
  const chapter = readSingle(query, ['chapter'])
  const title = readSingle(query, ['title', 'name'])
  const author = readSingle(query, ['author'])
  const cover = readSingle(query, ['cover', 'coverUrl', 'cover_url'])
  const sourceName = readSingle(query, ['sourceName', 'source_name'])
  const shelfId = readSingle(query, ['shelfId', 'shelf_id'])

  if (source) next.source = source
  if (ref) next.ref = ref
  if (toc) next.toc = toc
  if (chapter) next.chapter = chapter
  if (title) next.title = title
  if (author) next.author = author
  if (cover) next.cover = cover
  if (sourceName) next.sourceName = sourceName
  if (shelfId) next.shelfId = shelfId
  return next
}

/** Migrates `/comic/online/detail` query parameters to `/comic/item/online` (§1.3). */
export function migrateComicOnlineDetailQuery(query: LocationQuery): LocationQueryRaw {
  const next: LocationQueryRaw = {}
  const source = readSingle(query, ['source', 'sourceId', 'source_id'])
  const ref = readSingle(query, ['ref', 'comicUrl', 'comic_url'])
  const title = readSingle(query, ['title', 'name'])
  const cover = readSingle(query, ['cover', 'coverUrl', 'cover_url'])
  const sourceName = readSingle(query, ['sourceName', 'source_name'])
  const author = readSingle(query, ['author'])
  const toc = readSingle(query, ['toc', 'tocUrl', 'toc_url'])

  if (source) next.source = source
  if (ref) next.ref = ref
  if (title) next.title = title
  if (cover) next.cover = cover
  if (sourceName) next.sourceName = sourceName
  if (author) next.author = author
  if (toc) next.toc = toc
  return next
}

/** Migrates `/comic/online/read` query parameters to `/comic/play/online` (§1.3). */
export function migrateComicOnlineReadQuery(query: LocationQuery): LocationQueryRaw {
  const next: LocationQueryRaw = {}
  const source = readSingle(query, ['source', 'sourceId', 'source_id'])
  const ref = readSingle(query, ['ref', 'comicUrl', 'comic_url'])
  const chapter = readSingle(query, ['chapter', 'chapterUrl', 'chapter_url'])
  const chapterIndex = readSingle(query, ['chapterIndex', 'chapter_index'])
  const toc = readSingle(query, ['toc', 'tocUrl', 'toc_url'])
  const page = readSingle(query, ['page'])
  const title = readSingle(query, ['title', 'name'])
  const sourceName = readSingle(query, ['sourceName', 'source_name'])
  const author = readSingle(query, ['author'])
  const cover = readSingle(query, ['cover', 'coverUrl', 'cover_url'])
  const readingDirection = readSingle(query, ['readingDirection', 'reading_direction'])

  if (source) next.source = source
  if (ref) next.ref = ref
  if (chapter) next.chapter = chapter
  if (chapterIndex) next.chapterIndex = chapterIndex
  if (toc) next.toc = toc
  if (page) next.page = page
  if (title) next.title = title
  if (sourceName) next.sourceName = sourceName
  if (author) next.author = author
  if (cover) next.cover = cover
  if (readingDirection) next.readingDirection = readingDirection
  return next
}

/** Migrates `/video/online/detail` query parameters to `/video/item/online` (§1.3). */
export function migrateVideoOnlineDetailQuery(query: LocationQuery): LocationQueryRaw {
  const next: LocationQueryRaw = {}
  const source = readSingle(query, ['source', 'sourceId', 'source_id'])
  const ref = readSingle(query, ['ref', 'detailUrl', 'detail_url'])
  const vid = readSingle(query, ['vid', 'videoId', 'video_id'])
  const title = readSingle(query, ['title', 'name'])
  const cover = readSingle(query, ['cover', 'coverUrl', 'cover_url', 'posterUrl'])
  const sourceName = readSingle(query, ['sourceName', 'source_name'])
  const remarks = readSingle(query, ['remarks'])
  const year = readSingle(query, ['year'])
  const area = readSingle(query, ['area'])

  if (source) next.source = source
  if (ref) next.ref = ref
  if (vid) next.vid = vid
  if (title) next.title = title
  if (cover) next.cover = cover
  if (sourceName) next.sourceName = sourceName
  if (remarks) next.remarks = remarks
  if (year) next.year = year
  if (area) next.area = area
  return next
}

/**
 * Migrates `/video/online/watch` query parameters to `/video/play/online` (§1.3).
 * Preserves `episodeIndex` verbatim (required by E2E tier2:3369).
 */
export function migrateVideoOnlineWatchQuery(query: LocationQuery): LocationQueryRaw {
  const next: LocationQueryRaw = {}
  const source = readSingle(query, ['source', 'sourceId', 'source_id'])
  const ref = readSingle(query, ['ref', 'detailUrl', 'detail_url'])
  const vid = readSingle(query, ['vid', 'videoId', 'video_id'])
  const play = readSingle(query, ['play', 'playUrl', 'play_url'])
  const group = readSingle(query, ['group', 'groupName', 'group_name'])
  const episodeIndex = readSingle(query, ['episodeIndex', 'episode_index'])
  const title = readSingle(query, ['title', 'name'])
  const cover = readSingle(query, ['cover', 'coverUrl', 'cover_url', 'posterUrl'])
  const sourceName = readSingle(query, ['sourceName', 'source_name'])

  if (source) next.source = source
  if (ref) next.ref = ref
  if (vid) next.vid = vid
  if (play) next.play = play
  if (group) next.group = group
  if (episodeIndex) next.episodeIndex = episodeIndex
  if (title) next.title = title
  if (cover) next.cover = cover
  if (sourceName) next.sourceName = sourceName
  return next
}

function buildOnlineScopeRedirect(domain: MediaKind, query: LocationQuery): RouteLocationRaw {
  const next = migrateQuery(query)
  next.scope = 'online'
  return { path: `/${domain}/library`, query: next }
}

/**
 * Maps legacy `/settings?tab=...&domain=...` query parameters to the new
 * single-level `/settings/:section` route (§1.4 & D6).
 */
export function migrateSettingsLocation(
  query: LocationQuery,
  options?: { isStandaloneApp?: boolean },
): RouteLocationRaw {
  const rawTab = readSingle(query, ['tab'])
  const rawDomain = readSingle(query, ['domain'])
  const domain: MediaKind | null = isMediaKind(rawDomain) ? rawDomain : null

  if (rawTab === 'general') {
    return { path: '/settings/about' }
  }

  if (rawTab === 'account' || rawTab === 'profile' || rawTab === 'users') {
    return { path: '/settings/account' }
  }

  if (
    rawTab === 'domain-general' ||
    rawTab === 'preferences' ||
    rawTab === 'reader' ||
    rawTab === 'playback'
  ) {
    return domain
      ? { path: '/settings/playback', hash: `#${domain}` }
      : { path: '/settings/playback' }
  }

  if (rawTab === 'domain-protocols' || rawTab === 'protocols' || rawTab === 'opds') {
    const isStandalone = options?.isStandaloneApp ?? useAppClientStore().isStandaloneApp
    if (isStandalone && (domain === 'book' || domain === 'comic' || domain === 'music')) {
      return {
        path: '/settings/sources',
        query: { domain, kind: 'remote' },
      }
    }
    return { path: '/settings/about', hash: '#protocols' }
  }

  if (rawTab === 'domain-datasources' || rawTab === 'datasources' || rawTab === 'sources') {
    // D6: legacy datasources deep links open with advanced mode enabled (E2E F12 compatibility)
    const nextQuery: LocationQueryRaw = { advanced: '1' }
    if (domain) nextQuery.domain = domain
    return { path: '/settings/sources', query: nextQuery }
  }

  if (rawTab === 'domain-libraries' || rawTab === 'libraries') {
    return domain
      ? { path: '/settings/libraries', query: { domain } }
      : { path: '/settings/libraries' }
  }

  // `appearance` or unrecognized/omitted tab
  return { path: '/settings/appearance' }
}

/**
 * Helper for a canonical `/settings` index route (e.g. mobile list-detail):
 * returns the migrated target when legacy `?tab=` or `?domain=` query params are present,
 * or `null` when visiting bare `/settings`.
 */
export function resolveLegacySettingsRedirect(
  to: RouteLocationNormalized,
  options?: { isStandaloneApp?: boolean },
): RouteLocationRaw | null {
  const hasTab = Boolean(readSingle(to.query, ['tab']))
  const hasDomain = Boolean(readSingle(to.query, ['domain']))
  if (!hasTab && !hasDomain) return null
  return migrateSettingsLocation(to.query, options)
}

/** Bookmarkable alias `/:domain/discover` -> `/:domain/library?scope=online` (D1). */
export const discoverAliasRoute: RouteRecordRaw = {
  path: '/:domain(book|comic|music|video)/discover',
  redirect: (to) => {
    const rawDomain = readParam(to.params.domain)
    const domain: MediaKind = isMediaKind(rawDomain) ? rawDomain : 'book'
    return buildOnlineScopeRedirect(domain, to.query)
  },
}

/** Legacy `/settings` query-based tab redirect route. */
export const settingsLegacyRedirectRoute: RouteRecordRaw = {
  path: '/settings',
  redirect: (to) => migrateSettingsLocation(to.query),
}

/**
 * Complete table of legacy route redirects (§1.4) ready to spread into `router/index.ts`
 * after the canonical routes (and before the catch-all `/:pathMatch(.*)*`).
 */
export const legacyRedirectRoutes: RouteRecordRaw[] = [
  // 0. Root & top-level plural aliases
  { path: '/connect', redirect: '/' },
  { path: '/books', redirect: '/book' },
  { path: '/comics', redirect: '/comic' },
  { path: '/videos', redirect: '/video' },
  { path: '/book/books', redirect: '/book/library' },
  discoverAliasRoute,

  // 1. Book domain legacy routes
  {
    path: '/book/catalog',
    redirect: (to) => {
      const next = migrateQuery(to.query, { extraKeys: BOOK_FILTER_KEYS })
      const section = readSingle(to.query, ['section'])
      if (section === 'recent') {
        next.tab = 'continue'
      } else if (section === 'favorites') {
        next.tab = 'favorites'
      }
      return { path: '/book/library', query: compactQuery(next) }
    },
  },
  {
    path: '/book/items/:id',
    redirect: (to) => `/book/item/${encodeURIComponent(readParam(to.params.id))}`,
  },
  { path: '/book/libraries', redirect: { path: '/book/library', query: { view: 'items' } } },
  { path: '/book/shelves', redirect: { path: '/book/library', query: { tab: 'collections' } } },
  { path: '/book/tags', redirect: { path: '/book/library', query: { view: 'tags' } } },
  { path: '/book/authors', redirect: { path: '/book/library', query: { view: 'authors' } } },
  { path: '/book/bookmarks', redirect: { path: '/book/library', query: { tab: 'continue' } } },
  {
    path: '/book/online',
    redirect: (to) => buildOnlineScopeRedirect('book', to.query),
  },
  {
    path: '/book/read/:id',
    redirect: (to) => ({
      path: `/book/play/${encodeURIComponent(readParam(to.params.id))}`,
      query: to.query,
    }),
  },
  {
    path: '/book/online/read',
    redirect: (to) => ({
      path: '/book/play/online',
      query: compactQuery(migrateBookOnlineReadQuery(to.query)),
    }),
  },

  // 2. Comic domain legacy routes
  {
    path: '/comic/series',
    redirect: (to) => {
      const next = migrateQuery(to.query, { extraKeys: COMIC_FILTER_KEYS })
      return { path: '/comic/library', query: compactQuery(next) }
    },
  },
  {
    path: '/comic/series/:id',
    redirect: (to) => `/comic/item/${encodeURIComponent(readParam(to.params.id))}`,
  },
  { path: '/comic/continue', redirect: { path: '/comic/library', query: { tab: 'continue' } } },
  { path: '/comic/favorites', redirect: { path: '/comic/library', query: { tab: 'favorites' } } },
  { path: '/comic/libraries', redirect: '/comic/library' },
  { path: '/comic/shelves', redirect: { path: '/comic/library', query: { tab: 'collections' } } },
  {
    path: '/comic/tags',
    redirect: (to) => {
      const next = migrateQuery(to.query, { extraKeys: COMIC_FILTER_KEYS })
      const rawSection = readSingle(to.query, ['section'])
      next.view = VALID_COMIC_TAG_SECTIONS.has(rawSection) ? rawSection : 'genres'
      return { path: '/comic/library', query: next }
    },
  },
  { path: '/comic/bookmarks', redirect: { path: '/comic/library', query: { tab: 'continue' } } },
  {
    path: '/comic/online',
    redirect: (to) => buildOnlineScopeRedirect('comic', to.query),
  },
  {
    path: '/comic/online/detail',
    redirect: (to) => ({
      path: '/comic/item/online',
      query: compactQuery(migrateComicOnlineDetailQuery(to.query)),
    }),
  },
  {
    path: '/comic/read/:chapterId',
    redirect: (to) => ({
      path: `/comic/play/${encodeURIComponent(readParam(to.params.chapterId))}`,
      query: to.query,
    }),
  },
  {
    path: '/comic/online/read',
    redirect: (to) => ({
      path: '/comic/play/online',
      query: compactQuery(migrateComicOnlineReadQuery(to.query)),
    }),
  },

  // 3. Music domain legacy routes
  {
    path: '/music/songs',
    redirect: (to) => {
      const next = migrateQuery(to.query, { extraKeys: MUSIC_FILTER_KEYS })
      const rawTab = readSingle(to.query, ['tab'])
      if (rawTab === 'albums') {
        next.view = 'items'
        delete next.tab
      } else if (rawTab === 'artists') {
        next.view = 'artists'
        delete next.tab
      } else {
        if (!next.view && next.scope !== 'online') {
          next.view = 'tracks'
        }
        if (rawTab && !VALID_LIBRARY_TABS.has(rawTab)) {
          delete next.tab
        }
      }
      return { path: '/music/library', query: compactQuery(next) }
    },
  },
  {
    path: '/music/albums',
    redirect: (to) => {
      const next = migrateQuery(to.query, { extraKeys: MUSIC_FILTER_KEYS })
      next.view = 'items'
      return { path: '/music/library', query: next }
    },
  },
  {
    path: '/music/artists',
    redirect: (to) => {
      const next = migrateQuery(to.query, { extraKeys: MUSIC_FILTER_KEYS })
      next.view = 'artists'
      return { path: '/music/library', query: next }
    },
  },
  {
    path: '/music/albums/:id',
    redirect: (to) => `/music/item/${encodeURIComponent(readParam(to.params.id))}`,
  },
  {
    path: '/music/artists/:id',
    redirect: (to) => ({
      path: '/music/library',
      query: { artist: readParam(to.params.id) },
    }),
  },
  { path: '/music/continue', redirect: { path: '/music/library', query: { tab: 'continue' } } },
  { path: '/music/favorites', redirect: { path: '/music/library', query: { tab: 'favorites' } } },
  { path: '/music/libraries', redirect: '/music/library' },
  { path: '/music/playlists', redirect: { path: '/music/library', query: { tab: 'collections' } } },
  {
    path: '/music/playlists/:id',
    redirect: (to) => ({
      path: '/music/library',
      query: { collection: readParam(to.params.id) },
    }),
  },
  { path: '/music/genres', redirect: { path: '/music/library', query: { view: 'genres' } } },
  { path: '/music/tags', redirect: { path: '/music/library', query: { view: 'tags' } } },
  {
    path: '/music/search',
    redirect: (to) => {
      const next = migrateQuery(to.query, { extraKeys: MUSIC_FILTER_KEYS })
      if (next.scope !== 'online') {
        next.view = 'tracks'
      }
      return { path: '/music/library', query: compactQuery(next) }
    },
  },

  // 4. Video domain legacy routes
  {
    path: '/video/catalog',
    redirect: (to) => {
      const next = migrateQuery(to.query, { extraKeys: VIDEO_FILTER_KEYS })
      const rawTab = readSingle(to.query, ['tab'])
      const rawKind = readSingle(to.query, ['kind'])
      if (rawTab === 'shows') {
        next.view = 'shows'
        delete next.tab
      } else if (rawTab && !VALID_LIBRARY_TABS.has(rawTab)) {
        delete next.tab
      }
      if (rawKind === 'standalone') {
        next.view = 'movies'
        delete next.kind
      }
      return { path: '/video/library', query: compactQuery(next) }
    },
  },
  { path: '/video/continue', redirect: { path: '/video/library', query: { tab: 'continue' } } },
  { path: '/video/favorites', redirect: { path: '/video/library', query: { tab: 'favorites' } } },
  { path: '/video/libraries', redirect: '/video/library' },
  { path: '/video/shelves', redirect: { path: '/video/library', query: { tab: 'collections' } } },
  {
    path: '/video/shelves/:id',
    redirect: (to) => ({
      path: '/video/library',
      query: { collection: readParam(to.params.id) },
    }),
  },
  { path: '/video/tags', redirect: { path: '/video/library', query: { view: 'tags' } } },
  { path: '/video/genres', redirect: { path: '/video/library', query: { view: 'genres' } } },
  {
    path: '/video/shows',
    redirect: (to) => {
      const next = migrateQuery(to.query, { extraKeys: VIDEO_FILTER_KEYS })
      next.view = 'shows'
      return { path: '/video/library', query: next }
    },
  },
  {
    path: '/video/shows/:id',
    redirect: (to) => `/video/item/${encodeURIComponent(readParam(to.params.id))}`,
  },
  {
    path: '/video/online',
    redirect: (to) => buildOnlineScopeRedirect('video', to.query),
  },
  {
    path: '/video/online/detail',
    redirect: (to) => ({
      path: '/video/item/online',
      query: compactQuery(migrateVideoOnlineDetailQuery(to.query)),
    }),
  },
  {
    path: '/video/watch/:id',
    redirect: (to) => ({
      path: `/video/play/${encodeURIComponent(readParam(to.params.id))}`,
      query: to.query,
    }),
  },
  {
    path: '/video/online/watch',
    redirect: (to) => ({
      path: '/video/play/online',
      query: compactQuery(migrateVideoOnlineWatchQuery(to.query)),
    }),
  },

  // 5. Legacy remote & settings aliases
  {
    path: '/remote',
    redirect: (to) => {
      const rawTab = readSingle(to.query, ['tab'])
      const rawDomain = readSingle(to.query, ['domain'])
      const query: LocationQueryRaw = { kind: 'remote' }
      if (rawTab === 'subsonic') {
        query.domain = 'music'
      } else if (rawTab === 'opds') {
        query.domain = 'book'
      } else if (isMediaKind(rawDomain)) {
        query.domain = rawDomain
      }
      return { path: '/settings/sources', query }
    },
  },
  // Note: `/settings` legacy `?tab=` redirects are handled by `resolveLegacySettingsRedirect`
  // inside the canonical `/settings` route `beforeEnter` so mobile (<840px) can stay on bare `/settings`.
]

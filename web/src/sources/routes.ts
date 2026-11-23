import type {
  LocationQuery,
  LocationQueryRaw,
  RouteLocationNormalizedLoaded,
  RouteLocationRaw,
} from 'vue-router'
import type { MediaKind } from '@/types/api'
import type { ItemHints, ItemRef, PlayPosition, SourceFilter, SourceScope } from './types'

export type LibraryTab = 'all' | 'continue' | 'favorites' | 'collections'

export interface LibraryRouteParams extends Partial<SourceFilter> {
  q?: string | null
  tab?: LibraryTab | null
  collection?: string | null
  view?: string | null
  library?: string | null
  sort?: string | null
  order?: 'asc' | 'desc' | string | null
  page?: number | null
  // Domain-scoped filters (§1.2)
  author?: string | string[] | null
  tag?: string | string[] | null
  format?: string | string[] | null
  missing?: boolean | null
  writer?: string | null
  penciller?: string | null
  genre?: string | null
  artist?: string | null
  kind?: string | null
}

const VALID_DOMAINS: readonly MediaKind[] = ['book', 'comic', 'music', 'video']
const VALID_SCOPES: readonly SourceScope[] = ['local', 'pool', 'online', 'remote']

export function isMediaKind(value: unknown): value is MediaKind {
  return typeof value === 'string' && (VALID_DOMAINS as readonly string[]).includes(value)
}

export function isSourceScope(value: unknown): value is SourceScope {
  return typeof value === 'string' && (VALID_SCOPES as readonly string[]).includes(value)
}

function readFirstQueryString(query: LocationQuery, keys: readonly string[]): string {
  for (const key of keys) {
    const raw = query[key]
    if (typeof raw === 'string' && raw.trim()) {
      return raw.trim()
    }
    if (Array.isArray(raw)) {
      for (const entry of raw) {
        if (typeof entry === 'string' && entry.trim()) {
          return entry.trim()
        }
      }
    }
  }
  return ''
}

function appendHintsQuery(query: Record<string, string>, hints?: ItemHints): void {
  if (!hints) return
  if (hints.title?.trim()) query.title = hints.title.trim()
  if (hints.cover?.trim()) query.cover = hints.cover.trim()
  if (hints.author?.trim()) query.author = hints.author.trim()
}

function setMultiOrSingle(
  target: LocationQueryRaw,
  key: string,
  val: string | string[] | null | undefined,
): void {
  if (!val) return
  if (Array.isArray(val)) {
    const clean = val.map((v) => v.trim()).filter(Boolean)
    if (clean.length === 1) target[key] = clean[0]!
    else if (clean.length > 1) target[key] = clean
    return
  }
  const trimmed = val.trim()
  if (trimmed) target[key] = trimmed
}

/**
 * Parses `SourceFilter` (`scope` + `source`) from any route query.
 * Omitted or unrecognized `scope` defaults to `'local'`.
 */
export function parseSourceFilter(query: LocationQuery): SourceFilter {
  const rawScope = readFirstQueryString(query, ['scope'])
  const scope: SourceScope = isSourceScope(rawScope) ? rawScope : 'local'
  if (scope === 'local' || scope === 'pool') {
    return { scope, sourceId: null }
  }
  const sourceId = readFirstQueryString(query, ['source', 'source_id', 'sourceId'])
  return {
    scope,
    sourceId: sourceId || null,
  }
}

/**
 * Builds the canonical `/:domain/library` route with clean §1.2 query parameters.
 */
export function libraryRoute(
  domain: MediaKind,
  params?: LibraryRouteParams,
): RouteLocationRaw {
  const query: LocationQueryRaw = {}
  if (params) {
    const scope = params.scope && isSourceScope(params.scope) ? params.scope : 'local'
    if (scope !== 'local') {
      query.scope = scope
      if (params.sourceId?.trim()) {
        query.source = params.sourceId.trim()
      }
    }
    if (params.q?.trim()) query.q = params.q.trim()
    if (params.tab && params.tab !== 'all') query.tab = params.tab
    if (params.collection?.trim()) query.collection = params.collection.trim()
    if (params.view?.trim()) query.view = params.view.trim()
    if (params.library?.trim()) query.library = params.library.trim()
    if (params.sort?.trim()) query.sort = params.sort.trim()
    if (params.order?.trim()) query.order = params.order.trim()
    if (params.page && Number.isFinite(params.page) && params.page > 1) {
      query.page = String(Math.floor(params.page))
    }

    setMultiOrSingle(query, 'author', params.author)
    setMultiOrSingle(query, 'tag', params.tag)
    setMultiOrSingle(query, 'format', params.format)
    if (params.missing) query.missing = '1'
    if (params.writer?.trim()) query.writer = params.writer.trim()
    if (params.penciller?.trim()) query.penciller = params.penciller.trim()
    if (params.genre?.trim()) query.genre = params.genre.trim()
    if (params.artist?.trim()) query.artist = params.artist.trim()
    if (params.kind?.trim()) query.kind = params.kind.trim()
  }

  return Object.keys(query).length > 0
    ? { path: `/${domain}/library`, query }
    : { path: `/${domain}/library` }
}

/**
 * Builds the canonical source-agnostic detail route `/:domain/item/:id` (§1.3).
 */
export function itemRoute(ref: ItemRef): RouteLocationRaw {
  if (ref.scope === 'local') {
    return { path: `/${ref.domain}/item/${encodeURIComponent(ref.id)}` }
  }

  const query: Record<string, string> = {
    source: ref.sourceId,
    ref: ref.ref,
  }

  const extra = ref.hints?.extra
  if (ref.domain === 'book') {
    const toc = extra?.toc || extra?.tocUrl || extra?.toc_url
    if (toc?.trim()) query.toc = toc.trim()
  } else if (ref.domain === 'video') {
    const vid = extra?.vid || extra?.videoId || extra?.video_id
    if (vid?.trim()) query.vid = vid.trim()
  }

  appendHintsQuery(query, ref.hints)
  return { path: `/${ref.domain}/item/${ref.scope}`, query }
}

/**
 * Builds the canonical fullscreen reader/player route `/:domain/play/:id` (§1.3).
 * Music has no fullscreen player route (uses global PlayerBar) and resolves to `itemRoute`.
 * Preserves the exact `episodeIndex` query key for video playback (E2E tier2:3369).
 */
export function playRoute(ref: ItemRef, pos?: PlayPosition): RouteLocationRaw {
  if (ref.domain === 'music') {
    return itemRoute(ref)
  }

  if (ref.scope === 'local') {
    const query: Record<string, string> = {}
    if (ref.domain === 'comic' && pos && Number.isFinite(pos.position) && pos.position > 0) {
      query.page = String(Math.floor(pos.position))
    } else if (ref.domain === 'book' && pos?.unitIndex !== undefined && pos.unitIndex >= 0) {
      query.chapter = String(Math.floor(pos.unitIndex))
    } else if (ref.domain === 'video' && pos && Number.isFinite(pos.position) && pos.position > 0) {
      query.t = String(Math.floor(pos.position))
    }
    const path = `/${ref.domain}/play/${encodeURIComponent(ref.id)}`
    return Object.keys(query).length > 0 ? { path, query } : { path }
  }

  const query: Record<string, string> = {
    source: ref.sourceId,
    ref: ref.ref,
  }
  const extra = ref.hints?.extra

  if (ref.domain === 'book') {
    const toc = extra?.toc || extra?.tocUrl || extra?.toc_url
    if (toc?.trim()) query.toc = toc.trim()
    const chapter =
      pos?.unitIndex !== undefined && Number.isFinite(pos.unitIndex)
        ? String(Math.max(0, Math.floor(pos.unitIndex)))
        : extra?.chapter?.trim()
    if (chapter) query.chapter = chapter
  } else if (ref.domain === 'comic') {
    const chapter = pos?.unitKey?.trim() || extra?.chapter?.trim() || extra?.chapterUrl?.trim()
    if (chapter) query.chapter = chapter
    if (pos && Number.isFinite(pos.position) && pos.position > 0) {
      query.page = String(Math.floor(pos.position))
    } else if (extra?.page?.trim()) {
      query.page = extra.page.trim()
    }
  } else if (ref.domain === 'video') {
    const vid = extra?.vid || extra?.videoId || extra?.video_id
    if (vid?.trim()) query.vid = vid.trim()
    const play = pos?.unitKey?.trim() || extra?.play?.trim() || extra?.playUrl?.trim()
    if (play) query.play = play
    const group = pos?.unitGroup?.trim() || extra?.group?.trim() || extra?.groupName?.trim()
    if (group) query.group = group
    if (pos?.unitIndex !== undefined && Number.isFinite(pos.unitIndex) && pos.unitIndex >= 0) {
      query.episodeIndex = String(Math.floor(pos.unitIndex))
    } else if (extra?.episodeIndex?.trim()) {
      query.episodeIndex = extra.episodeIndex.trim()
    }
  }

  appendHintsQuery(query, ref.hints)
  return { path: `/${ref.domain}/play/${ref.scope}`, query }
}

function resolveRouteDomain(
  route: RouteLocationNormalizedLoaded,
  explicitDomain?: MediaKind,
): MediaKind | null {
  if (explicitDomain) return explicitDomain
  if (isMediaKind(route.meta.domain)) return route.meta.domain
  if (isMediaKind(route.params.domain)) return route.params.domain
  const firstSegment = route.path.split('/').filter(Boolean)[0]
  return isMediaKind(firstSegment) ? firstSegment : null
}

/**
 * Parses an `ItemRef` from a detail (`/:domain/item/:id`) or player (`/:domain/play/:id`) route.
 * Accepts canonical §1.3 query keys and transparently falls back to legacy alias keys.
 */
export function parseItemRef(
  route: RouteLocationNormalizedLoaded,
  explicitDomain?: MediaKind,
): ItemRef | null {
  const domain = resolveRouteDomain(route, explicitDomain)
  if (!domain) return null

  const rawId =
    typeof route.params.id === 'string'
      ? route.params.id
      : typeof route.params.chapterId === 'string'
        ? route.params.chapterId
        : ''

  const isOnlinePath =
    rawId === 'online' ||
    route.path.includes(`/${domain}/online/`) ||
    route.path.endsWith(`/${domain}/item/online`) ||
    route.path.endsWith(`/${domain}/play/online`)
  const isRemotePath =
    rawId === 'remote' ||
    route.path.endsWith(`/${domain}/item/remote`) ||
    route.path.endsWith(`/${domain}/play/remote`)

  if (isOnlinePath || isRemotePath) {
    const scope: 'online' | 'remote' = isRemotePath ? 'remote' : 'online'
    const sourceId = readFirstQueryString(route.query, ['source', 'sourceId', 'source_id'])
    const refVal = readFirstQueryString(route.query, [
      'ref',
      'comicUrl',
      'comic_url',
      'detailUrl',
      'detail_url',
      'bookUrl',
      'book_url',
      'vid',
      'videoId',
      'video_id',
      'play',
      'playUrl',
      'play_url',
      'chapter',
      'chapterUrl',
      'chapter_url',
    ])
    if (!sourceId || !refVal) return null

    const extra: Record<string, string> = {}
    const toc = readFirstQueryString(route.query, ['toc', 'tocUrl', 'toc_url'])
    const vid = readFirstQueryString(route.query, ['vid', 'videoId', 'video_id'])
    const chapter = readFirstQueryString(route.query, ['chapter', 'chapterUrl', 'chapter_url'])
    const play = readFirstQueryString(route.query, ['play', 'playUrl', 'play_url'])
    const group = readFirstQueryString(route.query, ['group', 'groupName', 'group_name'])
    const episodeIndex = readFirstQueryString(route.query, ['episodeIndex', 'episode_index'])
    const page = readFirstQueryString(route.query, ['page'])
    if (toc) extra.toc = toc
    if (vid) extra.vid = vid
    if (chapter) extra.chapter = chapter
    if (play) extra.play = play
    if (group) extra.group = group
    if (episodeIndex) extra.episodeIndex = episodeIndex
    if (page) extra.page = page

    const title = readFirstQueryString(route.query, ['title', 'name'])
    const cover = readFirstQueryString(route.query, ['cover', 'coverUrl', 'cover_url', 'posterUrl'])
    const author = readFirstQueryString(route.query, ['author'])
    const hints: ItemHints = {}
    if (title) hints.title = title
    if (cover) hints.cover = cover
    if (author) hints.author = author
    if (Object.keys(extra).length > 0) hints.extra = extra

    return {
      scope,
      domain,
      sourceId,
      ref: refVal,
      hints: Object.keys(hints).length > 0 ? hints : undefined,
    }
  }

  if (!rawId) return null
  return {
    scope: 'local',
    domain,
    id: rawId,
  }
}

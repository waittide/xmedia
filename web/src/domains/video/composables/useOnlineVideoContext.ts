import type {
  LocationQuery,
  RouteLocationNormalizedLoaded,
  RouteLocationRaw,
} from 'vue-router'
import { parseItemRef, type ItemHints, type TrackedItemRef } from '@/sources'
import type {
  OnlineVideoContextStorage,
  OnlineVideoPlayGroup,
  OnlineVideoPlayerContextStorage,
} from '../onlineApi'

let inMemoryDetailContext: OnlineVideoContextStorage | null = null
let inMemoryPlayerContext: OnlineVideoPlayerContextStorage | null = null

export function readQueryString(query: LocationQuery, keys: readonly string[]): string {
  for (const key of keys) {
    const raw = query[key]
    if (typeof raw === 'string' && raw.trim()) {
      return raw.trim()
    }
    if (Array.isArray(raw)) {
      const hit = raw.find((v): v is string => typeof v === 'string' && v.trim().length > 0)
      if (hit) return hit.trim()
    }
  }
  return ''
}

export function looksLikeUrlOrPath(val: string): boolean {
  return /^https?:\/\//i.test(val) || val.startsWith('/')
}

export function readStoredDetailContext(): OnlineVideoContextStorage | null {
  return inMemoryDetailContext ? { ...inMemoryDetailContext } : null
}

export function writeStoredDetailContext(ctx: OnlineVideoContextStorage): void {
  inMemoryDetailContext = { ...ctx }
}

export function readStoredPlayerContext(): OnlineVideoPlayerContextStorage | null {
  return inMemoryPlayerContext ? { ...inMemoryPlayerContext } : null
}

export function writeStoredPlayerContext(ctx: OnlineVideoPlayerContextStorage): void {
  inMemoryPlayerContext = { ...ctx }
}

export function resolveOnlineVideoItemKey(params: {
  detailUrl?: string | null
  videoId?: string | null
  ref?: string | null
}): string {
  return (
    params.detailUrl?.trim() ||
    params.videoId?.trim() ||
    params.ref?.trim() ||
    ''
  )
}

export function buildOnlineVideoTrackedRef(params: {
  sourceId: string
  detailUrl?: string | null
  videoId?: string | null
  ref?: string | null
  title?: string | null
  cover?: string | null
  sourceName?: string | null
}): TrackedItemRef | null {
  const sourceId = params.sourceId.trim()
  const itemKey = resolveOnlineVideoItemKey(params)
  if (!sourceId || !itemKey) return null

  const extra: Record<string, string> = {}
  if (params.videoId?.trim()) extra.vid = params.videoId.trim()
  if (params.detailUrl?.trim()) extra.detail_url = params.detailUrl.trim()
  if (params.sourceName?.trim()) extra.source_name = params.sourceName.trim()

  const hints: ItemHints = {}
  if (params.title?.trim()) hints.title = params.title.trim()
  if (params.cover?.trim()) hints.cover = params.cover.trim()
  if (Object.keys(extra).length > 0) hints.extra = extra

  return {
    scope: 'online',
    domain: 'video',
    sourceId,
    ref: itemKey,
    hints: Object.keys(hints).length > 0 ? hints : undefined,
  }
}

export function buildOnlineVideoDetailRoute(params: {
  sourceId: string
  detailUrl?: string | null
  videoId?: string | null
  title?: string | null
  cover?: string | null
  sourceName?: string | null
  remarks?: string | null
  year?: string | null
  area?: string | null
}): RouteLocationRaw {
  const ref = resolveOnlineVideoItemKey(params)
  const query: Record<string, string> = {
    source: params.sourceId,
    ref,
  }
  if (params.videoId?.trim()) query.vid = params.videoId.trim()
  if (params.title?.trim()) query.title = params.title.trim()
  if (params.cover?.trim()) query.cover = params.cover.trim()
  if (params.sourceName?.trim()) query.sourceName = params.sourceName.trim()
  if (params.remarks?.trim()) query.remarks = params.remarks.trim()
  if (params.year?.trim()) query.year = params.year.trim()
  if (params.area?.trim()) query.area = params.area.trim()
  return { path: '/video/item/online', query }
}

export function buildOnlineVideoPlayRoute(params: {
  sourceId: string
  detailUrl?: string | null
  videoId?: string | null
  playUrl: string
  groupName?: string | null
  episodeIndex: number
  title?: string | null
  cover?: string | null
  sourceName?: string | null
}): RouteLocationRaw {
  const ref = resolveOnlineVideoItemKey(params) || params.playUrl
  const query: Record<string, string> = {
    source: params.sourceId,
    ref,
    play: params.playUrl,
    episodeIndex: String(Math.max(0, Math.floor(params.episodeIndex))),
  }
  if (params.videoId?.trim()) query.vid = params.videoId.trim()
  if (params.groupName?.trim()) query.group = params.groupName.trim()
  if (params.title?.trim()) query.title = params.title.trim()
  if (params.cover?.trim()) query.cover = params.cover.trim()
  if (params.sourceName?.trim()) query.sourceName = params.sourceName.trim()
  return { path: '/video/play/online', query }
}

export interface HydratedOnlineVideoDetail {
  sourceId: string
  sourceName: string
  videoId: string
  detailUrl: string
  itemKey: string
  fallbackCtx: OnlineVideoContextStorage
}

export function hydrateOnlineVideoDetailContext(
  route: RouteLocationNormalizedLoaded,
): HydratedOnlineVideoDetail {
  const parsed = parseItemRef(route, 'video')
  const parsedOnline = parsed && parsed.scope !== 'local' ? parsed : null

  const qSourceId =
    parsedOnline?.sourceId || readQueryString(route.query, ['source', 'sourceId', 'source_id'])
  const qRef = readQueryString(route.query, ['ref'])
  const qDetailExplicit = readQueryString(route.query, ['detailUrl', 'detail_url'])
  const qVideoExplicit = readQueryString(route.query, ['vid', 'videoId', 'video_id'])

  let qDetailUrl = qDetailExplicit
  let qVideoId = qVideoExplicit
  if (qRef) {
    if (qVideoExplicit || looksLikeUrlOrPath(qRef)) {
      if (!qDetailUrl) qDetailUrl = qRef
    } else if (!qVideoId) {
      qVideoId = qRef
    }
  }

  const qSourceName = readQueryString(route.query, ['sourceName', 'source_name'])
  const qTitle = readQueryString(route.query, ['title', 'name'])
  const qCoverUrl = readQueryString(route.query, ['cover', 'coverUrl', 'cover_url', 'posterUrl'])
  const qRemarks = readQueryString(route.query, ['remarks'])
  const qYear = readQueryString(route.query, ['year'])
  const qArea = readQueryString(route.query, ['area'])

  const stored = readStoredDetailContext()
  const hasQueryIdentity = Boolean(qSourceId && (qVideoId || qDetailUrl))
  const matchesStored = Boolean(
    stored &&
      ((hasQueryIdentity &&
        stored.sourceId === qSourceId &&
        ((qVideoId && stored.videoId === qVideoId) ||
          (qDetailUrl && stored.detailUrl === qDetailUrl))) ||
        (!qSourceId &&
          !qVideoId &&
          !qDetailUrl &&
          Boolean(stored.sourceId && (stored.videoId || stored.detailUrl)))),
  )

  const sourceId = qSourceId || (matchesStored && stored ? stored.sourceId : '')
  const videoId = qVideoId || (matchesStored && stored ? (stored.videoId ?? '') : '')
  const detailUrl = qDetailUrl || (matchesStored && stored ? (stored.detailUrl ?? '') : '')
  const sourceName = qSourceName || (matchesStored && stored ? (stored.sourceName ?? '') : '')

  const fallbackCtx: OnlineVideoContextStorage = {
    sourceId,
    sourceName: sourceName || undefined,
    videoId: videoId || undefined,
    detailUrl: detailUrl || undefined,
    title: qTitle || (matchesStored && stored ? stored.title : undefined),
    coverUrl: qCoverUrl || (matchesStored && stored ? stored.coverUrl : undefined),
    proxyCoverUrl: matchesStored && stored ? stored.proxyCoverUrl : undefined,
    remarks: qRemarks || (matchesStored && stored ? stored.remarks : undefined),
    year: qYear || (matchesStored && stored ? stored.year : undefined),
    area: qArea || (matchesStored && stored ? stored.area : undefined),
    typeName: matchesStored && stored ? stored.typeName : undefined,
    returnKeyword: matchesStored && stored ? stored.returnKeyword : undefined,
    returnSourceId: matchesStored && stored ? stored.returnSourceId : undefined,
  }

  return {
    sourceId,
    sourceName,
    videoId,
    detailUrl,
    itemKey: resolveOnlineVideoItemKey({ detailUrl, videoId, ref: qRef }),
    fallbackCtx,
  }
}

export interface HydratedOnlineVideoPlayer {
  sourceId: string
  sourceName: string
  videoId: string
  detailUrl: string
  itemKey: string
  title: string
  coverUrl: string | null
  proxyCoverUrl: string | null
  playGroups: OnlineVideoPlayGroup[]
  activeGroupName: string
  currentEpisodeIndex: number
  currentPlayUrl: string
}

export function hydrateOnlineVideoPlayerContext(
  route: RouteLocationNormalizedLoaded,
): HydratedOnlineVideoPlayer {
  const parsed = parseItemRef(route, 'video')
  const parsedOnline = parsed && parsed.scope !== 'local' ? parsed : null

  const qSourceId =
    parsedOnline?.sourceId || readQueryString(route.query, ['source', 'sourceId', 'source_id'])
  const qRef = readQueryString(route.query, ['ref'])
  const qDetailExplicit = readQueryString(route.query, ['detailUrl', 'detail_url'])
  const qVideoExplicit = readQueryString(route.query, ['vid', 'videoId', 'video_id'])
  const qPlayUrl = readQueryString(route.query, ['play', 'playUrl', 'play_url'])
  const qGroupName = readQueryString(route.query, ['group', 'groupName', 'group_name'])
  const qEpIdxRaw = readQueryString(route.query, ['episodeIndex', 'episode_index'])
  const qTitle = readQueryString(route.query, ['title', 'name'])
  const qSourceName = readQueryString(route.query, ['sourceName', 'source_name'])
  const qCover = readQueryString(route.query, ['cover', 'coverUrl', 'cover_url', 'posterUrl'])

  let qDetailUrl = qDetailExplicit
  let qVideoId = qVideoExplicit
  if (qRef && qRef !== qPlayUrl) {
    if (qVideoExplicit || looksLikeUrlOrPath(qRef)) {
      if (!qDetailUrl) qDetailUrl = qRef
    } else if (!qVideoId) {
      qVideoId = qRef
    }
  }

  const stored = readStoredPlayerContext()
  const matchesStored = Boolean(
    stored &&
      ((qSourceId &&
        stored.sourceId === qSourceId &&
        ((qPlayUrl && stored.playUrl === qPlayUrl) ||
          (qVideoId && stored.videoId === qVideoId) ||
          (qDetailUrl && stored.detailUrl === qDetailUrl) ||
          (stored.playGroups &&
            qPlayUrl &&
            stored.playGroups.some((g) => g.episodes.some((e) => e.play_url === qPlayUrl))))) ||
        (!qSourceId && !qPlayUrl && Boolean(stored.sourceId && stored.playUrl))),
  )

  const sourceId = qSourceId || (matchesStored && stored ? stored.sourceId : '')
  const videoId = qVideoId || (matchesStored && stored ? (stored.videoId ?? '') : '')
  const detailUrl = qDetailUrl || (matchesStored && stored ? (stored.detailUrl ?? '') : '')
  const title = qTitle || (matchesStored && stored ? (stored.title ?? '') : '')
  const sourceName = qSourceName || (matchesStored && stored ? (stored.sourceName ?? '') : '')
  const coverUrl = qCover || (matchesStored && stored ? (stored.coverUrl ?? null) : null)
  const proxyCoverUrl = matchesStored && stored ? (stored.proxyCoverUrl ?? null) : null
  const playGroups =
    matchesStored && stored && Array.isArray(stored.playGroups) ? stored.playGroups : []

  const parsedIdx = qEpIdxRaw ? Number.parseInt(qEpIdxRaw, 10) : Number.NaN
  const activeGroupName =
    qGroupName ||
    (matchesStored && stored ? (stored.groupName ?? '') : '') ||
    playGroups[0]?.name ||
    ''

  const groupEps =
    (playGroups.find((g) => g.name === activeGroupName) ?? playGroups[0])?.episodes ?? []

  let currentEpisodeIndex = 0
  let currentPlayUrl = ''

  if (qPlayUrl) {
    currentPlayUrl = qPlayUrl
    const foundIdx = groupEps.findIndex((e) => e.play_url === qPlayUrl)
    currentEpisodeIndex =
      foundIdx >= 0 ? foundIdx : Number.isFinite(parsedIdx) && parsedIdx >= 0 ? parsedIdx : 0
  } else if (Number.isFinite(parsedIdx) && parsedIdx >= 0 && groupEps[parsedIdx]) {
    currentEpisodeIndex = parsedIdx
    currentPlayUrl = groupEps[parsedIdx]!.play_url
  } else if (matchesStored && stored && stored.playUrl) {
    currentPlayUrl = stored.playUrl
    currentEpisodeIndex = stored.episodeIndex ?? 0
  } else {
    currentEpisodeIndex = 0
    currentPlayUrl = groupEps[0]?.play_url ?? ''
  }

  return {
    sourceId,
    sourceName,
    videoId,
    detailUrl,
    itemKey: resolveOnlineVideoItemKey({ detailUrl, videoId, ref: qRef }),
    title,
    coverUrl,
    proxyCoverUrl,
    playGroups,
    activeGroupName,
    currentEpisodeIndex,
    currentPlayUrl,
  }
}

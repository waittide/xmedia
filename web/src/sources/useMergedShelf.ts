import {
  computed,
  ref,
  toValue,
  watch,
  type ComputedRef,
  type MaybeRefOrGetter,
  type Ref,
} from 'vue'
import { useI18n } from 'vue-i18n'
import { withAuthToken } from '@/api/client'
import { api as bookApi } from '@/domains/book/api'
import { buildOnlineCoverUrl, onlineBookApi, type OnlineShelfBookDto } from '@/domains/book/onlineApi'
import type { BookSummaryDto } from '@/domains/book/types'
import { api as comicApi } from '@/domains/comic/api'
import { buildOnlineComicImageUrl } from '@/domains/comic/onlineApi'
import type { ContinueReadingItemDto, SeriesSummaryDto } from '@/domains/comic/types'
import { api as musicApi, buildCoverUrl as buildMusicCoverUrl } from '@/domains/music/api'
import { buildOnlineMusicCoverUrl } from '@/domains/music/onlineApi'
import type { AlbumDto, BookmarkDto, TrackDto } from '@/domains/music/types'
import { api as videoApi, buildPosterUrl as buildVideoPosterUrl } from '@/domains/video/api'
import { buildOnlineVideoPosterUrl } from '@/domains/video/onlineApi'
import type { ShowSummaryDto, VideoSummaryDto } from '@/domains/video/types'
import { useAppClientStore } from '@/stores/appServers'
import type { MediaKind } from '@/types/api'
import { onlineItemsApi } from './onlineItemsApi'
import { itemRoute, libraryRoute, playRoute } from './routes'
import {
  isAvailabilityBlocked,
  isOnlineItemDomain,
  onlineProgressToPlayPosition,
  type ItemHints,
  type MediaItemVM,
  type OnlineItem,
  type OnlineItemDomain,
  type OnlineItemSourceKind,
  type SourceScope,
  type TrackedItemRef,
} from './types'
import { seedOnlineItemStateCache } from './useOnlineItemState'

export type MergedShelfView = 'continue' | 'favorites'

export interface UseMergedShelfOptions {
  limit?: number
  /** Filter by source scope. Default: `'all'` (merges local + online + remote). */
  scope?: MaybeRefOrGetter<SourceScope | 'all'>
  /** Optional source ID filter when `scope` is `'online'` or `'remote'`. */
  sourceId?: MaybeRefOrGetter<string | null>
  /** Optional keyword filter for online/remote queries and client-side filtering. */
  query?: MaybeRefOrGetter<string>
  /** Automatically fetch when inputs change. Default: `true`. */
  autoLoad?: boolean
}

export interface UseMergedShelfReturn {
  items: ComputedRef<MediaItemVM[]>
  loading: Ref<boolean>
  error: Ref<string | null>
  refresh: () => Promise<void>
}

export function parseTimestampMs(raw: number | string | null | undefined): number {
  if (raw === null || raw === undefined) return 0
  if (typeof raw === 'number') {
    if (!Number.isFinite(raw) || raw <= 0) return 0
    return raw < 1e12 ? raw * 1000 : raw
  }
  const trimmed = raw.trim()
  if (!trimmed) return 0
  if (/^\d+$/.test(trimmed)) {
    const num = Number(trimmed)
    if (!Number.isFinite(num) || num <= 0) return 0
    return num < 1e12 ? num * 1000 : num
  }
  const parsed = Date.parse(trimmed)
  return Number.isFinite(parsed) ? parsed : 0
}

function resolveOnlineItemCover(
  domain: OnlineItemDomain,
  coverUrl: string | null | undefined,
  sourceId: string,
  sourceKind: OnlineItemSourceKind,
): string | undefined {
  const raw = coverUrl?.trim()
  if (!raw) return undefined
  if (raw.startsWith('/') && !raw.startsWith('//')) {
    return withAuthToken(raw)
  }
  if (sourceKind === 'remote') {
    return raw
  }
  if (domain === 'comic') {
    return buildOnlineComicImageUrl(raw, sourceId) || undefined
  }
  if (domain === 'music') {
    return buildOnlineMusicCoverUrl(raw, sourceId) || undefined
  }
  return buildOnlineVideoPosterUrl(raw, sourceId) || undefined
}

function extractRouteHintExtra(extra: Record<string, unknown> | undefined): Record<string, string> {
  if (!extra) return {}
  const out: Record<string, string> = {}
  const keys = [
    'toc',
    'toc_url',
    'tocUrl',
    'vid',
    'video_id',
    'videoId',
    'vod_id',
    'play',
    'play_url',
    'playUrl',
    'group',
    'group_name',
    'groupName',
    'chapter',
    'chapter_url',
    'chapterUrl',
  ] as const
  for (const k of keys) {
    const val = extra[k]
    if (typeof val === 'string' && val.trim()) {
      out[k] = val.trim()
    }
  }
  return out
}

function mapOnlineItemToVM(
  item: OnlineItem,
  view: MergedShelfView,
  resolveRemoteName: (sourceId: string) => string | undefined,
  blockedLabels: { disabled: string; missing: string },
): MediaItemVM {
  const domain = item.media_kind as OnlineItemDomain
  const blocked = isAvailabilityBlocked(item.availability)
  // Avoid issuing image/cover/poster-proxy requests against disabled or deleted sources (404)
  const cover = blocked
    ? undefined
    : resolveOnlineItemCover(domain, item.cover_url, item.source_id, item.source_kind)
  const hintExtra = extractRouteHintExtra(item.extra)
  const hints: ItemHints = {
    title: item.title,
    cover,
    author: item.subtitle ?? undefined,
    extra: Object.keys(hintExtra).length > 0 ? hintExtra : undefined,
  }
  const ref: TrackedItemRef = {
    scope: item.source_kind,
    domain,
    sourceId: item.source_id,
    ref: item.item_key,
    hints,
  }
  const pos = item.progress ? onlineProgressToPlayPosition(item.progress) : undefined
  const progressPercent =
    item.progress_ratio !== null && Number.isFinite(item.progress_ratio)
      ? Math.min(100, Math.max(0, Math.round(item.progress_ratio * 100)))
      : undefined

  const sourceLabel =
    item.source_name?.trim() ||
    (item.source_kind === 'remote' ? resolveRemoteName(item.source_id) : undefined) ||
    undefined

  const statusText =
    item.availability === 'source_disabled'
      ? blockedLabels.disabled
      : item.availability === 'source_missing'
        ? blockedLabels.missing
        : undefined

  const tsRaw =
    view === 'continue'
      ? (item.progress_at ?? item.updated_at)
      : (item.favorited_at ?? item.updated_at)

  return {
    ref,
    title: item.title,
    subtitle: item.subtitle ?? item.progress?.unit_title ?? undefined,
    cover,
    badge: {
      source: sourceLabel,
      sourceKind: item.source_kind,
      progressPercent,
      status: statusText,
      availability: item.availability,
      blocked,
    },
    to: itemRoute(ref),
    playTo: blocked ? undefined : playRoute(ref, pos),
    sortTimestamp: parseTimestampMs(tsRaw),
  }
}

function mapBookToVM(book: BookSummaryDto, view: MergedShelfView): MediaItemVM {
  const ref = { scope: 'local' as const, domain: 'book' as const, id: book.id }
  const progressPercent =
    book.progress && (book.progress.progress_percent > 0 || Boolean(book.progress.locator?.trim()))
      ? Math.min(100, Math.max(0.01, book.progress.progress_percent))
      : undefined
  const ts =
    view === 'continue'
      ? parseTimestampMs(book.last_read_at ?? book.updated_at)
      : parseTimestampMs(book.updated_at)

  return {
    ref,
    title: book.metadata.title || book.id,
    subtitle: book.metadata.authors.join(', ') || book.format.toUpperCase(),
    cover: book.has_cover ? bookApi.coverUrl(book.id, book.updated_at) : undefined,
    badge: {
      sourceKind: book.online_tracking ? 'online' : 'local',
      progressPercent,
    },
    to: itemRoute(ref),
    playTo: playRoute(ref, {
      unitIndex: book.progress?.chapter_index ?? undefined,
      position: book.progress?.progress_percent ?? 0,
    }),
    sortTimestamp: ts,
  }
}

function mapOnlineBookShelfToVM(book: OnlineShelfBookDto): MediaItemVM {
  const cover = book.cover_url ? buildOnlineCoverUrl(book.cover_url) || undefined : undefined
  const ref: TrackedItemRef = {
    scope: 'online',
    domain: 'book',
    sourceId: book.source_id,
    ref: book.book_url,
    hints: {
      title: book.name,
      cover,
      author: book.author || undefined,
      extra: {
        toc: book.toc_url,
        chapter: String(book.current_chapter_index || 0),
      },
    },
  }
  const progressPercent =
    book.total_chapters > 0
      ? Math.min(
          100,
          Math.max(0, Math.round(((book.current_chapter_index + book.scroll_ratio) / book.total_chapters) * 100)),
        )
      : undefined
  return {
    ref,
    title: book.name,
    subtitle: book.current_chapter_title || book.author || undefined,
    cover,
    badge: {
      source: book.source_name || undefined,
      sourceKind: 'online',
      progressPercent,
      availability: 'ok',
      blocked: false,
    },
    to: itemRoute(ref),
    playTo: playRoute(ref, {
      unitIndex: book.current_chapter_index,
      position: book.scroll_ratio,
    }),
    sortTimestamp: parseTimestampMs(book.updated_at),
  }
}

function mapComicContinueToVM(item: ContinueReadingItemDto): MediaItemVM {
  const ref = { scope: 'local' as const, domain: 'comic' as const, id: item.series.id }
  const ptr = item.pointer
  const progressPercent =
    ptr.total_pages > 0
      ? Math.min(100, Math.max(0, Math.round((ptr.current_page / ptr.total_pages) * 100)))
      : undefined
  const chapterRef = { scope: 'local' as const, domain: 'comic' as const, id: ptr.chapter_id }
  return {
    ref,
    title: item.series.title,
    subtitle: ptr.chapter_title || `#${ptr.chapter_number}`,
    cover: item.series.cover_url ? withAuthToken(item.series.cover_url) : undefined,
    badge: {
      sourceKind: 'local',
      progressPercent,
    },
    to: itemRoute(ref),
    playTo: playRoute(chapterRef, { position: ptr.current_page, duration: ptr.total_pages }),
    sortTimestamp: parseTimestampMs(ptr.last_read_at),
  }
}

function mapComicSeriesToVM(series: SeriesSummaryDto): MediaItemVM {
  const ref = { scope: 'local' as const, domain: 'comic' as const, id: series.id }
  const progressPercent =
    series.chapter_count > 0
      ? Math.min(100, Math.max(0, Math.round((series.completed_chapters / series.chapter_count) * 100)))
      : undefined
  return {
    ref,
    title: series.title,
    subtitle: series.writers.join(', ') || undefined,
    cover: series.cover_url ? withAuthToken(series.cover_url) : undefined,
    badge: {
      sourceKind: 'local',
      progressPercent,
    },
    to: itemRoute(ref),
    sortTimestamp: parseTimestampMs(series.updated_at),
  }
}

function mapMusicTrackToVM(track: TrackDto, bookmark?: BookmarkDto): MediaItemVM {
  const ref = { scope: 'local' as const, domain: 'music' as const, id: track.id }
  const cover = buildMusicCoverUrl(track.cover_key) ?? undefined
  const progressPercent =
    bookmark && track.duration_secs > 0
      ? Math.min(100, Math.max(0, Math.round((bookmark.position_ms / 1000 / track.duration_secs) * 100)))
      : undefined
  const to = track.album_id
    ? itemRoute({ scope: 'local', domain: 'music', id: track.album_id })
    : libraryRoute('music', { view: 'tracks', q: track.title })
  const ts = bookmark
    ? parseTimestampMs(bookmark.updated_at)
    : parseTimestampMs(track.last_played_at ?? track.starred_at ?? track.created_at)

  return {
    ref,
    title: track.title,
    subtitle: [track.artist_name, track.album_title].filter(Boolean).join(' · ') || undefined,
    cover,
    badge: {
      sourceKind: 'local',
      progressPercent,
    },
    to,
    sortTimestamp: ts,
  }
}

function mapMusicAlbumToVM(album: AlbumDto): MediaItemVM {
  const ref = { scope: 'local' as const, domain: 'music' as const, id: album.id }
  return {
    ref,
    title: album.title,
    subtitle: album.album_artist_name || undefined,
    cover: buildMusicCoverUrl(album.cover_key) ?? undefined,
    badge: {
      sourceKind: 'local',
    },
    to: itemRoute(ref),
    sortTimestamp: parseTimestampMs(album.starred_at ?? album.created_at),
  }
}

function mapVideoToVM(video: VideoSummaryDto, view: MergedShelfView): MediaItemVM {
  const ref = { scope: 'local' as const, domain: 'video' as const, id: video.id }
  const progressPercent =
    video.progress && video.progress.duration_secs > 0
      ? Math.min(
          100,
          Math.max(0, Math.round((video.progress.position_secs / video.progress.duration_secs) * 100)),
        )
      : undefined
  const to = video.show_id
    ? itemRoute({ scope: 'local', domain: 'video', id: video.show_id })
    : playRoute(ref)
  const ts =
    view === 'continue'
      ? parseTimestampMs(video.progress?.updated_at ?? video.created_at)
      : parseTimestampMs(video.created_at)

  return {
    ref,
    title: video.show_title ? `${video.show_title} · ${video.title}` : video.title,
    subtitle: video.year ? String(video.year) : undefined,
    cover: buildVideoPosterUrl(video.poster_key) ?? undefined,
    badge: {
      sourceKind: 'local',
      progressPercent,
    },
    to,
    playTo: playRoute(ref, {
      position: video.progress?.position_secs ?? 0,
      duration: video.progress?.duration_secs ?? video.duration_secs,
    }),
    sortTimestamp: ts,
  }
}

function mapShowToVM(show: ShowSummaryDto): MediaItemVM {
  const ref = { scope: 'local' as const, domain: 'video' as const, id: show.id }
  const progressPercent =
    show.episode_count > 0
      ? Math.min(100, Math.max(0, Math.round((show.watched_episode_count / show.episode_count) * 100)))
      : undefined
  return {
    ref,
    title: show.title,
    subtitle: show.year ? String(show.year) : undefined,
    cover: buildVideoPosterUrl(show.poster_key) ?? undefined,
    badge: {
      sourceKind: 'local',
      progressPercent,
    },
    to: itemRoute(ref),
    sortTimestamp: parseTimestampMs(show.created_at),
  }
}

async function fetchLocalShelfItems(
  domain: MediaKind,
  view: MergedShelfView,
  limit: number,
): Promise<{ items: MediaItemVM[]; trackedBookUrls?: Set<string> }> {
  if (domain === 'book') {
    if (view === 'continue') {
      const recent = await bookApi.listRecent(limit)
      const trackedUrls = new Set<string>()
      for (const b of recent) {
        if (b.online_tracking?.book_url) trackedUrls.add(b.online_tracking.book_url)
      }
      return { items: recent.map((b) => mapBookToVM(b, view)), trackedBookUrls: trackedUrls }
    }
    const res = await bookApi.searchBooks({
      favorites_only: true,
      page: 1,
      page_size: limit,
      sort_by: 'updated_at',
      sort_order: 'desc',
    })
    const trackedUrls = new Set<string>()
    for (const b of res.items) {
      if (b.online_tracking?.book_url) trackedUrls.add(b.online_tracking.book_url)
    }
    return { items: res.items.map((b) => mapBookToVM(b, view)), trackedBookUrls: trackedUrls }
  }

  if (domain === 'comic') {
    if (view === 'continue') {
      const home = await comicApi.homeOverview()
      return { items: home.continue_reading.map(mapComicContinueToVM) }
    }
    const list = await comicApi.listSeries({
      favorites_only: true,
      sort_by: 'updated_at',
      sort_dir: 'desc',
    })
    return { items: list.map(mapComicSeriesToVM) }
  }

  if (domain === 'music') {
    if (view === 'continue') {
      const home = await musicApi.homeOverview()
      const seen = new Set<string>()
      const out: MediaItemVM[] = []
      for (const bm of home.bookmarks) {
        seen.add(bm.track.id)
        out.push(mapMusicTrackToVM(bm.track, bm))
      }
      for (const tr of home.recent_played_tracks) {
        if (!seen.has(tr.id)) {
          seen.add(tr.id)
          out.push(mapMusicTrackToVM(tr))
        }
      }
      return { items: out }
    }
    const starred = await musicApi.getStarred()
    const out: MediaItemVM[] = [
      ...starred.albums.filter((a) => Boolean(a.starred_at)).map(mapMusicAlbumToVM),
      ...starred.tracks.filter((tr) => Boolean(tr.starred_at)).map((tr) => mapMusicTrackToVM(tr)),
    ]
    return { items: out }
  }

  // video
  if (view === 'continue') {
    const home = await videoApi.homeOverview()
    return { items: home.continue_watching.map((v) => mapVideoToVM(v, view)) }
  }
  const favs = await videoApi.listFavorites()
  return {
    items: [
      ...favs.shows.filter((s) => s.is_favorite).map(mapShowToVM),
      ...favs.videos.filter((v) => v.is_favorite).map((v) => mapVideoToVM(v, view)),
    ],
  }
}

export function useMergedShelf(
  domain: MaybeRefOrGetter<MediaKind>,
  view: MaybeRefOrGetter<MergedShelfView>,
  options: UseMergedShelfOptions = {},
): UseMergedShelfReturn {
  const autoLoad = options.autoLoad ?? true
  const limit = options.limit ?? 50
  const appClientStore = useAppClientStore()
  const { locale } = useI18n()

  const rawItems = ref<MediaItemVM[]>([])
  const loading = ref(false)
  const error = ref<string | null>(null)
  let reqSeq = 0

  const activeDomain = computed(() => toValue(domain))
  const activeView = computed(() => toValue(view))
  const activeScope = computed<SourceScope | 'all'>(() => toValue(options.scope) ?? 'all')
  const activeSourceId = computed<string | null>(() => toValue(options.sourceId) ?? null)
  const activeQuery = computed<string>(() => toValue(options.query)?.trim() ?? '')

  const remoteSourceNames = computed(() => {
    const map = new Map<string, string>()
    for (const s of appClientStore.remoteSources) {
      map.set(s.id, s.name || s.url)
    }
    return map
  })

  const items = computed<MediaItemVM[]>(() => rawItems.value)

  async function refresh(): Promise<void> {
    const d = activeDomain.value
    const v = activeView.value
    const scope = activeScope.value
    const sourceId = activeSourceId.value
    const q = activeQuery.value
    const seq = ++reqSeq

    loading.value = true
    error.value = null

    const includeLocal = scope === 'all' || scope === 'local'
    const includeOnlineOrRemote = scope === 'all' || scope === 'online' || scope === 'remote'
    const isZh = locale.value === 'zh-CN'
    const blockedLabels = {
      disabled: isZh ? '来源已停用' : 'Source disabled',
      missing: isZh ? '来源已删除' : 'Source removed',
    }

    try {
      const localPromise = includeLocal
        ? fetchLocalShelfItems(d, v, limit)
        : Promise.resolve({ items: [] as MediaItemVM[], trackedBookUrls: new Set<string>() })

      const onlinePromise: Promise<MediaItemVM[]> = (() => {
        if (!includeOnlineOrRemote) return Promise.resolve([])

        if (isOnlineItemDomain(d)) {
          const sourceKind: OnlineItemSourceKind | undefined =
            scope === 'online' || scope === 'remote' ? scope : undefined
          return onlineItemsApi
            .list(d, {
              view: v,
              source_kind: sourceKind,
              source_id: sourceId ?? undefined,
              q: q || undefined,
              limit,
            })
            .then((res) => {
              seedOnlineItemStateCache(res.items)
              return res.items.map((item) =>
                mapOnlineItemToVM(
                  item,
                  v,
                  (id) => remoteSourceNames.value.get(id),
                  blockedLabels,
                ),
              )
            })
        }

        // Book domain: merge untracked online bookshelf entries (deduped later against tracked books)
        if (scope === 'remote') return Promise.resolve([])
        return onlineBookApi.listShelf().then((shelfBooks) =>
          shelfBooks
            .filter((b) => !sourceId || b.source_id === sourceId)
            .map(mapOnlineBookShelfToVM),
        )
      })()

      const [localResult, onlineResult] = await Promise.allSettled([localPromise, onlinePromise])
      if (seq !== reqSeq) return

      const merged: MediaItemVM[] = []
      const errors: string[] = []
      let trackedBookUrls = new Set<string>()

      if (localResult.status === 'fulfilled') {
        merged.push(...localResult.value.items)
        if (localResult.value.trackedBookUrls) {
          trackedBookUrls = localResult.value.trackedBookUrls
        }
      } else {
        errors.push(
          localResult.reason instanceof Error
            ? localResult.reason.message
            : String(localResult.reason),
        )
      }

      if (onlineResult.status === 'fulfilled') {
        for (const item of onlineResult.value) {
          // Avoid duplicating tracked online books that already appear in local book results
          if (
            d === 'book' &&
            item.ref.scope === 'online' &&
            trackedBookUrls.has(item.ref.ref)
          ) {
            continue
          }
          merged.push(item)
        }
      } else {
        errors.push(
          onlineResult.reason instanceof Error
            ? onlineResult.reason.message
            : String(onlineResult.reason),
        )
      }

      const needle = q.toLowerCase()
      const filtered = needle
        ? merged.filter(
            (m) =>
              m.title.toLowerCase().includes(needle) ||
              (m.subtitle?.toLowerCase().includes(needle) ?? false),
          )
        : merged

      filtered.sort((a, b) => (b.sortTimestamp ?? 0) - (a.sortTimestamp ?? 0))
      rawItems.value = limit > 0 ? filtered.slice(0, limit) : filtered

      if (errors.length > 0 && merged.length === 0) {
        error.value = errors[0] ?? null
      }
    } finally {
      if (seq === reqSeq) {
        loading.value = false
      }
    }
  }

  if (autoLoad) {
    watch(
      [activeDomain, activeView, activeScope, activeSourceId, activeQuery],
      () => {
        void refresh()
      },
      { immediate: true },
    )
  }

  return {
    items,
    loading,
    error,
    refresh,
  }
}

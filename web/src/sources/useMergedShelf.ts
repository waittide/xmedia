import {
  computed,
  ref,
  toValue,
  watch,
  type ComputedRef,
  type MaybeRefOrGetter,
  type Ref,
} from 'vue'
import { withAuthToken } from '@/api/client'
import { api as bookApi } from '@/domains/book/api'
import type { BookSummaryDto } from '@/domains/book/types'
import { api as comicApi } from '@/domains/comic/api'
import type { ContinueReadingItemDto, SeriesSummaryDto } from '@/domains/comic/types'
import { api as musicApi, buildCoverUrl as buildMusicCoverUrl } from '@/domains/music/api'
import type { AlbumDto, BookmarkDto, TrackDto } from '@/domains/music/types'
import { api as videoApi, buildPosterUrl as buildVideoPosterUrl } from '@/domains/video/api'
import type { ShowSummaryDto, VideoSummaryDto } from '@/domains/video/types'
import type { MediaKind } from '@/types/api'
import { itemRoute, libraryRoute, playRoute } from './routes'
import type { MediaItemVM, SourceScope } from './types'

export type MergedShelfView = 'continue' | 'favorites'

export interface UseMergedShelfOptions {
  limit?: number
  /** Filter by source scope. Default: `'all'` (reads bound local library items). */
  scope?: MaybeRefOrGetter<SourceScope | 'all'>
  /** Unused; kept for signature compatibility. */
  sourceId?: MaybeRefOrGetter<string | null>
  /** Optional client-side keyword filter. */
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
      sourceKind: item.series.online_tracking ? 'online' : 'local',
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
      sourceKind: series.online_tracking ? 'online' : 'local',
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
      sourceKind: track.online_tracking ? 'online' : 'local',
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
      sourceKind: video.online_tracking ? 'online' : 'local',
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
      sourceKind: show.online_tracking ? 'online' : 'local',
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
): Promise<MediaItemVM[]> {
  if (domain === 'book') {
    if (view === 'continue') {
      const recent = await bookApi.listRecent(limit)
      return recent.map((b) => mapBookToVM(b, view))
    }
    const res = await bookApi.searchBooks({
      favorites_only: true,
      page: 1,
      page_size: limit,
      sort_by: 'updated_at',
      sort_order: 'desc',
    })
    return res.items.map((b) => mapBookToVM(b, view))
  }

  if (domain === 'comic') {
    if (view === 'continue') {
      const home = await comicApi.homeOverview()
      return home.continue_reading.map(mapComicContinueToVM)
    }
    const list = await comicApi.listSeries({
      favorites_only: true,
      sort_by: 'updated_at',
      sort_dir: 'desc',
    })
    return list.map(mapComicSeriesToVM)
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
      return out
    }
    const starred = await musicApi.getStarred()
    return [
      ...starred.albums.filter((a) => Boolean(a.starred_at)).map(mapMusicAlbumToVM),
      ...starred.tracks.filter((tr) => Boolean(tr.starred_at)).map((tr) => mapMusicTrackToVM(tr)),
    ]
  }

  // video
  if (view === 'continue') {
    const home = await videoApi.homeOverview()
    return home.continue_watching.map((v) => mapVideoToVM(v, view))
  }
  const favs = await videoApi.listFavorites()
  return [
    ...favs.shows.filter((s) => s.is_favorite).map(mapShowToVM),
    ...favs.videos.filter((v) => v.is_favorite).map((v) => mapVideoToVM(v, view)),
  ]
}

export function useMergedShelf(
  domain: MaybeRefOrGetter<MediaKind>,
  view: MaybeRefOrGetter<MergedShelfView>,
  options: UseMergedShelfOptions = {},
): UseMergedShelfReturn {
  const autoLoad = options.autoLoad ?? true
  const limit = options.limit ?? 50

  const rawItems = ref<MediaItemVM[]>([])
  const loading = ref(false)
  const error = ref<string | null>(null)
  let reqSeq = 0

  const activeDomain = computed(() => toValue(domain))
  const activeView = computed(() => toValue(view))
  const activeScope = computed<SourceScope | 'all'>(() => toValue(options.scope) ?? 'all')
  const activeQuery = computed<string>(() => toValue(options.query)?.trim() ?? '')

  const items = computed<MediaItemVM[]>(() => rawItems.value)

  async function refresh(): Promise<void> {
    const d = activeDomain.value
    const v = activeView.value
    const scope = activeScope.value
    const q = activeQuery.value
    const seq = ++reqSeq

    // Only bound local library items persist continue/favorites state.
    if (scope !== 'all' && scope !== 'local') {
      rawItems.value = []
      error.value = null
      loading.value = false
      return
    }

    loading.value = true
    error.value = null

    try {
      const localItems = await fetchLocalShelfItems(d, v, limit)
      if (seq !== reqSeq) return

      const needle = q.toLowerCase()
      const filtered = needle
        ? localItems.filter(
            (m) =>
              m.title.toLowerCase().includes(needle) ||
              (m.subtitle?.toLowerCase().includes(needle) ?? false),
          )
        : localItems

      filtered.sort((a, b) => (b.sortTimestamp ?? 0) - (a.sortTimestamp ?? 0))
      rawItems.value = limit > 0 ? filtered.slice(0, limit) : filtered
    } catch (err) {
      if (seq !== reqSeq) return
      error.value = err instanceof Error ? err.message : String(err)
    } finally {
      if (seq === reqSeq) {
        loading.value = false
      }
    }
  }

  if (autoLoad) {
    watch(
      [activeDomain, activeView, activeScope, activeQuery],
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

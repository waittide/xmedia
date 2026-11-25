import type { OnlineSourceManagerAdapter, OnlineSourceSummaryDto } from '@/components/online-sources/types'
import { requestJson, withAuthToken } from '@/api/client'
import { fetchHomeOverview } from '@/config/homeCache'
import { api as comicApi } from '@/domains/comic/api'
import { ensureComicLocale } from '@/domains/comic/i18n'
import { comicSourceAdapter } from '@/domains/comic/onlineApi'
import { useSidebarStore as useComicSidebarStore } from '@/domains/comic/store'
import type { ContinueReadingItemDto, SeriesSummaryDto } from '@/domains/comic/types'
import type { DomainPlugin } from '../domainRegistry'
import { itemRoute, playRoute } from '../routes'
import type { MediaItemVM } from '../types'
import { parseTimestampMs } from '../useMergedShelf'

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

export const comicDomainPlugin: DomainPlugin = {
  kind: 'comic',
  onlineSearchI18n: {
    titleKey: 'globalSearch.onlineComicSearchTitle',
    subtitleKey: 'globalSearch.onlineComicSearchSubtitle',
  },
  ensureLocale: () => ensureComicLocale(),
  resetSessionState: () => {
    useComicSidebarStore().resetState()
  },
  fetchHomeOverview: () => comicApi.homeOverview(),
  fetchLocalShelfItems: async (view) => {
    if (view === 'continue') {
      const home = await fetchHomeOverview('comic')
      return home.continue_reading.map(mapComicContinueToVM)
    }
    const list = await comicApi.listSeries({
      favorites_only: true,
      sort_by: 'updated_at',
      sort_dir: 'desc',
    })
    return list.map(mapComicSeriesToVM)
  },
  searchLocal: async ({ query, limit, signal }) => {
    const sp = new URLSearchParams({
      search: query,
      limit: String(limit),
    })
    const res = await requestJson<SeriesSummaryDto[]>(`/comic/series?${sp.toString()}`, {
      signal,
    })
    const items: MediaItemVM[] = res.slice(0, limit).map((s) => {
      const ref = { scope: 'local' as const, domain: 'comic' as const, id: s.id }
      return {
        ref,
        title: s.title,
        subtitle: s.writers?.join(', ') || `${s.chapter_count} ch`,
        cover: s.cover_url ? withAuthToken(s.cover_url) : undefined,
        badge: { sourceKind: 'local' },
        to: itemRoute(ref),
      }
    })
    return { items, total: res.length }
  },
  upload: {
    extensions: ['cbz', 'zip', 'epub', 'cbr', 'cb7'],
    executeUpload: async (files, { options, sendXhr }) => {
      const file = files[0]!
      const fd = new FormData()
      const sid = options?.comicTargetSeriesId ?? options?.targetSeriesId
      const stitle = options?.comicSeriesTitle ?? options?.seriesTitle
      if (sid) fd.append('target_series_id', sid)
      if (stitle) fd.append('series_title', stitle)
      fd.append('file', file, file.name)
      const res = await sendXhr<{
        uploaded?: { series_id: string }[]
        series_id?: string
      }>('/api/v1/comic/upload', fd)
      const resolvedSeriesId = res.uploaded?.[0]?.series_id ?? res.series_id
      if (!resolvedSeriesId) {
        throw new Error('No comic archive was imported')
      }
      return { targetRoute: `/comic/item/${resolvedSeriesId}` }
    },
  },
  pool: {
    pageSize: 24,
    paginated: false,
    skeletonMode: 'grid',
    fetchPool: async ({ query, libraryId }, t) => {
      const list = await comicApi.listPool({
        search: query,
        library_id: libraryId,
      })
      return {
        kind: 'posters',
        total: list.length,
        items: list.map((series) => ({
          id: series.id,
          title: series.title,
          subtitle: t('pool.chaptersCount', { count: series.chapter_count }),
          coverUrl: series.cover_url ? withAuthToken(series.cover_url) : null,
          bound: series.bound,
        })),
      }
    },
    bindItems: (payload) => comicApi.bindItems(payload),
    unbindItem: (id, userId) => comicApi.unbindItem(id, userId),
  },
  onlineSources: {
    remoteProtocol: 'opds',
    getAdapter: () => comicSourceAdapter as OnlineSourceManagerAdapter<OnlineSourceSummaryDto>,
    loadEditSheet: () => import('@/domains/comic/components/ComicSourceEditSheet.vue'),
    loadDebugSheet: () => import('@/domains/comic/components/ComicSourceDebugSheet.vue'),
  },
}

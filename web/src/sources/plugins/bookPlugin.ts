import type { OnlineSourceManagerAdapter, OnlineSourceSummaryDto } from '@/components/online-sources/types'
import { requestJson } from '@/api/client'
import { fetchHomeOverview, hasHomeOverviewAvailable } from '@/config/homeCache'
import { api as bookApi } from '@/domains/book/api'
import { ensureBookLocale } from '@/domains/book/i18n'
import { bookSourceAdapter, buildOnlineDownloadFileUrl } from '@/domains/book/onlineApi'
import { useCatalogStore } from '@/domains/book/store'
import type { BookSummaryDto, PagedResult as BookPagedResult } from '@/domains/book/types'
import {
  formatDownloadSpeed,
  isOnlineDownloadJobActive,
  useOnlineDownloadsStore,
} from '@/stores/onlineDownloads'
import { formatBytes } from '@/utils/format'
import type { DomainBackgroundTasksProvider, DomainPlugin, MergedShelfView } from '../domainRegistry'
import { itemRoute, playRoute } from '../routes'
import type { MediaItemVM } from '../types'
import { parseTimestampMs } from '../useMergedShelf'

function parseIsoMs(iso?: string): number {
  if (!iso) return 0
  const parsed = Date.parse(iso)
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

function createBookBackgroundTasksProvider(): DomainBackgroundTasksProvider {
  const store = useOnlineDownloadsStore()
  return {
    getTasks: (t) =>
      store.jobs.map((job) => {
        const isActive = isOnlineDownloadJobActive(job.status)
        const status = job.status === 'pending' ? 'queued' : job.status
        const progress =
          job.status === 'completed'
            ? 100
            : job.total_chapters > 0
              ? Math.min(100, Math.round((job.processed_chapters / job.total_chapters) * 100))
              : null

        const chaptersText = t('transfers.chaptersProgress', {
          processed: job.processed_chapters.toLocaleString(),
          total: job.total_chapters > 0 ? job.total_chapters.toLocaleString() : '?',
        })
        const speed = isActive ? formatDownloadSpeed(job.chapters_per_sec) : ''

        let metaText = ''
        if (isActive) {
          metaText = speed ? `${chaptersText} · ${speed}` : chaptersText
        } else if (job.status === 'completed') {
          const sizeText = job.file_size ? ` · ${formatBytes(job.file_size)}` : ''
          metaText = `${t('transfers.statusPacked')} · ${chaptersText}${sizeText}`
        } else if (job.status === 'cancelled') {
          metaText = `${t('transfers.statusCancelled')} · ${chaptersText}`
        } else {
          metaText = `${t('transfers.statusFailed')} · ${chaptersText}`
        }

        return {
          id: job.id,
          domain: 'book' as const,
          status,
          isActive,
          sortTime: parseIsoMs(job.updated_at || job.created_at),
          title: job.book_name,
          badge: job.format.toUpperCase(),
          metaText,
          secondaryText:
            isActive && job.current_chapter_title ? job.current_chapter_title : undefined,
          warningText:
            job.failed_chapters > 0
              ? t('transfers.failedChaptersNote', { count: job.failed_chapters })
              : undefined,
          errorText: job.status === 'failed' ? job.error || undefined : undefined,
          progress,
          targetRoute:
            job.status === 'completed' && job.imported_book_id
              ? `/book/item/${job.imported_book_id}`
              : undefined,
          downloadFileUrl:
            job.status === 'completed' ? buildOnlineDownloadFileUrl(job.id) : undefined,
          downloadFileName: job.output_file_name || undefined,
          canCancel: isActive,
          canDelete: !isActive,
          busy: store.busyJobId === job.id,
        }
      }),
    getActiveCount: () => store.activeJobs.length,
    getFailedCount: () => store.failedCount,
    getFinishedCount: () => store.finishedCount,
    refresh: () => store.refresh(),
    cancelTask: (id) => store.cancelJob(id),
    deleteTask: (id) => store.deleteJob(id),
    clearFinished: () => store.clearFinished(),
    resetState: () => store.resetState(),
  }
}

export const bookDomainPlugin: DomainPlugin = {
  kind: 'book',
  onlineSearchI18n: {
    titleKey: 'globalSearch.onlineBookSearchTitle',
    subtitleKey: 'globalSearch.onlineBookSearchSubtitle',
  },
  ensureLocale: () => ensureBookLocale(),
  resetSessionState: () => {
    useCatalogStore().resetState()
  },
  fetchHomeOverview: () => bookApi.homeOverview(),
  fetchLocalShelfItems: async (view, limit) => {
    if (view === 'continue') {
      if (hasHomeOverviewAvailable('book') || limit <= 6) {
        const home = await fetchHomeOverview('book')
        if (
          home.stats.reading_count <= home.continue_reading.length ||
          limit <= home.continue_reading.length
        ) {
          return home.continue_reading.slice(0, limit).map((b) => mapBookToVM(b, view))
        }
      }
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
  },
  searchLocal: async ({ query, limit, signal }) => {
    const sp = new URLSearchParams({
      keyword: query,
      page: '1',
      page_size: String(limit),
    })
    const res = await requestJson<BookPagedResult<BookSummaryDto>>(`/book/books?${sp.toString()}`, {
      signal,
    })
    const items: MediaItemVM[] = res.items.slice(0, limit).map((b) => {
      const ref = { scope: 'local' as const, domain: 'book' as const, id: b.id }
      return {
        ref,
        title: b.metadata.title || b.id,
        subtitle: b.metadata.authors.join(', ') || b.format.toUpperCase(),
        cover: b.has_cover ? bookApi.coverUrl(b.id, b.updated_at) : undefined,
        badge: { sourceKind: b.online_tracking ? 'online' : 'local' },
        to: itemRoute(ref),
        playTo: playRoute(ref),
      }
    })
    return { items, total: res.total }
  },
  upload: {
    extensions: ['epub', 'txt', 'pdf', 'mobi', 'azw3', 'fb2'],
    executeUpload: async (files, { sendXhr }) => {
      const file = files[0]!
      const fd = new FormData()
      fd.append('file', file, file.name)
      const res = await sendXhr<{ uploaded?: { id: string }[] }>('/api/v1/book/books/upload', fd)
      if (!res.uploaded || res.uploaded.length === 0) {
        throw new Error('No book files were imported')
      }
      return {
        targetRoute: res.uploaded[0]?.id ? `/book/item/${res.uploaded[0].id}` : undefined,
      }
    },
  },
  pool: {
    pageSize: 24,
    paginated: true,
    skeletonMode: 'grid',
    fetchPool: async ({ query, libraryId, page, pageSize }) => {
      const res = await bookApi.listPool({
        keyword: query,
        library_id: libraryId,
        page,
        page_size: pageSize,
        sort_by: 'created_at',
        sort_order: 'desc',
      })
      return {
        kind: 'posters',
        total: res.total,
        items: res.items.map((book) => ({
          id: book.id,
          title: book.metadata.title,
          subtitle: book.metadata.authors.join(', ') || book.library_name || '—',
          coverUrl: book.has_cover ? bookApi.coverUrl(book.id, book.updated_at) : null,
          bound: book.bound,
          badgeText: book.format,
        })),
      }
    },
    bindItems: (payload) => bookApi.bindItems(payload),
    unbindItem: (id, userId) => bookApi.unbindItem(id, userId),
  },
  onlineSources: {
    remoteProtocol: 'opds',
    getAdapter: () => bookSourceAdapter as OnlineSourceManagerAdapter<OnlineSourceSummaryDto>,
    loadEditSheet: () => import('@/domains/book/components/OnlineSourceEditSheet.vue'),
    loadDebugSheet: () => import('@/domains/book/components/OnlineSourceDebugSheet.vue'),
  },
  getBackgroundTasks: createBookBackgroundTasksProvider,
}

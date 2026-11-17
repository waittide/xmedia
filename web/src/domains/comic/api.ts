import { api as coreApi, requestJson, withAuthToken } from '@/api/client'
import type { CoreLibraryDto } from '@/types/api'
import type {
  ChapterItemDto,
  ChapterMetadataPatch,
  EffectiveReaderPrefDto,
  HomeOverviewDto,
  Library,
  ReaderContextDto,
  ScanResultDto,
  Series,
  SeriesDetailDto,
  SeriesMetadataPatch,
  SeriesQueryFilter,
  SeriesReaderPref,
  SeriesSummaryDto,
  Shelf,
  ShelfDetailDto,
  TaxonomyOverviewDto,
} from './types'

export const withAuthUrl = withAuthToken

export function chapterPageUrl(chapterId: string, pageNum: number): string {
  return withAuthToken(`/api/v1/comic/chapters/${encodeURIComponent(chapterId)}/pages/${pageNum}`)
}

export function mapComicLibrary(raw: CoreLibraryDto): Library {
  let scan_state: Library['scan_state']
  if (raw.scan_status.state === 'scanning') {
    scan_state = { state: 'scanning', started_at: Date.now() }
  } else if (raw.scan_status.state === 'completed') {
    scan_state = {
      state: 'completed',
      finished_at: Date.now(),
      series_count: raw.item_count,
      chapter_count: raw.scan_status.scanned,
    }
  } else if (raw.scan_status.state === 'failed') {
    scan_state = {
      state: 'failed',
      finished_at: Date.now(),
      error: raw.scan_status.message,
    }
  } else {
    scan_state = { state: 'idle' }
  }

  return {
    id: raw.id,
    name: raw.name,
    root_path: raw.kind.type === 'local_mount' ? raw.kind.root_path : '',
    kind: raw.kind.type === 'managed' ? 'managed_upload' : 'local_mount',
    scan_state,
    series_count: raw.item_count,
    created_at: raw.created_at,
    updated_at: raw.updated_at,
  }
}

export const api = {
  listLibraries: async (): Promise<Library[]> => {
    const libs = await coreApi.listLibraries('comic')
    return libs.map(mapComicLibrary)
  },

  createLibrary: async (payload: { name: string; root_path: string }): Promise<Library> => {
    const created = await coreApi.createLibrary({
      name: payload.name,
      media_kind: 'comic',
      root_path: payload.root_path,
    })
    return mapComicLibrary(created)
  },

  deleteLibrary: async (id: string): Promise<void> => {
    await coreApi.deleteLibrary(id)
  },

  scanLibrary: async (id: string): Promise<ScanResultDto> => {
    const scanned = await coreApi.scanLibrary(id)
    const mapped = mapComicLibrary(scanned)
    return {
      library: mapped,
      series_count: scanned.item_count,
      chapter_count: scanned.item_count,
      missing_count: 0,
    }
  },

  scanAllLibraries: async (): Promise<ScanResultDto[]> => {
    const libs = await coreApi.listLibraries('comic')
    const results: ScanResultDto[] = []
    for (const lib of libs) {
      if (lib.kind.type === 'local_mount' && lib.scan_status.state !== 'scanning') {
        const scanned = await coreApi.scanLibrary(lib.id)
        results.push({
          library: mapComicLibrary(scanned),
          series_count: scanned.item_count,
          chapter_count: scanned.item_count,
          missing_count: 0,
        })
      }
    }
    return results
  },

  homeOverview: () => requestJson<HomeOverviewDto>('/comic/home'),
  taxonomyOverview: () => requestJson<TaxonomyOverviewDto>('/comic/taxonomy'),

  listSeries: (filter: SeriesQueryFilter = {}) => {
    const params = new URLSearchParams()
    if (filter.search) params.set('search', filter.search)
    if (filter.library_id) params.set('library_id', filter.library_id)
    if (filter.status) params.set('status', filter.status)
    if (filter.writer) params.set('writer', filter.writer)
    if (filter.penciller) params.set('penciller', filter.penciller)
    if (filter.tag) params.set('tag', filter.tag)
    if (filter.genre) params.set('genre', filter.genre)
    if (filter.read_status) params.set('read_status', filter.read_status)
    if (filter.favorites_only) params.set('favorites_only', 'true')
    if (filter.shelf_id) params.set('shelf_id', filter.shelf_id)
    if (filter.sort_by) params.set('sort_by', filter.sort_by)
    if (filter.sort_dir) params.set('sort_dir', filter.sort_dir)
    const qs = params.toString()
    return requestJson<SeriesSummaryDto[]>(`/comic/series${qs ? `?${qs}` : ''}`)
  },

  getSeriesDetail: (id: string) =>
    requestJson<SeriesDetailDto>(`/comic/series/${encodeURIComponent(id)}`),

  updateSeriesMetadata: (id: string, patch: SeriesMetadataPatch) =>
    requestJson<Series>(`/comic/series/${encodeURIComponent(id)}`, {
      method: 'PATCH',
      body: JSON.stringify(patch),
    }),

  uploadSeriesCover: (id: string, file: File) => {
    const form = new FormData()
    form.append('file', file)
    return requestJson<Series>(`/comic/series/${encodeURIComponent(id)}/cover`, {
      method: 'POST',
      body: form,
    })
  },

  deleteSeries: (id: string) =>
    requestJson<void>(`/comic/series/${encodeURIComponent(id)}`, { method: 'DELETE' }),

  setFavorite: (seriesId: string, favorite: boolean) =>
    requestJson<void>(`/comic/series/${encodeURIComponent(seriesId)}/favorite`, {
      method: 'PUT',
      body: JSON.stringify({ favorite }),
    }),

  setSeriesShelves: (seriesId: string, shelfIds: string[]) =>
    requestJson<void>(`/comic/series/${encodeURIComponent(seriesId)}/shelves`, {
      method: 'PUT',
      body: JSON.stringify({ shelf_ids: shelfIds }),
    }),

  saveSeriesReaderPref: (
    seriesId: string,
    pref: Pick<
      EffectiveReaderPrefDto,
      'reader_mode' | 'fit_mode' | 'offset_cover_page' | 'webtoon_width'
    >,
  ) =>
    requestJson<SeriesReaderPref>(
      `/comic/series/${encodeURIComponent(seriesId)}/reader-pref`,
      { method: 'PUT', body: JSON.stringify(pref) },
    ),

  markSeriesRead: (seriesId: string, completed: boolean) =>
    requestJson<{ marked_count: number }>(
      `/comic/series/${encodeURIComponent(seriesId)}/mark-read`,
      { method: 'POST', body: JSON.stringify({ completed }) },
    ),

  markVolumeRead: (seriesId: string, volumeNumber: number, completed: boolean) =>
    requestJson<{ marked_count: number }>(
      `/comic/series/${encodeURIComponent(seriesId)}/mark-volume-read`,
      { method: 'POST', body: JSON.stringify({ volume_number: volumeNumber, completed }) },
    ),

  getReaderContext: (chapterId: string) =>
    requestJson<ReaderContextDto>(`/comic/chapters/${encodeURIComponent(chapterId)}/reader`),

  updateChapterMetadata: (chapterId: string, patch: ChapterMetadataPatch) =>
    requestJson<ChapterItemDto>(`/comic/chapters/${encodeURIComponent(chapterId)}`, {
      method: 'PATCH',
      body: JSON.stringify(patch),
    }),

  updateProgress: (chapterId: string, currentPage: number, isCompleted?: boolean) =>
    requestJson<void>(`/comic/chapters/${encodeURIComponent(chapterId)}/progress`, {
      method: 'PUT',
      body: JSON.stringify({ current_page: currentPage, is_completed: isCompleted }),
    }),

  setChapterCompleted: (chapterId: string, completed: boolean) =>
    requestJson<void>(`/comic/chapters/${encodeURIComponent(chapterId)}/complete`, {
      method: 'POST',
      body: JSON.stringify({ completed }),
    }),

  markPreviousRead: (chapterId: string) =>
    requestJson<{ marked_count: number }>(
      `/comic/chapters/${encodeURIComponent(chapterId)}/mark-previous-read`,
      { method: 'POST' },
    ),

  listShelves: () => requestJson<ShelfDetailDto[]>('/comic/shelves'),

  createShelf: (payload: { name: string; description?: string; series_ids?: string[] }) =>
    requestJson<Shelf>('/comic/shelves', {
      method: 'POST',
      body: JSON.stringify(payload),
    }),

  updateShelf: (
    id: string,
    payload: { name: string; description?: string; series_ids?: string[] },
  ) =>
    requestJson<Shelf>(`/comic/shelves/${encodeURIComponent(id)}`, {
      method: 'PUT',
      body: JSON.stringify(payload),
    }),

  deleteShelf: (id: string) =>
    requestJson<void>(`/comic/shelves/${encodeURIComponent(id)}`, { method: 'DELETE' }),
}

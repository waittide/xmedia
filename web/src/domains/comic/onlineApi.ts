import { requestJson, withAuthToken } from '@/api/client'
import { createDomainSourceApi } from '@/components/online-sources/createDomainSourceApi'
import {
  isSafeDataImageUrl,
  isSafeHttpUrl,
  type BaseSourceTestReportDto,
  type ExecutionDiagnostics,
  type OnlineSourceManagerAdapter,
  type OnlineSourceSummaryDto,
} from '@/components/online-sources/types'
import type { ChapterItemDto, SeriesDetailDto } from './types'

export type ComicSourceSummaryDto = OnlineSourceSummaryDto
export type ComicReadingDirection = 'rtl' | 'ltr' | 'webtoon'
export type ComicSourceTestMode = 'full' | 'search' | 'explore' | 'detail' | 'chapters' | 'pages'

export interface ComicSourceTestInput {
  keyword?: string | null
  mode?: ComicSourceTestMode
  target_url?: string | null
  explore_url?: string | null
  comic_url?: string | null
  toc_url?: string | null
  chapter_url?: string | null
  chapter_index?: number | null
  chapter_title?: string | null
}

export interface ComicSearchResult {
  title: string
  author: string
  comic_url: string
  cover_url: string | null
  intro: string | null
  kind: string | null
  last_chapter: string | null
  word_count: string | null
}

export interface ComicSearchResultItem extends ComicSearchResult {
  source_id: string
  source_name: string
  proxy_cover_url: string | null
}

export interface ComicExploreCategoryItem {
  title: string
  url: string
}

export interface OnlineComicDetail {
  title: string
  author: string
  comic_url: string
  toc_url: string
  cover_url: string | null
  intro: string | null
  kind: string | null
  status: string | null
  last_chapter: string | null
  word_count: string | null
  reading_direction: ComicReadingDirection | null
}

export interface OnlineComicDetailDto extends Omit<OnlineComicDetail, 'reading_direction'> {
  proxy_cover_url: string | null
  reading_direction: ComicReadingDirection
}

export interface OnlineComicChapter {
  index: number
  title: string
  url: string
  update_time: string | null
  is_volume: boolean
}

export interface OnlineComicPageDto {
  index: number
  image_url: string
  proxy_url: string
}

export interface ComicSourceTestReportDto extends BaseSourceTestReportDto<ComicSourceTestMode> {
  search_items: ComicSearchResult[]
  explore_categories: ComicExploreCategoryItem[]
  comic_detail: OnlineComicDetail | null
  chapters: OnlineComicChapter[]
  chapters_total_count: number
  chapter_index: number | null
  chapter_title: string | null
  chapter_url: string | null
  pages: OnlineComicPageDto[]
  page_count: number
}

export interface OnlineComicSearchErrorDto {
  source_id: string
  source_name: string
  error: string
}

export interface OnlineComicSearchResponseDto {
  keyword: string
  page: number
  results: ComicSearchResultItem[]
  errors: OnlineComicSearchErrorDto[]
  toasts: string[]
}

export interface OnlineComicExploreResponseDto {
  source_id: string
  explore_url: string
  page: number
  items: ComicSearchResultItem[]
  categories: ComicExploreCategoryItem[]
  diagnostics: ExecutionDiagnostics
}

export interface OnlineComicDetailResponseDto {
  source_id: string
  source_name: string
  detail: OnlineComicDetailDto
  chapters: OnlineComicChapter[]
  diagnostics: ExecutionDiagnostics
}

export interface OnlineComicChaptersResponseDto {
  source_id: string
  source_name: string
  toc_url: string
  chapters: OnlineComicChapter[]
  diagnostics: ExecutionDiagnostics
}

export interface OnlineComicPagesResponseDto {
  source_id: string
  source_name: string
  chapter_url: string
  reading_direction: ComicReadingDirection
  pages: OnlineComicPageDto[]
  diagnostics: ExecutionDiagnostics
}

export interface TrackOnlineComicInput {
  source_id: string
  comic_url: string
  toc_url?: string | null
  title?: string | null
  author?: string | null
  cover_url?: string | null
  summary?: string | null
  reading_direction?: ComicReadingDirection | null
  genres?: string[] | null
  status?: string | null
}

export interface ComicCacheReportDto {
  series_id: string
  total_requested: number
  cached_now: number
  already_cached: number
  failed: string[]
}

export const ONLINE_COMIC_CTX_KEY = 'xmedia_online_comic_ctx'
export const ONLINE_COMIC_READER_CTX_KEY = 'xmedia_online_comic_reader_ctx'

export interface OnlineComicContextStorage {
  sourceId: string
  sourceName?: string
  comicUrl: string
  tocUrl?: string
  title?: string
  author?: string
  coverUrl?: string | null
  intro?: string | null
  readingDirection?: ComicReadingDirection | null
  returnKeyword?: string
  returnSourceId?: string | null
}

export interface OnlineComicReaderContextStorage {
  sourceId: string
  sourceName?: string
  comicUrl: string
  tocUrl?: string
  title?: string
  author?: string
  coverUrl?: string | null
  readingDirection?: ComicReadingDirection | null
  chapterUrl: string
  chapterIndex: number
  chapterTitle?: string
  chapters?: OnlineComicChapter[]
}

export function buildOnlineComicImageUrl(
  rawUrl: string | null | undefined,
  sourceId?: string | null,
): string {
  if (!rawUrl || !rawUrl.trim()) return ''
  const u = rawUrl.trim()
  if (u.toLowerCase().startsWith('data:')) {
    return isSafeDataImageUrl(u) ? u : ''
  }
  if (!isSafeHttpUrl(u)) return ''
  const qs = new URLSearchParams({ url: u })
  if (sourceId) qs.set('source_id', sourceId)
  return withAuthToken(`/api/v1/comic/online/image-proxy?${qs.toString()}`)
}

export function resolveOnlineComicCoverUrl(
  proxyCoverUrl?: string | null,
  rawCoverUrl?: string | null,
  sourceId?: string | null,
): string {
  const proxy = proxyCoverUrl?.trim()
  if (proxy) {
    if (proxy.toLowerCase().startsWith('data:')) {
      return isSafeDataImageUrl(proxy) ? proxy : ''
    }
    if (proxy.startsWith('/') && !proxy.startsWith('//')) {
      try {
        const parsed = new URL(proxy, 'http://localhost')
        if (parsed.origin !== 'http://localhost') return ''
        if (parsed.pathname !== '/api/v1/comic/online/image-proxy') return ''
        const innerUrl = parsed.searchParams.get('url')?.trim()
        if (!innerUrl || !isSafeHttpUrl(innerUrl)) return ''
        return withAuthToken(proxy)
      } catch {
        return ''
      }
    }
    if (isSafeHttpUrl(proxy)) {
      return withAuthToken(proxy)
    }
    return ''
  }
  return buildOnlineComicImageUrl(rawCoverUrl, sourceId)
}

const baseCrud = createDomainSourceApi<ComicSourceSummaryDto>('comic', { hasSeeds: true })

export const onlineComicApi = {
  ...baseCrud,
  importSeeds: baseCrud.importSeeds!,

  testSource: (
    id: string,
    payload?: string | ComicSourceTestInput,
    options?: { signal?: AbortSignal },
  ): Promise<ComicSourceTestReportDto> => {
    const body: ComicSourceTestInput =
      typeof payload === 'string'
        ? { keyword: payload.trim() || null }
        : {
            keyword: payload?.keyword?.trim() || null,
            mode: payload?.mode ?? 'full',
            target_url: payload?.target_url?.trim() || null,
            explore_url: payload?.explore_url?.trim() || null,
            comic_url: payload?.comic_url?.trim() || null,
            toc_url: payload?.toc_url?.trim() || null,
            chapter_url: payload?.chapter_url?.trim() || null,
            chapter_index: payload?.chapter_index ?? null,
            chapter_title: payload?.chapter_title?.trim() || null,
          }
    return requestJson(`/comic/sources/${encodeURIComponent(id)}/test`, {
      method: 'POST',
      body: JSON.stringify(body),
      signal: options?.signal,
    })
  },

  search: (
    keyword: string,
    sourceIds?: string[],
    page = 1,
    options?: { signal?: AbortSignal },
  ): Promise<OnlineComicSearchResponseDto> =>
    requestJson('/comic/online/search', {
      method: 'POST',
      body: JSON.stringify({
        keyword,
        source_ids: sourceIds && sourceIds.length > 0 ? sourceIds : null,
        page,
      }),
      signal: options?.signal,
    }),

  explore: (
    sourceId: string,
    exploreUrl?: string | null,
    page = 1,
    options?: { signal?: AbortSignal },
  ): Promise<OnlineComicExploreResponseDto> =>
    requestJson('/comic/online/explore', {
      method: 'POST',
      body: JSON.stringify({
        source_id: sourceId,
        explore_url: exploreUrl ?? null,
        page,
      }),
      signal: options?.signal,
    }),

  fetchDetail: (
    sourceId: string,
    comicUrl: string,
    fallbackHint?: Partial<ComicSearchResult> | null,
    options?: { signal?: AbortSignal },
  ): Promise<OnlineComicDetailResponseDto> =>
    requestJson('/comic/online/detail', {
      method: 'POST',
      body: JSON.stringify({
        source_id: sourceId,
        comic_url: comicUrl,
        fallback_hint: fallbackHint ?? null,
      }),
      signal: options?.signal,
    }),

  fetchChapters: (
    sourceId: string,
    tocUrl: string,
    comic?: Partial<OnlineComicDetail> | null,
    options?: { signal?: AbortSignal },
  ): Promise<OnlineComicChaptersResponseDto> =>
    requestJson('/comic/online/chapters', {
      method: 'POST',
      body: JSON.stringify({
        source_id: sourceId,
        toc_url: tocUrl,
        comic: comic ?? undefined,
      }),
      signal: options?.signal,
    }),

  fetchPages: (
    sourceId: string,
    chapterUrl: string,
    comicUrl?: string | null,
    options?: {
      comic?: Partial<OnlineComicDetail> | null
      chapter?: Partial<OnlineComicChapter> | null
      signal?: AbortSignal
    },
  ): Promise<OnlineComicPagesResponseDto> =>
    requestJson('/comic/online/pages', {
      method: 'POST',
      body: JSON.stringify({
        source_id: sourceId,
        chapter_url: chapterUrl,
        comic_url: comicUrl ?? null,
        comic: options?.comic ?? undefined,
        chapter: options?.chapter ?? undefined,
      }),
      signal: options?.signal,
    }),

  trackOnlineComic: (payload: TrackOnlineComicInput): Promise<SeriesDetailDto> =>
    requestJson('/comic/online/track', {
      method: 'POST',
      body: JSON.stringify(payload),
    }),

  refreshTrackedSeries: (seriesId: string): Promise<SeriesDetailDto> =>
    requestJson(`/comic/series/${encodeURIComponent(seriesId)}/online/refresh`, {
      method: 'POST',
    }),

  cacheTrackedSeries: (seriesId: string, chapterIds?: string[]): Promise<ComicCacheReportDto> =>
    requestJson(`/comic/series/${encodeURIComponent(seriesId)}/online/cache`, {
      method: 'POST',
      body: JSON.stringify(chapterIds ? { chapter_ids: chapterIds } : {}),
    }),

  cacheTrackedChapter: (chapterId: string): Promise<ChapterItemDto> =>
    requestJson(`/comic/chapters/${encodeURIComponent(chapterId)}/online/cache`, {
      method: 'POST',
    }),
}

export const comicSourceAdapter: OnlineSourceManagerAdapter<ComicSourceSummaryDto> = {
  domain: 'comic',
  exportFilePrefix: 'xmedia-comic-sources',
  searchRoutePath: '/comic/library',
  listSources: onlineComicApi.listSources,
  importSources: onlineComicApi.importSources,
  importSeeds: onlineComicApi.importSeeds,
  reorderSources: onlineComicApi.reorderSources,
  batchSources: onlineComicApi.batchSources,
  exportSourcesBatch: onlineComicApi.exportSourcesBatch,
  updateSource: onlineComicApi.updateSource,
  deleteSource: onlineComicApi.deleteSource,
  exportSourceUrl: onlineComicApi.exportSourceUrl,
  fetchSourceJson: onlineComicApi.fetchSourceJson,
  getSourceVariable: onlineComicApi.getSourceVariable,
  runLoginAction: onlineComicApi.runLoginAction,
}

import { requestJson, withAuthToken } from '@/api/client'
import { createDomainSourceApi } from '@/components/online-sources/createDomainSourceApi'
import {
  isSafeDataImageUrl,
  isSafeHttpUrl,
  type BaseSourceTestReportDto,
  type BatchSourcesPayload,
  type BatchSourcesReportDto,
  type BrowserRedirectHint,
  type ExecutionDiagnostics,
  type ImportSourcesReportDto,
  type LoginActionResponseDto as SharedLoginActionResponseDto,
  type LoginUiItem,
  type LoginUiStyle,
  type OnlineSourceManagerAdapter,
  type OnlineSourceSummaryDto,
  type SourceDebugStepDto,
} from '@/components/online-sources/types'
import type { BookSummaryDto } from './types'

export { isSafeHttpUrl }
export type {
  BrowserRedirectHint,
  ExecutionDiagnostics,
  LoginUiItem,
  LoginUiStyle,
  SourceDebugStepDto,
}

export interface TrackedChapterMetaDto {
  index: number
  title: string
  url: string
  cached: boolean
}

export interface TrackedTocResponseDto {
  book_id: string
  source_id: string
  source_name: string
  chapters: TrackedChapterMetaDto[]
  cached_chapters: number
  total_chapters: number
  last_chapter: string | null
  epub_dirty: boolean
}

export interface TrackedChapterResponseDto {
  book_id: string
  source_id: string
  chapter_index: number
  title: string
  content: string
  cached: boolean
  cached_chapters: number
  total_chapters: number
  epub_dirty: boolean
}

export type BookSourceSummaryDto = OnlineSourceSummaryDto
export type ImportBookSourcesReportDto = ImportSourcesReportDto<BookSourceSummaryDto>
export type BatchBookSourcesPayload = BatchSourcesPayload
export type BatchBookSourcesReportDto = BatchSourcesReportDto
export type LoginActionResponseDto = SharedLoginActionResponseDto<BookSourceSummaryDto>

export type SourceTestMode = 'full' | 'search' | 'book_info' | 'toc' | 'content'

export interface SourceTestInput {
  keyword?: string | null
  mode?: SourceTestMode
  book_url?: string | null
  toc_url?: string | null
  chapter_url?: string | null
  chapter_index?: number | null
  chapter_title?: string | null
}

export interface SourceTestReportDto extends BaseSourceTestReportDto<SourceTestMode> {
  search_items: SearchBookResult[]
  book_detail: OnlineBookDetail | null
  toc_chapters: OnlineChapterItem[]
  toc_total_count: number
  content_chapter_index: number | null
  content_chapter_title: string | null
  content_chapter_url: string | null
  content_text: string | null
  content_char_count: number
}

export interface SearchBookResult {
  name: string
  author: string
  book_url: string
  cover_url: string | null
  intro: string | null
  kind: string | null
  last_chapter: string | null
  word_count: string | null
}

export interface AggregatedSearchItemDto {
  source_id: string
  source_name: string
  book: SearchBookResult
}

export interface SourceSearchErrorDto {
  source_id: string
  source_name: string
  error: string
}

export interface AggregatedSearchResponseDto {
  keyword: string
  page: number
  items: AggregatedSearchItemDto[]
  errors: SourceSearchErrorDto[]
  toasts: string[]
}

export interface OnlineBookDetail {
  name: string
  author: string
  book_url: string
  toc_url: string
  cover_url: string | null
  intro: string | null
  kind: string | null
  last_chapter: string | null
  word_count: string | null
}

export interface OnlineBookDetailResponseDto {
  source_id: string
  source_name: string
  detail: OnlineBookDetail
  diagnostics: ExecutionDiagnostics
}

export interface OnlineChapterItem {
  index: number
  title: string
  url: string
  update_time: string | null
  is_volume: boolean
}

export interface OnlineTocResponseDto {
  source_id: string
  chapters: OnlineChapterItem[]
  diagnostics: ExecutionDiagnostics
}

export interface OnlineChapterContent {
  title: string
  content: string
  raw_html: string
}

export interface OnlineContentResponseDto {
  source_id: string
  chapter_index: number
  content: OnlineChapterContent
  cached: boolean
  diagnostics: ExecutionDiagnostics
}

export interface OnlineShelfBookDto {
  id: string
  source_id: string
  source_name: string
  name: string
  author: string
  book_url: string
  toc_url: string
  cover_url: string | null
  intro: string | null
  kind: string | null
  last_chapter: string | null
  total_chapters: number
  current_chapter_index: number
  current_chapter_title: string | null
  scroll_ratio: number
  updated_at: string
}

export type OnlineDownloadFormat = 'epub' | 'txt'
export type DownloadJobStatus = 'pending' | 'running' | 'completed' | 'failed' | 'cancelled'

export interface OnlineDownloadJobDto {
  id: string
  user_id: string
  source_id: string
  source_name: string
  book_name: string
  author: string
  format: OnlineDownloadFormat
  status: DownloadJobStatus
  total_chapters: number
  processed_chapters: number
  failed_chapters: number
  chapters_per_sec?: number
  elapsed_ms?: number
  current_chapter_title: string | null
  output_file_name: string | null
  file_size: number | null
  import_to_library: boolean
  imported_book_id: string | null
  error: string | null
  created_at: string
  updated_at: string
}

export interface BookJsContextInput {
  name: string
  author: string
  book_url: string
  toc_url: string
  cover_url?: string | null
  intro?: string | null
  dur_chapter_index?: number
  dur_chapter_title?: string
  custom_variable?: string
}

export interface ChapterJsContextInput {
  index: number
  title: string
  url: string
}

export function buildOnlineCoverUrl(rawUrl: string | null | undefined): string {
  if (!rawUrl || !rawUrl.trim()) return ''
  const u = rawUrl.trim()
  if (u.toLowerCase().startsWith('data:')) {
    return isSafeDataImageUrl(u) ? u : ''
  }
  if (!isSafeHttpUrl(u)) return ''
  return withAuthToken(`/api/v1/book/online/cover-proxy?url=${encodeURIComponent(u)}`)
}

export function buildOnlineDownloadFileUrl(jobId: string): string {
  return withAuthToken(`/api/v1/book/online/downloads/${encodeURIComponent(jobId)}/file`)
}

const bookSourceCrud = createDomainSourceApi<BookSourceSummaryDto>('book')

export const bookSourceAdapter: OnlineSourceManagerAdapter<BookSourceSummaryDto> = {
  domain: 'book',
  exportFilePrefix: 'xmedia-book-sources',
  searchRoutePath: '/book/library',
  ...bookSourceCrud,
}

export const onlineBookApi = {
  ...bookSourceCrud,

  testSource: (
    id: string,
    payload?: string | SourceTestInput,
    options?: { signal?: AbortSignal },
  ): Promise<SourceTestReportDto> => {
    const body: SourceTestInput =
      typeof payload === 'string'
        ? { keyword: payload.trim() || null }
        : {
            keyword: payload?.keyword?.trim() || null,
            mode: payload?.mode ?? 'full',
            book_url: payload?.book_url?.trim() || null,
            toc_url: payload?.toc_url?.trim() || null,
            chapter_url: payload?.chapter_url?.trim() || null,
            chapter_index: payload?.chapter_index ?? null,
            chapter_title: payload?.chapter_title?.trim() || null,
          }
    return requestJson(`/book/sources/${encodeURIComponent(id)}/test`, {
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
  ): Promise<AggregatedSearchResponseDto> =>
    requestJson('/book/online/search', {
      method: 'POST',
      body: JSON.stringify({
        keyword,
        source_ids: sourceIds && sourceIds.length > 0 ? sourceIds : null,
        page,
      }),
      signal: options?.signal,
    }),

  fetchBookInfo: (
    sourceId: string,
    bookUrl: string,
    fallbackHint?: BookJsContextInput,
  ): Promise<OnlineBookDetailResponseDto> =>
    requestJson('/book/online/book-info', {
      method: 'POST',
      body: JSON.stringify({
        source_id: sourceId,
        book_url: bookUrl,
        fallback_hint: fallbackHint
          ? {
              ...fallbackHint,
              dur_chapter_index: fallbackHint.dur_chapter_index ?? 0,
              dur_chapter_title: fallbackHint.dur_chapter_title ?? '',
              custom_variable: fallbackHint.custom_variable ?? '',
            }
          : null,
      }),
    }),

  fetchToc: (
    sourceId: string,
    tocUrl: string,
    book: BookJsContextInput,
  ): Promise<OnlineTocResponseDto> =>
    requestJson('/book/online/toc', {
      method: 'POST',
      body: JSON.stringify({
        source_id: sourceId,
        toc_url: tocUrl,
        book: {
          ...book,
          dur_chapter_index: book.dur_chapter_index ?? 0,
          dur_chapter_title: book.dur_chapter_title ?? '',
          custom_variable: book.custom_variable ?? '',
        },
      }),
    }),

  fetchContent: (
    sourceId: string,
    chapterUrl: string,
    book: BookJsContextInput,
    chapter: ChapterJsContextInput,
    forceRefresh = false,
  ): Promise<OnlineContentResponseDto> =>
    requestJson('/book/online/content', {
      method: 'POST',
      body: JSON.stringify({
        source_id: sourceId,
        chapter_url: chapterUrl,
        book: {
          ...book,
          dur_chapter_index: book.dur_chapter_index ?? chapter.index,
          dur_chapter_title: book.dur_chapter_title ?? chapter.title,
          custom_variable: book.custom_variable ?? '',
        },
        chapter,
        force_refresh: forceRefresh,
      }),
    }),

  listShelf: (): Promise<OnlineShelfBookDto[]> =>
    requestJson('/book/online/shelf'),

  addToShelf: (payload: {
    source_id: string
    name: string
    author: string
    book_url: string
    toc_url: string
    cover_url?: string | null
    intro?: string | null
    kind?: string | null
    last_chapter?: string | null
    total_chapters?: number
  }): Promise<OnlineShelfBookDto> =>
    requestJson('/book/online/shelf', {
      method: 'POST',
      body: JSON.stringify(payload),
    }),

  updateShelfBook: (
    id: string,
    patch: {
      current_chapter_index?: number
      current_chapter_title?: string | null
      scroll_ratio?: number
      total_chapters?: number
      last_chapter?: string | null
      toc_url?: string
      source_id?: string
      book_url?: string
    },
  ): Promise<OnlineShelfBookDto> =>
    requestJson(`/book/online/shelf/${encodeURIComponent(id)}`, {
      method: 'PATCH',
      body: JSON.stringify(patch),
    }),

  removeFromShelf: (id: string): Promise<void> =>
    requestJson(`/book/online/shelf/${encodeURIComponent(id)}`, {
      method: 'DELETE',
    }),

  listDownloads: (): Promise<OnlineDownloadJobDto[]> =>
    requestJson('/book/online/downloads'),

  trackOnlineBook: (payload: {
    source_id: string
    name: string
    author: string
    book_url: string
    toc_url?: string
    cover_url?: string | null
    intro?: string | null
    kind?: string | null
    last_chapter?: string | null
  }): Promise<BookSummaryDto> =>
    requestJson('/book/online/track', {
      method: 'POST',
      body: JSON.stringify(payload),
    }),

  getTrackedToc: (bookId: string, refresh = false): Promise<TrackedTocResponseDto> =>
    requestJson(
      `/book/books/${encodeURIComponent(bookId)}/online/toc${refresh ? '?refresh=true' : ''}`,
    ),

  getTrackedChapter: (
    bookId: string,
    index: number,
    refresh = false,
  ): Promise<TrackedChapterResponseDto> =>
    requestJson(
      `/book/books/${encodeURIComponent(bookId)}/online/chapter/${index}${refresh ? '?refresh=true' : ''}`,
    ),

  switchTrackedSource: (
    bookId: string,
    payload: {
      source_id: string
      book_url: string
      toc_url: string
      cover_url?: string | null
      intro?: string | null
      last_chapter?: string | null
      clear_cache?: boolean
    },
  ): Promise<TrackedTocResponseDto> =>
    requestJson(`/book/books/${encodeURIComponent(bookId)}/online/switch-source`, {
      method: 'POST',
      body: JSON.stringify(payload),
    }),

  syncTrackedEpub: (bookId: string): Promise<BookSummaryDto> =>
    requestJson(`/book/books/${encodeURIComponent(bookId)}/online/sync-epub`, {
      method: 'POST',
    }),

  startDownload: (payload: {
    book_id?: string
    source_id: string
    book_name: string
    author: string
    book_url: string
    toc_url: string
    cover_url?: string | null
    intro?: string | null
    format: OnlineDownloadFormat
    start_chapter?: number
    end_chapter?: number
    import_to_library: boolean
  }): Promise<OnlineDownloadJobDto> =>
    requestJson('/book/online/downloads', {
      method: 'POST',
      body: JSON.stringify(payload),
    }),

  getDownload: (id: string): Promise<OnlineDownloadJobDto> =>
    requestJson(`/book/online/downloads/${encodeURIComponent(id)}`),

  cancelDownload: (id: string): Promise<OnlineDownloadJobDto> =>
    requestJson(`/book/online/downloads/${encodeURIComponent(id)}/cancel`, {
      method: 'POST',
    }),

  deleteDownload: (id: string): Promise<void> =>
    requestJson(`/book/online/downloads/${encodeURIComponent(id)}`, {
      method: 'DELETE',
    }),
}

import { api as coreApi, requestJson, withAuthToken } from '@/api/client'
import type { CoreLibraryDto } from '@/types/api'
import type {
  BookBookmarkItemDto,
  BookFormat,
  BookHomeOverviewDto,
  Bookmark,
  BookSortField,
  BookSummaryDto,
  FacetCount,
  LibraryDto,
  PagedResult,
  ReadingProgress,
  ShelfDto,
  SortOrder,
  TagMatchMode,
  TocItem,
  TxtChapterContent,
  UpdateMetadataPatch,
} from './types'

export function mapBookLibrary(raw: CoreLibraryDto): LibraryDto {
  return {
    id: raw.id,
    name: raw.name,
    kind:
      raw.kind.type === 'managed'
        ? { type: 'managed' }
        : { type: 'mounted', root_path: raw.kind.root_path },
    scan_status:
      raw.scan_status.state === 'scanning'
        ? {
            state: 'scanning',
            processed: raw.scan_status.scanned,
            total: raw.scan_status.total ?? undefined,
          }
        : raw.scan_status,
    last_scanned_at: raw.last_scanned_at,
    book_count: raw.item_count,
    created_at: raw.created_at,
    updated_at: raw.updated_at,
  }
}

export const api = {
  listLibraries: async (): Promise<LibraryDto[]> => {
    const libs = await coreApi.listLibraries('book')
    return libs.map(mapBookLibrary)
  },
  createLibrary: async (payload: { name: string; root_path: string; scan_now?: boolean }): Promise<LibraryDto> => {
    const created = await coreApi.createLibrary({
      name: payload.name,
      media_kind: 'book',
      root_path: payload.root_path,
    })
    if (payload.scan_now) {
      const scanned = await coreApi.scanLibrary(created.id)
      return mapBookLibrary(scanned)
    }
    return mapBookLibrary(created)
  },
  renameLibrary: async (id: string, name: string): Promise<LibraryDto> =>
    mapBookLibrary(await coreApi.renameLibrary(id, name)),
  deleteLibrary: async (id: string): Promise<void> => {
    await coreApi.deleteLibrary(id)
  },
  scanLibrary: async (id: string): Promise<LibraryDto> =>
    mapBookLibrary(await coreApi.scanLibrary(id)),

  searchBooks: (params: {
    keyword?: string
    author?: string
    authors?: string[]
    tag?: string
    tags?: string[]
    tag_mode?: TagMatchMode
    format?: BookFormat
    formats?: BookFormat[]
    library_id?: string
    library_ids?: string[]
    favorites_only?: boolean
    shelf_id?: string
    include_missing?: boolean
    sort_by?: BookSortField
    sort_order?: SortOrder
    page?: number
    page_size?: number
  }) => {
    const sp = new URLSearchParams()
    for (const [k, v] of Object.entries(params)) {
      if (Array.isArray(v)) {
        if (v.length > 0) sp.set(k, JSON.stringify(v))
      } else if (v !== undefined && v !== null && v !== '') {
        sp.set(k, String(v))
      }
    }
    const qs = sp.toString()
    return requestJson<PagedResult<BookSummaryDto>>(`/book/books${qs ? `?${qs}` : ''}`)
  },
  batchBooks: (payload: {
    book_ids: string[]
    add_tags?: string[]
    remove_tags?: string[]
    set_favorite?: boolean
  }) =>
    requestJson<{ affected: number }>('/book/books/batch', {
      method: 'POST',
      body: JSON.stringify(payload),
    }),
  getBook: (id: string) => requestJson<BookSummaryDto>(`/book/books/${encodeURIComponent(id)}`),
  updateBook: (id: string, patch: UpdateMetadataPatch) =>
    requestJson<BookSummaryDto>(`/book/books/${encodeURIComponent(id)}`, {
      method: 'PATCH',
      body: JSON.stringify(patch),
    }),
  replaceCover: (id: string, file: File) => {
    const fd = new FormData()
    fd.append('cover', file)
    return requestJson<BookSummaryDto>(`/book/books/${encodeURIComponent(id)}/cover`, {
      method: 'PUT',
      body: fd,
    })
  },
  deleteBook: (id: string) =>
    requestJson<void>(`/book/books/${encodeURIComponent(id)}`, { method: 'DELETE' }),
  getToc: (id: string) => requestJson<TocItem[]>(`/book/books/${encodeURIComponent(id)}/toc`),
  getTxtChapter: (id: string, index: number) =>
    requestJson<TxtChapterContent>(`/book/books/${encodeURIComponent(id)}/chapters/${index}`),
  listAuthors: () => requestJson<FacetCount[]>('/book/authors'),
  listTags: () => requestJson<FacetCount[]>('/book/tags'),
  homeOverview: () => requestJson<BookHomeOverviewDto>('/book/home'),
  listAllBookmarks: () => requestJson<BookBookmarkItemDto[]>('/book/bookmarks'),

  listRecent: (limit = 24) => requestJson<BookSummaryDto[]>(`/book/recent?limit=${limit}`),
  getProgress: (bookId: string) =>
    requestJson<ReadingProgress | null>(`/book/books/${encodeURIComponent(bookId)}/progress`),
  saveProgress: (
    bookId: string,
    payload: {
      progress_percent: number
      locator: string
      chapter_index?: number | null
      page_number?: number | null
      scroll_ratio?: number | null
    },
  ) =>
    requestJson<ReadingProgress>(`/book/books/${encodeURIComponent(bookId)}/progress`, {
      method: 'PUT',
      body: JSON.stringify(payload),
    }),
  setFavorite: (bookId: string, favorite: boolean) =>
    requestJson<void>(`/book/books/${encodeURIComponent(bookId)}/favorite`, {
      method: 'PUT',
      body: JSON.stringify({ favorite }),
    }),
  listBookmarks: (bookId: string) =>
    requestJson<Bookmark[]>(`/book/books/${encodeURIComponent(bookId)}/bookmarks`),
  createBookmark: (
    bookId: string,
    payload: {
      title: string
      note?: string
      progress_percent: number
      locator: string
      chapter_index?: number | null
      page_number?: number | null
      scroll_ratio?: number | null
    },
  ) =>
    requestJson<Bookmark>(`/book/books/${encodeURIComponent(bookId)}/bookmarks`, {
      method: 'POST',
      body: JSON.stringify(payload),
    }),
  deleteBookmark: (bookmarkId: string) =>
    requestJson<void>(`/book/bookmarks/${encodeURIComponent(bookmarkId)}`, { method: 'DELETE' }),

  listShelves: () => requestJson<ShelfDto[]>('/book/shelves'),
  createShelf: (name: string) =>
    requestJson<ShelfDto>('/book/shelves', {
      method: 'POST',
      body: JSON.stringify({ name }),
    }),
  renameShelf: (shelfId: string, name: string) =>
    requestJson<ShelfDto>(`/book/shelves/${encodeURIComponent(shelfId)}`, {
      method: 'PATCH',
      body: JSON.stringify({ name }),
    }),
  deleteShelf: (shelfId: string) =>
    requestJson<void>(`/book/shelves/${encodeURIComponent(shelfId)}`, { method: 'DELETE' }),
  addBookToShelf: (shelfId: string, bookId: string) =>
    requestJson<ShelfDto>(
      `/book/shelves/${encodeURIComponent(shelfId)}/books/${encodeURIComponent(bookId)}`,
      { method: 'POST' },
    ),
  removeBookFromShelf: (shelfId: string, bookId: string) =>
    requestJson<ShelfDto>(
      `/book/shelves/${encodeURIComponent(shelfId)}/books/${encodeURIComponent(bookId)}`,
      { method: 'DELETE' },
    ),

  coverUrl: (bookId: string, updatedAt?: string) => {
    const params = new URLSearchParams()
    if (updatedAt) params.set('v', updatedAt)
    const qs = params.toString()
    return withAuthToken(`/api/v1/book/books/${encodeURIComponent(bookId)}/cover${qs ? `?${qs}` : ''}`)
  },
  fileUrl: (bookId: string, download = false) => {
    const params = new URLSearchParams()
    if (download) params.set('download', 'true')
    const qs = params.toString()
    return withAuthToken(`/api/v1/book/books/${encodeURIComponent(bookId)}/file${qs ? `?${qs}` : ''}`)
  },
}

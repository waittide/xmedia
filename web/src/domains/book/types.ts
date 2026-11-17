import type {
  AccentMode,
  CatppuccinAccent,
  CatppuccinFlavor,
  CustomPaletteTokens,
  ThemeMode,
} from '@/theme/catppuccin'

export type UserRole = 'admin' | 'reader'
export type LocaleCode = 'zh-CN' | 'en-US'
export type BookFormat = 'epub' | 'txt' | 'pdf'
export type ReadingFlow = 'paginated' | 'scrolled'
export type BookSortField = 'created_at' | 'updated_at' | 'title' | 'file_size'
export type SortOrder = 'asc' | 'desc'
export type TagMatchMode = 'any' | 'all'

export interface ReaderPreferences {
  font_family: string
  font_size: number
  line_height: number
  margin_px: number
  flow: ReadingFlow
}

export interface UserPreferences {
  theme_mode: ThemeMode
  light_preset: string
  dark_preset: string
  dark_flavor: CatppuccinFlavor
  accent_mode: AccentMode
  accent: CatppuccinAccent
  custom_accent: string | null
  custom_light: CustomPaletteTokens | null
  custom_dark: CustomPaletteTokens | null
  locale: LocaleCode
  reader: ReaderPreferences
}

export interface UserDto {
  id: string
  username: string
  display_name: string
  role: UserRole
  preferences: UserPreferences
  created_at: string
  updated_at: string
}

export interface AuthSessionDto {
  token: string
  user: UserDto
}

export type LibraryKind =
  | { type: 'mounted'; root_path: string }
  | { type: 'managed' }

export type ScanStatus =
  | { state: 'idle' }
  | { state: 'scanning'; processed?: number; total?: number }
  | { state: 'completed'; added: number; updated: number; missing: number }
  | { state: 'failed'; message: string }

export type SystemWsEvent = {
  event: 'library_scan_updated'
  data: LibraryDto
}

export interface LibraryDto {
  id: string
  name: string
  kind: LibraryKind
  scan_status: ScanStatus
  last_scanned_at: string | null
  book_count: number
  created_at: string
  updated_at: string
}

export interface BookMetadata {
  title: string
  authors: string[]
  description: string | null
  isbn: string | null
  language: string | null
  publisher: string | null
  published_at: string | null
  tags: string[]
  page_count: number | null
  chapter_count: number | null
}

export interface ReadingLocation {
  progress_percent: number
  locator: string
  chapter_index: number | null
  page_number: number | null
  scroll_ratio: number | null
}

export interface BookSummaryDto {
  id: string
  library_id: string
  library_name: string | null
  format: BookFormat
  is_managed: boolean
  file_size: number
  has_cover: boolean
  metadata: BookMetadata
  is_missing: boolean
  is_favorite: boolean
  progress: ReadingLocation | null
  last_read_at: string | null
  created_at: string
  updated_at: string
}

export interface PagedResult<T> {
  items: T[]
  total: number
  page: number
  page_size: number
}

export interface FacetCount {
  name: string
  count: number
}

export interface TocItem {
  index: number
  title: string
  href: string
  level: number
}

export interface TxtChapterContent {
  index: number
  volume_title?: string | null
  title: string
  content: string
  total_chapters: number
}

export interface ReadingProgress {
  user_id: string
  book_id: string
  location: ReadingLocation
  last_read_at: string
}

export interface Bookmark {
  id: string
  user_id: string
  book_id: string
  title: string
  note: string | null
  location: ReadingLocation
  created_at: string
}

export interface ShelfDto {
  id: string
  name: string
  is_favorite: boolean
  book_ids: string[]
  book_count: number
  created_at: string
  updated_at: string
}

export interface UpdateMetadataPatch {
  title?: string
  authors?: string[]
  description?: string
  isbn?: string
  language?: string
  publisher?: string
  published_at?: string
  tags?: string[]
}

export interface BookBookmarkItemDto {
  bookmark: Bookmark
  book_title: string
  book_authors: string[]
  book_format: BookFormat
  has_cover: boolean
  book_updated_at: string
}

export interface BookLibraryStatsDto {
  total_books: number
  reading_count: number
  bookmarks_count: number
  favorites_count: number
  shelves_count: number
  tags_count: number
  authors_count: number
  libraries_count: number
}

export interface BookHomeOverviewDto {
  stats: BookLibraryStatsDto
  continue_reading: BookSummaryDto[]
  recently_added: BookSummaryDto[]
}

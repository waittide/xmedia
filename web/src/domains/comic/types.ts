import type { UserRole } from '@/types/api'

export type { UserRole }
export type ReaderMode = 'rtl_double' | 'ltr_single' | 'ltr_double' | 'webtoon'
export type ReaderFitMode = 'fit_height' | 'fit_width' | 'original'
export type SeriesStatus = 'ongoing' | 'completed' | 'hiatus'
export type ReadingDirection = 'rtl' | 'ltr' | 'webtoon'
export type ChapterKind = 'volume' | 'chapter' | 'special'
export type LibraryKind = 'local_mount' | 'managed_upload'

export type ScanState =
  | { state: 'idle' }
  | { state: 'scanning'; started_at: number }
  | { state: 'completed'; finished_at: number; series_count: number; chapter_count: number }
  | { state: 'failed'; finished_at: number; error: string }

export interface Library {
  id: string
  name: string
  root_path: string
  kind: LibraryKind
  scan_state: ScanState
  series_count?: number | string
  created_at: number | string
  updated_at: number | string
}

export interface ScanResultDto {
  library: Library
  series_count: number
  chapter_count: number
  missing_count: number
}

export interface ContinueReadingPointer {
  series_id: string
  chapter_id: string
  chapter_number: string
  chapter_title: string
  current_page: number
  total_pages: number
  is_resuming_mid_chapter: boolean
  last_read_at: number | string
}

export interface OnlineTrackedSeriesDto {
  source_id: string
  comic_url: string
  toc_url: string
}

export interface OnlineTrackedChapterDto {
  source_id: string
  chapter_url: string
  cached: boolean
}

export interface SeriesSummaryDto {
  id: string
  library_id: string
  title: string
  writers: string[]
  pencillers: string[]
  status: SeriesStatus
  reading_direction: ReadingDirection
  age_rating: string | null
  genres: string[]
  tags: string[]
  cover_url: string | null
  chapter_count: number
  completed_chapters: number
  is_favorite: boolean
  continue_reading: ContinueReadingPointer | null
  online_tracking?: OnlineTrackedSeriesDto | null
  created_at: number | string
  updated_at: number | string
}

export interface ContinueReadingItemDto {
  series: SeriesSummaryDto
  pointer: ContinueReadingPointer
}

export interface LibraryStatsDto {
  series_count: number
  chapter_count: number
  completed_chapters: number
  favorites_count: number
  in_progress_count: number
  bookmarks_count: number
  shelves_count: number
  tags_count: number
  writers_count: number
}

export interface ComicBookmark {
  id: string
  user_id: string
  series_id: string
  chapter_id: string
  page_number: number
  title: string
  note: string | null
  created_at: number | string
}

export interface ComicBookmarkItemDto {
  bookmark: ComicBookmark
  series_title: string
  chapter_number: string
  chapter_title: string
  total_pages: number
  cover_url: string | null
}

export interface HomeOverviewDto {
  continue_reading: ContinueReadingItemDto[]
  recently_updated: SeriesSummaryDto[]
  recently_added: SeriesSummaryDto[]
  stats: LibraryStatsDto
}

export interface Series {
  id: string
  library_id: string
  title: string
  sort_title: string
  alt_titles: string[]
  summary: string
  writers: string[]
  pencillers: string[]
  publisher: string | null
  status: SeriesStatus
  reading_direction: ReadingDirection
  age_rating: string | null
  genres: string[]
  tags: string[]
  language: string | null
  year: number | null
  cover_path: string | null
  chapter_count: number
  source_dir: string
  metadata_locked: boolean
  created_at: number | string
  updated_at: number | string
}

export interface SeriesMetadataPatch {
  title?: string
  sort_title?: string
  alt_titles?: string[]
  summary?: string
  writers?: string[]
  pencillers?: string[]
  publisher?: string | null
  status?: SeriesStatus
  reading_direction?: ReadingDirection
  age_rating?: string | null
  genres?: string[]
  tags?: string[]
  language?: string | null
  year?: number | null
  metadata_locked?: boolean
}

export interface ChapterItemDto {
  id: string
  series_id: string
  kind: ChapterKind
  number: number
  number_label: string
  volume_number: number | null
  title: string
  page_count: number
  source_kind: 'archive_file' | 'image_directory' | 'online_tracked'
  file_size: number
  cover_url: string | null
  summary: string
  writers: string[]
  pencillers: string[]
  is_missing: boolean
  current_page: number
  is_completed: boolean
  online_tracking?: OnlineTrackedChapterDto | null
  created_at: number | string
  updated_at: number | string
}

export interface ChapterMetadataPatch {
  kind?: ChapterKind
  number?: number | string
  volume_number?: number | null
  title?: string
  summary?: string
  writers?: string[]
  pencillers?: string[]
}

export interface SeriesReaderPref {
  user_id: string
  series_id: string
  reader_mode: ReaderMode
  fit_mode: ReaderFitMode
  offset_cover_page: boolean
  webtoon_width: number
  updated_at: number | string
}

export interface SeriesDetailDto {
  series: Series
  cover_url: string | null
  is_favorite: boolean
  shelf_ids: string[]
  completed_chapters: number
  continue_reading: ContinueReadingPointer | null
  reader_pref: SeriesReaderPref | null
  online_tracking?: OnlineTrackedSeriesDto | null
  chapters: ChapterItemDto[]
}

export interface EffectiveReaderPrefDto {
  reader_mode: ReaderMode
  fit_mode: ReaderFitMode
  offset_cover_page: boolean
  dim_images_in_dark: boolean
  preload_pages: number
  webtoon_width: number
}

export interface AdjacentChapterDto {
  id: string
  kind: ChapterKind
  number: number
  number_label: string
  volume_number: number | null
  title: string
  page_count: number
  is_completed: boolean
}

export interface ReaderContextDto {
  series_id: string
  series_title: string
  reading_direction: ReadingDirection
  chapter: ChapterItemDto
  effective_pref: EffectiveReaderPrefDto
  prev_chapter: AdjacentChapterDto | null
  next_chapter: AdjacentChapterDto | null
  all_chapters: AdjacentChapterDto[]
}

export interface TaxonomyCountDto {
  name: string
  count: number
}

export interface TaxonomyOverviewDto {
  tags: TaxonomyCountDto[]
  genres: TaxonomyCountDto[]
  writers: TaxonomyCountDto[]
  pencillers: TaxonomyCountDto[]
}

export interface Shelf {
  id: string
  user_id: string
  name: string
  description: string
  series_ids: string[]
  created_at: number | string
  updated_at: number | string
}

export interface ShelfDetailDto {
  shelf: Shelf
  series: SeriesSummaryDto[]
}

export interface UploadArchiveResultDto {
  series_id: string
  series_title: string
  chapter_id: string
  chapter_title: string
  page_count: number
}

export interface SeriesQueryFilter {
  search?: string
  library_id?: string
  status?: SeriesStatus | ''
  writer?: string
  penciller?: string
  tag?: string
  genre?: string
  read_status?: 'unread' | 'in_progress' | 'completed' | ''
  favorites_only?: boolean
  shelf_id?: string
  sort_by?: 'updated_at' | 'created_at' | 'title' | 'chapter_count'
  sort_dir?: 'asc' | 'desc'
}

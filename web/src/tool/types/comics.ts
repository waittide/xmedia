export interface ComicPageInfoDto {
  index: number
  entry_name: string
  file_size: number
  width: number | null
  height: number | null
  page_type: 'Story' | 'FrontCover' | 'InnerCover' | 'BackCover' | 'Advertisement' | string
  double_page: boolean
}

export interface ComicPagesSnapshotDto {
  format: string
  total_pages: number
  total_size_bytes: number
  pages: ComicPageInfoDto[]
}

export interface SaveComicPageInput {
  entry_name: string
  page_type: string
  double_page: boolean
}

export type ComicImageReencodeFormat = 'keep' | 'jpeg' | 'webp'

export interface ComicOptimizeReportDto {
  before_bytes: number
  after_bytes: number
  page_count: number
  reencoded_pages: number
  removed_junk_entries: number
}

export interface MergeComicsReportDto {
  output_rel_path: string
  source_count: number
  total_pages: number
  output_bytes: number
}

export interface BatchComicSeriesSyncInput {
  series?: string
  series_group?: string
  writers?: string[]
  pencillers?: string[]
  publisher?: string
  genres?: string[]
  tags?: string[]
  reading_direction?: string
  numbering_mode?: 'none' | 'chapter' | 'volume'
  start_index?: number
  pad_width?: number
}

export interface BatchComicSeriesSyncReportDto {
  updated_count: number
  failed_paths: string[]
}

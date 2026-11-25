import type { ZhConvertDirection } from './common'
import type { MediaFileDetail } from './inspector'

export type ChapterEditMode = 'plain' | 'xhtml'

export interface EditableBookChapterDto {
  id: string
  title: string
  level: number
  plain_text: string
  xhtml_body: string
  has_rich_media: boolean
  source_href: string | null
}

export interface BookContentDocumentDto {
  format: 'txt' | 'epub'
  title: string
  authors: string[]
  stylesheets: string[]
  chapters: EditableBookChapterDto[]
}

export interface SaveBookChapterInput {
  title: string
  level: number
  mode: ChapterEditMode
  plain_text: string
  xhtml_body: string
  source_href?: string | null
}

export interface ConvertTxtToEpubResponseDto {
  detail: MediaFileDetail
  deleted_source_txt: boolean
}

export interface BookTextTransformOptions {
  zh_direction?: ZhConvertDirection | null
  convert_quotes_to_horizontal?: boolean
  vertical_to_horizontal?: boolean
  clean_webnovel_ads?: boolean
  normalize_punctuation?: boolean
  custom_strip_patterns?: string[]
}

export interface TransformBookContentResultDto {
  chapters: EditableBookChapterDto[]
  stylesheets: string[]
  changed_chapters: number
}

export interface InsertedEpubImageDto {
  asset_path: string
  chapter_rel_href: string
  mime_type: string
}

export interface PolishBookResponseDto {
  jacket_updated: boolean
  removed_orphan_files: number
  detail: MediaFileDetail
}

export type BookImageCompressFormat = 'webp' | 'jpeg' | 'keep'

export interface NormalizeBookToEpubOptionsDto {
  keep_source: boolean
  compress_images: boolean
  image_format: BookImageCompressFormat
  max_image_dimension: number | null
  image_quality: number
  generate_jacket: boolean
  remove_orphans: boolean
  pdf_scanned_fallback: boolean
  skip_normalized: boolean
}

export interface NormalizeBookItemReportDto {
  source_rel_path: string
  output_rel_path: string | null
  backup_rel_path: string | null
  source_format: string
  before_bytes: number
  after_bytes: number
  chapter_count: number
  compressed_images: number
  removed_orphans: number
  deleted_source: boolean
  skipped: boolean
  skip_reason: string | null
  error: string | null
}

export interface NormalizeBooksBatchReportDto {
  total: number
  succeeded: number
  skipped: number
  failed: number
  total_before_bytes: number
  total_after_bytes: number
  items: NormalizeBookItemReportDto[]
}

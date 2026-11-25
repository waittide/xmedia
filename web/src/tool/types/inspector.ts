import type { MediaKind } from '@/types/api'
import type { ZhConvertDirection } from './common'
import type { MediaFileEntry } from './workspace'

export interface BookFileMeta {
  format: string
  title: string
  authors: string[]
  series: string | null
  subseries: string | null
  series_index: string | null
  description: string | null
  publisher: string | null
  published_at: string | null
  language: string | null
  isbn: string | null
  tags: string[]
  locked_fields: string[]
  is_normalized?: boolean
  has_cover: boolean
  writable: boolean
}

export interface ComicFileMeta {
  format: string
  series: string | null
  series_group: string | null
  title: string | null
  number: string | null
  volume: number | null
  summary: string | null
  writers: string[]
  pencillers: string[]
  publisher: string | null
  genres: string[]
  tags: string[]
  year: number | null
  language_iso: string | null
  reading_direction: string
  page_count: number
  has_comic_info_xml: boolean
  has_cover: boolean
  has_series_cover: boolean
}

export interface MusicFileMeta {
  format: string
  title: string
  artist: string
  album_artist: string | null
  album: string
  track_number: number | null
  disc_number: number | null
  year: number | null
  genre: string | null
  comment: string | null
  duration_secs: number
  bit_rate_kbps: number | null
  sample_rate_hz: number | null
  bit_depth: number | null
  channels: number | null
  has_embedded_cover: boolean
  has_directory_cover: boolean
  embedded_lyrics: string | null
  sidecar_lrc: string | null
  sidecar_lrc_filename: string
}

export interface ProbedAudioStreamDto {
  stream_index: number
  codec: string
  channels: number
  language: string | null
  title: string | null
  is_default: boolean
}

export interface ProbedSubtitleStreamDto {
  stream_index: number
  codec: string
  language: string | null
  title: string | null
  is_text_based: boolean
  is_default: boolean
}

export interface VideoFileMeta {
  container: string
  is_episode: boolean
  title: string
  original_title: string | null
  show_title: string | null
  season_number: number | null
  episode_number: number | null
  year: number | null
  rating: number | null
  summary: string | null
  genres: string[]
  tags: string[]
  actors: string[]
  directors: string[]
  studio: string | null
  collection_set?: string | null
  tmdb_id: string | null
  bangumi_id: string | null
  locked_fields?: string[]
  has_nfo: boolean
  nfo_filename: string
  has_poster: boolean
  has_fanart: boolean
  has_thumb: boolean
  has_clearlogo?: boolean
  duration_secs: number
  width: number | null
  height: number | null
  video_codec: string | null
  bit_rate_kbps: number | null
  audio_streams: ProbedAudioStreamDto[]
  subtitle_streams: ProbedSubtitleStreamDto[]
  sidecar_subtitles: string[]
}

export type MediaMetadataPayload =
  | { kind: 'book'; data: BookFileMeta }
  | { kind: 'comic'; data: ComicFileMeta }
  | { kind: 'music'; data: MusicFileMeta }
  | { kind: 'video'; data: VideoFileMeta }

export interface MediaFileDetail {
  entry: MediaFileEntry
  metadata: MediaMetadataPayload
}

export interface BookMetaPatch {
  title?: string
  authors?: string[]
  series?: string
  subseries?: string
  series_index?: string
  description?: string
  publisher?: string
  published_at?: string
  language?: string
  isbn?: string
  tags?: string[]
  locked_fields?: string[]
}

export interface ComicMetaPatch {
  series?: string
  series_group?: string
  title?: string
  number?: string
  volume?: number
  summary?: string
  writers?: string[]
  pencillers?: string[]
  publisher?: string
  genres?: string[]
  tags?: string[]
  year?: number
  language_iso?: string
  reading_direction?: string
}

export interface MusicMetaPatch {
  title?: string
  artist?: string
  album_artist?: string
  album?: string
  track_number?: number
  disc_number?: number
  year?: number
  genre?: string
  comment?: string
  embedded_lyrics?: string
  sidecar_lrc?: string
}

export interface VideoMetaPatch {
  is_episode?: boolean
  title?: string
  original_title?: string
  show_title?: string
  season_number?: number
  episode_number?: number
  year?: number
  rating?: number
  summary?: string
  genres?: string[]
  tags?: string[]
  actors?: string[]
  directors?: string[]
  studio?: string
  collection_set?: string
  tmdb_id?: string
  bangumi_id?: string
  locked_fields?: string[]
}

export type MediaMetadataPatchEnvelope =
  | { kind: 'book'; patch: BookMetaPatch }
  | { kind: 'comic'; patch: ComicMetaPatch }
  | { kind: 'music'; patch: MusicMetaPatch }
  | { kind: 'video'; patch: VideoMetaPatch }

export interface BatchTextReplaceRuleDto {
  find: string
  replace: string
  use_regex: boolean
  fields: string[]
}

export interface BatchMetadataEditInputDto {
  paths: string[]
  media_kind: MediaKind
  title_template?: string
  creator?: string
  series_or_album?: string
  publisher_or_studio?: string
  year?: number | null
  language?: string
  genre?: string
  add_tags?: string[]
  remove_tags?: string[]
  replace_rule?: BatchTextReplaceRuleDto
  zh_direction?: ZhConvertDirection | null
}

export interface BatchMetadataEditReportDto {
  batch_id: string | null
  updated_count: number
  skipped_count: number
  failed_count: number
  errors: string[]
}

export interface MetadataSnapshotRecordDto {
  rel_path: string
  file_name: string
}

export interface MetadataHistoryBatchDto {
  batch_id: string
  root_id: string
  media_kind: MediaKind
  label: string
  created_at_unix: number
  updated_count: number
  records: MetadataSnapshotRecordDto[]
}

export interface UndoMetadataReportDto {
  batch_id: string
  restored_count: number
  failed_paths: string[]
}

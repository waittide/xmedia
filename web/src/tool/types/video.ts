import type { ZhConvertDirection } from './common'

export interface VideoSubtitleInfoDto {
  filename: string
  extension: string
  file_size: number
  detected_encoding: string
  is_utf8: boolean
  language_tag: string | null
  forced: boolean
}

export interface ProcessSubtitleInput {
  subtitle_filename: string
  convert_utf8?: boolean
  offset_ms?: number
  zh_direction?: ZhConvertDirection | null
  rename_language?: string
  forced?: boolean
}

export interface ProcessSubtitleReportDto {
  original_filename: string
  final_filename: string
  original_encoding: string
  shifted_cues: number
  converted_utf8: boolean
}

export interface ExtractSubtitleReportDto {
  created_filename: string
  file_size: number
}

export interface BatchRemapEpisodesInput {
  show_title?: string
  season_number?: number | null
  mode?: 'sequential' | 'absolute_offset'
  start_episode?: number
  episode_offset?: number
  write_season_nfo?: boolean
}

export interface BatchRemapEpisodesReportDto {
  updated_count: number
  failed_paths: string[]
}

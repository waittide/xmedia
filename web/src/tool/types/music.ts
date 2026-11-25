import type { ZhConvertDirection } from './common'

export interface MusicLyricsToolInput {
  offset_ms?: number
  strip_credit_noise?: boolean
  zh_direction?: ZhConvertDirection | null
  sync_mode?: 'none' | 'sidecar_to_embedded' | 'embedded_to_sidecar' | 'both'
  custom_text?: string
}

export interface MusicLyricsToolReportDto {
  updated_embedded: boolean
  updated_sidecar: boolean
  removed_lines: number
  final_lyrics: string
}

export interface MusicCoverOptimizeReportDto {
  had_cover: boolean
  optimized: boolean
  before_bytes: number
  after_bytes: number
  width: number | null
  height: number | null
}

export interface ReplayGainTrackResultDto {
  rel_path: string
  track_gain_db: number
  track_peak: number
}

export interface ReplayGainReportDto {
  processed: number
  album_gain_db: number | null
  album_peak: number | null
  tracks: ReplayGainTrackResultDto[]
  failed_paths: string[]
}

export interface CueSplitReportDto {
  created_files: string[]
  album_title: string | null
  album_artist: string | null
}

export interface BatchMusicAlbumSyncInput {
  album?: string
  album_artist?: string
  artist?: string
  year?: number | null
  genre?: string
  disc_number?: number | null
  auto_number_tracks?: boolean
  start_track?: number
}

export interface BatchMusicAlbumSyncReportDto {
  updated_count: number
  failed_paths: string[]
}

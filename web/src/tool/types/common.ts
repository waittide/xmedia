import type { FfmpegStatusDto } from '@/types/api'

export type ScrapeTargetKind =
  | 'book'
  | 'comic'
  | 'music_track'
  | 'music_album'
  | 'lyrics'
  | 'movie'
  | 'tv_show'

export type ZhConvertDirection = 't2s' | 's2t'

export interface ToolStatusDto {
  version: string
  requires_password: boolean
  authenticated: boolean
  ffmpeg: FfmpegStatusDto
}

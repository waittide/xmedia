import type { CoreLibraryKind, CoreScanStatus, VideoQualityProfile } from '@/types/api'

export type { VideoQualityProfile }

export type VideoContainer = 'mp4' | 'mkv' | 'webm' | 'mov' | 'avi' | 'ts' | 'flv' | 'm4v'
export type VideoKindFilter = 'all' | 'standalone' | 'episodes'
export type VideoSortBy = 'created_at' | 'title' | 'year' | 'duration'

export interface VideoLibraryDto {
  id: string
  name: string
  kind: CoreLibraryKind
  video_count: number
  scan_status: CoreScanStatus
  last_scanned_at: string | null
  created_at: string
  updated_at: string
}

export interface VideoSystemStatusDto {
  ffmpeg: {
    available: boolean
    version: string | null
  }
  scanning: boolean
  video_count: number
}

export interface ProbedAudioStream {
  stream_index: number
  codec: string
  channels: number
  language: string | null
  title: string | null
  is_default: boolean
}

export interface SubtitleTrackDto {
  id: string
  source_kind: string
  codec_or_format: string
  language: string | null
  label: string
  is_default: boolean
}

export interface WatchProgressDto {
  user_id: string
  video_id: string
  position_secs: number
  duration_secs: number
  completed: boolean
  selected_audio_stream: number | null
  selected_subtitle: string | null
  updated_at: string
}

export interface OnlineTrackedVideoDto {
  source_id: string
  video_id: string
  detail_url: string
  play_url: string
  group_name: string | null
  episode_name: string | null
  cached: boolean
}

export interface OnlineTrackedShowDto {
  source_id: string
  item_key: string
}

export interface VideoSummaryDto {
  id: string
  library_id: string
  show_id: string | null
  show_title: string | null
  season_number: number | null
  episode_number: number | null
  title: string
  summary: string
  year: number | null
  genres: string[]
  tags: string[]
  container: VideoContainer
  mime_type: string
  can_direct_play: boolean
  file_size: number
  duration_secs: number
  width: number | null
  height: number | null
  resolution: string | null
  video_codec: string | null
  poster_key: string | null
  is_favorite: boolean
  progress: WatchProgressDto | null
  created_at: string
  online_tracking?: OnlineTrackedVideoDto | null
}

export interface VideoDetailDto {
  video: VideoSummaryDto
  bit_rate_kbps: number | null
  path: string
  audio_streams: ProbedAudioStream[]
  subtitles: SubtitleTrackDto[]
  prev_episode_id: string | null
  next_episode_id: string | null
}

export interface ShowSummaryDto {
  id: string
  library_id: string
  title: string
  summary: string
  year: number | null
  genres: string[]
  tags: string[]
  poster_key: string | null
  season_count: number
  episode_count: number
  watched_episode_count: number
  is_favorite: boolean
  created_at: string
  online_tracking?: OnlineTrackedShowDto | null
}

export interface SeasonGroupDto {
  season_number: number
  episodes: VideoSummaryDto[]
}

export interface ShowDetailDto {
  show: ShowSummaryDto
  seasons: SeasonGroupDto[]
  continue_episode: VideoSummaryDto | null
}

export interface VideoHomeOverviewDto {
  continue_watching: VideoSummaryDto[]
  recent_videos: VideoSummaryDto[]
  recent_shows: ShowSummaryDto[]
  total_videos: number
  movies_count: number
  total_shows: number
  watching_count: number
  favorites_count: number
  shelves_count: number
  genres_count: number
  tags_count: number
  libraries_count: number
}

export interface VideoFavoritesDto {
  videos: VideoSummaryDto[]
  shows: ShowSummaryDto[]
}

export interface VideoShelfDto {
  id: string
  name: string
  description: string
  video_count: number
  poster_key: string | null
  updated_at: string
}

export interface VideoShelfDetailDto {
  shelf: VideoShelfDto
  videos: VideoSummaryDto[]
}

export interface VideoMetadataPatch {
  title?: string
  summary?: string
  year?: number | null
  genres?: string[]
  tags?: string[]
  season_number?: number | null
  episode_number?: number | null
}

export interface ShowMetadataPatch {
  title?: string
  summary?: string
  year?: number | null
  genres?: string[]
  tags?: string[]
}

export interface VideoTagStat {
  name: string
  count: number
}

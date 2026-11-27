import type { ThemeMode } from '@/theme/md3'

export type LocaleCode = 'zh-CN' | 'en-US'
export type MediaKind = 'book' | 'comic' | 'music' | 'video'
export type UserRole = 'admin' | 'member'
export type ReadingFlow = 'paginated' | 'scrolled'
export type ComicReaderMode = 'rtl_double' | 'ltr_single' | 'ltr_double' | 'webtoon'
export type ComicFitMode = 'fit_height' | 'fit_width' | 'original'
export type TranscodeProfile = 'raw' | 'opus128' | 'mp3192' | 'mp3320' | 'aac256'
export type PlayMode = 'sequence' | 'loop_one' | 'shuffle'
export type VideoQualityProfile = 'auto' | 'direct' | 'p1080' | 'p720' | 'p480'
export type CustomFontFormat = 'ttf' | 'otf' | 'ttc' | 'woff' | 'woff2'

export interface CustomFontDto {
  id: string
  name: string
  file_name: string
  format: CustomFontFormat
  css_format: string
  mime_type: string
  size_bytes: number
  created_at: string
}

export interface ReaderPreferences {
  font_family: string
  font_size: number
  line_height: number
  margin_px: number
  margin_top_px: number
  margin_bottom_px: number
  margin_left_px: number
  margin_right_px: number
  paragraph_indent_em: number
  letter_spacing_em: number
  paragraph_spacing_px: number
  text_color?: string | null
  bg_color?: string | null
  flow: ReadingFlow
  pdf_scale: number
}

export interface ComicReaderPreferences {
  default_reader_mode: ComicReaderMode
  default_fit_mode: ComicFitMode
  offset_cover_page: boolean
  dim_images_in_dark: boolean
  preload_pages: number
  webtoon_width: number
}

export interface PlaybackPreferences {
  transcode_profile: TranscodeProfile
  volume: number
  muted: boolean
  play_mode: PlayMode
}

export interface VideoPlaybackPreferences {
  default_quality: VideoQualityProfile
  preferred_audio_lang: string | null
  preferred_subtitle_lang: string | null
  autoplay_next: boolean
  playback_rate: number
}

export interface UserPreferences {
  theme_mode: ThemeMode
  seed_color?: string | null
  app_icon?: string | null
  ui_font?: string | null
  locale: LocaleCode
  active_domain?: MediaKind | null
  book: ReaderPreferences
  comic: ComicReaderPreferences
  music: PlaybackPreferences
  video: VideoPlaybackPreferences
}

export interface UserDto {
  id: string
  username: string
  display_name: string
  role: UserRole
  is_active: boolean
  api_key: string
  preferences: UserPreferences
  created_at: string
  updated_at: string
}

export interface AuthSessionDto {
  token: string
  user: UserDto
}

export interface FfmpegStatusDto {
  ffmpeg_available: boolean
  ffmpeg_version: string | null
  ffprobe_available: boolean
}

export interface SystemCapabilitiesDto {
  version: string
  modules: MediaKind[]
  ffmpeg: FfmpegStatusDto
}

export type CoreLibraryKind =
  | { type: 'managed' }
  | { type: 'local_mount'; root_path: string }

export type CoreScanStatus =
  | { state: 'idle' }
  | { state: 'scanning'; scanned: number; total?: number | null }
  | { state: 'completed'; scanned: number; added: number; updated: number; missing: number }
  | { state: 'failed'; message: string }

export interface CoreLibraryDto {
  id: string
  name: string
  media_kind: MediaKind
  kind: CoreLibraryKind
  scan_status: CoreScanStatus
  item_count: number
  last_scanned_at: string | null
  created_at: string
  updated_at: string
}

export interface SystemOverviewDto {
  capabilities: SystemCapabilitiesDto
  domains: {
    media_kind: MediaKind
    library_count: number
    item_count: number
    scanning: boolean
  }[]
  libraries: CoreLibraryDto[]
}

export interface BindMediaItemsInput {
  item_ids?: string[]
  library_id?: string | null
  user_id?: string | null
}

export interface BindMediaItemsResultDto {
  bound_count: number
  total_requested: number
}

export interface UnbindMediaItemResultDto {
  unbound: boolean
  remaining_bindings: number
  deleted_from_pool: boolean
}

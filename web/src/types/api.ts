import type {
  AccentMode,
  CatppuccinAccent,
  CatppuccinFlavor,
  CustomPaletteTokens,
  ThemeMode,
} from '@/theme/catppuccin'

export type MediaKind = 'book' | 'comic' | 'music' | 'video'
export type UserRole = 'admin' | 'member'
export type Locale = 'zh-CN' | 'en-US'

// --- Preferences ---

export type ReadingFlow = 'paginated' | 'scrolled'
export interface BookReaderPreferences {
  font_family: string
  font_size: number
  line_height: number
  margin_px: number
  flow: ReadingFlow
  pdf_scale: number
}

export type ComicReaderMode = 'rtl_double' | 'ltr_single' | 'ltr_double' | 'webtoon'
export type ComicFitMode = 'fit_height' | 'fit_width' | 'original'
export interface ComicReaderPreferences {
  default_reader_mode: ComicReaderMode
  default_fit_mode: ComicFitMode
  offset_cover_page: boolean
  dim_images_in_dark: boolean
  preload_pages: number
  webtoon_width: number
}

export type AudioTranscodeProfile = 'raw' | 'opus128' | 'mp3192' | 'mp3320' | 'aac256'
export type MusicPlayMode = 'sequence' | 'loop_one' | 'shuffle'
export interface MusicPlaybackPreferences {
  transcode_profile: AudioTranscodeProfile
  volume: number
  muted: boolean
  play_mode: MusicPlayMode
}

export type VideoQualityProfile = 'auto' | 'direct' | 'p1080' | 'p720' | 'p480'
export interface VideoPlaybackPreferences {
  default_quality: VideoQualityProfile
  preferred_audio_lang: string | null
  preferred_subtitle_lang: string | null
  autoplay_next: boolean
  playback_rate: number
}

export interface UserPreferences {
  theme_mode: ThemeMode
  light_preset: string
  dark_preset: string
  dark_flavor: CatppuccinFlavor
  accent_mode: AccentMode | string
  accent: CatppuccinAccent
  custom_accent: string | null
  custom_light: CustomPaletteTokens | null
  custom_dark: CustomPaletteTokens | null
  locale: Locale
  book: BookReaderPreferences
  comic: ComicReaderPreferences
  music: MusicPlaybackPreferences
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

export type ScanStatus =
  | { state: 'idle' }
  | { state: 'scanning'; scanned: number }
  | { state: 'completed'; scanned: number; added: number; updated: number; missing: number }
  | { state: 'failed'; message: string }

export type LibraryKind =
  | { type: 'managed' }
  | { type: 'local_mount'; root_path: string }

export interface LibraryDto {
  id: string
  name: string
  media_kind: MediaKind
  kind: LibraryKind
  scan_status: ScanStatus
  item_count: number
  last_scanned_at: string | null
  created_at: string
  updated_at: string
}

export interface DomainStatSummary {
  media_kind: MediaKind
  library_count: number
  item_count: number
  scanning: boolean
}

export interface SystemOverviewDto {
  capabilities: SystemCapabilitiesDto
  domains: DomainStatSummary[]
  libraries: LibraryDto[]
}

export interface PagedResult<T> {
  items: T[]
  total: number
  page: number
  page_size: number
}

// --- Book Domain ---

export type BookFormat = 'epub' | 'pdf' | 'txt'

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
  cover_mime: string | null
  metadata: BookMetadata
  is_missing: boolean
  is_favorite: boolean
  progress: ReadingLocation | null
  last_read_at: string | null
  created_at: string
  updated_at: string
}

export interface TocItem {
  index: number
  title: string
  href: string
  level: number
}

export interface TxtChapterContent {
  index: number
  volume_title?: string
  title: string
  content: string
  total_chapters: number
}

export interface BookBookmark {
  id: string
  book_id: string
  title: string
  note: string | null
  location: ReadingLocation
  created_at: string
}

export interface BookShelfDto {
  id: string
  name: string
  is_favorite: boolean
  book_ids: string[]
  book_count: number
  created_at: string
  updated_at: string
}

// --- Comic Domain ---

export interface SeriesSummaryDto {
  id: string
  library_id: string
  title: string
  authors: string[]
  tags: string[]
  genres: string[]
  status: string
  reading_direction: string
  chapter_count: number
  completed_chapters: number
  is_favorite: boolean
  cover_url: string | null
  updated_at: string
}

export interface ContinueReadingItemDto {
  series: SeriesSummaryDto
  chapter_id: string
  chapter_number: string
  chapter_title: string
  current_page: number
  page_count: number
  last_read_at: string
}

export interface ComicHomeOverviewDto {
  continue_reading: ContinueReadingItemDto[]
  recently_updated: SeriesSummaryDto[]
  recently_added: SeriesSummaryDto[]
  stats: {
    series_count: number
    chapter_count: number
    completed_chapters: number
    favorites_count: number
  }
}

export interface ComicChapterItemDto {
  id: string
  series_id: string
  kind: string
  number: number
  number_label: string
  volume_number: number | null
  title: string
  page_count: number
  file_size: number
  cover_url: string | null
  summary: string
  is_missing: boolean
  current_page: number
  is_completed: boolean
  created_at: string
}

export interface SeriesDetailDto {
  series: {
    id: string
    library_id: string
    title: string
    summary: string
    writers: string[]
    pencillers: string[]
    genres: string[]
    tags: string[]
    reading_direction: string
  }
  cover_url: string | null
  is_favorite: boolean
  completed_chapters: number
  continue_reading: {
    chapter_id: string
    chapter_number: string
    chapter_title: string
    current_page: number
    page_count: number
  } | null
  chapters: ComicChapterItemDto[]
}

export interface ComicReaderContextDto {
  series_id: string
  series_title: string
  reading_direction: string
  chapter: ComicChapterItemDto
  effective_pref: {
    reader_mode: ComicReaderMode
    fit_mode: ComicFitMode
    offset_cover_page: boolean
    dim_images_in_dark: boolean
    preload_pages: number
    webtoon_width: number
  }
  prev_chapter: { id: string; number_label: string; title: string; page_count: number } | null
  next_chapter: { id: string; number_label: string; title: string; page_count: number } | null
  all_chapters: { id: string; number_label: string; title: string; page_count: number; is_completed: boolean }[]
}

// --- Music Domain ---

export interface TrackDto {
  id: string
  library_id: string
  album_id: string
  artist_id: string
  album_artist_id: string
  title: string
  artist_name: string
  album_artist_name: string
  album_title: string
  track_number: number | null
  disc_number: number | null
  year: number | null
  genre: string | null
  duration_secs: number
  bit_rate_kbps: number | null
  sample_rate_hz: number | null
  bit_depth: number | null
  channels: number | null
  format: string
  mime_type: string
  is_lossless: boolean
  file_size: number
  has_lyrics: boolean
  cover_key: string | null
  starred_at: string | null
  user_rating: number | null
  play_count: number
  last_played_at: string | null
  created_at: string
}

export interface AlbumDto {
  id: string
  library_id: string
  title: string
  album_artist_id: string
  album_artist_name: string
  year: number | null
  genre: string | null
  disc_count: number
  track_count: number
  duration_secs: number
  cover_key: string | null
  starred_at: string | null
  play_count: number
  created_at: string
}

export interface ArtistDto {
  id: string
  name: string
  album_count: number
  track_count: number
  cover_key: string | null
  starred_at: string | null
}

export interface AlbumDetailDto {
  album: AlbumDto
  tracks: TrackDto[]
}

export interface ArtistDetailDto {
  artist: ArtistDto
  albums: AlbumDto[]
  appears_on: AlbumDto[]
  top_tracks: TrackDto[]
}

export interface PlaylistSummaryDto {
  id: string
  owner_id: string
  owner_username: string
  name: string
  comment: string | null
  is_public: boolean
  track_count: number
  duration_secs: number
  cover_key: string | null
}

export interface PlaylistDetailDto {
  playlist: PlaylistSummaryDto
  tracks: TrackDto[]
}

export interface StructuredLyricsDto {
  lang: string
  synced: boolean
  offset_ms: number
  display_artist: string | null
  display_title: string | null
  lines: { start_ms: number; text: string }[]
  plain_text: string
}

export interface MusicHomeOverviewDto {
  recent_added_albums: AlbumDto[]
  frequent_albums: AlbumDto[]
  recent_played_tracks: TrackDto[]
  random_tracks: TrackDto[]
  stats: {
    total_tracks: number
    total_albums: number
    total_artists: number
    total_genres: number
  }
}

export interface PlayQueueDto {
  tracks: TrackDto[]
  current_track_id: string | null
  position_ms: number
  changed_by: string
  updated_at: string
}

// --- Video Domain ---

export interface WatchProgress {
  user_id: string
  video_id: string
  position_secs: number
  duration_secs: number
  completed: boolean
  selected_audio_stream: number | null
  selected_subtitle: string | null
  updated_at: string
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
  container: string
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
  progress: WatchProgress | null
  created_at: string
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
}

export interface ShowDetailDto {
  show: ShowSummaryDto
  seasons: { season_number: number; episodes: VideoSummaryDto[] }[]
  continue_episode: VideoSummaryDto | null
}

export interface VideoHomeOverviewDto {
  continue_watching: VideoSummaryDto[]
  recent_videos: VideoSummaryDto[]
  recent_shows: ShowSummaryDto[]
  total_videos: number
  total_shows: number
}

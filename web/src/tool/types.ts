import type { FfmpegStatusDto, MediaKind } from '@/types/api'

export type ScrapeTargetKind =
  | 'book'
  | 'comic'
  | 'music_track'
  | 'music_album'
  | 'lyrics'
  | 'movie'
  | 'tv_show'

export interface ToolStatusDto {
  version: string
  requires_password: boolean
  authenticated: boolean
  ffmpeg: FfmpegStatusDto
}

export interface ManagedRootDto {
  id: string
  name: string
  path: string
  default_kind: MediaKind | null
}

export interface RenameTemplatesDto {
  book: string
  comic: string
  music: string
  video_movie: string
  video_episode: string
}

export interface ScraperPrioritiesDto {
  book: string[]
  comic: string[]
  music: string[]
  video: string[]
}

export interface PublicSettingsDto {
  roots: ManagedRootDto[]
  has_tmdb_api_key: boolean
  tmdb_api_key_masked: string | null
  has_bangumi_token: boolean
  bangumi_token_masked: string | null
  http_proxy: string | null
  rename_templates: RenameTemplatesDto
  scraper_priorities: ScraperPrioritiesDto
  default_scrape_locale: string
}

export interface UpdateToolSettingsInput {
  tmdb_api_key?: string
  bangumi_access_token?: string
  http_proxy?: string
  rename_templates?: RenameTemplatesDto
  scraper_priorities?: ScraperPrioritiesDto
  default_scrape_locale?: string
}

export interface CompletenessFlags {
  has_title: boolean
  has_creator: boolean
  has_cover: boolean
  has_summary_or_lyrics: boolean
  has_nfo_or_tag_block: boolean
}

export interface FolderEntryDto {
  name: string
  rel_path: string
  child_dir_count: number
  media_file_count: number
}

export interface MediaFileEntry {
  root_id: string
  rel_path: string
  file_name: string
  extension: string
  media_kind: MediaKind
  file_size: number
  modified_unix_secs: number
  display_title: string
  display_subtitle: string
  year: number | null
  completeness: CompletenessFlags
}

export interface DirectorySnapshot {
  root_id: string
  root_name: string
  current_rel_path: string
  parent_rel_path: string | null
  folders: FolderEntryDto[]
  files: MediaFileEntry[]
}

export interface BookFileMeta {
  format: string
  title: string
  authors: string[]
  description: string | null
  publisher: string | null
  published_at: string | null
  language: string | null
  isbn: string | null
  tags: string[]
  has_cover: boolean
  writable: boolean
}

export interface ComicFileMeta {
  format: string
  series: string | null
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
  tmdb_id: string | null
  bangumi_id: string | null
  has_nfo: boolean
  nfo_filename: string
  has_poster: boolean
  has_fanart: boolean
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
  description?: string
  publisher?: string
  published_at?: string
  language?: string
  isbn?: string
  tags?: string[]
}

export interface ComicMetaPatch {
  series?: string
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
  tmdb_id?: string
  bangumi_id?: string
}

export type MediaMetadataPatchEnvelope =
  | { kind: 'book'; patch: BookMetaPatch }
  | { kind: 'comic'; patch: ComicMetaPatch }
  | { kind: 'music'; patch: MusicMetaPatch }
  | { kind: 'video'; patch: VideoMetaPatch }

export interface RenamePlanItem {
  from_rel_path: string
  to_rel_path: string
  companion_moves: [string, string][]
  conflict: string | null
  unchanged: boolean
}

export interface ProviderStatusDto {
  id: string
  name: string
  description: string
  website: string
  supported_targets: ScrapeTargetKind[]
  requires_api_key: boolean
  configured: boolean
}

export interface ScrapeQuery {
  target?: ScrapeTargetKind
  providers?: string[]
  keyword: string
  artist_or_author?: string
  album_or_series?: string
  year?: number | null
  season?: number | null
  episode?: number | null
  duration_secs?: number | null
  isbn?: string
  locale?: string
}

export interface ScrapeCandidate {
  provider_id: string
  provider_name: string
  external_id: string
  target: ScrapeTargetKind
  title: string
  subtitle: string | null
  year: number | null
  summary: string | null
  cover_url: string | null
  synced_lyrics: string | null
  plain_lyrics: string | null
}

export interface ScrapedMetadata {
  provider_id: string
  external_id: string
  target: ScrapeTargetKind | null
  title: string | null
  original_title: string | null
  series_or_album: string | null
  creators: string[]
  illustrators_or_directors: string[]
  cast: string[]
  publisher_or_studio: string | null
  year: number | null
  published_date: string | null
  language: string | null
  isbn: string | null
  rating: number | null
  track_or_episode_number: number | null
  disc_or_season_number: number | null
  summary: string | null
  show_summary?: string | null
  season_title?: string | null
  season_summary?: string | null
  season_poster_url?: string | null
  episode_still_url?: string | null
  genres: string[]
  tags: string[]
  cover_url: string | null
  backdrop_url: string | null
  synced_lyrics: string | null
  plain_lyrics: string | null
  external_ids?: Record<string, string>
}

export interface FieldSourceBadge {
  provider_id: string
  provider_name: string
}

export interface MergedScrapedResult {
  metadata: ScrapedMetadata
  field_sources: Record<string, FieldSourceBadge>
}

export interface ProviderBriefDto {
  id: string
  name: string
}

export interface AggregateScrapeResponse {
  provider_order: ProviderBriefDto[]
  candidates: ScrapeCandidate[]
  selected_by_provider: Record<string, ScrapeCandidate>
  details_by_provider: Record<string, ScrapedMetadata>
  merged: MergedScrapedResult
}

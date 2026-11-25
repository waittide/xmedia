import type { ScrapeTargetKind } from './common'
import type { MediaFileDetail } from './inspector'

export interface ScrapeCacheStatsDto {
  total_entries: number
  valid_entries: number
  total_bytes: number
}

export type AutoScrapeMatchMode = 'strict' | 'standard' | 'loose'

export interface AutoScrapeOptionsDto {
  match_mode?: AutoScrapeMatchMode
  only_fill_missing?: boolean
  apply_cover?: boolean
  write_sidecar_lrc?: boolean
  organize_by_template?: boolean
  providers?: string[]
  field_groups?: string[]
}

export interface AutoScrapeResultDto {
  matched: boolean
  confidence?: number
  detail: MediaFileDetail
}

export interface ProviderStatusDto {
  id: string
  name: string
  description: string
  website: string
  supported_targets: ScrapeTargetKind[]
  requires_api_key: boolean
  enabled: boolean
  credentials_ready: boolean
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

export interface DryRunScrapeItemDto {
  rel_path: string
  display_title: string
  query_keyword: string
  matched: boolean
  matched_title: string | null
  matched_creator: string | null
  matched_year: number | null
  matched_provider: string | null
  confidence: number
  low_confidence: boolean
  error: string | null
}

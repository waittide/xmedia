import type { MediaKind } from '@/types/api'

export interface ManagedRootDto {
  id: string
  name: string
  path: string
  default_kind: MediaKind | null
  read_only?: boolean
}

export interface InboxConfigDto {
  enabled: boolean
  inbox_path: string | null
  target_root_id: string | null
  use_hardlink: boolean
  delete_source: boolean
  auto_scrape: boolean
  organize_by_template: boolean
}

export interface InboxImportedItemDto {
  source_rel: string
  target_rel: string
  mode: string
  scraped: boolean
  error: string | null
}

export interface InboxScanReportDto {
  imported_count: number
  skipped_count: number
  failed_count: number
  items: InboxImportedItemDto[]
}

export interface WebhookTestReportDto {
  ok: boolean
  matched_libraries: string[]
  total_libraries: number
  message: string
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

export interface ProviderConfigOverrideDto {
  enabled?: boolean | null
  http_proxy?: string | null
  locale?: string | null
  timeout_secs?: number | null
  user_agent?: string | null
}

export interface PublicSettingsDto {
  roots: ManagedRootDto[]
  has_tmdb_api_key: boolean
  tmdb_api_key_masked: string | null
  has_bangumi_token: boolean
  bangumi_token_masked: string | null
  has_qishui_cookie?: boolean
  qishui_cookie_masked?: string | null
  http_proxy: string | null
  enable_adult_scrapers?: boolean
  rename_templates: RenameTemplatesDto
  scraper_priorities: ScraperPrioritiesDto
  provider_configs?: Record<string, ProviderConfigOverrideDto>
  default_scrape_locale: string
  scrape_cache_ttl_ms: number
  inbox?: InboxConfigDto
  xmedia_webhook_enabled?: boolean
  xmedia_webhook_base_url?: string
  has_xmedia_webhook_api_key?: boolean
  xmedia_webhook_api_key_masked?: string | null
  xmedia_webhook_notify_on_rename?: boolean
  xmedia_webhook_notify_on_scrape?: boolean
  xmedia_webhook_notify_on_inbox?: boolean
}

export interface UpdateToolSettingsInput {
  tmdb_api_key?: string
  bangumi_access_token?: string
  qishui_cookie?: string
  http_proxy?: string
  enable_adult_scrapers?: boolean
  rename_templates?: RenameTemplatesDto
  scraper_priorities?: ScraperPrioritiesDto
  provider_configs?: Record<string, ProviderConfigOverrideDto>
  default_scrape_locale?: string
  scrape_cache_ttl_ms?: number
  inbox?: InboxConfigDto
  xmedia_webhook_enabled?: boolean
  xmedia_webhook_base_url?: string
  xmedia_webhook_api_key?: string
  xmedia_webhook_notify_on_rename?: boolean
  xmedia_webhook_notify_on_scrape?: boolean
  xmedia_webhook_notify_on_inbox?: boolean
}

import type { MediaKind } from '@/types/api'
import { SCRAPER_IMAGE_PROXY_DOMAINS } from '../providers'
import type {
  AggregateScrapeResponse,
  AutoScrapeOptionsDto,
  AutoScrapeResultDto,
  DryRunScrapeItemDto,
  ProviderStatusDto,
  ScrapeCacheStatsDto,
  ScrapeQuery,
  ScrapeTargetKind,
  ScrapedMetadata,
} from '../types'
import { toolRequest } from './client'

function needsScraperImageProxy(rawUrl: string): boolean {
  try {
    const parsed = new URL(rawUrl)
    if (parsed.protocol !== 'http:' && parsed.protocol !== 'https:') return false
    const host = parsed.hostname.toLowerCase()
    return SCRAPER_IMAGE_PROXY_DOMAINS.some(
      (domain) => host === domain || host.endsWith(`.${domain}`),
    )
  } catch {
    return false
  }
}

export function resolveScraperCoverUrl(rawUrl: string | null | undefined): string {
  if (!rawUrl) return ''
  const cleaned = rawUrl.replace(/[\u0000-\u001F\u007F]+/g, '').trim()
  if (!cleaned) return ''
  if (cleaned.startsWith('/')) {
    if (cleaned.startsWith('//')) return ''
    return cleaned
  }
  try {
    const parsed = new URL(cleaned)
    if (parsed.protocol !== 'http:' && parsed.protocol !== 'https:') {
      return ''
    }
    if (needsScraperImageProxy(parsed.href)) {
      const params = new URLSearchParams({ url: parsed.href })
      return `/api/v1/tool/scrapers/image-proxy?${params.toString()}`
    }
    return parsed.href
  } catch {
    return ''
  }
}

export const scrapeApi = {
  listProviders(target?: ScrapeTargetKind, domain?: MediaKind): Promise<ProviderStatusDto[]> {
    const params = new URLSearchParams()
    if (target) params.set('target', target)
    if (domain) params.set('domain', domain)
    const qs = params.toString()
    return toolRequest<ProviderStatusDto[]>(`/scrapers/providers${qs ? `?${qs}` : ''}`)
  },

  aggregateScrapers(query: ScrapeQuery): Promise<AggregateScrapeResponse> {
    return toolRequest<AggregateScrapeResponse>('/scrapers/aggregate', {
      method: 'POST',
      body: JSON.stringify(query),
    })
  },

  getScraperDetail(
    providerId: string,
    externalId: string,
    target: ScrapeTargetKind,
    locale?: string,
  ): Promise<ScrapedMetadata> {
    const q = new URLSearchParams({
      provider_id: providerId,
      external_id: externalId,
      target,
    })
    if (locale) q.set('locale', locale)
    return toolRequest<ScrapedMetadata>(`/scrapers/detail?${q.toString()}`)
  },

  autoScrapeFile(
    rootId: string,
    relPath: string,
    mediaKind: MediaKind,
    options?: AutoScrapeOptionsDto,
    signal?: AbortSignal,
  ): Promise<AutoScrapeResultDto> {
    return toolRequest<AutoScrapeResultDto>('/scrapers/auto-apply', {
      method: 'POST',
      signal,
      body: JSON.stringify({
        root_id: rootId,
        path: relPath,
        media_kind: mediaKind,
        ...(options ?? {}),
      }),
    })
  },

  getScrapeCacheStats(): Promise<ScrapeCacheStatsDto> {
    return toolRequest<ScrapeCacheStatsDto>('/scrapers/cache')
  },

  clearScrapeCache(): Promise<{ removed_count: number }> {
    return toolRequest<{ removed_count: number }>('/scrapers/cache', {
      method: 'DELETE',
    })
  },

  dryRunScrape(
    rootId: string,
    paths: string[],
    locale?: string,
    options?: AutoScrapeOptionsDto,
  ): Promise<DryRunScrapeItemDto[]> {
    return toolRequest<DryRunScrapeItemDto[]>('/scrapers/dry-run', {
      method: 'POST',
      body: JSON.stringify({
        root_id: rootId,
        paths,
        locale: locale || undefined,
        ...(options ?? {}),
      }),
    })
  },
}

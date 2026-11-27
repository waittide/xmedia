import { requestJson, requestText, withAuthToken } from '@/api/client'
import {
  isSafeDataImageUrl,
  isSafeHttpUrl,
  type BatchExportPayload,
  type BatchSourcesPayload,
  type BatchSourcesReportDto,
  type ImportSourcesReportDto,
  type LoginActionResponseDto,
  type OnlineDomain,
  type OnlineSourceSummaryDto,
  type PluginAuthActionPayload,
  type PluginAuthActionResponseDto,
  type UpdateSourcePatch,
} from './types'

const DOMAIN_IMAGE_PROXY_PATHS: Record<OnlineDomain, string> = {
  book: '/api/v1/book/online/cover-proxy',
  comic: '/api/v1/comic/online/image-proxy',
  music: '/api/v1/music/online/cover-proxy',
  video: '/api/v1/video/online/poster-proxy',
}

export function buildDomainProxyImageUrl(
  domain: OnlineDomain,
  rawUrl: string | null | undefined,
  sourceId?: string | null,
  cacheBust?: string | number | null,
): string {
  if (!rawUrl || !rawUrl.trim()) return ''
  const u = rawUrl.trim()
  if (u.toLowerCase().startsWith('data:')) {
    return isSafeDataImageUrl(u) ? u : ''
  }
  if (!isSafeHttpUrl(u)) return ''
  const qs = new URLSearchParams({ url: u })
  if (sourceId && sourceId.trim()) {
    qs.set('source_id', sourceId.trim())
  }
  if (cacheBust !== undefined && cacheBust !== null && String(cacheBust) !== '') {
    qs.set('_r', String(cacheBust))
  }
  return withAuthToken(`${DOMAIN_IMAGE_PROXY_PATHS[domain]}?${qs.toString()}`)
}

export interface DomainSourceCrudApi<S extends OnlineSourceSummaryDto = OnlineSourceSummaryDto> {
  listSources: () => Promise<S[]>
  importSources: (rawJson: string) => Promise<ImportSourcesReportDto<S>>
  reorderSources: (orderedIds: string[]) => Promise<S[]>
  batchSources: (payload: BatchSourcesPayload) => Promise<BatchSourcesReportDto>
  exportSourcesBatch: (ids: string[]) => Promise<BatchExportPayload>
  updateSource: (id: string, patch: UpdateSourcePatch) => Promise<S>
  deleteSource: (id: string) => Promise<void>
  exportSourceUrl: (id: string) => string
  fetchSourceJson: (id: string) => Promise<Record<string, unknown>>
  fetchSourceRaw: (id: string) => Promise<string>
  getSourceVariable: (id: string) => Promise<{ variable: string }>
  runLoginAction: (
    id: string,
    loginInfo: Record<string, string>,
    action?: string,
  ) => Promise<LoginActionResponseDto<S>>
  runPluginAuth: (
    id: string,
    payload: PluginAuthActionPayload,
  ) => Promise<PluginAuthActionResponseDto<S>>
  updatePluginSettings: (id: string, values: Record<string, unknown>) => Promise<S>
  buildProxyImageUrl: (
    rawUrl: string | null | undefined,
    sourceId?: string | null,
    cacheBust?: string | number | null,
  ) => string
}

export function createDomainSourceApi<S extends OnlineSourceSummaryDto = OnlineSourceSummaryDto>(
  domain: OnlineDomain,
): DomainSourceCrudApi<S> {
  const base = '/plugins'
  const domainQuery = `domain=${encodeURIComponent(domain)}`
  return {
    listSources: (): Promise<S[]> => requestJson(`${base}?${domainQuery}`),

    importSources: (rawJson: string): Promise<ImportSourcesReportDto<S>> =>
      requestJson(`${base}/import?${domainQuery}`, {
        method: 'POST',
        body: JSON.stringify({ domain, raw_json: rawJson }),
      }),

    reorderSources: (orderedIds: string[]): Promise<S[]> =>
      requestJson(`${base}/reorder?${domainQuery}`, {
        method: 'POST',
        body: JSON.stringify({ domain, ordered_ids: orderedIds }),
      }),

    batchSources: (payload: BatchSourcesPayload): Promise<BatchSourcesReportDto> =>
      requestJson(`${base}/batch?${domainQuery}`, {
        method: 'POST',
        body: JSON.stringify({ domain, ...payload }),
      }),

    exportSourcesBatch: async (ids: string[]): Promise<BatchExportPayload> => {
      const raw = await requestText(`${base}/export-batch?${domainQuery}`, {
        method: 'POST',
        body: JSON.stringify({ domain, ids }),
      })
      const trimmed = raw.trim()
      if (trimmed.startsWith('[')) {
        try {
          const parsed = JSON.parse(trimmed) as unknown[]
          return {
            content: JSON.stringify(parsed, null, 2),
            extension: 'json',
            mimeType: 'application/json;charset=utf-8',
            count: Array.isArray(parsed) ? parsed.length : ids.length,
          }
        } catch {
          // fall through to raw download
        }
      }
      return {
        content: raw,
        extension: 'js',
        mimeType: 'application/javascript;charset=utf-8',
        count: ids.length,
      }
    },

    updateSource: (id: string, patch: UpdateSourcePatch): Promise<S> =>
      requestJson(`${base}/${encodeURIComponent(id)}`, {
        method: 'PATCH',
        body: JSON.stringify(patch),
      }),

    deleteSource: (id: string): Promise<void> =>
      requestJson(`${base}/${encodeURIComponent(id)}`, {
        method: 'DELETE',
      }),

    exportSourceUrl: (id: string): string =>
      withAuthToken(`/api/v1${base}/${encodeURIComponent(id)}/export`),

    fetchSourceJson: (id: string): Promise<Record<string, unknown>> =>
      requestJson(`${base}/${encodeURIComponent(id)}/export`),

    fetchSourceRaw: (id: string): Promise<string> =>
      requestText(`${base}/${encodeURIComponent(id)}/export`),

    getSourceVariable: (id: string): Promise<{ variable: string }> =>
      requestJson(`${base}/${encodeURIComponent(id)}/variable`),

    runLoginAction: (
      id: string,
      loginInfo: Record<string, string>,
      action?: string,
    ): Promise<LoginActionResponseDto<S>> =>
      requestJson(`${base}/${encodeURIComponent(id)}/login-action`, {
        method: 'POST',
        body: JSON.stringify({ login_info: loginInfo, action: action ?? null }),
      }),

    runPluginAuth: (
      id: string,
      payload: PluginAuthActionPayload,
    ): Promise<PluginAuthActionResponseDto<S>> =>
      requestJson(`${base}/${encodeURIComponent(id)}/auth`, {
        method: 'POST',
        body: JSON.stringify(payload),
      }),

    updatePluginSettings: (id: string, values: Record<string, unknown>): Promise<S> =>
      requestJson(`${base}/${encodeURIComponent(id)}/settings`, {
        method: 'PUT',
        body: JSON.stringify({ values }),
      }),

    buildProxyImageUrl: (
      rawUrl: string | null | undefined,
      sourceId?: string | null,
      cacheBust?: string | number | null,
    ): string => buildDomainProxyImageUrl(domain, rawUrl, sourceId, cacheBust),
  }
}

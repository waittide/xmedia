import { requestJson, withAuthToken } from '@/api/client'
import type {
  BatchSourcesPayload,
  BatchSourcesReportDto,
  ImportSourcesReportDto,
  LoginActionResponseDto,
  OnlineDomain,
  OnlineSourceSummaryDto,
  UpdateSourcePatch,
} from './types'

export interface DomainSourceCrudApi<S extends OnlineSourceSummaryDto = OnlineSourceSummaryDto> {
  listSources: () => Promise<S[]>
  importSources: (rawJson: string) => Promise<ImportSourcesReportDto<S>>
  reorderSources: (orderedIds: string[]) => Promise<S[]>
  batchSources: (payload: BatchSourcesPayload) => Promise<BatchSourcesReportDto>
  exportSourcesBatch: (ids: string[]) => Promise<Record<string, unknown>[]>
  updateSource: (id: string, patch: UpdateSourcePatch) => Promise<S>
  deleteSource: (id: string) => Promise<void>
  exportSourceUrl: (id: string) => string
  fetchSourceJson: (id: string) => Promise<Record<string, unknown>>
  getSourceVariable: (id: string) => Promise<{ variable: string }>
  runLoginAction: (
    id: string,
    loginInfo: Record<string, string>,
    action?: string,
  ) => Promise<LoginActionResponseDto<S>>
}

export function createDomainSourceApi<S extends OnlineSourceSummaryDto = OnlineSourceSummaryDto>(
  domain: OnlineDomain,
): DomainSourceCrudApi<S> {
  const base = `/${domain}/sources`
  return {
    listSources: (): Promise<S[]> => requestJson(base),

    importSources: (rawJson: string): Promise<ImportSourcesReportDto<S>> =>
      requestJson(`${base}/import`, {
        method: 'POST',
        body: JSON.stringify({ raw_json: rawJson }),
      }),

    reorderSources: (orderedIds: string[]): Promise<S[]> =>
      requestJson(`${base}/reorder`, {
        method: 'POST',
        body: JSON.stringify({ ordered_ids: orderedIds }),
      }),

    batchSources: (payload: BatchSourcesPayload): Promise<BatchSourcesReportDto> =>
      requestJson(`${base}/batch`, {
        method: 'POST',
        body: JSON.stringify(payload),
      }),

    exportSourcesBatch: (ids: string[]): Promise<Record<string, unknown>[]> =>
      requestJson(`${base}/export-batch`, {
        method: 'POST',
        body: JSON.stringify({ ids }),
      }),

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
  }
}

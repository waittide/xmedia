import type { MediaKind } from '@/types/api'

export type OnlineDomain = MediaKind

export interface LoginUiStyle {
  layout_flex_grow?: number | null
  layout_flex_basis_percent?: number | null
}

export interface LoginUiItem {
  name: string
  /** Supported controls: `text` | `password` | `button` | `image` (`img`) | `qrcode` */
  type: string
  action?: string | null
  url?: string | null
  style?: LoginUiStyle | null
}

export interface BrowserRedirectHint {
  url: string
  title: string
}

export interface ExecutionDiagnostics {
  toasts: string[]
  logs: string[]
  browser_urls: BrowserRedirectHint[]
}

export interface OnlineSourceSummaryDto {
  id: string
  name: string
  source_url: string
  group_name: string
  comment: string | null
  check_keyword: string | null
  enabled: boolean
  weight: number
  source_type: number
  has_login_url: boolean
  login_ui: LoginUiItem[]
  login_info: Record<string, string>
  has_variable: boolean
  variable_preview: string
  cookie_hosts: string[]
  last_test_ok: boolean | null
  last_test_msg: string | null
  updated_at: string
}

export interface ImportSourcesReportDto<S = OnlineSourceSummaryDto> {
  imported_count: number
  updated_count: number
  sources: S[]
}

export interface UpdateSourcePatch {
  enabled?: boolean
  weight?: number
  group_name?: string
  raw_json?: string
  variable?: string
}

export type BatchSourcesPayload =
  | { ids: string[]; action: 'enable' | 'disable' | 'delete' }
  | { ids: string[]; action: 'set_group'; group_name: string }

export interface BatchSourcesReportDto {
  affected_count: number
}

export interface LoginActionResponseDto<S = OnlineSourceSummaryDto> {
  result: string
  diagnostics: ExecutionDiagnostics
  source: S
}

export interface SourceDebugStepDto {
  stage: string
  label: string
  ok: boolean
  elapsed_ms: number
  summary: string
  error: string | null
  diagnostics: ExecutionDiagnostics
}

export interface BaseSourceTestReportDto<M extends string = string> {
  ok: boolean
  mode: M
  keyword: string
  total_elapsed_ms: number
  result_count: number
  sample_titles: string[]
  error: string | null
  diagnostics: ExecutionDiagnostics
  steps: SourceDebugStepDto[]
}

export interface OnlineSourceManagerAdapter<
  S extends OnlineSourceSummaryDto = OnlineSourceSummaryDto,
> {
  domain: OnlineDomain
  exportFilePrefix: string
  searchRoutePath: string
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
  buildProxyImageUrl: (
    rawUrl: string | null | undefined,
    sourceId?: string | null,
    cacheBust?: string | number | null,
  ) => string
}

export const SAFE_DATA_IMAGE_RE = /^data:image\/(png|jpeg|jpg|webp|gif|avif);base64,/i

export function isSafeHttpUrl(rawUrl: string | null | undefined): boolean {
  if (!rawUrl || !rawUrl.trim()) return false
  try {
    const parsed = new URL(rawUrl.trim())
    return parsed.protocol === 'http:' || parsed.protocol === 'https:'
  } catch {
    return false
  }
}

export function isSafeDataImageUrl(rawUrl: string | null | undefined): boolean {
  if (!rawUrl || !rawUrl.trim()) return false
  return SAFE_DATA_IMAGE_RE.test(rawUrl.trim())
}

export function extractApiError(err: unknown): string {
  return err instanceof Error ? err.message : String(err)
}

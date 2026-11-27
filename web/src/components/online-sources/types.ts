import type { MediaKind } from '@/types/api'

export type OnlineDomain = MediaKind
export type PluginFormat = 'json_rule' | 'js_module'

export interface PluginAuthCapsDto {
  qr_code: boolean
  otp: boolean
  password: boolean
  cookie: boolean
  has_profile: boolean
  has_logout: boolean
  description?: string | null
  cookie_hint?: string | null
}

export interface PluginSettingOptionDto {
  label: string
  value: string
}

export interface PluginSettingFieldDto {
  key: string
  label: string
  type: 'select' | 'switch' | 'text' | 'password' | string
  default_value?: unknown
  options?: PluginSettingOptionDto[]
  description?: string | null
  placeholder?: string | null
}

export interface PluginProfileDto {
  logged_in: boolean
  user_id?: string | null
  nickname?: string | null
  avatar_url?: string | null
  vip_label?: string | null
  message?: string | null
}

export type PluginAuthActionPayload =
  | { action: 'status' }
  | { action: 'qr_create' }
  | { action: 'qr_poll'; key: string }
  | { action: 'otp_send'; country_code?: string; phone: string }
  | { action: 'otp_verify'; country_code?: string; phone: string; code: string }
  | { action: 'password_login'; username: string; password: string }
  | { action: 'cookie_login'; cookie: string }
  | { action: 'logout' }

export interface PluginAuthActionResponseDto<S = OnlineSourceSummaryDto> {
  ok: boolean
  status?: string | null
  message?: string | null
  qr_key?: string | null
  qr_url?: string | null
  qr_image?: string | null
  cooldown_secs?: number | null
  profile?: PluginProfileDto | null
  diagnostics: ExecutionDiagnostics
  source: S
}

export interface BatchExportPayload {
  content: string
  extension: 'json' | 'js'
  mimeType: string
  count: number
}

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
  domain?: OnlineDomain
  format?: PluginFormat
  version?: string | null
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
  auth?: PluginAuthCapsDto | null
  settings_schema?: PluginSettingFieldDto[]
  settings_values?: Record<string, unknown>
  has_variable: boolean
  variable_preview: string
  cookie_hosts: string[]
  last_test_ok: boolean | null
  last_test_msg: string | null
  updated_at: string
}

export function isContractAuthSupported(auth?: PluginAuthCapsDto | null): boolean {
  if (!auth) return false
  return Boolean(
    auth.qr_code ||
      auth.otp ||
      auth.password ||
      auth.cookie ||
      auth.has_profile ||
      auth.has_logout,
  )
}

export function hasSourceAuthOrSettings(src: OnlineSourceSummaryDto): boolean {
  return Boolean(
    src.has_login_url ||
      src.login_ui.length > 0 ||
      isContractAuthSupported(src.auth) ||
      (src.settings_schema && src.settings_schema.length > 0),
  )
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

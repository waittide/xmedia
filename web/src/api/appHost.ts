import { localizeApiErrorMessage } from '@/i18n/apiErrors'
import { setHostSystemFonts, type HostSystemFontFaceDto } from '@/theme/customFonts'
import type { AuthSessionDto, MediaKind } from '@/types/api'

export type RemoteProtocol = 'opds' | 'subsonic'
export type SubsonicAlbumListType = 'newest' | 'random' | 'alphabeticalByName'

export interface SafeAreaInsetsDto {
  top: number
  right: number
  bottom: number
  left: number
}

export interface StandaloneHealthResponse {
  mode: 'standalone'
  version: string
  remote_source_count: number
  enabled_modules: MediaKind[]
  safe_area?: SafeAreaInsetsDto | null
  system_fonts?: HostSystemFontFaceDto[]
}

export interface StandaloneAppearanceResponse {
  dark: boolean
  safe_area?: SafeAreaInsetsDto | null
  system_fonts?: HostSystemFontFaceDto[]
}

const HOST_SAFE_AREA_STORAGE_KEY = 'xmedia.host_safe_area'

function sanitizeInsetPx(value: unknown): number {
  return typeof value === 'number' && Number.isFinite(value) && value > 0
    ? Math.round(value * 100) / 100
    : 0
}

export function applyHostSafeAreaInsets(raw?: Partial<SafeAreaInsetsDto> | null): void {
  if (!raw || typeof raw !== 'object' || typeof document === 'undefined') return
  const normalized: SafeAreaInsetsDto = {
    top: sanitizeInsetPx(raw.top),
    right: sanitizeInsetPx(raw.right),
    bottom: sanitizeInsetPx(raw.bottom),
    left: sanitizeInsetPx(raw.left),
  }
  const style = document.documentElement.style
  style.setProperty('--host-safe-area-top', `${normalized.top}px`)
  style.setProperty('--host-safe-area-right', `${normalized.right}px`)
  style.setProperty('--host-safe-area-bottom', `${normalized.bottom}px`)
  style.setProperty('--host-safe-area-left', `${normalized.left}px`)
  try {
    sessionStorage.setItem(HOST_SAFE_AREA_STORAGE_KEY, JSON.stringify(normalized))
  } catch {
    // ignore storage errors
  }
}

export interface RemoteSourceSummaryDto {
  id: string
  name: string
  protocol: RemoteProtocol
  url: string
  username: string
  has_password: boolean
  allow_insecure_tls: boolean
  last_ok_at?: number | null
}

export interface RemoteSourcesListResponse {
  sources: RemoteSourceSummaryDto[]
}

export interface RemoteSourceMutationResponse {
  source: RemoteSourceSummaryDto
  sources: RemoteSourceSummaryDto[]
}

export interface CreateRemoteSourcePayload {
  name: string
  protocol: RemoteProtocol
  url: string
  username?: string
  password?: string
  allow_insecure_tls: boolean
}

export interface UpdateRemoteSourcePayload {
  name?: string
  protocol?: RemoteProtocol
  url?: string
  username?: string
  password?: string
  clear_password?: boolean
  allow_insecure_tls?: boolean
}

export interface TestRemoteSourcePayload {
  source_id?: string
  protocol: RemoteProtocol
  url: string
  username?: string
  password?: string
  allow_insecure_tls: boolean
}

export interface TestRemoteSourceResponse {
  ok: boolean
  latency_ms: number
  protocol: string
  detail?: string
  error?: string
}

export interface OpdsNavLinkDto {
  title: string
  href: string
  count?: number | null
}

export interface OpdsDownloadLinkDto {
  href: string
  title?: string | null
  format: string
  media_type: string
}

export interface OpdsEntryDto {
  id: string
  title: string
  authors: string[]
  summary?: string | null
  cover_url?: string | null
  updated?: string | null
  downloads: OpdsDownloadLinkDto[]
  pse_stream_url?: string | null
  page_count?: number | null
}

export interface OpdsFeedDto {
  title: string
  url: string
  opds_version: string
  search_url?: string | null
  prev_url?: string | null
  next_url?: string | null
  nav_links: OpdsNavLinkDto[]
  entries: OpdsEntryDto[]
}

export interface OpdsImportPayload {
  url: string
  filename?: string
}

export interface OpdsImportResponse {
  media_kind: 'book' | 'comic'
  imported_id: string
  title: string
}

export interface SubsonicAlbumDto {
  id: string
  name: string
  artist: string
  year?: number | null
  song_count?: number | null
  duration?: number | null
  cover_url?: string | null
}

export interface SubsonicSongDto {
  id: string
  title: string
  artist: string
  album: string
  track?: number | null
  duration?: number | null
  suffix?: string | null
  bit_rate?: number | null
  size?: number | null
  cover_url?: string | null
  stream_url: string
}

export interface SubsonicAlbumsResponse {
  albums: SubsonicAlbumDto[]
}

export interface SubsonicAlbumDetailResponse {
  album: SubsonicAlbumDto
  songs: SubsonicSongDto[]
}

export interface SubsonicSearchResponse {
  albums: SubsonicAlbumDto[]
  songs: SubsonicSongDto[]
}

export interface SubsonicImportResponse {
  media_kind: 'music'
  imported_id: string
  title: string
}

function isJsonResponse(res: Response): boolean {
  const contentType = res.headers.get('content-type') ?? ''
  return contentType.toLowerCase().includes('application/json')
}

const APP_HOST_TIMEOUT_MS = 15_000

let activeAppServerBaseUrl = ''
let activeBearerToken: string | null = null

export function setActiveAppServerContext(options: {
  baseUrl?: string | null
  token?: string | null
}): void {
  if (options.baseUrl !== undefined) {
    activeAppServerBaseUrl = (options.baseUrl ?? '').trim().replace(/\/+$/, '')
  }
  if (options.token !== undefined) {
    const trimmed = options.token?.trim() ?? ''
    activeBearerToken = trimmed.length > 0 ? trimmed : null
  }
}

export function getActiveAppServerBaseUrl(): string {
  return activeAppServerBaseUrl
}

export function getActiveAuthToken(): string | null {
  return activeBearerToken
}

export function getActiveAuthHeader(): Record<string, string> {
  return activeBearerToken ? { Authorization: `Bearer ${activeBearerToken}` } : {}
}

export function resolveAppApiUrl(path: string): string {
  if (!path) return activeAppServerBaseUrl || ''
  if (/^https?:\/\//i.test(path) || path.startsWith('data:') || path.startsWith('blob:')) {
    return path
  }
  const normalizedPath = path.startsWith('/') ? path : `/${path}`
  return activeAppServerBaseUrl ? `${activeAppServerBaseUrl}${normalizedPath}` : normalizedPath
}

async function fetchWithTimeout(
  input: string,
  init: RequestInit = {},
  timeoutMs = APP_HOST_TIMEOUT_MS,
): Promise<Response> {
  const controller = new AbortController()
  const timer = setTimeout(() => controller.abort(), timeoutMs)
  const upstreamSignal = init.signal
  const onUpstreamAbort = () => controller.abort()
  if (upstreamSignal) {
    if (upstreamSignal.aborted) {
      controller.abort()
    } else {
      upstreamSignal.addEventListener('abort', onUpstreamAbort, { once: true })
    }
  }
  try {
    return await fetch(input, {
      ...init,
      signal: controller.signal,
    })
  } finally {
    clearTimeout(timer)
    upstreamSignal?.removeEventListener('abort', onUpstreamAbort)
  }
}

async function requestAppJson<T>(path: string, init: RequestInit = {}): Promise<T> {
  const headers = new Headers(init.headers)
  headers.set('Accept', 'application/json')
  headers.set('X-XMedia-App', 'standalone')
  if (init.body && !headers.has('Content-Type')) {
    headers.set('Content-Type', 'application/json')
  }

  const res = await fetchWithTimeout(`/_app${path}`, {
    ...init,
    headers,
    cache: 'no-store',
  })

  if (!res.ok) {
    let message = res.statusText || `HTTP ${res.status}`
    if (isJsonResponse(res)) {
      try {
        const data = (await res.json()) as {
          error?: { message?: string } | string
          message?: string
        }
        if (typeof data.error === 'string' && data.error) {
          message = data.error
        } else if (data.error && typeof data.error === 'object' && data.error.message) {
          message = data.error.message
        } else if (typeof data.message === 'string' && data.message) {
          message = data.message
        }
      } catch {
        // ignore invalid error body
      }
    }
    throw new Error(localizeApiErrorMessage(message))
  }

  if (res.status === 204) {
    return undefined as T
  }

  if (!isJsonResponse(res)) {
    throw new Error('Unexpected non-JSON response from standalone host')
  }

  return (await res.json()) as T
}

export async function probeAppHost(): Promise<StandaloneHealthResponse | null> {
  try {
    const res = await fetchWithTimeout(
      '/_app/health',
      {
        method: 'GET',
        headers: { Accept: 'application/json', 'X-XMedia-App': 'standalone' },
        cache: 'no-store',
      },
      5_000,
    )
    if (!res.ok || !isJsonResponse(res)) {
      return null
    }
    const data = (await res.json()) as Partial<StandaloneHealthResponse> | null
    if (!data || typeof data !== 'object' || data.mode !== 'standalone') {
      return null
    }
    if (data.safe_area) {
      applyHostSafeAreaInsets(data.safe_area)
    }
    if (Array.isArray(data.system_fonts) && data.system_fonts.length > 0) {
      setHostSystemFonts(data.system_fonts)
    }
    return {
      mode: 'standalone',
      version: typeof data.version === 'string' ? data.version : '',
      remote_source_count:
        typeof data.remote_source_count === 'number' ? data.remote_source_count : 0,
      enabled_modules: Array.isArray(data.enabled_modules)
        ? data.enabled_modules
        : ['book', 'comic', 'music', 'video'],
      safe_area: data.safe_area ?? null,
      system_fonts: Array.isArray(data.system_fonts) ? data.system_fonts : [],
    }
  } catch {
    return null
  }
}

export const appHostApi = {
  probe: probeAppHost,

  syncAppearance: async (dark: boolean): Promise<StandaloneAppearanceResponse> => {
    const res = await requestAppJson<StandaloneAppearanceResponse>('/appearance', {
      method: 'PUT',
      body: JSON.stringify({ dark }),
    })
    if (res?.safe_area) {
      applyHostSafeAreaInsets(res.safe_area)
    }
    if (Array.isArray(res?.system_fonts) && res.system_fonts.length > 0) {
      setHostSystemFonts(res.system_fonts)
    }
    return res
  },

  setAppIcon: (icon: string) =>
    requestAppJson<{ icon: string }>('/icon', {
      method: 'PUT',
      body: JSON.stringify({ icon }),
    }),

  fetchAutoSession: () => requestAppJson<AuthSessionDto>('/auto-session', { method: 'POST' }),

  listRemoteSources: () =>
    requestAppJson<RemoteSourcesListResponse>('/remote-sources', { method: 'GET' }),

  createRemoteSource: (payload: CreateRemoteSourcePayload) =>
    requestAppJson<RemoteSourceMutationResponse>('/remote-sources', {
      method: 'POST',
      body: JSON.stringify(payload),
    }),

  updateRemoteSource: (id: string, payload: UpdateRemoteSourcePayload) =>
    requestAppJson<RemoteSourceMutationResponse>(
      `/remote-sources/${encodeURIComponent(id)}`,
      {
        method: 'PUT',
        body: JSON.stringify(payload),
      },
    ),

  deleteRemoteSource: (id: string) =>
    requestAppJson<RemoteSourcesListResponse>(
      `/remote-sources/${encodeURIComponent(id)}`,
      { method: 'DELETE' },
    ),

  testRemoteSource: (payload: TestRemoteSourcePayload) =>
    requestAppJson<TestRemoteSourceResponse>('/remote-sources/test', {
      method: 'POST',
      body: JSON.stringify(payload),
    }),

  browseOpds: (id: string, params?: { url?: string; q?: string }) => {
    const query = new URLSearchParams()
    if (params?.url) query.set('url', params.url)
    if (params?.q) query.set('q', params.q)
    const qs = query.toString()
    return requestAppJson<OpdsFeedDto>(
      `/opds/${encodeURIComponent(id)}/browse${qs ? `?${qs}` : ''}`,
      { method: 'GET' },
    )
  },

  importOpdsEntry: (id: string, payload: OpdsImportPayload) =>
    requestAppJson<OpdsImportResponse>(`/opds/${encodeURIComponent(id)}/import`, {
      method: 'POST',
      body: JSON.stringify(payload),
    }),

  listSubsonicAlbums: (
    id: string,
    params?: { type?: SubsonicAlbumListType; size?: number; offset?: number },
  ) => {
    const query = new URLSearchParams()
    query.set('type', params?.type ?? 'newest')
    query.set('size', String(params?.size ?? 30))
    query.set('offset', String(params?.offset ?? 0))
    return requestAppJson<SubsonicAlbumsResponse>(
      `/subsonic/${encodeURIComponent(id)}/albums?${query.toString()}`,
      { method: 'GET' },
    )
  },

  getSubsonicAlbum: (id: string, albumId: string) =>
    requestAppJson<SubsonicAlbumDetailResponse>(
      `/subsonic/${encodeURIComponent(id)}/albums/${encodeURIComponent(albumId)}`,
      { method: 'GET' },
    ),

  searchSubsonic: (id: string, q: string) => {
    const query = new URLSearchParams({ q })
    return requestAppJson<SubsonicSearchResponse>(
      `/subsonic/${encodeURIComponent(id)}/search?${query.toString()}`,
      { method: 'GET' },
    )
  },

  importSubsonicSong: (id: string, songId: string, filename?: string) =>
    requestAppJson<SubsonicImportResponse>(
      `/subsonic/${encodeURIComponent(id)}/import/${encodeURIComponent(songId)}`,
      {
        method: 'POST',
        body: JSON.stringify(filename ? { filename } : {}),
      },
    ),
}

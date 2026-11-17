import type {
  AuthSessionDto,
  CoreLibraryDto,
  MediaKind,
  SystemCapabilitiesDto,
  SystemOverviewDto,
  UserDto,
  UserPreferences,
  UserRole,
} from '@/types/api'

const TOKEN_STORAGE_KEY = 'xmedia.jwt'

let unauthorizedHandler: (() => void) | null = null

export function setUnauthorizedHandler(handler: () => void): void {
  unauthorizedHandler = handler
}

export function getStoredToken(): string | null {
  return localStorage.getItem(TOKEN_STORAGE_KEY)
}

export function setStoredToken(token: string | null): void {
  if (token) {
    localStorage.setItem(TOKEN_STORAGE_KEY, token)
  } else {
    localStorage.removeItem(TOKEN_STORAGE_KEY)
  }
}

export function withAuthToken(rawUrl: string | null | undefined): string {
  if (!rawUrl) return ''
  const token = getStoredToken()
  if (!token) return rawUrl
  const sep = rawUrl.includes('?') ? '&' : '?'
  return `${rawUrl}${sep}token=${encodeURIComponent(token)}`
}

export class ApiRequestError extends Error {
  status: number
  code: string

  constructor(status: number, code: string, message: string) {
    super(message)
    this.status = status
    this.code = code
  }
}

export async function requestJson<T>(
  path: string,
  init: RequestInit = {},
  suppressAuthRedirect = false,
): Promise<T> {
  const headers = new Headers(init.headers)
  const token = getStoredToken()
  if (token && !headers.has('Authorization')) {
    headers.set('Authorization', `Bearer ${token}`)
  }
  if (init.body && !(init.body instanceof FormData) && !headers.has('Content-Type')) {
    headers.set('Content-Type', 'application/json')
  }

  const res = await fetch(`/api/v1${path}`, {
    ...init,
    headers,
    credentials: 'include',
  })

  if (!res.ok) {
    let code = 'http_error'
    let message = res.statusText || `HTTP ${res.status}`
    try {
      const data = (await res.json()) as {
        error?: { code?: string; message?: string } | string
        code?: string
      }
      if (typeof data.error === 'string') {
        message = data.error
      } else if (data.error) {
        if (data.error.code) code = data.error.code
        if (data.error.message) message = data.error.message
      }
      if (data.code) code = data.code
    } catch {
      // ignore non-json body
    }

    if (
      res.status === 401 &&
      !suppressAuthRedirect &&
      path !== '/auth/login' &&
      path !== '/auth/password'
    ) {
      unauthorizedHandler?.()
    }

    throw new ApiRequestError(res.status, code, message)
  }

  if (res.status === 204) {
    return undefined as T
  }

  return (await res.json()) as T
}

export function uploadWithXhr<T>(
  url: string,
  formData: FormData,
  onProgress?: (loaded: number, total: number) => void,
): Promise<T> {
  return new Promise((resolve, reject) => {
    const xhr = new XMLHttpRequest()
    xhr.open('POST', url)
    xhr.withCredentials = true
    const token = getStoredToken()
    if (token) {
      xhr.setRequestHeader('Authorization', `Bearer ${token}`)
    }
    xhr.upload.onprogress = (ev) => {
      if (ev.lengthComputable && onProgress) {
        onProgress(ev.loaded, ev.total)
      }
    }
    xhr.onload = () => {
      if (xhr.status >= 200 && xhr.status < 300) {
        try {
          resolve(JSON.parse(xhr.responseText) as T)
        } catch (err) {
          reject(err instanceof Error ? err : new Error(String(err)))
        }
      } else {
        if (xhr.status === 401) {
          unauthorizedHandler?.()
        }
        let message = `HTTP ${xhr.status}`
        try {
          const parsed = JSON.parse(xhr.responseText) as {
            error?: { message?: string } | string
          }
          if (typeof parsed.error === 'string') message = parsed.error
          else if (parsed.error?.message) message = parsed.error.message
        } catch {
          // ignore
        }
        reject(new Error(message))
      }
    }
    xhr.onerror = () => reject(new Error('Network error during upload'))
    xhr.send(formData)
  })
}

export const api = {
  capabilities: () => requestJson<SystemCapabilitiesDto>('/system/capabilities', {}, true),
  systemOverview: () => requestJson<SystemOverviewDto>('/system/overview'),

  setupStatus: () => requestJson<{ needs_setup: boolean }>('/auth/setup-status', {}, true),
  setupAdmin: (payload: { username: string; display_name?: string; password: string }) =>
    requestJson<AuthSessionDto>('/auth/setup', {
      method: 'POST',
      body: JSON.stringify(payload),
    }),
  login: (payload: { username: string; password: string }) =>
    requestJson<AuthSessionDto>(
      '/auth/login',
      { method: 'POST', body: JSON.stringify(payload) },
      true,
    ),
  logout: () => requestJson<{ ok: boolean }>('/auth/logout', { method: 'POST' }, true),
  me: () => requestJson<UserDto>('/auth/me', {}, true),
  updatePreferences: (prefs: UserPreferences) =>
    requestJson<UserDto>('/auth/preferences', {
      method: 'PUT',
      body: JSON.stringify(prefs),
    }),
  changePassword: (payload: { current_password: string; new_password: string }) =>
    requestJson<UserDto>(
      '/auth/password',
      { method: 'POST', body: JSON.stringify(payload) },
      true,
    ),
  rotateApiKey: () => requestJson<UserDto>('/auth/rotate-api-key', { method: 'POST' }),

  listUsers: () => requestJson<UserDto[]>('/users'),
  createUser: (payload: {
    username: string
    display_name?: string
    password: string
    role: UserRole
  }) => requestJson<UserDto>('/users', { method: 'POST', body: JSON.stringify(payload) }),
  updateUser: (
    id: string,
    payload: { display_name?: string; role?: UserRole; is_active?: boolean; password?: string },
  ) =>
    requestJson<UserDto>(`/users/${encodeURIComponent(id)}`, {
      method: 'PATCH',
      body: JSON.stringify(payload),
    }),
  deleteUser: (id: string) =>
    requestJson<{ ok: boolean }>(`/users/${encodeURIComponent(id)}`, { method: 'DELETE' }),

  listLibraries: (mediaKind?: MediaKind) =>
    requestJson<CoreLibraryDto[]>(
      mediaKind ? `/libraries?media_kind=${mediaKind}` : '/libraries',
    ),
  createLibrary: (payload: { name: string; media_kind: MediaKind; root_path: string }) =>
    requestJson<CoreLibraryDto>('/libraries', {
      method: 'POST',
      body: JSON.stringify(payload),
    }),
  renameLibrary: (id: string, name: string) =>
    requestJson<CoreLibraryDto>(`/libraries/${encodeURIComponent(id)}`, {
      method: 'PATCH',
      body: JSON.stringify({ name }),
    }),
  scanLibrary: (id: string) =>
    requestJson<CoreLibraryDto>(`/libraries/${encodeURIComponent(id)}/scan`, {
      method: 'POST',
    }),
  deleteLibrary: (id: string) =>
    requestJson<{ ok: boolean }>(`/libraries/${encodeURIComponent(id)}`, {
      method: 'DELETE',
    }),
}

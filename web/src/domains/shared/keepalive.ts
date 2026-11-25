import * as appHostModule from '@/api/appHost'
import { withAuthToken } from '@/api/client'

type AuthHeaderCandidate =
  | HeadersInit
  | Record<string, string | undefined>
  | string
  | null
  | undefined

interface AppHostKeepaliveBridge {
  appHostApi: typeof appHostModule.appHostApi
  resolveAppApiUrl?: (path: string) => string
  getActiveAuthHeader?: () => AuthHeaderCandidate
}

const appHostBridge: AppHostKeepaliveBridge = appHostModule

function mergeAuthHeaders(headers: Headers, authHeader: AuthHeaderCandidate): void {
  if (!authHeader) return
  if (typeof authHeader === 'string') {
    const trimmed = authHeader.trim()
    if (trimmed) {
      headers.set('Authorization', trimmed.startsWith('Bearer ') ? trimmed : `Bearer ${trimmed}`)
    }
    return
  }
  if (authHeader instanceof Headers) {
    authHeader.forEach((value, key) => {
      headers.set(key, value)
    })
    return
  }
  if (Array.isArray(authHeader)) {
    for (const entry of authHeader) {
      if (entry.length >= 2 && entry[0] && entry[1]) {
        headers.set(entry[0], entry[1])
      }
    }
    return
  }
  for (const [key, value] of Object.entries(authHeader)) {
    if (typeof value === 'string' && value) {
      headers.set(key, value)
    }
  }
}

export function sendKeepalivePutJson(apiPath: string, payload: unknown): void {
  try {
    const url = appHostBridge.resolveAppApiUrl
      ? appHostBridge.resolveAppApiUrl(apiPath)
      : withAuthToken(apiPath)
    const headers = new Headers({ 'Content-Type': 'application/json' })
    mergeAuthHeaders(headers, appHostBridge.getActiveAuthHeader?.())

    void fetch(url, {
      method: 'PUT',
      credentials: 'include',
      headers,
      body: JSON.stringify(payload),
      keepalive: true,
    })
  } catch {
    // ignore unload errors
  }
}

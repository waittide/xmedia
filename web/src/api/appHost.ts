export interface ServerProfile {
  id: string
  name: string
  url: string
  allow_insecure_tls: boolean
  last_connected_at?: number | null
}

export interface AppHealthResponse {
  mode: 'tauri'
  version: string
  active_server: ServerProfile | null
  has_servers: boolean
}

export interface AppServersListResponse {
  active_server_id: string | null
  servers: ServerProfile[]
}

export interface AppServerMutationResponse {
  active_server_id: string | null
  server: ServerProfile
  servers: ServerProfile[]
}

export interface CreateServerPayload {
  name: string
  url: string
  allow_insecure_tls: boolean
  activate?: boolean
}

export interface TestServerPayload {
  url: string
  allow_insecure_tls: boolean
}

export interface TestServerResponse {
  ok: boolean
  latency_ms: number
  normalized_url: string
  needs_setup?: boolean
  version?: string
  error?: string
}

function isJsonResponse(res: Response): boolean {
  const contentType = res.headers.get('content-type') ?? ''
  return contentType.toLowerCase().includes('application/json')
}

async function requestAppJson<T>(path: string, init: RequestInit = {}): Promise<T> {
  const headers = new Headers(init.headers)
  headers.set('Accept', 'application/json')
  if (init.body && !headers.has('Content-Type')) {
    headers.set('Content-Type', 'application/json')
  }

  const res = await fetch(`/_app${path}`, {
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
    throw new Error(message)
  }

  if (!isJsonResponse(res)) {
    throw new Error('Unexpected non-JSON response from client host')
  }

  return (await res.json()) as T
}

export async function probeAppHost(): Promise<AppHealthResponse | null> {
  try {
    const res = await fetch('/_app/health', {
      method: 'GET',
      headers: { Accept: 'application/json' },
      cache: 'no-store',
    })
    if (!res.ok || !isJsonResponse(res)) {
      return null
    }
    const data = (await res.json()) as Partial<AppHealthResponse> | null
    if (!data || typeof data !== 'object' || data.mode !== 'tauri') {
      return null
    }
    return {
      mode: 'tauri',
      version: typeof data.version === 'string' ? data.version : '',
      active_server: data.active_server ?? null,
      has_servers: Boolean(data.has_servers),
    }
  } catch {
    return null
  }
}

export const appHostApi = {
  probe: probeAppHost,

  listServers: () => requestAppJson<AppServersListResponse>('/servers', { method: 'GET' }),

  addServer: (payload: CreateServerPayload) =>
    requestAppJson<AppServerMutationResponse>('/servers', {
      method: 'POST',
      body: JSON.stringify(payload),
    }),

  activateServer: (id: string) =>
    requestAppJson<AppServerMutationResponse>(
      `/servers/${encodeURIComponent(id)}/activate`,
      { method: 'PUT' },
    ),

  deleteServer: (id: string) =>
    requestAppJson<AppServersListResponse>(`/servers/${encodeURIComponent(id)}`, {
      method: 'DELETE',
    }),

  testServer: (payload: TestServerPayload) =>
    requestAppJson<TestServerResponse>('/servers/test', {
      method: 'POST',
      body: JSON.stringify(payload),
    }),
}

export interface StreamProgressPayload {
  processed: number
  total: number | null
  detail?: string
}

export type StreamProgressCallback = (progress: StreamProgressPayload) => void

type NdjsonStreamFrame<T> =
  | { type: 'progress'; processed: number; total: number | null; detail?: string }
  | { type: 'done'; data: T }
  | { type: 'error'; message: string }

export interface ToolRequestError extends Error {
  status?: number
  streamRouteMissing?: boolean
}

function buildToolHeaders(init: RequestInit): Headers {
  const headers = new Headers(init.headers)
  if (init.body && !(init.body instanceof FormData) && !headers.has('Content-Type')) {
    headers.set('Content-Type', 'application/json')
  }
  return headers
}

async function fetchToolResponse(path: string, init: RequestInit = {}): Promise<Response> {
  return fetch(`/api/v1/tool${path}`, {
    ...init,
    headers: buildToolHeaders(init),
    credentials: 'include',
  })
}

function createToolHttpError(
  path: string,
  status: number,
  rawText: string,
  parsed: unknown,
  checkStreamRouteMissing = false,
): ToolRequestError {
  if (status === 401 && path !== '/auth/unlock') {
    window.dispatchEvent(new CustomEvent('xmedia-tool:unauthorized'))
  }
  let errMsg = `HTTP ${status}`
  if (
    typeof parsed === 'object' &&
    parsed !== null &&
    'error' in parsed &&
    typeof (parsed as { error?: { message?: string } }).error?.message === 'string'
  ) {
    errMsg = (parsed as { error: { message: string } }).error.message
  } else if (checkStreamRouteMissing && rawText.trim()) {
    errMsg = rawText.trim()
  }
  const error = new Error(errMsg) as ToolRequestError
  error.status = status
  if (checkStreamRouteMissing && status === 404 && rawText.trim() === 'Not Found') {
    error.streamRouteMissing = true
  }
  return error
}

function tryParseJson(text: string): unknown {
  if (!text) return null
  try {
    return JSON.parse(text)
  } catch {
    return text
  }
}

export async function toolRequest<T>(path: string, init: RequestInit = {}): Promise<T> {
  const res = await fetchToolResponse(path, init)
  if (res.status === 204) {
    return undefined as T
  }

  const text = await res.text()
  const parsed = tryParseJson(text)

  if (!res.ok) {
    throw createToolHttpError(path, res.status, text, parsed)
  }

  return parsed as T
}

export async function toolStreamRequest<T>(
  path: string,
  init: RequestInit,
  onProgress: StreamProgressCallback,
): Promise<T> {
  const res = await fetchToolResponse(path, init)

  if (!res.ok) {
    const text = await res.text()
    const parsed = tryParseJson(text)
    throw createToolHttpError(path, res.status, text, parsed, true)
  }

  if (!res.body) {
    throw new Error('Streaming response body unavailable')
  }

  const reader = res.body.getReader()
  const decoder = new TextDecoder()
  let buffer = ''
  let finalData: T | undefined
  let receivedDone = false

  function handleLine(rawLine: string): void {
    const trimmed = rawLine.trim()
    if (!trimmed) return
    const frame = JSON.parse(trimmed) as NdjsonStreamFrame<T>
    if (frame.type === 'progress') {
      onProgress({
        processed: frame.processed,
        total: frame.total ?? null,
        detail: frame.detail,
      })
    } else if (frame.type === 'done') {
      finalData = frame.data
      receivedDone = true
    } else if (frame.type === 'error') {
      const error = new Error(frame.message) as Error & { status?: number }
      if (/not found|不存在|not a directory/i.test(frame.message)) {
        error.status = 404
      }
      throw error
    }
  }

  try {
    while (true) {
      const { value, done } = await reader.read()
      if (done) break
      buffer += decoder.decode(value, { stream: true })
      let nl = buffer.indexOf('\n')
      while (nl >= 0) {
        const line = buffer.slice(0, nl)
        buffer = buffer.slice(nl + 1)
        handleLine(line)
        nl = buffer.indexOf('\n')
      }
    }
    buffer += decoder.decode()
    if (buffer.trim()) {
      handleLine(buffer)
    }

    if (!receivedDone) {
      throw new Error('Stream ended before completion')
    }
    return finalData as T
  } finally {
    void reader.cancel().catch(() => {})
  }
}

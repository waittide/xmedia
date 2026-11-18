import type { MediaKind } from '@/types/api'
import type {
  AggregateScrapeResponse,
  AutoScrapeResultDto,
  BookContentDocumentDto,
  ConvertTxtToEpubResponseDto,
  DirectorySnapshot,
  EditableBookChapterDto,
  ManagedRootDto,
  MediaFileDetail,
  MediaMetadataPatchEnvelope,
  ProviderStatusDto,
  PublicSettingsDto,
  RenamePlanItem,
  SaveBookChapterInput,
  ScrapeCacheStatsDto,
  ScrapeCandidate,
  ScrapeQuery,
  ScrapeTargetKind,
  ScrapedMetadata,
  ToolStatusDto,
  UpdateToolSettingsInput,
} from './types'

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

const TOKEN_STORAGE_KEY = 'xmedia_tool.token'

export function getToolToken(): string | null {
  return localStorage.getItem(TOKEN_STORAGE_KEY)
}

export function setToolToken(token: string | null): void {
  if (token) {
    localStorage.setItem(TOKEN_STORAGE_KEY, token)
  } else {
    localStorage.removeItem(TOKEN_STORAGE_KEY)
  }
}

async function toolRequest<T>(path: string, init: RequestInit = {}): Promise<T> {
  const headers = new Headers(init.headers)
  const token = getToolToken()
  if (token && !headers.has('Authorization')) {
    headers.set('Authorization', `Bearer ${token}`)
  }
  if (init.body && !(init.body instanceof FormData) && !headers.has('Content-Type')) {
    headers.set('Content-Type', 'application/json')
  }

  const res = await fetch(`/api/v1/tool${path}`, {
    ...init,
    headers,
    credentials: 'same-origin',
  })

  if (res.status === 204) {
    return undefined as T
  }

  const text = await res.text()
  let parsed: unknown = null
  if (text) {
    try {
      parsed = JSON.parse(text)
    } catch {
      parsed = text
    }
  }

  if (!res.ok) {
    if (res.status === 401 && path !== '/auth/unlock') {
      setToolToken(null)
      window.dispatchEvent(new CustomEvent('xmedia-tool:unauthorized'))
    }
    const errMsg =
      typeof parsed === 'object' &&
      parsed !== null &&
      'error' in parsed &&
      typeof (parsed as { error?: { message?: string } }).error?.message === 'string'
        ? (parsed as { error: { message: string } }).error.message
        : `HTTP ${res.status}`
    const error = new Error(errMsg) as Error & { status?: number }
    error.status = res.status
    throw error
  }

  return parsed as T
}

async function toolStreamRequest<T>(
  path: string,
  init: RequestInit,
  onProgress: StreamProgressCallback,
): Promise<T> {
  const headers = new Headers(init.headers)
  const token = getToolToken()
  if (token && !headers.has('Authorization')) {
    headers.set('Authorization', `Bearer ${token}`)
  }
  if (init.body && !(init.body instanceof FormData) && !headers.has('Content-Type')) {
    headers.set('Content-Type', 'application/json')
  }

  const res = await fetch(`/api/v1/tool${path}`, {
    ...init,
    headers,
    credentials: 'same-origin',
  })

  if (!res.ok) {
    if (res.status === 401) {
      setToolToken(null)
      window.dispatchEvent(new CustomEvent('xmedia-tool:unauthorized'))
    }
    const text = await res.text()
    let errMsg = `HTTP ${res.status}`
    try {
      const parsed = JSON.parse(text) as { error?: { message?: string } }
      if (parsed?.error?.message) errMsg = parsed.error.message
    } catch {
      if (text.trim()) errMsg = text.trim()
    }
    const error = new Error(errMsg) as Error & {
      status?: number
      streamRouteMissing?: boolean
    }
    error.status = res.status
    if (res.status === 404 && text.trim() === 'Not Found') {
      error.streamRouteMissing = true
    }
    throw error
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
}

export function buildArtworkUrl(
  rootId: string,
  relPath: string,
  kind: MediaKind,
  cacheBust = 0,
  slot?: string,
): string {
  const params = new URLSearchParams({
    root_id: rootId,
    path: relPath,
    kind,
    v: String(cacheBust),
  })
  if (slot) {
    params.set('slot', slot)
  }
  const tok = getToolToken()
  if (tok) {
    params.set('token', tok)
  }
  return `/api/v1/tool/files/artwork?${params.toString()}`
}

export function buildEpubAssetUrl(rootId: string, relPath: string, assetPath: string): string {
  const params = new URLSearchParams({
    root_id: rootId,
    path: relPath,
    asset: assetPath,
  })
  const tok = getToolToken()
  if (tok) {
    params.set('token', tok)
  }
  return `/api/v1/tool/books/epub-asset?${params.toString()}`
}

export const toolApi = {
  getStatus(): Promise<ToolStatusDto> {
    return toolRequest<ToolStatusDto>('/status')
  },

  async unlock(password: string): Promise<string> {
    const res = await toolRequest<{ token: string }>('/auth/unlock', {
      method: 'POST',
      body: JSON.stringify({ password }),
    })
    setToolToken(res.token)
    return res.token
  },

  getSettings(): Promise<PublicSettingsDto> {
    return toolRequest<PublicSettingsDto>('/settings')
  },

  updateSettings(input: UpdateToolSettingsInput): Promise<PublicSettingsDto> {
    return toolRequest<PublicSettingsDto>('/settings', {
      method: 'PATCH',
      body: JSON.stringify(input),
    })
  },

  addRoot(name: string, path: string, defaultKind: MediaKind | null): Promise<ManagedRootDto> {
    return toolRequest<ManagedRootDto>('/roots', {
      method: 'POST',
      body: JSON.stringify({ name, path, default_kind: defaultKind }),
    })
  },

  removeRoot(id: string): Promise<void> {
    return toolRequest<void>(`/roots/${encodeURIComponent(id)}`, {
      method: 'DELETE',
    })
  },

  browse(
    rootId: string,
    relPath: string,
    kind: MediaKind | null,
    recursive = true,
    onProgress?: StreamProgressCallback,
  ): Promise<DirectorySnapshot> {
    const q = new URLSearchParams({
      root_id: rootId,
      path: relPath,
      recursive: recursive ? 'true' : 'false',
    })
    if (kind) q.set('kind', kind)
    if (onProgress) {
      return toolStreamRequest<DirectorySnapshot>(
        `/fs/browse/stream?${q.toString()}`,
        { method: 'GET' },
        onProgress,
      ).catch((err: Error & { streamRouteMissing?: boolean }) => {
        if (err?.streamRouteMissing) {
          return toolRequest<DirectorySnapshot>(`/fs/browse?${q.toString()}`)
        }
        throw err
      })
    }
    return toolRequest<DirectorySnapshot>(`/fs/browse?${q.toString()}`)
  },

  inspectFile(rootId: string, relPath: string, kind: MediaKind | null): Promise<MediaFileDetail> {
    const q = new URLSearchParams({ root_id: rootId, path: relPath })
    if (kind) q.set('kind', kind)
    return toolRequest<MediaFileDetail>(`/files/inspect?${q.toString()}`)
  },

  writeMetadata(
    rootId: string,
    relPath: string,
    envelope: MediaMetadataPatchEnvelope,
  ): Promise<MediaFileDetail> {
    return toolRequest<MediaFileDetail>('/files/metadata', {
      method: 'PATCH',
      body: JSON.stringify({
        root_id: rootId,
        path: relPath,
        ...envelope,
      }),
    })
  },

  uploadArtwork(
    rootId: string,
    relPath: string,
    file: File,
    slot: 'cover' | 'series_cover' | 'album_cover' | 'poster' | 'fanart' | 'thumb',
    kind: MediaKind,
  ): Promise<MediaFileDetail> {
    const fd = new FormData()
    fd.append('root_id', rootId)
    fd.append('path', relPath)
    fd.append('slot', slot)
    fd.append('kind', kind)
    fd.append('file', file)
    return toolRequest<MediaFileDetail>('/files/artwork', {
      method: 'POST',
      body: fd,
    })
  },

  previewRename(
    rootId: string,
    paths: string[],
    template: string | null,
    kind: MediaKind | null,
    episodeTemplate?: string | null,
    onProgress?: StreamProgressCallback,
  ): Promise<RenamePlanItem[]> {
    const body = JSON.stringify({
      root_id: rootId,
      paths,
      template: template || undefined,
      episode_template: episodeTemplate || undefined,
      kind: kind || undefined,
    })
    if (onProgress) {
      return toolStreamRequest<RenamePlanItem[]>(
        '/files/rename/preview/stream',
        { method: 'POST', body },
        onProgress,
      ).catch((err: Error & { streamRouteMissing?: boolean }) => {
        if (err?.streamRouteMissing) {
          return toolRequest<RenamePlanItem[]>('/files/rename/preview', {
            method: 'POST',
            body,
          })
        }
        throw err
      })
    }
    return toolRequest<RenamePlanItem[]>('/files/rename/preview', {
      method: 'POST',
      body,
    })
  },

  executeRename(
    rootId: string,
    plans: RenamePlanItem[],
    onProgress?: StreamProgressCallback,
  ): Promise<{ moved_count: number }> {
    const body = JSON.stringify({
      root_id: rootId,
      plans,
    })
    if (onProgress) {
      return toolStreamRequest<{ moved_count: number }>(
        '/files/rename/execute/stream',
        { method: 'POST', body },
        onProgress,
      ).catch((err: Error & { streamRouteMissing?: boolean }) => {
        if (err?.streamRouteMissing) {
          return toolRequest<{ moved_count: number }>('/files/rename/execute', {
            method: 'POST',
            body,
          })
        }
        throw err
      })
    }
    return toolRequest<{ moved_count: number }>('/files/rename/execute', {
      method: 'POST',
      body,
    })
  },

  listProviders(target?: ScrapeTargetKind, domain?: MediaKind): Promise<ProviderStatusDto[]> {
    const params = new URLSearchParams()
    if (target) params.set('target', target)
    if (domain) params.set('domain', domain)
    const qs = params.toString()
    return toolRequest<ProviderStatusDto[]>(`/scrapers/providers${qs ? `?${qs}` : ''}`)
  },

  searchScrapers(query: ScrapeQuery): Promise<ScrapeCandidate[]> {
    return toolRequest<ScrapeCandidate[]>('/scrapers/search', {
      method: 'POST',
      body: JSON.stringify(query),
    })
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

  applyScrapedMetadata(
    rootId: string,
    relPath: string,
    mediaKind: MediaKind,
    metadata: ScrapedMetadata,
    applyCover: boolean,
    writeSidecarLrc: boolean,
    organizeByTemplate?: boolean,
    onlyFillMissing = false,
    writeAlbumCover = false,
  ): Promise<MediaFileDetail> {
    return toolRequest<MediaFileDetail>('/scrapers/apply', {
      method: 'POST',
      body: JSON.stringify({
        root_id: rootId,
        path: relPath,
        media_kind: mediaKind,
        metadata,
        apply_cover: applyCover,
        write_sidecar_lrc: writeSidecarLrc,
        organize_by_template: organizeByTemplate,
        only_fill_missing: onlyFillMissing,
        write_album_cover: writeAlbumCover,
      }),
    })
  },

  autoScrapeFile(
    rootId: string,
    relPath: string,
    mediaKind: MediaKind,
  ): Promise<AutoScrapeResultDto> {
    return toolRequest<AutoScrapeResultDto>('/scrapers/auto-apply', {
      method: 'POST',
      body: JSON.stringify({
        root_id: rootId,
        path: relPath,
        media_kind: mediaKind,
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

  getBookContent(rootId: string, relPath: string): Promise<BookContentDocumentDto> {
    const q = new URLSearchParams({ root_id: rootId, path: relPath })
    return toolRequest<BookContentDocumentDto>(`/books/content?${q.toString()}`)
  },

  resegmentTxtBook(
    rootId: string,
    relPath: string,
    chapterPattern?: string,
    volumePattern?: string,
  ): Promise<EditableBookChapterDto[]> {
    return toolRequest<EditableBookChapterDto[]>('/books/resegment-txt', {
      method: 'POST',
      body: JSON.stringify({
        root_id: rootId,
        path: relPath,
        chapter_pattern: chapterPattern || undefined,
        volume_pattern: volumePattern || undefined,
      }),
    })
  },

  saveBookContent(
    rootId: string,
    relPath: string,
    chapters: SaveBookChapterInput[],
  ): Promise<MediaFileDetail> {
    return toolRequest<MediaFileDetail>('/books/content', {
      method: 'PUT',
      body: JSON.stringify({
        root_id: rootId,
        path: relPath,
        chapters,
      }),
    })
  },

  convertTxtToEpub(
    rootId: string,
    relPath: string,
    chapters: SaveBookChapterInput[] | undefined,
    deleteSourceTxt: boolean,
  ): Promise<ConvertTxtToEpubResponseDto> {
    return toolRequest<ConvertTxtToEpubResponseDto>('/books/convert-epub', {
      method: 'POST',
      body: JSON.stringify({
        root_id: rootId,
        path: relPath,
        chapters,
        delete_source_txt: deleteSourceTxt,
      }),
    })
  },
}

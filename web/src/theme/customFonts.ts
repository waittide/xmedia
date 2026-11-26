import { ref, watch } from 'vue'
import {
  getActiveAppServerBaseUrl,
  getActiveAuthHeader,
  getActiveAuthToken,
  resolveAppApiUrl,
} from '@/api/appHost'
import type { CustomFontDto } from '@/types/api'

export const DEFAULT_SANS_FONT_STACK =
  'system-ui, -apple-system, "Segoe UI", Roboto, "Noto Sans SC", "PingFang SC", "Microsoft YaHei", "Helvetica Neue", Arial, sans-serif'

export const DEFAULT_SERIF_FONT_STACK =
  '"Noto Serif SC", "Noto Serif CJK SC", "Source Han Serif SC", "Songti SC", "STSong", "SimSun", Georgia, serif'

export const DEFAULT_MONO_FONT_STACK =
  '"JetBrains Mono", "Sarasa Mono SC", "Cascadia Mono", ui-monospace, SFMono-Regular, Menlo, Consolas, monospace'

const CUSTOM_FONT_FALLBACK_TAIL = '"Noto Sans SC", "PingFang SC", system-ui, sans-serif'
const CUSTOM_FONTS_STYLE_ID = 'xmedia-custom-fonts'
const TOKEN_STORAGE_KEY = 'xmedia.token'

let fontsApiBase = '/api/v1/fonts'
let loadedOnce = false
let loadInFlight: Promise<CustomFontDto[]> | null = null
const deletedListeners = new Set<(fontId: string) => void>()

export const uploadedFonts = ref<CustomFontDto[]>([])
export const loadingFonts = ref(false)
export const uploadingFonts = ref(false)
export const localSystemFonts = ref<string[]>([])
export const loadingLocalFonts = ref(false)
export const supportsLocalFontAccess =
  typeof window !== 'undefined' && 'queryLocalFonts' in window

export function configureCustomFontsApiBase(base: string): void {
  const normalized = base.trim().replace(/\/+$/, '') || '/api/v1/fonts'
  if (normalized !== fontsApiBase) {
    fontsApiBase = normalized
    loadedOnce = false
  }
}

export function getCustomFontsApiBase(): string {
  return fontsApiBase
}

export function onCustomFontDeleted(listener: (fontId: string) => void): () => void {
  deletedListeners.add(listener)
  return () => {
    deletedListeners.delete(listener)
  }
}

export function sanitizeFontFamilyName(raw: string): string {
  return raw
    .replace(/[\u0000-\u001F\u007F;{}<>\\"']/g, '')
    .trim()
    .slice(0, 128)
}

export function normalizeStoredUiFont(raw: string | null | undefined): string | null {
  if (!raw) return null
  const cleaned = sanitizeFontFamilyName(raw)
  if (!cleaned) return null
  const lower = cleaned.toLowerCase()
  if (lower === 'system-ui' || lower === 'inherit') return null
  return cleaned
}

function sanitizeFontId(rawId: string): string {
  return rawId.replace(/[^a-zA-Z0-9_-]/g, '').slice(0, 96)
}

export function resolveFontStack(
  spec: string | null | undefined,
  uiFontSpec?: string | null,
): string {
  // Touch uploadedFonts so Vue computed callers re-evaluate when custom fonts hydrate.
  const currentUploaded = uploadedFonts.value

  const trimmed = spec ? spec.trim() : ''
  if (trimmed === 'inherit') {
    const nextUi = uiFontSpec && uiFontSpec.trim() !== 'inherit' ? uiFontSpec : null
    return resolveFontStack(nextUi, null)
  }

  if (!trimmed || trimmed.toLowerCase() === 'system-ui') {
    return DEFAULT_SANS_FONT_STACK
  }
  if (trimmed === 'serif') {
    return DEFAULT_SERIF_FONT_STACK
  }
  if (trimmed === 'mono') {
    return DEFAULT_MONO_FONT_STACK
  }

  if (trimmed.startsWith('font:')) {
    const requestedId = sanitizeFontFamilyName(trimmed.slice(5))
    const matched = currentUploaded.find((f) => f.id === requestedId)
    const safeId = sanitizeFontId(matched ? matched.id : requestedId)
    if (!safeId) return DEFAULT_SANS_FONT_STACK
    return `"xmedia-font-${safeId}", ${CUSTOM_FONT_FALLBACK_TAIL}`
  }

  const sanitized = sanitizeFontFamilyName(trimmed)
  if (!sanitized) return DEFAULT_SANS_FONT_STACK
  return `"${sanitized}", ${CUSTOM_FONT_FALLBACK_TAIL}`
}

function resolveFontsEndpoint(suffix = ''): string {
  const path = `${fontsApiBase}${suffix}`
  return fontsApiBase.startsWith('/api/v1') ? resolveAppApiUrl(path) : path
}

function readStoredBearerToken(): string | null {
  try {
    const raw = localStorage.getItem(TOKEN_STORAGE_KEY)
    const trimmed = raw?.trim() ?? ''
    return trimmed.length > 0 ? trimmed : null
  } catch {
    return null
  }
}

function buildFontsHeaders(extra?: HeadersInit): Headers {
  const headers = new Headers(extra)
  if (fontsApiBase.startsWith('/api/v1') && !headers.has('Authorization')) {
    const activeAuth = getActiveAuthHeader()
    if (activeAuth.Authorization) {
      headers.set('Authorization', activeAuth.Authorization)
    } else {
      const storedToken = readStoredBearerToken()
      if (storedToken) {
        headers.set('Authorization', `Bearer ${storedToken}`)
      }
    }
  }
  return headers
}

export function buildCustomFontFileUrl(fontId: string): string {
  const rawPath = `${fontsApiBase}/${encodeURIComponent(fontId)}/file`
  const resolvedPath = fontsApiBase.startsWith('/api/v1') ? resolveAppApiUrl(rawPath) : rawPath
  const baseOrigin =
    typeof window !== 'undefined' && window.location.origin && window.location.origin !== 'null'
      ? window.location.origin
      : 'http://localhost'
  let href = new URL(resolvedPath, baseOrigin).href
  if (fontsApiBase.startsWith('/api/v1')) {
    const token = getActiveAuthToken()
    if (token && getActiveAppServerBaseUrl() && !/[?&]token=/.test(href)) {
      const sep = href.includes('?') ? '&' : '?'
      href = `${href}${sep}token=${encodeURIComponent(token)}`
    }
  }
  return href
}

export function buildUploadedFontFaceCss(
  fonts: readonly CustomFontDto[] = uploadedFonts.value,
): string {
  const rules: string[] = []
  for (const font of fonts) {
    const safeId = sanitizeFontId(font.id)
    if (!safeId) continue
    const cssFormat = sanitizeFontFamilyName(font.css_format || '') || 'truetype'
    const safeUrl = buildCustomFontFileUrl(font.id).replace(/["\\]/g, '')
    rules.push(
      `@font-face {\n  font-family: "xmedia-font-${safeId}";\n  src: url("${safeUrl}") format("${cssFormat}");\n  font-display: swap;\n}`,
    )
  }
  return rules.join('\n')
}

export function syncDocumentCustomFontStyles(): void {
  if (typeof document === 'undefined' || !document.head) return
  const css = buildUploadedFontFaceCss(uploadedFonts.value)
  let styleEl = document.getElementById(CUSTOM_FONTS_STYLE_ID) as HTMLStyleElement | null
  if (!css) {
    styleEl?.remove()
    return
  }
  if (!styleEl) {
    styleEl = document.createElement('style')
    styleEl.id = CUSTOM_FONTS_STYLE_ID
    document.head.appendChild(styleEl)
  }
  if (styleEl.textContent !== css) {
    styleEl.textContent = css
  }
}

watch(
  uploadedFonts,
  () => {
    syncDocumentCustomFontStyles()
  },
  { deep: true, immediate: true },
)

async function extractErrorMessage(res: Response): Promise<string> {
  let message = res.statusText || `HTTP ${res.status}`
  try {
    const contentType = res.headers.get('content-type') ?? ''
    if (contentType.toLowerCase().includes('application/json')) {
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
    } else {
      const text = (await res.text()).trim()
      if (text) message = text
    }
  } catch {
    // ignore invalid error body
  }
  return message
}

export async function refreshCustomFonts(): Promise<CustomFontDto[]> {
  if (loadInFlight) return loadInFlight
  loadingFonts.value = true
  let currentTask: Promise<CustomFontDto[]> | null = null
  currentTask = (async () => {
    try {
      const res = await fetch(resolveFontsEndpoint(''), {
        method: 'GET',
        headers: buildFontsHeaders({ Accept: 'application/json' }),
        credentials: 'include',
      })
      if (!res.ok) {
        throw new Error(await extractErrorMessage(res))
      }
      const data = (await res.json()) as unknown
      const list = Array.isArray(data) ? (data as CustomFontDto[]) : []
      uploadedFonts.value = list
      loadedOnce = true
      return list
    } finally {
      loadingFonts.value = false
      if (loadInFlight === currentTask) {
        loadInFlight = null
      }
    }
  })()
  loadInFlight = currentTask
  return currentTask
}

export async function ensureCustomFontsLoaded(force = false): Promise<CustomFontDto[]> {
  if (loadedOnce && !force) return uploadedFonts.value
  try {
    return await refreshCustomFonts()
  } catch {
    return uploadedFonts.value
  }
}

export async function uploadCustomFontFiles(
  files: FileList | readonly File[],
): Promise<CustomFontDto[]> {
  const list = Array.from(files)
  if (list.length === 0) return []
  const form = new FormData()
  for (const file of list) {
    form.append('file', file, file.name)
  }

  uploadingFonts.value = true
  try {
    const res = await fetch(resolveFontsEndpoint(''), {
      method: 'POST',
      headers: buildFontsHeaders({ Accept: 'application/json' }),
      body: form,
      credentials: 'include',
    })
    if (!res.ok) {
      throw new Error(await extractErrorMessage(res))
    }
    const createdRaw = (await res.json()) as unknown
    const created = Array.isArray(createdRaw) ? (createdRaw as CustomFontDto[]) : []
    if (created.length > 0) {
      const createdIds = new Set(created.map((f) => f.id))
      uploadedFonts.value = [
        ...uploadedFonts.value.filter((f) => !createdIds.has(f.id)),
        ...created,
      ]
    }
    loadedOnce = true
    return created
  } finally {
    uploadingFonts.value = false
  }
}

export async function deleteCustomFont(id: string): Promise<void> {
  const res = await fetch(resolveFontsEndpoint(`/${encodeURIComponent(id)}`), {
    method: 'DELETE',
    headers: buildFontsHeaders({ Accept: 'application/json' }),
    credentials: 'include',
  })
  if (!res.ok) {
    throw new Error(await extractErrorMessage(res))
  }
  uploadedFonts.value = uploadedFonts.value.filter((f) => f.id !== id)
  for (const listener of deletedListeners) {
    listener(id)
  }
}

interface LocalFontDataLike {
  family?: string
}

export async function querySystemFontFamilies(): Promise<string[]> {
  if (typeof window === 'undefined' || !('queryLocalFonts' in window)) {
    return []
  }
  loadingLocalFonts.value = true
  try {
    const queryFn = (
      window as Window & { queryLocalFonts?: () => Promise<LocalFontDataLike[]> }
    ).queryLocalFonts
    if (typeof queryFn !== 'function') return []
    const records = await queryFn.call(window)
    const families = new Set<string>()
    for (const item of records ?? []) {
      const fam = sanitizeFontFamilyName(item?.family ?? '')
      if (fam) families.add(fam)
    }
    const sorted = [...families].sort((a, b) =>
      a.localeCompare(b, undefined, { sensitivity: 'base' }),
    )
    localSystemFonts.value = sorted
    return sorted
  } finally {
    loadingLocalFonts.value = false
  }
}

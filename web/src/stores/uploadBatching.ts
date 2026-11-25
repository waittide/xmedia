import { getActiveAuthHeader, resolveAppApiUrl } from '@/api/appHost'
import { i18n } from '@/i18n'
import { getUploadRelativePath } from '@/utils/fileDrop'

const MAX_BATCH_FILES = 50
const MAX_BATCH_BYTES = 400 * 1024 * 1024

export class UploadAbortedError extends Error {
  constructor() {
    super('Upload cancelled')
    this.name = 'UploadAbortedError'
  }
}

let fallbackTaskSeq = 0

export function generateUploadTaskId(): string {
  const cryptoObj = typeof globalThis !== 'undefined' ? globalThis.crypto : undefined
  if (cryptoObj && typeof cryptoObj.randomUUID === 'function') {
    return cryptoObj.randomUUID()
  }
  if (cryptoObj && typeof cryptoObj.getRandomValues === 'function') {
    const bytes = new Uint8Array(16)
    cryptoObj.getRandomValues(bytes)
    bytes[6] = (bytes[6]! & 0x0f) | 0x40
    bytes[8] = (bytes[8]! & 0x3f) | 0x80
    const hex = Array.from(bytes, (b) => b.toString(16).padStart(2, '0')).join('')
    return `${hex.slice(0, 8)}-${hex.slice(8, 12)}-${hex.slice(12, 16)}-${hex.slice(16, 20)}-${hex.slice(20)}`
  }
  fallbackTaskSeq += 1
  return `upload-${Date.now().toString(36)}-${fallbackTaskSeq}-${Math.random().toString(36).slice(2, 10)}`
}

export function sendUploadXhr<T>(
  url: string,
  formData: FormData,
  options: {
    onProgress?: (loaded: number, total: number) => void
    onXhrCreated?: (xhr: XMLHttpRequest) => void
    onUnauthorized?: () => void
  } = {},
): Promise<T> {
  return new Promise((resolve, reject) => {
    const xhr = new XMLHttpRequest()
    xhr.open('POST', resolveAppApiUrl(url))
    xhr.withCredentials = true
    const authHeaders = getActiveAuthHeader()
    for (const [headerName, headerValue] of Object.entries(authHeaders)) {
      xhr.setRequestHeader(headerName, headerValue)
    }
    xhr.upload.onprogress = (ev) => {
      if (ev.lengthComputable && options.onProgress) {
        options.onProgress(ev.loaded, ev.total)
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
          options.onUnauthorized?.()
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
    xhr.onabort = () => reject(new UploadAbortedError())
    options.onXhrCreated?.(xhr)
    xhr.send(formData)
  })
}

export function splitFileStemAndExt(name: string): { stem: string; ext: string } {
  const dotIdx = name.lastIndexOf('.')
  if (dotIdx <= 0 || dotIdx === name.length - 1) {
    return { stem: name, ext: '' }
  }
  return {
    stem: name.slice(0, dotIdx),
    ext: name.slice(dotIdx + 1).toLowerCase(),
  }
}

export function getParentDirectory(file: File): string {
  const relativePath = getUploadRelativePath(file)
  const slashIdx = relativePath.lastIndexOf('/')
  return slashIdx >= 0 ? relativePath.slice(0, slashIdx) : ''
}

function buildDirectoryDisplayNames(dirs: readonly string[]): Map<string, string> {
  const uniqueDirs = Array.from(new Set(dirs.filter(Boolean)))
  const segmentsByDir = new Map<string, string[]>()
  const depthByDir = new Map<string, number>()

  for (const dir of uniqueDirs) {
    const segments = dir.split('/').filter(Boolean)
    segmentsByDir.set(dir, segments)
    depthByDir.set(dir, 1)
  }

  const labelFor = (dir: string): string => {
    const segments = segmentsByDir.get(dir) ?? []
    const depth = depthByDir.get(dir) ?? 1
    return segments.slice(-depth).join('/') || dir
  }

  while (true) {
    const collisions = new Map<string, string[]>()
    for (const dir of uniqueDirs) {
      const label = labelFor(dir)
      const list = collisions.get(label)
      if (list) list.push(dir)
      else collisions.set(label, [dir])
    }

    let expanded = false
    for (const group of collisions.values()) {
      if (group.length <= 1) continue
      for (const dir of group) {
        const segments = segmentsByDir.get(dir) ?? []
        const depth = depthByDir.get(dir) ?? 1
        if (depth < segments.length) {
          depthByDir.set(dir, depth + 1)
          expanded = true
        }
      }
    }
    if (!expanded) break
  }

  const result = new Map<string, string>()
  for (const dir of uniqueDirs) {
    result.set(dir, labelFor(dir))
  }
  return result
}

function buildAtomicMediaUnits(
  files: File[],
  sidecarExts: readonly string[],
  allowStemPrefixSidecars: boolean,
): File[][] {
  const primaryEntries: Array<{ file: File; stemLower: string; sidecars: File[] }> = []
  const sidecars: File[] = []

  for (const file of files) {
    const { stem, ext } = splitFileStemAndExt(file.name)
    if (sidecarExts.includes(ext)) {
      sidecars.push(file)
    } else {
      primaryEntries.push({ file, stemLower: stem.toLowerCase(), sidecars: [] })
    }
  }

  const unmatchedSidecars: File[] = []

  if (allowStemPrefixSidecars && primaryEntries.length === 1) {
    primaryEntries[0]!.sidecars.push(...sidecars)
  } else {
    const exactIndex = new Map<string, number>()
    for (let i = 0; i < primaryEntries.length; i += 1) {
      const stemLower = primaryEntries[i]!.stemLower
      if (!exactIndex.has(stemLower)) {
        exactIndex.set(stemLower, i)
      }
    }

    for (const sidecar of sidecars) {
      const sidecarStemLower = splitFileStemAndExt(sidecar.name).stem.toLowerCase()
      let matchedIdx = exactIndex.get(sidecarStemLower)

      if (matchedIdx === undefined && allowStemPrefixSidecars) {
        let bestLen = -1
        for (let i = 0; i < primaryEntries.length; i += 1) {
          const primaryStemLower = primaryEntries[i]!.stemLower
          if (
            primaryStemLower.length > bestLen &&
            sidecarStemLower.startsWith(`${primaryStemLower}.`)
          ) {
            bestLen = primaryStemLower.length
            matchedIdx = i
          }
        }
      }

      if (matchedIdx !== undefined) {
        primaryEntries[matchedIdx]!.sidecars.push(sidecar)
      } else {
        unmatchedSidecars.push(sidecar)
      }
    }
  }

  const units: File[][] = primaryEntries.map((entry) => [entry.file, ...entry.sidecars])
  for (const sidecar of unmatchedSidecars) {
    units.push([sidecar])
  }
  return units
}

function chunkMediaUnits(units: File[][]): File[][] {
  const batches: File[][] = []
  let currentFiles: File[] = []
  let currentBytes = 0

  for (const unit of units) {
    const unitBytes = unit.reduce((sum, file) => sum + file.size, 0)
    const exceedsCount = currentFiles.length + unit.length > MAX_BATCH_FILES
    const exceedsBytes = currentBytes + unitBytes > MAX_BATCH_BYTES

    if (currentFiles.length > 0 && (exceedsCount || exceedsBytes)) {
      batches.push(currentFiles)
      currentFiles = []
      currentBytes = 0
    }

    currentFiles.push(...unit)
    currentBytes += unitBytes
  }

  if (currentFiles.length > 0) {
    batches.push(currentFiles)
  }

  return batches
}

function formatBatchLabel(
  files: File[],
  dirLabel: string | null,
  chunkIndex: number,
  totalChunks: number,
): string {
  if (dirLabel) {
    const countText =
      files.length === 1
        ? i18n.global.t('uploads.fileCountOne')
        : i18n.global.t('uploads.fileCount', { count: files.length })
    if (totalChunks > 1) {
      return `${dirLabel} (${chunkIndex + 1}/${totalChunks} · ${countText})`
    }
    return `${dirLabel} (${countText})`
  }
  if (files.length === 1) {
    return files[0]!.name
  }
  const baseLabel = `${files[0]!.name} +${files.length - 1}`
  return totalChunks > 1 ? `${baseLabel} (${chunkIndex + 1}/${totalChunks})` : baseLabel
}

export interface PlannedMediaBatch {
  label: string
  files: File[]
  totalBytes: number
}

export function planDomainMediaBatches(
  files: File[],
  options: {
    groupByDirectory: boolean
    sidecarExtensions?: readonly string[]
    allowStemPrefixSidecars?: boolean
  },
): PlannedMediaBatch[] {
  if (files.length === 0) return []
  const sidecarExts = options.sidecarExtensions ?? []
  const allowStemPrefix = Boolean(options.allowStemPrefixSidecars)

  const groups = new Map<string, File[]>()
  if (options.groupByDirectory) {
    for (const file of files) {
      const parentDir = getParentDirectory(file)
      const list = groups.get(parentDir)
      if (list) list.push(file)
      else groups.set(parentDir, [file])
    }
  } else {
    groups.set('', files)
  }

  const displayNames = options.groupByDirectory
    ? buildDirectoryDisplayNames(Array.from(groups.keys()))
    : new Map<string, string>()

  const planned: PlannedMediaBatch[] = []
  for (const [parentDir, groupFiles] of groups.entries()) {
    const units = buildAtomicMediaUnits(groupFiles, sidecarExts, allowStemPrefix)
    const chunks = chunkMediaUnits(units)
    const dirLabel = parentDir ? (displayNames.get(parentDir) ?? parentDir) : null
    for (let i = 0; i < chunks.length; i += 1) {
      const chunk = chunks[i]!
      const totalBytes = chunk.reduce((sum, f) => sum + f.size, 0)
      planned.push({
        label: formatBatchLabel(chunk, dirLabel, i, chunks.length),
        files: chunk,
        totalBytes,
      })
    }
  }

  return planned
}

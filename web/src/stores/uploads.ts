import { computed, ref } from 'vue'
import { defineStore } from 'pinia'
import { getStoredToken } from '@/api/client'
import { invalidateHomeCache } from '@/config/homeCache'
import { i18n } from '@/i18n'
import { useAuthStore } from '@/stores/auth'
import { useEventsStore } from '@/stores/events'
import { useSnackbarStore } from '@/stores/snackbar'
import type { MediaKind } from '@/types/api'
import { getUploadRelativePath, isIgnoredSystemFile } from '@/utils/fileDrop'

const MAX_CONCURRENT_UPLOADS = 3
const MAX_BATCH_FILES = 50
const MAX_BATCH_BYTES = 400 * 1024 * 1024

const SIDECAR_EXTENSIONS_BY_KIND: Record<'music' | 'video', readonly string[]> = {
  music: ['lrc'],
  video: ['srt', 'vtt', 'webvtt', 'ass', 'ssa'],
}

export const UPLOAD_EXTENSIONS_BY_KIND: Record<MediaKind, readonly string[]> = {
  book: ['epub', 'txt', 'pdf'],
  comic: ['cbz', 'zip'],
  music: ['flac', 'mp3', 'm4a', 'aac', 'alac', 'ogg', 'opus', 'wav', 'lrc'],
  video: [
    'mp4',
    'mkv',
    'webm',
    'mov',
    'avi',
    'ts',
    'm2ts',
    'mts',
    'm4v',
    'flv',
    'srt',
    'vtt',
    'webvtt',
    'ass',
    'ssa',
  ],
}

export function classifyUploadFileKind(name: string): MediaKind | null {
  const dotIdx = name.lastIndexOf('.')
  if (dotIdx <= 0 || dotIdx === name.length - 1) return null
  const ext = name.slice(dotIdx + 1).toLowerCase()
  for (const kind of ['book', 'comic', 'music', 'video'] as const) {
    if (UPLOAD_EXTENSIONS_BY_KIND[kind].includes(ext)) return kind
  }
  return null
}

export function getAcceptedUploadExtensions(enabledModules: readonly MediaKind[]): string {
  const exts: string[] = []
  for (const kind of enabledModules) {
    for (const ext of UPLOAD_EXTENSIONS_BY_KIND[kind] ?? []) {
      exts.push(`.${ext}`)
    }
  }
  return exts.join(',')
}

export type UploadTaskStatus = 'queued' | 'uploading' | 'completed' | 'failed' | 'cancelled'

export interface UploadEnqueueOptions {
  comicTargetSeriesId?: string
  comicSeriesTitle?: string
  targetSeriesId?: string
  seriesTitle?: string
  fromDirectory?: boolean
}

export interface UploadTask {
  id: string
  label: string
  mediaKind: MediaKind
  status: UploadTaskStatus
  totalBytes: number
  uploadedBytes: number
  progress: number
  error?: string
  targetRoute?: string
  canRetry: boolean
}

interface StoredUploadPayload {
  kind: MediaKind
  files: File[]
  options?: UploadEnqueueOptions
}

class UploadAbortedError extends Error {
  constructor() {
    super('Upload cancelled')
    this.name = 'UploadAbortedError'
  }
}

function sendUploadXhr<T>(
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
    xhr.open('POST', url)
    xhr.withCredentials = true
    const token = getStoredToken()
    if (token) {
      xhr.setRequestHeader('Authorization', `Bearer ${token}`)
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

function isZhLocale(): boolean {
  return String(i18n.global.locale.value).toLowerCase().startsWith('zh')
}

function splitFileStemAndExt(name: string): { stem: string; ext: string } {
  const dotIdx = name.lastIndexOf('.')
  if (dotIdx <= 0 || dotIdx === name.length - 1) {
    return { stem: name, ext: '' }
  }
  return {
    stem: name.slice(0, dotIdx),
    ext: name.slice(dotIdx + 1).toLowerCase(),
  }
}

function getParentDirectory(file: File): string {
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

function buildAtomicMediaUnits(kind: 'music' | 'video', files: File[]): File[][] {
  const sidecarExts = SIDECAR_EXTENSIONS_BY_KIND[kind]
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

  if (kind === 'video' && primaryEntries.length === 1) {
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

      if (matchedIdx === undefined && kind === 'video') {
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
  const isZh = isZhLocale()
  if (dirLabel) {
    const countText = isZh
      ? `${files.length} 个文件`
      : `${files.length} ${files.length === 1 ? 'file' : 'files'}`
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

interface PlannedMediaBatch {
  label: string
  files: File[]
  totalBytes: number
}

function planMediaBatches(
  kind: 'music' | 'video',
  files: File[],
  groupByDirectory: boolean,
): PlannedMediaBatch[] {
  if (files.length === 0) return []

  const groups = new Map<string, File[]>()
  if (groupByDirectory) {
    for (const file of files) {
      const parentDir = getParentDirectory(file)
      const list = groups.get(parentDir)
      if (list) list.push(file)
      else groups.set(parentDir, [file])
    }
  } else {
    groups.set('', files)
  }

  const displayNames = groupByDirectory
    ? buildDirectoryDisplayNames(Array.from(groups.keys()))
    : new Map<string, string>()

  const planned: PlannedMediaBatch[] = []
  for (const [parentDir, groupFiles] of groups.entries()) {
    const units = buildAtomicMediaUnits(kind, groupFiles)
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

function formatUnsupportedMessage(names: string[]): string {
  const isZh = isZhLocale()
  if (names.length === 1) {
    return isZh
      ? `不支持的文件格式：${names[0]}`
      : `Unsupported file format: ${names[0]}`
  }
  const preview = names.slice(0, 2).join(', ') + (names.length > 2 ? '…' : '')
  return isZh
    ? `已跳过 ${names.length} 个不支持的文件：${preview}`
    : `Skipped ${names.length} unsupported file(s): ${preview}`
}

function formatDirectorySkippedMessage(acceptedCount: number, skippedCount: number): string {
  if (isZhLocale()) {
    return `已加入 ${acceptedCount} 个文件，已跳过 ${skippedCount} 个不支持的文件`
  }
  return `Queued ${acceptedCount} file(s), skipped ${skippedCount} unsupported file(s)`
}

function formatNoSupportedDirectoryFilesMessage(skippedCount: number): string {
  if (isZhLocale()) {
    return skippedCount > 0
      ? `文件夹中没有支持的媒体文件（已跳过 ${skippedCount} 个文件）`
      : '文件夹中没有支持的媒体文件'
  }
  return skippedCount > 0
    ? `No supported media files found in folder (skipped ${skippedCount})`
    : 'No supported media files found in folder'
}

export const useUploadStore = defineStore('uploads', () => {
  const eventsStore = useEventsStore()
  const snackbarStore = useSnackbarStore()
  const tasks = ref<UploadTask[]>([])
  const collapsed = ref(false)
  const versionCounter = ref(0)

  let runningCount = 0
  let queueEpoch = 0
  const pendingJobs: Array<{ taskId: string; epoch: number; execute: () => Promise<void> }> = []
  const activeXhrs = new Map<string, XMLHttpRequest>()
  const taskPayloads = new Map<string, StoredUploadPayload>()

  const activeCount = computed(
    () => tasks.value.filter((t) => t.status === 'queued' || t.status === 'uploading').length,
  )
  const completedCount = computed(
    () => tasks.value.filter((t) => t.status === 'completed').length,
  )
  const failedCount = computed(() => tasks.value.filter((t) => t.status === 'failed').length)
  const overallProgress = computed(() => {
    const active = tasks.value.filter((t) => t.status === 'uploading' || t.status === 'completed')
    const total = active.reduce((acc, t) => acc + t.totalBytes, 0)
    if (total <= 0) return 0
    const loaded = active.reduce((acc, t) => acc + t.uploadedBytes, 0)
    return Math.min(100, Math.round((loaded / total) * 100))
  })

  function notifyCatalogChanged(kind?: MediaKind): void {
    versionCounter.value += 1
    if (kind) {
      eventsStore.bumpDomain(kind)
    } else {
      invalidateHomeCache()
    }
  }

  function pumpUploadQueue(): void {
    while (runningCount < MAX_CONCURRENT_UPLOADS && pendingJobs.length > 0) {
      const next = pendingJobs.shift()
      if (!next || next.epoch !== queueEpoch) continue
      const task = tasks.value.find((t) => t.id === next.taskId)
      if (!task || task.status !== 'queued') continue
      runningCount += 1
      void next.execute().finally(() => {
        runningCount = Math.max(0, runningCount - 1)
        pumpUploadQueue()
      })
    }
  }

  function scheduleExecution(task: UploadTask, execute: (epoch: number) => Promise<void>): void {
    const epoch = queueEpoch
    pendingJobs.push({
      taskId: task.id,
      epoch,
      execute: async () => {
        if (epoch !== queueEpoch || task.status !== 'queued') return
        task.status = 'uploading'
        await execute(epoch)
      },
    })
    pumpUploadQueue()
  }

  async function executePayload(task: UploadTask, payload: StoredUploadPayload, epoch: number): Promise<void> {
    const authStore = useAuthStore()
    const onProgress = (loaded: number, total: number) => {
      if (epoch !== queueEpoch || task.status !== 'uploading') return
      task.uploadedBytes = loaded
      task.totalBytes = total
      task.progress = Math.round((loaded / Math.max(1, total)) * 100)
    }
    const onXhrCreated = (xhr: XMLHttpRequest) => {
      activeXhrs.set(task.id, xhr)
    }
    const onUnauthorized = () => {
      authStore.clearLocalSession()
    }

    try {
      if (payload.kind === 'book') {
        const file = payload.files[0]!
        const fd = new FormData()
        fd.append('file', file, file.name)
        const res = await sendUploadXhr<{ uploaded?: { id: string }[] }>(
          '/api/v1/book/books/upload',
          fd,
          { onProgress, onXhrCreated, onUnauthorized },
        )
        if (!res.uploaded || res.uploaded.length === 0) {
          throw new Error('No book files were imported')
        }
        task.status = 'completed'
        task.uploadedBytes = task.totalBytes
        task.progress = 100
        task.canRetry = false
        taskPayloads.delete(task.id)
        if (res.uploaded[0]?.id) {
          task.targetRoute = `/book/items/${res.uploaded[0].id}`
        }
        notifyCatalogChanged('book')
      } else if (payload.kind === 'comic') {
        const file = payload.files[0]!
        const fd = new FormData()
        const sid = payload.options?.comicTargetSeriesId ?? payload.options?.targetSeriesId
        const stitle = payload.options?.comicSeriesTitle ?? payload.options?.seriesTitle
        if (sid) fd.append('target_series_id', sid)
        if (stitle) fd.append('series_title', stitle)
        fd.append('file', file, file.name)
        const res = await sendUploadXhr<{
          uploaded?: { series_id: string }[]
          series_id?: string
        }>('/api/v1/comic/upload', fd, { onProgress, onXhrCreated, onUnauthorized })
        const resolvedSeriesId = res.uploaded?.[0]?.series_id ?? res.series_id
        if (!resolvedSeriesId) {
          throw new Error('No comic archive was imported')
        }
        task.status = 'completed'
        task.uploadedBytes = task.totalBytes
        task.progress = 100
        task.canRetry = false
        taskPayloads.delete(task.id)
        task.targetRoute = `/comic/series/${resolvedSeriesId}`
        notifyCatalogChanged('comic')
      } else if (payload.kind === 'music') {
        const fd = new FormData()
        for (const f of payload.files) fd.append('file', f, f.name)
        const res = await sendUploadXhr<{
          uploaded_tracks?: { id: string }[]
          attached_lyrics_count?: number
        }>('/api/v1/music/tracks/upload', fd, { onProgress, onXhrCreated, onUnauthorized })
        const trackCount = res.uploaded_tracks?.length ?? 0
        const lyricCount = res.attached_lyrics_count ?? 0
        if (trackCount === 0 && lyricCount === 0) {
          throw new Error('No music tracks or matching lyrics were imported')
        }
        task.status = 'completed'
        task.uploadedBytes = task.totalBytes
        task.progress = 100
        task.canRetry = false
        taskPayloads.delete(task.id)
        task.targetRoute = '/music/songs'
        notifyCatalogChanged('music')
      } else {
        const fd = new FormData()
        for (const f of payload.files) fd.append('files', f, f.name)
        const res = await sendUploadXhr<{
          uploaded_videos?: { id: string }[]
          attached_subtitles?: number
        }>('/api/v1/video/upload', fd, { onProgress, onXhrCreated, onUnauthorized })
        const videoCount = res.uploaded_videos?.length ?? 0
        const subCount = res.attached_subtitles ?? 0
        if (videoCount === 0 && subCount === 0) {
          throw new Error('No video files or matching subtitles were imported')
        }
        task.status = 'completed'
        task.uploadedBytes = task.totalBytes
        task.progress = 100
        task.canRetry = false
        taskPayloads.delete(task.id)
        if (res.uploaded_videos?.[0]?.id) {
          task.targetRoute = `/video/watch/${res.uploaded_videos[0].id}`
        }
        notifyCatalogChanged('video')
      }
    } catch (err) {
      if (epoch !== queueEpoch) return
      if (err instanceof UploadAbortedError || task.status === 'cancelled') {
        task.status = 'cancelled'
        task.error = undefined
        task.canRetry = taskPayloads.has(task.id)
      } else {
        task.status = 'failed'
        task.error = err instanceof Error ? err.message : String(err)
        task.canRetry = taskPayloads.has(task.id)
      }
    } finally {
      activeXhrs.delete(task.id)
    }
  }

  function createAndScheduleTask(
    label: string,
    payload: StoredUploadPayload,
    totalBytes: number,
  ): void {
    const task: UploadTask = {
      id: crypto.randomUUID(),
      label,
      mediaKind: payload.kind,
      status: 'queued',
      totalBytes,
      uploadedBytes: 0,
      progress: 0,
      canRetry: true,
    }
    taskPayloads.set(task.id, payload)
    tasks.value.unshift(task)
    scheduleExecution(task, (epoch) => executePayload(task, payload, epoch))
  }

  function enqueueFiles(fileList: FileList | File[], options?: UploadEnqueueOptions): void {
    const rawFiles = Array.from(fileList)
    const hasSubdirPath = rawFiles.some((file) => getParentDirectory(file) !== '')
    const isFromDirectory = Boolean(options?.fromDirectory || hasSubdirPath)

    if (rawFiles.length === 0) {
      if (options?.fromDirectory) {
        snackbarStore.show({
          message: formatNoSupportedDirectoryFilesMessage(0),
          tone: 'error',
        })
      }
      return
    }

    const authStore = useAuthStore()
    const musicBatch: File[] = []
    const videoBatch: File[] = []
    const unsupportedNames: string[] = []
    let acceptedCount = 0

    for (const file of rawFiles) {
      const relativePath = getUploadRelativePath(file)
      if (isIgnoredSystemFile(relativePath) || isIgnoredSystemFile(file.name)) {
        continue
      }
      const kind = classifyUploadFileKind(file.name)
      if (!kind || !authStore.hasModule(kind)) {
        unsupportedNames.push(file.name)
        continue
      }
      acceptedCount += 1
      if (kind === 'music') {
        musicBatch.push(file)
      } else if (kind === 'video') {
        videoBatch.push(file)
      } else if (kind === 'book') {
        createAndScheduleTask(file.name, { kind: 'book', files: [file] }, file.size)
      } else if (kind === 'comic') {
        createAndScheduleTask(file.name, { kind: 'comic', files: [file], options }, file.size)
      }
    }

    const musicGroupByDir = musicBatch.some((file) => getParentDirectory(file) !== '')
    for (const batch of planMediaBatches('music', musicBatch, musicGroupByDir)) {
      createAndScheduleTask(batch.label, { kind: 'music', files: batch.files }, batch.totalBytes)
    }

    const videoGroupByDir = videoBatch.some((file) => getParentDirectory(file) !== '')
    for (const batch of planMediaBatches('video', videoBatch, videoGroupByDir)) {
      createAndScheduleTask(batch.label, { kind: 'video', files: batch.files }, batch.totalBytes)
    }

    if (acceptedCount > 0) {
      collapsed.value = false
    }

    if (isFromDirectory) {
      if (acceptedCount === 0) {
        snackbarStore.show({
          message: formatNoSupportedDirectoryFilesMessage(unsupportedNames.length),
          tone: 'error',
        })
      } else if (unsupportedNames.length > 0) {
        snackbarStore.show({
          message: formatDirectorySkippedMessage(acceptedCount, unsupportedNames.length),
          tone: 'default',
        })
      }
    } else if (unsupportedNames.length > 0) {
      snackbarStore.show({
        message: formatUnsupportedMessage(unsupportedNames),
        tone: 'error',
      })
    }
  }

  function cancelTask(taskId: string): void {
    const task = tasks.value.find((t) => t.id === taskId)
    if (!task || (task.status !== 'queued' && task.status !== 'uploading')) return

    task.status = 'cancelled'
    task.error = undefined
    task.canRetry = taskPayloads.has(taskId)

    for (let i = pendingJobs.length - 1; i >= 0; i -= 1) {
      if (pendingJobs[i]?.taskId === taskId) {
        pendingJobs.splice(i, 1)
      }
    }

    const xhr = activeXhrs.get(taskId)
    if (xhr) {
      activeXhrs.delete(taskId)
      try {
        xhr.abort()
      } catch {
        // ignore
      }
    }
  }

  function retryTask(taskId: string): void {
    const task = tasks.value.find((t) => t.id === taskId)
    if (!task || (task.status !== 'failed' && task.status !== 'cancelled')) return
    const payload = taskPayloads.get(taskId)
    if (!payload) return

    task.status = 'queued'
    task.uploadedBytes = 0
    task.progress = 0
    task.error = undefined
    task.targetRoute = undefined
    collapsed.value = false
    scheduleExecution(task, (epoch) => executePayload(task, payload, epoch))
  }

  function clearCompleted(): void {
    const remaining: UploadTask[] = []
    for (const t of tasks.value) {
      if (t.status === 'completed') {
        taskPayloads.delete(t.id)
      } else {
        remaining.push(t)
      }
    }
    tasks.value = remaining
  }

  function dismissAll(): void {
    if (activeCount.value === 0) {
      tasks.value = []
      taskPayloads.clear()
    }
  }

  function resetState(): void {
    queueEpoch += 1
    pendingJobs.length = 0
    for (const xhr of activeXhrs.values()) {
      try {
        xhr.abort()
      } catch {
        // ignore
      }
    }
    activeXhrs.clear()
    taskPayloads.clear()
    tasks.value = []
    collapsed.value = false
  }

  return {
    tasks,
    collapsed,
    versionCounter,
    catalogVersion: versionCounter,
    activeCount,
    completedCount,
    failedCount,
    overallProgress,
    enqueueFiles,
    cancelTask,
    retryTask,
    notifyCatalogChanged,
    clearCompleted,
    dismissAll,
    resetState,
  }
})

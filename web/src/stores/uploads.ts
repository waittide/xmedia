import { computed, ref } from 'vue'
import { defineStore } from 'pinia'
import { getStoredToken } from '@/api/client'
import { invalidateHomeCache } from '@/config/homeCache'
import { i18n } from '@/i18n'
import { useAuthStore } from '@/stores/auth'
import { useEventsStore } from '@/stores/events'
import { useSnackbarStore } from '@/stores/snackbar'
import type { MediaKind } from '@/types/api'

const MAX_CONCURRENT_UPLOADS = 3

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

function formatUnsupportedMessage(names: string[]): string {
  const isZh = String(i18n.global.locale.value).toLowerCase().startsWith('zh')
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
    const files = Array.from(fileList)
    if (files.length === 0) return

    const authStore = useAuthStore()
    const musicBatch: File[] = []
    const videoBatch: File[] = []
    const unsupportedNames: string[] = []
    let acceptedCount = 0

    for (const file of files) {
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

    if (musicBatch.length > 0) {
      const totalBytes = musicBatch.reduce((s, f) => s + f.size, 0)
      const label =
        musicBatch.length === 1
          ? musicBatch[0]!.name
          : `${musicBatch[0]!.name} +${musicBatch.length - 1}`
      createAndScheduleTask(label, { kind: 'music', files: musicBatch }, totalBytes)
    }

    if (videoBatch.length > 0) {
      const totalBytes = videoBatch.reduce((s, f) => s + f.size, 0)
      const label =
        videoBatch.length === 1
          ? videoBatch[0]!.name
          : `${videoBatch[0]!.name} +${videoBatch.length - 1}`
      createAndScheduleTask(label, { kind: 'video', files: videoBatch }, totalBytes)
    }

    if (acceptedCount > 0) {
      collapsed.value = false
    }

    if (unsupportedNames.length > 0) {
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

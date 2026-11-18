import { computed, ref } from 'vue'
import { defineStore } from 'pinia'
import { uploadWithXhr } from '@/api/client'
import { invalidateHomeCache } from '@/config/homeCache'
import { useEventsStore } from '@/stores/events'
import type { MediaKind } from '@/types/api'

const MAX_CONCURRENT_UPLOADS = 3

export interface UploadTask {
  id: string
  label: string
  mediaKind: MediaKind
  status: 'queued' | 'uploading' | 'completed' | 'failed'
  totalBytes: number
  uploadedBytes: number
  progress: number
  error?: string
  targetRoute?: string
}

function classifyFileKind(name: string): MediaKind | null {
  const ext = name.split('.').pop()?.toLowerCase() ?? ''
  if (['epub', 'txt', 'pdf'].includes(ext)) return 'book'
  if (['cbz', 'zip'].includes(ext)) return 'comic'
  if (['flac', 'mp3', 'm4a', 'aac', 'alac', 'ogg', 'opus', 'wav', 'lrc'].includes(ext)) {
    return 'music'
  }
  if (
    ['mp4', 'mkv', 'webm', 'mov', 'avi', 'ts', 'm2ts', 'mts', 'm4v', 'flv', 'srt', 'vtt', 'webvtt', 'ass', 'ssa'].includes(
      ext,
    )
  ) {
    return 'video'
  }
  return null
}

export const useUploadStore = defineStore('uploads', () => {
  const eventsStore = useEventsStore()
  const tasks = ref<UploadTask[]>([])
  const collapsed = ref(false)
  const versionCounter = ref(0)
  let runningCount = 0
  let queueEpoch = 0
  const pendingJobs: Array<{ epoch: number; execute: () => Promise<void> }> = []

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
    invalidateHomeCache(kind)
    if (kind) eventsStore.bumpDomain(kind)
  }

  function pumpUploadQueue(): void {
    while (runningCount < MAX_CONCURRENT_UPLOADS && pendingJobs.length > 0) {
      const next = pendingJobs.shift()
      if (!next || next.epoch !== queueEpoch) continue
      runningCount += 1
      void next.execute().finally(() => {
        runningCount = Math.max(0, runningCount - 1)
        pumpUploadQueue()
      })
    }
  }

  function scheduleTask(task: UploadTask, execute: () => Promise<void>): void {
    tasks.value.unshift(task)
    const epoch = queueEpoch
    pendingJobs.push({
      epoch,
      execute: async () => {
        if (epoch !== queueEpoch) return
        task.status = 'uploading'
        await execute()
      },
    })
    pumpUploadQueue()
  }

  function enqueueFiles(
    fileList: FileList | File[],
    options?: {
      comicTargetSeriesId?: string
      comicSeriesTitle?: string
      targetSeriesId?: string
      seriesTitle?: string
    },
  ): void {
    const files = Array.from(fileList)
    if (files.length === 0) return
    collapsed.value = false

    const musicBatch: File[] = []
    const videoBatch: File[] = []

    for (const file of files) {
      const kind = classifyFileKind(file.name)
      if (!kind) continue
      if (kind === 'music') {
        musicBatch.push(file)
      } else if (kind === 'video') {
        videoBatch.push(file)
      } else if (kind === 'book') {
        enqueueBookUpload(file)
      } else if (kind === 'comic') {
        enqueueComicUpload(file, options)
      }
    }

    if (musicBatch.length > 0) {
      enqueueMusicBatchUpload(musicBatch)
    }
    if (videoBatch.length > 0) {
      enqueueVideoBatchUpload(videoBatch)
    }
  }

  function enqueueBookUpload(file: File): void {
    const task: UploadTask = {
      id: crypto.randomUUID(),
      label: file.name,
      mediaKind: 'book',
      status: 'queued',
      totalBytes: file.size,
      uploadedBytes: 0,
      progress: 0,
    }
    scheduleTask(task, async () => {
      try {
        const fd = new FormData()
        fd.append('file', file, file.name)
        const res = await uploadWithXhr<{ uploaded?: { id: string }[] }>(
          '/api/v1/book/books/upload',
          fd,
          (loaded, total) => {
            task.uploadedBytes = loaded
            task.totalBytes = total
            task.progress = Math.round((loaded / Math.max(1, total)) * 100)
          },
        )
        if (!res.uploaded || res.uploaded.length === 0) {
          throw new Error('No book files were imported')
        }
        task.status = 'completed'
        task.uploadedBytes = task.totalBytes
        task.progress = 100
        if (res.uploaded[0]?.id) {
          task.targetRoute = `/book/items/${res.uploaded[0].id}`
        }
        notifyCatalogChanged('book')
      } catch (err) {
        task.status = 'failed'
        task.error = err instanceof Error ? err.message : String(err)
      }
    })
  }

  function enqueueComicUpload(
    file: File,
    options?: {
      comicTargetSeriesId?: string
      comicSeriesTitle?: string
      targetSeriesId?: string
      seriesTitle?: string
    },
  ): void {
    const task: UploadTask = {
      id: crypto.randomUUID(),
      label: file.name,
      mediaKind: 'comic',
      status: 'queued',
      totalBytes: file.size,
      uploadedBytes: 0,
      progress: 0,
    }
    scheduleTask(task, async () => {
      try {
        const fd = new FormData()
        const sid = options?.comicTargetSeriesId ?? options?.targetSeriesId
        const stitle = options?.comicSeriesTitle ?? options?.seriesTitle
        if (sid) fd.append('target_series_id', sid)
        if (stitle) fd.append('series_title', stitle)
        fd.append('file', file, file.name)
        const res = await uploadWithXhr<{
          uploaded?: { series_id: string }[]
          series_id?: string
        }>(
          '/api/v1/comic/upload',
          fd,
          (loaded, total) => {
            task.uploadedBytes = loaded
            task.totalBytes = total
            task.progress = Math.round((loaded / Math.max(1, total)) * 100)
          },
        )
        const resolvedSeriesId = res.uploaded?.[0]?.series_id ?? res.series_id
        if (!resolvedSeriesId) {
          throw new Error('No comic archive was imported')
        }
        task.status = 'completed'
        task.uploadedBytes = task.totalBytes
        task.progress = 100
        task.targetRoute = `/comic/series/${resolvedSeriesId}`
        notifyCatalogChanged('comic')
      } catch (err) {
        task.status = 'failed'
        task.error = err instanceof Error ? err.message : String(err)
      }
    })
  }

  function enqueueMusicBatchUpload(files: File[]): void {
    const totalBytes = files.reduce((s, f) => s + f.size, 0)
    const label = files.length === 1 ? files[0]!.name : `${files[0]!.name} +${files.length - 1}`
    const task: UploadTask = {
      id: crypto.randomUUID(),
      label,
      mediaKind: 'music',
      status: 'queued',
      totalBytes,
      uploadedBytes: 0,
      progress: 0,
    }
    scheduleTask(task, async () => {
      try {
        const fd = new FormData()
        for (const f of files) fd.append('file', f, f.name)
        const res = await uploadWithXhr<{
          uploaded_tracks?: { id: string }[]
          attached_lyrics_count?: number
        }>('/api/v1/music/tracks/upload', fd, (loaded, total) => {
          task.uploadedBytes = loaded
          task.totalBytes = total
          task.progress = Math.round((loaded / Math.max(1, total)) * 100)
        })
        const trackCount = res.uploaded_tracks?.length ?? 0
        const lyricCount = res.attached_lyrics_count ?? 0
        if (trackCount === 0 && lyricCount === 0) {
          throw new Error('No music tracks or matching lyrics were imported')
        }
        task.status = 'completed'
        task.uploadedBytes = task.totalBytes
        task.progress = 100
        task.targetRoute = '/music/songs'
        notifyCatalogChanged('music')
      } catch (err) {
        task.status = 'failed'
        task.error = err instanceof Error ? err.message : String(err)
      }
    })
  }

  function enqueueVideoBatchUpload(files: File[]): void {
    const totalBytes = files.reduce((s, f) => s + f.size, 0)
    const label = files.length === 1 ? files[0]!.name : `${files[0]!.name} +${files.length - 1}`
    const task: UploadTask = {
      id: crypto.randomUUID(),
      label,
      mediaKind: 'video',
      status: 'queued',
      totalBytes,
      uploadedBytes: 0,
      progress: 0,
    }
    scheduleTask(task, async () => {
      try {
        const fd = new FormData()
        for (const f of files) fd.append('files', f, f.name)
        const res = await uploadWithXhr<{
          uploaded_videos?: { id: string }[]
          attached_subtitles?: number
        }>(
          '/api/v1/video/upload',
          fd,
          (loaded, total) => {
            task.uploadedBytes = loaded
            task.totalBytes = total
            task.progress = Math.round((loaded / Math.max(1, total)) * 100)
          },
        )
        const videoCount = res.uploaded_videos?.length ?? 0
        const subCount = res.attached_subtitles ?? 0
        if (videoCount === 0 && subCount === 0) {
          throw new Error('No video files or matching subtitles were imported')
        }
        task.status = 'completed'
        task.uploadedBytes = task.totalBytes
        task.progress = 100
        if (res.uploaded_videos?.[0]?.id) {
          task.targetRoute = `/video/watch/${res.uploaded_videos[0].id}`
        }
        notifyCatalogChanged('video')
      } catch (err) {
        task.status = 'failed'
        task.error = err instanceof Error ? err.message : String(err)
      }
    })
  }

  function clearCompleted(): void {
    tasks.value = tasks.value.filter((t) => t.status !== 'completed')
  }

  function dismissAll(): void {
    if (activeCount.value === 0) {
      tasks.value = []
    }
  }

  function resetState(): void {
    queueEpoch += 1
    pendingJobs.length = 0
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
    notifyCatalogChanged,
    clearCompleted,
    dismissAll,
    resetState,
  }
})

import { computed, ref } from 'vue'
import { defineStore } from 'pinia'
import { notifyUnauthorized } from '@/api/client'
import { invalidateHomeCache } from '@/config/homeCache'
import {
  getDomainPlugin,
  listDomainPlugins,
  tryGetDomainPlugin,
  type DomainUploadEnqueueOptions,
} from '@/sources/domainRegistry'
import { useAuthStore } from '@/stores/auth'
import { useEventsStore } from '@/stores/events'
import { useSnackbarStore } from '@/stores/snackbar'
import {
  formatDirectorySkippedUploadMessage,
  formatNoSupportedDirectoryUploadMessage,
  formatUnsupportedUploadMessage,
  generateUploadTaskId,
  getParentDirectory,
  planDomainMediaBatches,
  sendUploadXhr,
  splitFileStemAndExt,
  UploadAbortedError,
} from '@/stores/uploadBatching'
import type { MediaKind } from '@/types/api'
import { getUploadRelativePath, isIgnoredSystemFile } from '@/utils/fileDrop'

const MAX_CONCURRENT_UPLOADS = 3

export type UploadEnqueueOptions = DomainUploadEnqueueOptions
export type UploadTaskStatus = 'queued' | 'uploading' | 'completed' | 'failed' | 'cancelled'

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

export const UPLOAD_EXTENSIONS_BY_KIND: Record<MediaKind, readonly string[]> = {
  get book() {
    return tryGetDomainPlugin('book')?.upload.extensions ?? []
  },
  get comic() {
    return tryGetDomainPlugin('comic')?.upload.extensions ?? []
  },
  get music() {
    return tryGetDomainPlugin('music')?.upload.extensions ?? []
  },
  get video() {
    return tryGetDomainPlugin('video')?.upload.extensions ?? []
  },
}

export function classifyUploadFileKind(name: string): MediaKind | null {
  const { ext } = splitFileStemAndExt(name)
  if (!ext) return null
  for (const plugin of listDomainPlugins()) {
    if (plugin.upload.extensions.includes(ext)) {
      return plugin.kind
    }
  }
  return null
}

export function getAcceptedUploadExtensions(enabledModules: readonly MediaKind[]): string {
  const exts: string[] = []
  for (const kind of enabledModules) {
    const plugin = tryGetDomainPlugin(kind)
    for (const ext of plugin?.upload.extensions ?? []) {
      exts.push(`.${ext}`)
    }
  }
  return exts.join(',')
}

export const useUploadStore = defineStore('uploads', () => {
  const eventsStore = useEventsStore()
  const snackbarStore = useSnackbarStore()
  const tasks = ref<UploadTask[]>([])
  const collapsed = ref(false)
  const versionCounter = ref(0)

  let runningCount = 0
  let queueEpoch = 0
  let nextAttemptId = 0
  const pendingJobs: Array<{
    taskId: string
    epoch: number
    attemptId: number
    execute: () => Promise<void>
  }> = []
  const activeXhrs = new Map<string, { attemptId: number; xhr: XMLHttpRequest }>()
  const taskAttempts = new Map<string, number>()
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
      if (taskAttempts.get(next.taskId) !== next.attemptId) continue
      const task = tasks.value.find((t) => t.id === next.taskId)
      if (!task || task.status !== 'queued') continue
      const runEpoch = next.epoch
      runningCount += 1
      void next.execute().finally(() => {
        if (runEpoch === queueEpoch) {
          runningCount = Math.max(0, runningCount - 1)
          pumpUploadQueue()
        }
      })
    }
  }

  function scheduleExecution(
    task: UploadTask,
    execute: (epoch: number, attemptId: number) => Promise<void>,
  ): void {
    const epoch = queueEpoch
    const attemptId = ++nextAttemptId
    taskAttempts.set(task.id, attemptId)
    pendingJobs.push({
      taskId: task.id,
      epoch,
      attemptId,
      execute: async () => {
        if (
          epoch !== queueEpoch ||
          taskAttempts.get(task.id) !== attemptId ||
          task.status !== 'queued'
        ) {
          return
        }
        task.status = 'uploading'
        await execute(epoch, attemptId)
      },
    })
    pumpUploadQueue()
  }

  async function executePayload(
    task: UploadTask,
    payload: StoredUploadPayload,
    epoch: number,
    attemptId: number,
  ): Promise<void> {
    const authStore = useAuthStore()
    const isStale = () => epoch !== queueEpoch || taskAttempts.get(task.id) !== attemptId
    const onProgress = (loaded: number, total: number) => {
      if (isStale() || task.status !== 'uploading') return
      task.uploadedBytes = loaded
      task.totalBytes = total
      task.progress = Math.round((loaded / Math.max(1, total)) * 100)
    }
    const onXhrCreated = (xhr: XMLHttpRequest) => {
      if (isStale()) {
        try {
          xhr.abort()
        } catch {
          // ignore
        }
        return
      }
      activeXhrs.set(task.id, { attemptId, xhr })
    }
    const onUnauthorized = () => {
      notifyUnauthorized()
      if (authStore.isAuthenticated) {
        authStore.clearLocalSession()
      }
    }

    try {
      const plugin = getDomainPlugin(payload.kind)
      const outcome = await plugin.upload.executeUpload(payload.files, {
        options: payload.options,
        sendXhr: <T>(url: string, fd: FormData) =>
          sendUploadXhr<T>(url, fd, { onProgress, onXhrCreated, onUnauthorized }),
      })
      if (isStale()) return
      task.status = 'completed'
      task.uploadedBytes = task.totalBytes
      task.progress = 100
      task.canRetry = false
      taskPayloads.delete(task.id)
      if (outcome.targetRoute) {
        task.targetRoute = outcome.targetRoute
      }
      notifyCatalogChanged(payload.kind)
    } catch (err) {
      if (isStale()) return
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
      const current = activeXhrs.get(task.id)
      if (current && current.attemptId === attemptId) {
        activeXhrs.delete(task.id)
      }
    }
  }

  function createAndScheduleTask(
    label: string,
    payload: StoredUploadPayload,
    totalBytes: number,
  ): void {
    const task: UploadTask = {
      id: generateUploadTaskId(),
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
    scheduleExecution(task, (epoch, attemptId) => executePayload(task, payload, epoch, attemptId))
  }

  function enqueueFiles(fileList: FileList | File[], options?: UploadEnqueueOptions): void {
    const rawFiles = Array.from(fileList)
    const hasSubdirPath = rawFiles.some((file) => getParentDirectory(file) !== '')
    const isFromDirectory = Boolean(options?.fromDirectory || hasSubdirPath)

    if (rawFiles.length === 0) {
      if (options?.fromDirectory) {
        snackbarStore.show({
          message: formatNoSupportedDirectoryUploadMessage(0),
          tone: 'error',
        })
      }
      return
    }

    const authStore = useAuthStore()
    const batchBuckets = new Map<MediaKind, File[]>()
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
      const plugin = getDomainPlugin(kind)
      if (plugin.upload.batchable) {
        const list = batchBuckets.get(kind)
        if (list) list.push(file)
        else batchBuckets.set(kind, [file])
      } else {
        createAndScheduleTask(file.name, { kind, files: [file], options }, file.size)
      }
    }

    for (const [kind, bucketFiles] of batchBuckets.entries()) {
      const plugin = getDomainPlugin(kind)
      const groupByDirectory = bucketFiles.some((file) => getParentDirectory(file) !== '')
      const batches = planDomainMediaBatches(bucketFiles, {
        groupByDirectory,
        sidecarExtensions: plugin.upload.sidecarExtensions,
        allowStemPrefixSidecars: plugin.upload.allowStemPrefixSidecars,
      })
      for (const batch of batches) {
        createAndScheduleTask(batch.label, { kind, files: batch.files, options }, batch.totalBytes)
      }
    }

    if (acceptedCount > 0) {
      collapsed.value = false
    }

    if (isFromDirectory) {
      if (acceptedCount === 0) {
        snackbarStore.show({
          message: formatNoSupportedDirectoryUploadMessage(unsupportedNames.length),
          tone: 'error',
        })
      } else if (unsupportedNames.length > 0) {
        snackbarStore.show({
          message: formatDirectorySkippedUploadMessage(acceptedCount, unsupportedNames.length),
          tone: 'default',
        })
      }
    } else if (unsupportedNames.length > 0) {
      snackbarStore.show({
        message: formatUnsupportedUploadMessage(unsupportedNames),
        tone: 'error',
      })
    }
  }

  function cancelTask(taskId: string): void {
    const task = tasks.value.find((t) => t.id === taskId)
    if (!task || (task.status !== 'queued' && task.status !== 'uploading')) return

    taskAttempts.set(taskId, ++nextAttemptId)
    task.status = 'cancelled'
    task.error = undefined
    task.canRetry = taskPayloads.has(taskId)

    for (let i = pendingJobs.length - 1; i >= 0; i -= 1) {
      if (pendingJobs[i]?.taskId === taskId) {
        pendingJobs.splice(i, 1)
      }
    }

    const active = activeXhrs.get(taskId)
    if (active) {
      activeXhrs.delete(taskId)
      try {
        active.xhr.abort()
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
    scheduleExecution(task, (epoch, attemptId) => executePayload(task, payload, epoch, attemptId))
  }

  function removeTask(taskId: string): void {
    cancelTask(taskId)
    taskAttempts.delete(taskId)
    taskPayloads.delete(taskId)
    tasks.value = tasks.value.filter((t) => t.id !== taskId)
  }

  function clearCompleted(): void {
    const remaining: UploadTask[] = []
    for (const t of tasks.value) {
      if (t.status === 'completed') {
        taskPayloads.delete(t.id)
        taskAttempts.delete(t.id)
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
      taskAttempts.clear()
    }
  }

  function resetState(): void {
    queueEpoch += 1
    runningCount = 0
    pendingJobs.length = 0
    for (const active of activeXhrs.values()) {
      try {
        active.xhr.abort()
      } catch {
        // ignore
      }
    }
    activeXhrs.clear()
    taskAttempts.clear()
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
    removeTask,
    notifyCatalogChanged,
    clearCompleted,
    dismissAll,
    resetState,
  }
})

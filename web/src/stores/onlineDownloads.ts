import { computed, ref } from 'vue'
import { defineStore } from 'pinia'
import {
  onlineBookApi,
  type DownloadJobStatus,
  type OnlineDownloadJobDto,
} from '@/domains/book/onlineApi'
import { useSnackbarStore } from '@/stores/snackbar'
import { useUploadStore } from '@/stores/uploads'

const POLL_INTERVAL_MS = 1200

export function isOnlineDownloadJobActive(status: DownloadJobStatus): boolean {
  return status === 'pending' || status === 'running'
}

export function formatDownloadSpeed(cps?: number): string {
  if (typeof cps !== 'number' || !Number.isFinite(cps) || cps <= 0) return ''
  return `${cps.toFixed(1)} 章/秒`
}

export const useOnlineDownloadsStore = defineStore('onlineDownloads', () => {
  const snackbar = useSnackbarStore()

  const jobs = ref<OnlineDownloadJobDto[]>([])
  const loading = ref(false)
  const busyJobId = ref<string | null>(null)

  let pollTimer: ReturnType<typeof setTimeout> | null = null
  let pollInFlight = false
  let refreshPromise: Promise<void> | null = null
  let sessionEpoch = 0
  let hydrated = false

  const previousStatusById = new Map<string, DownloadJobStatus>()
  const notifiedImportedIds = new Set<string>()

  const activeJobs = computed(() =>
    jobs.value.filter((job) => isOnlineDownloadJobActive(job.status)),
  )
  const activeCount = computed(() => activeJobs.value.length)
  const completedCount = computed(
    () => jobs.value.filter((job) => job.status === 'completed').length,
  )
  const failedCount = computed(() => jobs.value.filter((job) => job.status === 'failed').length)
  const cancelledCount = computed(
    () => jobs.value.filter((job) => job.status === 'cancelled').length,
  )
  const finishedCount = computed(
    () => jobs.value.filter((job) => !isOnlineDownloadJobActive(job.status)).length,
  )
  const hasActiveJobs = computed(() => activeCount.value > 0)

  function maybeNotifyImported(job: OnlineDownloadJobDto, isInitialHydration: boolean): void {
    const prevStatus = previousStatusById.get(job.id)
    if (job.status === 'completed' && job.imported_book_id) {
      if (isInitialHydration && prevStatus === undefined) {
        notifiedImportedIds.add(job.id)
        return
      }
      if (prevStatus !== 'completed' && !notifiedImportedIds.has(job.id)) {
        notifiedImportedIds.add(job.id)
        useUploadStore().notifyCatalogChanged('book')
      }
    }
  }

  function applyJobList(list: OnlineDownloadJobDto[], isInitialHydration: boolean): void {
    for (const job of list) {
      maybeNotifyImported(job, isInitialHydration)
      previousStatusById.set(job.id, job.status)
    }
    const currentIds = new Set(list.map((job) => job.id))
    for (const id of Array.from(previousStatusById.keys())) {
      if (!currentIds.has(id)) {
        previousStatusById.delete(id)
        notifiedImportedIds.delete(id)
      }
    }
    jobs.value = list
    hydrated = true
    ensurePolling()
  }

  function stopPolling(): void {
    if (pollTimer !== null) {
      clearTimeout(pollTimer)
      pollTimer = null
    }
  }

  function ensurePolling(): void {
    if (!hasActiveJobs.value) {
      stopPolling()
      return
    }
    if (pollTimer !== null || pollInFlight) return

    const epoch = sessionEpoch
    pollTimer = setTimeout(async () => {
      pollTimer = null
      if (epoch !== sessionEpoch || !hasActiveJobs.value) return
      pollInFlight = true
      try {
        const list = await onlineBookApi.listDownloads()
        if (epoch !== sessionEpoch) return
        applyJobList(list, false)
      } catch {
        // Ignore transient network errors during background polling
      } finally {
        pollInFlight = false
        if (epoch === sessionEpoch && hasActiveJobs.value) {
          ensurePolling()
        }
      }
    }, POLL_INTERVAL_MS)
  }

  async function refresh(): Promise<void> {
    if (refreshPromise) return refreshPromise

    const epoch = sessionEpoch
    const isInitialHydration = !hydrated
    loading.value = true

    const task = (async () => {
      try {
        const list = await onlineBookApi.listDownloads()
        if (epoch !== sessionEpoch) return
        applyJobList(list, isInitialHydration)
      } catch (err) {
        if (epoch !== sessionEpoch) return
        snackbar.show({
          message: err instanceof Error ? err.message : String(err),
          tone: 'error',
        })
      } finally {
        if (epoch === sessionEpoch) {
          loading.value = false
        }
        refreshPromise = null
      }
    })()

    refreshPromise = task
    return task
  }

  function trackJob(job: OnlineDownloadJobDto): void {
    maybeNotifyImported(job, false)
    previousStatusById.set(job.id, job.status)
    hydrated = true

    const idx = jobs.value.findIndex((item) => item.id === job.id)
    if (idx >= 0) {
      const next = [...jobs.value]
      next[idx] = job
      jobs.value = next
    } else {
      jobs.value = [job, ...jobs.value]
    }
    ensurePolling()
  }

  async function cancelJob(id: string): Promise<OnlineDownloadJobDto | null> {
    if (busyJobId.value === id) return null
    const epoch = sessionEpoch
    busyJobId.value = id
    try {
      const updated = await onlineBookApi.cancelDownload(id)
      if (epoch !== sessionEpoch) return null
      trackJob(updated)
      return updated
    } catch (err) {
      if (epoch === sessionEpoch) {
        snackbar.show({
          message: err instanceof Error ? err.message : String(err),
          tone: 'error',
        })
      }
      return null
    } finally {
      if (epoch === sessionEpoch && busyJobId.value === id) {
        busyJobId.value = null
      }
    }
  }

  async function deleteJob(id: string): Promise<boolean> {
    if (busyJobId.value === id) return false
    const epoch = sessionEpoch
    busyJobId.value = id
    try {
      await onlineBookApi.deleteDownload(id)
      if (epoch !== sessionEpoch) return false
      jobs.value = jobs.value.filter((job) => job.id !== id)
      previousStatusById.delete(id)
      notifiedImportedIds.delete(id)
      if (!hasActiveJobs.value) {
        stopPolling()
      }
      return true
    } catch (err) {
      if (epoch === sessionEpoch) {
        snackbar.show({
          message: err instanceof Error ? err.message : String(err),
          tone: 'error',
        })
      }
      return false
    } finally {
      if (epoch === sessionEpoch && busyJobId.value === id) {
        busyJobId.value = null
      }
    }
  }

  async function clearFinished(): Promise<void> {
    const finished = jobs.value.filter((job) => !isOnlineDownloadJobActive(job.status))
    if (finished.length === 0) return

    const epoch = sessionEpoch
    const results = await Promise.allSettled(
      finished.map(async (job) => {
        await onlineBookApi.deleteDownload(job.id)
        return job.id
      }),
    )
    if (epoch !== sessionEpoch) return

    const deletedIds = new Set<string>()
    let failedDeletes = 0
    for (const res of results) {
      if (res.status === 'fulfilled') {
        deletedIds.add(res.value)
        previousStatusById.delete(res.value)
        notifiedImportedIds.delete(res.value)
      } else {
        failedDeletes += 1
      }
    }

    if (deletedIds.size > 0) {
      jobs.value = jobs.value.filter((job) => !deletedIds.has(job.id))
    }
    if (!hasActiveJobs.value) {
      stopPolling()
    }
    if (failedDeletes > 0) {
      snackbar.show({
        message: `有 ${failedDeletes} 条任务记录未能清理`,
        tone: 'error',
      })
    }
  }

  function resetState(): void {
    sessionEpoch += 1
    stopPolling()
    pollInFlight = false
    refreshPromise = null
    hydrated = false
    jobs.value = []
    loading.value = false
    busyJobId.value = null
    previousStatusById.clear()
    notifiedImportedIds.clear()
  }

  return {
    jobs,
    loading,
    busyJobId,
    activeJobs,
    activeCount,
    completedCount,
    failedCount,
    cancelledCount,
    finishedCount,
    hasActiveJobs,
    refresh,
    trackJob,
    cancelJob,
    deleteJob,
    clearFinished,
    resetState,
  }
})

import { computed, ref } from 'vue'
import { defineStore } from 'pinia'
import {
  onlineBookApi,
  type DownloadJobStatus,
  type OnlineDownloadJobDto,
} from '@/domains/book/onlineApi'
import { i18n } from '@/i18n'
import { useEventsStore } from '@/stores/events'
import { useSnackbarStore } from '@/stores/snackbar'

const POLL_INTERVAL_MS = 1200
const POLL_MAX_BACKOFF_MS = 15000

export function isOnlineDownloadJobActive(status: DownloadJobStatus): boolean {
  return status === 'pending' || status === 'running'
}

export function formatDownloadSpeed(cps?: number): string {
  if (typeof cps !== 'number' || !Number.isFinite(cps) || cps <= 0) return ''
  return String(i18n.global.t('transfers.chaptersPerSec', { rate: cps.toFixed(1) }))
}

export const useOnlineDownloadsStore = defineStore('onlineDownloads', () => {
  const snackbar = useSnackbarStore()

  const jobs = ref<OnlineDownloadJobDto[]>([])
  const loading = ref(false)
  const busyJobId = ref<string | null>(null)

  let pollTimer: ReturnType<typeof setTimeout> | null = null
  let pollInFlight = false
  let consecutivePollErrors = 0
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
        useEventsStore().bumpDomain('book')
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
      consecutivePollErrors = 0
      return
    }
    if (pollTimer !== null || pollInFlight) return

    const epoch = sessionEpoch
    const delayMs =
      consecutivePollErrors === 0
        ? POLL_INTERVAL_MS
        : Math.min(POLL_MAX_BACKOFF_MS, POLL_INTERVAL_MS * 2 ** consecutivePollErrors)

    pollTimer = setTimeout(async () => {
      pollTimer = null
      if (epoch !== sessionEpoch || !hasActiveJobs.value) return
      if (typeof navigator !== 'undefined' && navigator.onLine === false) {
        consecutivePollErrors = Math.min(consecutivePollErrors + 1, 4)
        ensurePolling()
        return
      }
      pollInFlight = true
      try {
        const list = await onlineBookApi.listDownloads()
        if (epoch !== sessionEpoch) return
        consecutivePollErrors = 0
        applyJobList(list, false)
      } catch {
        consecutivePollErrors = Math.min(consecutivePollErrors + 1, 4)
      } finally {
        pollInFlight = false
        if (epoch === sessionEpoch && hasActiveJobs.value) {
          ensurePolling()
        }
      }
    }, delayMs)
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
        message: String(
          i18n.global.t('transfers.clearFinishedPartialError', { count: failedDeletes }),
        ),
        tone: 'error',
      })
    }
  }

  function resetState(): void {
    sessionEpoch += 1
    stopPolling()
    pollInFlight = false
    consecutivePollErrors = 0
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

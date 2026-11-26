import { computed, ref } from 'vue'
import { defineStore } from 'pinia'
import { ApiRequestError, api, withAuthToken } from '@/api/client'
import { invalidateHomeCache } from '@/config/homeCache'
import { useAuthStore } from '@/stores/auth'
import type { MediaKind } from '@/types/api'

const VALID_MEDIA_KINDS = new Set<MediaKind>(['book', 'comic', 'music', 'video'])

function isValidMediaKind(kind: unknown): kind is MediaKind {
  return typeof kind === 'string' && VALID_MEDIA_KINDS.has(kind as MediaKind)
}

interface SseEventFrame {
  event:
    | 'library_scan_started'
    | 'library_scan_progress'
    | 'library_scan_completed'
    | 'library_scan_failed'
    | 'media_changed'
  library_id?: string
  library_name?: string
  media_kind: MediaKind
  scanned?: number
  total?: number | null
  added?: number
  updated?: number
  missing?: number
  message?: string
}

const RECONNECT_BASE_DELAY_MS = 2000
const RECONNECT_MAX_DELAY_MS = 30000

export interface ScanTaskState {
  library_id: string
  library_name: string
  media_kind: MediaKind
  status: 'running' | 'completed' | 'failed'
  scanned: number
  total: number | null
  added?: number
  updated?: number
  missing?: number
  error?: string
  updated_at: number
}

export const useEventsStore = defineStore('events', () => {
  const connected = ref(false)
  const scanTasks = ref<Record<string, ScanTaskState>>({})
  const domainRefreshTick = ref<Record<MediaKind, number>>({
    book: 0,
    comic: 0,
    music: 0,
    video: 0,
  })

  const activeScans = computed(() => {
    const out: Record<
      string,
      { media_kind: MediaKind; library_name: string; scanned: number; total: number | null }
    > = {}
    for (const [id, task] of Object.entries(scanTasks.value)) {
      if (task.status === 'running') {
        out[id] = {
          media_kind: task.media_kind,
          library_name: task.library_name,
          scanned: task.scanned,
          total: task.total,
        }
      }
    }
    return out
  })

  const scanTaskList = computed(() =>
    Object.values(scanTasks.value).sort((a, b) => {
      if (a.status === 'running' && b.status !== 'running') return -1
      if (a.status !== 'running' && b.status === 'running') return 1
      return b.updated_at - a.updated_at
    }),
  )

  const anyScanning = computed(() => Object.keys(activeScans.value).length > 0)
  const firstActiveScan = computed(() => Object.values(activeScans.value)[0] ?? null)

  let eventSource: EventSource | null = null
  let reconnectTimer: ReturnType<typeof setTimeout> | null = null
  let reconnectAttempts = 0
  let consecutiveUnopenedErrors = 0
  let intentionalDisconnect = false
  let waitingForOnline = false
  let authProbeInFlight = false

  function isAuthenticated(): boolean {
    return useAuthStore().isAuthenticated
  }

  function isOffline(): boolean {
    return typeof navigator !== 'undefined' && navigator.onLine === false
  }

  function clearReconnectTimer(): void {
    if (reconnectTimer) {
      clearTimeout(reconnectTimer)
      reconnectTimer = null
    }
  }

  function clearOnlineListener(): void {
    if (waitingForOnline && typeof window !== 'undefined') {
      window.removeEventListener('online', handleOnline)
      waitingForOnline = false
    }
  }

  function waitForOnline(): void {
    if (waitingForOnline || typeof window === 'undefined') return
    waitingForOnline = true
    window.addEventListener('online', handleOnline)
  }

  function handleOnline(): void {
    clearOnlineListener()
    if (intentionalDisconnect || !isAuthenticated()) return
    reconnectAttempts = 0
    connect()
  }

  function closeEventSource(): void {
    if (eventSource) {
      const es = eventSource
      eventSource = null
      es.onopen = null
      es.onmessage = null
      es.onerror = null
      es.close()
    }
    connected.value = false
  }

  function scheduleReconnect(): void {
    if (intentionalDisconnect || !isAuthenticated()) return
    clearReconnectTimer()
    if (isOffline()) {
      waitForOnline()
      return
    }
    const delay = Math.min(RECONNECT_BASE_DELAY_MS * 2 ** reconnectAttempts, RECONNECT_MAX_DELAY_MS)
    reconnectAttempts += 1
    reconnectTimer = setTimeout(() => {
      reconnectTimer = null
      if (intentionalDisconnect || !isAuthenticated()) return
      if (isOffline()) {
        waitForOnline()
        return
      }
      connect()
    }, delay)
  }

  async function probeAuthOrReconnect(): Promise<void> {
    if (authProbeInFlight || intentionalDisconnect || !isAuthenticated()) return
    authProbeInFlight = true
    try {
      await api.me()
      if (!intentionalDisconnect && isAuthenticated()) {
        scheduleReconnect()
      }
    } catch (err) {
      if (err instanceof ApiRequestError && (err.status === 401 || err.status === 403)) {
        disconnect()
        useAuthStore().clearLocalSession()
        return
      }
      if (!intentionalDisconnect && isAuthenticated()) {
        scheduleReconnect()
      }
    } finally {
      authProbeInFlight = false
    }
  }

  function bumpDomain(kind: MediaKind): void {
    if (!isValidMediaKind(kind)) return
    invalidateHomeCache(kind)
    domainRefreshTick.value[kind] += 1
  }

  function clearCompletedScans(): void {
    for (const [id, task] of Object.entries(scanTasks.value)) {
      if (task.status === 'completed') {
        delete scanTasks.value[id]
      }
    }
  }

  function dismissFinishedScans(): void {
    for (const [id, task] of Object.entries(scanTasks.value)) {
      if (task.status !== 'running') {
        delete scanTasks.value[id]
      }
    }
  }

  function connect(): void {
    intentionalDisconnect = false
    clearReconnectTimer()
    clearOnlineListener()
    if (!isAuthenticated()) return
    if (eventSource) {
      if (eventSource.readyState === EventSource.CLOSED) {
        closeEventSource()
      } else {
        return
      }
    }
    if (isOffline()) {
      waitForOnline()
      return
    }

    const es = new EventSource(withAuthToken('/api/v1/events/stream'), { withCredentials: true })
    eventSource = es
    let openedOnce = false

    es.onopen = () => {
      if (eventSource !== es) return
      openedOnce = true
      connected.value = true
      reconnectAttempts = 0
      consecutiveUnopenedErrors = 0
    }

    es.onmessage = (ev) => {
      if (eventSource !== es) return
      try {
        const msg = JSON.parse(String(ev.data)) as SseEventFrame
        if (!isValidMediaKind(msg.media_kind)) return
        if (msg.event === 'library_scan_started' && msg.library_id) {
          const prev = scanTasks.value[msg.library_id]
          scanTasks.value[msg.library_id] = {
            library_id: msg.library_id,
            library_name: msg.library_name || prev?.library_name || msg.media_kind.toUpperCase(),
            media_kind: msg.media_kind,
            status: 'running',
            scanned: 0,
            total: null,
            updated_at: Date.now(),
          }
        } else if (msg.event === 'library_scan_progress' && msg.library_id) {
          const prev = scanTasks.value[msg.library_id]
          scanTasks.value[msg.library_id] = {
            library_id: msg.library_id,
            library_name: msg.library_name || prev?.library_name || msg.media_kind.toUpperCase(),
            media_kind: msg.media_kind,
            status: 'running',
            scanned: msg.scanned ?? 0,
            total: msg.total !== undefined ? msg.total : (prev?.total ?? null),
            updated_at: Date.now(),
          }
        } else if (msg.event === 'library_scan_completed' && msg.library_id) {
          const prev = scanTasks.value[msg.library_id]
          const scanned = msg.scanned ?? prev?.scanned ?? 0
          scanTasks.value[msg.library_id] = {
            library_id: msg.library_id,
            library_name: msg.library_name || prev?.library_name || msg.media_kind.toUpperCase(),
            media_kind: msg.media_kind,
            status: 'completed',
            scanned,
            total: scanned,
            added: msg.added ?? 0,
            updated: msg.updated ?? 0,
            missing: msg.missing ?? 0,
            updated_at: Date.now(),
          }
          invalidateHomeCache(msg.media_kind)
          domainRefreshTick.value[msg.media_kind] += 1
        } else if (msg.event === 'library_scan_failed' && msg.library_id) {
          const prev = scanTasks.value[msg.library_id]
          scanTasks.value[msg.library_id] = {
            library_id: msg.library_id,
            library_name: msg.library_name || prev?.library_name || msg.media_kind.toUpperCase(),
            media_kind: msg.media_kind,
            status: 'failed',
            scanned: prev?.scanned ?? 0,
            total: prev?.total ?? null,
            error: msg.message || 'Scan failed',
            updated_at: Date.now(),
          }
          invalidateHomeCache(msg.media_kind)
          domainRefreshTick.value[msg.media_kind] += 1
        } else if (msg.event === 'media_changed') {
          invalidateHomeCache(msg.media_kind)
          domainRefreshTick.value[msg.media_kind] += 1
        }
      } catch {
        // ignore malformed frame
      }
    }

    es.onerror = () => {
      if (eventSource !== es) return
      closeEventSource()
      if (!openedOnce) {
        consecutiveUnopenedErrors += 1
        if (consecutiveUnopenedErrors >= 2) {
          void probeAuthOrReconnect()
          return
        }
      }
      scheduleReconnect()
    }
  }

  function disconnect(): void {
    intentionalDisconnect = true
    reconnectAttempts = 0
    consecutiveUnopenedErrors = 0
    clearReconnectTimer()
    clearOnlineListener()
    closeEventSource()
    scanTasks.value = {}
  }

  return {
    connected,
    scanTasks,
    scanTaskList,
    activeScans,
    domainRefreshTick,
    anyScanning,
    firstActiveScan,
    bumpDomain,
    clearCompletedScans,
    dismissFinishedScans,
    connect,
    disconnect,
  }
})

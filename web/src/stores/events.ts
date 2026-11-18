import { computed, ref } from 'vue'
import { defineStore } from 'pinia'
import { getStoredToken } from '@/api/client'
import { invalidateHomeCache } from '@/config/homeCache'
import type { MediaKind } from '@/types/api'

interface WsEventFrame {
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

  let ws: WebSocket | null = null
  let reconnectTimer: ReturnType<typeof setTimeout> | null = null

  function bumpDomain(kind: MediaKind): void {
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
    const token = getStoredToken()
    if (!token || ws) return
    const proto = window.location.protocol === 'https:' ? 'wss:' : 'ws:'
    ws = new WebSocket(
      `${proto}//${window.location.host}/api/v1/events/ws?token=${encodeURIComponent(token)}`,
    )

    ws.onmessage = (ev) => {
      try {
        const msg = JSON.parse(String(ev.data)) as WsEventFrame
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

    ws.onclose = () => {
      ws = null
      if (getStoredToken()) {
        reconnectTimer = setTimeout(connect, 3000)
      }
    }
  }

  function disconnect(): void {
    if (reconnectTimer) {
      clearTimeout(reconnectTimer)
      reconnectTimer = null
    }
    if (ws) {
      ws.close()
      ws = null
    }
    scanTasks.value = {}
  }

  return {
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

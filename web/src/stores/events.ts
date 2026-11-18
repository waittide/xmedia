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
  media_kind: MediaKind
  scanned?: number
}

export const useEventsStore = defineStore('events', () => {
  const activeScans = ref<Record<string, { media_kind: MediaKind; scanned: number }>>({})
  const domainRefreshTick = ref<Record<MediaKind, number>>({
    book: 0,
    comic: 0,
    music: 0,
    video: 0,
  })

  const anyScanning = computed(() => Object.keys(activeScans.value).length > 0)
  const firstActiveScan = computed(() => Object.values(activeScans.value)[0] ?? null)

  let ws: WebSocket | null = null
  let reconnectTimer: ReturnType<typeof setTimeout> | null = null

  function bumpDomain(kind: MediaKind): void {
    invalidateHomeCache(kind)
    domainRefreshTick.value[kind] += 1
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
          activeScans.value[msg.library_id] = { media_kind: msg.media_kind, scanned: 0 }
        } else if (msg.event === 'library_scan_progress' && msg.library_id) {
          activeScans.value[msg.library_id] = {
            media_kind: msg.media_kind,
            scanned: msg.scanned ?? 0,
          }
        } else if (msg.event === 'library_scan_completed' && msg.library_id) {
          delete activeScans.value[msg.library_id]
          invalidateHomeCache(msg.media_kind)
          domainRefreshTick.value[msg.media_kind] += 1
        } else if (msg.event === 'library_scan_failed' && msg.library_id) {
          delete activeScans.value[msg.library_id]
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
    activeScans.value = {}
  }

  return {
    activeScans,
    domainRefreshTick,
    anyScanning,
    firstActiveScan,
    bumpDomain,
    connect,
    disconnect,
  }
})

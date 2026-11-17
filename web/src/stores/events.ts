import { ref } from 'vue'
import { defineStore } from 'pinia'
import { getStoredToken } from '@/api/client'
import type { MediaKind } from '@/types/api'

export interface RealtimeSystemEvent {
  event:
    | 'library_scan_started'
    | 'library_scan_progress'
    | 'library_scan_completed'
    | 'library_scan_failed'
    | 'media_changed'
  library_id?: string
  media_kind: MediaKind
  scanned?: number
  added?: number
  updated?: number
  missing?: number
  message?: string
}

export const useEventsStore = defineStore('events', () => {
  const activeScans = ref<Record<string, { media_kind: MediaKind; scanned: number }>>({})
  const changeTick = ref<Record<MediaKind, number>>({
    book: 0,
    comic: 0,
    music: 0,
    video: 0,
  })
  const lastToast = ref('')

  let ws: WebSocket | null = null
  let reconnectTimer: ReturnType<typeof setTimeout> | null = null

  function connect(): void {
    const token = getStoredToken()
    if (!token || ws) return

    const proto = window.location.protocol === 'https:' ? 'wss:' : 'ws:'
    const url = `${proto}//${window.location.host}/api/v1/events/ws?token=${encodeURIComponent(token)}`
    ws = new WebSocket(url)

    ws.onmessage = (ev) => {
      try {
        const msg = JSON.parse(String(ev.data)) as RealtimeSystemEvent
        if (msg.event === 'library_scan_started' && msg.library_id) {
          activeScans.value[msg.library_id] = { media_kind: msg.media_kind, scanned: 0 }
        } else if (msg.event === 'library_scan_progress' && msg.library_id) {
          activeScans.value[msg.library_id] = {
            media_kind: msg.media_kind,
            scanned: msg.scanned ?? 0,
          }
        } else if (msg.event === 'library_scan_completed' && msg.library_id) {
          delete activeScans.value[msg.library_id]
          changeTick.value[msg.media_kind] += 1
          lastToast.value = `[${msg.media_kind.toUpperCase()}] Scan done: +${msg.added ?? 0} / ~${msg.updated ?? 0}`
        } else if (msg.event === 'library_scan_failed' && msg.library_id) {
          delete activeScans.value[msg.library_id]
        } else if (msg.event === 'media_changed') {
          changeTick.value[msg.media_kind] += 1
        }
      } catch {
        // ignore malformed frame
      }
    }

    ws.onclose = () => {
      ws = null
      if (getStoredToken()) {
        reconnectTimer = setTimeout(connect, 3500)
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
    changeTick,
    lastToast,
    connect,
    disconnect,
  }
})

import type { ComputedRef, Ref } from 'vue'
import { useAuthStore } from '@/stores/auth'
import { api } from '../api'
import { makeLocalQueueItem, type LocalQueueItem, type PlayerQueueItem } from './queueHelpers'

export function createPlayerCloudSync(options: {
  queue: Ref<PlayerQueueItem[]>
  currentIndex: Ref<number>
  currentItem: ComputedRef<PlayerQueueItem | null>
  currentTimeSecs: Ref<number>
  onRestoredQueue: (resumeSecs: number) => Promise<void>
}) {
  let queueEpoch = 0
  let queueDirty = false
  let queueHadLocalOnClear = false
  let queueSyncTimer: ReturnType<typeof setTimeout> | null = null
  let restoreQueueAbort: AbortController | null = null

  function clearSyncTimer(): void {
    if (queueSyncTimer) {
      clearTimeout(queueSyncTimer)
      queueSyncTimer = null
    }
  }

  function bumpEpoch(): void {
    queueEpoch += 1
  }

  function resetClearFlag(): void {
    queueHadLocalOnClear = false
  }

  function markClearedWithLocal(hadLocal: boolean): void {
    queueHadLocalOnClear = hadLocal
  }

  function markRemovedItem(removedItem: PlayerQueueItem | undefined): void {
    if (removedItem?.kind === 'local' && !options.queue.value.some((i) => i.kind === 'local')) {
      queueHadLocalOnClear = true
    }
  }

  function buildLocalQueueSyncPayload(): {
    track_ids: string[]
    current_track_id: string | null
    position_ms: number
  } | null {
    const localItems = options.queue.value.filter((i): i is LocalQueueItem => i.kind === 'local')
    if (localItems.length === 0) {
      if (queueHadLocalOnClear) {
        queueHadLocalOnClear = false
        return {
          track_ids: [],
          current_track_id: null,
          position_ms: 0,
        }
      }
      return null
    }
    queueHadLocalOnClear = false
    const cur = options.currentItem.value
    return {
      track_ids: localItems.map((i) => i.track.id),
      current_track_id: cur?.kind === 'local' ? cur.track.id : (localItems[0]?.track.id ?? null),
      position_ms: cur?.kind === 'local' ? Math.round(options.currentTimeSecs.value * 1000) : 0,
    }
  }

  function scheduleQueueSync(): void {
    if (!useAuthStore().isAuthenticated) return
    const hasLocal = options.queue.value.some((i) => i.kind === 'local')
    if (!hasLocal && !queueHadLocalOnClear) {
      return
    }
    queueDirty = true
    clearSyncTimer()
    const epoch = queueEpoch
    queueSyncTimer = setTimeout(() => {
      queueSyncTimer = null
      if (!useAuthStore().isAuthenticated || epoch !== queueEpoch) return
      queueDirty = false
      const payload = buildLocalQueueSyncPayload()
      if (!payload) return
      void api.savePlayQueue(payload).catch(() => {})
    }, 800)
  }

  function flushQueueOnPageHide(): void {
    if (!useAuthStore().isAuthenticated) return
    if (!queueDirty && options.queue.value.length === 0) return
    queueDirty = false
    clearSyncTimer()
    const payload = buildLocalQueueSyncPayload()
    if (!payload) return
    api.savePlayQueueKeepalive(payload)
  }

  async function restoreRemoteQueue(force = false): Promise<void> {
    if (!force && options.queue.value.length > 0) return
    restoreQueueAbort?.abort()
    const controller = new AbortController()
    restoreQueueAbort = controller
    const epoch = queueEpoch
    try {
      const saved = await api.getPlayQueue({ signal: controller.signal })
      if (controller.signal.aborted || epoch !== queueEpoch) return
      if (!force && options.queue.value.length > 0) return
      if (!saved || saved.tracks.length === 0) return
      queueHadLocalOnClear = false
      options.queue.value = saved.tracks.map(makeLocalQueueItem)
      const foundIdx = saved.current_track_id
        ? saved.tracks.findIndex((t) => t.id === saved.current_track_id)
        : 0
      options.currentIndex.value = foundIdx >= 0 ? foundIdx : 0
      const resumeSecs = Math.max(0, (saved.position_ms || 0) / 1000)
      await options.onRestoredQueue(resumeSecs)
    } catch {
      // ignore restore failures cleanly
    } finally {
      if (restoreQueueAbort === controller) {
        restoreQueueAbort = null
      }
    }
  }

  function resetCloudSync(): void {
    queueEpoch += 1
    restoreQueueAbort?.abort()
    restoreQueueAbort = null
    queueDirty = false
    queueHadLocalOnClear = false
    clearSyncTimer()
  }

  if (typeof window !== 'undefined') {
    window.addEventListener('pagehide', flushQueueOnPageHide)
  }

  function dispose(): void {
    if (typeof window !== 'undefined') {
      window.removeEventListener('pagehide', flushQueueOnPageHide)
    }
    clearSyncTimer()
    restoreQueueAbort?.abort()
    restoreQueueAbort = null
  }

  return {
    bumpEpoch,
    resetClearFlag,
    markClearedWithLocal,
    markRemovedItem,
    scheduleQueueSync,
    restoreRemoteQueue,
    resetCloudSync,
    dispose,
  }
}

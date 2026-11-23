import { onBeforeUnmount, onMounted, reactive, ref, watch } from 'vue'
import { useRoute } from 'vue-router'
import { activityApi, formatLocalDate } from '@/api/activity'
import { usePlayerStore } from '@/domains/music/player'
import { useAuthStore } from '@/stores/auth'
import type { MediaKind } from '@/types/api'

const FLUSH_INTERVAL_SECS = 30
const BROWSE_IDLE_MS = 180_000
const IMMERSIVE_IDLE_MS = 300_000
const MAX_DELTA_PER_HEARTBEAT = 300

const DOMAINS: readonly MediaKind[] = ['book', 'comic', 'music', 'video']

/** Live unflushed seconds accumulated in the current client session per domain (today). */
export const livePendingSeconds = reactive<Record<MediaKind, number>>({
  book: 0,
  comic: 0,
  music: 0,
  video: 0,
})

/** Increments whenever a heartbeat finishes flushing to the backend. */
export const activityFlushRevision = ref(0)

function isMediaKind(raw: unknown): raw is MediaKind {
  return raw === 'book' || raw === 'comic' || raw === 'music' || raw === 'video'
}

function hasActiveMediaPlayback(): boolean {
  if (typeof document === 'undefined') return false
  const mediaEls = document.querySelectorAll<HTMLMediaElement>('video, audio')
  for (const el of mediaEls) {
    if (!el.paused && !el.ended && el.readyState >= 2) {
      return true
    }
  }
  return false
}

/**
 * Tracks active time spent per media domain (browsing, reading, watching, listening)
 * and periodically flushes increments to `/api/v1/activity/heartbeat`.
 */
export function useActivityTracker(): void {
  const route = useRoute()
  const authStore = useAuthStore()
  const playerStore = usePlayerStore()

  let tickTimer: ReturnType<typeof setInterval> | null = null
  let lastInteractionAt = Date.now()
  let lastInteractionThrottleAt = 0
  let currentDate = formatLocalDate()

  const sessionMarked: Record<MediaKind, boolean> = {
    book: false,
    comic: false,
    music: false,
    video: false,
  }
  const flushing: Record<MediaKind, boolean> = {
    book: false,
    comic: false,
    music: false,
    video: false,
  }

  function recordInteraction(): void {
    const now = Date.now()
    if (now - lastInteractionThrottleAt < 1000) return
    lastInteractionThrottleAt = now
    lastInteractionAt = now
  }

  function resolveItemTitle(domain: MediaKind): string | undefined {
    if (domain === 'music' && playerStore.currentTrack?.title) {
      return playerStore.currentTrack.title.slice(0, 120)
    }
    if (typeof document === 'undefined') return undefined
    const raw = document.title.trim()
    if (!raw || raw.startsWith('xmedia')) return undefined
    return raw.replace(/\s*[—|-]\s*xmedia.*$/i, '').trim().slice(0, 120) || undefined
  }

  async function flushDomain(
    domain: MediaKind,
    options?: { keepalive?: boolean; dateOverride?: string },
  ): Promise<void> {
    if (!authStore.isAuthenticated) {
      livePendingSeconds[domain] = 0
      return
    }
    const pending = Math.floor(livePendingSeconds[domain])
    if (pending <= 0) return
    if (flushing[domain] && !options?.keepalive) return

    const toSend = Math.min(pending, MAX_DELTA_PER_HEARTBEAT)
    const isNewSession = !sessionMarked[domain]
    const dateStr = options?.dateOverride || currentDate
    livePendingSeconds[domain] = Math.max(0, livePendingSeconds[domain] - toSend)
    sessionMarked[domain] = true
    flushing[domain] = true

    try {
      await activityApi.recordHeartbeat(
        {
          media_kind: domain,
          date: dateStr,
          delta_secs: toSend,
          new_session: isNewSession,
          item_title: resolveItemTitle(domain),
        },
        { keepalive: options?.keepalive },
      )
      activityFlushRevision.value += 1
    } catch {
      // Restore pending seconds so transient network errors do not drop tracked time
      livePendingSeconds[domain] = Math.min(
        MAX_DELTA_PER_HEARTBEAT,
        livePendingSeconds[domain] + toSend,
      )
      if (isNewSession) {
        sessionMarked[domain] = false
      }
    } finally {
      flushing[domain] = false
    }
  }

  function flushAll(options?: { keepalive?: boolean; dateOverride?: string }): void {
    for (const d of DOMAINS) {
      if (livePendingSeconds[d] > 0) {
        void flushDomain(d, options)
      }
    }
  }

  function onTick(): void {
    if (!authStore.isAuthenticated) return

    const today = formatLocalDate()
    if (today !== currentDate) {
      const previousDate = currentDate
      flushAll({ dateOverride: previousDate })
      currentDate = today
      for (const d of DOMAINS) {
        sessionMarked[d] = false
      }
    }

    const isVisible = typeof document !== 'undefined' && document.visibilityState === 'visible'
    const routeDomain = isMediaKind(route.meta.domain) ? route.meta.domain : null
    const isFullscreenMedia = Boolean(route.meta.fullscreen)
    const musicPlaying = Boolean(playerStore.isPlaying)
    const idleLimit = isFullscreenMedia ? IMMERSIVE_IDLE_MS : BROWSE_IDLE_MS
    const isRecentlyActive = Date.now() - lastInteractionAt <= idleLimit
    const mediaPlaying = musicPlaying || hasActiveMediaPlayback()

    if (isVisible && routeDomain && (isRecentlyActive || mediaPlaying)) {
      livePendingSeconds[routeDomain] += 1
      if (livePendingSeconds[routeDomain] >= FLUSH_INTERVAL_SECS) {
        void flushDomain(routeDomain)
      }
    }

    // Credit background music listening even while browsing another domain or hidden tab
    if (musicPlaying && (!isVisible || routeDomain !== 'music')) {
      livePendingSeconds.music += 1
      if (livePendingSeconds.music >= FLUSH_INTERVAL_SECS) {
        void flushDomain('music')
      }
    }
  }

  function handleVisibilityChange(): void {
    if (document.visibilityState === 'hidden') {
      flushAll({ keepalive: true })
    } else {
      lastInteractionAt = Date.now()
    }
  }

  function handlePageHide(): void {
    flushAll({ keepalive: true })
  }

  watch(
    () => route.meta.domain,
    (next, prev) => {
      if (isMediaKind(prev) && prev !== next && livePendingSeconds[prev] > 0) {
        void flushDomain(prev)
      }
      lastInteractionAt = Date.now()
    },
  )

  watch(
    () => authStore.user?.id,
    (uid, prevUid) => {
      if (!uid || uid !== prevUid) {
        for (const d of DOMAINS) {
          livePendingSeconds[d] = 0
          sessionMarked[d] = false
        }
      }
    },
  )

  onMounted(() => {
    lastInteractionAt = Date.now()
    window.addEventListener('pointerdown', recordInteraction, { passive: true })
    window.addEventListener('keydown', recordInteraction, { passive: true })
    window.addEventListener('wheel', recordInteraction, { passive: true })
    window.addEventListener('touchstart', recordInteraction, { passive: true })
    document.addEventListener('visibilitychange', handleVisibilityChange)
    window.addEventListener('pagehide', handlePageHide)
    tickTimer = setInterval(onTick, 1000)
  })

  onBeforeUnmount(() => {
    flushAll({ keepalive: true })
    if (tickTimer !== null) {
      clearInterval(tickTimer)
      tickTimer = null
    }
    window.removeEventListener('pointerdown', recordInteraction)
    window.removeEventListener('keydown', recordInteraction)
    window.removeEventListener('wheel', recordInteraction)
    window.removeEventListener('touchstart', recordInteraction)
    document.removeEventListener('visibilitychange', handleVisibilityChange)
    window.removeEventListener('pagehide', handlePageHide)
  })
}

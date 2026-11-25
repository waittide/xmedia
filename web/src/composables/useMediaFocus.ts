import { getCurrentScope, onScopeDispose, readonly, ref, type DeepReadonly, type Ref } from 'vue'

export type MediaFocusSource = 'music' | 'video' | 'book' | 'comic' | (string & {})

export const CLAIM_AUDIO_FOCUS_EVENT = 'xmedia:claim-audio-focus'

interface FocusClaimDetail {
  source?: string
}

const participants = new Map<string, Set<() => void>>()
const activeSourceRef = ref<string | null>(null)
let windowBridgeBound = false
let dispatchingEvent = false

function ensureWindowBridge(): void {
  if (windowBridgeBound || typeof window === 'undefined') return
  windowBridgeBound = true
  window.addEventListener(CLAIM_AUDIO_FOCUS_EVENT, (ev: Event) => {
    if (dispatchingEvent) return
    const source = (ev as CustomEvent<FocusClaimDetail>).detail?.source?.trim()
    if (!source) return
    claimMediaFocus(source, { broadcastEvent: false })
  })
}

export function registerMediaFocus(source: MediaFocusSource, onPause: () => void): () => void {
  ensureWindowBridge()
  let bucket = participants.get(source)
  if (!bucket) {
    bucket = new Set()
    participants.set(source, bucket)
  }
  bucket.add(onPause)

  const unregister = (): void => {
    const current = participants.get(source)
    if (!current) return
    current.delete(onPause)
    if (current.size === 0) {
      participants.delete(source)
      if (activeSourceRef.value === source) {
        activeSourceRef.value = null
      }
    }
  }

  if (getCurrentScope()) {
    onScopeDispose(unregister)
  }

  return unregister
}

export function claimMediaFocus(
  source: MediaFocusSource,
  options: { broadcastEvent?: boolean } = {},
): void {
  ensureWindowBridge()
  activeSourceRef.value = source

  for (const [participantSource, callbacks] of participants.entries()) {
    if (participantSource === source) continue
    for (const pauseFn of callbacks) {
      try {
        pauseFn()
      } catch {
        // ignore participant pause errors
      }
    }
  }

  if (options.broadcastEvent !== false && typeof window !== 'undefined' && !dispatchingEvent) {
    dispatchingEvent = true
    try {
      window.dispatchEvent(
        new CustomEvent<FocusClaimDetail>(CLAIM_AUDIO_FOCUS_EVENT, {
          detail: { source },
        }),
      )
    } finally {
      dispatchingEvent = false
    }
  }
}

export function releaseMediaFocus(source: MediaFocusSource): void {
  if (activeSourceRef.value === source) {
    activeSourceRef.value = null
  }
}

export function pauseAllMediaPlayers(): void {
  activeSourceRef.value = null
  for (const callbacks of participants.values()) {
    for (const pauseFn of callbacks) {
      try {
        pauseFn()
      } catch {
        // ignore participant pause errors
      }
    }
  }
}

export interface UseMediaFocusReturn {
  activeSource: DeepReadonly<Ref<string | null>>
  register: (source: MediaFocusSource, onPause: () => void) => () => void
  claim: (source: MediaFocusSource) => void
  release: (source: MediaFocusSource) => void
  pauseAll: () => void
}

export function useMediaFocus(): UseMediaFocusReturn {
  ensureWindowBridge()
  return {
    activeSource: readonly(activeSourceRef),
    register: registerMediaFocus,
    claim: (source) => claimMediaFocus(source),
    release: releaseMediaFocus,
    pauseAll: pauseAllMediaPlayers,
  }
}

import type { Ref } from 'vue'
import type Hls from 'hls.js'
import type { SelectOption } from '@/components/ui'

const VOLUME_STORAGE_KEY = 'xmedia.video.volume'
const MUTED_STORAGE_KEY = 'xmedia.video.muted'

export const VIDEO_SPEED_OPTIONS: readonly SelectOption[] = [
  { value: '0.75', label: '0.75x' },
  { value: '1', label: '1.0x' },
  { value: '1.25', label: '1.25x' },
  { value: '1.5', label: '1.5x' },
  { value: '2', label: '2.0x' },
]

export function loadStoredVideoVolume(): number {
  try {
    const raw = window.localStorage.getItem(VOLUME_STORAGE_KEY)
    if (raw === null) return 0.95
    const parsed = Number.parseFloat(raw)
    if (!Number.isFinite(parsed)) return 0.95
    return Math.max(0, Math.min(1, parsed))
  } catch {
    return 0.95
  }
}

export function loadStoredVideoMuted(): boolean {
  try {
    return window.localStorage.getItem(MUTED_STORAGE_KEY) === 'true'
  } catch {
    return false
  }
}

export function persistVideoAudioPrefs(vol: number, isMuted: boolean): void {
  try {
    window.localStorage.setItem(VOLUME_STORAGE_KEY, String(vol))
    window.localStorage.setItem(MUTED_STORAGE_KEY, String(isMuted))
  } catch {
    // ignore storage quota / privacy mode errors
  }
}

function setMediaSessionAction(
  action: MediaSessionAction,
  handler: MediaSessionActionHandler | null,
): void {
  if (typeof navigator === 'undefined' || !('mediaSession' in navigator) || !navigator.mediaSession) {
    return
  }
  try {
    navigator.mediaSession.setActionHandler(action, handler)
  } catch {
    // ignore unsupported MediaSession actions
  }
}

export function setVideoMediaSessionState(state: MediaSessionPlaybackState): void {
  if (typeof navigator === 'undefined' || !('mediaSession' in navigator) || !navigator.mediaSession) {
    return
  }
  try {
    navigator.mediaSession.playbackState = state
  } catch {
    // ignore
  }
}

export interface VideoMediaSessionParams {
  title: string
  artist?: string
  album?: string
  posterUrl?: string | null
  onPlay: () => void
  onPause: () => void
  onSeekBy: (deltaSecs: number) => void
  onSeekTo: (targetSecs: number) => void
  onPrev?: (() => void) | null
  onNext?: (() => void) | null
}

export function updateVideoMediaSession(params: VideoMediaSessionParams): void {
  if (typeof navigator === 'undefined' || !('mediaSession' in navigator) || !navigator.mediaSession) {
    return
  }
  if (typeof MediaMetadata !== 'undefined') {
    const artwork: MediaImage[] = []
    if (params.posterUrl) {
      try {
        artwork.push({
          src: new URL(params.posterUrl, window.location.origin).href,
          sizes: '512x512',
        })
      } catch {
        // ignore invalid poster url
      }
    }
    try {
      navigator.mediaSession.metadata = new MediaMetadata({
        title: params.title,
        artist: params.artist,
        album: params.album,
        artwork,
      })
    } catch {
      // ignore invalid metadata errors
    }
  }

  setMediaSessionAction('play', params.onPlay)
  setMediaSessionAction('pause', params.onPause)
  setMediaSessionAction('seekbackward', (details) => {
    const offset = details?.seekOffset && details.seekOffset > 0 ? details.seekOffset : 10
    params.onSeekBy(-offset)
  })
  setMediaSessionAction('seekforward', (details) => {
    const offset = details?.seekOffset && details.seekOffset > 0 ? details.seekOffset : 10
    params.onSeekBy(offset)
  })
  setMediaSessionAction('seekto', (details) => {
    if (typeof details?.seekTime === 'number') {
      params.onSeekTo(details.seekTime)
    }
  })
  setMediaSessionAction('previoustrack', params.onPrev ?? null)
  setMediaSessionAction('nexttrack', params.onNext ?? null)
}

export function clearVideoMediaSession(): void {
  if (typeof navigator === 'undefined' || !('mediaSession' in navigator) || !navigator.mediaSession) {
    return
  }
  const actions: MediaSessionAction[] = [
    'play',
    'pause',
    'seekbackward',
    'seekforward',
    'seekto',
    'previoustrack',
    'nexttrack',
  ]
  for (const action of actions) {
    setMediaSessionAction(action, null)
  }
  try {
    navigator.mediaSession.metadata = null
  } catch {
    // ignore
  }
  setVideoMediaSessionState('none')
}

export interface AttachVideoStreamOptions {
  url: string
  isHls: boolean
  startSecs: number
  playbackRate: number
  isStale: () => boolean
  onSeekApplied?: (targetSecs: number) => void
  onManifestReady?: () => void
  onPlayRejected?: () => void
  onFatalError: (message: string) => void
  unsupportedHlsMessage: string
  defaultErrorMessage: string
}

export interface VideoMediaController {
  hasHls: () => boolean
  clearPendingSeek: () => void
  destroyHls: () => void
  stopAndClear: () => void
  attachDirectWithSeek: (
    url: string,
    opts: {
      startSecs: number
      playbackRate: number
      applySeekOnMetadata: boolean
      autoPlay: boolean
      onSeekApplied?: (targetSecs: number) => void
      onPlayRejected?: () => void
    },
  ) => void
  attachStream: (opts: AttachVideoStreamOptions) => Promise<void>
}

export function createVideoMediaController(
  videoEl: Ref<HTMLVideoElement | null>,
): VideoMediaController {
  let hlsInstance: Hls | null = null
  let pendingLoadedMetadataHandler: (() => void) | null = null

  function clearPendingSeek(): void {
    if (pendingLoadedMetadataHandler && videoEl.value) {
      videoEl.value.removeEventListener('loadedmetadata', pendingLoadedMetadataHandler)
    }
    pendingLoadedMetadataHandler = null
  }

  function destroyHls(): void {
    if (hlsInstance) {
      try {
        hlsInstance.destroy()
      } catch {
        // ignore destroy errors
      }
      hlsInstance = null
    }
  }

  function stopAndClear(): void {
    clearPendingSeek()
    const el = videoEl.value
    if (el) {
      try {
        el.pause()
        el.removeAttribute('src')
        el.load()
      } catch {
        // ignore cleanup errors
      }
    }
    destroyHls()
  }

  function attachSeekHandler(
    el: HTMLVideoElement,
    startSecs: number,
    playbackRate: number,
    applySeek: boolean,
    onSeekApplied?: (targetSecs: number) => void,
  ): void {
    clearPendingSeek()
    const handler = (): void => {
      clearPendingSeek()
      el.playbackRate = playbackRate
      if (applySeek && startSecs > 0) {
        const maxDur = Number.isFinite(el.duration) && el.duration > 0 ? el.duration : startSecs
        const target = Math.min(startSecs, Math.max(0, maxDur - 1))
        if (target > 0) {
          el.currentTime = target
          onSeekApplied?.(target)
        }
      }
    }
    pendingLoadedMetadataHandler = handler
    el.addEventListener('loadedmetadata', handler, { once: true })
  }

  function attachDirectWithSeek(
    url: string,
    opts: {
      startSecs: number
      playbackRate: number
      applySeekOnMetadata: boolean
      autoPlay: boolean
      onSeekApplied?: (targetSecs: number) => void
      onPlayRejected?: () => void
    },
  ): void {
    const el = videoEl.value
    if (!el) return
    clearPendingSeek()
    destroyHls()
    el.src = url
    el.load()
    attachSeekHandler(
      el,
      opts.startSecs,
      opts.playbackRate,
      opts.applySeekOnMetadata,
      opts.onSeekApplied,
    )
    el.playbackRate = opts.playbackRate
    if (opts.autoPlay) {
      void el.play().catch(() => {
        opts.onPlayRejected?.()
      })
    }
  }

  async function attachStream(opts: AttachVideoStreamOptions): Promise<void> {
    const el = videoEl.value
    if (!el) return

    clearPendingSeek()
    destroyHls()

    if (!opts.isHls) {
      attachSeekHandler(el, opts.startSecs, opts.playbackRate, true, opts.onSeekApplied)
      el.src = opts.url
      el.load()
      void el.play().catch(() => {
        opts.onPlayRejected?.()
      })
      return
    }

    const { default: HlsCtor } = await import('hls.js')
    if (opts.isStale() || videoEl.value !== el) return

    if (HlsCtor.isSupported()) {
      let networkRetries = 0
      let mediaRetries = 0
      const unrecoverableManifestErrors = new Set<string>([
        HlsCtor.ErrorDetails.MANIFEST_PARSING_ERROR,
        HlsCtor.ErrorDetails.MANIFEST_INCOMPATIBLE_CODECS_ERROR,
        HlsCtor.ErrorDetails.LEVEL_EMPTY_ERROR,
      ])
      const manifestLoadErrors = new Set<string>([
        HlsCtor.ErrorDetails.MANIFEST_LOAD_ERROR,
        HlsCtor.ErrorDetails.MANIFEST_LOAD_TIMEOUT,
      ])
      const hls = new HlsCtor({
        startPosition: opts.startSecs > 0 ? opts.startSecs : -1,
        xhrSetup: (xhr) => {
          xhr.withCredentials = true
        },
      })
      hlsInstance = hls
      hls.on(HlsCtor.Events.MANIFEST_PARSED, () => {
        if (opts.isStale()) return
        el.playbackRate = opts.playbackRate
        if (opts.startSecs > 0 && Math.abs((el.currentTime || 0) - opts.startSecs) > 1.5) {
          el.currentTime = opts.startSecs
        }
        opts.onManifestReady?.()
        void el.play().catch(() => {
          opts.onPlayRejected?.()
        })
      })
      hls.on(HlsCtor.Events.ERROR, (_event, data) => {
        if (opts.isStale() || !data.fatal) return
        if (!unrecoverableManifestErrors.has(data.details)) {
          if (data.type === HlsCtor.ErrorTypes.NETWORK_ERROR && networkRetries < 1) {
            networkRetries += 1
            if (manifestLoadErrors.has(data.details) || hls.levels.length === 0) {
              hls.loadSource(opts.url)
            } else {
              hls.startLoad()
            }
            return
          }
          if (data.type === HlsCtor.ErrorTypes.MEDIA_ERROR && mediaRetries < 1) {
            mediaRetries += 1
            hls.recoverMediaError()
            return
          }
        }
        stopAndClear()
        opts.onFatalError(data.reason || opts.defaultErrorMessage)
      })
      hls.loadSource(opts.url)
      hls.attachMedia(el)
    } else if (el.canPlayType('application/vnd.apple.mpegurl') !== '') {
      attachSeekHandler(el, opts.startSecs, opts.playbackRate, true, opts.onSeekApplied)
      el.src = opts.url
      el.load()
      void el.play().catch(() => {
        opts.onPlayRejected?.()
      })
    } else {
      opts.onFatalError(opts.unsupportedHlsMessage)
    }
  }

  return {
    hasHls: () => Boolean(hlsInstance),
    clearPendingSeek,
    destroyHls,
    stopAndClear,
    attachDirectWithSeek,
    attachStream,
  }
}

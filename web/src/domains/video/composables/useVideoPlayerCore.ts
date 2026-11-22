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

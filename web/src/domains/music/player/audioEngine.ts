import { buildCoverUrl } from '../api'
import type { PlayerTrackDisplay } from './queueHelpers'

export interface MediaSessionActions {
  onPlayPause: () => void
  onPrev: () => void
  onNext: () => void
}

export interface PlayerAudioEngineCallbacks {
  onTimeUpdate: (audio: HTMLAudioElement) => void
  onDurationChange: (duration: number) => void
  onPlay: () => void
  onPause: () => void
  onEnded: () => void
  onError: (audio: HTMLAudioElement) => void
}

export function createPlayerAudioEngine(callbacks: PlayerAudioEngineCallbacks) {
  const audio = new Audio()
  audio.preload = 'auto'
  audio.hidden = true
  audio.setAttribute('aria-hidden', 'true')
  audio.dataset.musicPlayerAudio = '1'

  let pendingSeekHandler: (() => void) | null = null

  function clearPendingSeek(): void {
    if (pendingSeekHandler) {
      audio.removeEventListener('loadedmetadata', pendingSeekHandler)
      pendingSeekHandler = null
    }
  }

  function scheduleMetadataSeek(startAtSecs: number, shouldApply: () => boolean): void {
    clearPendingSeek()
    if (startAtSecs <= 0) return
    const handler = (): void => {
      clearPendingSeek()
      if (shouldApply()) {
        audio.currentTime = startAtSecs
      }
    }
    pendingSeekHandler = handler
    audio.addEventListener('loadedmetadata', handler, { once: true })
  }

  function attachAudioElement(host: HTMLElement | null): void {
    if (!host) return
    if (audio.parentElement !== host) {
      host.appendChild(audio)
    }
  }

  function detachAudioElement(host?: HTMLElement | null): void {
    if (!audio.parentElement) return
    if (host && audio.parentElement !== host) return
    audio.parentElement.removeChild(audio)
  }

  function syncGain(volume: number, muted: boolean): void {
    audio.volume = Math.max(0, Math.min(1, volume))
    audio.muted = muted
  }

  function resetSource(reload = false): void {
    clearPendingSeek()
    audio.pause()
    audio.removeAttribute('src')
    if (reload) {
      audio.load()
    }
  }

  function updateMediaSession(
    track: PlayerTrackDisplay | null,
    actions: MediaSessionActions,
  ): void {
    if (!('mediaSession' in navigator) || typeof MediaMetadata === 'undefined') return
    try {
      if (!track) {
        navigator.mediaSession.metadata = null
        navigator.mediaSession.setActionHandler('play', null)
        navigator.mediaSession.setActionHandler('pause', null)
        navigator.mediaSession.setActionHandler('previoustrack', null)
        navigator.mediaSession.setActionHandler('nexttrack', null)
        return
      }
      const relCover =
        track.kind === 'local'
          ? track.cover_key && track.cover_id
            ? buildCoverUrl(track.cover_id, 512)
            : null
          : track.cover_url
      const absCover = relCover
        ? relCover.startsWith('data:')
          ? relCover
          : new URL(relCover, window.location.origin).href
        : null
      navigator.mediaSession.metadata = new MediaMetadata({
        title: track.title,
        artist: track.artist_name,
        album: track.album_title || undefined,
        artwork: absCover ? [{ src: absCover, sizes: '512x512' }] : [],
      })
      navigator.mediaSession.setActionHandler('play', actions.onPlayPause)
      navigator.mediaSession.setActionHandler('pause', actions.onPlayPause)
      navigator.mediaSession.setActionHandler('previoustrack', actions.onPrev)
      navigator.mediaSession.setActionHandler('nexttrack', actions.onNext)
    } catch {
      // Ignore MediaSession errors on restricted/non-HTTPS contexts
    }
  }

  const handleAudioTimeUpdate = (): void => {
    callbacks.onTimeUpdate(audio)
  }

  const handleAudioDurationChange = (): void => {
    callbacks.onDurationChange(audio.duration || 0)
  }

  const handleAudioPlay = (): void => {
    callbacks.onPlay()
  }

  const handleAudioPause = (): void => {
    callbacks.onPause()
  }

  const handleAudioEnded = (): void => {
    callbacks.onEnded()
  }

  const handleAudioError = (): void => {
    callbacks.onError(audio)
  }

  audio.addEventListener('timeupdate', handleAudioTimeUpdate)
  audio.addEventListener('durationchange', handleAudioDurationChange)
  audio.addEventListener('play', handleAudioPlay)
  audio.addEventListener('pause', handleAudioPause)
  audio.addEventListener('ended', handleAudioEnded)
  audio.addEventListener('error', handleAudioError)

  function dispose(): void {
    clearPendingSeek()
    audio.removeEventListener('timeupdate', handleAudioTimeUpdate)
    audio.removeEventListener('durationchange', handleAudioDurationChange)
    audio.removeEventListener('play', handleAudioPlay)
    audio.removeEventListener('pause', handleAudioPause)
    audio.removeEventListener('ended', handleAudioEnded)
    audio.removeEventListener('error', handleAudioError)
    audio.pause()
    audio.removeAttribute('src')
    detachAudioElement()
  }

  return {
    audio,
    attachAudioElement,
    detachAudioElement,
    syncGain,
    clearPendingSeek,
    scheduleMetadataSeek,
    resetSource,
    updateMediaSession,
    dispose,
  }
}

import type { ComputedRef, Ref } from 'vue'
import { useSnackbarStore } from '@/stores/snackbar'
import { api, buildStreamUrl, canPlayFormatNatively } from '../api'
import { onlineMusicApi } from '../onlineApi'
import { buildRemoteSubsonicStreamUrl } from '../onlineTrackRef'
import { useMusicStore } from '../store'
import type { TranscodeProfile } from '../types'
import type { createPlayerAudioEngine } from './audioEngine'
import type { createPlayerCloudSync } from './cloudSync'
import type { createLyricsSyncController } from './lyricsSync'
import {
  isOnlineItemExpired,
  isTranscodeProfile,
  pickDefaultOnlineQuality,
  resolveOnlineItemStreamUrl,
  type OnlineQueueItem,
  type PlayerQueueItem,
  type PlayerTrackDisplay,
} from './queueHelpers'

export interface CreatePlayerTrackLoaderDeps {
  audioEngine: ReturnType<typeof createPlayerAudioEngine>
  lyricsCtrl: ReturnType<typeof createLyricsSyncController>
  cloudSync: ReturnType<typeof createPlayerCloudSync>
  currentItem: ComputedRef<PlayerQueueItem | null>
  currentOnlineItem: ComputedRef<OnlineQueueItem | null>
  currentTrack: ComputedRef<PlayerTrackDisplay | null>
  transcodeProfile: ComputedRef<TranscodeProfile>
  isPlaying: Ref<boolean>
  currentTimeSecs: Ref<number>
  rawAudioDuration: Ref<number>
  bufferedRatio: Ref<number>
  streamOffsetSecs: Ref<number>
  currentStreamIsTranscoded: Ref<boolean>
  activeEffectiveProfile: Ref<TranscodeProfile>
  fallbackFormats: Ref<Set<string>>
  onlineResolving: Ref<boolean>
  playbackError: Ref<string | null>
  trackingCurrentOnline: Ref<boolean>
  trackedOnlineKeys: Ref<Record<string, boolean>>
  syncAudioGain: () => void
  refreshMediaSession: (track: PlayerTrackDisplay | null) => void
  tMusic: (key: string, params?: Record<string, unknown>) => string
}

export function createPlayerTrackLoader(deps: CreatePlayerTrackLoaderDeps) {
  let loadSeq = 0
  let trackResolveAbort: AbortController | null = null

  function abortTrackResolve(): void {
    trackResolveAbort?.abort()
    trackResolveAbort = null
  }

  function invalidateLoad(): void {
    loadSeq += 1
    abortTrackResolve()
  }

  async function trackCurrentOnlineToLibrary(): Promise<void> {
    const item = deps.currentOnlineItem.value
    if (!item || deps.trackingCurrentOnline.value) return
    deps.trackingCurrentOnline.value = true
    try {
      const tr = item.resolved?.track
        ? {
            ...item.track,
            ...item.resolved.track,
            cover_url:
              item.resolved.cover_url || item.resolved.track.cover_url || item.track.cover_url,
          }
        : item.track
      await onlineMusicApi.trackOnlineTrack({
        source_id: tr.source_id,
        track_id: tr.track_id,
        title: tr.title,
        artist: tr.artist,
        album: tr.album,
        duration_secs: tr.duration_secs,
        cover_url: tr.cover_url,
        cover_id: tr.cover_id,
        url_id: tr.url_id,
        lyric_id: tr.lyric_id,
        play_url: tr.play_url,
        detail_url: tr.detail_url,
        format: tr.format,
        quality: item.selectedQuality ?? tr.qualities?.[0] ?? null,
      })
      deps.trackedOnlineKeys.value = { ...deps.trackedOnlineKeys.value, [item.key]: true }
      const musicStore = useMusicStore()
      musicStore.bumpCatalogVersion()
      void musicStore.refreshSidebar()
      useSnackbarStore().show(deps.tMusic('online.trackedToast', { title: tr.title }))
    } catch (err) {
      useSnackbarStore().show({
        message: err instanceof Error ? err.message : String(err),
        tone: 'error',
      })
    } finally {
      deps.trackingCurrentOnline.value = false
    }
  }

  async function loadAndPlayCurrent(
    startAtSecs = 0,
    autoPlay = true,
    overrideProfileOrQuality?: TranscodeProfile | string,
  ): Promise<void> {
    const item = deps.currentItem.value
    if (!item) return
    abortTrackResolve()
    const seq = ++loadSeq
    const targetKey = item.key
    deps.lyricsCtrl.prepareForTrackSwitch(targetKey)
    const { audio } = deps.audioEngine

    if (item.kind === 'local') {
      deps.onlineResolving.value = false
      deps.playbackError.value = null
      const track = item.track
      const overrideProfile = isTranscodeProfile(overrideProfileOrQuality)
        ? overrideProfileOrQuality
        : undefined

      deps.syncAudioGain()
      const requestedProfile = overrideProfile ?? deps.transcodeProfile.value
      const formatKey = track.format.toLowerCase()
      const profile =
        !overrideProfile && requestedProfile === 'raw' && deps.fallbackFormats.value.has(formatKey)
          ? 'mp3320'
          : requestedProfile
      deps.activeEffectiveProfile.value = profile
      const needsTranscode = profile !== 'raw' || !canPlayFormatNatively(track.format)
      deps.currentStreamIsTranscoded.value = needsTranscode
      const useServerOffset = needsTranscode && startAtSecs > 0
      deps.streamOffsetSecs.value = useServerOffset ? Math.floor(startAtSecs) : 0
      deps.currentTimeSecs.value = startAtSecs
      deps.rawAudioDuration.value = 0
      deps.bufferedRatio.value = 0

      deps.audioEngine.clearPendingSeek()
      audio.src = buildStreamUrl(track.id, profile, deps.streamOffsetSecs.value, track.format)
      audio.load()

      if (!useServerOffset && startAtSecs > 0) {
        deps.audioEngine.scheduleMetadataSeek(
          startAtSecs,
          () => seq === loadSeq && deps.currentItem.value?.key === targetKey,
        )
      }

      deps.refreshMediaSession(deps.currentTrack.value)
      deps.lyricsCtrl.clearForPendingTrack(targetKey)
      if (deps.lyricsCtrl.lyricsOpen.value && !deps.lyricsCtrl.isLoadedFor(targetKey)) {
        void deps.lyricsCtrl.fetchLyricsFor(item)
      }

      if (autoPlay) {
        try {
          await audio.play()
          if (seq !== loadSeq) return
          void api.scrobble(track.id, false).catch(() => {})
        } catch (err) {
          if (seq !== loadSeq) return
          if (err instanceof DOMException && err.name === 'AbortError') return
          if (!overrideProfile && err instanceof DOMException && err.name === 'NotSupportedError') {
            deps.fallbackFormats.value.add(formatKey)
            await loadAndPlayCurrent(startAtSecs, true, 'mp3320')
            return
          }
          const msg =
            err instanceof Error && err.message ? err.message : deps.tMusic('player.playbackError')
          deps.playbackError.value = msg
          deps.isPlaying.value = false
          useSnackbarStore().show({ message: msg, tone: 'error' })
        }
      }
      if (seq === loadSeq) {
        deps.cloudSync.scheduleQueueSync()
      }
      return
    }

    if (item.kind === 'remote') {
      deps.onlineResolving.value = false
      deps.playbackError.value = null
      deps.syncAudioGain()
      deps.currentStreamIsTranscoded.value = false
      deps.streamOffsetSecs.value = 0
      deps.currentTimeSecs.value = startAtSecs
      deps.rawAudioDuration.value = 0
      deps.bufferedRatio.value = 0
      deps.audioEngine.clearPendingSeek()

      audio.src =
        item.song.stream_url || buildRemoteSubsonicStreamUrl(item.source_id, item.song.id)
      audio.load()

      if (startAtSecs > 0) {
        deps.audioEngine.scheduleMetadataSeek(
          startAtSecs,
          () => seq === loadSeq && deps.currentItem.value?.key === targetKey,
        )
      }

      deps.lyricsCtrl.markNoLyricsForKey(targetKey)
      deps.refreshMediaSession(deps.currentTrack.value)

      if (autoPlay) {
        try {
          await audio.play()
          if (seq !== loadSeq) return
        } catch (err) {
          if (seq !== loadSeq) return
          if (err instanceof DOMException && err.name === 'AbortError') return
          const msg = err instanceof Error ? err.message : String(err)
          deps.playbackError.value = msg
          deps.isPlaying.value = false
          useSnackbarStore().show({ message: msg, tone: 'error' })
        }
      }
      return
    }

    const overrideQuality =
      typeof overrideProfileOrQuality === 'string' ? overrideProfileOrQuality : undefined
    deps.syncAudioGain()
    deps.currentStreamIsTranscoded.value = false
    deps.streamOffsetSecs.value = 0
    deps.currentTimeSecs.value = startAtSecs
    deps.rawAudioDuration.value = 0
    deps.bufferedRatio.value = 0
    deps.onlineResolving.value = false
    deps.playbackError.value = null
    deps.audioEngine.resetSource(true)
    deps.lyricsCtrl.clearForPendingTrack(targetKey)
    deps.refreshMediaSession(deps.currentTrack.value)

    const isExpired = isOnlineItemExpired(item)
    const requestedQuality =
      overrideQuality ?? item.selectedQuality ?? pickDefaultOnlineQuality(item.track.qualities)
    const needsFullInfo = !item.resolved || isExpired
    const needsQualitySwitch =
      !needsFullInfo && overrideQuality !== undefined && overrideQuality !== item.selectedQuality

    if (needsFullInfo || needsQualitySwitch) {
      const resolveController = new AbortController()
      trackResolveAbort = resolveController
      deps.onlineResolving.value = true
      try {
        if (needsFullInfo) {
          const resolved = await onlineMusicApi.fetchTrackInfo(
            {
              source_id: item.source_id,
              track_id: item.track.track_id || null,
              detail_url: item.track.detail_url || null,
              quality: requestedQuality,
              title: item.track.title || null,
              artist: item.track.artist || null,
              album: item.track.album ?? null,
              duration_secs: item.track.duration_secs ?? null,
              cover_url: item.track.cover_url ?? null,
              cover_id: item.track.cover_id ?? null,
              url_id: item.track.url_id ?? null,
              lyric_id: item.track.lyric_id ?? null,
              play_url: item.track.play_url ?? null,
            },
            { signal: resolveController.signal },
          )
          if (
            resolveController.signal.aborted ||
            seq !== loadSeq ||
            deps.currentItem.value?.key !== targetKey
          ) {
            return
          }
          item.resolved = resolved
          item.resolvedAtMs = Date.now()
          item.selectedQuality = requestedQuality
          item.activeFormat = resolved.format ?? item.track.format
          item.activeBitrate = resolved.bitrate
          deps.lyricsCtrl.applyResolvedLyrics(targetKey, resolved.lyrics, resolved.translated_lyrics)
        } else if (needsQualitySwitch && item.resolved) {
          const streamRes = await onlineMusicApi.fetchStream(
            {
              source_id: item.source_id,
              track_id: item.resolved.track.track_id || item.track.track_id || null,
              url_id: item.resolved.track.url_id ?? item.track.url_id ?? null,
              play_url: item.resolved.track.play_url ?? item.track.play_url ?? null,
              quality: overrideQuality ?? null,
              detail_url: item.resolved.track.detail_url ?? item.track.detail_url ?? null,
              title: item.resolved.track.title || item.track.title || null,
              artist: item.resolved.track.artist || item.track.artist || null,
            },
            { signal: resolveController.signal },
          )
          if (
            resolveController.signal.aborted ||
            seq !== loadSeq ||
            deps.currentItem.value?.key !== targetKey
          ) {
            return
          }
          item.resolved = {
            ...item.resolved,
            stream_url: streamRes.stream_url,
            proxy_stream_url: streamRes.proxy_stream_url,
            format: streamRes.format ?? item.resolved.format,
            bitrate: streamRes.bitrate ?? item.resolved.bitrate,
            expires_in_secs: streamRes.expires_in_secs,
          }
          item.resolvedAtMs = Date.now()
          item.selectedQuality = overrideQuality ?? null
          item.activeFormat = streamRes.format ?? item.activeFormat
          item.activeBitrate = streamRes.bitrate ?? item.activeBitrate
        }
      } catch (err) {
        if (
          resolveController.signal.aborted ||
          seq !== loadSeq ||
          deps.currentItem.value?.key !== targetKey ||
          (err instanceof Error && err.name === 'AbortError')
        ) {
          return
        }
        const msg = err instanceof Error ? err.message : String(err)
        deps.playbackError.value = msg
        deps.isPlaying.value = false
        useSnackbarStore().show({ message: msg, tone: 'error' })
        return
      } finally {
        if (trackResolveAbort === resolveController) {
          trackResolveAbort = null
        }
        if (seq === loadSeq) {
          deps.onlineResolving.value = false
        }
      }
    } else if (item.resolved) {
      deps.lyricsCtrl.applyResolvedLyrics(
        targetKey,
        item.resolved.lyrics,
        item.resolved.translated_lyrics,
      )
    }

    if (seq !== loadSeq || deps.currentItem.value?.key !== targetKey) return

    const streamUrl = resolveOnlineItemStreamUrl(item)
    if (!streamUrl) {
      const msg = deps.tMusic('player.noPlayableStream')
      deps.playbackError.value = msg
      deps.isPlaying.value = false
      useSnackbarStore().show({ message: msg, tone: 'error' })
      return
    }

    audio.src = streamUrl
    audio.load()

    if (startAtSecs > 0) {
      deps.audioEngine.scheduleMetadataSeek(
        startAtSecs,
        () => seq === loadSeq && deps.currentItem.value?.key === targetKey,
      )
    }

    deps.refreshMediaSession(deps.currentTrack.value)

    if (autoPlay) {
      try {
        await audio.play()
        if (seq !== loadSeq) return
      } catch (err) {
        if (seq !== loadSeq) return
        if (err instanceof DOMException && err.name === 'AbortError') return
        const msg = err instanceof Error ? err.message : String(err)
        deps.playbackError.value = msg
        deps.isPlaying.value = false
      }
    }
  }

  return {
    abortTrackResolve,
    invalidateLoad,
    trackCurrentOnlineToLibrary,
    loadAndPlayCurrent,
  }
}

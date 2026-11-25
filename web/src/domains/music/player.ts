import { computed, onScopeDispose, ref, watch } from 'vue'
import { defineStore } from 'pinia'
import type { SubsonicSongDto } from '@/api/appHost'
import { usePreferencesStore } from '@/stores/preferences'
import { useSnackbarStore } from '@/stores/snackbar'
import { api, buildStreamUrl, canPlayFormatNatively } from './api'
import { useMusicI18n } from './i18n'
import { onlineMusicApi, type OnlineMusicTrackDto } from './onlineApi'
import { buildRemoteSubsonicStreamUrl } from './onlineTrackRef'
import { createPlayerAudioEngine } from './player/audioEngine'
import { createPlayerCloudSync } from './player/cloudSync'
import { createLyricsSyncController } from './player/lyricsSync'
import {
  buildOnlineTrackKey,
  buildPlayerTrackDisplay,
  buildRemoteSongKey,
  createShuffleTracker,
  formatPlayerQualitySummary,
  isOnlineItemExpired,
  isTranscodeProfile,
  makeLocalQueueItem,
  makeOnlineQueueItem,
  makeRemoteQueueItem,
  pickDefaultOnlineQuality,
  resolveOnlineItemCoverUrl,
  resolveOnlineItemStreamUrl,
  shuffleCopy,
  type LocalQueueItem,
  type OnlineQueueItem,
  type PlayerQueueItem,
  type PlayerTrackDisplay,
  type RemoteQueueItem,
} from './player/queueHelpers'
import { useMusicStore } from './store'
import type { PlayMode, TrackDto, TranscodeProfile } from './types'

export {
  buildOnlineTrackKey,
  buildRemoteSongKey,
  resolveOnlineItemCoverUrl,
  resolveOnlineItemStreamUrl,
}
export type {
  LocalQueueItem,
  OnlineQueueItem,
  PlayerQueueItem,
  PlayerTrackDisplay,
  RemoteQueueItem,
}

export const usePlayerStore = defineStore('player', () => {
  const preferencesStore = usePreferencesStore()
  const { t: tMusic, ensureLocale: ensureMusicLocale } = useMusicI18n()
  void ensureMusicLocale()

  const queue = ref<PlayerQueueItem[]>([])
  const currentIndex = ref<number>(-1)
  const isPlaying = ref(false)
  const currentTimeSecs = ref(0)
  const rawAudioDuration = ref(0)
  const bufferedRatio = ref(0)
  const streamOffsetSecs = ref(0)
  const currentStreamIsTranscoded = ref(false)
  const activeEffectiveProfile = ref<TranscodeProfile>('raw')
  const fallbackFormats = ref<Set<string>>(new Set())
  const onlineResolving = ref(false)
  const playbackError = ref<string | null>(null)
  const queueOpen = ref(false)

  let scrobbledCurrent = false
  let loadSeq = 0
  let trackResolveAbort: AbortController | null = null

  function abortTrackResolve(): void {
    trackResolveAbort?.abort()
    trackResolveAbort = null
  }

  const shuffleTracker = createShuffleTracker()

  const trackingCurrentOnline = ref(false)
  const trackedOnlineKeys = ref<Record<string, boolean>>({})

  const currentItem = computed<PlayerQueueItem | null>(() => {
    const idx = currentIndex.value
    if (idx < 0 || idx >= queue.value.length) return null
    return queue.value[idx] ?? null
  })

  const currentLocalTrack = computed<TrackDto | null>(() => {
    const item = currentItem.value
    return item?.kind === 'local' ? item.track : null
  })

  const currentOnlineItem = computed<OnlineQueueItem | null>(() => {
    const item = currentItem.value
    return item?.kind === 'online' ? item : null
  })

  const currentRemoteItem = computed<RemoteQueueItem | null>(() => {
    const item = currentItem.value
    return item?.kind === 'remote' ? item : null
  })

  const isCurrentOnline = computed<boolean>(() => currentItem.value?.kind === 'online')
  const isCurrentRemote = computed<boolean>(() => currentItem.value?.kind === 'remote')

  const isCurrentOnlineTracked = computed<boolean>(() => {
    const item = currentOnlineItem.value
    return item ? Boolean(trackedOnlineKeys.value[item.key]) : false
  })

  const currentTrack = computed<PlayerTrackDisplay | null>(() =>
    buildPlayerTrackDisplay(currentItem.value),
  )

  const durationSecs = computed<number>(() => {
    const metaDur = currentTrack.value?.duration_secs ?? 0
    if (metaDur > 0) return metaDur
    if (Number.isFinite(rawAudioDuration.value) && rawAudioDuration.value > 0) {
      return streamOffsetSecs.value + rawAudioDuration.value
    }
    return 0
  })

  const bufferedPercent = computed<number>(() => Math.round(bufferedRatio.value * 100))
  const transcodeProfile = computed<TranscodeProfile>(
    () => preferencesStore.playback.transcode_profile,
  )
  const playMode = computed<PlayMode>(() => preferencesStore.playback.play_mode)
  const volume = computed<number>(() => preferencesStore.playback.volume)
  const muted = computed<boolean>(() => preferencesStore.playback.muted)
  const qualitySummary = computed<string>(() =>
    formatPlayerQualitySummary(currentItem.value, activeEffectiveProfile.value),
  )

  const lyricsCtrl = createLyricsSyncController({
    currentItem,
    currentTimeSecs,
    onlineResolving,
  })

  const cloudSync = createPlayerCloudSync({
    queue,
    currentIndex,
    currentItem,
    currentTimeSecs,
    onRestoredQueue: async (resumeSecs) => {
      scrobbledCurrent = false
      await loadAndPlayCurrent(resumeSecs, false)
    },
  })

  const audioEngine = createPlayerAudioEngine({
    onTimeUpdate: (el) => {
      const t = streamOffsetSecs.value + (el.currentTime || 0)
      currentTimeSecs.value = t

      if (el.buffered.length > 0 && durationSecs.value > 0) {
        const end = streamOffsetSecs.value + el.buffered.end(el.buffered.length - 1)
        bufferedRatio.value = Math.max(0, Math.min(1, end / durationSecs.value))
      }

      const item = currentItem.value
      if (item?.kind === 'local' && !scrobbledCurrent && durationSecs.value > 0) {
        const threshold = Math.min(240, Math.max(3, durationSecs.value * 0.5))
        if (t >= threshold) {
          scrobbledCurrent = true
          item.track.play_count += 1
          void api.scrobble(item.track.id, true).catch(() => {})
        }
      }
    },
    onDurationChange: (duration) => {
      rawAudioDuration.value = duration
    },
    onPlay: () => {
      isPlaying.value = true
      playbackError.value = null
    },
    onPause: () => {
      isPlaying.value = false
      cloudSync.scheduleQueueSync()
    },
    onEnded: () => {
      if (playMode.value === 'loop_one' && currentTrack.value) {
        scrobbledCurrent = false
        void loadAndPlayCurrent(0, true)
        return
      }
      playNext()
    },
    onError: (el) => {
      if (!el.src || !currentItem.value) return
      isPlaying.value = false
      const msg = el.error?.message || tMusic('player.playbackError')
      playbackError.value = msg
      if (currentItem.value.kind !== 'online') {
        useSnackbarStore().show({ message: msg, tone: 'error' })
      }
    },
    onExternalAudioFocusClaimed: () => {
      pause()
    },
  })

  const { audio, attachAudioElement, detachAudioElement } = audioEngine

  function syncAudioGain(): void {
    audioEngine.syncGain(volume.value, muted.value)
  }

  function refreshMediaSession(track: PlayerTrackDisplay | null): void {
    audioEngine.updateMediaSession(track, {
      onPlayPause: () => void togglePlay(),
      onPrev: () => playPrev(),
      onNext: () => playNext(),
    })
  }

  watch([volume, muted], syncAudioGain, { immediate: true })

  watch(transcodeProfile, (next, prev) => {
    if (!prev || next === prev) return
    if (currentItem.value?.kind === 'local' && activeEffectiveProfile.value !== next) {
      void loadAndPlayCurrent(currentTimeSecs.value, isPlaying.value)
    }
  })

  async function trackCurrentOnlineToLibrary(): Promise<void> {
    const item = currentOnlineItem.value
    if (!item || trackingCurrentOnline.value) return
    trackingCurrentOnline.value = true
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
      trackedOnlineKeys.value = { ...trackedOnlineKeys.value, [item.key]: true }
      const musicStore = useMusicStore()
      musicStore.bumpCatalogVersion()
      void musicStore.refreshSidebar()
      useSnackbarStore().show(tMusic('online.trackedToast', { title: tr.title }))
    } catch (err) {
      useSnackbarStore().show({
        message: err instanceof Error ? err.message : String(err),
        tone: 'error',
      })
    } finally {
      trackingCurrentOnline.value = false
    }
  }

  function dismissPlaybackError(): void {
    playbackError.value = null
  }

  async function loadAndPlayCurrent(
    startAtSecs = 0,
    autoPlay = true,
    overrideProfileOrQuality?: TranscodeProfile | string,
  ): Promise<void> {
    const item = currentItem.value
    if (!item) return
    abortTrackResolve()
    const seq = ++loadSeq
    const targetKey = item.key
    lyricsCtrl.prepareForTrackSwitch(targetKey)

    if (item.kind === 'local') {
      onlineResolving.value = false
      playbackError.value = null
      const track = item.track
      const overrideProfile = isTranscodeProfile(overrideProfileOrQuality)
        ? overrideProfileOrQuality
        : undefined

      syncAudioGain()
      const requestedProfile = overrideProfile ?? transcodeProfile.value
      const formatKey = track.format.toLowerCase()
      const profile =
        !overrideProfile && requestedProfile === 'raw' && fallbackFormats.value.has(formatKey)
          ? 'mp3320'
          : requestedProfile
      activeEffectiveProfile.value = profile
      const needsTranscode = profile !== 'raw' || !canPlayFormatNatively(track.format)
      currentStreamIsTranscoded.value = needsTranscode
      const useServerOffset = needsTranscode && startAtSecs > 0
      streamOffsetSecs.value = useServerOffset ? Math.floor(startAtSecs) : 0
      currentTimeSecs.value = startAtSecs
      rawAudioDuration.value = 0
      bufferedRatio.value = 0

      audioEngine.clearPendingSeek()
      audio.src = buildStreamUrl(track.id, profile, streamOffsetSecs.value, track.format)
      audio.load()

      if (!useServerOffset && startAtSecs > 0) {
        audioEngine.scheduleMetadataSeek(
          startAtSecs,
          () => seq === loadSeq && currentItem.value?.key === targetKey,
        )
      }

      refreshMediaSession(currentTrack.value)
      lyricsCtrl.clearForPendingTrack(targetKey)
      if (lyricsCtrl.lyricsOpen.value && !lyricsCtrl.isLoadedFor(targetKey)) {
        void lyricsCtrl.fetchLyricsFor(item)
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
            fallbackFormats.value.add(formatKey)
            await loadAndPlayCurrent(startAtSecs, true, 'mp3320')
            return
          }
          const msg =
            err instanceof Error && err.message ? err.message : tMusic('player.playbackError')
          playbackError.value = msg
          isPlaying.value = false
          useSnackbarStore().show({ message: msg, tone: 'error' })
        }
      }
      if (seq === loadSeq) {
        cloudSync.scheduleQueueSync()
      }
      return
    }

    if (item.kind === 'remote') {
      onlineResolving.value = false
      playbackError.value = null
      syncAudioGain()
      currentStreamIsTranscoded.value = false
      streamOffsetSecs.value = 0
      currentTimeSecs.value = startAtSecs
      rawAudioDuration.value = 0
      bufferedRatio.value = 0
      audioEngine.clearPendingSeek()

      audio.src =
        item.song.stream_url || buildRemoteSubsonicStreamUrl(item.source_id, item.song.id)
      audio.load()

      if (startAtSecs > 0) {
        audioEngine.scheduleMetadataSeek(
          startAtSecs,
          () => seq === loadSeq && currentItem.value?.key === targetKey,
        )
      }

      lyricsCtrl.markNoLyricsForKey(targetKey)
      refreshMediaSession(currentTrack.value)

      if (autoPlay) {
        try {
          await audio.play()
          if (seq !== loadSeq) return
        } catch (err) {
          if (seq !== loadSeq) return
          if (err instanceof DOMException && err.name === 'AbortError') return
          const msg = err instanceof Error ? err.message : String(err)
          playbackError.value = msg
          isPlaying.value = false
          useSnackbarStore().show({ message: msg, tone: 'error' })
        }
      }
      return
    }

    const overrideQuality =
      typeof overrideProfileOrQuality === 'string' ? overrideProfileOrQuality : undefined
    syncAudioGain()
    currentStreamIsTranscoded.value = false
    streamOffsetSecs.value = 0
    currentTimeSecs.value = startAtSecs
    rawAudioDuration.value = 0
    bufferedRatio.value = 0
    onlineResolving.value = false
    playbackError.value = null
    audioEngine.resetSource(true)
    lyricsCtrl.clearForPendingTrack(targetKey)
    refreshMediaSession(currentTrack.value)

    const isExpired = isOnlineItemExpired(item)
    const requestedQuality =
      overrideQuality ?? item.selectedQuality ?? pickDefaultOnlineQuality(item.track.qualities)
    const needsFullInfo = !item.resolved || isExpired
    const needsQualitySwitch =
      !needsFullInfo && overrideQuality !== undefined && overrideQuality !== item.selectedQuality

    if (needsFullInfo || needsQualitySwitch) {
      const resolveController = new AbortController()
      trackResolveAbort = resolveController
      onlineResolving.value = true
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
            currentItem.value?.key !== targetKey
          ) {
            return
          }
          item.resolved = resolved
          item.resolvedAtMs = Date.now()
          item.selectedQuality = requestedQuality
          item.activeFormat = resolved.format ?? item.track.format
          item.activeBitrate = resolved.bitrate
          lyricsCtrl.applyResolvedLyrics(targetKey, resolved.lyrics, resolved.translated_lyrics)
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
            currentItem.value?.key !== targetKey
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
          currentItem.value?.key !== targetKey ||
          (err instanceof Error && err.name === 'AbortError')
        ) {
          return
        }
        const msg = err instanceof Error ? err.message : String(err)
        playbackError.value = msg
        isPlaying.value = false
        useSnackbarStore().show({ message: msg, tone: 'error' })
        return
      } finally {
        if (trackResolveAbort === resolveController) {
          trackResolveAbort = null
        }
        if (seq === loadSeq) {
          onlineResolving.value = false
        }
      }
    } else if (item.resolved) {
      lyricsCtrl.applyResolvedLyrics(targetKey, item.resolved.lyrics, item.resolved.translated_lyrics)
    }

    if (seq !== loadSeq || currentItem.value?.key !== targetKey) return

    const streamUrl = resolveOnlineItemStreamUrl(item)
    if (!streamUrl) {
      const msg = tMusic('player.noPlayableStream')
      playbackError.value = msg
      isPlaying.value = false
      useSnackbarStore().show({ message: msg, tone: 'error' })
      return
    }

    audio.src = streamUrl
    audio.load()

    if (startAtSecs > 0) {
      audioEngine.scheduleMetadataSeek(
        startAtSecs,
        () => seq === loadSeq && currentItem.value?.key === targetKey,
      )
    }

    refreshMediaSession(currentTrack.value)

    if (autoPlay) {
      try {
        await audio.play()
        if (seq !== loadSeq) return
      } catch (err) {
        if (seq !== loadSeq) return
        if (err instanceof DOMException && err.name === 'AbortError') return
        const msg = err instanceof Error ? err.message : String(err)
        playbackError.value = msg
        isPlaying.value = false
      }
    }
  }

  function replaceQueue(
    items: PlayerQueueItem[],
    startIndex = 0,
    startAtSecs = 0,
    bumpEpoch = true,
  ): void {
    if (items.length === 0) return
    if (bumpEpoch) cloudSync.bumpEpoch()
    cloudSync.resetClearFlag()
    queue.value = items
    currentIndex.value = Math.max(0, Math.min(items.length - 1, startIndex))
    scrobbledCurrent = false
    shuffleTracker.reset()
    void loadAndPlayCurrent(Math.max(0, startAtSecs), true)
  }

  function shuffleAndReplaceQueue(items: PlayerQueueItem[]): void {
    if (items.length === 0) return
    preferencesStore.updatePlaybackPreferences({ play_mode: 'shuffle' })
    replaceQueue(shuffleCopy(items), 0, 0, true)
  }

  function playOrInsertQueueItem(
    item: PlayerQueueItem,
    startAtSecs = 0,
    bumpEpochOnEmpty = true,
  ): void {
    const resumeAt = Math.max(0, startAtSecs)
    const existingIdx = queue.value.findIndex((existing) => existing.key === item.key)
    if (existingIdx >= 0) {
      shuffleTracker.recordJump(currentIndex.value, existingIdx)
      currentIndex.value = existingIdx
      scrobbledCurrent = false
      void loadAndPlayCurrent(resumeAt, true)
    } else if (queue.value.length > 0) {
      const insertAt = currentIndex.value >= 0 ? currentIndex.value + 1 : queue.value.length
      shuffleTracker.recordInsert(insertAt, currentIndex.value, false)
      queue.value.splice(insertAt, 0, item)
      currentIndex.value = insertAt
      scrobbledCurrent = false
      void loadAndPlayCurrent(resumeAt, true)
    } else {
      replaceQueue([item], 0, resumeAt, bumpEpochOnEmpty)
    }
  }

  function enqueueQueueItemNext(item: PlayerQueueItem, syncLocal = false): void {
    if (queue.value.length === 0) {
      replaceQueue([item], 0, 0, true)
      return
    }
    const insertAt = currentIndex.value >= 0 ? currentIndex.value + 1 : queue.value.length
    shuffleTracker.recordInsert(insertAt, currentIndex.value, true)
    queue.value.splice(insertAt, 0, item)
    if (syncLocal) cloudSync.scheduleQueueSync()
  }

  function enqueueQueueItemIfAbsent(item: PlayerQueueItem): void {
    if (queue.value.length === 0) {
      replaceQueue([item], 0, 0, true)
      return
    }
    if (queue.value.some((existing) => existing.key === item.key)) return
    queue.value.push(item)
  }

  function playQueue(tracks: TrackDto[], startIndex = 0): void {
    replaceQueue(tracks.map(makeLocalQueueItem), startIndex, 0, true)
  }

  function resumeQueue(tracks: TrackDto[], startIndex = 0, startAtSecs = 0): void {
    replaceQueue(tracks.map(makeLocalQueueItem), startIndex, startAtSecs, false)
  }

  function shuffleAndPlay(tracks: TrackDto[]): void {
    shuffleAndReplaceQueue(tracks.map(makeLocalQueueItem))
  }

  function playOnlineTracks(tracks: OnlineMusicTrackDto[], startIndex = 0): void {
    replaceQueue(tracks.map(makeOnlineQueueItem), startIndex, 0, true)
  }

  function shuffleAndPlayOnline(tracks: OnlineMusicTrackDto[]): void {
    shuffleAndReplaceQueue(tracks.map(makeOnlineQueueItem))
  }

  function playAtIndex(index: number): void {
    if (index < 0 || index >= queue.value.length) return
    if (index === currentIndex.value) {
      void togglePlay()
      return
    }
    shuffleTracker.recordJump(currentIndex.value, index)
    currentIndex.value = index
    scrobbledCurrent = false
    void loadAndPlayCurrent(0, true)
  }

  function playTrackNow(track: TrackDto): void {
    playTrackAt(track, 0)
  }

  function playTrackAt(track: TrackDto, startAtSecs = 0): void {
    playOrInsertQueueItem(makeLocalQueueItem(track), startAtSecs, false)
  }

  function playOnlineTrackNow(track: OnlineMusicTrackDto): void {
    playOnlineTrackAt(track, 0)
  }

  function playOnlineTrackAt(track: OnlineMusicTrackDto, startAtSecs = 0): void {
    playOrInsertQueueItem(makeOnlineQueueItem(track), startAtSecs, true)
  }

  function playRemoteSongs(
    sourceId: string,
    sourceName: string,
    songs: SubsonicSongDto[],
    startIndex = 0,
    startAtSecs = 0,
  ): void {
    replaceQueue(
      songs.map((song) => makeRemoteQueueItem(sourceId, sourceName, song)),
      startIndex,
      startAtSecs,
      true,
    )
  }

  function shuffleAndPlayRemoteSongs(
    sourceId: string,
    sourceName: string,
    songs: SubsonicSongDto[],
  ): void {
    shuffleAndReplaceQueue(songs.map((song) => makeRemoteQueueItem(sourceId, sourceName, song)))
  }

  function playRemoteSongAt(
    sourceId: string,
    sourceName: string,
    song: SubsonicSongDto,
    startAtSecs = 0,
  ): void {
    playOrInsertQueueItem(makeRemoteQueueItem(sourceId, sourceName, song), startAtSecs, true)
  }

  function enqueueTracks(tracks: TrackDto[]): void {
    if (tracks.length === 0) return
    if (queue.value.length === 0) {
      playQueue(tracks, 0)
      return
    }
    for (const tr of tracks) {
      queue.value.push(makeLocalQueueItem(tr))
    }
    cloudSync.scheduleQueueSync()
  }

  function enqueueRemoteSong(sourceId: string, sourceName: string, song: SubsonicSongDto): void {
    enqueueQueueItemIfAbsent(makeRemoteQueueItem(sourceId, sourceName, song))
  }

  function enqueueRemoteSongNext(
    sourceId: string,
    sourceName: string,
    song: SubsonicSongDto,
  ): void {
    enqueueQueueItemNext(makeRemoteQueueItem(sourceId, sourceName, song))
  }

  function isRemoteSongInQueue(sourceId: string, songId: string): boolean {
    const key = buildRemoteSongKey(sourceId, songId)
    return queue.value.some((item) => item.key === key)
  }

  function enqueueNext(track: TrackDto): void {
    enqueueQueueItemNext(makeLocalQueueItem(track), true)
  }

  function enqueueOnlineTrackNext(track: OnlineMusicTrackDto): void {
    enqueueQueueItemNext(makeOnlineQueueItem(track))
  }

  function enqueueOnlineTrack(track: OnlineMusicTrackDto): void {
    enqueueQueueItemIfAbsent(makeOnlineQueueItem(track))
  }

  function isOnlineTrackInQueue(track: OnlineMusicTrackDto): boolean {
    const key = buildOnlineTrackKey(track)
    return queue.value.some((item) => item.key === key)
  }

  function pause(): void {
    if (isPlaying.value || !audio.paused) {
      audio.pause()
    }
  }

  async function togglePlay(): Promise<void> {
    const item = currentItem.value
    if (!item || onlineResolving.value) return
    if (isPlaying.value) {
      audio.pause()
    } else if (item.kind === 'online' && (!item.resolved || isOnlineItemExpired(item))) {
      await loadAndPlayCurrent(currentTimeSecs.value, true)
    } else if (!audio.src || audio.error) {
      await loadAndPlayCurrent(currentTimeSecs.value, true)
    } else {
      try {
        await audio.play()
      } catch (err) {
        if (err instanceof DOMException && err.name === 'AbortError') return
        if (audio.error) {
          await loadAndPlayCurrent(currentTimeSecs.value, true)
        } else {
          isPlaying.value = false
        }
      }
    }
  }

  async function retryCurrentPlayback(): Promise<void> {
    const item = currentItem.value
    if (!item || onlineResolving.value) return
    if (item.kind === 'online') {
      item.resolved = null
      item.resolvedAtMs = null
    }
    await loadAndPlayCurrent(currentTimeSecs.value, true)
  }

  function playNext(): void {
    const len = queue.value.length
    if (len === 0) return
    currentIndex.value = shuffleTracker.pickNextIndex(
      len,
      currentIndex.value,
      playMode.value === 'shuffle',
    )
    scrobbledCurrent = false
    void loadAndPlayCurrent(0, true)
  }

  function playPrev(): void {
    const len = queue.value.length
    if (len === 0) return
    if (currentTimeSecs.value > 4) {
      seekTo(0)
      return
    }
    currentIndex.value = shuffleTracker.pickPrevIndex(
      len,
      currentIndex.value,
      playMode.value === 'shuffle',
    )
    scrobbledCurrent = false
    void loadAndPlayCurrent(0, true)
  }

  function seekTo(targetSecs: number): void {
    if (!currentTrack.value) return
    const clamped = Math.max(0, Math.min(durationSecs.value || targetSecs, targetSecs))
    if (!currentStreamIsTranscoded.value) {
      streamOffsetSecs.value = 0
      audio.currentTime = clamped
      currentTimeSecs.value = clamped
    } else {
      void loadAndPlayCurrent(clamped, isPlaying.value)
    }
    cloudSync.scheduleQueueSync()
  }

  function setTranscodeProfile(next: TranscodeProfile): void {
    if (next === transcodeProfile.value) return
    preferencesStore.updatePlaybackPreferences({ transcode_profile: next }, { immediate: true })
  }

  function setOnlineQuality(quality: string): void {
    const item = currentOnlineItem.value
    if (!item || !quality || item.selectedQuality === quality) return
    void loadAndPlayCurrent(currentTimeSecs.value, isPlaying.value, quality)
  }

  function cyclePlayMode(): void {
    const order: PlayMode[] = ['sequence', 'loop_one', 'shuffle']
    const idx = order.indexOf(playMode.value)
    const next = order[(idx + 1) % order.length]!
    preferencesStore.updatePlaybackPreferences({ play_mode: next })
  }

  function setVolume(nextVol: number): void {
    const clamped = Math.max(0, Math.min(1, nextVol))
    preferencesStore.updatePlaybackPreferences({
      volume: clamped,
      muted: clamped === 0,
    })
  }

  function toggleMute(): void {
    preferencesStore.updatePlaybackPreferences({ muted: !muted.value })
  }

  function moveQueueItem(fromIndex: number, toIndex: number): void {
    const len = queue.value.length
    if (
      fromIndex < 0 ||
      fromIndex >= len ||
      toIndex < 0 ||
      toIndex >= len ||
      fromIndex === toIndex
    ) {
      return
    }
    const [moved] = queue.value.splice(fromIndex, 1)
    if (!moved) return
    queue.value.splice(toIndex, 0, moved)
    currentIndex.value = shuffleTracker.remapMove(fromIndex, toIndex, currentIndex.value)
    cloudSync.scheduleQueueSync()
  }

  function clearStoppedRuntimeState(closeOverlays = false): void {
    audioEngine.resetSource(closeOverlays)
    currentIndex.value = -1
    currentTimeSecs.value = 0
    rawAudioDuration.value = 0
    bufferedRatio.value = 0
    onlineResolving.value = false
    playbackError.value = null
    lyricsCtrl.resetLyrics(closeOverlays)
    shuffleTracker.reset()
  }

  function removeQueueAt(idx: number): void {
    if (idx < 0 || idx >= queue.value.length) return
    const removedItem = queue.value[idx]
    const wasCurrent = idx === currentIndex.value
    queue.value.splice(idx, 1)
    shuffleTracker.remapRemove(idx)
    cloudSync.markRemovedItem(removedItem)
    if (queue.value.length === 0) {
      loadSeq += 1
      abortTrackResolve()
      clearStoppedRuntimeState(false)
    } else if (wasCurrent) {
      currentIndex.value = Math.min(idx, queue.value.length - 1)
      scrobbledCurrent = false
      void loadAndPlayCurrent(0, isPlaying.value)
    } else if (idx < currentIndex.value) {
      currentIndex.value -= 1
    }
    cloudSync.scheduleQueueSync()
  }

  function removeByTrackId(trackId: string): void {
    for (let i = queue.value.length - 1; i >= 0; i--) {
      const item = queue.value[i]
      if (item?.kind === 'local' && item.track.id === trackId) {
        removeQueueAt(i)
      }
    }
  }

  function clearQueue(): void {
    loadSeq += 1
    abortTrackResolve()
    cloudSync.markClearedWithLocal(queue.value.some((i) => i.kind === 'local'))
    queue.value = []
    clearStoppedRuntimeState(false)
    cloudSync.scheduleQueueSync()
  }

  function resetForLogout(): void {
    loadSeq += 1
    abortTrackResolve()
    cloudSync.resetCloudSync()
    queue.value = []
    isPlaying.value = false
    streamOffsetSecs.value = 0
    currentStreamIsTranscoded.value = false
    activeEffectiveProfile.value = 'raw'
    fallbackFormats.value.clear()
    queueOpen.value = false
    trackedOnlineKeys.value = {}
    trackingCurrentOnline.value = false
    scrobbledCurrent = false
    clearStoppedRuntimeState(true)
    refreshMediaSession(null)
  }

  onScopeDispose(() => {
    cloudSync.dispose()
    abortTrackResolve()
    lyricsCtrl.abortLyricsFetch()
    audioEngine.dispose()
  })

  return {
    queue,
    currentIndex,
    currentItem,
    currentLocalTrack,
    currentOnlineItem,
    currentRemoteItem,
    isCurrentOnline,
    isCurrentRemote,
    currentTrack,
    isPlaying,
    onlineResolving,
    playbackError,
    currentTimeSecs,
    durationSecs,
    bufferedPercent,
    transcodeProfile,
    playMode,
    volume,
    muted,
    qualitySummary,
    lyricsOpen: lyricsCtrl.lyricsOpen,
    queueOpen,
    lyricsLoading: lyricsCtrl.lyricsLoading,
    lyrics: lyricsCtrl.lyrics,
    translatedLyrics: lyricsCtrl.translatedLyrics,
    activeLyricIndex: lyricsCtrl.activeLyricIndex,
    isCurrentOnlineTracked,
    trackingCurrentOnline,
    trackCurrentOnlineToLibrary,
    dismissPlaybackError,
    attachAudioElement,
    detachAudioElement,
    playQueue,
    resumeQueue,
    shuffleAndPlay,
    playOnlineTracks,
    shuffleAndPlayOnline,
    playAtIndex,
    playTrackNow,
    playTrackAt,
    playOnlineTrackNow,
    playOnlineTrackAt,
    playRemoteSongs,
    shuffleAndPlayRemoteSongs,
    playRemoteSongAt,
    enqueueTracks,
    enqueueNext,
    enqueueOnlineTrackNext,
    enqueueOnlineTrack,
    isOnlineTrackInQueue,
    enqueueRemoteSong,
    enqueueRemoteSongNext,
    isRemoteSongInQueue,
    pause,
    togglePlay,
    retryCurrentPlayback,
    playNext,
    playPrev,
    seekTo,
    setTranscodeProfile,
    setOnlineQuality,
    cyclePlayMode,
    setVolume,
    toggleMute,
    moveQueueItem,
    removeQueueAt,
    removeByTrackId,
    clearQueue,
    resetForLogout,
    restoreRemoteQueue: cloudSync.restoreRemoteQueue,
  }
})

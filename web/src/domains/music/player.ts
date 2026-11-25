import { computed, onScopeDispose, ref, watch } from 'vue'
import { defineStore } from 'pinia'
import type { SubsonicSongDto } from '@/api/appHost'
import { claimMediaFocus, registerMediaFocus, releaseMediaFocus } from '@/composables/useMediaFocus'
import { usePreferencesStore } from '@/stores/preferences'
import { useSnackbarStore } from '@/stores/snackbar'
import { api } from './api'
import { useMusicI18n } from './i18n'
import type { OnlineMusicTrackDto } from './onlineApi'
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
  resolveOnlineItemCoverUrl,
  resolveOnlineItemStreamUrl,
  shuffleCopy,
  toPlayerQueueItems,
  type LocalQueueItem,
  type OnlineQueueItem,
  type PlayableQueueInput,
  type PlayerQueueItem,
  type PlayerTrackDisplay,
  type RemoteQueueItem,
} from './player/queueHelpers'
import { createPlayerTrackLoader } from './player/trackLoader'
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
  PlayableQueueInput,
  PlayerQueueItem,
  PlayerTrackDisplay,
  RemoteQueueItem,
}

export interface PlayQueueOptions {
  startIndex?: number
  startAtSecs?: number
  shuffle?: boolean
  bumpEpoch?: boolean
  mode?: 'replace' | 'play-or-insert'
}

export type EnqueueMode = 'end' | 'next' | 'if-absent'

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
  const trackingCurrentOnline = ref(false)
  const trackedOnlineKeys = ref<Record<string, boolean>>({})

  let scrobbledCurrent = false
  const shuffleTracker = createShuffleTracker()

  const currentItem = computed<PlayerQueueItem | null>(() => {
    const idx = currentIndex.value
    return idx >= 0 && idx < queue.value.length ? (queue.value[idx] ?? null) : null
  })
  const currentLocalTrack = computed<TrackDto | null>(() =>
    currentItem.value?.kind === 'local' ? currentItem.value.track : null,
  )
  const currentOnlineItem = computed<OnlineQueueItem | null>(() =>
    currentItem.value?.kind === 'online' ? currentItem.value : null,
  )
  const currentRemoteItem = computed<RemoteQueueItem | null>(() =>
    currentItem.value?.kind === 'remote' ? currentItem.value : null,
  )
  const isCurrentOnline = computed(() => currentItem.value?.kind === 'online')
  const isCurrentRemote = computed(() => currentItem.value?.kind === 'remote')
  const isCurrentOnlineTracked = computed(() => {
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

  const bufferedPercent = computed(() => Math.round(bufferedRatio.value * 100))
  const transcodeProfile = computed(() => preferencesStore.playback.transcode_profile)
  const playMode = computed<PlayMode>(() => preferencesStore.playback.play_mode)
  const volume = computed(() => preferencesStore.playback.volume)
  const muted = computed(() => preferencesStore.playback.muted)
  const qualitySummary = computed(() =>
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
      await trackLoader.loadAndPlayCurrent(resumeSecs, false)
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
      claimMediaFocus('music')
    },
    onPause: () => {
      isPlaying.value = false
      releaseMediaFocus('music')
      cloudSync.scheduleQueueSync()
    },
    onEnded: () => {
      if (playMode.value === 'loop_one' && currentTrack.value) {
        scrobbledCurrent = false
        void trackLoader.loadAndPlayCurrent(0, true)
        return
      }
      releaseMediaFocus('music')
      playNext()
    },
    onError: (el) => {
      if (!el.src || !currentItem.value) return
      isPlaying.value = false
      releaseMediaFocus('music')
      const msg = el.error?.message || tMusic('player.playbackError')
      playbackError.value = msg
      if (currentItem.value.kind !== 'online') {
        useSnackbarStore().show({ message: msg, tone: 'error' })
      }
    },
  })

  const { audio, attachAudioElement, detachAudioElement } = audioEngine
  const unregisterMediaFocus = registerMediaFocus('music', () => pause())

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

  const trackLoader = createPlayerTrackLoader({
    audioEngine,
    lyricsCtrl,
    cloudSync,
    currentItem,
    currentOnlineItem,
    currentTrack,
    transcodeProfile,
    isPlaying,
    currentTimeSecs,
    rawAudioDuration,
    bufferedRatio,
    streamOffsetSecs,
    currentStreamIsTranscoded,
    activeEffectiveProfile,
    fallbackFormats,
    onlineResolving,
    playbackError,
    trackingCurrentOnline,
    trackedOnlineKeys,
    syncAudioGain,
    refreshMediaSession,
    tMusic: (key, params) => tMusic(key, params as never),
  })

  watch([volume, muted], syncAudioGain, { immediate: true })
  watch(transcodeProfile, (next, prev) => {
    if (!prev || next === prev) return
    if (currentItem.value?.kind === 'local' && activeEffectiveProfile.value !== next) {
      void trackLoader.loadAndPlayCurrent(currentTimeSecs.value, isPlaying.value)
    }
  })

  function dismissPlaybackError(): void {
    playbackError.value = null
  }

  // Unified queue API collapsing combinatoric local/online/remote variants
  function playItems(input: PlayableQueueInput, options: PlayQueueOptions = {}): void {
    const built = toPlayerQueueItems(input)
    if (built.length === 0) return

    if (options.mode === 'play-or-insert') {
      const item = built[0]!
      const resumeAt = Math.max(0, options.startAtSecs ?? 0)
      const existingIdx = queue.value.findIndex((existing) => existing.key === item.key)
      if (existingIdx >= 0) {
        shuffleTracker.recordJump(currentIndex.value, existingIdx)
        currentIndex.value = existingIdx
        scrobbledCurrent = false
        void trackLoader.loadAndPlayCurrent(resumeAt, true)
        return
      }
      if (queue.value.length > 0) {
        const insertAt = currentIndex.value >= 0 ? currentIndex.value + 1 : queue.value.length
        shuffleTracker.recordInsert(insertAt, currentIndex.value, false)
        queue.value.splice(insertAt, 0, item)
        currentIndex.value = insertAt
        scrobbledCurrent = false
        void trackLoader.loadAndPlayCurrent(resumeAt, true)
        return
      }
    }

    const items = options.shuffle ? shuffleCopy(built) : built
    if (options.shuffle) {
      preferencesStore.updatePlaybackPreferences({ play_mode: 'shuffle' })
    }
    if (options.bumpEpoch ?? true) cloudSync.bumpEpoch()
    cloudSync.resetClearFlag()
    queue.value = items
    currentIndex.value = options.shuffle
      ? 0
      : Math.max(0, Math.min(items.length - 1, options.startIndex ?? 0))
    scrobbledCurrent = false
    shuffleTracker.reset()
    void trackLoader.loadAndPlayCurrent(Math.max(0, options.startAtSecs ?? 0), true)
  }

  function enqueueItems(input: PlayableQueueInput, mode: EnqueueMode = 'end'): void {
    const items = toPlayerQueueItems(input)
    if (items.length === 0) return
    if (queue.value.length === 0) {
      playItems(input, { startIndex: 0 })
      return
    }
    if (mode === 'next') {
      const insertAt = currentIndex.value >= 0 ? currentIndex.value + 1 : queue.value.length
      items.forEach((item, offset) => {
        shuffleTracker.recordInsert(insertAt + offset, currentIndex.value, true)
        queue.value.splice(insertAt + offset, 0, item)
      })
    } else if (mode === 'if-absent') {
      for (const item of items) {
        if (!queue.value.some((existing) => existing.key === item.key)) {
          queue.value.push(item)
        }
      }
    } else {
      queue.value.push(...items)
    }
    if (input.kind === 'local') {
      cloudSync.scheduleQueueSync()
    }
  }

  // Thin 1-line ergonomic aliases for domain views
  const playQueue = (tracks: TrackDto[], startIndex = 0) =>
    playItems({ kind: 'local', tracks }, { startIndex })
  const resumeQueue = (tracks: TrackDto[], startIndex = 0, startAtSecs = 0) =>
    playItems({ kind: 'local', tracks }, { startIndex, startAtSecs, bumpEpoch: false })
  const shuffleAndPlay = (tracks: TrackDto[]) =>
    playItems({ kind: 'local', tracks }, { shuffle: true })
  const playOnlineTracks = (tracks: OnlineMusicTrackDto[], startIndex = 0) =>
    playItems({ kind: 'online', tracks }, { startIndex })
  const shuffleAndPlayOnline = (tracks: OnlineMusicTrackDto[]) =>
    playItems({ kind: 'online', tracks }, { shuffle: true })
  const playTrackAt = (track: TrackDto, startAtSecs = 0) =>
    playItems(
      { kind: 'local', tracks: [track] },
      { startAtSecs, bumpEpoch: false, mode: 'play-or-insert' },
    )
  const playTrackNow = (track: TrackDto) => playTrackAt(track, 0)
  const playOnlineTrackAt = (track: OnlineMusicTrackDto, startAtSecs = 0) =>
    playItems({ kind: 'online', tracks: [track] }, { startAtSecs, mode: 'play-or-insert' })
  const playOnlineTrackNow = (track: OnlineMusicTrackDto) => playOnlineTrackAt(track, 0)
  const playRemoteSongs = (
    sourceId: string,
    sourceName: string,
    songs: SubsonicSongDto[],
    startIndex = 0,
    startAtSecs = 0,
  ) => playItems({ kind: 'remote', sourceId, sourceName, songs }, { startIndex, startAtSecs })
  const shuffleAndPlayRemoteSongs = (
    sourceId: string,
    sourceName: string,
    songs: SubsonicSongDto[],
  ) => playItems({ kind: 'remote', sourceId, sourceName, songs }, { shuffle: true })
  const playRemoteSongAt = (
    sourceId: string,
    sourceName: string,
    song: SubsonicSongDto,
    startAtSecs = 0,
  ) =>
    playItems(
      { kind: 'remote', sourceId, sourceName, songs: [song] },
      { startAtSecs, mode: 'play-or-insert' },
    )
  const enqueueTracks = (tracks: TrackDto[]) => enqueueItems({ kind: 'local', tracks }, 'end')
  const enqueueNext = (track: TrackDto) => enqueueItems({ kind: 'local', tracks: [track] }, 'next')
  const enqueueOnlineTrackNext = (track: OnlineMusicTrackDto) =>
    enqueueItems({ kind: 'online', tracks: [track] }, 'next')
  const enqueueOnlineTrack = (track: OnlineMusicTrackDto) =>
    enqueueItems({ kind: 'online', tracks: [track] }, 'if-absent')
  const enqueueRemoteSong = (sourceId: string, sourceName: string, song: SubsonicSongDto) =>
    enqueueItems({ kind: 'remote', sourceId, sourceName, songs: [song] }, 'if-absent')
  const enqueueRemoteSongNext = (sourceId: string, sourceName: string, song: SubsonicSongDto) =>
    enqueueItems({ kind: 'remote', sourceId, sourceName, songs: [song] }, 'next')
  const isOnlineTrackInQueue = (track: OnlineMusicTrackDto): boolean =>
    queue.value.some((item) => item.key === buildOnlineTrackKey(track))
  const isRemoteSongInQueue = (sourceId: string, songId: string): boolean =>
    queue.value.some((item) => item.key === buildRemoteSongKey(sourceId, songId))

  function playAtIndex(index: number): void {
    if (index < 0 || index >= queue.value.length) return
    if (index === currentIndex.value) {
      void togglePlay()
      return
    }
    shuffleTracker.recordJump(currentIndex.value, index)
    currentIndex.value = index
    scrobbledCurrent = false
    void trackLoader.loadAndPlayCurrent(0, true)
  }

  function pause(): void {
    if (isPlaying.value || !audio.paused) audio.pause()
  }

  async function togglePlay(): Promise<void> {
    const item = currentItem.value
    if (!item || onlineResolving.value) return
    if (isPlaying.value) {
      audio.pause()
    } else if (
      (item.kind === 'online' && (!item.resolved || isOnlineItemExpired(item))) ||
      !audio.src ||
      audio.error
    ) {
      await trackLoader.loadAndPlayCurrent(currentTimeSecs.value, true)
    } else {
      try {
        await audio.play()
      } catch (err) {
        if (err instanceof DOMException && err.name === 'AbortError') return
        if (audio.error) {
          await trackLoader.loadAndPlayCurrent(currentTimeSecs.value, true)
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
    await trackLoader.loadAndPlayCurrent(currentTimeSecs.value, true)
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
    void trackLoader.loadAndPlayCurrent(0, true)
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
    void trackLoader.loadAndPlayCurrent(0, true)
  }

  function seekTo(targetSecs: number): void {
    if (!currentTrack.value) return
    const clamped = Math.max(0, Math.min(durationSecs.value || targetSecs, targetSecs))
    if (!currentStreamIsTranscoded.value) {
      streamOffsetSecs.value = 0
      audio.currentTime = clamped
      currentTimeSecs.value = clamped
    } else {
      void trackLoader.loadAndPlayCurrent(clamped, isPlaying.value)
    }
    cloudSync.scheduleQueueSync()
  }

  function setTranscodeProfile(next: TranscodeProfile): void {
    if (next !== transcodeProfile.value) {
      preferencesStore.updatePlaybackPreferences({ transcode_profile: next }, { immediate: true })
    }
  }

  function setOnlineQuality(quality: string): void {
    const item = currentOnlineItem.value
    if (!item || !quality || item.selectedQuality === quality) return
    void trackLoader.loadAndPlayCurrent(currentTimeSecs.value, isPlaying.value, quality)
  }

  function cyclePlayMode(): void {
    const order: PlayMode[] = ['sequence', 'loop_one', 'shuffle']
    const next = order[(order.indexOf(playMode.value) + 1) % order.length]!
    preferencesStore.updatePlaybackPreferences({ play_mode: next })
  }

  function setVolume(nextVol: number): void {
    const clamped = Math.max(0, Math.min(1, nextVol))
    preferencesStore.updatePlaybackPreferences({ volume: clamped, muted: clamped === 0 })
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
      trackLoader.invalidateLoad()
      clearStoppedRuntimeState(false)
    } else if (wasCurrent) {
      currentIndex.value = Math.min(idx, queue.value.length - 1)
      scrobbledCurrent = false
      void trackLoader.loadAndPlayCurrent(0, isPlaying.value)
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
    trackLoader.invalidateLoad()
    cloudSync.markClearedWithLocal(queue.value.some((i) => i.kind === 'local'))
    queue.value = []
    clearStoppedRuntimeState(false)
    cloudSync.scheduleQueueSync()
  }

  function resetForLogout(): void {
    trackLoader.invalidateLoad()
    cloudSync.resetCloudSync()
    queue.value = []
    isPlaying.value = false
    releaseMediaFocus('music')
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
    unregisterMediaFocus()
    cloudSync.dispose()
    trackLoader.abortTrackResolve()
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
    trackCurrentOnlineToLibrary: trackLoader.trackCurrentOnlineToLibrary,
    dismissPlaybackError,
    attachAudioElement,
    detachAudioElement,
    playItems,
    enqueueItems,
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

import { computed, ref, watch } from 'vue'
import { defineStore } from 'pinia'
import { getStoredToken } from '@/api/client'
import { useThemeStore } from '@/stores/theme'
import { api, buildCoverUrl, buildStreamUrl, canPlayFormatNatively } from './api'
import type { PlayMode, StructuredLyricsDto, TrackDto, TranscodeProfile } from './types'

export const usePlayerStore = defineStore('player', () => {
  const themeStore = useThemeStore()
  const audio = new Audio()
  audio.preload = 'auto'

  const queue = ref<TrackDto[]>([])
  const currentIndex = ref<number>(-1)
  const isPlaying = ref(false)
  const currentTimeSecs = ref(0)
  const rawAudioDuration = ref(0)
  const bufferedRatio = ref(0)
  const streamOffsetSecs = ref(0)
  const currentStreamIsTranscoded = ref(false)
  const activeEffectiveProfile = ref<TranscodeProfile>('raw')
  const fallbackFormats = ref<Set<string>>(new Set())

  const lyricsOpen = ref(false)
  const queueOpen = ref(false)
  const lyricsLoading = ref(false)
  const lyrics = ref<StructuredLyricsDto | null>(null)

  let scrobbledCurrent = false
  let queueSyncTimer: ReturnType<typeof setTimeout> | null = null
  let pendingSeekHandler: (() => void) | null = null
  let lyricsLoadedTrackId: string | null = null
  let playHistory: number[] = []
  let shufflePool: number[] = []

  const currentTrack = computed<TrackDto | null>(() => {
    const idx = currentIndex.value
    if (idx < 0 || idx >= queue.value.length) return null
    return queue.value[idx] ?? null
  })

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
    () => themeStore.playback.transcode_profile,
  )
  const playMode = computed<PlayMode>(() => themeStore.playback.play_mode)
  const volume = computed<number>(() => themeStore.playback.volume)
  const muted = computed<boolean>(() => themeStore.playback.muted)

  const qualitySummary = computed<string>(() => {
    const tr = currentTrack.value
    if (!tr) return ''
    const prof = currentTrack.value ? activeEffectiveProfile.value : transcodeProfile.value
    if (prof === 'raw') {
      const fmt = tr.format.toUpperCase()
      const parts: string[] = [fmt]
      if (tr.bit_depth && tr.sample_rate_hz) {
        const khz = Number((tr.sample_rate_hz / 1000).toFixed(1))
        parts.push(`${tr.bit_depth}bit/${khz}kHz`)
      } else if (tr.bit_rate_kbps) {
        parts.push(`${tr.bit_rate_kbps}k`)
      }
      return parts.join(' ')
    }
    if (prof === 'opus128') return 'OPUS 128k'
    if (prof === 'mp3192') return 'MP3 192k'
    if (prof === 'mp3320') return 'MP3 320k'
    return 'AAC 256k'
  })

  const activeLyricIndex = computed<number>(() => {
    const lyr = lyrics.value
    if (!lyr || !lyr.synced || lyr.lines.length === 0) return -1
    const nowMs = currentTimeSecs.value * 1000 + 120 + (lyr.offset_ms || 0)
    let idx = -1
    for (let i = 0; i < lyr.lines.length; i++) {
      if (lyr.lines[i]!.start_ms <= nowMs) {
        idx = i
      } else {
        break
      }
    }
    return idx
  })

  function syncAudioGain(): void {
    audio.volume = Math.max(0, Math.min(1, volume.value))
    audio.muted = muted.value
  }

  watch([volume, muted], syncAudioGain, { immediate: true })

  audio.addEventListener('timeupdate', () => {
    const t = streamOffsetSecs.value + (audio.currentTime || 0)
    currentTimeSecs.value = t

    if (audio.buffered.length > 0 && durationSecs.value > 0) {
      const end = streamOffsetSecs.value + audio.buffered.end(audio.buffered.length - 1)
      bufferedRatio.value = Math.max(0, Math.min(1, end / durationSecs.value))
    }

    const tr = currentTrack.value
    if (tr && !scrobbledCurrent && durationSecs.value > 0) {
      const threshold = Math.min(240, Math.max(3, durationSecs.value * 0.5))
      if (t >= threshold) {
        scrobbledCurrent = true
        tr.play_count += 1
        void api.scrobble(tr.id, true).catch(() => {})
      }
    }
  })

  audio.addEventListener('durationchange', () => {
    rawAudioDuration.value = audio.duration || 0
  })

  audio.addEventListener('play', () => {
    isPlaying.value = true
  })

  audio.addEventListener('pause', () => {
    isPlaying.value = false
    scheduleQueueSync()
  })

  audio.addEventListener('ended', () => {
    if (playMode.value === 'loop_one' && currentTrack.value) {
      void loadAndPlayCurrent(0, true)
      return
    }
    playNext()
  })

  function updateMediaSession(track: TrackDto | null): void {
    if (!('mediaSession' in navigator) || typeof MediaMetadata === 'undefined') return
    try {
      if (!track) {
        navigator.mediaSession.metadata = null
        return
      }
      const relCover = track.cover_key ? buildCoverUrl(track.id, 512) : null
      const absCover = relCover ? new URL(relCover, window.location.origin).href : null
      navigator.mediaSession.metadata = new MediaMetadata({
        title: track.title,
        artist: track.artist_name,
        album: track.album_title,
        artwork: absCover ? [{ src: absCover, sizes: '512x512' }] : [],
      })
      navigator.mediaSession.setActionHandler('play', () => void togglePlay())
      navigator.mediaSession.setActionHandler('pause', () => void togglePlay())
      navigator.mediaSession.setActionHandler('previoustrack', () => playPrev())
      navigator.mediaSession.setActionHandler('nexttrack', () => playNext())
    } catch {
      // Ignore MediaSession errors on restricted/non-HTTPS contexts
    }
  }

  function clearPendingSeek(): void {
    if (pendingSeekHandler) {
      audio.removeEventListener('loadedmetadata', pendingSeekHandler)
      pendingSeekHandler = null
    }
  }

  async function fetchLyricsFor(trackId: string): Promise<void> {
    lyricsLoading.value = true
    try {
      const res = await api.getTrackLyrics(trackId)
      if (currentTrack.value?.id === trackId) {
        lyrics.value = res
        lyricsLoadedTrackId = trackId
      }
    } catch {
      if (currentTrack.value?.id === trackId) {
        lyrics.value = null
        lyricsLoadedTrackId = trackId
      }
    } finally {
      if (currentTrack.value?.id === trackId) {
        lyricsLoading.value = false
      }
    }
  }

  watch(lyricsOpen, (isOpen) => {
    const tr = currentTrack.value
    if (isOpen && tr && lyricsLoadedTrackId !== tr.id && !lyricsLoading.value) {
      void fetchLyricsFor(tr.id)
    }
  })

  function resetShuffleAndHistory(): void {
    playHistory = []
    shufflePool = []
  }

  function refillShufflePool(len: number, excludeIndex: number): void {
    const indices: number[] = []
    for (let i = 0; i < len; i++) {
      if (i !== excludeIndex) indices.push(i)
    }
    for (let i = indices.length - 1; i > 0; i--) {
      const j = Math.floor(Math.random() * (i + 1))
      const tmp = indices[i]!
      indices[i] = indices[j]!
      indices[j] = tmp
    }
    shufflePool = indices
  }

  async function loadAndPlayCurrent(
    startAtSecs = 0,
    autoPlay = true,
    overrideProfile?: TranscodeProfile,
  ): Promise<void> {
    const track = currentTrack.value
    if (!track) return

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
    bufferedRatio.value = 0

    clearPendingSeek()
    audio.src = buildStreamUrl(track.id, profile, streamOffsetSecs.value, track.format)
    audio.load()

    if (!useServerOffset && startAtSecs > 0) {
      const targetTrackId = track.id
      const handler = (): void => {
        clearPendingSeek()
        if (currentTrack.value?.id === targetTrackId) {
          audio.currentTime = startAtSecs
        }
      }
      pendingSeekHandler = handler
      audio.addEventListener('loadedmetadata', handler, { once: true })
    }

    updateMediaSession(track)
    if (lyricsLoadedTrackId !== track.id) {
      lyrics.value = null
      lyricsLoadedTrackId = null
      lyricsLoading.value = false
    }
    if (lyricsOpen.value && lyricsLoadedTrackId !== track.id) {
      void fetchLyricsFor(track.id)
    }

    if (autoPlay) {
      try {
        await audio.play()
        void api.scrobble(track.id, false).catch(() => {})
      } catch (err) {
        if (!overrideProfile && err instanceof DOMException && err.name === 'NotSupportedError') {
          fallbackFormats.value.add(formatKey)
          await loadAndPlayCurrent(startAtSecs, true, 'mp3320')
          return
        }
        isPlaying.value = false
      }
    }
    scheduleQueueSync()
  }

  function playQueue(tracks: TrackDto[], startIndex = 0): void {
    if (tracks.length === 0) return
    queue.value = [...tracks]
    currentIndex.value = Math.max(0, Math.min(tracks.length - 1, startIndex))
    scrobbledCurrent = false
    resetShuffleAndHistory()
    void loadAndPlayCurrent(0, true)
  }

  function resumeQueue(tracks: TrackDto[], startIndex = 0, startAtSecs = 0): void {
    if (tracks.length === 0) return
    queue.value = [...tracks]
    currentIndex.value = Math.max(0, Math.min(tracks.length - 1, startIndex))
    scrobbledCurrent = false
    resetShuffleAndHistory()
    void loadAndPlayCurrent(Math.max(0, startAtSecs), true)
  }

  function shuffleAndPlay(tracks: TrackDto[]): void {
    if (tracks.length === 0) return
    const copy = [...tracks]
    for (let i = copy.length - 1; i > 0; i--) {
      const j = Math.floor(Math.random() * (i + 1))
      const tmp = copy[i]!
      copy[i] = copy[j]!
      copy[j] = tmp
    }
    themeStore.updatePlaybackPreferences({ play_mode: 'shuffle' })
    playQueue(copy, 0)
  }

  function playAtIndex(index: number): void {
    if (index < 0 || index >= queue.value.length) return
    if (index === currentIndex.value) {
      void togglePlay()
      return
    }
    if (currentIndex.value >= 0) {
      playHistory.push(currentIndex.value)
      if (playHistory.length > 200) playHistory.shift()
    }
    currentIndex.value = index
    shufflePool = shufflePool.filter((i) => i !== index)
    scrobbledCurrent = false
    void loadAndPlayCurrent(0, true)
  }

  function playTrackNow(track: TrackDto): void {
    playTrackAt(track, 0)
  }

  function playTrackAt(track: TrackDto, startAtSecs = 0): void {
    const existingIdx = queue.value.findIndex((t) => t.id === track.id)
    if (existingIdx >= 0) {
      if (currentIndex.value >= 0 && currentIndex.value !== existingIdx) {
        playHistory.push(currentIndex.value)
        if (playHistory.length > 200) playHistory.shift()
      }
      currentIndex.value = existingIdx
      shufflePool = shufflePool.filter((i) => i !== existingIdx)
      scrobbledCurrent = false
      void loadAndPlayCurrent(Math.max(0, startAtSecs), true)
    } else if (queue.value.length > 0) {
      const insertAt = currentIndex.value >= 0 ? currentIndex.value + 1 : queue.value.length
      if (currentIndex.value >= 0) {
        playHistory.push(currentIndex.value)
        if (playHistory.length > 200) playHistory.shift()
      }
      playHistory = playHistory.map((i) => (i >= insertAt ? i + 1 : i))
      shufflePool = shufflePool.map((i) => (i >= insertAt ? i + 1 : i))
      queue.value.splice(insertAt, 0, track)
      currentIndex.value = insertAt
      scrobbledCurrent = false
      void loadAndPlayCurrent(Math.max(0, startAtSecs), true)
    } else {
      resumeQueue([track], 0, startAtSecs)
    }
  }

  function enqueueNext(track: TrackDto): void {
    if (queue.value.length === 0) {
      playQueue([track], 0)
      return
    }
    const insertAt = currentIndex.value >= 0 ? currentIndex.value + 1 : queue.value.length
    playHistory = playHistory.map((i) => (i >= insertAt ? i + 1 : i))
    shufflePool = shufflePool.map((i) => (i >= insertAt ? i + 1 : i))
    shufflePool.push(insertAt)
    queue.value.splice(insertAt, 0, track)
    scheduleQueueSync()
  }

  function pause(): void {
    if (isPlaying.value || !audio.paused) {
      audio.pause()
    }
  }

  async function togglePlay(): Promise<void> {
    if (!currentTrack.value) return
    if (isPlaying.value) {
      audio.pause()
    } else {
      if (!audio.src) {
        await loadAndPlayCurrent(currentTimeSecs.value, true)
      } else {
        try {
          await audio.play()
        } catch {
          await loadAndPlayCurrent(currentTimeSecs.value, true)
        }
      }
    }
  }

  function playNext(): void {
    const len = queue.value.length
    if (len === 0) return
    if (currentIndex.value >= 0) {
      playHistory.push(currentIndex.value)
      if (playHistory.length > 200) playHistory.shift()
    }
    if (playMode.value === 'shuffle' && len > 1) {
      shufflePool = shufflePool.filter((i) => i >= 0 && i < len && i !== currentIndex.value)
      if (shufflePool.length === 0) {
        refillShufflePool(len, currentIndex.value)
      }
      currentIndex.value = shufflePool.pop() ?? (currentIndex.value + 1) % len
    } else {
      currentIndex.value = (currentIndex.value + 1) % len
    }
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
    if (playMode.value === 'shuffle') {
      while (playHistory.length > 0) {
        const prev = playHistory.pop()!
        if (prev >= 0 && prev < len && prev !== currentIndex.value) {
          currentIndex.value = prev
          scrobbledCurrent = false
          void loadAndPlayCurrent(0, true)
          return
        }
      }
    }
    currentIndex.value = (currentIndex.value - 1 + len) % len
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
    scheduleQueueSync()
  }

  function setTranscodeProfile(next: TranscodeProfile): void {
    if (next === transcodeProfile.value) return
    const resumeSecs = currentTimeSecs.value
    const wasPlaying = isPlaying.value
    themeStore.updatePlaybackPreferences({ transcode_profile: next })
    if (currentTrack.value) {
      void loadAndPlayCurrent(resumeSecs, wasPlaying)
    }
  }

  function cyclePlayMode(): void {
    const order: PlayMode[] = ['sequence', 'loop_one', 'shuffle']
    const idx = order.indexOf(playMode.value)
    const next = order[(idx + 1) % order.length]!
    themeStore.updatePlaybackPreferences({ play_mode: next })
  }

  function setVolume(nextVol: number): void {
    const clamped = Math.max(0, Math.min(1, nextVol))
    themeStore.updatePlaybackPreferences({
      volume: clamped,
      muted: clamped === 0 ? true : false,
    })
  }

  function toggleMute(): void {
    themeStore.updatePlaybackPreferences({ muted: !muted.value })
  }

  function removeQueueAt(idx: number): void {
    if (idx < 0 || idx >= queue.value.length) return
    const wasCurrent = idx === currentIndex.value
    queue.value.splice(idx, 1)
    playHistory = playHistory
      .filter((i) => i !== idx)
      .map((i) => (i > idx ? i - 1 : i))
    shufflePool = shufflePool
      .filter((i) => i !== idx)
      .map((i) => (i > idx ? i - 1 : i))
    if (queue.value.length === 0) {
      clearPendingSeek()
      audio.pause()
      audio.removeAttribute('src')
      currentIndex.value = -1
      currentTimeSecs.value = 0
      lyrics.value = null
      lyricsLoadedTrackId = null
      resetShuffleAndHistory()
    } else if (wasCurrent) {
      currentIndex.value = Math.min(idx, queue.value.length - 1)
      scrobbledCurrent = false
      void loadAndPlayCurrent(0, isPlaying.value)
    } else if (idx < currentIndex.value) {
      currentIndex.value -= 1
    }
    scheduleQueueSync()
  }

  function removeByTrackId(trackId: string): void {
    for (let i = queue.value.length - 1; i >= 0; i--) {
      if (queue.value[i]?.id === trackId) {
        removeQueueAt(i)
      }
    }
  }

  function clearQueue(): void {
    clearPendingSeek()
    audio.pause()
    audio.removeAttribute('src')
    queue.value = []
    currentIndex.value = -1
    currentTimeSecs.value = 0
    lyrics.value = null
    lyricsLoadedTrackId = null
    resetShuffleAndHistory()
    scheduleQueueSync()
  }

  function resetForLogout(): void {
    if (queueSyncTimer) {
      clearTimeout(queueSyncTimer)
      queueSyncTimer = null
    }
    clearPendingSeek()
    audio.pause()
    audio.removeAttribute('src')
    audio.load()
    queue.value = []
    currentIndex.value = -1
    isPlaying.value = false
    currentTimeSecs.value = 0
    rawAudioDuration.value = 0
    bufferedRatio.value = 0
    streamOffsetSecs.value = 0
    currentStreamIsTranscoded.value = false
    activeEffectiveProfile.value = 'raw'
    fallbackFormats.value.clear()
    lyricsOpen.value = false
    queueOpen.value = false
    lyricsLoading.value = false
    lyrics.value = null
    lyricsLoadedTrackId = null
    scrobbledCurrent = false
    resetShuffleAndHistory()
    updateMediaSession(null)
  }

  function scheduleQueueSync(): void {
    if (!getStoredToken()) return
    if (queueSyncTimer) clearTimeout(queueSyncTimer)
    queueSyncTimer = setTimeout(() => {
      queueSyncTimer = null
      void api
        .savePlayQueue({
          track_ids: queue.value.map((t) => t.id),
          current_track_id: currentTrack.value?.id ?? null,
          position_ms: Math.round(currentTimeSecs.value * 1000),
        })
        .catch(() => {})
    }, 800)
  }

  function flushQueueOnPageHide(): void {
    const token = getStoredToken()
    if (!token || queue.value.length === 0) return
    if (queueSyncTimer) {
      clearTimeout(queueSyncTimer)
      queueSyncTimer = null
    }
    try {
      void fetch('/api/v1/music/play-queue', {
        method: 'PUT',
        headers: {
          'Content-Type': 'application/json',
          Authorization: `Bearer ${token}`,
        },
        body: JSON.stringify({
          track_ids: queue.value.map((t) => t.id),
          current_track_id: currentTrack.value?.id ?? null,
          position_ms: Math.round(currentTimeSecs.value * 1000),
        }),
        keepalive: true,
      })
    } catch {
      // ignore unload errors
    }
  }

  if (typeof window !== 'undefined') {
    window.addEventListener('pagehide', flushQueueOnPageHide)
  }

  async function restoreRemoteQueue(force = false): Promise<void> {
    if (!force && queue.value.length > 0) return
    try {
      const saved = await api.getPlayQueue()
      if (!saved || saved.tracks.length === 0) return
      queue.value = saved.tracks
      const foundIdx = saved.current_track_id
        ? saved.tracks.findIndex((t) => t.id === saved.current_track_id)
        : 0
      currentIndex.value = foundIdx >= 0 ? foundIdx : 0
      const resumeSecs = Math.max(0, (saved.position_ms || 0) / 1000)
      scrobbledCurrent = false
      await loadAndPlayCurrent(resumeSecs, false)
    } catch {
      // ignore
    }
  }

  return {
    queue,
    currentIndex,
    currentTrack,
    isPlaying,
    currentTimeSecs,
    durationSecs,
    bufferedPercent,
    transcodeProfile,
    playMode,
    volume,
    muted,
    qualitySummary,
    lyricsOpen,
    queueOpen,
    lyricsLoading,
    lyrics,
    activeLyricIndex,
    playQueue,
    resumeQueue,
    shuffleAndPlay,
    playAtIndex,
    playTrackNow,
    playTrackAt,
    enqueueNext,
    pause,
    togglePlay,
    playNext,
    playPrev,
    seekTo,
    setTranscodeProfile,
    cyclePlayMode,
    setVolume,
    toggleMute,
    removeQueueAt,
    removeByTrackId,
    clearQueue,
    resetForLogout,
    restoreRemoteQueue,
  }
})

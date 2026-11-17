import { computed, ref, watch } from 'vue'
import { defineStore } from 'pinia'
import { api } from '@/api/client'
import { useThemeStore } from '@/stores/theme'
import type { StructuredLyricsDto, TrackDto } from '@/types/api'

export const useMusicPlayerStore = defineStore('musicPlayer', () => {
  const theme = useThemeStore()
  const audio = new Audio()
  audio.preload = 'auto'

  const queue = ref<TrackDto[]>([])
  const currentIndex = ref(-1)
  const isPlaying = ref(false)
  const currentTimeSecs = ref(0)
  const rawDuration = ref(0)
  const streamOffsetSecs = ref(0)
  const lyricsOpen = ref(false)
  const lyrics = ref<StructuredLyricsDto | null>(null)

  let scrobbledCurrent = false

  const currentTrack = computed<TrackDto | null>(() => {
    const i = currentIndex.value
    return i >= 0 && i < queue.value.length ? (queue.value[i] ?? null) : null
  })

  const durationSecs = computed(() => {
    const d = currentTrack.value?.duration_secs ?? 0
    return d > 0 ? d : streamOffsetSecs.value + (rawDuration.value || 0)
  })

  const activeLyricIndex = computed(() => {
    const lyr = lyrics.value
    if (!lyr || !lyr.synced || lyr.lines.length === 0) return -1
    const nowMs = currentTimeSecs.value * 1000 + 120
    let idx = -1
    for (let i = 0; i < lyr.lines.length; i++) {
      if (lyr.lines[i]!.start_ms <= nowMs) idx = i
      else break
    }
    return idx
  })

  watch(
    () => [theme.music.volume, theme.music.muted] as const,
    ([vol, muted]) => {
      audio.volume = Math.max(0, Math.min(1, vol))
      audio.muted = muted
    },
    { immediate: true },
  )

  audio.addEventListener('timeupdate', () => {
    const t = streamOffsetSecs.value + (audio.currentTime || 0)
    currentTimeSecs.value = t
    const tr = currentTrack.value
    if (tr && !scrobbledCurrent && durationSecs.value > 0) {
      const threshold = Math.min(240, Math.max(3, durationSecs.value * 0.5))
      if (t >= threshold) {
        scrobbledCurrent = true
        void api.scrobbleMusic(tr.id, true).catch(() => {})
      }
    }
  })

  audio.addEventListener('durationchange', () => {
    rawDuration.value = Number.isFinite(audio.duration) ? audio.duration : 0
  })

  audio.addEventListener('play', () => {
    isPlaying.value = true
  })

  audio.addEventListener('pause', () => {
    isPlaying.value = false
  })

  audio.addEventListener('ended', () => {
    if (theme.music.play_mode === 'loop_one' && currentTrack.value) {
      void loadAndPlay(0, true)
      return
    }
    next()
  })

  function resolveTranscodeOpts(offsetSecs: number): {
    format?: string
    maxBitRate?: number
    timeOffset?: number
  } {
    const prof = theme.music.transcode_profile
    if (prof === 'opus128') return { format: 'opus', maxBitRate: 128, timeOffset: offsetSecs }
    if (prof === 'mp3192') return { format: 'mp3', maxBitRate: 192, timeOffset: offsetSecs }
    if (prof === 'mp3320') return { format: 'mp3', maxBitRate: 320, timeOffset: offsetSecs }
    if (prof === 'aac256') return { format: 'aac', maxBitRate: 256, timeOffset: offsetSecs }
    return offsetSecs > 0 ? { format: 'mp3', maxBitRate: 320, timeOffset: offsetSecs } : {}
  }

  async function loadAndPlay(startOffset = 0, autoplay = true): Promise<void> {
    const tr = currentTrack.value
    if (!tr) return
    scrobbledCurrent = false
    streamOffsetSecs.value = startOffset
    currentTimeSecs.value = startOffset

    const opts = resolveTranscodeOpts(startOffset)
    audio.src = api.musicStreamUrl(tr.id, opts)
    audio.load()
    void fetchLyrics(tr.id)
    void syncRemoteQueue()

    if (autoplay) {
      try {
        await audio.play()
      } catch {
        isPlaying.value = false
      }
    }
  }

  async function fetchLyrics(trackId: string): Promise<void> {
    try {
      lyrics.value = await api.getTrackLyrics(trackId)
    } catch {
      lyrics.value = null
    }
  }

  async function syncRemoteQueue(): Promise<void> {
    if (queue.value.length === 0) return
    try {
      await api.savePlayQueue(
        queue.value.map((t) => t.id),
        currentTrack.value?.id ?? null,
        Math.round(currentTimeSecs.value * 1000),
      )
    } catch {
      // ignore transient sync error
    }
  }

  async function restoreRemoteQueue(): Promise<void> {
    if (queue.value.length > 0) return
    try {
      const saved = await api.getPlayQueue()
      if (saved && saved.tracks.length > 0) {
        queue.value = saved.tracks
        const idx = saved.current_track_id
          ? saved.tracks.findIndex((t) => t.id === saved.current_track_id)
          : 0
        currentIndex.value = idx >= 0 ? idx : 0
        void fetchLyrics(queue.value[currentIndex.value]!.id)
      }
    } catch {
      // ignore
    }
  }

  function playTracks(tracks: TrackDto[], startIndex = 0): void {
    if (tracks.length === 0) return
    queue.value = [...tracks]
    currentIndex.value = Math.max(0, Math.min(tracks.length - 1, startIndex))
    void loadAndPlay(0, true)
  }

  function togglePlay(): void {
    if (!currentTrack.value) return
    if (isPlaying.value) {
      audio.pause()
    } else {
      if (!audio.src) {
        void loadAndPlay(0, true)
      } else {
        void audio.play().catch(() => {})
      }
    }
  }

  function pause(): void {
    if (isPlaying.value) audio.pause()
  }

  function seekTo(secs: number): void {
    if (!currentTrack.value) return
    const target = Math.max(0, Math.min(durationSecs.value || secs, secs))
    if (theme.music.transcode_profile === 'raw' && streamOffsetSecs.value === 0) {
      audio.currentTime = target
      currentTimeSecs.value = target
    } else {
      void loadAndPlay(Math.floor(target), isPlaying.value)
    }
  }

  function prev(): void {
    if (queue.value.length === 0) return
    if (currentTimeSecs.value > 4) {
      seekTo(0)
      return
    }
    currentIndex.value = (currentIndex.value - 1 + queue.value.length) % queue.value.length
    void loadAndPlay(0, true)
  }

  function next(): void {
    if (queue.value.length === 0) return
    if (theme.music.play_mode === 'shuffle' && queue.value.length > 1) {
      currentIndex.value = Math.floor(Math.random() * queue.value.length)
    } else {
      currentIndex.value = (currentIndex.value + 1) % queue.value.length
    }
    void loadAndPlay(0, true)
  }

  function cyclePlayMode(): void {
    const order = ['sequence', 'shuffle', 'loop_one'] as const
    const idx = order.indexOf(theme.music.play_mode)
    theme.music.play_mode = order[(idx + 1) % order.length]!
    theme.persistPreferences()
  }

  return {
    queue,
    currentIndex,
    currentTrack,
    isPlaying,
    currentTimeSecs,
    durationSecs,
    lyricsOpen,
    lyrics,
    activeLyricIndex,
    playTracks,
    togglePlay,
    pause,
    seekTo,
    prev,
    next,
    cyclePlayMode,
    restoreRemoteQueue,
  }
})

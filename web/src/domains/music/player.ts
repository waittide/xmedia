import { computed, ref, watch } from 'vue'
import { defineStore } from 'pinia'
import { useAuthStore } from '@/stores/auth'
import { useSnackbarStore } from '@/stores/snackbar'
import { useThemeStore } from '@/stores/theme'
import { api, buildCoverUrl, buildStreamUrl, canPlayFormatNatively } from './api'
import {
  onlineMusicApi,
  resolveOnlineMusicAudioUrl,
  resolveOnlineMusicCoverUrl,
  type OnlineMusicResolvedTrackDto,
  type OnlineMusicTrackDto,
} from './onlineApi'
import type { PlayMode, StructuredLyricsDto, TrackDto, TranscodeProfile } from './types'

export interface LocalQueueItem {
  kind: 'local'
  key: string
  track: TrackDto
}

export interface OnlineQueueItem {
  kind: 'online'
  key: string
  source_id: string
  source_name: string
  track: OnlineMusicTrackDto
  resolved: OnlineMusicResolvedTrackDto | null
  resolvedAtMs: number | null
  selectedQuality: string | null
  activeFormat: string | null
  activeBitrate: number | null
}

export type PlayerQueueItem = LocalQueueItem | OnlineQueueItem

export interface PlayerTrackDisplay {
  kind: 'local' | 'online'
  id: string
  title: string
  artist_name: string
  album_title: string
  duration_secs: number
  cover_id: string | null
  cover_key: string | null
  cover_url: string | null
  artist_id: string | null
  album_id: string | null
  starred_at: string | null
  source_id: string | null
  source_name: string | null
  qualities: string[]
}

export function buildOnlineTrackKey(track: OnlineMusicTrackDto): string {
  const fallback = track.detail_url || track.play_url || track.url_id || track.title
  return `online:${track.source_id}:${track.track_id || ''}:${fallback}`
}

export function resolveOnlineItemCoverUrl(item: OnlineQueueItem): string | null {
  const resolved = resolveOnlineMusicCoverUrl(
    item.resolved?.proxy_cover_url || item.track.proxy_cover_url,
    item.resolved?.cover_url || item.track.cover_url,
    item.source_id,
  )
  return resolved || null
}

export function resolveOnlineItemStreamUrl(item: OnlineQueueItem): string {
  return resolveOnlineMusicAudioUrl(
    item.resolved?.proxy_stream_url,
    item.resolved?.stream_url,
    item.source_id,
  )
}

function makeLocalQueueItem(track: TrackDto): LocalQueueItem {
  return {
    kind: 'local',
    key: track.id,
    track,
  }
}

function pickDefaultOnlineQuality(qualities: string[]): string | null {
  if (!qualities || qualities.length === 0) return null
  if (qualities.includes('320')) return '320'
  return qualities[0] ?? null
}

function makeOnlineQueueItem(track: OnlineMusicTrackDto): OnlineQueueItem {
  return {
    kind: 'online',
    key: buildOnlineTrackKey(track),
    source_id: track.source_id,
    source_name: track.source_name,
    track,
    resolved: null,
    resolvedAtMs: null,
    selectedQuality: pickDefaultOnlineQuality(track.qualities),
    activeFormat: track.format ?? null,
    activeBitrate: null,
  }
}

function isOnlineItemExpired(item: OnlineQueueItem): boolean {
  return (
    item.resolved?.expires_in_secs != null &&
    item.resolvedAtMs != null &&
    Date.now() - item.resolvedAtMs >= item.resolved.expires_in_secs * 1000
  )
}

export const usePlayerStore = defineStore('player', () => {
  const themeStore = useThemeStore()
  const audio = new Audio()
  audio.preload = 'auto'
  audio.hidden = true
  audio.setAttribute('aria-hidden', 'true')
  audio.dataset.musicPlayerAudio = '1'

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
  let queueHadLocalOnClear = false

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

  const isCurrentOnline = computed<boolean>(() => currentItem.value?.kind === 'online')

  const currentTrack = computed<PlayerTrackDisplay | null>(() => {
    const item = currentItem.value
    if (!item) return null
    if (item.kind === 'local') {
      const tr = item.track
      return {
        kind: 'local',
        id: tr.id,
        title: tr.title,
        artist_name: tr.artist_name,
        album_title: tr.album_title,
        duration_secs: tr.duration_secs,
        cover_id: tr.id,
        cover_key: tr.cover_key,
        cover_url: null,
        artist_id: tr.artist_id,
        album_id: tr.album_id,
        starred_at: tr.starred_at,
        source_id: null,
        source_name: null,
        qualities: [],
      }
    }
    const base = item.track
    const resolvedTrack = item.resolved?.track
    return {
      kind: 'online',
      id: item.key,
      title: resolvedTrack?.title || base.title,
      artist_name: resolvedTrack?.artist || base.artist,
      album_title: resolvedTrack?.album ?? base.album ?? '',
      duration_secs: resolvedTrack?.duration_secs ?? base.duration_secs ?? 0,
      cover_id: null,
      cover_key: null,
      cover_url: resolveOnlineItemCoverUrl(item),
      artist_id: null,
      album_id: null,
      starred_at: null,
      source_id: item.source_id,
      source_name: item.source_name,
      qualities: base.qualities,
    }
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
    const item = currentItem.value
    if (!item) return ''
    if (item.kind === 'online') {
      const fmt = (item.activeFormat || item.track.format || '').toUpperCase()
      const bitrate = item.activeBitrate
      if (fmt && bitrate) return `${fmt} ${bitrate}k`
      if (fmt) return fmt
      if (item.selectedQuality) {
        const q = item.selectedQuality.toUpperCase()
        return /^\d+$/.test(q) ? `${q}k` : q
      }
      return ''
    }
    const tr = item.track
    const prof = activeEffectiveProfile.value
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

    const item = currentItem.value
    if (item?.kind === 'local' && !scrobbledCurrent && durationSecs.value > 0) {
      const threshold = Math.min(240, Math.max(3, durationSecs.value * 0.5))
      if (t >= threshold) {
        scrobbledCurrent = true
        item.track.play_count += 1
        void api.scrobble(item.track.id, true).catch(() => {})
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
      scrobbledCurrent = false
      void loadAndPlayCurrent(0, true)
      return
    }
    playNext()
  })

  audio.addEventListener('error', () => {
    if (!audio.src || !currentItem.value) return
    isPlaying.value = false
    if (currentItem.value.kind === 'online') {
      playbackError.value = audio.error?.message || 'Audio playback error'
    }
  })

  function updateMediaSession(track: PlayerTrackDisplay | null): void {
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

  async function fetchLyricsFor(item: PlayerQueueItem): Promise<void> {
    const targetKey = item.key
    if (item.kind === 'online' && item.resolved) {
      if (currentItem.value?.key === targetKey) {
        lyrics.value = item.resolved.lyrics ?? null
        lyricsLoadedTrackId = targetKey
        lyricsLoading.value = false
      }
      return
    }
    lyricsLoading.value = true
    try {
      const res =
        item.kind === 'local'
          ? await api.getTrackLyrics(item.track.id)
          : await onlineMusicApi.fetchLyrics({
              source_id: item.source_id,
              track_id: item.track.track_id || null,
              lyric_id: item.track.lyric_id || null,
              title: item.track.title || null,
              artist: item.track.artist || null,
              detail_url: item.track.detail_url || null,
            })
      if (currentItem.value?.key === targetKey) {
        lyrics.value = res
        lyricsLoadedTrackId = targetKey
      }
    } catch {
      if (currentItem.value?.key === targetKey) {
        lyrics.value = null
        lyricsLoadedTrackId = targetKey
      }
    } finally {
      if (currentItem.value?.key === targetKey) {
        lyricsLoading.value = false
      }
    }
  }

  watch(lyricsOpen, (isOpen) => {
    const item = currentItem.value
    if (
      isOpen &&
      item &&
      lyricsLoadedTrackId !== item.key &&
      !lyricsLoading.value &&
      !(item.kind === 'online' && onlineResolving.value)
    ) {
      void fetchLyricsFor(item)
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

  let loadSeq = 0
  let queueEpoch = 0
  let queueDirty = false

  async function loadAndPlayCurrent(
    startAtSecs = 0,
    autoPlay = true,
    overrideProfileOrQuality?: TranscodeProfile | string,
  ): Promise<void> {
    const item = currentItem.value
    if (!item) return
    const seq = ++loadSeq
    const targetKey = item.key

    if (item.kind === 'local') {
      onlineResolving.value = false
      playbackError.value = null
      const track = item.track
      const overrideProfile =
        typeof overrideProfileOrQuality === 'string' &&
        (overrideProfileOrQuality === 'raw' ||
          overrideProfileOrQuality === 'opus128' ||
          overrideProfileOrQuality === 'mp3192' ||
          overrideProfileOrQuality === 'mp3320' ||
          overrideProfileOrQuality === 'aac256')
          ? (overrideProfileOrQuality as TranscodeProfile)
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

      clearPendingSeek()
      audio.src = buildStreamUrl(track.id, profile, streamOffsetSecs.value, track.format)
      audio.load()

      if (!useServerOffset && startAtSecs > 0) {
        const handler = (): void => {
          clearPendingSeek()
          if (seq === loadSeq && currentItem.value?.key === targetKey) {
            audio.currentTime = startAtSecs
          }
        }
        pendingSeekHandler = handler
        audio.addEventListener('loadedmetadata', handler, { once: true })
      }

      updateMediaSession(currentTrack.value)
      if (lyricsLoadedTrackId !== targetKey) {
        lyrics.value = null
        lyricsLoadedTrackId = null
        lyricsLoading.value = false
      }
      if (lyricsOpen.value && lyricsLoadedTrackId !== targetKey) {
        void fetchLyricsFor(item)
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
          isPlaying.value = false
        }
      }
      if (seq === loadSeq) {
        scheduleQueueSync()
      }
      return
    }

    // Online track resolution and playback
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
    clearPendingSeek()
    audio.pause()
    audio.removeAttribute('src')
    audio.load()

    if (lyricsLoadedTrackId !== targetKey) {
      lyrics.value = null
      lyricsLoadedTrackId = null
      lyricsLoading.value = false
    }
    updateMediaSession(currentTrack.value)

    const isExpired = isOnlineItemExpired(item)
    const requestedQuality =
      overrideQuality ?? item.selectedQuality ?? pickDefaultOnlineQuality(item.track.qualities)
    const needsFullInfo = !item.resolved || isExpired
    const needsQualitySwitch =
      !needsFullInfo && overrideQuality !== undefined && overrideQuality !== item.selectedQuality

    if (needsFullInfo || needsQualitySwitch) {
      onlineResolving.value = true
      try {
        if (needsFullInfo) {
          const resolved = await onlineMusicApi.fetchTrackInfo({
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
          })
          if (seq !== loadSeq || currentItem.value?.key !== targetKey) return
          item.resolved = resolved
          item.resolvedAtMs = Date.now()
          item.selectedQuality = requestedQuality
          item.activeFormat = resolved.format ?? item.track.format
          item.activeBitrate = resolved.bitrate
          lyrics.value = resolved.lyrics ?? null
          lyricsLoadedTrackId = targetKey
          lyricsLoading.value = false
        } else if (needsQualitySwitch && item.resolved) {
          const streamRes = await onlineMusicApi.fetchStream({
            source_id: item.source_id,
            track_id: item.resolved.track.track_id || item.track.track_id || null,
            url_id: item.resolved.track.url_id ?? item.track.url_id ?? null,
            play_url: item.resolved.track.play_url ?? item.track.play_url ?? null,
            quality: overrideQuality ?? null,
            detail_url: item.resolved.track.detail_url ?? item.track.detail_url ?? null,
            title: item.resolved.track.title || item.track.title || null,
            artist: item.resolved.track.artist || item.track.artist || null,
          })
          if (seq !== loadSeq || currentItem.value?.key !== targetKey) return
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
        if (seq !== loadSeq || currentItem.value?.key !== targetKey) return
        const msg = err instanceof Error ? err.message : String(err)
        playbackError.value = msg
        isPlaying.value = false
        useSnackbarStore().show({ message: msg, tone: 'error' })
        return
      } finally {
        if (seq === loadSeq) {
          onlineResolving.value = false
        }
      }
    } else if (item.resolved) {
      lyrics.value = item.resolved.lyrics ?? null
      lyricsLoadedTrackId = targetKey
      lyricsLoading.value = false
    }

    if (seq !== loadSeq || currentItem.value?.key !== targetKey) return

    const streamUrl = resolveOnlineItemStreamUrl(item)
    if (!streamUrl) {
      const msg = 'No playable stream URL resolved'
      playbackError.value = msg
      isPlaying.value = false
      useSnackbarStore().show({ message: msg, tone: 'error' })
      return
    }

    audio.src = streamUrl
    audio.load()

    if (startAtSecs > 0) {
      const handler = (): void => {
        clearPendingSeek()
        if (seq === loadSeq && currentItem.value?.key === targetKey) {
          audio.currentTime = startAtSecs
        }
      }
      pendingSeekHandler = handler
      audio.addEventListener('loadedmetadata', handler, { once: true })
    }

    updateMediaSession(currentTrack.value)

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

  function playQueue(tracks: TrackDto[], startIndex = 0): void {
    if (tracks.length === 0) return
    queueEpoch += 1
    queueHadLocalOnClear = false
    queue.value = tracks.map(makeLocalQueueItem)
    currentIndex.value = Math.max(0, Math.min(tracks.length - 1, startIndex))
    scrobbledCurrent = false
    resetShuffleAndHistory()
    void loadAndPlayCurrent(0, true)
  }

  function resumeQueue(tracks: TrackDto[], startIndex = 0, startAtSecs = 0): void {
    if (tracks.length === 0) return
    queueHadLocalOnClear = false
    queue.value = tracks.map(makeLocalQueueItem)
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

  function playOnlineTracks(tracks: OnlineMusicTrackDto[], startIndex = 0): void {
    if (tracks.length === 0) return
    queueEpoch += 1
    queueHadLocalOnClear = false
    queue.value = tracks.map(makeOnlineQueueItem)
    currentIndex.value = Math.max(0, Math.min(tracks.length - 1, startIndex))
    scrobbledCurrent = false
    resetShuffleAndHistory()
    void loadAndPlayCurrent(0, true)
  }

  function shuffleAndPlayOnline(tracks: OnlineMusicTrackDto[]): void {
    if (tracks.length === 0) return
    const copy = [...tracks]
    for (let i = copy.length - 1; i > 0; i--) {
      const j = Math.floor(Math.random() * (i + 1))
      const tmp = copy[i]!
      copy[i] = copy[j]!
      copy[j] = tmp
    }
    themeStore.updatePlaybackPreferences({ play_mode: 'shuffle' })
    playOnlineTracks(copy, 0)
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
    const existingIdx = queue.value.findIndex(
      (item) => item.kind === 'local' && item.track.id === track.id,
    )
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
      queue.value.splice(insertAt, 0, makeLocalQueueItem(track))
      currentIndex.value = insertAt
      scrobbledCurrent = false
      void loadAndPlayCurrent(Math.max(0, startAtSecs), true)
    } else {
      resumeQueue([track], 0, startAtSecs)
    }
  }

  function playOnlineTrackNow(track: OnlineMusicTrackDto): void {
    const targetKey = buildOnlineTrackKey(track)
    const existingIdx = queue.value.findIndex((item) => item.key === targetKey)
    if (existingIdx >= 0) {
      if (currentIndex.value >= 0 && currentIndex.value !== existingIdx) {
        playHistory.push(currentIndex.value)
        if (playHistory.length > 200) playHistory.shift()
      }
      currentIndex.value = existingIdx
      shufflePool = shufflePool.filter((i) => i !== existingIdx)
      scrobbledCurrent = false
      void loadAndPlayCurrent(0, true)
    } else if (queue.value.length > 0) {
      const insertAt = currentIndex.value >= 0 ? currentIndex.value + 1 : queue.value.length
      if (currentIndex.value >= 0) {
        playHistory.push(currentIndex.value)
        if (playHistory.length > 200) playHistory.shift()
      }
      playHistory = playHistory.map((i) => (i >= insertAt ? i + 1 : i))
      shufflePool = shufflePool.map((i) => (i >= insertAt ? i + 1 : i))
      queue.value.splice(insertAt, 0, makeOnlineQueueItem(track))
      currentIndex.value = insertAt
      scrobbledCurrent = false
      void loadAndPlayCurrent(0, true)
    } else {
      playOnlineTracks([track], 0)
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
    queue.value.splice(insertAt, 0, makeLocalQueueItem(track))
    scheduleQueueSync()
  }

  function enqueueOnlineTrackNext(track: OnlineMusicTrackDto): void {
    if (queue.value.length === 0) {
      playOnlineTracks([track], 0)
      return
    }
    const insertAt = currentIndex.value >= 0 ? currentIndex.value + 1 : queue.value.length
    playHistory = playHistory.map((i) => (i >= insertAt ? i + 1 : i))
    shufflePool = shufflePool.map((i) => (i >= insertAt ? i + 1 : i))
    shufflePool.push(insertAt)
    queue.value.splice(insertAt, 0, makeOnlineQueueItem(track))
  }

  function enqueueOnlineTrack(track: OnlineMusicTrackDto): void {
    if (queue.value.length === 0) {
      playOnlineTracks([track], 0)
      return
    }
    const key = buildOnlineTrackKey(track)
    if (queue.value.some((item) => item.key === key)) return
    queue.value.push(makeOnlineQueueItem(track))
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
    if (!item) return
    if (isPlaying.value) {
      audio.pause()
    } else {
      if (item.kind === 'online' && (!item.resolved || isOnlineItemExpired(item))) {
        await loadAndPlayCurrent(currentTimeSecs.value, true)
        return
      }
      if (!audio.src || audio.error) {
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
    if (currentItem.value?.kind === 'local') {
      void loadAndPlayCurrent(resumeSecs, wasPlaying)
    }
  }

  function setOnlineQuality(quality: string): void {
    const item = currentOnlineItem.value
    if (!item || !quality || item.selectedQuality === quality) return
    const resumeSecs = currentTimeSecs.value
    const wasPlaying = isPlaying.value
    void loadAndPlayCurrent(resumeSecs, wasPlaying, quality)
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
    const removedItem = queue.value[idx]
    const wasCurrent = idx === currentIndex.value
    queue.value.splice(idx, 1)
    playHistory = playHistory
      .filter((i) => i !== idx)
      .map((i) => (i > idx ? i - 1 : i))
    shufflePool = shufflePool
      .filter((i) => i !== idx)
      .map((i) => (i > idx ? i - 1 : i))
    if (removedItem?.kind === 'local' && !queue.value.some((i) => i.kind === 'local')) {
      queueHadLocalOnClear = true
    }
    if (queue.value.length === 0) {
      loadSeq += 1
      clearPendingSeek()
      audio.pause()
      audio.removeAttribute('src')
      currentIndex.value = -1
      currentTimeSecs.value = 0
      rawAudioDuration.value = 0
      bufferedRatio.value = 0
      onlineResolving.value = false
      playbackError.value = null
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
      const item = queue.value[i]
      if (item?.kind === 'local' && item.track.id === trackId) {
        removeQueueAt(i)
      }
    }
  }

  function clearQueue(): void {
    loadSeq += 1
    queueHadLocalOnClear = queue.value.some((i) => i.kind === 'local')
    clearPendingSeek()
    audio.pause()
    audio.removeAttribute('src')
    queue.value = []
    currentIndex.value = -1
    currentTimeSecs.value = 0
    rawAudioDuration.value = 0
    bufferedRatio.value = 0
    onlineResolving.value = false
    playbackError.value = null
    lyrics.value = null
    lyricsLoadedTrackId = null
    resetShuffleAndHistory()
    scheduleQueueSync()
  }

  function resetForLogout(): void {
    loadSeq += 1
    queueEpoch += 1
    queueDirty = false
    queueHadLocalOnClear = false
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
    onlineResolving.value = false
    playbackError.value = null
    lyricsOpen.value = false
    queueOpen.value = false
    lyricsLoading.value = false
    lyrics.value = null
    lyricsLoadedTrackId = null
    scrobbledCurrent = false
    resetShuffleAndHistory()
    updateMediaSession(null)
  }

  function buildLocalQueueSyncPayload(): {
    track_ids: string[]
    current_track_id: string | null
    position_ms: number
  } | null {
    const localItems = queue.value.filter((i): i is LocalQueueItem => i.kind === 'local')
    if (localItems.length === 0) {
      if (queueHadLocalOnClear) {
        queueHadLocalOnClear = false
        return {
          track_ids: [],
          current_track_id: null,
          position_ms: 0,
        }
      }
      return null
    }
    queueHadLocalOnClear = false
    const cur = currentItem.value
    return {
      track_ids: localItems.map((i) => i.track.id),
      current_track_id: cur?.kind === 'local' ? cur.track.id : (localItems[0]?.track.id ?? null),
      position_ms: cur?.kind === 'local' ? Math.round(currentTimeSecs.value * 1000) : 0,
    }
  }

  function scheduleQueueSync(): void {
    if (!useAuthStore().isAuthenticated) return
    const hasLocal = queue.value.some((i) => i.kind === 'local')
    if (!hasLocal && !queueHadLocalOnClear) {
      return
    }
    queueDirty = true
    if (queueSyncTimer) clearTimeout(queueSyncTimer)
    const epoch = queueEpoch
    queueSyncTimer = setTimeout(() => {
      queueSyncTimer = null
      if (!useAuthStore().isAuthenticated || epoch !== queueEpoch) return
      queueDirty = false
      const payload = buildLocalQueueSyncPayload()
      if (!payload) return
      void api.savePlayQueue(payload).catch(() => {})
    }, 800)
  }

  function flushQueueOnPageHide(): void {
    if (!useAuthStore().isAuthenticated) return
    if (!queueDirty && queue.value.length === 0) return
    queueDirty = false
    if (queueSyncTimer) {
      clearTimeout(queueSyncTimer)
      queueSyncTimer = null
    }
    const payload = buildLocalQueueSyncPayload()
    if (!payload) return
    try {
      void fetch('/api/v1/music/play-queue', {
        method: 'PUT',
        credentials: 'include',
        headers: {
          'Content-Type': 'application/json',
        },
        body: JSON.stringify(payload),
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
    const epoch = queueEpoch
    try {
      const saved = await api.getPlayQueue()
      if (epoch !== queueEpoch) return
      if (!force && queue.value.length > 0) return
      if (!saved || saved.tracks.length === 0) return
      queueHadLocalOnClear = false
      queue.value = saved.tracks.map(makeLocalQueueItem)
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
    currentItem,
    currentLocalTrack,
    currentOnlineItem,
    isCurrentOnline,
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
    lyricsOpen,
    queueOpen,
    lyricsLoading,
    lyrics,
    activeLyricIndex,
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
    enqueueNext,
    enqueueOnlineTrackNext,
    enqueueOnlineTrack,
    isOnlineTrackInQueue,
    pause,
    togglePlay,
    playNext,
    playPrev,
    seekTo,
    setTranscodeProfile,
    setOnlineQuality,
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

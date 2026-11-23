<script setup lang="ts">
import { computed, onBeforeUnmount, onMounted, ref, watch } from 'vue'
import { useRoute, useRouter } from 'vue-router'
import Hls from 'hls.js'
import type { SelectOption } from '@/components/ui'
import { usePlayerStore } from '@/domains/music/player'
import { useThemeStore } from '@/stores/theme'
import { formatDuration } from '@/utils/format'
import {
  api,
  buildPosterUrl,
  buildSubtitleVttUrl,
  buildVideoDownloadUrl,
  buildVideoStreamUrl,
} from '../api'
import VideoPlayerShell, { type VideoPlayerEpisodeItem } from '../components/VideoPlayerShell.vue'
import {
  clearVideoMediaSession,
  loadStoredVideoMuted,
  loadStoredVideoVolume,
  persistVideoAudioPrefs,
  setVideoMediaSessionState,
  updateVideoMediaSession,
} from '../composables/useVideoPlayerCore'
import { useVideoI18n as useI18n } from '../i18n'
import {
  onlineVideoApi,
  resolveOnlineVideoStreamUrl,
  resolveOnlineVideoSubtitleUrl,
  type OnlineVideoSubtitle,
  type TrackedVideoPlayResponseDto,
} from '../onlineApi'
import { useVideoStore } from '../store'
import type { VideoDetailDto, VideoQualityProfile, VideoSummaryDto } from '../types'

interface ResolvedTrackedSubtitle extends OnlineVideoSubtitle {
  resolvedUrl: string
}

const { t } = useI18n()
const route = useRoute()
const router = useRouter()
const themeStore = useThemeStore()
const musicPlayerStore = usePlayerStore()
const videoStore = useVideoStore()

const shellRef = ref<InstanceType<typeof VideoPlayerShell> | null>(null)
const videoEl = ref<HTMLVideoElement | null>(null)

const detail = ref<VideoDetailDto | null>(null)
const trackedPlayResult = ref<TrackedVideoPlayResponseDto | null>(null)
const trackedStreamType = ref<'hls' | 'mp4'>('mp4')
const siblingVideos = ref<VideoSummaryDto[]>([])
const quality = ref<VideoQualityProfile>(themeStore.video.default_quality || 'auto')
const selectedAudioStream = ref<number | null>(null)
const selectedSubtitleId = ref<string>('off')
const playbackRate = ref<number>(themeStore.video.playback_rate || 1.0)

const loadingVideo = ref(true)
const isBuffering = ref(false)
const loadError = ref<string | null>(null)

const isPlaying = ref(false)
const muted = ref(loadStoredVideoMuted())
const volume = ref(loadStoredVideoVolume())
const streamOffsetSecs = ref(0)
const currentTimeSecs = ref(0)
const rawDurationSecs = ref(0)

let hlsInstance: Hls | null = null
let activeAbort: AbortController | null = null
let progressSaveTimer: ReturnType<typeof setInterval> | null = null
let pendingLoadedMetadataHandler: (() => void) | null = null
let exitPersisted = false
let initRequestSeq = 0

const activeShelfId = computed(() => {
  const raw = route.query.shelf
  return typeof raw === 'string' && raw.trim() ? raw.trim() : null
})

const isUncachedTracked = computed(() =>
  Boolean(detail.value?.video.online_tracking && !detail.value.video.online_tracking.cached),
)

const isTranscodedStream = computed(() => {
  if (!detail.value || isUncachedTracked.value) return false
  if (quality.value === 'direct') return false
  if (quality.value !== 'auto') return true
  const defaultAudioIdx = detail.value.audio_streams[0]?.stream_index ?? null
  if (selectedAudioStream.value !== null && selectedAudioStream.value !== defaultAudioIdx) {
    return true
  }
  return !detail.value.video.can_direct_play
})

const resolvedTrackedSubtitles = computed<ResolvedTrackedSubtitle[]>(() => {
  const srcId =
    trackedPlayResult.value?.source_id ?? detail.value?.video.online_tracking?.source_id ?? null
  return (trackedPlayResult.value?.subtitles ?? [])
    .map((s) => ({
      ...s,
      resolvedUrl: resolveOnlineVideoSubtitleUrl(s.proxy_url, s.url, srcId),
    }))
    .filter((s) => Boolean(s.resolvedUrl))
})

const totalDurationSecs = computed(() => {
  const metaDur = detail.value?.video.duration_secs ?? 0
  if (metaDur > 0) return metaDur
  if (Number.isFinite(rawDurationSecs.value) && rawDurationSecs.value > 0) {
    return streamOffsetSecs.value + rawDurationSecs.value
  }
  return 0
})

const activeSubtitleUrl = computed(() => {
  if (!detail.value || !selectedSubtitleId.value || selectedSubtitleId.value === 'off') {
    return null
  }
  if (isUncachedTracked.value) {
    const match = /^online-(\d+)$/.exec(selectedSubtitleId.value)
    if (!match) return null
    const idx = Number.parseInt(match[1]!, 10)
    return resolvedTrackedSubtitles.value[idx]?.resolvedUrl ?? null
  }
  return buildSubtitleVttUrl(
    detail.value.video.id,
    selectedSubtitleId.value,
    isTranscodedStream.value ? streamOffsetSecs.value : 0,
  )
})

const headerTitle = computed(() => detail.value?.video.title || '')

const headerSubtitle = computed(() => {
  const v = detail.value?.video
  if (!v) return ''
  const parts: string[] = []
  if (v.show_title) parts.push(v.show_title)
  if (v.season_number != null && v.episode_number != null) {
    parts.push(
      `S${String(v.season_number).padStart(2, '0')}E${String(v.episode_number).padStart(2, '0')}`,
    )
  }
  if (isUncachedTracked.value && trackedPlayResult.value) {
    parts.push(
      trackedStreamType.value === 'hls' ? t('online.streamTypeHls') : t('online.streamTypeMp4'),
    )
  }
  return parts.join(' · ')
})

const qualityOptions = computed<SelectOption[]>(() => [
  { value: 'auto', label: t('player.qualityAuto') },
  { value: 'direct', label: t('player.qualityDirect') },
  { value: 'p1080', label: t('player.quality1080p') },
  { value: 'p720', label: t('player.quality720p') },
  { value: 'p480', label: t('player.quality480p') },
])

const audioOptions = computed<SelectOption[]>(() => {
  const streams = detail.value?.audio_streams ?? []
  return streams.map((s, idx) => {
    const parts = [
      s.title || s.language?.toUpperCase() || `${t('player.audioTrack')} #${idx + 1}`,
      s.codec.toUpperCase(),
      s.channels ? `${s.channels}ch` : '',
    ].filter(Boolean)
    return {
      value: String(s.stream_index),
      label: parts.join(' · '),
    }
  })
})

const subtitleOptions = computed<SelectOption[]>(() => {
  if (isUncachedTracked.value) {
    return [
      { value: 'off', label: t('player.subtitleOff') },
      ...resolvedTrackedSubtitles.value.map((sub, idx) => ({
        value: `online-${idx}`,
        label: sub.label || sub.lang || `Sub #${idx + 1}`,
      })),
    ]
  }
  return [
    { value: 'off', label: t('player.subtitleOff') },
    ...(detail.value?.subtitles ?? []).map((sub) => ({
      value: sub.id,
      label: sub.label,
    })),
  ]
})

const currentSiblingIndex = computed(() => {
  const curId = detail.value?.video.id
  if (!curId || siblingVideos.value.length === 0) return -1
  return siblingVideos.value.findIndex((v) => v.id === curId)
})

const prevEpisodeId = computed<string | null>(() => {
  if (activeShelfId.value && currentSiblingIndex.value > 0) {
    return siblingVideos.value[currentSiblingIndex.value - 1]?.id ?? null
  }
  return detail.value?.prev_episode_id ?? null
})

const nextEpisodeId = computed<string | null>(() => {
  if (
    activeShelfId.value &&
    currentSiblingIndex.value >= 0 &&
    currentSiblingIndex.value + 1 < siblingVideos.value.length
  ) {
    return siblingVideos.value[currentSiblingIndex.value + 1]?.id ?? null
  }
  return detail.value?.next_episode_id ?? null
})

const episodeDrawerItems = computed<VideoPlayerEpisodeItem[]>(() => {
  const curId = detail.value?.video.id
  return siblingVideos.value.map((v) => {
    const epCode =
      v.season_number != null && v.episode_number != null
        ? `S${String(v.season_number).padStart(2, '0')}E${String(v.episode_number).padStart(2, '0')}`
        : ''
    const dur = v.duration_secs > 0 ? formatDuration(v.duration_secs) : ''
    return {
      id: v.id,
      label: epCode ? `${epCode} · ${v.title}` : v.title,
      subtitle: dur || undefined,
      active: v.id === curId,
      watched: Boolean(v.progress?.completed),
    }
  })
})

const downloadUrl = computed(() =>
  detail.value && !isUncachedTracked.value ? buildVideoDownloadUrl(detail.value.video.id) : null,
)

function clearPendingSeekHandler(): void {
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

function stopAndClearVideoElement(): void {
  clearPendingSeekHandler()
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

function syncMediaSession(d: VideoDetailDto): void {
  updateVideoMediaSession({
    title: d.video.title,
    artist: d.video.show_title || undefined,
    album:
      d.video.season_number != null
        ? t('catalog.seasonHeader', { season: d.video.season_number })
        : undefined,
    posterUrl: buildPosterUrl(d.video.poster_key, 512),
    onPlay: () => {
      const el = videoEl.value
      if (el && el.paused) {
        musicPlayerStore.pause()
        void el.play().catch(() => {
          isPlaying.value = false
        })
      }
    },
    onPause: () => {
      videoEl.value?.pause()
    },
    onSeekBy: (delta) => seekTo(currentTimeSecs.value + delta),
    onSeekTo: (target) => seekTo(target),
    onPrev: prevEpisodeId.value ? () => goToPrevEpisode() : null,
    onNext: nextEpisodeId.value ? () => goToNextEpisode() : null,
  })
}

async function loadSiblingQueue(d: VideoDetailDto, reqSeq: number): Promise<void> {
  try {
    if (activeShelfId.value) {
      const shelf = await api.getShelf(activeShelfId.value)
      if (reqSeq !== initRequestSeq) return
      siblingVideos.value = shelf.videos
      syncMediaSession(d)
      return
    }
    if (d.video.show_id) {
      const show = await api.getShow(d.video.show_id)
      if (reqSeq !== initRequestSeq) return
      siblingVideos.value = show.seasons.flatMap((sg) => sg.episodes)
      syncMediaSession(d)
      return
    }
    siblingVideos.value = []
  } catch {
    // ignore non-fatal sidebar queue errors
  }
}

function loadStreamAt(startSecs: number, autoPlay = true): void {
  const el = videoEl.value
  const d = detail.value
  if (!el || !d) return

  clearPendingSeekHandler()
  musicPlayerStore.pause()
  loadError.value = null
  isBuffering.value = true

  const useServerOffset = isTranscodedStream.value && startSecs > 0
  streamOffsetSecs.value = useServerOffset ? Math.floor(startSecs) : 0
  currentTimeSecs.value = startSecs

  el.src = buildVideoStreamUrl(
    d.video.id,
    quality.value,
    selectedAudioStream.value,
    streamOffsetSecs.value,
  )
  el.load()
  el.volume = volume.value
  el.muted = muted.value

  const targetRate = playbackRate.value
  const handler = (): void => {
    clearPendingSeekHandler()
    el.playbackRate = targetRate
    if (!useServerOffset && startSecs > 0) {
      el.currentTime = startSecs
    }
  }
  pendingLoadedMetadataHandler = handler
  el.addEventListener('loadedmetadata', handler, { once: true })
  el.playbackRate = targetRate

  if (autoPlay) {
    void el.play().catch(() => {
      isPlaying.value = false
      isBuffering.value = false
    })
  }
}

async function loadTrackedOnlineStream(
  videoId: string,
  startSecs: number,
  reqSeq: number,
  signal: AbortSignal,
): Promise<void> {
  const res = await onlineVideoApi.resolveTrackedPlay(videoId, { signal })
  if (reqSeq !== initRequestSeq) return

  if (res.cached) {
    if (detail.value?.video.online_tracking) {
      detail.value = {
        ...detail.value,
        video: {
          ...detail.value.video,
          online_tracking: { ...detail.value.video.online_tracking, cached: true },
        },
      }
    }
    loadStreamAt(startSecs, true)
    return
  }

  trackedPlayResult.value = res
  const rawStream = res.stream_url || ''
  const isHls = res.stream_type === 'hls' || /\.m3u8(\?|$)/i.test(rawStream)
  trackedStreamType.value = isHls ? 'hls' : 'mp4'
  const proxyHint = isHls
    ? res.stream_type === 'hls' && res.proxy_stream_url?.includes('hls-proxy')
      ? res.proxy_stream_url
      : ''
    : res.proxy_stream_url
  const srcId = res.source_id ?? detail.value?.video.online_tracking?.source_id ?? null
  const playableUrl = resolveOnlineVideoStreamUrl(
    proxyHint,
    rawStream,
    isHls ? 'hls' : 'mp4',
    srcId,
  )

  if (!playableUrl) {
    loadError.value = t('online.streamLoadFailed')
    return
  }

  const el = videoEl.value
  if (!el) return

  clearPendingSeekHandler()
  destroyHls()
  streamOffsetSecs.value = 0
  currentTimeSecs.value = startSecs
  el.volume = volume.value
  el.muted = muted.value
  el.playbackRate = playbackRate.value
  isBuffering.value = true

  const attachMetadataSeek = (): void => {
    const targetRate = playbackRate.value
    const handler = (): void => {
      clearPendingSeekHandler()
      el.playbackRate = targetRate
      if (startSecs > 0) {
        const maxDur = Number.isFinite(el.duration) && el.duration > 0 ? el.duration : startSecs
        const target = Math.min(startSecs, Math.max(0, maxDur - 1))
        if (target > 0) {
          el.currentTime = target
          currentTimeSecs.value = target
        }
      }
    }
    pendingLoadedMetadataHandler = handler
    el.addEventListener('loadedmetadata', handler, { once: true })
  }

  if (isHls) {
    if (Hls.isSupported()) {
      let networkRetries = 0
      let mediaRetries = 0
      const unrecoverableManifestErrors = new Set<string>([
        Hls.ErrorDetails.MANIFEST_PARSING_ERROR,
        Hls.ErrorDetails.MANIFEST_INCOMPATIBLE_CODECS_ERROR,
        Hls.ErrorDetails.LEVEL_EMPTY_ERROR,
      ])
      const manifestLoadErrors = new Set<string>([
        Hls.ErrorDetails.MANIFEST_LOAD_ERROR,
        Hls.ErrorDetails.MANIFEST_LOAD_TIMEOUT,
      ])
      const hls = new Hls({
        startPosition: startSecs > 0 ? startSecs : -1,
        xhrSetup: (xhr) => {
          xhr.withCredentials = true
        },
      })
      hlsInstance = hls
      hls.on(Hls.Events.MANIFEST_PARSED, () => {
        if (reqSeq !== initRequestSeq) return
        el.playbackRate = playbackRate.value
        if (startSecs > 0 && Math.abs((el.currentTime || 0) - startSecs) > 1.5) {
          el.currentTime = startSecs
        }
        isBuffering.value = false
        void el.play().catch(() => {
          isPlaying.value = false
          isBuffering.value = false
        })
      })
      hls.on(Hls.Events.ERROR, (_event, data) => {
        if (reqSeq !== initRequestSeq || !data.fatal) return
        if (!unrecoverableManifestErrors.has(data.details)) {
          if (data.type === Hls.ErrorTypes.NETWORK_ERROR && networkRetries < 1) {
            networkRetries += 1
            if (manifestLoadErrors.has(data.details) || hls.levels.length === 0) {
              hls.loadSource(playableUrl)
            } else {
              hls.startLoad()
            }
            return
          }
          if (data.type === Hls.ErrorTypes.MEDIA_ERROR && mediaRetries < 1) {
            mediaRetries += 1
            hls.recoverMediaError()
            return
          }
        }
        stopAndClearVideoElement()
        isPlaying.value = false
        isBuffering.value = false
        loadError.value = data.reason || t('online.streamLoadFailed')
      })
      hls.loadSource(playableUrl)
      hls.attachMedia(el)
    } else if (el.canPlayType('application/vnd.apple.mpegurl') !== '') {
      attachMetadataSeek()
      el.src = playableUrl
      el.load()
      void el.play().catch(() => {
        isPlaying.value = false
        isBuffering.value = false
      })
    } else {
      isBuffering.value = false
      loadError.value = t('player.unsupportedHls')
    }
  } else {
    attachMetadataSeek()
    el.src = playableUrl
    el.load()
    void el.play().catch(() => {
      isPlaying.value = false
      isBuffering.value = false
    })
  }
}

async function initVideo(): Promise<void> {
  const reqSeq = ++initRequestSeq
  activeAbort?.abort()
  const controller = new AbortController()
  activeAbort = controller
  exitPersisted = false
  stopAndClearVideoElement()
  trackedPlayResult.value = null
  isPlaying.value = false
  loadingVideo.value = true
  isBuffering.value = false
  loadError.value = null
  rawDurationSecs.value = 0
  streamOffsetSecs.value = 0
  musicPlayerStore.pause()

  const id = String(route.params.id || '').trim()
  if (!id) {
    loadError.value = t('player.loadErrorDesc')
    loadingVideo.value = false
    return
  }

  try {
    const d = await api.getVideo(id)
    if (reqSeq !== initRequestSeq) return
    detail.value = d
    syncMediaSession(d)
    void loadSiblingQueue(d, reqSeq)

    const savedProg = d.video.progress
    if (savedProg?.selected_audio_stream != null) {
      selectedAudioStream.value = savedProg.selected_audio_stream
    } else {
      const defAudio = d.audio_streams.find((a) => a.is_default) ?? d.audio_streams[0]
      selectedAudioStream.value = defAudio ? defAudio.stream_index : null
    }

    if (d.video.online_tracking && !d.video.online_tracking.cached) {
      selectedSubtitleId.value = 'off'
    } else if (savedProg?.selected_subtitle) {
      selectedSubtitleId.value = savedProg.selected_subtitle
    } else {
      const defSub = d.subtitles.find((s) => s.is_default)
      selectedSubtitleId.value = defSub ? defSub.id : 'off'
    }

    const qStartRaw = typeof route.query.t === 'string' ? Number.parseFloat(route.query.t) : Number.NaN
    const resumePos =
      Number.isFinite(qStartRaw) && qStartRaw > 0
        ? qStartRaw
        : savedProg && !savedProg.completed && savedProg.position_secs > 3
          ? savedProg.position_secs
          : 0

    if (d.video.online_tracking && !d.video.online_tracking.cached) {
      await loadTrackedOnlineStream(d.video.id, resumePos, reqSeq, controller.signal)
    } else {
      loadStreamAt(resumePos, true)
    }
    shellRef.value?.showHud()
  } catch (err) {
    if (reqSeq !== initRequestSeq) return
    if (err instanceof Error && err.name === 'AbortError') return
    isPlaying.value = false
    isBuffering.value = false
    loadError.value = err instanceof Error && err.message ? err.message : t('player.loadErrorDesc')
  } finally {
    if (reqSeq === initRequestSeq) {
      loadingVideo.value = false
    }
  }
}

async function persistProgress(
  completedOverride?: boolean,
  options?: { bumpCatalog?: boolean },
): Promise<void> {
  const d = detail.value
  if (!d) return
  const roundedDur = Math.round(totalDurationSecs.value)
  const completedPayload =
    completedOverride !== undefined
      ? completedOverride
      : roundedDur > 0
        ? undefined
        : false
  try {
    await api.saveProgress(d.video.id, {
      position_secs: currentTimeSecs.value,
      duration_secs: roundedDur > 0 ? roundedDur : undefined,
      completed: completedPayload,
      selected_audio_stream: selectedAudioStream.value,
      selected_subtitle: selectedSubtitleId.value,
    })
    if (options?.bumpCatalog) {
      videoStore.bumpCatalogVersion()
    }
  } catch {
    // ignore transient network errors
  }
}

function togglePlay(): void {
  const el = videoEl.value
  if (!el || !detail.value || loadError.value) return
  if (el.paused) {
    musicPlayerStore.pause()
    void el.play().catch(() => {
      isPlaying.value = false
    })
  } else {
    el.pause()
  }
}

function seekTo(targetSecs: number): void {
  const el = videoEl.value
  if (!el || !detail.value) return
  const clamped = Math.max(0, Math.min(totalDurationSecs.value || targetSecs, targetSecs))
  if (!isTranscodedStream.value) {
    streamOffsetSecs.value = 0
    el.currentTime = clamped
    currentTimeSecs.value = clamped
  } else {
    loadStreamAt(clamped, !el.paused)
  }
  void persistProgress()
}

function onQualityChange(val: string): void {
  const next = val as VideoQualityProfile
  if (next === quality.value) return
  quality.value = next
  themeStore.updateVideoPreferences({ default_quality: next })
  loadStreamAt(currentTimeSecs.value, isPlaying.value)
}

function onAudioStreamChange(val: string): void {
  const nextIdx = Number.parseInt(val, 10)
  if (!Number.isFinite(nextIdx) || nextIdx === selectedAudioStream.value) return
  selectedAudioStream.value = nextIdx
  loadStreamAt(currentTimeSecs.value, isPlaying.value)
  void persistProgress()
}

function onSubtitleChange(val: string): void {
  selectedSubtitleId.value = val
  void persistProgress()
}

function onPlaybackRateChange(rate: number): void {
  playbackRate.value = rate
  if (videoEl.value) videoEl.value.playbackRate = rate
  themeStore.updateVideoPreferences({ playback_rate: rate })
}

function setVolume(val: number): void {
  const clamped = Math.max(0, Math.min(1, Math.round(val * 100) / 100))
  volume.value = clamped
  muted.value = clamped === 0
  if (videoEl.value) {
    videoEl.value.volume = clamped
    videoEl.value.muted = muted.value
  }
  persistVideoAudioPrefs(volume.value, muted.value)
}

function adjustVolume(delta: number): void {
  const base = muted.value && volume.value === 0 ? 0 : volume.value
  setVolume(base + delta)
}

function toggleMute(): void {
  if (muted.value && volume.value === 0) {
    volume.value = 0.5
    muted.value = false
  } else {
    muted.value = !muted.value
  }
  if (videoEl.value) {
    videoEl.value.volume = volume.value
    videoEl.value.muted = muted.value
  }
  persistVideoAudioPrefs(volume.value, muted.value)
}

function navigateToVideo(videoId: string): void {
  if (!videoId || videoId === detail.value?.video.id) return
  void persistProgress()
  const query = activeShelfId.value ? { shelf: activeShelfId.value } : undefined
  void router.replace({ path: `/video/play/${encodeURIComponent(videoId)}`, query })
}

function goToPrevEpisode(): void {
  if (prevEpisodeId.value) navigateToVideo(prevEpisodeId.value)
}

function goToNextEpisode(): void {
  if (nextEpisodeId.value) navigateToVideo(nextEpisodeId.value)
}

function onTimeUpdate(): void {
  const el = videoEl.value
  if (!el) return
  currentTimeSecs.value = streamOffsetSecs.value + (el.currentTime || 0)
}

function onDurationChange(): void {
  if (videoEl.value) {
    rawDurationSecs.value = Number.isFinite(videoEl.value.duration) ? videoEl.value.duration : 0
  }
}

function onVideoPlay(): void {
  isPlaying.value = true
  isBuffering.value = false
  musicPlayerStore.pause()
  setVideoMediaSessionState('playing')
}

function onVideoPause(): void {
  isPlaying.value = false
  setVideoMediaSessionState('paused')
  if (exitPersisted || videoEl.value?.ended) return
  void persistProgress()
}

function onVideoEnded(): void {
  isPlaying.value = false
  isBuffering.value = false
  setVideoMediaSessionState('paused')
  void persistProgress(true, { bumpCatalog: true })
  if (themeStore.video.autoplay_next && nextEpisodeId.value) {
    navigateToVideo(nextEpisodeId.value)
  }
}

function onVideoError(): void {
  const el = videoEl.value
  if (!detail.value || !el || (!el.getAttribute('src') && !hlsInstance) || !el.error) return
  stopAndClearVideoElement()
  isPlaying.value = false
  isBuffering.value = false
  loadError.value = isUncachedTracked.value
    ? t('online.streamLoadFailed')
    : t('player.loadErrorDesc')
}

function goBack(): void {
  if (!exitPersisted) {
    exitPersisted = true
    void persistProgress(undefined, { bumpCatalog: true })
  }
  if (window.history.state?.back) {
    router.back()
  } else if (detail.value?.video.show_id) {
    void router.push(`/video/item/${encodeURIComponent(detail.value.video.show_id)}`)
  } else if (detail.value?.video.id) {
    void router.push({
      path: `/video/item/${encodeURIComponent(detail.value.video.id)}`,
      query: { kind: 'movie' },
    })
  } else {
    void router.push('/video')
  }
}

function flushVideoProgressOnPageHide(): void {
  const d = detail.value
  if (!d || exitPersisted) return
  exitPersisted = true
  const roundedDur = Math.round(totalDurationSecs.value)
  try {
    void fetch(`/api/v1/video/videos/${encodeURIComponent(d.video.id)}/progress`, {
      method: 'PUT',
      credentials: 'include',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({
        position_secs: currentTimeSecs.value,
        duration_secs: roundedDur > 0 ? roundedDur : undefined,
        selected_audio_stream: selectedAudioStream.value,
        selected_subtitle: selectedSubtitleId.value === 'off' ? null : selectedSubtitleId.value,
      }),
      keepalive: true,
    })
  } catch {
    // ignore unload errors
  }
}

onMounted(() => {
  musicPlayerStore.pause()
  if (videoEl.value) {
    videoEl.value.volume = volume.value
    videoEl.value.muted = muted.value
  }
  void initVideo()
  window.addEventListener('pagehide', flushVideoProgressOnPageHide)
  progressSaveTimer = setInterval(() => {
    if (isPlaying.value) {
      void persistProgress()
    }
  }, 8000)
})

onBeforeUnmount(() => {
  initRequestSeq += 1
  activeAbort?.abort()
  activeAbort = null
  if (!exitPersisted) {
    exitPersisted = true
    void persistProgress(undefined, { bumpCatalog: true })
  }
  stopAndClearVideoElement()
  clearVideoMediaSession()
  if (progressSaveTimer) clearInterval(progressSaveTimer)
  window.removeEventListener('pagehide', flushVideoProgressOnPageHide)
})

watch(
  () => route.params.id,
  () => {
    void initVideo()
  },
)
</script>

<template>
  <VideoPlayerShell
    ref="shellRef"
    :title="headerTitle"
    :subtitle="headerSubtitle"
    :back-label="t('player.back')"
    :loading="loadingVideo"
    :loading-text="t('player.loading')"
    :buffering="isBuffering"
    :error="loadError"
    :error-title="t('player.loadErrorTitle')"
    :show-retry="true"
    :is-playing="isPlaying"
    :current-time="currentTimeSecs"
    :duration="totalDurationSecs"
    :volume="volume"
    :muted="muted"
    :playback-rate="playbackRate"
    :has-prev="Boolean(prevEpisodeId)"
    :has-next="Boolean(nextEpisodeId)"
    :show-footer="Boolean(detail)"
    :header-select-value="isUncachedTracked ? '' : quality"
    :header-select-options="detail && !isUncachedTracked ? qualityOptions : []"
    :header-select-aria-label="t('player.quality')"
    :quality-value="isUncachedTracked ? '' : quality"
    :quality-options="detail && !isUncachedTracked ? qualityOptions : []"
    :audio-value="String(selectedAudioStream ?? detail?.audio_streams[0]?.stream_index ?? 0)"
    :audio-options="isUncachedTracked ? [] : audioOptions"
    :subtitle-value="selectedSubtitleId"
    :subtitle-options="subtitleOptions"
    :download-url="downloadUrl"
    :episodes="episodeDrawerItems"
    :video-element="videoEl"
    @back="goBack"
    @retry="initVideo"
    @toggle-play="togglePlay"
    @seek="seekTo"
    @seek-delta="seekTo(currentTimeSecs + $event)"
    @prev="goToPrevEpisode"
    @next="goToNextEpisode"
    @update:volume="setVolume"
    @adjust-volume="adjustVolume"
    @toggle-mute="toggleMute"
    @update:playback-rate="onPlaybackRateChange"
    @header-select-change="onQualityChange"
    @update:quality="onQualityChange"
    @update:audio="onAudioStreamChange"
    @update:subtitle="onSubtitleChange"
    @select-episode="navigateToVideo"
  >
    <video
      ref="videoEl"
      class="h-full w-full bg-black object-contain"
      crossorigin="use-credentials"
      playsinline
      @loadstart="isBuffering = true"
      @waiting="isBuffering = true"
      @canplay="isBuffering = false"
      @playing="isBuffering = false"
      @play="onVideoPlay"
      @pause="onVideoPause"
      @timeupdate="onTimeUpdate"
      @durationchange="onDurationChange"
      @ended="onVideoEnded"
      @error="onVideoError"
    >
      <track
        v-if="activeSubtitleUrl"
        :key="activeSubtitleUrl"
        kind="subtitles"
        :src="activeSubtitleUrl"
        srclang="und"
        label="Subtitles"
        default
      />
    </video>
  </VideoPlayerShell>
</template>

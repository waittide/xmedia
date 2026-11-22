<script setup lang="ts">
import { computed, onBeforeUnmount, onMounted, ref, watch } from 'vue'
import { useRoute, useRouter } from 'vue-router'
import Hls from 'hls.js'
import {
  LuAlertCircle,
  LuArrowLeft,
  LuLoader2,
  LuMaximize,
  LuPause,
  LuPictureInPicture2,
  LuPlay,
  LuRefreshCw,
  LuSkipBack,
  LuSkipForward,
  LuVolume2,
  LuVolumeX,
} from 'vue-icons-plus/lu'
import { isSafeHttpUrl } from '@/components/online-sources/types'
import AppSelect from '@/components/ui/AppSelect.vue'
import AppSlider from '@/components/ui/AppSlider.vue'
import { useEscapeStack } from '@/composables/useEscapeStack'
import { usePlayerStore } from '@/domains/music/player'
import { useThemeStore } from '@/stores/theme'
import { formatTime } from '@/utils/format'
import { useVideoI18n as useI18n } from '../i18n'
import {
  ONLINE_VIDEO_PLAYER_CTX_KEY,
  onlineVideoApi,
  resolveOnlineVideoPosterUrl,
  resolveOnlineVideoStreamUrl,
  type OnlineVideoEpisode,
  type OnlineVideoPlayGroup,
  type OnlineVideoPlayResponseDto,
  type OnlineVideoPlayerContextStorage,
  type OnlineVideoSubtitle,
} from '../onlineApi'

const VOLUME_STORAGE_KEY = 'xmedia.video.volume'
const MUTED_STORAGE_KEY = 'xmedia.video.muted'

function loadStoredVolume(): number {
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

function loadStoredMuted(): boolean {
  try {
    return window.localStorage.getItem(MUTED_STORAGE_KEY) === 'true'
  } catch {
    return false
  }
}

function persistAudioPrefs(vol: number, isMuted: boolean): void {
  try {
    window.localStorage.setItem(VOLUME_STORAGE_KEY, String(vol))
    window.localStorage.setItem(MUTED_STORAGE_KEY, String(isMuted))
  } catch {
    // ignore storage errors
  }
}

const { t } = useI18n()
const route = useRoute()
const router = useRouter()
const themeStore = useThemeStore()
const musicPlayerStore = usePlayerStore()

const stageRef = ref<HTMLDivElement | null>(null)
const videoEl = ref<HTMLVideoElement | null>(null)

const sourceId = ref('')
const sourceName = ref('')
const videoId = ref('')
const detailUrl = ref('')
const title = ref('')
const coverUrl = ref<string | null>(null)
const proxyCoverUrl = ref<string | null>(null)

const playGroups = ref<OnlineVideoPlayGroup[]>([])
const activeGroupName = ref('')
const currentEpisodeIndex = ref(0)
const currentPlayUrl = ref('')

const playResult = ref<OnlineVideoPlayResponseDto | null>(null)
const activeStreamType = ref<'hls' | 'mp4'>('mp4')
const selectedSubtitleIdx = ref<string>('off')
const playbackRate = ref<number>(themeStore.video.playback_rate || 1.0)

const loadingStream = ref(true)
const isBuffering = ref(false)
const loadError = ref<string | null>(null)

const isPlaying = ref(false)
const muted = ref(loadStoredMuted())
const volume = ref(loadStoredVolume())
const currentTimeSecs = ref(0)
const durationSecs = ref(0)
const hudVisible = ref(true)

const pipSupported = computed(
  () => typeof document !== 'undefined' && Boolean(document.pictureInPictureEnabled),
)

let hlsInstance: Hls | null = null
let hudHideTimer: ReturnType<typeof setTimeout> | null = null
let pendingLoadedMetadataHandler: (() => void) | null = null
let playSeq = 0
let detailSeq = 0
let unmounted = false

const activeGroup = computed<OnlineVideoPlayGroup | null>(() => {
  if (playGroups.value.length === 0) return null
  return (
    playGroups.value.find((g) => g.name === activeGroupName.value) ??
    playGroups.value[0] ??
    null
  )
})

const activeEpisodes = computed<OnlineVideoEpisode[]>(() => activeGroup.value?.episodes ?? [])

const currentEpisode = computed<OnlineVideoEpisode | null>(
  () => activeEpisodes.value[currentEpisodeIndex.value] ?? null,
)

const hasPrevEpisode = computed(() => currentEpisodeIndex.value > 0)
const hasNextEpisode = computed(
  () => currentEpisodeIndex.value >= 0 && currentEpisodeIndex.value + 1 < activeEpisodes.value.length,
)

const groupOptions = computed(() =>
  playGroups.value.map((g) => ({
    value: g.name,
    label: g.name,
    badge: g.episodes.length,
  })),
)

const safeSubtitles = computed<OnlineVideoSubtitle[]>(() =>
  (playResult.value?.subtitles ?? []).filter((s) => isSafeHttpUrl(s.url)),
)

const subtitleOptions = computed(() => [
  { value: 'off', label: t('player.subtitleOff') },
  ...safeSubtitles.value.map((sub, idx) => ({
    value: String(idx),
    label: sub.label || sub.lang || `Sub #${idx + 1}`,
  })),
])

const activeSubtitle = computed<OnlineVideoSubtitle | null>(() => {
  if (selectedSubtitleIdx.value === 'off') return null
  const idx = Number.parseInt(selectedSubtitleIdx.value, 10)
  if (!Number.isFinite(idx) || idx < 0) return null
  return safeSubtitles.value[idx] ?? null
})

const speedOptions = [
  { value: '0.75', label: '0.75x' },
  { value: '1', label: '1.0x' },
  { value: '1.25', label: '1.25x' },
  { value: '1.5', label: '1.5x' },
  { value: '2', label: '2.0x' },
]

function readQueryParam(keys: string[]): string {
  for (const k of keys) {
    const val = route.query[k]
    if (typeof val === 'string' && val.trim()) {
      return val.trim()
    }
  }
  return ''
}

function readStoredPlayerContext(): OnlineVideoPlayerContextStorage | null {
  try {
    const raw = sessionStorage.getItem(ONLINE_VIDEO_PLAYER_CTX_KEY)
    if (!raw) return null
    const parsed: unknown = JSON.parse(raw)
    if (!parsed || typeof parsed !== 'object' || Array.isArray(parsed)) return null
    return parsed as OnlineVideoPlayerContextStorage
  } catch {
    return null
  }
}

function persistPlayerContext(): void {
  if (!sourceId.value || !currentPlayUrl.value) return
  const payload: OnlineVideoPlayerContextStorage = {
    sourceId: sourceId.value,
    sourceName: sourceName.value || undefined,
    videoId: videoId.value || undefined,
    detailUrl: detailUrl.value || undefined,
    title: title.value || undefined,
    coverUrl: coverUrl.value,
    proxyCoverUrl: proxyCoverUrl.value,
    groupName: activeGroupName.value || undefined,
    episodeIndex: currentEpisodeIndex.value,
    episodeName: currentEpisode.value?.name,
    playUrl: currentPlayUrl.value,
    playGroups: playGroups.value,
  }
  try {
    sessionStorage.setItem(ONLINE_VIDEO_PLAYER_CTX_KEY, JSON.stringify(payload))
  } catch {
    // ignore storage errors
  }
}

function hydrateContextFromRouteAndStorage(): void {
  const qSourceId = readQueryParam(['sourceId', 'source_id'])
  const qVideoId = readQueryParam(['videoId', 'video_id'])
  const qDetailUrl = readQueryParam(['detailUrl', 'detail_url'])
  const qGroupName = readQueryParam(['groupName', 'group_name'])
  const qEpIdxRaw = readQueryParam(['episodeIndex', 'episode_index'])
  const qPlayUrl = readQueryParam(['playUrl', 'play_url'])
  const qTitle = readQueryParam(['title', 'name'])
  const qSourceName = readQueryParam(['sourceName', 'source_name'])

  const stored = readStoredPlayerContext()
  const matchesStored =
    stored &&
    ((qSourceId &&
      stored.sourceId === qSourceId &&
      ((qPlayUrl && stored.playUrl === qPlayUrl) ||
        (qVideoId && stored.videoId === qVideoId) ||
        (qDetailUrl && stored.detailUrl === qDetailUrl) ||
        (stored.playGroups && stored.playGroups.some((g) => g.episodes.some((e) => e.play_url === qPlayUrl))))) ||
      (!qSourceId && !qPlayUrl && Boolean(stored.sourceId && stored.playUrl)))

  sourceId.value = qSourceId || (matchesStored ? stored.sourceId : '')
  videoId.value = qVideoId || (matchesStored ? (stored.videoId ?? '') : '')
  detailUrl.value = qDetailUrl || (matchesStored ? (stored.detailUrl ?? '') : '')
  title.value = qTitle || (matchesStored ? (stored.title ?? '') : '')
  sourceName.value = qSourceName || (matchesStored ? (stored.sourceName ?? '') : '')
  coverUrl.value = matchesStored ? (stored.coverUrl ?? null) : null
  proxyCoverUrl.value = matchesStored ? (stored.proxyCoverUrl ?? null) : null
  playGroups.value = matchesStored && Array.isArray(stored.playGroups) ? stored.playGroups : []

  const parsedIdx = qEpIdxRaw ? Number.parseInt(qEpIdxRaw, 10) : Number.NaN
  activeGroupName.value =
    qGroupName ||
    (matchesStored ? (stored.groupName ?? '') : '') ||
    playGroups.value[0]?.name ||
    ''

  const groupEps = activeEpisodes.value
  if (qPlayUrl) {
    currentPlayUrl.value = qPlayUrl
    const foundIdx = groupEps.findIndex((e) => e.play_url === qPlayUrl)
    currentEpisodeIndex.value =
      foundIdx >= 0 ? foundIdx : Number.isFinite(parsedIdx) && parsedIdx >= 0 ? parsedIdx : 0
  } else if (Number.isFinite(parsedIdx) && parsedIdx >= 0 && groupEps[parsedIdx]) {
    currentEpisodeIndex.value = parsedIdx
    currentPlayUrl.value = groupEps[parsedIdx]!.play_url
  } else if (matchesStored && stored.playUrl) {
    currentPlayUrl.value = stored.playUrl
    currentEpisodeIndex.value = stored.episodeIndex ?? 0
  } else {
    currentEpisodeIndex.value = 0
    currentPlayUrl.value = groupEps[0]?.play_url ?? ''
  }
}

async function fetchDetailFallbackIfNeeded(): Promise<boolean> {
  if (playGroups.value.length > 0 && currentPlayUrl.value) return false
  if (!sourceId.value || (!videoId.value && !detailUrl.value)) return false

  const seq = ++detailSeq
  try {
    const res = await onlineVideoApi.detail(sourceId.value, {
      video_id: videoId.value || null,
      detail_url: detailUrl.value || null,
    })
    if (unmounted || seq !== detailSeq) return false

    if (!title.value && res.title) title.value = res.title
    if (!sourceName.value && res.source_name) sourceName.value = res.source_name
    if (!coverUrl.value && res.cover_url) coverUrl.value = res.cover_url
    if (!proxyCoverUrl.value && res.proxy_cover_url) proxyCoverUrl.value = res.proxy_cover_url
    if (res.id && !videoId.value) videoId.value = res.id

    const groups = res.play_groups ?? []
    if (groups.length > 0) {
      playGroups.value = groups
      if (!groups.some((g) => g.name === activeGroupName.value)) {
        activeGroupName.value = groups[0]!.name
      }
      const grp = groups.find((g) => g.name === activeGroupName.value) ?? groups[0]!
      if (currentPlayUrl.value) {
        const idx = grp.episodes.findIndex((e) => e.play_url === currentPlayUrl.value)
        if (idx >= 0) currentEpisodeIndex.value = idx
      } else if (grp.episodes[currentEpisodeIndex.value]) {
        currentPlayUrl.value = grp.episodes[currentEpisodeIndex.value]!.play_url
      } else if (grp.episodes[0]) {
        currentEpisodeIndex.value = 0
        currentPlayUrl.value = grp.episodes[0].play_url
      }
    }
    persistPlayerContext()
    return true
  } catch {
    return false
  }
}

function revealHud(): void {
  hudVisible.value = true
  if (hudHideTimer) clearTimeout(hudHideTimer)
  hudHideTimer = setTimeout(() => {
    if (isPlaying.value) {
      hudVisible.value = false
    }
  }, 3200)
}

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
  destroyHls()
  const el = videoEl.value
  if (!el) return
  try {
    el.pause()
    el.removeAttribute('src')
    el.load()
  } catch {
    // ignore cleanup errors
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

function setMediaSessionPlaybackState(state: MediaSessionPlaybackState): void {
  if (typeof navigator === 'undefined' || !('mediaSession' in navigator) || !navigator.mediaSession) {
    return
  }
  try {
    navigator.mediaSession.playbackState = state
  } catch {
    // ignore
  }
}

function updateMediaSession(): void {
  if (typeof navigator === 'undefined' || !('mediaSession' in navigator) || !navigator.mediaSession) {
    return
  }
  if (typeof MediaMetadata !== 'undefined') {
    const posterPath = resolveOnlineVideoPosterUrl(
      proxyCoverUrl.value,
      coverUrl.value,
      sourceId.value,
    )
    const artwork: MediaImage[] = posterPath
      ? [{ src: new URL(posterPath, window.location.origin).href, sizes: '512x512' }]
      : []
    try {
      navigator.mediaSession.metadata = new MediaMetadata({
        title: currentEpisode.value?.name
          ? `${title.value || sourceName.value || 'Video'} · ${currentEpisode.value.name}`
          : title.value || 'Online Video',
        artist: sourceName.value || undefined,
        artwork,
      })
    } catch {
      // ignore invalid metadata errors
    }
  }

  setMediaSessionAction('play', () => {
    const el = videoEl.value
    if (el && el.paused) {
      musicPlayerStore.pause()
      void el.play().catch(() => {
        isPlaying.value = false
      })
    }
  })
  setMediaSessionAction('pause', () => {
    videoEl.value?.pause()
  })
  setMediaSessionAction('seekbackward', (details) => {
    const offset = details?.seekOffset && details.seekOffset > 0 ? details.seekOffset : 10
    seekTo(currentTimeSecs.value - offset)
  })
  setMediaSessionAction('seekforward', (details) => {
    const offset = details?.seekOffset && details.seekOffset > 0 ? details.seekOffset : 10
    seekTo(currentTimeSecs.value + offset)
  })
  setMediaSessionAction('seekto', (details) => {
    if (typeof details?.seekTime === 'number') {
      seekTo(details.seekTime)
    }
  })
  setMediaSessionAction('previoustrack', hasPrevEpisode.value ? () => goToPrevEpisode() : null)
  setMediaSessionAction('nexttrack', hasNextEpisode.value ? () => goToNextEpisode() : null)
}

function clearMediaSession(): void {
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
  setMediaSessionPlaybackState('none')
}

async function loadPlay(): Promise<void> {
  const seq = ++playSeq
  stopAndClearVideoElement()
  musicPlayerStore.pause()
  isPlaying.value = false
  loadingStream.value = true
  isBuffering.value = false
  loadError.value = null
  currentTimeSecs.value = 0
  durationSecs.value = 0
  selectedSubtitleIdx.value = 'off'
  hudVisible.value = true

  if (!sourceId.value) {
    loadingStream.value = false
    loadError.value = t('online.missingPlayerContext')
    return
  }

  if (!currentPlayUrl.value) {
    await fetchDetailFallbackIfNeeded()
    if (unmounted || seq !== playSeq) return
  }

  if (!currentPlayUrl.value) {
    loadingStream.value = false
    loadError.value = t('online.missingPlayerContext')
    return
  }

  persistPlayerContext()
  updateMediaSession()

  try {
    const res = await onlineVideoApi.play(
      sourceId.value,
      currentPlayUrl.value,
      activeGroupName.value || null,
    )
    if (unmounted || seq !== playSeq) return
    playResult.value = res

    const isHls = res.stream_type === 'hls' || /\.m3u8(\?|$)/i.test(res.stream_url)
    activeStreamType.value = isHls ? 'hls' : 'mp4'
    const proxyHint = isHls
      ? res.stream_type === 'hls' && res.proxy_stream_url?.includes('hls-proxy')
        ? res.proxy_stream_url
        : ''
      : res.proxy_stream_url
    const playableUrl = resolveOnlineVideoStreamUrl(
      proxyHint,
      res.stream_url,
      isHls ? 'hls' : 'mp4',
      sourceId.value,
    )

    if (!playableUrl) {
      loadError.value = t('online.streamLoadFailed')
      return
    }

    const el = videoEl.value
    if (!el) return

    el.volume = volume.value
    el.muted = muted.value
    el.playbackRate = playbackRate.value
    isBuffering.value = true

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
          xhrSetup: (xhr) => {
            xhr.withCredentials = true
          },
        })
        hlsInstance = hls
        hls.on(Hls.Events.MANIFEST_PARSED, () => {
          if (unmounted || seq !== playSeq) return
          el.playbackRate = playbackRate.value
          isBuffering.value = false
          void el.play().catch(() => {
            isPlaying.value = false
            isBuffering.value = false
          })
        })
        hls.on(Hls.Events.ERROR, (_event, data) => {
          if (unmounted || seq !== playSeq || !data.fatal) return
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
          destroyHls()
          isPlaying.value = false
          isBuffering.value = false
          loadError.value = data.reason || t('online.streamLoadFailed')
          hudVisible.value = true
        })
        hls.loadSource(playableUrl)
        hls.attachMedia(el)
      } else if (el.canPlayType('application/vnd.apple.mpegurl') !== '') {
        const handler = (): void => {
          clearPendingSeekHandler()
          el.playbackRate = playbackRate.value
        }
        pendingLoadedMetadataHandler = handler
        el.addEventListener('loadedmetadata', handler, { once: true })
        el.src = playableUrl
        el.load()
        void el.play().catch(() => {
          isPlaying.value = false
          isBuffering.value = false
        })
      } else {
        isBuffering.value = false
        loadError.value = t('player.unsupportedHls')
        hudVisible.value = true
      }
    } else {
      const handler = (): void => {
        clearPendingSeekHandler()
        el.playbackRate = playbackRate.value
      }
      pendingLoadedMetadataHandler = handler
      el.addEventListener('loadedmetadata', handler, { once: true })
      el.src = playableUrl
      el.load()
      void el.play().catch(() => {
        isPlaying.value = false
        isBuffering.value = false
      })
    }

    revealHud()
  } catch (err) {
    if (unmounted || seq !== playSeq) return
    isPlaying.value = false
    isBuffering.value = false
    loadError.value = err instanceof Error && err.message ? err.message : t('online.streamLoadFailed')
    hudVisible.value = true
  } finally {
    if (!unmounted && seq === playSeq) {
      loadingStream.value = false
    }
  }
}

function selectEpisode(epIndex: number, groupNameOverride?: string): void {
  const targetGroupName = groupNameOverride || activeGroupName.value
  const targetGroup = playGroups.value.find((g) => g.name === targetGroupName) ?? activeGroup.value
  if (!targetGroup || !targetGroup.episodes[epIndex]) return

  const ep = targetGroup.episodes[epIndex]!
  activeGroupName.value = targetGroup.name
  currentEpisodeIndex.value = epIndex
  currentPlayUrl.value = ep.play_url
  persistPlayerContext()

  const nextQuery: Record<string, string> = {
    sourceId: sourceId.value,
    playUrl: ep.play_url,
    episodeIndex: String(epIndex),
    groupName: targetGroup.name,
  }
  if (videoId.value) nextQuery.videoId = videoId.value
  if (detailUrl.value) nextQuery.detailUrl = detailUrl.value
  if (title.value) nextQuery.title = title.value
  if (sourceName.value) nextQuery.sourceName = sourceName.value

  void router.replace({ path: '/video/online/watch', query: nextQuery })
  void loadPlay()
}

function onGroupChange(nextGroupName: string): void {
  const grp = playGroups.value.find((g) => g.name === nextGroupName)
  if (!grp || grp.episodes.length === 0) return
  const targetIdx = grp.episodes[currentEpisodeIndex.value] ? currentEpisodeIndex.value : 0
  selectEpisode(targetIdx, grp.name)
}

function goToPrevEpisode(): void {
  if (!hasPrevEpisode.value) return
  selectEpisode(currentEpisodeIndex.value - 1)
}

function goToNextEpisode(): void {
  if (!hasNextEpisode.value) return
  selectEpisode(currentEpisodeIndex.value + 1)
}

function togglePlay(): void {
  const el = videoEl.value
  if (!el || loadError.value || !currentPlayUrl.value) return
  if (el.paused) {
    musicPlayerStore.pause()
    void el.play().catch(() => {
      isPlaying.value = false
    })
  } else {
    el.pause()
  }
  revealHud()
}

function seekTo(targetSecs: number): void {
  const el = videoEl.value
  if (!el) return
  const maxDur = Number.isFinite(durationSecs.value) && durationSecs.value > 0 ? durationSecs.value : targetSecs
  const clamped = Math.max(0, Math.min(maxDur, targetSecs))
  el.currentTime = clamped
  currentTimeSecs.value = clamped
}

function onSpeedChange(val: string): void {
  const rate = Number.parseFloat(val) || 1.0
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
  persistAudioPrefs(volume.value, muted.value)
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
  persistAudioPrefs(volume.value, muted.value)
}

async function toggleFullscreen(): Promise<void> {
  if (!stageRef.value) return
  try {
    if (document.fullscreenElement) {
      await document.exitFullscreen()
    } else {
      await stageRef.value.requestFullscreen()
    }
  } catch {
    // ignore fullscreen permission errors
  }
}

async function togglePip(): Promise<void> {
  const el = videoEl.value
  if (
    !el ||
    !document.pictureInPictureEnabled ||
    typeof el.requestPictureInPicture !== 'function'
  ) {
    return
  }
  try {
    if (document.pictureInPictureElement) {
      await document.exitPictureInPicture()
    } else if (el.readyState >= 1) {
      await el.requestPictureInPicture()
    }
  } catch {
    // ignore PiP errors
  }
}

function onTimeUpdate(): void {
  const el = videoEl.value
  if (!el) return
  currentTimeSecs.value = el.currentTime || 0
}

function onDurationChange(): void {
  const el = videoEl.value
  if (!el) return
  durationSecs.value = Number.isFinite(el.duration) ? el.duration : 0
}

function onVideoPlay(): void {
  isPlaying.value = true
  isBuffering.value = false
  musicPlayerStore.pause()
  setMediaSessionPlaybackState('playing')
  revealHud()
}

function onVideoPause(): void {
  isPlaying.value = false
  hudVisible.value = true
  setMediaSessionPlaybackState('paused')
}

function onVideoEnded(): void {
  isPlaying.value = false
  isBuffering.value = false
  setMediaSessionPlaybackState('paused')
  hudVisible.value = true
  if (themeStore.video.autoplay_next && hasNextEpisode.value) {
    goToNextEpisode()
  }
}

function onVideoError(): void {
  const el = videoEl.value
  if (!el || (!el.getAttribute('src') && !hlsInstance) || !el.error) return
  isPlaying.value = false
  isBuffering.value = false
  loadError.value = t('online.streamLoadFailed')
  hudVisible.value = true
}

function goBack(): void {
  if (document.fullscreenElement) {
    void document.exitFullscreen().catch(() => {})
  }
  stopAndClearVideoElement()
  clearMediaSession()

  if (sourceId.value && (videoId.value || detailUrl.value)) {
    const query: Record<string, string> = { sourceId: sourceId.value }
    if (videoId.value) query.videoId = videoId.value
    if (detailUrl.value) query.detailUrl = detailUrl.value
    if (title.value) query.title = title.value
    if (sourceName.value) query.sourceName = sourceName.value
    void router.push({ path: '/video/online/detail', query })
    return
  }
  void router.push({ path: '/video/catalog', query: { scope: 'online' } })
}

const escapeStack = useEscapeStack()
escapeStack.register(
  () => !document.fullscreenElement,
  () => {
    goBack()
  },
)

function isEditableTarget(target: EventTarget | null): boolean {
  if (!(target instanceof HTMLElement)) return false
  if (target.isContentEditable) return true
  const tag = target.tagName
  return tag === 'INPUT' || tag === 'TEXTAREA' || tag === 'SELECT'
}

function isDropdownOrDialogOpen(target: EventTarget | null): boolean {
  if (typeof document !== 'undefined') {
    if (document.querySelector('[role="listbox"], [aria-modal="true"]')) return true
  }
  if (target instanceof HTMLElement) {
    const role = target.getAttribute('role')
    if (role === 'combobox' || role === 'listbox' || role === 'option') return true
  }
  return false
}

function onKeydown(ev: KeyboardEvent): void {
  if (isEditableTarget(ev.target) || isDropdownOrDialogOpen(ev.target)) return
  if (ev.ctrlKey || ev.metaKey || ev.altKey) return

  if (ev.code === 'Space') {
    ev.preventDefault()
    togglePlay()
  } else if (ev.code === 'ArrowRight') {
    ev.preventDefault()
    seekTo(currentTimeSecs.value + 10)
    revealHud()
  } else if (ev.code === 'ArrowLeft') {
    ev.preventDefault()
    seekTo(currentTimeSecs.value - 10)
    revealHud()
  } else if (ev.code === 'ArrowUp') {
    ev.preventDefault()
    adjustVolume(0.05)
    revealHud()
  } else if (ev.code === 'ArrowDown') {
    ev.preventDefault()
    adjustVolume(-0.05)
    revealHud()
  } else if (ev.key === 'm' || ev.key === 'M') {
    ev.preventDefault()
    toggleMute()
    revealHud()
  } else if (ev.key === 'f' || ev.key === 'F') {
    ev.preventDefault()
    void toggleFullscreen()
  } else if (ev.key === 'p' || ev.key === 'P') {
    ev.preventDefault()
    void togglePip()
  } else if ((ev.key === 'n' || ev.key === 'N') && hasNextEpisode.value) {
    ev.preventDefault()
    goToNextEpisode()
  }
}

onMounted(() => {
  unmounted = false
  musicPlayerStore.pause()
  if (videoEl.value) {
    videoEl.value.volume = volume.value
    videoEl.value.muted = muted.value
  }
  hydrateContextFromRouteAndStorage()
  void loadPlay()
  if (playGroups.value.length === 0 && currentPlayUrl.value) {
    void fetchDetailFallbackIfNeeded()
  }
  window.addEventListener('keydown', onKeydown)
})

onBeforeUnmount(() => {
  unmounted = true
  playSeq += 1
  detailSeq += 1
  stopAndClearVideoElement()
  clearMediaSession()
  if (hudHideTimer) clearTimeout(hudHideTimer)
  window.removeEventListener('keydown', onKeydown)
})

watch(
  () => [
    route.query.sourceId,
    route.query.source_id,
    route.query.playUrl,
    route.query.play_url,
    route.query.groupName,
    route.query.episodeIndex,
  ],
  () => {
    const qSourceId = readQueryParam(['sourceId', 'source_id'])
    const qPlayUrl = readQueryParam(['playUrl', 'play_url'])
    const qGroupName = readQueryParam(['groupName', 'group_name'])
    const qEpIdx = readQueryParam(['episodeIndex', 'episode_index'])
    if (
      qSourceId === sourceId.value &&
      qPlayUrl === currentPlayUrl.value &&
      qGroupName === activeGroupName.value &&
      qEpIdx === String(currentEpisodeIndex.value)
    ) {
      return
    }
    hydrateContextFromRouteAndStorage()
    void loadPlay()
    if (playGroups.value.length === 0 && currentPlayUrl.value) {
      void fetchDetailFallbackIfNeeded()
    }
  },
)
</script>

<template>
  <div
    ref="stageRef"
    class="relative flex h-dvh w-screen flex-col justify-between overflow-hidden bg-black text-white select-none"
    @mousemove="revealHud"
    @touchstart.passive="revealHud"
    @click.self="togglePlay"
  >
    <!-- HTML5 Video Stage -->
    <video
      ref="videoEl"
      class="h-full w-full bg-black object-contain"
      crossorigin="use-credentials"
      playsinline
      @click="togglePlay"
      @dblclick="toggleFullscreen"
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
        v-if="activeSubtitle"
        :key="activeSubtitle.url"
        kind="subtitles"
        :src="activeSubtitle.url"
        :srclang="activeSubtitle.lang || 'und'"
        :label="activeSubtitle.label"
        default
      />
    </video>

    <!-- Loading / Buffering Overlay -->
    <div
      v-if="(loadingStream || isBuffering) && !loadError"
      class="pointer-events-none absolute inset-0 z-10 flex items-center justify-center bg-black/35"
    >
      <div
        class="flex items-center gap-3 rounded-full bg-[var(--md-sys-color-surface-container-high)]/90 px-5 py-3 text-xs font-semibold text-[var(--md-sys-color-on-surface)] shadow-[var(--md-sys-elevation-2)] backdrop-blur-md"
      >
        <LuLoader2 class="h-4 w-4 animate-spin text-[var(--md-sys-color-primary)]" />
        <span>{{ loadingStream ? t('online.resolvingStream') : t('player.loading') }}</span>
      </div>
    </div>

    <!-- Error & Retry Overlay -->
    <div
      v-if="loadError"
      class="absolute inset-0 z-30 flex items-center justify-center bg-black/75 p-4 backdrop-blur-xs"
    >
      <div
        class="flex w-full max-w-md flex-col items-center rounded-[28px] bg-[var(--md-sys-color-surface-container-high)] p-6 text-center text-[var(--md-sys-color-on-surface)] shadow-[var(--md-sys-elevation-3)]"
      >
        <div
          class="mb-4 flex h-12 w-12 items-center justify-center rounded-2xl bg-[var(--md-sys-color-error-container)] text-[var(--md-sys-color-on-error-container)]"
        >
          <LuAlertCircle class="h-6 w-6" />
        </div>
        <h2 class="text-base font-bold">{{ t('online.streamLoadFailed') }}</h2>
        <p class="mt-1.5 text-xs leading-relaxed text-[var(--md-sys-color-on-surface-variant)]">
          {{ loadError }}
        </p>
        <div class="mt-5 flex items-center gap-2.5">
          <button
            type="button"
            class="md-btn md-btn-sm md-btn-outlined md-state"
            @click="goBack"
          >
            <LuArrowLeft class="h-3.5 w-3.5" />
            <span>{{ t('player.backToDetail') }}</span>
          </button>
          <button
            v-if="sourceId && currentPlayUrl"
            type="button"
            class="md-btn md-btn-sm md-btn-filled md-state"
            @click="loadPlay"
          >
            <LuRefreshCw class="h-3.5 w-3.5" />
            <span>{{ t('player.retry') }}</span>
          </button>
        </div>
      </div>
    </div>

    <!-- Top HUD Overlay -->
    <header
      class="absolute inset-x-0 top-0 z-20 flex flex-wrap items-center justify-between gap-2 bg-gradient-to-b from-black/80 via-black/40 to-transparent px-4 py-3 transition-opacity duration-200 sm:gap-4 sm:px-8 sm:py-4"
      :class="hudVisible || loadingStream || loadError ? 'opacity-100' : 'pointer-events-none opacity-0'"
    >
      <div class="flex min-w-0 items-center gap-3">
        <button
          type="button"
          :aria-label="t('player.backToDetail')"
          class="inline-flex items-center gap-1.5 rounded-full bg-white/15 px-3.5 py-2 text-xs font-semibold text-white backdrop-blur-md transition hover:bg-white/25"
          @click="goBack"
        >
          <LuArrowLeft class="h-4 w-4" />
          <span>{{ t('player.backToDetail') }}</span>
        </button>

        <div class="min-w-0">
          <h1 class="truncate text-sm font-bold text-white sm:text-base">
            {{ title || sourceName || 'Online Video' }}
          </h1>
          <p class="truncate text-[11px] text-white/70">
            <span v-if="currentEpisode?.name">{{ currentEpisode.name }}</span>
            <span v-if="activeGroupName">
              <span v-if="currentEpisode?.name"> · </span>
              {{ t('online.switchLine', { name: activeGroupName }) }}
            </span>
            <span v-if="playResult">
              · {{ activeStreamType === 'hls' ? t('online.streamTypeHls') : t('online.streamTypeMp4') }}
            </span>
          </p>
        </div>
      </div>

      <div class="flex flex-wrap items-center gap-1.5 sm:gap-2">
        <div v-if="playGroups.length > 1" class="w-32 sm:w-40">
          <AppSelect
            :model-value="activeGroupName"
            :options="groupOptions"
            variant="compact"
            :teleport-to="stageRef"
            @change="onGroupChange"
          />
        </div>

        <div v-if="safeSubtitles.length > 0" class="w-28 sm:w-36">
          <AppSelect
            v-model="selectedSubtitleIdx"
            :options="subtitleOptions"
            variant="compact"
            :teleport-to="stageRef"
          />
        </div>

        <div class="w-20 sm:w-24">
          <AppSelect
            :model-value="String(playbackRate)"
            :options="speedOptions"
            variant="compact"
            :teleport-to="stageRef"
            @change="onSpeedChange"
          />
        </div>
      </div>
    </header>

    <!-- Bottom HUD Overlay -->
    <footer
      v-if="!loadError"
      class="absolute inset-x-0 bottom-0 z-20 space-y-2.5 bg-gradient-to-t from-black/85 via-black/50 to-transparent px-5 pt-8 pb-5 transition-opacity duration-200 sm:px-8"
      :class="hudVisible ? 'opacity-100' : 'pointer-events-none opacity-0'"
    >
      <!-- Compact Episode Strip (in-player quick switch) -->
      <div
        v-if="activeEpisodes.length > 1"
        class="no-scrollbar flex items-center gap-1.5 overflow-x-auto pb-1"
      >
        <button
          v-for="(ep, idx) in activeEpisodes"
          :key="`${activeGroupName}-${ep.index}-${ep.play_url}`"
          type="button"
          class="shrink-0 rounded-full px-3 py-1 text-[11px] font-semibold transition"
          :class="
            idx === currentEpisodeIndex
              ? 'bg-[var(--md-sys-color-primary)] text-[var(--md-sys-color-on-primary)]'
              : 'bg-white/15 text-white hover:bg-white/25'
          "
          @click="selectEpisode(idx)"
        >
          {{ ep.name }}
        </button>
      </div>

      <!-- Seek Slider -->
      <div class="flex items-center gap-3">
        <span class="w-14 text-right font-mono text-xs text-white/85">
          {{ formatTime(currentTimeSecs) }}
        </span>
        <div class="flex-1">
          <AppSlider
            :model-value="currentTimeSecs"
            :min="0"
            :max="Math.max(1, durationSecs)"
            :step="0.5"
            commit-on-release
            aria-label="Video timeline"
            @change="seekTo($event)"
          />
        </div>
        <span class="w-14 font-mono text-xs text-white/85">
          {{ formatTime(durationSecs) }}
        </span>
      </div>

      <!-- Controls Row -->
      <div class="flex items-center justify-between gap-4">
        <div class="flex items-center gap-2 sm:gap-3">
          <button
            v-if="hasPrevEpisode"
            type="button"
            class="inline-flex items-center gap-1 rounded-full bg-white/15 px-3 py-1.5 text-xs font-semibold text-white backdrop-blur-md hover:bg-white/25"
            @click="goToPrevEpisode"
          >
            <LuSkipBack class="h-3.5 w-3.5 fill-current" />
            <span class="hidden sm:inline">{{ t('player.prevEpisode') }}</span>
          </button>

          <button
            type="button"
            :aria-label="isPlaying ? t('player.pause') : t('player.play')"
            :title="isPlaying ? t('player.pause') : t('player.play')"
            class="flex h-10 w-10 items-center justify-center rounded-full bg-[var(--md-sys-color-primary)] text-[var(--md-sys-color-on-primary)] shadow-[var(--md-sys-elevation-2)] transition hover:scale-105"
            @click="togglePlay"
          >
            <LuPause v-if="isPlaying" class="h-5 w-5 fill-current" />
            <LuPlay v-else class="ml-0.5 h-5 w-5 fill-current" />
          </button>

          <button
            v-if="hasNextEpisode"
            type="button"
            class="inline-flex items-center gap-1 rounded-full bg-white/15 px-3 py-1.5 text-xs font-semibold text-white backdrop-blur-md hover:bg-white/25"
            @click="goToNextEpisode"
          >
            <span class="hidden sm:inline">{{ t('player.nextEpisode') }}</span>
            <LuSkipForward class="h-3.5 w-3.5 fill-current" />
          </button>

          <div class="flex items-center gap-2 sm:w-32">
            <button
              type="button"
              :aria-label="muted || volume === 0 ? t('player.unmute') : t('player.mute')"
              :title="muted || volume === 0 ? t('player.unmute') : t('player.mute')"
              class="rounded-lg p-1.5 text-white/85 hover:bg-white/15"
              @click="toggleMute"
            >
              <LuVolumeX v-if="muted || volume === 0" class="h-4 w-4" />
              <LuVolume2 v-else class="h-4 w-4" />
            </button>
            <div class="hidden w-24 sm:block">
              <AppSlider
                :model-value="muted ? 0 : volume"
                :min="0"
                :max="1"
                :step="0.02"
                aria-label="Volume"
                @update:model-value="setVolume($event)"
              />
            </div>
          </div>
        </div>

        <div class="flex items-center gap-2">
          <button
            v-if="pipSupported"
            type="button"
            class="rounded-xl bg-white/15 p-2 text-white backdrop-blur-md transition hover:bg-white/25"
            :title="t('player.pip')"
            :aria-label="t('player.pip')"
            @click="togglePip"
          >
            <LuPictureInPicture2 class="h-4 w-4" />
          </button>

          <button
            type="button"
            class="rounded-xl bg-white/15 p-2 text-white backdrop-blur-md transition hover:bg-white/25"
            :title="t('player.fullscreen')"
            :aria-label="t('player.fullscreen')"
            @click="toggleFullscreen"
          >
            <LuMaximize class="h-4 w-4" />
          </button>
        </div>
      </div>
    </footer>
  </div>
</template>

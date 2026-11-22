<script setup lang="ts">
import { computed, nextTick, onBeforeUnmount, onMounted, ref, watch } from 'vue'
import { useRoute, useRouter } from 'vue-router'
import Hls from 'hls.js'
import type { SelectOption } from '@/components/ui'
import { usePlayerStore } from '@/domains/music/player'
import {
  libraryRoute,
  useOnlineItemState,
  type OnlineItemMetadataInput,
  type TrackedItemRef,
} from '@/sources'
import { useThemeStore } from '@/stores/theme'
import VideoPlayerShell, { type VideoPlayerEpisodeItem } from '../components/VideoPlayerShell.vue'
import {
  buildOnlineVideoDetailRoute,
  buildOnlineVideoPlayRoute,
  buildOnlineVideoTrackedRef,
  hydrateOnlineVideoPlayerContext,
  readQueryString,
  writeStoredPlayerContext,
} from '../composables/useOnlineVideoContext'
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
  resolveOnlineVideoPosterUrl,
  resolveOnlineVideoStreamUrl,
  resolveOnlineVideoSubtitleUrl,
  type OnlineVideoEpisode,
  type OnlineVideoPlayGroup,
  type OnlineVideoPlayResponseDto,
  type OnlineVideoSubtitle,
} from '../onlineApi'

interface ResolvedOnlineVideoSubtitle extends OnlineVideoSubtitle {
  resolvedUrl: string
}

const { t } = useI18n()
const route = useRoute()
const router = useRouter()
const themeStore = useThemeStore()
const musicPlayerStore = usePlayerStore()

const shellRef = ref<InstanceType<typeof VideoPlayerShell> | null>(null)
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
const muted = ref(loadStoredVideoMuted())
const volume = ref(loadStoredVideoVolume())
const currentTimeSecs = ref(0)
const durationSecs = ref(0)

let hlsInstance: Hls | null = null
let pendingLoadedMetadataHandler: (() => void) | null = null
let playSeq = 0
let detailSeq = 0
let syncUrlSeq = 0
let syncingUrl = false
let unmounted = false
let lastSavedProgressAt = 0

// Unified online progress & favorite state
const trackedRef = computed<TrackedItemRef | null>(() =>
  buildOnlineVideoTrackedRef({
    sourceId: sourceId.value,
    detailUrl: detailUrl.value,
    videoId: videoId.value,
    ref: currentPlayUrl.value || undefined,
    title: title.value || sourceName.value || 'Online Video',
    cover: coverUrl.value,
    sourceName: sourceName.value,
  }),
)

const itemMeta = computed<OnlineItemMetadataInput>(() => {
  const extra: Record<string, unknown> = {}
  if (videoId.value) extra.vid = videoId.value
  if (detailUrl.value) extra.detail_url = detailUrl.value
  if (sourceName.value) extra.source_name = sourceName.value
  return {
    title: title.value || sourceName.value || 'Online Video',
    subtitle: sourceName.value || undefined,
    coverUrl: coverUrl.value,
    extra,
  }
})

const itemState = useOnlineItemState(trackedRef, itemMeta, { autoLoad: false })

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

const groupOptions = computed<SelectOption[]>(() =>
  playGroups.value.map((g) => ({
    value: g.name,
    label: g.name,
    badge: g.episodes.length,
  })),
)

const safeSubtitles = computed<ResolvedOnlineVideoSubtitle[]>(() =>
  (playResult.value?.subtitles ?? [])
    .map((s) => ({
      ...s,
      resolvedUrl: resolveOnlineVideoSubtitleUrl(s.proxy_url, s.url, sourceId.value),
    }))
    .filter((s) => Boolean(s.resolvedUrl)),
)

const subtitleOptions = computed<SelectOption[]>(() => [
  { value: 'off', label: t('player.subtitleOff') },
  ...safeSubtitles.value.map((sub, idx) => ({
    value: String(idx),
    label: sub.label || sub.lang || `Sub #${idx + 1}`,
  })),
])

const activeSubtitle = computed<ResolvedOnlineVideoSubtitle | null>(() => {
  if (selectedSubtitleIdx.value === 'off') return null
  const idx = Number.parseInt(selectedSubtitleIdx.value, 10)
  if (!Number.isFinite(idx) || idx < 0) return null
  return safeSubtitles.value[idx] ?? null
})

// Single top-bar combobox: Play Line (when multiple lines), or Subtitle (when 1 line + subtitles available)
const headerSelectMode = computed<'line' | 'subtitle' | 'none'>(() => {
  if (playGroups.value.length > 1) return 'line'
  if (safeSubtitles.value.length > 0) return 'subtitle'
  return 'none'
})

const headerSelectOptions = computed<SelectOption[]>(() => {
  if (headerSelectMode.value === 'line') return groupOptions.value
  if (headerSelectMode.value === 'subtitle') return subtitleOptions.value
  return []
})

const headerSelectValue = computed(() => {
  if (headerSelectMode.value === 'line') return activeGroupName.value
  if (headerSelectMode.value === 'subtitle') return selectedSubtitleIdx.value
  return ''
})

const headerSelectAriaLabel = computed(() =>
  headerSelectMode.value === 'line' ? t('player.lineLabel') : t('player.subtitles'),
)

const headerTitle = computed(() => title.value || sourceName.value || 'Online Video')

const headerSubtitle = computed(() => {
  const parts: string[] = []
  if (currentEpisode.value?.name) parts.push(currentEpisode.value.name)
  if (activeGroupName.value) parts.push(t('online.switchLine', { name: activeGroupName.value }))
  if (playResult.value) {
    parts.push(
      activeStreamType.value === 'hls' ? t('online.streamTypeHls') : t('online.streamTypeMp4'),
    )
  }
  return parts.join(' · ')
})

const episodeDrawerItems = computed<VideoPlayerEpisodeItem[]>(() =>
  activeEpisodes.value.map((ep, idx) => ({
    id: String(idx),
    label: ep.name,
    active: idx === currentEpisodeIndex.value,
    disabled: !ep.play_url,
  })),
)

function persistPlayerContext(): void {
  if (!sourceId.value || !currentPlayUrl.value) return
  writeStoredPlayerContext({
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
  })
}

function applyHydratedContext(): void {
  const ctx = hydrateOnlineVideoPlayerContext(route)
  sourceId.value = ctx.sourceId
  sourceName.value = ctx.sourceName
  videoId.value = ctx.videoId
  detailUrl.value = ctx.detailUrl
  title.value = ctx.title
  coverUrl.value = ctx.coverUrl
  proxyCoverUrl.value = ctx.proxyCoverUrl
  playGroups.value = ctx.playGroups
  activeGroupName.value = ctx.activeGroupName
  currentEpisodeIndex.value = ctx.currentEpisodeIndex
  currentPlayUrl.value = ctx.currentPlayUrl
}

function reportOnlineProgress(opts?: { immediate?: boolean; completed?: boolean }): void {
  if (!trackedRef.value || !currentPlayUrl.value) return
  const pos = Math.max(0, currentTimeSecs.value)
  const dur = Number.isFinite(durationSecs.value) && durationSecs.value > 0 ? durationSecs.value : undefined
  if (!opts?.immediate && !opts?.completed && pos < 1) return

  const completed =
    opts?.completed ?? (dur !== undefined && dur > 30 && pos / dur >= 0.95 ? true : false)

  itemState.saveProgress(
    {
      unitIndex: currentEpisodeIndex.value,
      unitKey: currentPlayUrl.value,
      unitTitle: currentEpisode.value?.name,
      unitGroup: activeGroupName.value || undefined,
      position: pos,
      duration: dur,
      totalUnits: activeEpisodes.value.length > 0 ? activeEpisodes.value.length : undefined,
      completed,
    },
    { immediate: opts?.immediate },
  )
}

function resolveSavedResumeSeconds(): number {
  const saved = itemState.progress.value
  if (!saved || saved.completed || saved.position <= 3) return 0
  const matchesEpisode =
    (saved.unitKey && saved.unitKey === currentPlayUrl.value) ||
    (saved.unitIndex !== undefined &&
      saved.unitIndex === currentEpisodeIndex.value &&
      (!saved.unitGroup || !activeGroupName.value || saved.unitGroup === activeGroupName.value))
  return matchesEpisode ? saved.position : 0
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

function syncMediaSession(): void {
  updateVideoMediaSession({
    title: currentEpisode.value?.name
      ? `${title.value || sourceName.value || 'Video'} · ${currentEpisode.value.name}`
      : title.value || 'Online Video',
    artist: sourceName.value || undefined,
    posterUrl:
      resolveOnlineVideoPosterUrl(proxyCoverUrl.value, coverUrl.value, sourceId.value) || null,
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
    onPrev: hasPrevEpisode.value ? () => goToPrevEpisode() : null,
    onNext: hasNextEpisode.value ? () => goToNextEpisode() : null,
  })
}

function attachSeekOnMetadata(el: HTMLVideoElement, seekSecs: number): void {
  clearPendingSeekHandler()
  const handler = (): void => {
    clearPendingSeekHandler()
    el.playbackRate = playbackRate.value
    if (seekSecs > 0) {
      const maxDur = Number.isFinite(el.duration) && el.duration > 0 ? el.duration : seekSecs
      const target = Math.min(seekSecs, Math.max(0, maxDur - 1))
      if (target > 0) {
        el.currentTime = target
        currentTimeSecs.value = target
      }
    }
  }
  pendingLoadedMetadataHandler = handler
  el.addEventListener('loadedmetadata', handler, { once: true })
}

async function loadPlay(options?: { preserveTimeSecs?: number; checkSavedResume?: boolean }): Promise<void> {
  const seq = ++playSeq
  const requestedPreserve = options?.preserveTimeSecs ?? 0

  // Immediately clear stale stream badge, subtitle track, and previous stream (E2E T2-F16-10)
  stopAndClearVideoElement()
  playResult.value = null
  selectedSubtitleIdx.value = 'off'
  musicPlayerStore.pause()
  isPlaying.value = false
  loadingStream.value = true
  isBuffering.value = false
  loadError.value = null
  currentTimeSecs.value = requestedPreserve > 0 ? requestedPreserve : 0
  durationSecs.value = 0

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

  let targetStartSecs = requestedPreserve
  if (targetStartSecs <= 0 && options?.checkSavedResume && trackedRef.value) {
    try {
      await itemState.refresh(false)
    } catch {
      // ignore non-fatal state lookup error
    }
    if (unmounted || seq !== playSeq) return
    targetStartSecs = resolveSavedResumeSeconds()
    if (targetStartSecs > 0) {
      currentTimeSecs.value = targetStartSecs
    }
  }

  persistPlayerContext()
  syncMediaSession()

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
          startPosition: targetStartSecs > 0 ? targetStartSecs : -1,
          xhrSetup: (xhr) => {
            xhr.withCredentials = true
          },
        })
        hlsInstance = hls
        hls.on(Hls.Events.MANIFEST_PARSED, () => {
          if (unmounted || seq !== playSeq) return
          el.playbackRate = playbackRate.value
          if (targetStartSecs > 0 && Math.abs((el.currentTime || 0) - targetStartSecs) > 1.5) {
            el.currentTime = targetStartSecs
          }
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
          stopAndClearVideoElement()
          isPlaying.value = false
          isBuffering.value = false
          loadError.value = data.reason || t('online.streamLoadFailed')
        })
        hls.loadSource(playableUrl)
        hls.attachMedia(el)
      } else if (el.canPlayType('application/vnd.apple.mpegurl') !== '') {
        attachSeekOnMetadata(el, targetStartSecs)
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
      attachSeekOnMetadata(el, targetStartSecs)
      el.src = playableUrl
      el.load()
      void el.play().catch(() => {
        isPlaying.value = false
        isBuffering.value = false
      })
    }

    shellRef.value?.showHud()
  } catch (err) {
    if (unmounted || seq !== playSeq) return
    isPlaying.value = false
    isBuffering.value = false
    loadError.value = err instanceof Error && err.message ? err.message : t('online.streamLoadFailed')
  } finally {
    if (!unmounted && seq === playSeq) {
      loadingStream.value = false
    }
  }
}

function selectEpisode(
  epIndex: number,
  groupNameOverride?: string,
  opts?: { preserveTimeSecs?: number },
): void {
  const targetGroupName = groupNameOverride || activeGroupName.value
  const targetGroup = playGroups.value.find((g) => g.name === targetGroupName) ?? activeGroup.value
  if (!targetGroup || !targetGroup.episodes[epIndex]) return

  // Save outgoing episode progress when moving to a different episode
  if (!opts?.preserveTimeSecs && currentTimeSecs.value > 1) {
    reportOnlineProgress({ immediate: true })
  }

  const ep = targetGroup.episodes[epIndex]!
  activeGroupName.value = targetGroup.name
  currentEpisodeIndex.value = epIndex
  currentPlayUrl.value = ep.play_url
  persistPlayerContext()

  const targetRoute = buildOnlineVideoPlayRoute({
    sourceId: sourceId.value,
    detailUrl: detailUrl.value,
    videoId: videoId.value,
    playUrl: ep.play_url,
    groupName: targetGroup.name,
    episodeIndex: epIndex,
    title: title.value,
    cover: coverUrl.value,
    sourceName: sourceName.value,
  })

  const seq = ++syncUrlSeq
  syncingUrl = true
  void router.replace(targetRoute).finally(() =>
    nextTick(() => {
      if (seq === syncUrlSeq) {
        syncingUrl = false
      }
    }),
  )
  void loadPlay({ preserveTimeSecs: opts?.preserveTimeSecs ?? 0 })
}

function onGroupChange(nextGroupName: string): void {
  const grp = playGroups.value.find((g) => g.name === nextGroupName)
  if (!grp || grp.episodes.length === 0) return
  // Preserve current playback position when switching play lines (V-05)
  const preserveSecs = currentTimeSecs.value
  const targetIdx = grp.episodes[currentEpisodeIndex.value] ? currentEpisodeIndex.value : 0
  selectEpisode(targetIdx, grp.name, { preserveTimeSecs: preserveSecs })
}

function handleHeaderSelectChange(val: string): void {
  if (headerSelectMode.value === 'line') {
    onGroupChange(val)
  } else if (headerSelectMode.value === 'subtitle') {
    selectedSubtitleIdx.value = val
  }
}

function handleSelectEpisodeById(id: string): void {
  const idx = Number.parseInt(id, 10)
  if (Number.isFinite(idx) && idx >= 0) {
    selectEpisode(idx)
  }
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
}

function seekTo(targetSecs: number): void {
  const el = videoEl.value
  if (!el) return
  const maxDur =
    Number.isFinite(durationSecs.value) && durationSecs.value > 0 ? durationSecs.value : targetSecs
  const clamped = Math.max(0, Math.min(maxDur, targetSecs))
  el.currentTime = clamped
  currentTimeSecs.value = clamped
  reportOnlineProgress()
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

function onTimeUpdate(): void {
  const el = videoEl.value
  if (!el) return
  currentTimeSecs.value = el.currentTime || 0
  const now = Date.now()
  if (isPlaying.value && currentTimeSecs.value > 1 && now - lastSavedProgressAt >= 4000) {
    lastSavedProgressAt = now
    reportOnlineProgress()
  }
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
  setVideoMediaSessionState('playing')
}

function onVideoPause(): void {
  isPlaying.value = false
  setVideoMediaSessionState('paused')
  if (currentTimeSecs.value > 1 && !videoEl.value?.ended) {
    reportOnlineProgress({ immediate: true })
  }
}

function onVideoEnded(): void {
  isPlaying.value = false
  isBuffering.value = false
  setVideoMediaSessionState('paused')
  reportOnlineProgress({ immediate: true, completed: true })
  if (themeStore.video.autoplay_next && hasNextEpisode.value) {
    goToNextEpisode()
  }
}

function onVideoError(): void {
  const el = videoEl.value
  if (!el || (!el.getAttribute('src') && !hlsInstance) || !el.error) return
  stopAndClearVideoElement()
  isPlaying.value = false
  isBuffering.value = false
  loadError.value = t('online.streamLoadFailed')
}

function goBack(): void {
  if (currentTimeSecs.value > 1) {
    reportOnlineProgress({ immediate: true })
  }
  stopAndClearVideoElement()
  clearVideoMediaSession()

  if (window.history.state?.back) {
    router.back()
    return
  }

  if (sourceId.value && (videoId.value || detailUrl.value)) {
    void router.push(
      buildOnlineVideoDetailRoute({
        sourceId: sourceId.value,
        detailUrl: detailUrl.value,
        videoId: videoId.value,
        title: title.value,
        cover: coverUrl.value,
        sourceName: sourceName.value,
      }),
    )
    return
  }

  void router.push(libraryRoute('video', { scope: 'online' }))
}

onMounted(() => {
  unmounted = false
  musicPlayerStore.pause()
  if (videoEl.value) {
    videoEl.value.volume = volume.value
    videoEl.value.muted = muted.value
  }
  applyHydratedContext()
  void loadPlay({ checkSavedResume: true })
  if (playGroups.value.length === 0 && currentPlayUrl.value) {
    void fetchDetailFallbackIfNeeded()
  }
})

onBeforeUnmount(() => {
  unmounted = true
  playSeq += 1
  detailSeq += 1
  if (currentTimeSecs.value > 1) {
    reportOnlineProgress({ immediate: true })
  }
  stopAndClearVideoElement()
  clearVideoMediaSession()
})

watch(
  () => [
    route.query.source,
    route.query.sourceId,
    route.query.source_id,
    route.query.play,
    route.query.playUrl,
    route.query.play_url,
    route.query.group,
    route.query.groupName,
    route.query.group_name,
    route.query.episodeIndex,
    route.query.episode_index,
  ],
  () => {
    if (syncingUrl) return
    const qSourceId = readQueryString(route.query, ['source', 'sourceId', 'source_id'])
    const qPlayUrl = readQueryString(route.query, ['play', 'playUrl', 'play_url'])
    const qGroupName = readQueryString(route.query, ['group', 'groupName', 'group_name'])
    const qEpIdx = readQueryString(route.query, ['episodeIndex', 'episode_index'])
    if (
      qSourceId === sourceId.value &&
      qPlayUrl === currentPlayUrl.value &&
      qGroupName === activeGroupName.value &&
      qEpIdx === String(currentEpisodeIndex.value)
    ) {
      return
    }
    applyHydratedContext()
    void loadPlay({ checkSavedResume: true })
    if (playGroups.value.length === 0 && currentPlayUrl.value) {
      void fetchDetailFallbackIfNeeded()
    }
  },
)
</script>

<template>
  <VideoPlayerShell
    ref="shellRef"
    :title="headerTitle"
    :subtitle="headerSubtitle"
    :back-label="t('player.backToDetail')"
    :loading="loadingStream"
    :loading-text="loadingStream ? t('online.resolvingStream') : t('player.loading')"
    :buffering="isBuffering"
    :error="loadError"
    :error-title="t('online.streamLoadFailed')"
    :show-retry="Boolean(sourceId && currentPlayUrl)"
    :is-playing="isPlaying"
    :current-time="currentTimeSecs"
    :duration="durationSecs"
    :volume="volume"
    :muted="muted"
    :playback-rate="playbackRate"
    :has-prev="hasPrevEpisode"
    :has-next="hasNextEpisode"
    :show-footer="Boolean(playResult)"
    :header-select-value="headerSelectValue"
    :header-select-options="headerSelectOptions"
    :header-select-aria-label="headerSelectAriaLabel"
    :line-value="activeGroupName"
    :line-options="groupOptions"
    :subtitle-value="selectedSubtitleIdx"
    :subtitle-options="safeSubtitles.length > 0 ? subtitleOptions : []"
    :episodes="episodeDrawerItems"
    :episode-group-value="activeGroupName"
    :episode-group-options="groupOptions"
    :video-element="videoEl"
    @back="goBack"
    @retry="loadPlay({ preserveTimeSecs: currentTimeSecs })"
    @toggle-play="togglePlay"
    @seek="seekTo"
    @seek-delta="seekTo(currentTimeSecs + $event)"
    @prev="goToPrevEpisode"
    @next="goToNextEpisode"
    @update:volume="setVolume"
    @adjust-volume="adjustVolume"
    @toggle-mute="toggleMute"
    @update:playback-rate="onPlaybackRateChange"
    @header-select-change="handleHeaderSelectChange"
    @update:line="onGroupChange"
    @update:subtitle="selectedSubtitleIdx = $event"
    @select-episode="handleSelectEpisodeById"
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
        v-if="activeSubtitle"
        :key="activeSubtitle.resolvedUrl"
        kind="subtitles"
        :src="activeSubtitle.resolvedUrl"
        :srclang="activeSubtitle.lang || 'und'"
        :label="activeSubtitle.label"
        default
      />
    </video>
  </VideoPlayerShell>
</template>

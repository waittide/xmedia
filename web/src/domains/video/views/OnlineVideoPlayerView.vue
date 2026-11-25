<script setup lang="ts">
import { computed, nextTick, onBeforeUnmount, onMounted, ref, watch } from 'vue'
import { useRoute, useRouter } from 'vue-router'
import { LuLibrary } from 'vue-icons-plus/lu'
import { MdButton, type SelectOption } from '@/components/ui'
import { libraryRoute } from '@/sources'
import { usePreferencesStore } from '@/stores/preferences'
import { useSnackbarStore } from '@/stores/snackbar'
import { api as videoApi } from '../api'
import VideoPlayerShell, { type VideoPlayerEpisodeItem } from '../components/VideoPlayerShell.vue'
import {
  buildOnlineVideoDetailRoute,
  buildOnlineVideoPlayRoute,
  hydrateOnlineVideoPlayerContext,
  readQueryString,
  writeStoredPlayerContext,
} from '../composables/useOnlineVideoContext'
import {
  claimVideoAudioFocus,
  clearVideoMediaSession,
  updateVideoMediaSession,
  useVideoPlaybackCore,
  type VideoSubtitleTrackSpec,
} from '../composables/useVideoPlayerCore'
import { useVideoI18n as useI18n } from '../i18n'
import {
  onlineVideoApi,
  resolveOnlineVideoPosterUrl,
  resolveOnlineVideoSubtitleUrl,
  type OnlineVideoEpisode,
  type OnlineVideoPlayGroup,
  type OnlineVideoPlayResponseDto,
  type OnlineVideoSubtitle,
} from '../onlineApi'
import { useVideoStore } from '../store'

interface ResolvedOnlineVideoSubtitle extends OnlineVideoSubtitle {
  resolvedUrl: string
}

const { t } = useI18n()
const route = useRoute()
const router = useRouter()
const preferencesStore = usePreferencesStore()
const videoStore = useVideoStore()
const snackbar = useSnackbarStore()
const trackingToLibrary = ref(false)

const shellRef = ref<InstanceType<typeof VideoPlayerShell> | null>(null)

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
const loadingStream = ref(true)

let playSeq = 0
let detailSeq = 0
let syncUrlSeq = 0
let syncingUrl = false
let unmounted = false
let detailAbortController: AbortController | null = null
let playAbortController: AbortController | null = null

const {
  videoEl,
  isPlaying,
  isBuffering,
  loadError,
  muted,
  volume,
  playbackRate,
  currentTimeSecs,
  rawDurationSecs: durationSecs,
  videoBindings,
  syncAudioGainToElement,
  stopAndClearVideoElement,
  toggleElementPlay,
  resumeFromMediaSession,
  onPlaybackRateChange,
  setVolume,
  adjustVolume,
  toggleMute,
  attachResolvedOnlineStream,
} = useVideoPlaybackCore({
  onEnded: () => {
    if (preferencesStore.video.autoplay_next && hasNextEpisode.value) {
      goToNextEpisode()
    }
  },
  getErrorMessage: () => t('online.streamLoadFailed'),
})

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

const activeSubtitleTrack = computed<VideoSubtitleTrackSpec | null>(() => {
  if (selectedSubtitleIdx.value === 'off') return null
  const idx = Number.parseInt(selectedSubtitleIdx.value, 10)
  if (!Number.isFinite(idx) || idx < 0) return null
  const sub = safeSubtitles.value[idx]
  if (!sub?.resolvedUrl) return null
  return {
    src: sub.resolvedUrl,
    srclang: sub.lang || 'und',
    label: sub.label || 'Subtitles',
  }
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

async function handleTrackToLibrary(): Promise<void> {
  if (
    trackingToLibrary.value ||
    !sourceId.value ||
    (!videoId.value && !detailUrl.value)
  ) {
    return
  }
  trackingToLibrary.value = true
  try {
    const res = await onlineVideoApi.trackOnlineVideo({
      source_id: sourceId.value,
      video_id: videoId.value || null,
      detail_url: detailUrl.value || null,
      group_name: activeGroupName.value || null,
    })

    let targetVideoId: string | null = null
    if (res.kind === 'show' && res.show) {
      const allEps = res.show.seasons.flatMap((s) => s.episodes)
      const curEp = currentEpisode.value
      const matched =
        (curEp
          ? allEps.find(
              (ep) =>
                ep.episode_number === currentEpisodeIndex.value + 1 ||
                ep.title === curEp.name,
            )
          : undefined) ??
        allEps[currentEpisodeIndex.value] ??
        allEps[0]
      targetVideoId = matched?.id ?? null
    } else if (res.video) {
      targetVideoId = res.video.id
    }

    if (targetVideoId && currentTimeSecs.value > 1) {
      try {
        await videoApi.saveProgress(targetVideoId, {
          position_secs: Math.round(currentTimeSecs.value),
          duration_secs: durationSecs.value > 0 ? Math.round(durationSecs.value) : undefined,
          completed: false,
        })
      } catch {
        // Non-fatal: still transition to local player even if initial progress transfer fails
      }
    }

    videoStore.bumpCatalogVersion()
    void videoStore.refreshSidebar(true)
    snackbar.show({ message: t('online.trackedToLibraryToast') })

    if (targetVideoId) {
      stopAndClearVideoElement()
      clearVideoMediaSession()
      await router.replace(`/video/play/${encodeURIComponent(targetVideoId)}`)
    }
  } catch (err) {
    snackbar.show({
      message: err instanceof Error ? err.message : String(err),
      tone: 'error',
    })
  } finally {
    trackingToLibrary.value = false
  }
}

async function fetchDetailFallbackIfNeeded(): Promise<boolean> {
  if (playGroups.value.length > 0 && currentPlayUrl.value) return false
  if (!sourceId.value || (!videoId.value && !detailUrl.value)) return false

  detailAbortController?.abort()
  const controller = new AbortController()
  detailAbortController = controller
  const seq = ++detailSeq
  try {
    const res = await onlineVideoApi.detail(
      sourceId.value,
      {
        video_id: videoId.value || null,
        detail_url: detailUrl.value || null,
      },
      { signal: controller.signal },
    )
    if (unmounted || controller.signal.aborted || seq !== detailSeq) return false

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

function syncMediaSession(): void {
  updateVideoMediaSession({
    title: currentEpisode.value?.name
      ? `${title.value || sourceName.value || 'Video'} · ${currentEpisode.value.name}`
      : title.value || 'Online Video',
    artist: sourceName.value || undefined,
    posterUrl:
      resolveOnlineVideoPosterUrl(proxyCoverUrl.value, coverUrl.value, sourceId.value) || null,
    onPlay: resumeFromMediaSession,
    onPause: () => videoEl.value?.pause(),
    onSeekBy: (delta) => seekTo(currentTimeSecs.value + delta),
    onSeekTo: (target) => seekTo(target),
    onPrev: hasPrevEpisode.value ? () => goToPrevEpisode() : null,
    onNext: hasNextEpisode.value ? () => goToNextEpisode() : null,
  })
}

async function loadPlay(options?: { preserveTimeSecs?: number }): Promise<void> {
  playAbortController?.abort()
  const controller = new AbortController()
  playAbortController = controller
  const seq = ++playSeq
  const requestedPreserve = options?.preserveTimeSecs ?? 0

  // Immediately clear stale stream badge, subtitle track, and previous stream (E2E T2-F16-10)
  stopAndClearVideoElement()
  playResult.value = null
  selectedSubtitleIdx.value = 'off'
  claimVideoAudioFocus()
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
    if (unmounted || controller.signal.aborted || seq !== playSeq) return
  }

  if (!currentPlayUrl.value) {
    loadingStream.value = false
    loadError.value = t('online.missingPlayerContext')
    return
  }

  persistPlayerContext()
  syncMediaSession()

  try {
    const res = await onlineVideoApi.play(
      sourceId.value,
      currentPlayUrl.value,
      activeGroupName.value || null,
      { signal: controller.signal },
    )
    if (unmounted || controller.signal.aborted || seq !== playSeq) return
    playResult.value = res

    const kind = await attachResolvedOnlineStream({
      streamType: res.stream_type,
      streamUrl: res.stream_url,
      proxyStreamUrl: res.proxy_stream_url,
      sourceId: sourceId.value,
      startSecs: requestedPreserve,
      isStale: () => unmounted || controller.signal.aborted || seq !== playSeq,
      unsupportedHlsMessage: t('player.unsupportedHls'),
      defaultErrorMessage: t('online.streamLoadFailed'),
    })
    if (!kind) return
    activeStreamType.value = kind

    shellRef.value?.showHud()
  } catch (err) {
    if (
      unmounted ||
      controller.signal.aborted ||
      seq !== playSeq ||
      (err instanceof Error && err.name === 'AbortError')
    ) {
      return
    }
    isPlaying.value = false
    isBuffering.value = false
    loadError.value = err instanceof Error && err.message ? err.message : t('online.streamLoadFailed')
  } finally {
    if (!unmounted && !controller.signal.aborted && seq === playSeq) {
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
  toggleElementPlay(Boolean(currentPlayUrl.value))
}

function seekTo(targetSecs: number): void {
  const el = videoEl.value
  if (!el) return
  const maxDur =
    Number.isFinite(durationSecs.value) && durationSecs.value > 0 ? durationSecs.value : targetSecs
  const clamped = Math.max(0, Math.min(maxDur, targetSecs))
  el.currentTime = clamped
  currentTimeSecs.value = clamped
}

function goBack(): void {
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
  claimVideoAudioFocus()
  syncAudioGainToElement()
  applyHydratedContext()
  void loadPlay()
  if (playGroups.value.length === 0 && currentPlayUrl.value) {
    void fetchDetailFallbackIfNeeded()
  }
})

onBeforeUnmount(() => {
  unmounted = true
  playSeq += 1
  detailSeq += 1
  playAbortController?.abort()
  playAbortController = null
  detailAbortController?.abort()
  detailAbortController = null
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
    void loadPlay()
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
    :video-bindings="videoBindings"
    :subtitle-track="activeSubtitleTrack"
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
    <template v-if="sourceId && (videoId || detailUrl)" #header-actions>
      <MdButton
        variant="tonal"
        :icon="LuLibrary"
        :loading="trackingToLibrary"
        :disabled="trackingToLibrary"
        @click="handleTrackToLibrary"
      >
        {{ trackingToLibrary ? t('online.trackingBtn') : t('online.trackToLibraryBtn') }}
      </MdButton>
    </template>
  </VideoPlayerShell>
</template>

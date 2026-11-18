<script setup lang="ts">
import { computed, onBeforeUnmount, onMounted, ref, watch } from 'vue'
import { useRoute, useRouter } from 'vue-router'
import {
  LuArrowLeft,
  LuPlay,
  LuPause,
  LuSkipBack,
  LuSkipForward,
  LuVolume2,
  LuVolumeX,
  LuMaximize,
  LuPictureInPicture2,
  LuDownload,
} from 'vue-icons-plus/lu'
import AppSelect from '@/components/ui/AppSelect.vue'
import AppSlider from '@/components/ui/AppSlider.vue'
import { usePlayerStore } from '@/domains/music/player'
import { useThemeStore } from '@/stores/theme'
import { api, buildSubtitleVttUrl, buildVideoDownloadUrl, buildVideoStreamUrl } from '../api'
import { useVideoI18n as useI18n } from '../i18n'
import { useVideoStore } from '../store'
import type { VideoDetailDto, VideoQualityProfile } from '../types'

const { t } = useI18n()
const route = useRoute()
const router = useRouter()
const themeStore = useThemeStore()
const musicPlayerStore = usePlayerStore()
const videoStore = useVideoStore()

const stageRef = ref<HTMLDivElement | null>(null)
const videoEl = ref<HTMLVideoElement | null>(null)

const detail = ref<VideoDetailDto | null>(null)
const quality = ref<VideoQualityProfile>(themeStore.video.default_quality || 'auto')
const selectedAudioStream = ref<number | null>(null)
const selectedSubtitleId = ref<string>('off')
const playbackRate = ref<number>(themeStore.video.playback_rate || 1.0)

const isPlaying = ref(false)
const muted = ref(false)
const volume = ref(0.95)
const streamOffsetSecs = ref(0)
const currentTimeSecs = ref(0)
const rawDurationSecs = ref(0)
const hudVisible = ref(true)

let hudHideTimer: ReturnType<typeof setTimeout> | null = null
let progressSaveTimer: ReturnType<typeof setInterval> | null = null

const isTranscodedStream = computed(() => {
  if (!detail.value) return false
  if (quality.value === 'direct') return false
  if (quality.value !== 'auto') return true
  const defaultAudioIdx = detail.value.audio_streams[0]?.stream_index ?? null
  if (selectedAudioStream.value !== null && selectedAudioStream.value !== defaultAudioIdx) {
    return true
  }
  return !detail.value.video.can_direct_play
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
  return buildSubtitleVttUrl(
    detail.value.video.id,
    selectedSubtitleId.value,
    isTranscodedStream.value ? streamOffsetSecs.value : 0,
  )
})

const qualityOptions = computed(() => [
  { value: 'auto', label: t('player.qualityAuto') },
  { value: 'direct', label: t('player.qualityDirect') },
  { value: 'p1080', label: t('player.quality1080p') },
  { value: 'p720', label: t('player.quality720p') },
  { value: 'p480', label: t('player.quality480p') },
])

const audioOptions = computed(() => {
  const streams = detail.value?.audio_streams ?? []
  if (streams.length === 0) return []
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

const subtitleOptions = computed(() => [
  { value: 'off', label: t('player.subtitleOff') },
  ...(detail.value?.subtitles ?? []).map((sub) => ({
    value: sub.id,
    label: sub.label,
  })),
])

const speedOptions = [
  { value: '0.75', label: '0.75x' },
  { value: '1', label: '1.0x' },
  { value: '1.25', label: '1.25x' },
  { value: '1.5', label: '1.5x' },
  { value: '2', label: '2.0x' },
]

function formatTime(secs: number): string {
  if (!Number.isFinite(secs) || secs < 0) return '0:00'
  const h = Math.floor(secs / 3600)
  const m = Math.floor((secs % 3600) / 60)
  const s = Math.floor(secs % 60)
  if (h > 0) return `${h}:${String(m).padStart(2, '0')}:${String(s).padStart(2, '0')}`
  return `${m}:${String(s).padStart(2, '0')}`
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

function loadStreamAt(startSecs: number, autoPlay = true): void {
  const el = videoEl.value
  const d = detail.value
  if (!el || !d) return

  musicPlayerStore.pause()

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
  el.playbackRate = playbackRate.value
  el.volume = volume.value
  el.muted = muted.value

  if (!useServerOffset && startSecs > 0) {
    el.addEventListener(
      'loadedmetadata',
      () => {
        el.currentTime = startSecs
      },
      { once: true },
    )
  }

  if (autoPlay) {
    void el.play().catch(() => {
      isPlaying.value = false
    })
  }
}

async function initVideo(): Promise<void> {
  musicPlayerStore.pause()
  const id = String(route.params.id || '')
  if (!id) return
  const d = await api.getVideo(id)
  detail.value = d

  const savedProg = d.video.progress
  if (savedProg?.selected_audio_stream != null) {
    selectedAudioStream.value = savedProg.selected_audio_stream
  } else {
    const defAudio = d.audio_streams.find((a) => a.is_default) ?? d.audio_streams[0]
    selectedAudioStream.value = defAudio ? defAudio.stream_index : null
  }

  if (savedProg?.selected_subtitle) {
    selectedSubtitleId.value = savedProg.selected_subtitle
  } else {
    const defSub = d.subtitles.find((s) => s.is_default)
    selectedSubtitleId.value = defSub ? defSub.id : 'off'
  }

  const resumePos =
    savedProg && !savedProg.completed && savedProg.position_secs > 3 ? savedProg.position_secs : 0
  loadStreamAt(resumePos, true)
  revealHud()
}

async function persistProgress(completedOverride?: boolean): Promise<void> {
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
    videoStore.bumpCatalogVersion()
  } catch {
    // ignore transient network errors
  }
}

function togglePlay(): void {
  const el = videoEl.value
  if (!el) return
  if (el.paused) {
    musicPlayerStore.pause()
    void el.play()
  } else {
    el.pause()
  }
  revealHud()
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

function onSpeedChange(val: string): void {
  const rate = Number.parseFloat(val) || 1.0
  playbackRate.value = rate
  if (videoEl.value) videoEl.value.playbackRate = rate
  themeStore.updateVideoPreferences({ playback_rate: rate })
}

function setVolume(val: number): void {
  volume.value = val
  muted.value = val === 0
  if (videoEl.value) {
    videoEl.value.volume = val
    videoEl.value.muted = muted.value
  }
}

function toggleMute(): void {
  muted.value = !muted.value
  if (videoEl.value) videoEl.value.muted = muted.value
}

async function toggleFullscreen(): Promise<void> {
  if (!stageRef.value) return
  if (document.fullscreenElement) {
    await document.exitFullscreen()
  } else {
    await stageRef.value.requestFullscreen()
  }
}

async function togglePip(): Promise<void> {
  const el = videoEl.value
  if (!el || !document.pictureInPictureEnabled) return
  if (document.pictureInPictureElement) {
    await document.exitPictureInPicture()
  } else {
    await el.requestPictureInPicture()
  }
}

function onTimeUpdate(): void {
  const el = videoEl.value
  if (!el) return
  currentTimeSecs.value = streamOffsetSecs.value + (el.currentTime || 0)
}

function onDurationChange(): void {
  if (videoEl.value) {
    rawDurationSecs.value = videoEl.value.duration || 0
  }
}

function onVideoPause(): void {
  isPlaying.value = false
  hudVisible.value = true
  if (videoEl.value?.ended) return
  void persistProgress()
}

function onVideoEnded(): void {
  isPlaying.value = false
  void persistProgress(true)
  if (themeStore.video.autoplay_next && detail.value?.next_episode_id) {
    void router.replace(`/video/watch/${detail.value.next_episode_id}`)
  }
}

function goBack(): void {
  void persistProgress()
  if (window.history.state?.back) {
    router.back()
  } else if (detail.value?.video.show_id) {
    void router.push(`/video/shows/${detail.value.video.show_id}`)
  } else {
    void router.push('/video')
  }
}

function onKeydown(ev: KeyboardEvent): void {
  if (ev.target instanceof HTMLInputElement || ev.target instanceof HTMLTextAreaElement) return
  if (ev.code === 'Space') {
    ev.preventDefault()
    togglePlay()
  } else if (ev.code === 'ArrowRight') {
    ev.preventDefault()
    seekTo(currentTimeSecs.value + 10)
  } else if (ev.code === 'ArrowLeft') {
    ev.preventDefault()
    seekTo(currentTimeSecs.value - 10)
  } else if (ev.key === 'f' || ev.key === 'F') {
    void toggleFullscreen()
  } else if (ev.key === 'Escape' && !document.fullscreenElement) {
    goBack()
  }
}

onMounted(() => {
  musicPlayerStore.pause()
  void initVideo()
  window.addEventListener('keydown', onKeydown)
  progressSaveTimer = setInterval(() => {
    if (isPlaying.value) {
      void persistProgress()
    }
  }, 8000)
})

onBeforeUnmount(() => {
  void persistProgress()
  if (hudHideTimer) clearTimeout(hudHideTimer)
  if (progressSaveTimer) clearInterval(progressSaveTimer)
  window.removeEventListener('keydown', onKeydown)
})

watch(() => route.params.id, () => {
  void initVideo()
})
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
      @play="isPlaying = true; musicPlayerStore.pause(); revealHud()"
      @pause="onVideoPause"
      @timeupdate="onTimeUpdate"
      @durationchange="onDurationChange"
      @ended="onVideoEnded"
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

    <!-- Top HUD Overlay -->
    <header
      class="absolute inset-x-0 top-0 z-20 flex flex-wrap items-center justify-between gap-2 bg-gradient-to-b from-black/80 via-black/40 to-transparent px-4 py-3 transition-opacity duration-200 sm:gap-4 sm:px-8 sm:py-4"
      :class="hudVisible ? 'opacity-100' : 'pointer-events-none opacity-0'"
    >
      <div class="flex min-w-0 items-center gap-3">
        <button
          type="button"
          class="inline-flex items-center gap-1.5 rounded-full bg-white/15 px-3.5 py-2 text-xs font-semibold text-white backdrop-blur-md transition hover:bg-white/25"
          @click="goBack"
        >
          <LuArrowLeft class="h-4 w-4" />
          <span>{{ t('player.back') }}</span>
        </button>

        <div v-if="detail" class="min-w-0">
          <h1 class="truncate text-sm font-bold text-white sm:text-base">
            {{ detail.video.title }}
          </h1>
          <p v-if="detail.video.show_title" class="truncate text-[11px] text-white/70">
            {{ detail.video.show_title }}
            <span v-if="detail.video.season_number != null && detail.video.episode_number != null">
              · S{{ String(detail.video.season_number).padStart(2, '0') }}E{{ String(detail.video.episode_number).padStart(2, '0') }}
            </span>
          </p>
        </div>
      </div>

      <div v-if="detail" class="flex flex-wrap items-center gap-1.5 sm:gap-2">
        <div v-if="audioOptions.length > 1" class="w-32 sm:w-44">
          <AppSelect
            :model-value="String(selectedAudioStream ?? detail.audio_streams[0]?.stream_index ?? 0)"
            :options="audioOptions"
            variant="compact"
            @change="onAudioStreamChange"
          />
        </div>

        <div v-if="subtitleOptions.length > 1" class="w-28 sm:w-40">
          <AppSelect
            :model-value="selectedSubtitleId"
            :options="subtitleOptions"
            variant="compact"
            @change="onSubtitleChange"
          />
        </div>

        <div class="w-28 sm:w-40">
          <AppSelect
            :model-value="quality"
            :options="qualityOptions"
            variant="compact"
            @change="onQualityChange"
          />
        </div>

        <div class="hidden w-24 sm:block">
          <AppSelect
            :model-value="String(playbackRate)"
            :options="speedOptions"
            variant="compact"
            @change="onSpeedChange"
          />
        </div>

        <a
          :href="buildVideoDownloadUrl(detail.video.id)"
          class="rounded-full bg-white/15 p-2 text-white backdrop-blur-md transition hover:bg-white/25"
          :title="t('common.download')"
        >
          <LuDownload class="h-4 w-4" />
        </a>
      </div>
    </header>

    <!-- Bottom HUD Overlay -->
    <footer
      class="absolute inset-x-0 bottom-0 z-20 space-y-2.5 bg-gradient-to-t from-black/85 via-black/45 to-transparent px-5 pt-8 pb-5 transition-opacity duration-200 sm:px-8"
      :class="hudVisible ? 'opacity-100' : 'pointer-events-none opacity-0'"
    >
      <!-- Seek Slider -->
      <div class="flex items-center gap-3">
        <span class="w-14 text-right font-mono text-xs text-white/85">
          {{ formatTime(currentTimeSecs) }}
        </span>
        <div class="flex-1">
          <AppSlider
            :model-value="currentTimeSecs"
            :min="0"
            :max="Math.max(1, totalDurationSecs)"
            :step="0.5"
            commit-on-release
            aria-label="Video timeline"
            @change="seekTo($event)"
          />
        </div>
        <span class="w-14 font-mono text-xs text-white/85">
          {{ formatTime(totalDurationSecs) }}
        </span>
      </div>

      <!-- Controls Row -->
      <div class="flex items-center justify-between gap-4">
        <div class="flex items-center gap-2 sm:gap-3">
          <button
            v-if="detail?.prev_episode_id"
            type="button"
            class="inline-flex items-center gap-1 rounded-full bg-white/15 px-3 py-1.5 text-xs font-semibold text-white backdrop-blur-md hover:bg-white/25"
            @click="router.replace(`/video/watch/${detail.prev_episode_id}`)"
          >
            <LuSkipBack class="h-3.5 w-3.5 fill-current" />
            <span class="hidden sm:inline">{{ t('player.prevEpisode') }}</span>
          </button>

          <button
            type="button"
            class="flex h-10 w-10 items-center justify-center rounded-full bg-[var(--ctp-accent)] text-[var(--ctp-on-accent)] shadow-lg transition hover:scale-105"
            @click="togglePlay"
          >
            <LuPause v-if="isPlaying" class="h-5 w-5 fill-current" />
            <LuPlay v-else class="ml-0.5 h-5 w-5 fill-current" />
          </button>

          <button
            v-if="detail?.next_episode_id"
            type="button"
            class="inline-flex items-center gap-1 rounded-full bg-white/15 px-3 py-1.5 text-xs font-semibold text-white backdrop-blur-md hover:bg-white/25"
            @click="router.replace(`/video/watch/${detail.next_episode_id}`)"
          >
            <span class="hidden sm:inline">{{ t('player.nextEpisode') }}</span>
            <LuSkipForward class="h-3.5 w-3.5 fill-current" />
          </button>

          <div class="hidden w-32 items-center gap-2 sm:flex">
            <button
              type="button"
              class="rounded-lg p-1.5 text-white/85 hover:bg-white/15"
              @click="toggleMute"
            >
              <LuVolumeX v-if="muted || volume === 0" class="h-4 w-4" />
              <LuVolume2 v-else class="h-4 w-4" />
            </button>
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

        <div class="flex items-center gap-2">
          <button
            type="button"
            class="rounded-xl bg-white/15 p-2 text-white backdrop-blur-md transition hover:bg-white/25"
            :title="t('player.pip')"
            @click="togglePip"
          >
            <LuPictureInPicture2 class="h-4 w-4" />
          </button>

          <button
            type="button"
            class="rounded-xl bg-white/15 p-2 text-white backdrop-blur-md transition hover:bg-white/25"
            :title="t('player.fullscreen')"
            @click="toggleFullscreen"
          >
            <LuMaximize class="h-4 w-4" />
          </button>
        </div>
      </div>
    </footer>
  </div>
</template>

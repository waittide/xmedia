<script setup lang="ts">
import { computed, onBeforeUnmount, onMounted, ref, type Component } from 'vue'
import {
  LuAlertTriangle,
  LuArrowLeft,
  LuFastForward,
  LuListVideo,
  LuMaximize,
  LuMinimize,
  LuPause,
  LuPictureInPicture2,
  LuPlay,
  LuRefreshCw,
  LuRewind,
  LuSettings,
  LuSkipBack,
  LuSkipForward,
  LuVolume2,
  LuVolumeX,
} from 'vue-icons-plus/lu'
import { ImmersiveMediaTemplate } from '@/components/domain'
import {
  AppSlider,
  MdButton,
  MdCircularProgress,
  MdDialog,
  MdIconButton,
  MdSelect,
  type SelectOption,
} from '@/components/ui'
import { formatTime } from '@/utils/format'
import type {
  VideoElementBindings,
  VideoSubtitleTrackSpec,
} from '../composables/useVideoPlayerCore'
import { useVideoI18n as useI18n } from '../i18n'
import VideoPlayerSheetContent, {
  type VideoPlayerEpisodeItem,
} from './player/VideoPlayerSheetContent.vue'

export type { VideoPlayerEpisodeItem }

const INTERACTIVE_SELECTOR =
  'button, a, input, select, textarea, label, [role="button"], [role="combobox"], [role="slider"], [role="menuitem"], [role="tab"], [role="option"], [data-immersive-ignore]'

const props = withDefaults(
  defineProps<{
    title: string
    subtitle?: string
    backLabel?: string
    loading?: boolean
    loadingText?: string
    buffering?: boolean
    error?: string | null
    errorTitle?: string
    showRetry?: boolean
    isPlaying: boolean
    currentTime: number
    duration: number
    volume: number
    muted: boolean
    playbackRate: number
    hasPrev?: boolean
    hasNext?: boolean
    showFooter?: boolean
    headerSelectValue?: string
    headerSelectOptions?: SelectOption[]
    headerSelectAriaLabel?: string
    qualityValue?: string
    qualityOptions?: SelectOption[]
    lineValue?: string
    lineOptions?: SelectOption[]
    audioValue?: string
    audioOptions?: SelectOption[]
    subtitleValue?: string
    subtitleOptions?: SelectOption[]
    downloadUrl?: string | null
    episodes?: VideoPlayerEpisodeItem[]
    episodeGroupValue?: string
    episodeGroupOptions?: SelectOption[]
    videoElement?: HTMLVideoElement | null
    videoBindings?: VideoElementBindings | null
    subtitleTrack?: VideoSubtitleTrackSpec | null
  }>(),
  {
    subtitle: '',
    backLabel: '',
    loading: false,
    loadingText: '',
    buffering: false,
    error: null,
    errorTitle: '',
    showRetry: true,
    hasPrev: false,
    hasNext: false,
    showFooter: true,
    headerSelectValue: '',
    headerSelectOptions: () => [],
    headerSelectAriaLabel: '',
    qualityValue: '',
    qualityOptions: () => [],
    lineValue: '',
    lineOptions: () => [],
    audioValue: '',
    audioOptions: () => [],
    subtitleValue: 'off',
    subtitleOptions: () => [],
    downloadUrl: null,
    episodes: () => [],
    episodeGroupValue: '',
    episodeGroupOptions: () => [],
    videoElement: null,
    videoBindings: null,
    subtitleTrack: null,
  },
)

const emit = defineEmits<{
  back: []
  retry: []
  'toggle-play': []
  seek: [targetSecs: number]
  'seek-delta': [deltaSecs: number]
  prev: []
  next: []
  'update:volume': [volume: number]
  'adjust-volume': [delta: number]
  'toggle-mute': []
  'update:playbackRate': [rate: number]
  'header-select-change': [value: string]
  'update:quality': [value: string]
  'update:line': [value: string]
  'update:audio': [value: string]
  'update:subtitle': [value: string]
  'select-episode': [id: string]
}>()

const { t } = useI18n()

const shellRootRef = ref<HTMLElement | null>(null)
const internalVideoEl = ref<HTMLVideoElement | null>(null)

function bindInternalVideo(el: unknown): void {
  const video = el instanceof HTMLVideoElement ? el : null
  internalVideoEl.value = video
  props.videoBindings?.bindElement(video)
}

const immersiveRef = ref<InstanceType<typeof ImmersiveMediaTemplate> | null>(null)
const activeSheet = ref<'settings' | 'episodes' | null>(null)
const isFullscreen = ref(false)
const lastPointerType = ref<string>('mouse')
const scrubbingTime = ref<number | null>(null)

const displayedTime = computed(() =>
  scrubbingTime.value !== null ? scrubbingTime.value : props.currentTime,
)

const pipSupported = computed(
  () => typeof document !== 'undefined' && Boolean(document.pictureInPictureEnabled),
)

const hasEpisodes = computed(() => props.episodes.length > 1)

const hudLocked = computed(
  () =>
    !props.isPlaying ||
    props.loading ||
    props.buffering ||
    Boolean(props.error) ||
    activeSheet.value !== null,
)

const sheetTitle = computed(() => {
  if (activeSheet.value === 'episodes') return t('player.episodes')
  if (activeSheet.value === 'settings') return t('player.settings')
  return ''
})

const sheetSubtitle = computed(() => {
  if (activeSheet.value === 'episodes' && props.episodes.length > 0) {
    return t('common.episodesCount', { count: props.episodes.length })
  }
  return ''
})

function showOsd(text: string, icon?: Component): void {
  immersiveRef.value?.showOsd(text, { icon })
}

function showHud(): void {
  immersiveRef.value?.showHud()
}

function handleFullscreenChange(): void {
  isFullscreen.value = Boolean(document.fullscreenElement)
}

async function toggleFullscreen(): Promise<void> {
  const root = shellRootRef.value
  if (!root) return
  try {
    if (document.fullscreenElement) {
      await document.exitFullscreen()
    } else {
      await root.requestFullscreen()
    }
  } catch {
    // ignore fullscreen permission errors
  }
}

async function togglePip(): Promise<void> {
  const el = props.videoElement ?? internalVideoEl.value
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

function handleBack(): void {
  if (document.fullscreenElement) {
    void document.exitFullscreen().catch(() => {})
  }
  emit('back')
}

function handleStagePointerDown(ev: PointerEvent): void {
  lastPointerType.value = ev.pointerType || 'mouse'
}

function handleStageClick(ev: MouseEvent): void {
  if (props.error) return
  const target = ev.target as HTMLElement | null
  if (!target || target.closest(INTERACTIVE_SELECTOR)) return

  if (lastPointerType.value === 'touch') {
    immersiveRef.value?.toggleHud()
    return
  }

  emit('toggle-play')
  immersiveRef.value?.showHud()
}

function handleStageDblClick(ev: MouseEvent): void {
  if (props.error || lastPointerType.value === 'touch') return
  const target = ev.target as HTMLElement | null
  if (!target || target.closest(INTERACTIVE_SELECTOR)) return
  void toggleFullscreen()
}

function handleSliderInput(val: number): void {
  scrubbingTime.value = val
}

function handleSliderCommit(val: number): void {
  scrubbingTime.value = null
  emit('seek', val)
}

function handleSpeedSelect(rateStr: string): void {
  const rate = Number.parseFloat(rateStr) || 1.0
  emit('update:playbackRate', rate)
  showOsd(`${rate}x`)
}

function handlePickEpisode(id: string): void {
  activeSheet.value = null
  emit('select-episode', id)
}

function isEditableTarget(target: EventTarget | null): boolean {
  return (
    target instanceof HTMLElement &&
    (target.isContentEditable || ['INPUT', 'TEXTAREA', 'SELECT'].includes(target.tagName))
  )
}

function isPopupOpen(target: EventTarget | null): boolean {
  if (typeof document !== 'undefined' && document.querySelector('[role="listbox"], [aria-modal="true"]')) {
    return true
  }
  const role = target instanceof HTMLElement ? target.getAttribute('role') : null
  return role === 'combobox' || role === 'listbox' || role === 'option'
}

function onKeydown(ev: KeyboardEvent): void {
  if (activeSheet.value !== null) return
  if (isEditableTarget(ev.target) || isPopupOpen(ev.target)) return
  if (ev.ctrlKey || ev.metaKey || ev.altKey) return

  if (ev.code === 'Space') {
    ev.preventDefault()
    emit('toggle-play')
    showOsd(
      props.isPlaying ? t('player.pause') : t('player.play'),
      props.isPlaying ? LuPause : LuPlay,
    )
    immersiveRef.value?.showHud()
  } else if (ev.code === 'ArrowRight') {
    ev.preventDefault()
    emit('seek-delta', 10)
    showOsd(t('player.seekForward', { secs: 10 }), LuFastForward)
    immersiveRef.value?.showHud()
  } else if (ev.code === 'ArrowLeft') {
    ev.preventDefault()
    emit('seek-delta', -10)
    showOsd(t('player.seekBackward', { secs: 10 }), LuRewind)
    immersiveRef.value?.showHud()
  } else if (ev.code === 'ArrowUp') {
    ev.preventDefault()
    emit('adjust-volume', 0.05)
    const nextPct = Math.round(Math.min(1, props.volume + 0.05) * 100)
    showOsd(`${t('player.volume')} ${nextPct}%`, LuVolume2)
    immersiveRef.value?.showHud()
  } else if (ev.code === 'ArrowDown') {
    ev.preventDefault()
    emit('adjust-volume', -0.05)
    const nextPct = Math.round(Math.max(0, props.volume - 0.05) * 100)
    showOsd(`${t('player.volume')} ${nextPct}%`, nextPct === 0 ? LuVolumeX : LuVolume2)
    immersiveRef.value?.showHud()
  } else if (ev.key === 'm' || ev.key === 'M') {
    ev.preventDefault()
    emit('toggle-mute')
    showOsd(
      props.muted ? t('player.unmute') : t('player.mute'),
      props.muted ? LuVolume2 : LuVolumeX,
    )
    immersiveRef.value?.showHud()
  } else if (ev.key === 'f' || ev.key === 'F') {
    ev.preventDefault()
    void toggleFullscreen()
  } else if (ev.key === 'p' || ev.key === 'P') {
    ev.preventDefault()
    void togglePip()
  } else if ((ev.key === 'n' || ev.key === 'N') && props.hasNext) {
    ev.preventDefault()
    emit('next')
  }
}

onMounted(() => {
  window.addEventListener('keydown', onKeydown)
  document.addEventListener('fullscreenchange', handleFullscreenChange)
  handleFullscreenChange()
})

onBeforeUnmount(() => {
  window.removeEventListener('keydown', onKeydown)
  document.removeEventListener('fullscreenchange', handleFullscreenChange)
})

defineExpose({
  showHud,
  showOsd,
  toggleFullscreen,
  togglePip,
})
</script>

<template>
  <div ref="shellRootRef" class="md-scheme-dark h-dvh w-screen overflow-hidden bg-black">
    <ImmersiveMediaTemplate
      ref="immersiveRef"
      :title="title"
      :subtitle="subtitle"
      dark-scheme
      stage-bg-class="bg-black text-on-surface"
      :auto-hide="true"
      :auto-hide-delay-ms="2800"
      :lock-hud="hudLocked"
      :hide-cursor-when-idle="isPlaying"
      :back-label="backLabel || t('player.back')"
      center-tap-mode="disabled"
      :active-sheet="activeSheet"
      :sheet-title="sheetTitle"
      :sheet-subtitle="sheetSubtitle"
      sheet-size="sm"
      :sheet-teleport-to="shellRootRef"
      @update:active-sheet="activeSheet = ($event as 'settings' | 'episodes' | null)"
      @back="handleBack"
    >
      <template #default="{ hudVisible }">
        <div
          class="relative flex h-full w-full items-center justify-center bg-black"
          @pointerdown="handleStagePointerDown"
          @click="handleStageClick"
          @dblclick="handleStageDblClick"
        >
          <slot>
            <video
              v-if="videoBindings"
              :ref="bindInternalVideo"
              class="h-full w-full bg-black object-contain"
              crossorigin="use-credentials"
              playsinline
              @loadstart="videoBindings.onBufferingStart"
              @waiting="videoBindings.onBufferingStart"
              @canplay="videoBindings.onBufferingEnd"
              @playing="videoBindings.onBufferingEnd"
              @play="videoBindings.onPlay"
              @pause="videoBindings.onPause"
              @timeupdate="videoBindings.onTimeUpdate"
              @durationchange="videoBindings.onDurationChange"
              @ended="videoBindings.onEnded"
              @error="videoBindings.onError"
            >
              <track
                v-if="subtitleTrack?.src"
                :key="subtitleTrack.src"
                kind="subtitles"
                :src="subtitleTrack.src"
                :srclang="subtitleTrack.srclang || 'und'"
                :label="subtitleTrack.label || 'Subtitles'"
                default
              />
            </video>
          </slot>

          <div
            v-if="hudVisible && !loading && !buffering && !error && lastPointerType === 'touch'"
            class="pointer-events-none absolute inset-0 flex items-center justify-center"
          >
            <button
              type="button"
              :aria-label="isPlaying ? t('player.pause') : t('player.play')"
              class="md-state pointer-events-auto flex h-14 w-14 cursor-pointer items-center justify-center rounded-corner-full bg-scrim/65 text-white backdrop-blur-md transition-transform duration-150 ease-standard active:scale-95"
              @click.stop="emit('toggle-play')"
            >
              <LuPause v-if="isPlaying" class="h-6 w-6 fill-current" aria-hidden="true" />
              <LuPlay v-else class="ml-0.5 h-6 w-6 fill-current" aria-hidden="true" />
            </button>
          </div>
        </div>
      </template>

      <template #header-controls>
        <slot name="header-actions" />

        <div v-if="headerSelectOptions.length > 0" class="w-32 sm:w-40">
          <MdSelect
            :model-value="headerSelectValue"
            :options="headerSelectOptions"
            variant="compact"
            :aria-label="headerSelectAriaLabel"
            :teleport-to="shellRootRef"
            @change="emit('header-select-change', $event)"
          />
        </div>

        <MdIconButton
          variant="standard"
          size="sm"
          :icon="LuSettings"
          :aria-label="t('player.settings')"
          :tooltip="t('player.settings')"
          @click="activeSheet = activeSheet === 'settings' ? null : 'settings'"
        />
      </template>

      <template v-if="showFooter && !error" #footer>
        <div class="space-y-2">
          <div class="flex items-center gap-3">
            <span class="text-label-medium w-14 text-right font-mono tabular-nums text-on-surface">
              {{ formatTime(displayedTime) }}
            </span>
            <div class="flex-1">
              <AppSlider
                :model-value="displayedTime"
                :min="0"
                :max="Math.max(1, duration)"
                :step="0.5"
                commit-on-release
                :aria-label="t('player.timeline')"
                @update:model-value="handleSliderInput"
                @change="handleSliderCommit"
              />
            </div>
            <span class="text-label-medium w-14 font-mono tabular-nums text-on-surface-variant">
              {{ formatTime(duration) }}
            </span>
          </div>

          <div class="flex items-center justify-between gap-3">
            <div class="flex items-center gap-1.5 sm:gap-2">
              <MdIconButton
                v-if="hasPrev"
                variant="standard"
                size="sm"
                :icon="LuSkipBack"
                :aria-label="t('player.prevEpisode')"
                :tooltip="t('player.prevEpisode')"
                @click="emit('prev')"
              />

              <MdIconButton
                variant="filled"
                size="md"
                :aria-label="isPlaying ? t('player.pause') : t('player.play')"
                :tooltip="isPlaying ? t('player.pause') : t('player.play')"
                @click="emit('toggle-play')"
              >
                <LuPause v-if="isPlaying" class="h-5 w-5 fill-current" aria-hidden="true" />
                <LuPlay v-else class="ml-0.5 h-5 w-5 fill-current" aria-hidden="true" />
              </MdIconButton>

              <MdIconButton
                v-if="hasNext"
                variant="standard"
                size="sm"
                :icon="LuSkipForward"
                :aria-label="t('player.nextEpisode')"
                :tooltip="t('player.nextEpisode')"
                @click="emit('next')"
              />

              <div class="flex items-center gap-1.5 sm:w-32">
                <MdIconButton
                  variant="standard"
                  size="sm"
                  :icon="muted || volume === 0 ? LuVolumeX : LuVolume2"
                  :aria-label="muted || volume === 0 ? t('player.unmute') : t('player.mute')"
                  :tooltip="muted || volume === 0 ? t('player.unmute') : t('player.mute')"
                  @click="emit('toggle-mute')"
                />
                <div class="hidden w-24 sm:block">
                  <AppSlider
                    :model-value="muted ? 0 : volume"
                    :min="0"
                    :max="1"
                    :step="0.02"
                    :aria-label="t('player.volume')"
                    @update:model-value="emit('update:volume', $event)"
                  />
                </div>
              </div>
            </div>

            <div class="flex items-center gap-1">
              <MdButton
                v-if="hasEpisodes"
                variant="text"
                :icon="LuListVideo"
                :aria-label="t('player.episodes')"
                @click="activeSheet = activeSheet === 'episodes' ? null : 'episodes'"
              >
                <span class="hidden sm:inline">{{ t('player.episodesBtn') }}</span>
              </MdButton>

              <MdIconButton
                v-if="pipSupported"
                variant="standard"
                size="sm"
                :icon="LuPictureInPicture2"
                :aria-label="t('player.pip')"
                :tooltip="t('player.pip')"
                @click="togglePip"
              />

              <MdIconButton
                variant="standard"
                size="sm"
                :icon="isFullscreen ? LuMinimize : LuMaximize"
                :aria-label="isFullscreen ? t('player.exitFullscreen') : t('player.fullscreen')"
                :tooltip="isFullscreen ? t('player.exitFullscreen') : t('player.fullscreen')"
                @click="toggleFullscreen"
              />
            </div>
          </div>
        </div>
      </template>

      <template #overlay>
        <div
          v-if="(loading || buffering) && !error"
          class="pointer-events-none fixed inset-0 z-10 flex items-center justify-center bg-scrim/35"
        >
          <div
            class="flex items-center gap-3 rounded-corner-full bg-surface-container-high/92 px-5 py-3 text-label-large text-on-surface shadow-elevation-2 backdrop-blur-md"
          >
            <MdCircularProgress indeterminate :size="20" :stroke-width="2.5" />
            <span>{{ loadingText || t('player.loading') }}</span>
          </div>
        </div>

        <MdDialog
          :open="Boolean(error)"
          :title="errorTitle || t('player.loadErrorTitle')"
          size="sm"
          :teleport-to="shellRootRef"
          @close="handleBack"
        >
          <template #icon>
            <LuAlertTriangle class="h-6 w-6 text-error" aria-hidden="true" />
          </template>

          <p role="alert" class="text-body-medium text-on-surface-variant">
            {{ error }}
          </p>

          <template #footer>
            <MdButton variant="outlined" :icon="LuArrowLeft" @click="handleBack">
              {{ backLabel || t('player.back') }}
            </MdButton>
            <MdButton
              v-if="showRetry"
              variant="filled"
              :icon="LuRefreshCw"
              @click="emit('retry')"
            >
              {{ t('player.retry') }}
            </MdButton>
          </template>
        </MdDialog>
      </template>

      <template #sheet="{ activeSheet: sheetKey }">
        <VideoPlayerSheetContent
          :sheet-key="sheetKey"
          :teleport-to="shellRootRef"
          :playback-rate="playbackRate"
          :quality-value="qualityValue"
          :quality-options="qualityOptions"
          :line-value="lineValue"
          :line-options="lineOptions"
          :audio-value="audioValue"
          :audio-options="audioOptions"
          :subtitle-value="subtitleValue"
          :subtitle-options="subtitleOptions"
          :download-url="downloadUrl"
          :episodes="episodes"
          :episode-group-value="episodeGroupValue"
          :episode-group-options="episodeGroupOptions"
          @select-speed="handleSpeedSelect"
          @update:quality="emit('update:quality', $event)"
          @update:line="emit('update:line', $event)"
          @update:audio="emit('update:audio', $event)"
          @update:subtitle="emit('update:subtitle', $event)"
          @select-episode="handlePickEpisode"
        />
      </template>
    </ImmersiveMediaTemplate>
  </div>
</template>

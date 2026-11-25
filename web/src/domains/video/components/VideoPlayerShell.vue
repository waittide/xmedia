<script setup lang="ts">
import { computed, onBeforeUnmount, onMounted, ref, watch, type Component } from 'vue'
import {
  LuAlertTriangle,
  LuArrowLeft,
  LuCheck,
  LuDownload,
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
  MdChip,
  MdCircularProgress,
  MdDialog,
  MdIconButton,
  MdSelect,
  MdTextField,
  type SelectOption,
} from '@/components/ui'
import { formatTime } from '@/utils/format'
import {
  VIDEO_SPEED_OPTIONS,
  type VideoElementBindings,
  type VideoSubtitleTrackSpec,
} from '../composables/useVideoPlayerCore'
import { useVideoI18n as useI18n } from '../i18n'

export interface VideoPlayerEpisodeItem {
  id: string
  label: string
  subtitle?: string
  active: boolean
  watched?: boolean
  disabled?: boolean
}

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
    /** Single primary combobox rendered in `<header>` (Quality for local / Line or Subtitle for online). */
    headerSelectValue?: string
    headerSelectOptions?: SelectOption[]
    headerSelectAriaLabel?: string
    /** Settings sheet controls */
    qualityValue?: string
    qualityOptions?: SelectOption[]
    lineValue?: string
    lineOptions?: SelectOption[]
    audioValue?: string
    audioOptions?: SelectOption[]
    subtitleValue?: string
    subtitleOptions?: SelectOption[]
    downloadUrl?: string | null
    /** On-demand Episodes MdSheet */
    episodes?: VideoPlayerEpisodeItem[]
    episodeGroupValue?: string
    episodeGroupOptions?: SelectOption[]
    /** Underlying `<video>` element reference for Picture-in-Picture */
    videoElement?: HTMLVideoElement | null
    /** Unified `<video>` event bindings when using built-in video stage */
    videoBindings?: VideoElementBindings | null
    /** Active VTT subtitle track rendered inside built-in `<video>` */
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
const episodeFilter = ref('')

// Real-time scrubbing preview without mutating video until release
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

const filteredEpisodes = computed(() => {
  const q = episodeFilter.value.trim().toLowerCase()
  if (!q) return props.episodes
  return props.episodes.filter(
    (ep) =>
      ep.label.toLowerCase().includes(q) ||
      (ep.subtitle?.toLowerCase().includes(q) ?? false),
  )
})

watch(activeSheet, (sheet) => {
  if (sheet !== 'episodes') {
    episodeFilter.value = ''
  }
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

  // Touch tap toggles HUD without accidentally pausing playback (V-03)
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
  if (!(target instanceof HTMLElement)) return false
  if (target.isContentEditable) return true
  const tag = target.tagName
  return tag === 'INPUT' || tag === 'TEXTAREA' || tag === 'SELECT'
}

function isPopupOpen(target: EventTarget | null): boolean {
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
  if (activeSheet.value !== null) return
  if (isEditableTarget(ev.target) || isPopupOpen(ev.target)) return
  if (ev.ctrlKey || ev.metaKey || ev.altKey) return

  if (ev.code === 'Space') {
    ev.preventDefault()
    emit('toggle-play')
    showOsd(props.isPlaying ? t('player.pause') : t('player.play'), props.isPlaying ? LuPause : LuPlay)
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
    showOsd(props.muted ? t('player.unmute') : t('player.mute'), props.muted ? LuVolume2 : LuVolumeX)
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
      <!-- Stage Slot (wraps `<video>` with clean mouse vs touch gesture handling) -->
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

          <!-- Touch Center Play/Pause Button (visible when HUD is open on touch devices) -->
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

      <!-- Top HUD Controls -->
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

      <!-- Bottom HUD Controls -->
      <template v-if="showFooter && !error" #footer>
        <div class="space-y-2">
          <!-- Seek Timeline -->
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

          <!-- Transport & Utility Row -->
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
                size="sm"
                :icon="LuListVideo"
                :aria-label="t('player.episodes')"
                @click="activeSheet = activeSheet === 'episodes' ? null : 'episodes'"
              >
                <span class="hidden sm:inline">{{ t('player.episodes') }}</span>
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

      <!-- Loading / Buffering & Error Overlays -->
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
            <MdButton variant="outlined" size="sm" :icon="LuArrowLeft" @click="handleBack">
              {{ backLabel || t('player.back') }}
            </MdButton>
            <MdButton
              v-if="showRetry"
              variant="filled"
              size="sm"
              :icon="LuRefreshCw"
              @click="emit('retry')"
            >
              {{ t('player.retry') }}
            </MdButton>
          </template>
        </MdDialog>
      </template>

      <!-- Mutually-Exclusive MdSheet: Episodes OR Playback Settings -->
      <template #sheet="{ activeSheet: sheetKey }">
        <!-- Episodes Drawer -->
        <div v-if="sheetKey === 'episodes'" class="space-y-4">
          <div v-if="episodeGroupOptions.length > 1" class="space-y-1.5">
            <span class="text-label-medium block text-on-surface-variant">
              {{ t('player.lineLabel') }}
            </span>
            <MdSelect
              :model-value="episodeGroupValue"
              :options="episodeGroupOptions"
              size="sm"
              :teleport-to="shellRootRef"
              @change="emit('update:line', $event)"
            />
          </div>

          <MdTextField
            v-if="episodes.length > 12"
            v-model="episodeFilter"
            type="search"
            size="sm"
            clearable
            :placeholder="t('online.filterEpisodesPlaceholder')"
          />

          <div class="space-y-1.5">
            <button
              v-for="ep in filteredEpisodes"
              :key="ep.id"
              type="button"
              :disabled="ep.disabled"
              class="md-state flex w-full cursor-pointer items-center justify-between gap-3 rounded-corner-md border px-3.5 py-2.5 text-left transition-colors duration-150 ease-standard disabled:cursor-not-allowed disabled:opacity-38"
              :class="
                ep.active
                  ? 'border-primary/40 bg-primary-container text-on-primary-container'
                  : 'border-outline-variant/60 bg-surface-container-low text-on-surface hover:bg-surface-container'
              "
              @click="handlePickEpisode(ep.id)"
            >
              <div class="min-w-0 flex-1">
                <div class="text-title-small truncate">{{ ep.label }}</div>
                <div
                  v-if="ep.subtitle"
                  class="text-body-small mt-0.5 truncate opacity-80"
                >
                  {{ ep.subtitle }}
                </div>
              </div>
              <LuCheck v-if="ep.active || ep.watched" class="h-4 w-4 shrink-0" aria-hidden="true" />
            </button>
          </div>
        </div>

        <!-- Settings Drawer (Speed / Quality / Line / Audio / Subtitles / Download) -->
        <div v-else-if="sheetKey === 'settings'" class="space-y-5">
          <div class="space-y-2">
            <span class="text-label-medium block text-on-surface-variant">
              {{ t('player.speed') }}
            </span>
            <div class="flex flex-wrap gap-1.5">
              <MdChip
                v-for="opt in VIDEO_SPEED_OPTIONS"
                :key="opt.value"
                variant="filter"
                :label="opt.label"
                :selected="String(playbackRate) === opt.value"
                hide-check
                @click="handleSpeedSelect(opt.value)"
              />
            </div>
          </div>

          <div v-if="qualityOptions.length > 0" class="space-y-1.5">
            <span class="text-label-medium block text-on-surface-variant">
              {{ t('player.quality') }}
            </span>
            <MdSelect
              :model-value="qualityValue"
              :options="qualityOptions"
              size="sm"
              :teleport-to="shellRootRef"
              @change="emit('update:quality', $event)"
            />
          </div>

          <div v-if="lineOptions.length > 1" class="space-y-1.5">
            <span class="text-label-medium block text-on-surface-variant">
              {{ t('player.lineLabel') }}
            </span>
            <MdSelect
              :model-value="lineValue"
              :options="lineOptions"
              size="sm"
              :teleport-to="shellRootRef"
              @change="emit('update:line', $event)"
            />
          </div>

          <div v-if="audioOptions.length > 1" class="space-y-1.5">
            <span class="text-label-medium block text-on-surface-variant">
              {{ t('player.audioTrack') }}
            </span>
            <MdSelect
              :model-value="audioValue"
              :options="audioOptions"
              size="sm"
              :teleport-to="shellRootRef"
              @change="emit('update:audio', $event)"
            />
          </div>

          <div v-if="subtitleOptions.length > 1" class="space-y-1.5">
            <span class="text-label-medium block text-on-surface-variant">
              {{ t('player.subtitles') }}
            </span>
            <MdSelect
              :model-value="subtitleValue"
              :options="subtitleOptions"
              size="sm"
              :teleport-to="shellRootRef"
              @change="emit('update:subtitle', $event)"
            />
          </div>

          <div v-if="downloadUrl" class="border-t border-outline-variant pt-4">
            <MdButton
              variant="tonal"
              :icon="LuDownload"
              :href="downloadUrl"
              block
            >
              {{ t('common.download') }}
            </MdButton>
          </div>
        </div>
      </template>
    </ImmersiveMediaTemplate>
  </div>
</template>

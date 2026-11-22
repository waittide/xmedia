<script setup lang="ts">
import { computed, nextTick, onBeforeUnmount, ref, watch } from 'vue'
import { useRouter } from 'vue-router'
import {
  LuBookmark,
  LuChevronDown,
  LuDisc3,
  LuHeart,
  LuLanguages,
  LuListMusic,
  LuLoader2,
  LuMic2,
  LuPause,
  LuPlay,
  LuRepeat,
  LuRepeat1,
  LuShuffle,
  LuSkipBack,
  LuSkipForward,
  LuSlidersHorizontal,
  LuUser,
  LuVolume2,
  LuVolumeX,
  LuX,
} from 'vue-icons-plus/lu'
import {
  MdButton,
  MdIconButton,
  MdMenu,
  MdMenuItem,
  MdTabs,
  type MdTabItem,
} from '@/components/ui'
import MediaCoverImage from '@/components/ui/MediaCoverImage.vue'
import { useEscapeStack } from '@/composables/useEscapeStack'
import { useFocusTrap } from '@/composables/useFocusTrap'
import { useSnackbarStore } from '@/stores/snackbar'
import { stripAriaModalOnLeave } from '@/utils/dom'
import { formatTime } from '@/utils/format'
import { api, buildCoverUrl } from '../api'
import { useMusicI18n as useI18n } from '../i18n'
import { usePlayerStore } from '../player'
import { useMusicStore } from '../store'
import type { TranscodeProfile } from '../types'
import MusicSeekSlider from './MusicSeekSlider.vue'
import QueueListContent from './QueueListContent.vue'

type MobileStageTab = 'cover' | 'lyrics' | 'queue'

const { t } = useI18n()
const router = useRouter()
const snackbar = useSnackbarStore()
const playerStore = usePlayerStore()
const musicStore = useMusicStore()
const escapeStack = useEscapeStack()

const overlayRef = ref<HTMLElement | null>(null)
const lyricsContainerRef = ref<HTMLDivElement | null>(null)
const lineRefs = ref<(HTMLButtonElement | null)[]>([])

const scrubPreviewSecs = ref<number | null>(null)
const savingBookmark = ref(false)
const showQueuePanel = ref(false)
const showTranslation = ref(true)
const mobileTab = ref<MobileStageTab>('lyrics')

const autoScrollPaused = ref(false)
let autoScrollTimer: ReturnType<typeof setTimeout> | null = null

useFocusTrap(overlayRef, () => playerStore.lyricsOpen && Boolean(playerStore.currentTrack))

escapeStack.register(
  () => playerStore.lyricsOpen && Boolean(playerStore.currentTrack),
  () => {
    if (showQueuePanel.value) {
      closeQueueStage()
      return
    }
    playerStore.lyricsOpen = false
  },
)

function clearAutoScrollPause(): void {
  autoScrollPaused.value = false
  if (autoScrollTimer) {
    clearTimeout(autoScrollTimer)
    autoScrollTimer = null
  }
}

function pauseAutoScroll(): void {
  autoScrollPaused.value = true
  if (autoScrollTimer) clearTimeout(autoScrollTimer)
  autoScrollTimer = setTimeout(() => {
    autoScrollPaused.value = false
    autoScrollTimer = null
  }, 4000)
}

function setLineRef(el: unknown, idx: number): void {
  lineRefs.value[idx] = el instanceof HTMLButtonElement ? el : null
}

watch(
  () => playerStore.lyricsOpen,
  (isOpen) => {
    if (!isOpen) {
      showQueuePanel.value = false
      mobileTab.value = 'lyrics'
      scrubPreviewSecs.value = null
      clearAutoScrollPause()
    }
  },
)

watch(mobileTab, (tab) => {
  showQueuePanel.value = tab === 'queue'
})

watch(
  () => [playerStore.currentTrack?.id, playerStore.lyrics] as const,
  () => {
    lineRefs.value = []
    clearAutoScrollPause()
    if (lyricsContainerRef.value) {
      lyricsContainerRef.value.scrollTop = 0
    }
  },
)

const backdropCoverUrl = computed(() => {
  const tr = playerStore.currentTrack
  if (!tr) return null
  if (tr.kind !== 'local') {
    return tr.cover_url || null
  }
  if (!tr.cover_key || !tr.cover_id) return null
  return buildCoverUrl(tr.cover_id, 512)
})

const artworkCoverUrl = computed(() => {
  const tr = playerStore.currentTrack
  if (!tr) return null
  if (tr.kind !== 'local') {
    return tr.cover_url || null
  }
  if (!tr.cover_key || !tr.cover_id) return null
  return buildCoverUrl(tr.cover_id, 640)
})

const backdropStyle = computed<Record<string, string>>(() => {
  if (!backdropCoverUrl.value) return {} as Record<string, string>
  const escaped = backdropCoverUrl.value.replace(/["\\]/g, '\\$&')
  return { backgroundImage: `url("${escaped}")` }
})

const isCurrentStarred = computed(() => {
  if (playerStore.currentLocalTrack) {
    return Boolean(playerStore.currentLocalTrack.starred_at)
  }
  if (playerStore.currentTrack) {
    return playerStore.currentOnlineFavorite
  }
  return false
})

const displayTimeSecs = computed(() => scrubPreviewSecs.value ?? playerStore.currentTimeSecs)

const playModeTitle = computed(() => {
  if (playerStore.playMode === 'loop_one') return t('player.modeLoopOne')
  if (playerStore.playMode === 'shuffle') return t('player.modeShuffle')
  return t('player.modeSequence')
})

const qualityOptions = computed(() => [
  {
    value: 'raw' as TranscodeProfile,
    label: t('player.qualityRaw'),
    trailing:
      playerStore.currentLocalTrack && playerStore.transcodeProfile === 'raw'
        ? playerStore.qualitySummary
        : '',
  },
  { value: 'opus128' as TranscodeProfile, label: t('player.qualityOpus128'), trailing: '' },
  { value: 'mp3192' as TranscodeProfile, label: t('player.qualityMp3192'), trailing: '' },
  { value: 'mp3320' as TranscodeProfile, label: t('player.qualityMp3320'), trailing: '' },
  { value: 'aac256' as TranscodeProfile, label: t('player.qualityAac256'), trailing: '' },
])

const onlineQualityOptions = computed(() => {
  const tr = playerStore.currentTrack
  if (!tr || tr.kind !== 'online') return []
  return tr.qualities.map((q) => ({
    value: q,
    label: /^\d+$/.test(q) ? `${q}k` : q.toUpperCase(),
  }))
})

const hasQualityMenuItems = computed(() => {
  if (playerStore.currentLocalTrack) return true
  return playerStore.isCurrentOnline && onlineQualityOptions.value.length > 0
})

const mobileStageTabs = computed<MdTabItem<MobileStageTab>[]>(() => [
  { value: 'cover', label: t('player.tabCover') },
  { value: 'lyrics', label: t('player.tabLyrics') },
  {
    value: 'queue',
    label: t('player.tabQueue'),
    badge: playerStore.queue.length > 0 ? playerStore.queue.length : undefined,
  },
])

const hasTranslatedLyrics = computed(() => {
  const tl = playerStore.translatedLyrics
  return Boolean(tl && (tl.lines.length > 0 || tl.plain_text.trim().length > 0))
})

const matchedTranslations = computed<Array<string | null>>(() => {
  const primary = playerStore.lyrics
  const translated = playerStore.translatedLyrics
  if (!primary?.synced || !translated?.lines.length) {
    return []
  }
  const tLines = translated.lines
  return primary.lines.map((line) => {
    let best: string | null = null
    let bestDiff = 450
    for (const candidate of tLines) {
      const diff = Math.abs(candidate.start_ms - line.start_ms)
      if (diff <= bestDiff && candidate.text.trim()) {
        bestDiff = diff
        best = candidate.text.trim()
      }
    }
    return best
  })
})

async function scrollToActiveLyric(behavior: ScrollBehavior = 'smooth'): Promise<void> {
  const activeIdx = playerStore.activeLyricIndex
  if (!playerStore.lyricsOpen || activeIdx < 0 || autoScrollPaused.value) return
  if (showQueuePanel.value) return
  await nextTick()
  const container = lyricsContainerRef.value
  const targetEl = lineRefs.value[activeIdx]
  if (!container || !targetEl) return
  const containerRect = container.getBoundingClientRect()
  const targetRect = targetEl.getBoundingClientRect()
  const nextScrollTop =
    container.scrollTop +
    (targetRect.top - containerRect.top) -
    container.clientHeight / 2 +
    targetRect.height / 2
  container.scrollTo({
    top: Math.max(0, nextScrollTop),
    behavior,
  })
}

watch(
  () =>
    [
      playerStore.activeLyricIndex,
      playerStore.lyricsOpen,
      playerStore.lyrics,
      showQueuePanel.value,
      mobileTab.value,
    ] as const,
  ([activeIdx, isOpen]) => {
    if (!isOpen || activeIdx < 0) return
    void scrollToActiveLyric('smooth')
  },
)

function handleLyricLineClick(startMs: number): void {
  clearAutoScrollPause()
  playerStore.seekTo(startMs / 1000)
}

async function handleToggleFavorite(): Promise<void> {
  if (playerStore.currentLocalTrack) {
    await musicStore.toggleTrackStar(playerStore.currentLocalTrack)
    return
  }
  if (playerStore.currentTrack) {
    await playerStore.toggleCurrentOnlineFavorite()
  }
}

async function saveCurrentBookmark(): Promise<void> {
  const tr = playerStore.currentLocalTrack
  if (!tr || savingBookmark.value) return
  savingBookmark.value = true
  try {
    await api.saveBookmark(tr.id, Math.round(playerStore.currentTimeSecs * 1000))
    snackbar.show({ message: t('player.bookmarkSaved') })
  } catch (err) {
    snackbar.show({
      message: err instanceof Error ? err.message : t('player.playbackError'),
      tone: 'error',
    })
  } finally {
    savingBookmark.value = false
  }
}

function closeQueueStage(): void {
  showQueuePanel.value = false
  mobileTab.value = 'lyrics'
}

function toggleQueueStage(): void {
  if (showQueuePanel.value) {
    closeQueueStage()
  } else {
    showQueuePanel.value = true
    mobileTab.value = 'queue'
  }
}

function navigateAndClose(path: string): void {
  playerStore.lyricsOpen = false
  void router.push(path)
}

onBeforeUnmount(() => {
  clearAutoScrollPause()
})
</script>

<template>
  <Teleport to="body">
    <Transition name="md-fade" @before-leave="stripAriaModalOnLeave">
      <div
        v-if="playerStore.lyricsOpen && playerStore.currentTrack"
        ref="overlayRef"
        role="dialog"
        :aria-modal="playerStore.lyricsOpen && playerStore.currentTrack ? 'true' : undefined"
        data-player-shortcuts
        tabindex="-1"
        :aria-label="t('player.lyricsTitle')"
        class="fixed inset-0 z-[var(--z-fullscreen)] flex h-dvh w-screen flex-col overflow-hidden bg-surface text-on-surface outline-none"
      >
        <!-- Atmospheric Blurred Artwork Backdrop -->
        <div
          v-if="backdropCoverUrl"
          class="pointer-events-none absolute inset-0 scale-125 bg-cover bg-center opacity-20 blur-3xl"
          :style="backdropStyle"
        />
        <div
          class="pointer-events-none absolute inset-0 bg-gradient-to-b from-surface/70 via-surface/85 to-surface/96"
        />

        <!-- Minimalist Top App Bar -->
        <header
          class="safe-pt safe-px relative z-10 flex shrink-0 items-center justify-between gap-3 px-4 py-3 sm:px-8 sm:py-4"
        >
          <div class="flex min-w-0 items-center gap-2">
            <MdButton
              variant="tonal"
              size="sm"
              :icon="LuChevronDown"
              @click="playerStore.lyricsOpen = false"
            >
              {{ t('player.closeLyrics') }}
            </MdButton>
            <span
              v-if="playerStore.qualitySummary"
              class="text-label-small hidden font-mono text-on-surface-variant sm:inline"
            >
              {{ playerStore.qualitySummary }}
            </span>
          </div>

          <div class="flex shrink-0 items-center gap-1">
            <MdIconButton
              v-if="hasTranslatedLyrics"
              size="sm"
              :variant="showTranslation ? 'tonal' : 'standard'"
              :selected="showTranslation"
              :icon="LuLanguages"
              :aria-label="t('player.translationToggle')"
              :title="t('player.translationToggle')"
              @click="showTranslation = !showTranslation"
            />

            <MdIconButton
              size="sm"
              class="hidden md:inline-flex"
              :variant="showQueuePanel ? 'tonal' : 'standard'"
              :selected="showQueuePanel"
              :icon="LuListMusic"
              :aria-label="t('player.queueTitle')"
              :title="t('player.queueTitle')"
              @click="toggleQueueStage"
            />

            <MdMenu
              v-if="hasQualityMenuItems || playerStore.currentLocalTrack"
              placement="bottom-end"
              :min-width="220"
              :aria-label="t('player.optionsMenu')"
            >
              <template #trigger="{ toggle, open, triggerAttrs }">
                <MdIconButton
                  v-bind="triggerAttrs"
                  size="sm"
                  :variant="open ? 'tonal' : 'standard'"
                  :icon="LuSlidersHorizontal"
                  :aria-label="t('player.optionsMenu')"
                  :title="t('player.optionsMenu')"
                  @click="toggle"
                />
              </template>

              <template v-if="hasQualityMenuItems">
                <div
                  class="text-label-small flex items-center justify-between px-3 py-1.5 text-on-surface-variant"
                >
                  <span>{{ t('player.qualityHeading') }}</span>
                  <span v-if="playerStore.qualitySummary" class="font-mono">
                    {{ playerStore.qualitySummary }}
                  </span>
                </div>

                <template v-if="playerStore.currentLocalTrack">
                  <MdMenuItem
                    v-for="opt in qualityOptions"
                    :key="opt.value"
                    :label="opt.label"
                    :trailing-text="opt.trailing"
                    :selected="playerStore.transcodeProfile === opt.value"
                    @select="playerStore.setTranscodeProfile(opt.value)"
                  />
                </template>

                <template v-else-if="playerStore.isCurrentOnline">
                  <MdMenuItem
                    v-for="opt in onlineQualityOptions"
                    :key="opt.value"
                    :label="opt.label"
                    :selected="
                      (playerStore.currentOnlineItem?.selectedQuality ??
                        onlineQualityOptions[0]?.value) === opt.value
                    "
                    @select="playerStore.setOnlineQuality(opt.value)"
                  />
                </template>
              </template>

              <template v-if="playerStore.currentLocalTrack">
                <div class="my-1 border-t border-outline-variant" role="separator" />
                <MdMenuItem
                  :icon="LuBookmark"
                  :label="t('player.bookmarkBtn')"
                  :disabled="savingBookmark"
                  @select="saveCurrentBookmark"
                />
                <MdMenuItem
                  :icon="LuUser"
                  :label="t('common.viewArtist')"
                  @select="
                    navigateAndClose(
                      `/music/library?artist=${playerStore.currentLocalTrack.artist_id}`,
                    )
                  "
                />
                <MdMenuItem
                  :icon="LuDisc3"
                  :label="t('common.viewAlbum')"
                  @select="navigateAndClose(`/music/item/${playerStore.currentLocalTrack.album_id}`)"
                />
              </template>
            </MdMenu>
          </div>
        </header>

        <!-- Playback Error Banner (Retry MUST remain the first <button> inside [role="alert"]) -->
        <div
          v-if="playerStore.playbackError"
          role="alert"
          class="relative z-10 mx-4 mb-2 flex items-center justify-between gap-2 rounded-corner-md bg-error-container px-4 py-2 text-body-small text-on-error-container sm:mx-8 md:mx-auto md:w-full md:max-w-5xl"
        >
          <span class="min-w-0 flex-1 truncate" :title="playerStore.playbackError">
            {{ playerStore.playbackError }}
          </span>
          <div class="flex shrink-0 items-center gap-1">
            <MdButton variant="tonal" size="sm" @click="playerStore.retryCurrentPlayback()">
              {{ t('player.retryPlayback') }}
            </MdButton>
            <MdIconButton
              size="sm"
              :icon="LuX"
              :aria-label="t('player.dismissError')"
              :title="t('player.dismissError')"
              @click="playerStore.dismissPlaybackError()"
            />
          </div>
        </div>

        <!-- Compact (< 768px) Stage Tabs: Cover / Lyrics / Queue -->
        <div class="relative z-10 px-4 md:hidden">
          <MdTabs
            v-model="mobileTab"
            :tabs="mobileStageTabs"
            variant="secondary"
            :scrollable="false"
            :aria-label="t('player.lyricsTitle')"
          />
        </div>

        <!-- Main Immersive Split Stage -->
        <div
          class="safe-px safe-pb relative z-10 mx-auto grid min-h-0 w-full max-w-6xl flex-1 grid-cols-1 gap-6 overflow-hidden px-6 pb-6 sm:px-10 md:grid-cols-12 md:gap-10 md:pb-8"
        >
          <!-- Left Column: Artwork & Full Controls -->
          <div
            class="min-h-0 flex-col items-center justify-center overflow-y-auto py-4 md:col-span-5 md:flex"
            :class="mobileTab === 'cover' ? 'flex' : 'hidden'"
          >
            <div class="w-full max-w-xs sm:max-w-sm">
              <MediaCoverImage
                :src="artworkCoverUrl"
                :alt="playerStore.currentTrack.title"
                loading="eager"
                rounded="rounded-corner-xl"
                :fallback-icon="LuDisc3"
                class="w-full shadow-elevation-3"
              />

              <div class="mt-6 flex items-start justify-between gap-3">
                <div class="min-w-0 flex-1">
                  <h1 class="text-headline-small truncate text-on-surface">
                    {{ playerStore.currentTrack.title }}
                  </h1>
                  <div
                    class="text-body-medium mt-1 flex flex-wrap items-center gap-1.5 text-on-surface-variant"
                  >
                    <template v-if="playerStore.currentLocalTrack">
                      <button
                        type="button"
                        class="cursor-pointer transition-colors hover:text-primary hover:underline"
                        @click="
                          navigateAndClose(
                            `/music/library?artist=${playerStore.currentLocalTrack.artist_id}`,
                          )
                        "
                      >
                        {{ playerStore.currentLocalTrack.artist_name }}
                      </button>
                      <span aria-hidden="true">·</span>
                      <button
                        type="button"
                        class="cursor-pointer truncate transition-colors hover:text-primary hover:underline"
                        @click="
                          navigateAndClose(`/music/item/${playerStore.currentLocalTrack.album_id}`)
                        "
                      >
                        {{ playerStore.currentLocalTrack.album_title }}
                      </button>
                    </template>
                    <template v-else>
                      <span>{{ playerStore.currentTrack.artist_name }}</span>
                      <template v-if="playerStore.currentTrack.album_title">
                        <span aria-hidden="true">·</span>
                        <span class="truncate">{{ playerStore.currentTrack.album_title }}</span>
                      </template>
                    </template>
                  </div>
                </div>

                <MdIconButton
                  :selected="isCurrentStarred"
                  :aria-label="isCurrentStarred ? t('common.unfavorite') : t('common.favorite')"
                  :title="isCurrentStarred ? t('common.unfavorite') : t('common.favorite')"
                  @click="handleToggleFavorite"
                >
                  <LuHeart
                    class="h-5 w-5"
                    :class="isCurrentStarred ? 'fill-error text-error' : 'text-on-surface-variant'"
                  />
                </MdIconButton>
              </div>

              <!-- Real-time Seek Slider -->
              <div class="mt-5">
                <MusicSeekSlider
                  :model-value="playerStore.currentTimeSecs"
                  :min="0"
                  :max="Math.max(1, playerStore.durationSecs)"
                  :step="0.2"
                  :buffered="playerStore.bufferedPercent"
                  always-show-thumb
                  aria-label="Seek"
                  :aria-value-text="`${formatTime(displayTimeSecs)} / ${formatTime(playerStore.durationSecs)}`"
                  @preview="scrubPreviewSecs = $event"
                  @commit="playerStore.seekTo($event)"
                />
                <div class="text-label-small mt-1 flex justify-between font-mono tabular-nums">
                  <span
                    :class="scrubPreviewSecs !== null ? 'text-primary' : 'text-on-surface-variant'"
                  >
                    {{ formatTime(displayTimeSecs) }}
                  </span>
                  <span class="text-on-surface-variant">
                    {{ formatTime(playerStore.durationSecs) }}
                  </span>
                </div>
              </div>

              <!-- Transport Controls -->
              <div class="mt-4 flex items-center justify-center gap-4">
                <MdIconButton
                  :variant="playerStore.playMode !== 'sequence' ? 'tonal' : 'standard'"
                  :selected="playerStore.playMode !== 'sequence'"
                  :aria-label="playModeTitle"
                  :title="playModeTitle"
                  @click="playerStore.cyclePlayMode()"
                >
                  <LuRepeat1 v-if="playerStore.playMode === 'loop_one'" class="h-5 w-5" />
                  <LuShuffle v-else-if="playerStore.playMode === 'shuffle'" class="h-5 w-5" />
                  <LuRepeat v-else class="h-5 w-5" />
                </MdIconButton>

                <MdIconButton
                  :aria-label="t('player.prev')"
                  :title="t('player.prev')"
                  @click="playerStore.playPrev()"
                >
                  <LuSkipBack class="h-6 w-6 fill-current text-on-surface" />
                </MdIconButton>

                <button
                  type="button"
                  :disabled="!playerStore.currentTrack || playerStore.onlineResolving"
                  :aria-busy="playerStore.onlineResolving ? 'true' : undefined"
                  class="md-state flex h-14 w-14 cursor-pointer items-center justify-center rounded-corner-full bg-primary text-on-primary shadow-elevation-2 transition-transform duration-150 ease-standard hover:scale-105 disabled:cursor-not-allowed disabled:opacity-55"
                  :title="
                    playerStore.onlineResolving
                      ? t('player.resolvingStream')
                      : playerStore.isPlaying
                        ? t('player.pause')
                        : t('player.play')
                  "
                  :aria-label="
                    playerStore.onlineResolving
                      ? t('player.resolvingStream')
                      : playerStore.isPlaying
                        ? t('player.pause')
                        : t('player.play')
                  "
                  @click="playerStore.togglePlay()"
                >
                  <LuLoader2 v-if="playerStore.onlineResolving" class="h-6 w-6 animate-spin" />
                  <LuPause v-else-if="playerStore.isPlaying" class="h-6 w-6 fill-current" />
                  <LuPlay v-else class="ml-0.5 h-6 w-6 fill-current" />
                </button>

                <MdIconButton
                  :aria-label="t('player.next')"
                  :title="t('player.next')"
                  @click="playerStore.playNext()"
                >
                  <LuSkipForward class="h-6 w-6 fill-current text-on-surface" />
                </MdIconButton>

                <MdIconButton
                  class="md:hidden"
                  :icon="LuListMusic"
                  :aria-label="t('player.queueTitle')"
                  :title="t('player.queueTitle')"
                  @click="mobileTab = 'queue'"
                />
              </div>

              <!-- Volume Control -->
              <div class="mt-4 flex items-center gap-2">
                <MdIconButton
                  size="sm"
                  :icon="playerStore.muted || playerStore.volume === 0 ? LuVolumeX : LuVolume2"
                  :aria-label="playerStore.muted ? t('player.unmute') : t('player.mute')"
                  :title="playerStore.muted ? t('player.unmute') : t('player.mute')"
                  @click="playerStore.toggleMute()"
                />
                <div class="min-w-0 flex-1">
                  <MusicSeekSlider
                    :model-value="playerStore.muted ? 0 : playerStore.volume"
                    :min="0"
                    :max="1"
                    :step="0.02"
                    :commit-on-release="false"
                    :aria-label="t('player.volume')"
                    @update:model-value="playerStore.setVolume($event)"
                  />
                </div>
              </div>
            </div>
          </div>

          <!-- Right Column: Borderless Immersive Lyrics OR In-Stage Queue Panel -->
          <div
            class="min-h-0 flex-col md:col-span-7 md:flex"
            :class="mobileTab !== 'cover' ? 'flex' : 'hidden'"
          >
            <!-- In-Stage Queue Panel -->
            <template v-if="showQueuePanel">
              <div class="flex items-center justify-between py-3">
                <div class="flex items-center gap-2">
                  <LuListMusic class="h-4 w-4 text-primary" aria-hidden="true" />
                  <span class="text-title-medium text-on-surface">
                    {{ t('player.queueTitle') }}
                  </span>
                  <span class="text-label-medium text-on-surface-variant">
                    {{ t('common.tracksCount', { count: playerStore.queue.length }) }}
                  </span>
                </div>
                <MdButton
                  variant="text"
                  size="sm"
                  class="hidden md:inline-flex"
                  :icon="LuMic2"
                  @click="closeQueueStage"
                >
                  {{ t('player.lyricsTitle') }}
                </MdButton>
              </div>
              <div class="min-h-0 flex-1 overflow-y-auto pb-4">
                <QueueListContent :active="showQueuePanel" compact />
              </div>
            </template>

            <!-- Borderless Full-Canvas Lyrics Stage -->
            <template v-else>
              <div
                ref="lyricsContainerRef"
                class="lyrics-feather min-h-0 flex-1 overflow-y-auto px-1 py-12 sm:px-4"
                @wheel.passive="pauseAutoScroll"
                @touchstart.passive="pauseAutoScroll"
              >
                <div
                  v-if="playerStore.lyricsLoading"
                  class="text-body-medium flex h-full items-center justify-center text-on-surface-variant"
                >
                  {{ t('common.loading') }}
                </div>

                <!-- Synced Interactive Lyrics (MUST stay <button> for T1-F15-03) -->
                <div
                  v-else-if="
                    playerStore.lyrics &&
                    playerStore.lyrics.synced &&
                    playerStore.lyrics.lines.length > 0
                  "
                  class="space-y-4 py-16"
                >
                  <button
                    v-for="(line, idx) in playerStore.lyrics.lines"
                    :key="`${line.start_ms}-${idx}`"
                    :ref="(el) => setLineRef(el, idx)"
                    type="button"
                    class="block w-full origin-left cursor-pointer rounded-corner-md px-3 py-2 text-left transition-all duration-200 ease-standard hover:bg-on-surface/5 focus-visible:outline-2 focus-visible:outline-secondary"
                    :class="
                      idx === playerStore.activeLyricIndex
                        ? 'scale-[1.03] text-headline-small text-on-surface sm:text-headline-medium'
                        : 'text-title-medium text-on-surface-variant/60 hover:text-on-surface sm:text-title-large'
                    "
                    @click="handleLyricLineClick(line.start_ms)"
                  >
                    <span class="block leading-snug">{{ line.text || '♪' }}</span>
                    <span
                      v-if="showTranslation && matchedTranslations[idx]"
                      class="mt-1 block text-body-medium leading-snug"
                      :class="
                        idx === playerStore.activeLyricIndex
                          ? 'text-on-surface-variant'
                          : 'text-on-surface-variant/55'
                      "
                    >
                      {{ matchedTranslations[idx] }}
                    </span>
                  </button>
                </div>

                <!-- Unsynced Plain Text Lyrics -->
                <div
                  v-else-if="playerStore.lyrics && playerStore.lyrics.plain_text"
                  class="text-body-large space-y-6 py-8 leading-loose whitespace-pre-line text-on-surface/90"
                >
                  <p>{{ playerStore.lyrics.plain_text }}</p>
                  <p
                    v-if="showTranslation && playerStore.translatedLyrics?.plain_text"
                    class="text-body-medium border-t border-outline-variant pt-6 text-on-surface-variant"
                  >
                    {{ playerStore.translatedLyrics.plain_text }}
                  </p>
                </div>

                <!-- Empty Lyrics State -->
                <div
                  v-else
                  class="text-body-medium flex h-full flex-col items-center justify-center gap-2 text-center text-on-surface-variant"
                >
                  <LuMic2 class="h-8 w-8 opacity-40" aria-hidden="true" />
                  <p>{{ t('player.noLyrics') }}</p>
                </div>
              </div>
            </template>

            <!-- Mobile Compact Bottom Transport Strip when viewing Lyrics / Queue -->
            <div
              class="mt-2 flex shrink-0 items-center justify-between gap-3 border-t border-outline-variant pt-3 md:hidden"
            >
              <div class="min-w-0 flex-1">
                <p class="text-title-small truncate text-on-surface">
                  {{ playerStore.currentTrack.title }}
                </p>
                <p class="text-body-small truncate text-on-surface-variant">
                  {{ playerStore.currentTrack.artist_name }}
                </p>
              </div>
              <div class="flex shrink-0 items-center gap-1">
                <MdIconButton
                  size="sm"
                  :aria-label="t('player.prev')"
                  @click="playerStore.playPrev()"
                >
                  <LuSkipBack class="h-4 w-4 fill-current text-on-surface" />
                </MdIconButton>
                <MdIconButton
                  variant="filled"
                  :loading="playerStore.onlineResolving"
                  :disabled="!playerStore.currentTrack || playerStore.onlineResolving"
                  :aria-label="playerStore.isPlaying ? t('player.pause') : t('player.play')"
                  @click="playerStore.togglePlay()"
                >
                  <LuPause v-if="playerStore.isPlaying" class="h-4 w-4 fill-current" />
                  <LuPlay v-else class="ml-0.5 h-4 w-4 fill-current" />
                </MdIconButton>
                <MdIconButton
                  size="sm"
                  :aria-label="t('player.next')"
                  @click="playerStore.playNext()"
                >
                  <LuSkipForward class="h-4 w-4 fill-current text-on-surface" />
                </MdIconButton>
              </div>
            </div>
          </div>
        </div>
      </div>
    </Transition>
  </Teleport>
</template>

<style scoped>
.lyrics-feather {
  mask-image: linear-gradient(
    to bottom,
    transparent 0%,
    #000 10%,
    #000 90%,
    transparent 100%
  );
  -webkit-mask-image: linear-gradient(
    to bottom,
    transparent 0%,
    #000 10%,
    #000 90%,
    transparent 100%
  );
}
</style>

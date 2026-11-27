<script setup lang="ts">
import { computed, ref, watch } from 'vue'
import { useRouter } from 'vue-router'
import {
  LuCheck,
  LuChevronDown,
  LuDisc3,
  LuHeart,
  LuLanguages,
  LuLibrary,
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
  LuVolume2,
  LuVolumeX,
  LuX,
} from 'vue-icons-plus/lu'
import {
  AppSlider,
  MdButton,
  MdIconButton,
  MdMenu,
  MdTabs,
  type MdTabItem,
} from '@/components/ui'
import MediaCoverImage from '@/components/ui/MediaCoverImage.vue'
import { useEscapeStack } from '@/composables/useEscapeStack'
import { useFocusTrap } from '@/composables/useFocusTrap'
import { stripAriaModalOnLeave } from '@/utils/dom'
import { formatTime } from '@/utils/format'
import { buildCoverUrl } from '../api'
import { usePlayerTransportControls } from '../composables/usePlayerTransportControls'
import { useMusicI18n as useI18n } from '../i18n'
import LyricsStageCanvas from './LyricsStageCanvas.vue'
import PlayerOptionsMenuContent from './PlayerOptionsMenuContent.vue'
import QueueListContent from './QueueListContent.vue'

type MobileStageTab = 'cover' | 'lyrics' | 'queue'

const { t } = useI18n()
const router = useRouter()
const escapeStack = useEscapeStack()

const {
  playerStore,
  scrubPreviewSecs,
  savingBookmark,
  downloadingCurrent,
  displayTimeSecs,
  isCurrentStarred,
  playModeTitle,
  qualityOptions,
  onlineQualityOptions,
  hasQualityMenuItems,
  handleToggleFavorite,
  saveCurrentBookmark,
  handleDownloadCurrent,
} = usePlayerTransportControls()

const overlayRef = ref<HTMLElement | null>(null)
const showQueuePanel = ref(false)
const showTranslation = ref(true)
const mobileTab = ref<MobileStageTab>('lyrics')

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

watch(
  () => playerStore.lyricsOpen,
  (isOpen) => {
    if (!isOpen) {
      showQueuePanel.value = false
      mobileTab.value = 'lyrics'
      scrubPreviewSecs.value = null
    }
  },
)

watch(mobileTab, (tab) => {
  showQueuePanel.value = tab === 'queue'
})

const backdropCoverUrl = computed(() => {
  const tr = playerStore.currentTrack
  if (!tr) return null
  if (tr.kind !== 'local') return tr.cover_url || null
  return tr.cover_key && tr.cover_id ? buildCoverUrl(tr.cover_id, 512) : null
})

const artworkCoverUrl = computed(() => {
  const tr = playerStore.currentTrack
  if (!tr) return null
  if (tr.kind !== 'local') return tr.cover_url || null
  return tr.cover_key && tr.cover_id ? buildCoverUrl(tr.cover_id, 640) : null
})

const backdropStyle = computed(() => {
  if (!backdropCoverUrl.value) return undefined
  const escaped = backdropCoverUrl.value.replace(/["\\]/g, '\\$&')
  return { backgroundImage: `url("${escaped}")` }
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
        <div
          v-if="backdropCoverUrl"
          class="pointer-events-none absolute inset-0 scale-125 bg-cover bg-center opacity-20 blur-3xl"
          :style="backdropStyle"
        />
        <div
          class="pointer-events-none absolute inset-0 bg-gradient-to-b from-surface/70 via-surface/85 to-surface/96"
        />

        <header
          class="relative z-10 flex shrink-0 items-center justify-between gap-3 pt-[calc(var(--safe-area-top)+0.75rem)] pb-3 pl-[calc(var(--safe-area-left)+1rem)] pr-[calc(var(--safe-area-right)+1rem)] sm:pt-[calc(var(--safe-area-top)+1rem)] sm:pb-4 sm:pl-[calc(var(--safe-area-left)+2rem)] sm:pr-[calc(var(--safe-area-right)+2rem)]"
        >
          <div class="flex min-w-0 items-center gap-2">
            <MdButton
              variant="tonal"
              :icon="LuChevronDown"
              @click="playerStore.lyricsOpen = false"
            >
              {{ t('player.closeLyricsBtn') }}
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
              v-if="playerStore.currentTrack"
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

              <PlayerOptionsMenuContent
                :quality-options="qualityOptions"
                :online-quality-options="onlineQualityOptions"
                :has-quality-menu-items="hasQualityMenuItems"
                :saving-bookmark="savingBookmark"
                :downloading-current="downloadingCurrent"
                show-entity-links
                @download="handleDownloadCurrent"
                @bookmark="saveCurrentBookmark"
                @navigate="navigateAndClose"
              />
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
            <MdButton variant="tonal" @click="playerStore.retryCurrentPlayback()">
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

        <div class="relative z-10 px-4 md:hidden">
          <MdTabs
            v-model="mobileTab"
            :tabs="mobileStageTabs"
            variant="secondary"
            :scrollable="false"
            :aria-label="t('player.lyricsTitle')"
          />
        </div>

        <div
          class="relative z-10 mx-auto grid min-h-0 w-full max-w-6xl flex-1 grid-cols-1 gap-6 overflow-hidden pb-[calc(var(--safe-area-bottom)+1.5rem)] pl-[calc(var(--safe-area-left)+1.5rem)] pr-[calc(var(--safe-area-right)+1.5rem)] sm:pl-[calc(var(--safe-area-left)+2.5rem)] sm:pr-[calc(var(--safe-area-right)+2.5rem)] md:grid-cols-12 md:gap-10 md:pb-[calc(var(--safe-area-bottom)+2rem)]"
        >
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
                  v-if="playerStore.currentLocalTrack"
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
                <MdIconButton
                  v-else-if="playerStore.isCurrentOnline"
                  :selected="playerStore.isCurrentOnlineTracked"
                  :loading="playerStore.trackingCurrentOnline"
                  :aria-label="
                    playerStore.isCurrentOnlineTracked
                      ? t('online.trackedInLibrary')
                      : t('online.trackToLibrary')
                  "
                  :tooltip="
                    playerStore.isCurrentOnlineTracked
                      ? t('online.trackedInLibrary')
                      : t('online.trackToLibrary')
                  "
                  @click="playerStore.trackCurrentOnlineToLibrary()"
                >
                  <LuCheck v-if="playerStore.isCurrentOnlineTracked" class="h-5 w-5 text-primary" />
                  <LuLibrary v-else class="h-5 w-5 text-on-surface-variant" />
                </MdIconButton>
              </div>

              <div class="mt-5">
                <AppSlider
                  :model-value="playerStore.currentTimeSecs"
                  :min="0"
                  :max="Math.max(1, playerStore.durationSecs)"
                  :step="0.2"
                  :buffered="playerStore.bufferedPercent"
                  commit-on-release
                  always-show-thumb
                  :aria-label="t('player.playbackProgress')"
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
                  class="md-state flex h-14 w-14 cursor-pointer items-center justify-center rounded-corner-full bg-primary text-on-primary shadow-elevation-2 transition-colors duration-150 ease-standard disabled:cursor-not-allowed disabled:bg-on-surface/12 disabled:text-on-surface/38 disabled:shadow-none"
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

              <div class="mt-4 flex items-center gap-2">
                <MdIconButton
                  size="sm"
                  :icon="playerStore.muted || playerStore.volume === 0 ? LuVolumeX : LuVolume2"
                  :aria-label="playerStore.muted ? t('player.unmute') : t('player.mute')"
                  :title="playerStore.muted ? t('player.unmute') : t('player.mute')"
                  @click="playerStore.toggleMute()"
                />
                <div class="min-w-0 flex-1">
                  <AppSlider
                    :model-value="playerStore.muted ? 0 : playerStore.volume"
                    :min="0"
                    :max="1"
                    :step="0.02"
                    :commit-on-release="false"
                    :always-show-thumb="false"
                    :aria-label="t('player.volume')"
                    @update:model-value="playerStore.setVolume($event)"
                  />
                </div>
              </div>
            </div>
          </div>

          <div
            class="min-h-0 flex-col md:col-span-7 md:flex"
            :class="mobileTab !== 'cover' ? 'flex' : 'hidden'"
          >
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

            <LyricsStageCanvas
              v-else
              :show-translation="showTranslation"
              :active="playerStore.lyricsOpen && !showQueuePanel"
            />

            <div
              class="mt-2 flex shrink-0 items-center justify-between gap-3 rounded-corner-lg bg-surface-container-low/80 px-3.5 py-2.5 md:hidden"
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

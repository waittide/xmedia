<script setup lang="ts">
import { computed, onBeforeUnmount, onMounted, ref } from 'vue'
import { useRoute, useRouter } from 'vue-router'
import {
  LuBookmark,
  LuChevronUp,
  LuDisc3,
  LuHeart,
  LuListMusic,
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
import { MdBadge, MdButton, MdIconButton, MdMenu, MdMenuItem } from '@/components/ui'
import MediaCoverImage from '@/components/ui/MediaCoverImage.vue'
import { useDialogStore } from '@/stores/dialog'
import { useSnackbarStore } from '@/stores/snackbar'
import { formatTime } from '@/utils/format'
import { api, buildCoverUrl } from '../api'
import { useMusicI18n as useI18n } from '../i18n'
import { usePlayerStore } from '../player'
import { useMusicStore } from '../store'
import type { TranscodeProfile } from '../types'
import MusicSeekSlider from './MusicSeekSlider.vue'

const { t } = useI18n()
const route = useRoute()
const router = useRouter()
const dialogStore = useDialogStore()
const snackbar = useSnackbarStore()
const playerStore = usePlayerStore()
const musicStore = useMusicStore()

const audioHostRef = ref<HTMLDivElement | null>(null)
const scrubPreviewSecs = ref<number | null>(null)
const savingBookmark = ref(false)

const EXCLUDED_SHORTCUT_ROUTES = new Set([
  'book-reader',
  'book-online-reader',
  'book-play',
  'comic-reader',
  'comic-online-reader',
  'comic-play',
  'video-player',
  'video-online-player',
  'video-play',
  'login',
  'setup',
])

const SEEK_STEP_SECS = 5

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

const currentCoverSrc = computed(() => {
  const tr = playerStore.currentTrack
  if (!tr) return null
  if (tr.kind === 'local') {
    return tr.cover_key && tr.cover_id ? buildCoverUrl(tr.cover_id, 160) : null
  }
  return tr.cover_url
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

const progressPercent = computed(() => {
  const dur = playerStore.durationSecs
  if (dur <= 0) return 0
  return Math.max(0, Math.min(100, (displayTimeSecs.value / dur) * 100))
})

const playModeTitle = computed(() => {
  if (playerStore.playMode === 'loop_one') return t('player.modeLoopOne')
  if (playerStore.playMode === 'shuffle') return t('player.modeShuffle')
  return t('player.modeSequence')
})

const hasQualityMenuItems = computed(() => {
  if (playerStore.currentLocalTrack) return true
  return playerStore.isCurrentOnline && onlineQualityOptions.value.length > 0
})

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

function isExcludedPlaybackRoute(): boolean {
  if (route.matched.some((record) => Boolean(record.meta.fullscreen))) return true
  const routeName = typeof route.name === 'string' ? route.name : ''
  if (EXCLUDED_SHORTCUT_ROUTES.has(routeName)) return true
  const p = route.path
  return (
    p.startsWith('/book/play') ||
    p.startsWith('/book/read') ||
    p.startsWith('/book/online/read') ||
    p.startsWith('/comic/play') ||
    p.startsWith('/comic/read') ||
    p.startsWith('/comic/online/read') ||
    p.startsWith('/video/play') ||
    p.startsWith('/video/watch') ||
    p.startsWith('/video/online/watch')
  )
}

function isEditableElement(el: HTMLElement | null): boolean {
  if (!el) return false
  const tag = el.tagName
  if (tag === 'INPUT' || tag === 'TEXTAREA' || tag === 'SELECT') return true
  if (el.isContentEditable) return true
  return Boolean(el.closest('[contenteditable="true"]'))
}

function handlePlaybackKeydown(ev: KeyboardEvent): void {
  if (!playerStore.currentTrack) return
  if (ev.ctrlKey || ev.metaKey || ev.altKey || ev.isComposing) return
  if (dialogStore.active !== null) return
  // Modal layers block playback shortcuts, except player-owned overlays (e.g. full-screen lyrics)
  // that opt in via `data-player-shortcuts`.
  if (
    typeof document !== 'undefined' &&
    document.querySelector('[aria-modal="true"]:not([data-player-shortcuts])')
  ) {
    return
  }
  if (isExcludedPlaybackRoute()) return

  const isSpace = ev.code === 'Space' || ev.key === ' '
  const isArrowLeft = ev.key === 'ArrowLeft'
  const isArrowRight = ev.key === 'ArrowRight'
  if (!isSpace && !isArrowLeft && !isArrowRight) return

  const target =
    (ev.target instanceof HTMLElement ? ev.target : null) ??
    (document.activeElement instanceof HTMLElement ? document.activeElement : null)
  if (isEditableElement(target)) return

  const role = target?.getAttribute('role') ?? ''
  if (isSpace) {
    const tag = target?.tagName
    if (
      tag === 'BUTTON' ||
      tag === 'A' ||
      tag === 'SUMMARY' ||
      role === 'button' ||
      role === 'switch' ||
      role === 'checkbox' ||
      role === 'radio' ||
      role === 'menuitem' ||
      role === 'option' ||
      role === 'slider'
    ) {
      return
    }
    ev.preventDefault()
    void playerStore.togglePlay()
    return
  }

  if (
    role === 'slider' ||
    role === 'spinbutton' ||
    role === 'listbox' ||
    role === 'menu' ||
    role === 'radiogroup' ||
    role === 'tablist' ||
    Boolean(target?.closest('[role="radiogroup"], [role="tablist"], [role="listbox"]'))
  ) {
    return
  }

  ev.preventDefault()
  const delta = isArrowLeft ? -SEEK_STEP_SECS : SEEK_STEP_SECS
  const maxDur = playerStore.durationSecs
  const next = Math.max(
    0,
    maxDur > 0
      ? Math.min(maxDur, playerStore.currentTimeSecs + delta)
      : playerStore.currentTimeSecs + delta,
  )
  playerStore.seekTo(next)
}

onMounted(() => {
  playerStore.attachAudioElement(audioHostRef.value)
  window.addEventListener('keydown', handlePlaybackKeydown)
})

onBeforeUnmount(() => {
  window.removeEventListener('keydown', handlePlaybackKeydown)
  playerStore.detachAudioElement(audioHostRef.value)
})
</script>

<template>
  <footer
    class="safe-px relative z-20 flex w-full shrink-0 flex-col border-t border-outline-variant bg-surface-container text-on-surface"
  >
    <div ref="audioHostRef" class="hidden" aria-hidden="true" />

    <!-- Compact (< 600px) 2px Top-Edge Progress Line -->
    <div
      class="h-0.5 w-full overflow-hidden bg-surface-container-highest sm:hidden"
      aria-hidden="true"
    >
      <div
        class="h-full bg-primary transition-[width] duration-150 ease-standard"
        :style="{ width: `${progressPercent}%` }"
      />
    </div>

    <!-- Playback Error Alert (Retry MUST remain the first <button> inside [role="alert"]) -->
    <div
      v-if="playerStore.playbackError"
      role="alert"
      class="text-body-small flex items-center justify-between gap-2 border-b border-error/25 bg-error-container px-3 py-1.5 text-on-error-container sm:px-6"
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

    <!-- Main Player Bar Row (56px compact single-row on < 600px, 80px on >= 600px) -->
    <div class="flex h-14 shrink-0 items-center justify-between gap-2 px-3 sm:h-20 sm:gap-4 sm:px-6">
      <!-- Left: Artwork, Title/Artist & Favorite -->
      <div
        v-if="playerStore.currentTrack"
        class="flex min-w-0 flex-1 items-center gap-2.5 sm:gap-3 md:max-w-[30%]"
      >
        <button
          type="button"
          class="md-touch-target group relative h-10 w-10 shrink-0 cursor-pointer overflow-hidden rounded-corner-sm shadow-elevation-1 focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-secondary sm:h-12 sm:w-12"
          :title="t('player.openLyrics')"
          :aria-label="t('player.openLyrics')"
          @click="playerStore.lyricsOpen = !playerStore.lyricsOpen"
        >
          <MediaCoverImage
            :src="currentCoverSrc"
            :alt="playerStore.currentTrack.title"
            rounded="rounded-corner-sm"
            :fallback-icon="LuDisc3"
            class="h-full w-full"
          />
          <span
            class="absolute inset-0 z-[2] flex items-center justify-center bg-scrim/45 text-white opacity-0 transition-opacity duration-150 ease-standard group-focus-visible:opacity-100 group-hover:opacity-100"
          >
            <LuChevronUp
              class="h-5 w-5 transition-transform duration-200 ease-standard"
              :class="playerStore.lyricsOpen ? 'rotate-180' : ''"
              aria-hidden="true"
            />
          </span>
        </button>

        <div class="min-w-0 flex-1">
          <button
            type="button"
            class="text-title-small block max-w-full cursor-pointer truncate text-left text-on-surface transition-colors hover:text-primary"
            @click="playerStore.lyricsOpen = true"
          >
            {{ playerStore.currentTrack.title }}
          </button>
          <button
            v-if="playerStore.currentLocalTrack"
            type="button"
            class="text-body-small block max-w-full cursor-pointer truncate text-left text-on-surface-variant transition-colors hover:text-primary hover:underline"
            @click="router.push(`/music/library?artist=${playerStore.currentLocalTrack.artist_id}`)"
          >
            {{ playerStore.currentLocalTrack.artist_name }}
          </button>
          <span
            v-else
            class="text-body-small block max-w-full truncate text-left text-on-surface-variant"
          >
            {{ playerStore.currentTrack.artist_name }}
          </span>
        </div>

        <div class="hidden sm:flex">
          <MdIconButton
            size="sm"
            :selected="isCurrentStarred"
            :aria-label="isCurrentStarred ? t('common.unfavorite') : t('common.favorite')"
            :title="isCurrentStarred ? t('common.unfavorite') : t('common.favorite')"
            @click="handleToggleFavorite"
          >
            <LuHeart
              class="h-4 w-4"
              :class="isCurrentStarred ? 'fill-error text-error' : 'text-on-surface-variant'"
            />
          </MdIconButton>
        </div>
      </div>

      <!-- Center: Transport Controls + Desktop Seek Bar -->
      <div class="flex shrink-0 flex-col items-center justify-center gap-0.5 sm:max-w-xl sm:flex-1">
        <div class="flex items-center gap-1 sm:gap-2">
          <div class="hidden items-center gap-1 sm:flex sm:gap-2">
            <MdIconButton
              size="sm"
              :variant="playerStore.playMode !== 'sequence' ? 'tonal' : 'standard'"
              :selected="playerStore.playMode !== 'sequence'"
              :aria-label="playModeTitle"
              :title="playModeTitle"
              @click="playerStore.cyclePlayMode()"
            >
              <LuRepeat1 v-if="playerStore.playMode === 'loop_one'" class="h-4 w-4" />
              <LuShuffle v-else-if="playerStore.playMode === 'shuffle'" class="h-4 w-4" />
              <LuRepeat v-else class="h-4 w-4" />
            </MdIconButton>

            <MdIconButton
              size="sm"
              :disabled="!playerStore.currentTrack"
              :aria-label="t('player.prev')"
              :title="t('player.prev')"
              @click="playerStore.playPrev()"
            >
              <LuSkipBack class="h-4 w-4 fill-current text-on-surface" />
            </MdIconButton>
          </div>

          <MdIconButton
            variant="filled"
            size="md"
            :loading="playerStore.onlineResolving"
            :disabled="!playerStore.currentTrack"
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
            <LuPause v-if="playerStore.isPlaying" class="h-4 w-4 fill-current" aria-hidden="true" />
            <LuPlay v-else class="ml-0.5 h-4 w-4 fill-current" aria-hidden="true" />
          </MdIconButton>

          <MdIconButton
            size="sm"
            :disabled="!playerStore.currentTrack"
            :aria-label="t('player.next')"
            :title="t('player.next')"
            @click="playerStore.playNext()"
          >
            <LuSkipForward class="h-4 w-4 fill-current text-on-surface" />
          </MdIconButton>
        </div>

        <div class="hidden w-full items-center gap-2.5 sm:flex">
          <span
            class="text-label-small w-11 text-right font-mono tabular-nums"
            :class="scrubPreviewSecs !== null ? 'text-primary' : 'text-on-surface-variant'"
          >
            {{ formatTime(displayTimeSecs) }}
          </span>
          <div class="min-w-0 flex-1">
            <MusicSeekSlider
              :model-value="playerStore.currentTimeSecs"
              :min="0"
              :max="Math.max(1, playerStore.durationSecs)"
              :step="0.2"
              :buffered="playerStore.bufferedPercent"
              :disabled="!playerStore.currentTrack"
              aria-label="Playback progress"
              :aria-value-text="`${formatTime(displayTimeSecs)} / ${formatTime(playerStore.durationSecs)}`"
              @preview="scrubPreviewSecs = $event"
              @commit="playerStore.seekTo($event)"
            />
          </div>
          <span class="text-label-small w-11 font-mono tabular-nums text-on-surface-variant">
            {{ formatTime(playerStore.durationSecs) }}
          </span>
        </div>
      </div>

      <!-- Right: Queue (always accessible), Lyrics, Volume (lg+) & Playback Options Menu (sm+) -->
      <div class="flex shrink-0 items-center justify-end gap-0.5 md:max-w-[30%] md:flex-1 md:gap-1">
        <div class="hidden sm:inline-flex">
          <MdIconButton
            size="sm"
            :variant="playerStore.lyricsOpen ? 'tonal' : 'standard'"
            :selected="playerStore.lyricsOpen"
            :icon="LuMic2"
            :title="t('player.openLyrics')"
            :aria-label="t('player.openLyrics')"
            @click="playerStore.lyricsOpen = !playerStore.lyricsOpen"
          />
        </div>

        <div class="relative inline-flex">
          <MdIconButton
            size="sm"
            :variant="playerStore.queueOpen ? 'tonal' : 'standard'"
            :selected="playerStore.queueOpen"
            :icon="LuListMusic"
            :title="t('player.queueTitle')"
            :aria-label="t('player.queueTitle')"
            @click="playerStore.queueOpen = !playerStore.queueOpen"
          />
          <MdBadge
            v-if="playerStore.queue.length > 0"
            :value="playerStore.queue.length"
            :max="999"
            tone="primary"
            class="pointer-events-none absolute -top-0.5 -right-0.5"
          />
        </div>

        <div class="hidden w-28 items-center gap-1 lg:flex">
          <MdIconButton
            size="sm"
            :icon="playerStore.muted || playerStore.volume === 0 ? LuVolumeX : LuVolume2"
            :title="playerStore.muted ? t('player.unmute') : t('player.mute')"
            :aria-label="playerStore.muted ? t('player.unmute') : t('player.mute')"
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

        <div v-if="hasQualityMenuItems || playerStore.currentLocalTrack" class="hidden sm:inline-flex">
          <MdMenu
            placement="top-end"
            :min-width="220"
            :aria-label="t('player.optionsMenu')"
          >
            <template #trigger="{ toggle, open, triggerAttrs }">
              <MdIconButton
                v-bind="triggerAttrs"
                size="sm"
                :variant="open ? 'tonal' : 'standard'"
                :icon="LuSlidersHorizontal"
                :title="t('player.optionsMenu')"
                :aria-label="t('player.optionsMenu')"
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
            </template>
          </MdMenu>
        </div>
      </div>
    </div>
  </footer>
</template>

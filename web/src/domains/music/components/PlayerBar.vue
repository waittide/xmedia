<script setup lang="ts">
import { computed, onBeforeUnmount, onMounted, ref } from 'vue'
import { useRoute, useRouter } from 'vue-router'
import {
  LuPlay,
  LuPause,
  LuSkipBack,
  LuSkipForward,
  LuRepeat,
  LuRepeat1,
  LuShuffle,
  LuVolume2,
  LuVolumeX,
  LuHeart,
  LuMic2,
  LuListMusic,
  LuChevronUp,
  LuBookmark,
  LuCheck,
  LuDisc3,
  LuLoader2,
} from 'vue-icons-plus/lu'
import AppSelect from '@/components/ui/AppSelect.vue'
import AppSlider from '@/components/ui/AppSlider.vue'
import MdBadge from '@/components/ui/MdBadge.vue'
import MediaCoverImage from '@/components/ui/MediaCoverImage.vue'
import { useDialogStore } from '@/stores/dialog'
import { formatTime } from '@/utils/format'
import { api, buildCoverUrl } from '../api'
import { useMusicI18n as useI18n } from '../i18n'
import { usePlayerStore } from '../player'
import { useMusicStore } from '../store'
import type { TranscodeProfile } from '../types'

const { t } = useI18n()
const route = useRoute()
const router = useRouter()
const dialogStore = useDialogStore()
const playerStore = usePlayerStore()
const musicStore = useMusicStore()

const bookmarkJustSaved = ref(false)
const audioHostRef = ref<HTMLDivElement | null>(null)
let bookmarkTimer: ReturnType<typeof setTimeout> | null = null

const EXCLUDED_SHORTCUT_ROUTES = new Set([
  'book-reader',
  'book-online-reader',
  'comic-reader',
  'comic-online-reader',
  'video-player',
  'video-online-player',
  'login',
  'setup',
])

const SEEK_STEP_SECS = 5

const qualityOptions = computed(() => [
  {
    value: 'raw',
    label:
      playerStore.currentLocalTrack && playerStore.transcodeProfile === 'raw'
        ? playerStore.qualitySummary
        : t('player.qualityRaw'),
  },
  { value: 'opus128', label: t('player.qualityOpus128') },
  { value: 'mp3192', label: t('player.qualityMp3192') },
  { value: 'mp3320', label: t('player.qualityMp3320') },
  { value: 'aac256', label: t('player.qualityAac256') },
])

const onlineQualityOptions = computed(() => {
  const tr = playerStore.currentTrack
  if (!tr || tr.kind !== 'online') return []
  return tr.qualities.map((q) => ({
    value: q,
    label: /^\d+$/.test(q) ? `${q}k` : q.toUpperCase(),
  }))
})

const playModeTitle = computed(() => {
  if (playerStore.playMode === 'loop_one') return t('player.modeLoopOne')
  if (playerStore.playMode === 'shuffle') return t('player.modeShuffle')
  return t('player.modeSequence')
})

function onQualityChange(val: string): void {
  playerStore.setTranscodeProfile(val as TranscodeProfile)
}

function onOnlineQualityChange(val: string): void {
  playerStore.setOnlineQuality(val)
}

async function saveCurrentBookmark(): Promise<void> {
  const tr = playerStore.currentLocalTrack
  if (!tr) return
  await api.saveBookmark(tr.id, Math.round(playerStore.currentTimeSecs * 1000))
  bookmarkJustSaved.value = true
  if (bookmarkTimer) clearTimeout(bookmarkTimer)
  bookmarkTimer = setTimeout(() => {
    bookmarkJustSaved.value = false
  }, 1500)
}

function isExcludedPlaybackRoute(): boolean {
  if (route.matched.some((record) => Boolean(record.meta.fullscreen))) return true
  const routeName = typeof route.name === 'string' ? route.name : ''
  if (EXCLUDED_SHORTCUT_ROUTES.has(routeName)) return true
  const p = route.path
  return (
    p.startsWith('/book/read') ||
    p.startsWith('/book/online/read') ||
    p.startsWith('/comic/read') ||
    p.startsWith('/comic/online/read') ||
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

  const target = (ev.target instanceof HTMLElement ? ev.target : null) ??
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
  const next = Math.max(0, maxDur > 0 ? Math.min(maxDur, playerStore.currentTimeSecs + delta) : playerStore.currentTimeSecs + delta)
  playerStore.seekTo(next)
}

onMounted(() => {
  playerStore.attachAudioElement(audioHostRef.value)
  window.addEventListener('keydown', handlePlaybackKeydown)
})

onBeforeUnmount(() => {
  window.removeEventListener('keydown', handlePlaybackKeydown)
  playerStore.detachAudioElement(audioHostRef.value)
  if (bookmarkTimer) clearTimeout(bookmarkTimer)
})
</script>

<template>
  <footer
    class="safe-px relative z-20 flex w-full shrink-0 flex-col border-t border-outline-variant bg-surface-container text-on-surface"
  >
    <div ref="audioHostRef" class="hidden" aria-hidden="true" />
    <div
      v-if="playerStore.playbackError"
      role="alert"
      class="flex items-center justify-between gap-2 border-b border-[var(--md-sys-color-error)]/25 bg-[var(--md-sys-color-error-container)]/90 px-3 py-1.5 text-xs text-[var(--md-sys-color-on-error-container)] sm:px-6"
    >
      <span class="truncate">{{ playerStore.playbackError }}</span>
      <button
        type="button"
        class="md-btn md-btn-sm md-btn-tonal md-state shrink-0"
        @click="playerStore.retryCurrentPlayback()"
      >
        {{ t('player.retryPlayback') }}
      </button>
    </div>
    <!-- Mobile Compact Draggable Progress Bar (< sm) -->
    <div class="flex items-center gap-2.5 px-3 pt-1 sm:hidden">
      <span
        class="w-9 shrink-0 text-right font-mono text-[10px] text-[var(--md-sys-color-on-surface-variant)]"
      >
        {{ formatTime(playerStore.currentTimeSecs) }}
      </span>
      <div class="min-w-0 flex-1 px-2">
        <AppSlider
          :model-value="playerStore.currentTimeSecs"
          :min="0"
          :max="Math.max(1, playerStore.durationSecs)"
          :step="0.2"
          :buffered="playerStore.bufferedPercent"
          :disabled="!playerStore.currentTrack"
          commit-on-release
          aria-label="Playback progress"
          @change="playerStore.seekTo($event)"
        />
      </div>
      <span
        class="w-9 shrink-0 font-mono text-[10px] text-[var(--md-sys-color-on-surface-variant)]"
      >
        {{ formatTime(playerStore.durationSecs) }}
      </span>
    </div>

    <!-- Main Player Bar Row -->
    <div class="flex h-16 shrink-0 items-center justify-between gap-2 px-3 sm:h-20 sm:gap-4 sm:px-6">
      <!-- Left: Track Info & Cover (Click Cover to open Fullscreen Lyrics) -->
      <div class="flex min-w-0 flex-1 items-center gap-2.5 sm:gap-3 md:max-w-[30%]">
        <template v-if="playerStore.currentTrack">
          <button
            type="button"
            class="group relative h-11 w-11 shrink-0 overflow-hidden rounded-xl shadow-sm focus-visible:outline-3 focus-visible:outline-offset-2 focus-visible:outline-[var(--md-sys-color-secondary)] sm:h-13 sm:w-13"
            :title="t('player.openLyrics')"
            :aria-label="t('player.openLyrics')"
            @click="playerStore.lyricsOpen = !playerStore.lyricsOpen"
          >
            <MediaCoverImage
              :src="
                playerStore.currentTrack.kind === 'local'
                  ? playerStore.currentTrack.cover_key && playerStore.currentTrack.cover_id
                    ? buildCoverUrl(playerStore.currentTrack.cover_id, 160)
                    : null
                  : playerStore.currentTrack.cover_url
              "
              :alt="playerStore.currentTrack.title"
              rounded="rounded-xl"
              :fallback-icon="LuDisc3"
              class="h-full w-full"
            />
            <span
              class="absolute inset-0 flex items-center justify-center bg-black/45 text-white opacity-0 transition group-hover:opacity-100"
            >
              <LuChevronUp
                class="h-5 w-5 transition-transform"
                :class="playerStore.lyricsOpen ? 'rotate-180' : ''"
              />
            </span>
          </button>

          <div class="min-w-0">
            <button
              type="button"
              class="block max-w-full truncate text-left text-xs font-bold text-[var(--md-sys-color-on-surface)] hover:text-[var(--md-sys-color-primary)]"
              @click="playerStore.lyricsOpen = true"
            >
              {{ playerStore.currentTrack.title }}
            </button>
            <button
              v-if="playerStore.currentLocalTrack"
              type="button"
              class="mt-0.5 block max-w-full truncate text-left text-[11px] text-[var(--md-sys-color-on-surface-variant)] hover:text-[var(--md-sys-color-primary)] hover:underline"
              @click="router.push(`/music/artists/${playerStore.currentLocalTrack.artist_id}`)"
            >
              {{ playerStore.currentLocalTrack.artist_name }}
            </button>
            <span
              v-else
              class="mt-0.5 block max-w-full truncate text-left text-[11px] text-[var(--md-sys-color-on-surface-variant)]"
            >
              {{ playerStore.currentTrack.artist_name }}
            </span>
          </div>

          <button
            v-if="playerStore.currentLocalTrack"
            type="button"
            class="md-icon-btn md-icon-btn-sm md-state hidden sm:inline-flex"
            :aria-label="playerStore.currentLocalTrack.starred_at ? t('common.unfavorite') : t('common.favorite')"
            :aria-pressed="Boolean(playerStore.currentLocalTrack.starred_at)"
            @click="musicStore.toggleTrackStar(playerStore.currentLocalTrack)"
          >
            <LuHeart
              class="h-4 w-4"
              :class="
                playerStore.currentLocalTrack.starred_at
                  ? 'fill-[var(--md-sys-color-error)] text-[var(--md-sys-color-error)]'
                  : 'text-[var(--md-sys-color-on-surface-variant)]'
              "
            />
          </button>

          <button
            v-if="playerStore.currentLocalTrack"
            type="button"
            class="md-icon-btn md-icon-btn-sm md-state hidden lg:inline-flex"
            :title="bookmarkJustSaved ? t('player.bookmarkSaved') : t('player.bookmarkBtn')"
            :aria-label="bookmarkJustSaved ? t('player.bookmarkSaved') : t('player.bookmarkBtn')"
            @click="saveCurrentBookmark"
          >
            <LuCheck v-if="bookmarkJustSaved" class="h-4 w-4 text-[var(--md-sys-color-success)]" />
            <LuBookmark v-else class="h-4 w-4" />
          </button>
        </template>

        <div v-else class="min-w-0">
          <p class="truncate text-xs font-semibold text-[var(--md-sys-color-on-surface)]">
            {{ t('player.noTrack') }}
          </p>
          <p class="truncate text-[11px] text-[var(--md-sys-color-on-surface-variant)]">
            {{ t('player.selectHint') }}
          </p>
        </div>
      </div>

      <!-- Center: Transport Controls + Desktop Seek Slider -->
      <div class="flex shrink-0 flex-col items-center justify-center gap-1 sm:max-w-xl sm:flex-1">
        <div class="flex items-center gap-1 sm:gap-2">
          <button
            type="button"
            class="md-icon-btn md-icon-btn-sm md-state hidden sm:inline-flex"
            :class="
              playerStore.playMode !== 'sequence'
                ? 'bg-[var(--md-sys-color-secondary-container)] text-[var(--md-sys-color-on-secondary-container)]'
                : ''
            "
            :title="playModeTitle"
            :aria-label="playModeTitle"
            @click="playerStore.cyclePlayMode()"
          >
            <LuRepeat1 v-if="playerStore.playMode === 'loop_one'" class="h-4 w-4" />
            <LuShuffle v-else-if="playerStore.playMode === 'shuffle'" class="h-4 w-4" />
            <LuRepeat v-else class="h-4 w-4" />
          </button>

          <button
            type="button"
            :disabled="!playerStore.currentTrack"
            class="md-icon-btn md-icon-btn-sm md-state text-[var(--md-sys-color-on-surface)] disabled:opacity-40"
            :title="t('player.prev')"
            :aria-label="t('player.prev')"
            @click="playerStore.playPrev()"
          >
            <LuSkipBack class="h-4 w-4 fill-current" />
          </button>

          <button
            type="button"
            :disabled="!playerStore.currentTrack || playerStore.onlineResolving"
            class="md-state flex h-10 w-10 items-center justify-center rounded-full bg-[var(--md-sys-color-primary)] text-[var(--md-sys-color-on-primary)] shadow-[var(--md-sys-elevation-1)] transition hover:scale-105 disabled:opacity-45"
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
            :aria-busy="playerStore.onlineResolving || undefined"
            @click="playerStore.togglePlay()"
          >
            <LuLoader2 v-if="playerStore.onlineResolving" class="h-4 w-4 animate-spin" />
            <LuPause v-else-if="playerStore.isPlaying" class="h-4 w-4 fill-current" />
            <LuPlay v-else class="ml-0.5 h-4 w-4 fill-current" />
          </button>

          <button
            type="button"
            :disabled="!playerStore.currentTrack"
            class="md-icon-btn md-icon-btn-sm md-state text-[var(--md-sys-color-on-surface)] disabled:opacity-40"
            :title="t('player.next')"
            :aria-label="t('player.next')"
            @click="playerStore.playNext()"
          >
            <LuSkipForward class="h-4 w-4 fill-current" />
          </button>
        </div>

        <div class="hidden w-full items-center gap-2.5 sm:flex">
          <span
            class="w-10 text-right font-mono text-[10px] text-[var(--md-sys-color-on-surface-variant)]"
          >
            {{ formatTime(playerStore.currentTimeSecs) }}
          </span>
          <div class="flex-1">
            <AppSlider
              :model-value="playerStore.currentTimeSecs"
              :min="0"
              :max="Math.max(1, playerStore.durationSecs)"
              :step="0.2"
              :buffered="playerStore.bufferedPercent"
              :disabled="!playerStore.currentTrack"
              commit-on-release
              aria-label="Playback progress"
              @change="playerStore.seekTo($event)"
            />
          </div>
          <span class="w-10 font-mono text-[10px] text-[var(--md-sys-color-on-surface-variant)]">
            {{ formatTime(playerStore.durationSecs) }}
          </span>
        </div>
      </div>

      <!-- Right: Quality Selector (md+), Always-Accessible Lyrics & Queue, Volume (lg+) -->
      <div class="flex shrink-0 items-center justify-end gap-1 md:flex-1 md:max-w-[30%] md:gap-2">
        <div v-if="!playerStore.isCurrentOnline" class="hidden w-40 md:block">
          <AppSelect
            :model-value="playerStore.transcodeProfile"
            :options="qualityOptions"
            variant="compact"
            @change="onQualityChange"
          />
        </div>
        <div v-else-if="onlineQualityOptions.length > 1" class="hidden w-28 md:block">
          <AppSelect
            :model-value="playerStore.currentOnlineItem?.selectedQuality ?? onlineQualityOptions[0]?.value ?? ''"
            :options="onlineQualityOptions"
            variant="compact"
            @change="onOnlineQualityChange"
          />
        </div>
        <span
          v-else-if="playerStore.qualitySummary"
          class="hidden rounded-full bg-[var(--md-sys-color-surface-container-high)] px-2.5 py-1 font-mono text-[10px] font-semibold text-[var(--md-sys-color-on-surface-variant)] md:inline-flex"
        >
          {{ playerStore.qualitySummary }}
        </span>

        <button
          type="button"
          class="md-icon-btn md-icon-btn-sm md-state sm:h-9 sm:w-9"
          :class="
            playerStore.lyricsOpen
              ? 'bg-[var(--md-sys-color-secondary-container)] text-[var(--md-sys-color-on-secondary-container)]'
              : ''
          "
          :title="t('player.openLyrics')"
          :aria-label="t('player.openLyrics')"
          :aria-pressed="playerStore.lyricsOpen"
          @click="playerStore.lyricsOpen = !playerStore.lyricsOpen"
        >
          <LuMic2 class="h-4 w-4" />
        </button>

        <button
          type="button"
          class="md-icon-btn md-icon-btn-sm md-state relative sm:h-9 sm:w-9"
          :class="
            playerStore.queueOpen
              ? 'bg-[var(--md-sys-color-secondary-container)] text-[var(--md-sys-color-on-secondary-container)]'
              : ''
          "
          :title="t('player.queueTitle')"
          :aria-label="t('player.queueTitle')"
          :aria-pressed="playerStore.queueOpen"
          @click="playerStore.queueOpen = !playerStore.queueOpen"
        >
          <LuListMusic class="h-4 w-4" />
          <MdBadge
            v-if="playerStore.queue.length > 0"
            :value="playerStore.queue.length"
            :max="999"
            tone="primary"
            class="pointer-events-none absolute -top-0.5 -right-0.5"
          />
        </button>

        <div class="hidden w-28 items-center gap-1.5 lg:flex">
          <button
            type="button"
            class="md-icon-btn md-icon-btn-sm md-state"
            :title="playerStore.muted ? t('player.unmute') : t('player.mute')"
            :aria-label="playerStore.muted ? t('player.unmute') : t('player.mute')"
            @click="playerStore.toggleMute()"
          >
            <LuVolumeX v-if="playerStore.muted || playerStore.volume === 0" class="h-4 w-4" />
            <LuVolume2 v-else class="h-4 w-4" />
          </button>
          <AppSlider
            :model-value="playerStore.muted ? 0 : playerStore.volume"
            :min="0"
            :max="1"
            :step="0.01"
            aria-label="Volume"
            @update:model-value="playerStore.setVolume($event)"
          />
        </div>
      </div>
    </div>
  </footer>
</template>

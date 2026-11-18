<script setup lang="ts">
import { computed, ref } from 'vue'
import { useRouter } from 'vue-router'
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
} from 'vue-icons-plus/lu'
import AppSelect from '@/components/ui/AppSelect.vue'
import AppSlider from '@/components/ui/AppSlider.vue'
import { api } from '../api'
import { useMusicI18n as useI18n } from '../i18n'
import { usePlayerStore } from '../player'
import { useMusicStore } from '../store'
import type { TranscodeProfile } from '../types'
import CoverImage from './CoverImage.vue'

const { t } = useI18n()
const router = useRouter()
const playerStore = usePlayerStore()
const musicStore = useMusicStore()

const bookmarkJustSaved = ref(false)

const qualityOptions = computed(() => [
  {
    value: 'raw',
    label:
      playerStore.currentTrack && playerStore.transcodeProfile === 'raw'
        ? playerStore.qualitySummary
        : t('player.qualityRaw'),
  },
  { value: 'opus128', label: t('player.qualityOpus128') },
  { value: 'mp3192', label: t('player.qualityMp3192') },
  { value: 'mp3320', label: t('player.qualityMp3320') },
  { value: 'aac256', label: t('player.qualityAac256') },
])

function formatTime(secs: number): string {
  if (!Number.isFinite(secs) || secs < 0) return '0:00'
  const m = Math.floor(secs / 60)
  const s = Math.floor(secs % 60)
  return `${m}:${String(s).padStart(2, '0')}`
}

const playModeTitle = computed(() => {
  if (playerStore.playMode === 'loop_one') return t('player.modeLoopOne')
  if (playerStore.playMode === 'shuffle') return t('player.modeShuffle')
  return t('player.modeSequence')
})

function onQualityChange(val: string): void {
  playerStore.setTranscodeProfile(val as TranscodeProfile)
}

async function saveCurrentBookmark(): Promise<void> {
  const tr = playerStore.currentTrack
  if (!tr) return
  await api.saveBookmark(tr.id, Math.round(playerStore.currentTimeSecs * 1000))
  bookmarkJustSaved.value = true
  setTimeout(() => {
    bookmarkJustSaved.value = false
  }, 1500)
}
</script>

<template>
  <footer
    class="fixed inset-x-0 bottom-0 z-30 flex h-20 shrink-0 items-center justify-between gap-4 border-t border-[var(--ctp-surface0)]/80 bg-[var(--ctp-shell)]/95 px-4 backdrop-blur-md sm:px-6"
  >
    <!-- Left: Track Info & Cover (Click Cover to open Fullscreen Lyrics) -->
    <div class="flex min-w-0 flex-1 items-center gap-3 md:max-w-[30%]">
      <template v-if="playerStore.currentTrack">
        <button
          type="button"
          class="group relative h-13 w-13 shrink-0 overflow-hidden rounded-xl shadow-sm outline-none"
          :title="t('player.openLyrics')"
          @click="playerStore.lyricsOpen = !playerStore.lyricsOpen"
        >
          <CoverImage
            :cover-id="playerStore.currentTrack.id"
            :has-cover="Boolean(playerStore.currentTrack.cover_key)"
            :size="160"
            rounded="rounded-xl"
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
            class="block max-w-full truncate text-left text-xs font-bold text-[var(--ctp-text)] hover:text-[var(--ctp-accent)]"
            @click="playerStore.lyricsOpen = true"
          >
            {{ playerStore.currentTrack.title }}
          </button>
          <button
            type="button"
            class="mt-0.5 block max-w-full truncate text-left text-[11px] text-[var(--ctp-subtext0)] hover:text-[var(--ctp-accent)] hover:underline"
            @click="router.push(`/music/artists/${playerStore.currentTrack.artist_id}`)"
          >
            {{ playerStore.currentTrack.artist_name }}
          </button>
        </div>

        <button
          type="button"
          class="hidden rounded-lg p-1.5 transition hover:bg-[var(--ctp-surface0)] sm:inline-flex"
          @click="musicStore.toggleTrackStar(playerStore.currentTrack)"
        >
          <LuHeart
            class="h-4 w-4"
            :class="
              playerStore.currentTrack.starred_at
                ? 'fill-[var(--ctp-red)] text-[var(--ctp-red)]'
                : 'text-[var(--ctp-subtext0)]'
            "
          />
        </button>

        <button
          type="button"
          class="hidden rounded-lg p-1.5 text-[var(--ctp-subtext0)] transition hover:bg-[var(--ctp-surface0)] hover:text-[var(--ctp-text)] lg:inline-flex"
          :title="bookmarkJustSaved ? t('player.bookmarkSaved') : t('player.bookmarkBtn')"
          @click="saveCurrentBookmark"
        >
          <LuCheck v-if="bookmarkJustSaved" class="h-4 w-4 text-[var(--ctp-green)]" />
          <LuBookmark v-else class="h-4 w-4" />
        </button>
      </template>

      <div v-else class="min-w-0">
        <p class="truncate text-xs font-semibold text-[var(--ctp-subtext1)]">
          {{ t('player.noTrack') }}
        </p>
        <p class="truncate text-[11px] text-[var(--ctp-subtext0)]">
          {{ t('player.selectHint') }}
        </p>
      </div>
    </div>

    <!-- Center: Transport Controls + Draggable Seek Slider -->
    <div class="flex max-w-xl flex-1 flex-col items-center justify-center gap-1">
      <div class="flex items-center gap-2 sm:gap-3">
        <button
          type="button"
          class="rounded-xl p-1.5 transition hover:bg-[var(--ctp-surface0)]"
          :class="
            playerStore.playMode !== 'sequence'
              ? 'text-[var(--ctp-accent)]'
              : 'text-[var(--ctp-subtext1)]'
          "
          :title="playModeTitle"
          @click="playerStore.cyclePlayMode()"
        >
          <LuRepeat1 v-if="playerStore.playMode === 'loop_one'" class="h-4 w-4" />
          <LuShuffle v-else-if="playerStore.playMode === 'shuffle'" class="h-4 w-4" />
          <LuRepeat v-else class="h-4 w-4" />
        </button>

        <button
          type="button"
          :disabled="!playerStore.currentTrack"
          class="rounded-xl p-1.5 text-[var(--ctp-text)] transition hover:bg-[var(--ctp-surface0)] disabled:opacity-40"
          :title="t('player.prev')"
          @click="playerStore.playPrev()"
        >
          <LuSkipBack class="h-4 w-4 fill-current" />
        </button>

        <button
          type="button"
          :disabled="!playerStore.currentTrack"
          class="flex h-10 w-10 items-center justify-center rounded-full bg-[var(--ctp-accent)] text-[var(--ctp-on-accent)] shadow-md transition hover:scale-105 disabled:opacity-45"
          :title="playerStore.isPlaying ? t('player.pause') : t('player.play')"
          @click="playerStore.togglePlay()"
        >
          <LuPause v-if="playerStore.isPlaying" class="h-4 w-4 fill-current" />
          <LuPlay v-else class="ml-0.5 h-4 w-4 fill-current" />
        </button>

        <button
          type="button"
          :disabled="!playerStore.currentTrack"
          class="rounded-xl p-1.5 text-[var(--ctp-text)] transition hover:bg-[var(--ctp-surface0)] disabled:opacity-40"
          :title="t('player.next')"
          @click="playerStore.playNext()"
        >
          <LuSkipForward class="h-4 w-4 fill-current" />
        </button>
      </div>

      <div class="hidden w-full items-center gap-2.5 sm:flex">
        <span class="w-10 text-right font-mono text-[10px] text-[var(--ctp-subtext0)]">
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
        <span class="w-10 font-mono text-[10px] text-[var(--ctp-subtext0)]">
          {{ formatTime(playerStore.durationSecs) }}
        </span>
      </div>
    </div>

    <!-- Right: Quality / Transcode Selector, Lyrics, Queue & Volume -->
    <div class="hidden flex-1 items-center justify-end gap-2 md:flex md:max-w-[30%]">
      <div class="w-40">
        <AppSelect
          :model-value="playerStore.transcodeProfile"
          :options="qualityOptions"
          variant="compact"
          @change="onQualityChange"
        />
      </div>

      <button
        type="button"
        class="rounded-xl p-2 transition"
        :class="
          playerStore.lyricsOpen
            ? 'bg-[var(--ctp-accent-soft)] text-[var(--ctp-accent)]'
            : 'text-[var(--ctp-subtext1)] hover:bg-[var(--ctp-surface0)] hover:text-[var(--ctp-text)]'
        "
        :title="t('player.openLyrics')"
        @click="playerStore.lyricsOpen = !playerStore.lyricsOpen"
      >
        <LuMic2 class="h-4 w-4" />
      </button>

      <button
        type="button"
        class="relative rounded-xl p-2 transition"
        :class="
          playerStore.queueOpen
            ? 'bg-[var(--ctp-accent-soft)] text-[var(--ctp-accent)]'
            : 'text-[var(--ctp-subtext1)] hover:bg-[var(--ctp-surface0)] hover:text-[var(--ctp-text)]'
        "
        :title="t('player.queueTitle')"
        @click="playerStore.queueOpen = !playerStore.queueOpen"
      >
        <LuListMusic class="h-4 w-4" />
        <span
          v-if="playerStore.queue.length > 0"
          class="absolute -top-0.5 -right-0.5 rounded-full bg-[var(--ctp-accent)] px-1 text-[9px] font-bold text-[var(--ctp-on-accent)]"
        >
          {{ playerStore.queue.length }}
        </span>
      </button>

      <div class="flex w-28 items-center gap-1.5">
        <button
          type="button"
          class="rounded-lg p-1.5 text-[var(--ctp-subtext1)] hover:bg-[var(--ctp-surface0)] hover:text-[var(--ctp-text)]"
          :title="playerStore.muted ? t('player.unmute') : t('player.mute')"
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

    <!-- Mobile Compact Lyrics & Queue Controls -->
    <div class="flex shrink-0 items-center gap-1 md:hidden">
      <button
        type="button"
        class="rounded-xl p-2 transition"
        :class="
          playerStore.lyricsOpen
            ? 'bg-[var(--ctp-accent-soft)] text-[var(--ctp-accent)]'
            : 'text-[var(--ctp-subtext1)] hover:bg-[var(--ctp-surface0)] hover:text-[var(--ctp-text)]'
        "
        :title="t('player.openLyrics')"
        @click="playerStore.lyricsOpen = !playerStore.lyricsOpen"
      >
        <LuMic2 class="h-4 w-4" />
      </button>

      <button
        type="button"
        class="relative rounded-xl p-2 transition"
        :class="
          playerStore.queueOpen
            ? 'bg-[var(--ctp-accent-soft)] text-[var(--ctp-accent)]'
            : 'text-[var(--ctp-subtext1)] hover:bg-[var(--ctp-surface0)] hover:text-[var(--ctp-text)]'
        "
        :title="t('player.queueTitle')"
        @click="playerStore.queueOpen = !playerStore.queueOpen"
      >
        <LuListMusic class="h-4 w-4" />
        <span
          v-if="playerStore.queue.length > 0"
          class="absolute -top-0.5 -right-0.5 rounded-full bg-[var(--ctp-accent)] px-1 text-[9px] font-bold text-[var(--ctp-on-accent)]"
        >
          {{ playerStore.queue.length }}
        </span>
      </button>
    </div>
  </footer>
</template>

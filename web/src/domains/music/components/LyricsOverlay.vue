<script setup lang="ts">
import { computed, nextTick, ref, watch } from 'vue'
import { useRouter } from 'vue-router'
import {
  LuChevronDown,
  LuPlay,
  LuPause,
  LuSkipBack,
  LuSkipForward,
  LuRepeat,
  LuRepeat1,
  LuShuffle,
  LuHeart,
  LuMic2,
} from 'vue-icons-plus/lu'
import AppSelect from '@/components/ui/AppSelect.vue'
import AppSlider from '@/components/ui/AppSlider.vue'
import { buildCoverUrl } from '../api'
import { useMusicI18n as useI18n } from '../i18n'
import { usePlayerStore } from '../player'
import { useMusicStore } from '../store'
import type { TranscodeProfile } from '../types'
import CoverImage from './CoverImage.vue'

const { t } = useI18n()
const router = useRouter()
const playerStore = usePlayerStore()
const musicStore = useMusicStore()

const lyricsContainerRef = ref<HTMLDivElement | null>(null)
const lineRefs = ref<(HTMLButtonElement | null)[]>([])

function setLineRef(el: unknown, idx: number): void {
  lineRefs.value[idx] = el as HTMLButtonElement | null
}

const backdropCoverUrl = computed(() => {
  const tr = playerStore.currentTrack
  if (!tr || !tr.cover_key) return null
  return buildCoverUrl(tr.id, 512)
})

const qualityOptions = computed(() => [
  { value: 'raw', label: t('player.qualityRaw') },
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

watch(
  () => [playerStore.activeLyricIndex, playerStore.lyricsOpen] as const,
  async ([activeIdx, isOpen]) => {
    if (!isOpen || activeIdx < 0) return
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
      behavior: 'smooth',
    })
  },
)

function navigateAndClose(path: string): void {
  playerStore.lyricsOpen = false
  void router.push(path)
}
</script>

<template>
  <Teleport to="body">
    <div
      v-if="playerStore.lyricsOpen && playerStore.currentTrack"
      class="fixed inset-0 z-[100] flex h-dvh w-screen flex-col overflow-hidden bg-[var(--ctp-canvas)] text-[var(--ctp-text)]"
    >
      <!-- Atmospheric Blurred Cover Backdrop -->
      <div
        v-if="backdropCoverUrl"
        class="pointer-events-none absolute inset-0 scale-125 bg-cover bg-center opacity-22 blur-3xl"
        :style="{ backgroundImage: `url(${backdropCoverUrl})` }"
      />
      <div class="pointer-events-none absolute inset-0 bg-gradient-to-b from-[var(--ctp-canvas)]/65 via-[var(--ctp-canvas)]/80 to-[var(--ctp-canvas)]/95" />

      <!-- Top Header Bar -->
      <header class="relative z-10 flex shrink-0 items-center justify-between px-6 py-4 sm:px-10">
        <button
          type="button"
          class="inline-flex items-center gap-2 rounded-full border border-[var(--ctp-surface0)] bg-[var(--ctp-shell)]/80 px-4 py-2 text-xs font-semibold text-[var(--ctp-text)] backdrop-blur-md transition hover:bg-[var(--ctp-surface0)]"
          @click="playerStore.lyricsOpen = false"
        >
          <LuChevronDown class="h-4 w-4" />
          <span>{{ t('player.closeLyrics') }}</span>
        </button>

        <div class="flex items-center gap-2">
          <span
            class="rounded-full bg-[var(--ctp-accent-soft)] px-3 py-1 font-mono text-[11px] font-bold text-[var(--ctp-accent)]"
          >
            {{ playerStore.qualitySummary }}
          </span>
          <div class="w-44">
            <AppSelect
              :model-value="playerStore.transcodeProfile"
              :options="qualityOptions"
              variant="compact"
              @change="playerStore.setTranscodeProfile($event as TranscodeProfile)"
            />
          </div>
        </div>
      </header>

      <!-- Main Split Stage: Cover & Controls (Left) + Immersive Lyrics (Right) -->
      <div
        class="relative z-10 mx-auto grid min-h-0 w-full max-w-6xl flex-1 grid-cols-1 gap-8 overflow-y-auto px-6 pb-8 sm:px-10 md:grid-cols-12 md:grid-rows-[minmax(0,1fr)] md:overflow-hidden"
      >
        <!-- Left Artwork & Player Column -->
        <div class="flex min-h-0 flex-col items-center justify-center md:col-span-5">
          <div class="w-full max-w-xs sm:max-w-sm">
            <CoverImage
              :cover-id="playerStore.currentTrack.id"
              :has-cover="Boolean(playerStore.currentTrack.cover_key)"
              :size="640"
              rounded="rounded-3xl"
              class="album-shadow w-full"
            />

            <div class="mt-6 flex items-start justify-between gap-3">
              <div class="min-w-0">
                <h1 class="truncate text-xl font-extrabold tracking-tight">
                  {{ playerStore.currentTrack.title }}
                </h1>
                <div class="mt-1 flex flex-wrap items-center gap-1.5 text-xs text-[var(--ctp-subtext1)]">
                  <button
                    type="button"
                    class="hover:text-[var(--ctp-accent)] hover:underline"
                    @click="navigateAndClose(`/music/artists/${playerStore.currentTrack.artist_id}`)"
                  >
                    {{ playerStore.currentTrack.artist_name }}
                  </button>
                  <span>·</span>
                  <button
                    type="button"
                    class="truncate hover:text-[var(--ctp-accent)] hover:underline"
                    @click="navigateAndClose(`/music/albums/${playerStore.currentTrack.album_id}`)"
                  >
                    {{ playerStore.currentTrack.album_title }}
                  </button>
                </div>
              </div>

              <button
                type="button"
                class="rounded-full p-2 transition hover:bg-[var(--ctp-surface0)]"
                @click="musicStore.toggleTrackStar(playerStore.currentTrack)"
              >
                <LuHeart
                  class="h-5 w-5"
                  :class="
                    playerStore.currentTrack.starred_at
                      ? 'fill-[var(--ctp-red)] text-[var(--ctp-red)]'
                      : 'text-[var(--ctp-subtext0)]'
                  "
                />
              </button>
            </div>

            <!-- Progress Bar -->
            <div class="mt-5">
              <AppSlider
                :model-value="playerStore.currentTimeSecs"
                :min="0"
                :max="Math.max(1, playerStore.durationSecs)"
                :step="0.2"
                :buffered="playerStore.bufferedPercent"
                commit-on-release
                aria-label="Seek"
                @change="playerStore.seekTo($event)"
              />
              <div class="mt-1 flex justify-between font-mono text-[11px] text-[var(--ctp-subtext0)]">
                <span>{{ formatTime(playerStore.currentTimeSecs) }}</span>
                <span>{{ formatTime(playerStore.durationSecs) }}</span>
              </div>
            </div>

            <!-- Transport Controls -->
            <div class="mt-4 flex items-center justify-center gap-5">
              <button
                type="button"
                class="rounded-full p-2 transition hover:bg-[var(--ctp-surface0)]"
                :class="
                  playerStore.playMode !== 'sequence'
                    ? 'text-[var(--ctp-accent)]'
                    : 'text-[var(--ctp-subtext1)]'
                "
                @click="playerStore.cyclePlayMode()"
              >
                <LuRepeat1 v-if="playerStore.playMode === 'loop_one'" class="h-5 w-5" />
                <LuShuffle v-else-if="playerStore.playMode === 'shuffle'" class="h-5 w-5" />
                <LuRepeat v-else class="h-5 w-5" />
              </button>

              <button
                type="button"
                class="rounded-full p-2.5 text-[var(--ctp-text)] transition hover:bg-[var(--ctp-surface0)]"
                @click="playerStore.playPrev()"
              >
                <LuSkipBack class="h-6 w-6 fill-current" />
              </button>

              <button
                type="button"
                class="flex h-14 w-14 items-center justify-center rounded-full bg-[var(--ctp-accent)] text-[var(--ctp-on-accent)] shadow-xl transition hover:scale-105"
                @click="playerStore.togglePlay()"
              >
                <LuPause v-if="playerStore.isPlaying" class="h-6 w-6 fill-current" />
                <LuPlay v-else class="ml-0.5 h-6 w-6 fill-current" />
              </button>

              <button
                type="button"
                class="rounded-full p-2.5 text-[var(--ctp-text)] transition hover:bg-[var(--ctp-surface0)]"
                @click="playerStore.playNext()"
              >
                <LuSkipForward class="h-6 w-6 fill-current" />
              </button>
            </div>
          </div>
        </div>

        <!-- Right Synced / Unsynced Lyrics Column -->
        <div class="flex min-h-0 flex-col md:col-span-7">
          <div class="mb-3 flex items-center justify-between">
            <div class="flex items-center gap-2">
              <LuMic2 class="h-4 w-4 text-[var(--ctp-accent)]" />
              <span class="text-xs font-bold uppercase tracking-wider text-[var(--ctp-subtext0)]">
                {{ t('player.lyricsTitle') }}
              </span>
              <span
                v-if="playerStore.lyrics?.synced"
                class="rounded-full bg-[var(--ctp-accent-soft)] px-2.5 py-0.5 text-[10px] font-semibold text-[var(--ctp-accent)]"
              >
                {{ t('player.syncedBadge') }}
              </span>
              <span
                v-else-if="playerStore.lyrics && playerStore.lyrics.plain_text"
                class="rounded-full bg-[var(--ctp-surface0)] px-2.5 py-0.5 text-[10px] font-medium text-[var(--ctp-subtext1)]"
              >
                {{ t('player.unsyncedBadge') }}
              </span>
            </div>

            <span
              v-if="playerStore.lyrics?.synced"
              class="hidden text-[11px] text-[var(--ctp-subtext0)] sm:inline"
            >
              {{ t('player.clickToSeek') }}
            </span>
          </div>

          <div
            ref="lyricsContainerRef"
            class="min-h-0 flex-1 overflow-y-auto rounded-3xl border border-[var(--ctp-surface0)]/55 bg-[var(--ctp-shell)]/45 px-6 py-10 backdrop-blur-md sm:px-10"
          >
            <div
              v-if="playerStore.lyricsLoading"
              class="flex h-full items-center justify-center text-sm text-[var(--ctp-subtext0)]"
            >
              {{ t('common.loading') }}
            </div>

            <!-- Synced Line-by-Line Interactive Lyrics -->
            <div
              v-else-if="playerStore.lyrics && playerStore.lyrics.synced && playerStore.lyrics.lines.length > 0"
              class="space-y-4 py-16"
            >
              <button
                v-for="(line, idx) in playerStore.lyrics.lines"
                :key="`${line.start_ms}-${idx}`"
                :ref="(el) => setLineRef(el, idx)"
                type="button"
                class="block w-full rounded-2xl px-3 py-2 text-left transition-all duration-200 hover:bg-[var(--ctp-surface0)]/35"
                :class="
                  idx === playerStore.activeLyricIndex
                    ? 'scale-[1.02] text-xl font-extrabold text-[var(--ctp-accent)] sm:text-2xl'
                    : 'text-base font-medium text-[var(--ctp-subtext0)]/75 hover:text-[var(--ctp-text)] sm:text-lg'
                "
                @click="playerStore.seekTo(line.start_ms / 1000)"
              >
                {{ line.text || '♪' }}
              </button>
            </div>

            <!-- Unsynced Plain Text Graceful Fallback -->
            <div
              v-else-if="playerStore.lyrics && playerStore.lyrics.plain_text"
              class="whitespace-pre-line text-base leading-loose font-medium text-[var(--ctp-text)]/90"
            >
              {{ playerStore.lyrics.plain_text }}
            </div>

            <!-- Empty Lyrics State -->
            <div
              v-else
              class="flex h-full flex-col items-center justify-center gap-2 text-center text-sm text-[var(--ctp-subtext0)]"
            >
              <LuMic2 class="h-8 w-8 opacity-40" />
              <p>{{ t('player.noLyrics') }}</p>
            </div>
          </div>
        </div>
      </div>
    </div>
  </Teleport>
</template>

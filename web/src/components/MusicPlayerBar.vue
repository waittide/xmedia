<script setup lang="ts">
import { computed } from 'vue'
import {
  LuDisc3,
  LuListMusic,
  LuPause,
  LuPlay,
  LuRepeat,
  LuRepeat1,
  LuShuffle,
  LuSkipBack,
  LuSkipForward,
  LuVolume2,
  LuVolumeX,
  LuX,
} from 'vue-icons-plus/lu'
import { api } from '@/api/client'
import { useMusicPlayerStore } from '@/stores/musicPlayer'
import { useThemeStore } from '@/stores/theme'

const player = useMusicPlayerStore()
const theme = useThemeStore()

function fmtTime(secs: number): string {
  if (!Number.isFinite(secs) || secs < 0) return '0:00'
  const m = Math.floor(secs / 60)
  const s = Math.floor(secs % 60)
  return `${m}:${String(s).padStart(2, '0')}`
}

const coverSrc = computed(() =>
  player.currentTrack?.cover_key ? api.musicCoverUrl(player.currentTrack.id, 128) : null,
)

function onSeekInput(ev: Event): void {
  const val = Number((ev.target as HTMLInputElement).value)
  player.seekTo(val)
}

function onVolumeInput(ev: Event): void {
  theme.music.volume = Number((ev.target as HTMLInputElement).value)
  theme.music.muted = false
  theme.persistPreferences()
}

function toggleMute(): void {
  theme.music.muted = !theme.music.muted
  theme.persistPreferences()
}
</script>

<template>
  <div v-if="player.currentTrack">
    <!-- Lyrics Drawer Overlay -->
    <div
      v-if="player.lyricsOpen"
      class="fixed inset-x-0 bottom-20 top-14 z-40 flex flex-col bg-[var(--ctp-shell)]/95 backdrop-blur-md border-t border-[var(--ctp-surface0)] p-6 overflow-y-auto"
    >
      <div class="mx-auto flex w-full max-w-3xl items-center justify-between pb-4 border-b border-[var(--ctp-surface0)]">
        <div>
          <h3 class="text-lg font-semibold text-[var(--ctp-text)]">
            {{ player.currentTrack.title }}
          </h3>
          <p class="text-xs text-[var(--ctp-subtext0)]">
            {{ player.currentTrack.artist_name }} · {{ player.currentTrack.album_title }}
          </p>
        </div>
        <button
          type="button"
          class="rounded-lg p-2 text-[var(--ctp-subtext0)] hover:bg-[var(--ctp-surface0)] hover:text-[var(--ctp-text)]"
          @click="player.lyricsOpen = false"
        >
          <LuX class="h-5 w-5" />
        </button>
      </div>

      <div class="mx-auto w-full max-w-3xl flex-1 py-6 space-y-3 text-center">
        <template v-if="player.lyrics && player.lyrics.synced && player.lyrics.lines.length > 0">
          <p
            v-for="(line, idx) in player.lyrics.lines"
            :key="idx"
            class="cursor-pointer transition-all duration-200 py-1"
            :class="
              idx === player.activeLyricIndex
                ? 'text-lg font-bold text-[var(--ctp-accent)] scale-105'
                : 'text-sm text-[var(--ctp-subtext0)] hover:text-[var(--ctp-text)]'
            "
            @click="player.seekTo(line.start_ms / 1000)"
          >
            {{ line.text || '♪' }}
          </p>
        </template>
        <pre
          v-else-if="player.lyrics && player.lyrics.plain_text"
          class="whitespace-pre-wrap font-sans text-sm leading-7 text-[var(--ctp-subtext1)]"
        >{{ player.lyrics.plain_text }}</pre>
        <p v-else class="py-16 text-sm text-[var(--ctp-subtext0)]">
          No lyrics found for this track
        </p>
      </div>
    </div>

    <!-- Persistent Bottom Bar -->
    <footer
      class="h-20 shrink-0 border-t border-[var(--ctp-surface0)] bg-[var(--ctp-mantle)]/95 px-4 backdrop-blur-md flex items-center justify-between gap-4 z-30"
    >
      <!-- Left: Track Info -->
      <div class="flex min-w-0 flex-1 items-center gap-3">
        <div
          class="flex h-12 w-12 shrink-0 items-center justify-center overflow-hidden rounded-lg bg-[var(--ctp-surface0)]"
        >
          <img v-if="coverSrc" :src="coverSrc" alt="" class="h-full w-full object-cover" />
          <LuDisc3 v-else class="h-6 w-6 text-[var(--ctp-subtext0)]" />
        </div>
        <div class="min-w-0">
          <div class="truncate text-sm font-medium text-[var(--ctp-text)]">
            {{ player.currentTrack.title }}
          </div>
          <div class="truncate text-xs text-[var(--ctp-subtext0)]">
            {{ player.currentTrack.artist_name }} · {{ player.currentTrack.format.toUpperCase() }}
          </div>
        </div>
      </div>

      <!-- Center: Controls & Scrubber -->
      <div class="flex flex-1 max-w-xl flex-col items-center gap-1.5">
        <div class="flex items-center gap-3">
          <button
            type="button"
            class="rounded-full p-1.5 text-[var(--ctp-subtext1)] hover:text-[var(--ctp-text)]"
            @click="player.cyclePlayMode()"
          >
            <LuShuffle v-if="theme.music.play_mode === 'shuffle'" class="h-4 w-4 text-[var(--ctp-accent)]" />
            <LuRepeat1 v-else-if="theme.music.play_mode === 'loop_one'" class="h-4 w-4 text-[var(--ctp-accent)]" />
            <LuRepeat v-else class="h-4 w-4" />
          </button>
          <button
            type="button"
            class="rounded-full p-1.5 text-[var(--ctp-text)] hover:bg-[var(--ctp-surface0)]"
            @click="player.prev()"
          >
            <LuSkipBack class="h-4 w-4" />
          </button>
          <button
            type="button"
            class="flex h-9 w-9 items-center justify-center rounded-full bg-[var(--ctp-accent)] text-[var(--ctp-on-accent)] shadow hover:opacity-90"
            @click="player.togglePlay()"
          >
            <LuPause v-if="player.isPlaying" class="h-4 w-4" />
            <LuPlay v-else class="h-4 w-4 ml-0.5" />
          </button>
          <button
            type="button"
            class="rounded-full p-1.5 text-[var(--ctp-text)] hover:bg-[var(--ctp-surface0)]"
            @click="player.next()"
          >
            <LuSkipForward class="h-4 w-4" />
          </button>
          <button
            type="button"
            class="rounded-lg px-2 py-1 text-xs font-medium transition"
            :class="
              player.lyricsOpen
                ? 'bg-[var(--ctp-accent-soft)] text-[var(--ctp-accent)]'
                : 'text-[var(--ctp-subtext1)] hover:bg-[var(--ctp-surface0)]'
            "
            @click="player.lyricsOpen = !player.lyricsOpen"
          >
            <LuListMusic class="inline h-3.5 w-3.5 mr-1" />LRC
          </button>
        </div>

        <div class="flex w-full items-center gap-2 text-[11px] text-[var(--ctp-subtext0)]">
          <span class="w-9 text-right tabular-nums">{{ fmtTime(player.currentTimeSecs) }}</span>
          <input
            type="range"
            min="0"
            :max="Math.max(1, player.durationSecs)"
            :value="player.currentTimeSecs"
            class="h-1.5 flex-1 cursor-pointer accent-[var(--ctp-accent)]"
            @change="onSeekInput"
          />
          <span class="w-9 tabular-nums">{{ fmtTime(player.durationSecs) }}</span>
        </div>
      </div>

      <!-- Right: Transcode Profile & Volume -->
      <div class="hidden md:flex flex-1 items-center justify-end gap-3">
        <select
          v-model="theme.music.transcode_profile"
          class="rounded-md border border-[var(--ctp-surface0)] bg-[var(--ctp-canvas)] px-2 py-1 text-xs text-[var(--ctp-text)]"
          @change="theme.persistPreferences()"
        >
          <option value="raw">RAW / Lossless</option>
          <option value="opus128">Opus 128k</option>
          <option value="mp3192">MP3 192k</option>
          <option value="mp3320">MP3 320k</option>
          <option value="aac256">AAC 256k</option>
        </select>

        <button
          type="button"
          class="text-[var(--ctp-subtext1)] hover:text-[var(--ctp-text)]"
          @click="toggleMute"
        >
          <LuVolumeX v-if="theme.music.muted" class="h-4 w-4 text-[var(--ctp-red)]" />
          <LuVolume2 v-else class="h-4 w-4" />
        </button>
        <input
          type="range"
          min="0"
          max="1"
          step="0.02"
          :value="theme.music.muted ? 0 : theme.music.volume"
          class="w-24 cursor-pointer accent-[var(--ctp-accent)]"
          @input="onVolumeInput"
        />
      </div>
    </footer>
  </div>
</template>

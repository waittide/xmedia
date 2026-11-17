<script setup lang="ts">
import { onBeforeUnmount, onMounted, ref } from 'vue'
import { useRouter } from 'vue-router'
import {
  LuPlay,
  LuPause,
  LuHeart,
  LuMoreHorizontal,
  LuListPlus,
  LuPlus,
  LuDownload,
  LuTrash2,
  LuVolume2,
} from 'vue-icons-plus/lu'
import { useDialogStore } from '@/stores/dialog'
import { api, buildDownloadUrl } from '../api'
import { useMusicI18n as useI18n } from '../i18n'
import { usePlayerStore } from '../player'
import { useMusicStore } from '../store'
import type { TrackDto } from '../types'
import CoverImage from './CoverImage.vue'

const props = withDefaults(
  defineProps<{
    tracks: TrackDto[]
    showAlbum?: boolean
    showCover?: boolean
    removable?: boolean
  }>(),
  {
    showAlbum: true,
    showCover: true,
    removable: false,
  },
)

const emit = defineEmits<{
  remove: [index: number]
}>()

const { t } = useI18n()
const router = useRouter()
const playerStore = usePlayerStore()
const musicStore = useMusicStore()
const dialogStore = useDialogStore()

const activeMenuIdx = ref<number | null>(null)
const menuPos = ref<{ top: string; left: string }>({ top: '0px', left: '0px' })
const menuRef = ref<HTMLDivElement | null>(null)

function formatDuration(secs: number): string {
  const m = Math.floor(secs / 60)
  const s = Math.floor(secs % 60)
  return `${m}:${String(s).padStart(2, '0')}`
}

function formatQuality(tr: TrackDto): string {
  const fmt = tr.format.toUpperCase()
  if (tr.bit_depth && tr.sample_rate_hz) {
    return `${fmt} ${tr.bit_depth}/${Math.round(tr.sample_rate_hz / 1000)}k`
  }
  if (tr.bit_rate_kbps) {
    return `${fmt} ${tr.bit_rate_kbps}k`
  }
  return fmt
}

function handlePlayRow(idx: number): void {
  const clicked = props.tracks[idx]
  if (!clicked) return
  if (playerStore.currentTrack?.id === clicked.id) {
    void playerStore.togglePlay()
  } else {
    playerStore.playQueue(props.tracks, idx)
  }
}

function openRowMenu(ev: MouseEvent, idx: number): void {
  ev.stopPropagation()
  if (activeMenuIdx.value === idx) {
    activeMenuIdx.value = null
    return
  }
  const rect = (ev.currentTarget as HTMLElement).getBoundingClientRect()
  const vw = window.innerWidth
  const vh = window.innerHeight
  const menuW = 216
  const menuH = 220
  const left = Math.min(rect.right - menuW, Math.max(8, vw - menuW - 8))
  const top = rect.bottom + menuH > vh ? Math.max(8, rect.top - menuH) : rect.bottom + 4
  menuPos.value = { top: `${Math.round(top)}px`, left: `${Math.round(left)}px` }
  activeMenuIdx.value = idx
}

async function createPlaylistWithTrack(track: TrackDto): Promise<void> {
  activeMenuIdx.value = null
  const name = await dialogStore.prompt({
    title: t('playlist.createTitle'),
    message: t('playlist.createPrompt'),
    placeholder: t('playlist.namePlaceholder'),
  })
  if (!name || !name.trim()) return
  await api.createPlaylist({ name: name.trim(), track_ids: [track.id] })
  await musicStore.refreshSidebar()
}

async function addToExistingPlaylist(playlistId: string, track: TrackDto): Promise<void> {
  activeMenuIdx.value = null
  await musicStore.addTrackToPlaylist(playlistId, track.id)
}

function handleOutside(ev: MouseEvent): void {
  if (activeMenuIdx.value === null) return
  if (menuRef.value?.contains(ev.target as Node)) return
  activeMenuIdx.value = null
}

onMounted(() => document.addEventListener('mousedown', handleOutside))
onBeforeUnmount(() => document.removeEventListener('mousedown', handleOutside))
</script>

<template>
  <div class="overflow-x-auto rounded-2xl border border-[var(--ctp-surface0)]/75 bg-[var(--ctp-shell)]/55">
    <table class="w-full border-collapse text-left text-xs">
      <thead>
        <tr class="border-b border-[var(--ctp-surface0)]/80 text-[11px] font-semibold uppercase tracking-wider text-[var(--ctp-subtext0)]">
          <th class="w-11 px-3 py-3 text-center">{{ t('catalog.colIndex') }}</th>
          <th class="px-3 py-3">{{ t('catalog.colTitle') }}</th>
          <th v-if="showAlbum" class="hidden px-3 py-3 md:table-cell">{{ t('catalog.colAlbum') }}</th>
          <th class="hidden px-3 py-3 lg:table-cell">{{ t('catalog.colQuality') }}</th>
          <th class="w-20 px-3 py-3 text-right">{{ t('catalog.colDuration') }}</th>
          <th class="w-20 px-3 py-3 text-right"></th>
        </tr>
      </thead>
      <tbody class="divide-y divide-[var(--ctp-surface0)]/45">
        <tr
          v-for="(tr, idx) in tracks"
          :key="`${tr.id}-${idx}`"
          class="group cursor-pointer transition hover:bg-[var(--ctp-row-hover)]"
          :class="
            playerStore.currentTrack?.id === tr.id
              ? 'bg-[var(--ctp-accent-soft)]/65'
              : ''
          "
        >
          <td class="px-3 py-2.5 text-center" @click="handlePlayRow(idx)">
            <button
              type="button"
              class="inline-flex h-7 w-7 items-center justify-center rounded-lg"
            >
              <LuPause
                v-if="playerStore.currentTrack?.id === tr.id && playerStore.isPlaying"
                class="h-3.5 w-3.5 text-[var(--ctp-accent)]"
              />
              <LuVolume2
                v-else-if="playerStore.currentTrack?.id === tr.id"
                class="h-3.5 w-3.5 text-[var(--ctp-accent)]"
              />
              <template v-else>
                <span class="text-[var(--ctp-subtext0)] group-hover:hidden">
                  {{ tr.track_number ?? idx + 1 }}
                </span>
                <LuPlay class="hidden h-3.5 w-3.5 fill-current text-[var(--ctp-text)] group-hover:block" />
              </template>
            </button>
          </td>

          <td class="px-3 py-2.5" @click="handlePlayRow(idx)">
            <div class="flex items-center gap-3">
              <CoverImage
                v-if="showCover"
                :cover-id="tr.id"
                :has-cover="Boolean(tr.cover_key)"
                :size="96"
                rounded="rounded-lg"
                class="h-10 w-10 shrink-0"
              />
              <div class="min-w-0">
                <div class="flex items-center gap-1.5">
                  <span
                    class="truncate font-semibold"
                    :class="
                      playerStore.currentTrack?.id === tr.id
                        ? 'text-[var(--ctp-accent)]'
                        : 'text-[var(--ctp-text)]'
                    "
                  >
                    {{ tr.title }}
                  </span>
                  <span
                    v-if="tr.has_lyrics"
                    class="shrink-0 rounded bg-[var(--ctp-accent-soft)] px-1 py-0.2 text-[9px] font-bold text-[var(--ctp-accent)]"
                  >
                    LRC
                  </span>
                </div>
                <div class="mt-0.5 truncate text-[11px] text-[var(--ctp-subtext0)]">
                  <span
                    class="hover:text-[var(--ctp-accent)] hover:underline"
                    @click.stop="router.push(`/music/artists/${tr.artist_id}`)"
                  >
                    {{ tr.artist_name }}
                  </span>
                </div>
              </div>
            </div>
          </td>

          <td v-if="showAlbum" class="hidden max-w-[14rem] px-3 py-2.5 md:table-cell">
            <button
              type="button"
              class="max-w-full truncate text-left text-[var(--ctp-subtext1)] hover:text-[var(--ctp-accent)] hover:underline"
              @click.stop="router.push(`/music/albums/${tr.album_id}`)"
            >
              {{ tr.album_title }}
            </button>
          </td>

          <td class="hidden px-3 py-2.5 lg:table-cell">
            <span
              class="inline-flex items-center rounded-md px-1.5 py-0.5 font-mono text-[10px] font-semibold"
              :class="
                tr.is_lossless
                  ? 'bg-[var(--ctp-accent-soft)] text-[var(--ctp-accent)]'
                  : 'bg-[var(--ctp-surface0)]/70 text-[var(--ctp-subtext1)]'
              "
            >
              {{ formatQuality(tr) }}
            </span>
          </td>

          <td class="px-3 py-2.5 text-right font-mono text-[11px] text-[var(--ctp-subtext0)]">
            {{ formatDuration(tr.duration_secs) }}
          </td>

          <td class="px-3 py-2.5 text-right">
            <div class="inline-flex items-center justify-end gap-1">
              <button
                type="button"
                class="rounded-lg p-1.5 transition hover:bg-[var(--ctp-surface0)]"
                @click.stop="musicStore.toggleTrackStar(tr)"
              >
                <LuHeart
                  class="h-3.5 w-3.5"
                  :class="
                    tr.starred_at
                      ? 'fill-[var(--ctp-red)] text-[var(--ctp-red)]'
                      : 'text-[var(--ctp-subtext0)]'
                  "
                />
              </button>

              <button
                type="button"
                class="rounded-lg p-1.5 text-[var(--ctp-subtext0)] transition hover:bg-[var(--ctp-surface0)] hover:text-[var(--ctp-text)]"
                @click="openRowMenu($event, idx)"
              >
                <LuMoreHorizontal class="h-4 w-4" />
              </button>
            </div>
          </td>
        </tr>
      </tbody>
    </table>
  </div>

  <Teleport to="body">
    <div
      v-if="activeMenuIdx !== null && tracks[activeMenuIdx]"
      ref="menuRef"
      :style="menuPos"
      class="fixed z-[115] w-56 rounded-2xl border border-[var(--ctp-surface0)] bg-[var(--ctp-canvas)] p-1.5 text-xs text-[var(--ctp-text)] shadow-2xl"
    >
      <button
        type="button"
        class="flex w-full items-center gap-2.5 rounded-xl px-3 py-2 text-left transition hover:bg-[var(--ctp-mantle)]"
        @click="playerStore.enqueueNext(tracks[activeMenuIdx]!); activeMenuIdx = null"
      >
        <LuListPlus class="h-3.5 w-3.5 text-[var(--ctp-subtext0)]" />
        <span>{{ t('common.addToQueue') }}</span>
      </button>

      <button
        type="button"
        class="flex w-full items-center gap-2.5 rounded-xl px-3 py-2 text-left transition hover:bg-[var(--ctp-mantle)]"
        @click="createPlaylistWithTrack(tracks[activeMenuIdx]!)"
      >
        <LuPlus class="h-3.5 w-3.5 text-[var(--ctp-accent)]" />
        <span>{{ t('nav.newPlaylist') }}</span>
      </button>

      <div
        v-if="musicStore.playlists.length > 0"
        class="my-1 max-h-28 overflow-y-auto border-t border-[var(--ctp-surface0)]/60 pt-1"
      >
        <button
          v-for="pl in musicStore.playlists"
          :key="pl.id"
          type="button"
          class="flex w-full items-center justify-between rounded-xl px-3 py-1.5 text-left text-[11px] text-[var(--ctp-subtext1)] transition hover:bg-[var(--ctp-mantle)] hover:text-[var(--ctp-text)]"
          @click="addToExistingPlaylist(pl.id, tracks[activeMenuIdx]!)"
        >
          <span class="truncate">+ {{ pl.name }}</span>
          <span class="font-mono text-[10px] text-[var(--ctp-subtext0)]">{{ pl.track_count }}</span>
        </button>
      </div>

      <a
        :href="buildDownloadUrl(tracks[activeMenuIdx]!.id)"
        class="mt-0.5 flex w-full items-center gap-2.5 rounded-xl px-3 py-2 text-left transition hover:bg-[var(--ctp-mantle)]"
        @click="activeMenuIdx = null"
      >
        <LuDownload class="h-3.5 w-3.5 text-[var(--ctp-subtext0)]" />
        <span>{{ t('common.download') }}</span>
      </a>

      <button
        v-if="removable"
        type="button"
        class="mt-0.5 flex w-full items-center gap-2.5 rounded-xl px-3 py-2 text-left text-[var(--ctp-red)] transition hover:bg-[var(--ctp-red)]/10"
        @click="emit('remove', activeMenuIdx); activeMenuIdx = null"
      >
        <LuTrash2 class="h-3.5 w-3.5" />
        <span>{{ t('playlist.removeTrack') }}</span>
      </button>
    </div>
  </Teleport>
</template>

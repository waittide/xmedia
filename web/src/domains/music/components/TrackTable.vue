<script setup lang="ts">
import { onBeforeUnmount, onMounted, ref } from 'vue'
import { useRouter } from 'vue-router'
import { useI18n as useGlobalI18n } from 'vue-i18n'
import {
  LuPlay,
  LuPause,
  LuHeart,
  LuMoreHorizontal,
  LuListPlus,
  LuPlus,
  LuTag,
  LuDownload,
  LuTrash2,
  LuVolume2,
} from 'vue-icons-plus/lu'
import { useAuthStore } from '@/stores/auth'
import { useDialogStore } from '@/stores/dialog'
import { formatTime as formatDuration } from '@/utils/format'
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
const { t: globalT } = useGlobalI18n()
const router = useRouter()
const authStore = useAuthStore()
const playerStore = usePlayerStore()
const musicStore = useMusicStore()
const dialogStore = useDialogStore()

const activeMenuIdx = ref<number | null>(null)
const menuPos = ref<{ top: string; left: string }>({ top: '0px', left: '0px' })
const menuRef = ref<HTMLDivElement | null>(null)

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

async function editTrackTags(track: TrackDto): Promise<void> {
  activeMenuIdx.value = null
  const raw = await dialogStore.prompt({
    title: globalT('musicTags.editPromptTitle'),
    message: globalT('musicTags.editPromptMessage'),
    placeholder: globalT('musicTags.editPlaceholder'),
    initialValue: (track.tags ?? []).join(', '),
  })
  if (raw === null) return
  const nextTags = raw
    .split(/[,，]/)
    .map((s) => s.trim())
    .filter(Boolean)
  const updated = await api.setTrackTags(track.id, nextTags)
  track.tags = updated.tags
  await musicStore.refreshSidebar()
}

async function confirmDeleteTrack(track: TrackDto): Promise<void> {
  activeMenuIdx.value = null
  const confirmed = await dialogStore.confirm({
    title: t('common.deleteTrackConfirmTitle'),
    message: t('common.deleteTrackConfirm', { title: track.title }),
    danger: true,
  })
  if (!confirmed) return
  await api.deleteTrack(track.id)
  playerStore.removeByTrackId(track.id)
  musicStore.bumpCatalogVersion()
  await musicStore.refreshSidebar()
}

function handleOutside(ev: MouseEvent): void {
  if (activeMenuIdx.value === null) return
  if (menuRef.value?.contains(ev.target as Node)) return
  activeMenuIdx.value = null
}

function handleKeydown(ev: KeyboardEvent): void {
  if (ev.key === 'Escape' && activeMenuIdx.value !== null) {
    activeMenuIdx.value = null
  }
}

onMounted(() => {
  document.addEventListener('mousedown', handleOutside)
  window.addEventListener('keydown', handleKeydown)
})

onBeforeUnmount(() => {
  document.removeEventListener('mousedown', handleOutside)
  window.removeEventListener('keydown', handleKeydown)
})
</script>

<template>
  <div
    class="md-card-filled overflow-x-auto border border-[var(--md-sys-color-outline-variant)]/50"
  >
    <table class="w-full border-collapse text-left text-xs">
      <thead>
        <tr
          class="border-b border-[var(--md-sys-color-outline-variant)]/60 text-[11px] font-semibold uppercase tracking-wider text-[var(--md-sys-color-on-surface-variant)]"
        >
          <th class="w-11 px-3 py-3 text-center">{{ t('catalog.colIndex') }}</th>
          <th class="px-3 py-3">{{ t('catalog.colTitle') }}</th>
          <th v-if="showAlbum" class="hidden px-3 py-3 md:table-cell">{{ t('catalog.colAlbum') }}</th>
          <th class="hidden px-3 py-3 lg:table-cell">{{ t('catalog.colQuality') }}</th>
          <th class="w-20 px-3 py-3 text-right">{{ t('catalog.colDuration') }}</th>
          <th class="w-20 px-3 py-3 text-right"></th>
        </tr>
      </thead>
      <tbody class="divide-y divide-[var(--md-sys-color-outline-variant)]/35">
        <tr
          v-for="(tr, idx) in tracks"
          :key="`${tr.id}-${idx}`"
          class="group cursor-pointer transition hover:bg-[var(--md-sys-color-surface-container-high)]/60"
          :class="
            playerStore.currentTrack?.id === tr.id
              ? 'bg-[var(--md-sys-color-secondary-container)]/55'
              : ''
          "
        >
          <td class="px-3 py-2.5 text-center" @click="handlePlayRow(idx)">
            <button
              type="button"
              :aria-label="
                playerStore.currentTrack?.id === tr.id && playerStore.isPlaying
                  ? t('player.pause')
                  : t('player.play')
              "
              class="inline-flex h-7 w-7 items-center justify-center rounded-full"
            >
              <LuPause
                v-if="playerStore.currentTrack?.id === tr.id && playerStore.isPlaying"
                class="h-3.5 w-3.5 text-[var(--md-sys-color-primary)]"
              />
              <LuVolume2
                v-else-if="playerStore.currentTrack?.id === tr.id"
                class="h-3.5 w-3.5 text-[var(--md-sys-color-primary)]"
              />
              <template v-else>
                <span class="text-[var(--md-sys-color-on-surface-variant)] group-hover:hidden">
                  {{ tr.track_number ?? idx + 1 }}
                </span>
                <LuPlay
                  class="hidden h-3.5 w-3.5 fill-current text-[var(--md-sys-color-on-surface)] group-hover:block"
                />
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
                        ? 'text-[var(--md-sys-color-primary)]'
                        : 'text-[var(--md-sys-color-on-surface)]'
                    "
                  >
                    {{ tr.title }}
                  </span>
                  <span
                    v-if="tr.has_lyrics"
                    class="md-label-small shrink-0 rounded bg-[var(--md-sys-color-primary-container)] px-1 py-0.2 font-bold text-[var(--md-sys-color-on-primary-container)]"
                  >
                    LRC
                  </span>
                </div>
                <div
                  class="mt-0.5 flex flex-wrap items-center gap-1.5 text-[11px] text-[var(--md-sys-color-on-surface-variant)]"
                >
                  <button
                    type="button"
                    class="truncate text-left hover:text-[var(--md-sys-color-primary)] hover:underline"
                    @click.stop="router.push(`/music/artists/${tr.artist_id}`)"
                  >
                    {{ tr.artist_name }}
                  </button>
                  <button
                    v-for="tg in tr.tags ?? []"
                    :key="tg"
                    type="button"
                    class="md-label-small rounded-md bg-[var(--md-sys-color-surface-container-high)] px-1.5 py-0.2 text-[var(--md-sys-color-on-surface-variant)] transition hover:bg-[var(--md-sys-color-secondary-container)] hover:text-[var(--md-sys-color-on-secondary-container)]"
                    @click.stop="router.push({ path: '/music/songs', query: { tag: tg } })"
                  >
                    #{{ tg }}
                  </button>
                </div>
              </div>
            </div>
          </td>

          <td v-if="showAlbum" class="hidden max-w-[14rem] px-3 py-2.5 md:table-cell">
            <button
              type="button"
              class="max-w-full truncate text-left text-[var(--md-sys-color-on-surface-variant)] hover:text-[var(--md-sys-color-primary)] hover:underline"
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
                  ? 'bg-[var(--md-sys-color-primary-container)] text-[var(--md-sys-color-on-primary-container)]'
                  : 'bg-[var(--md-sys-color-surface-container-high)] text-[var(--md-sys-color-on-surface-variant)]'
              "
            >
              {{ formatQuality(tr) }}
            </span>
          </td>

          <td
            class="px-3 py-2.5 text-right font-mono text-[11px] text-[var(--md-sys-color-on-surface-variant)]"
          >
            {{ formatDuration(tr.duration_secs) }}
          </td>

          <td class="px-3 py-2.5 text-right">
            <div class="inline-flex items-center justify-end gap-0.5">
              <button
                type="button"
                class="md-icon-btn md-icon-btn-sm md-state"
                :aria-label="tr.starred_at ? t('common.unfavorite') : t('common.favorite')"
                :aria-pressed="Boolean(tr.starred_at)"
                @click.stop="musicStore.toggleTrackStar(tr)"
              >
                <LuHeart
                  class="h-3.5 w-3.5"
                  :class="
                    tr.starred_at
                      ? 'fill-[var(--md-sys-color-error)] text-[var(--md-sys-color-error)]'
                      : 'text-[var(--md-sys-color-on-surface-variant)]'
                  "
                />
              </button>

              <button
                type="button"
                class="md-icon-btn md-icon-btn-sm md-state"
                :aria-label="t('common.moreActions')"
                :aria-expanded="activeMenuIdx === idx"
                aria-haspopup="menu"
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
    <Transition name="md-fade">
      <div
        v-if="activeMenuIdx !== null && tracks[activeMenuIdx]"
        ref="menuRef"
        role="menu"
        :style="menuPos"
        class="fixed z-[var(--z-popover)] w-56 rounded-2xl border border-[var(--md-sys-color-outline-variant)]/50 bg-[var(--md-sys-color-surface-container-high)] p-1.5 text-xs text-[var(--md-sys-color-on-surface)] shadow-[var(--md-sys-elevation-3)]"
      >
        <button
          type="button"
          role="menuitem"
          class="md-state flex w-full items-center gap-2.5 rounded-xl px-3 py-2 text-left"
          @click="playerStore.enqueueNext(tracks[activeMenuIdx]!); activeMenuIdx = null"
        >
          <LuListPlus class="h-3.5 w-3.5 text-[var(--md-sys-color-on-surface-variant)]" />
          <span>{{ t('common.addToQueue') }}</span>
        </button>

        <button
          v-if="authStore.isAdmin"
          type="button"
          role="menuitem"
          class="md-state flex w-full items-center gap-2.5 rounded-xl px-3 py-2 text-left"
          @click="editTrackTags(tracks[activeMenuIdx]!)"
        >
          <LuTag class="h-3.5 w-3.5 text-[var(--md-sys-color-primary)]" />
          <span>{{ globalT('musicTags.editTags') }}</span>
        </button>

        <button
          type="button"
          role="menuitem"
          class="md-state flex w-full items-center gap-2.5 rounded-xl px-3 py-2 text-left"
          @click="createPlaylistWithTrack(tracks[activeMenuIdx]!)"
        >
          <LuPlus class="h-3.5 w-3.5 text-[var(--md-sys-color-primary)]" />
          <span>{{ t('nav.newPlaylist') }}</span>
        </button>

        <div
          v-if="musicStore.playlists.length > 0"
          class="my-1 max-h-28 overflow-y-auto border-t border-[var(--md-sys-color-outline-variant)]/50 pt-1"
        >
          <button
            v-for="pl in musicStore.playlists"
            :key="pl.id"
            type="button"
            role="menuitem"
            class="md-state flex w-full items-center justify-between rounded-xl px-3 py-1.5 text-left text-[11px] text-[var(--md-sys-color-on-surface-variant)] hover:text-[var(--md-sys-color-on-surface)]"
            @click="addToExistingPlaylist(pl.id, tracks[activeMenuIdx]!)"
          >
            <span class="truncate">+ {{ pl.name }}</span>
            <span class="font-mono text-[10px] text-[var(--md-sys-color-on-surface-variant)]">
              {{ pl.track_count }}
            </span>
          </button>
        </div>

        <a
          :href="buildDownloadUrl(tracks[activeMenuIdx]!.id)"
          role="menuitem"
          class="md-state mt-0.5 flex w-full items-center gap-2.5 rounded-xl px-3 py-2 text-left"
          @click="activeMenuIdx = null"
        >
          <LuDownload class="h-3.5 w-3.5 text-[var(--md-sys-color-on-surface-variant)]" />
          <span>{{ t('common.download') }}</span>
        </a>

        <button
          v-if="removable"
          type="button"
          role="menuitem"
          class="md-state mt-0.5 flex w-full items-center gap-2.5 rounded-xl px-3 py-2 text-left text-[var(--md-sys-color-error)]"
          @click="emit('remove', activeMenuIdx); activeMenuIdx = null"
        >
          <LuTrash2 class="h-3.5 w-3.5" />
          <span>{{ t('playlist.removeTrack') }}</span>
        </button>
        <button
          v-if="authStore.isAdmin"
          type="button"
          role="menuitem"
          class="md-state mt-0.5 flex w-full items-center gap-2.5 rounded-xl px-3 py-2 text-left text-[var(--md-sys-color-error)]"
          @click="confirmDeleteTrack(tracks[activeMenuIdx]!)"
        >
          <LuTrash2 class="h-3.5 w-3.5" />
          <span>{{ t('common.delete') }}</span>
        </button>
      </div>
    </Transition>
  </Teleport>
</template>

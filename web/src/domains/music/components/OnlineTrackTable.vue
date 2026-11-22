<script setup lang="ts">
import { computed, nextTick, onBeforeUnmount, onMounted, ref } from 'vue'
import { LuCheck, LuListPlus, LuMoreHorizontal, LuPause, LuPlay, LuVolume2 } from 'vue-icons-plus/lu'
import { useSnackbarStore } from '@/stores/snackbar'
import { formatTime as formatDuration } from '@/utils/format'
import { useMusicI18n as useI18n } from '../i18n'
import { resolveOnlineMusicCoverUrl, type OnlineMusicTrackDto } from '../onlineApi'
import { buildOnlineTrackKey, usePlayerStore } from '../player'
import CoverImage from './CoverImage.vue'

const props = withDefaults(
  defineProps<{
    tracks: OnlineMusicTrackDto[]
    showSource?: boolean
    showAlbum?: boolean
  }>(),
  {
    showSource: true,
    showAlbum: true,
  },
)

const { t } = useI18n()
const playerStore = usePlayerStore()
const snackbarStore = useSnackbarStore()

const activeMenuKey = ref<string | null>(null)
const activeMenuTrack = computed<OnlineMusicTrackDto | null>(() =>
  activeMenuKey.value
    ? (props.tracks.find((tr) => buildOnlineTrackKey(tr) === activeMenuKey.value) ?? null)
    : null,
)
const menuPos = ref<{ top: string; left: string }>({ top: '0px', left: '0px' })
const menuRef = ref<HTMLDivElement | null>(null)

function trackKey(tr: OnlineMusicTrackDto): string {
  return buildOnlineTrackKey(tr)
}

function isCurrentTrack(tr: OnlineMusicTrackDto): boolean {
  return playerStore.currentTrack?.kind === 'online' && playerStore.currentTrack.id === trackKey(tr)
}

function resolveCoverSrc(tr: OnlineMusicTrackDto): string | null {
  const resolved = resolveOnlineMusicCoverUrl(tr.proxy_cover_url, tr.cover_url, tr.source_id)
  return resolved || null
}

function formatOnlineQuality(tr: OnlineMusicTrackDto): string {
  if (tr.format && tr.format.trim()) return tr.format.trim().toUpperCase()
  const qs = tr.qualities ?? []
  if (qs.includes('flac') || qs.includes('999') || qs.includes('740')) return 'FLAC'
  if (qs.includes('320')) return '320k'
  if (qs.length > 0) {
    const first = qs[0]!
    return /^\d+$/.test(first) ? `${first}k` : first.toUpperCase()
  }
  return t('online.badgeOnline')
}

function handlePlayRow(idx: number): void {
  const clicked = props.tracks[idx]
  if (!clicked) return
  if (isCurrentTrack(clicked)) {
    void playerStore.togglePlay()
  } else {
    playerStore.playOnlineTracks(props.tracks, idx)
  }
}

function handleEnqueueTrack(tr: OnlineMusicTrackDto): void {
  if (playerStore.isOnlineTrackInQueue(tr)) return
  playerStore.enqueueOnlineTrack(tr)
  snackbarStore.show(t('online.queuedToast'))
}

function handlePlayNext(tr: OnlineMusicTrackDto): void {
  activeMenuKey.value = null
  playerStore.enqueueOnlineTrackNext(tr)
  snackbarStore.show(t('online.playNextToast'))
}

function handleMenuEnqueue(tr: OnlineMusicTrackDto): void {
  activeMenuKey.value = null
  handleEnqueueTrack(tr)
}

async function openRowMenu(ev: MouseEvent, track: OnlineMusicTrackDto): Promise<void> {
  ev.stopPropagation()
  const key = trackKey(track)
  if (activeMenuKey.value === key) {
    activeMenuKey.value = null
    return
  }
  const rect = (ev.currentTarget as HTMLElement).getBoundingClientRect()
  const vw = window.innerWidth
  const vh = window.innerHeight
  const menuW = 208
  const estH = 110
  const left = Math.min(rect.right - menuW, Math.max(8, vw - menuW - 8))
  const top = rect.bottom + estH > vh ? Math.max(8, rect.top - estH) : rect.bottom + 4
  menuPos.value = { top: `${Math.round(top)}px`, left: `${Math.round(left)}px` }
  activeMenuKey.value = key

  await nextTick()
  if (activeMenuKey.value !== key || !menuRef.value) return
  const measured = menuRef.value.getBoundingClientRect()
  const finalLeft = Math.min(rect.right - measured.width, Math.max(8, vw - measured.width - 8))
  const finalTop =
    rect.bottom + measured.height + 8 > vh
      ? Math.max(8, rect.top - measured.height - 4)
      : rect.bottom + 4
  menuPos.value = { top: `${Math.round(finalTop)}px`, left: `${Math.round(finalLeft)}px` }
}

function handleOutside(ev: MouseEvent): void {
  if (activeMenuKey.value === null) return
  if (menuRef.value?.contains(ev.target as Node)) return
  activeMenuKey.value = null
}

function handleKeydown(ev: KeyboardEvent): void {
  if (ev.key === 'Escape' && activeMenuKey.value !== null) {
    activeMenuKey.value = null
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
    class="md-card-filled overflow-hidden border border-[var(--md-sys-color-outline-variant)]/50"
  >
    <table class="w-full table-fixed border-collapse text-left text-xs">
      <thead>
        <tr
          class="border-b border-[var(--md-sys-color-outline-variant)]/60 text-[11px] font-semibold uppercase tracking-wider text-[var(--md-sys-color-on-surface-variant)]"
        >
          <th class="w-11 px-2 py-3 text-center sm:px-3">{{ t('catalog.colIndex') }}</th>
          <th class="px-2 py-3 sm:px-3">{{ t('catalog.colTitle') }}</th>
          <th v-if="showAlbum" class="hidden w-40 px-3 py-3 lg:table-cell">{{ t('catalog.colAlbum') }}</th>
          <th v-if="showSource" class="hidden w-36 px-3 py-3 md:table-cell">{{ t('online.colSource') }}</th>
          <th class="hidden w-20 px-3 py-3 xl:table-cell">{{ t('catalog.colQuality') }}</th>
          <th class="hidden w-16 px-2 py-3 text-right sm:table-cell sm:px-3">{{ t('catalog.colDuration') }}</th>
          <th class="w-24 px-2 py-3 text-right sm:w-32 sm:px-3"></th>
        </tr>
      </thead>
      <tbody class="divide-y divide-[var(--md-sys-color-outline-variant)]/35">
        <tr
          v-for="(tr, idx) in tracks"
          :key="`${trackKey(tr)}-${idx}`"
          class="group cursor-pointer transition hover:bg-[var(--md-sys-color-surface-container-high)]/60"
          :class="
            isCurrentTrack(tr) ? 'bg-[var(--md-sys-color-secondary-container)]/55' : ''
          "
          @click="handlePlayRow(idx)"
        >
          <td class="w-11 px-2 py-2.5 text-center sm:px-3" @click.stop="handlePlayRow(idx)">
            <button
              type="button"
              :title="
                isCurrentTrack(tr) && playerStore.isPlaying ? t('player.pause') : t('player.play')
              "
              :aria-label="
                isCurrentTrack(tr) && playerStore.isPlaying ? t('player.pause') : t('player.play')
              "
              class="inline-flex h-7 w-7 items-center justify-center rounded-full hover:bg-[var(--md-sys-color-surface-container-highest)]"
              @click.stop="handlePlayRow(idx)"
            >
              <LuPause
                v-if="isCurrentTrack(tr) && playerStore.isPlaying"
                class="h-3.5 w-3.5 text-[var(--md-sys-color-primary)]"
              />
              <LuVolume2
                v-else-if="isCurrentTrack(tr)"
                class="h-3.5 w-3.5 text-[var(--md-sys-color-primary)]"
              />
              <LuPlay
                v-else
                class="h-3.5 w-3.5 fill-current text-[var(--md-sys-color-on-surface)]"
              />
            </button>
          </td>

          <td class="px-2 py-2.5 sm:px-3">
            <div class="flex min-w-0 items-center gap-2.5 sm:gap-3">
              <CoverImage
                :src="resolveCoverSrc(tr)"
                :has-cover="false"
                :size="96"
                :alt="tr.title"
                rounded="rounded-lg"
                class="h-10 w-10 shrink-0"
              />
              <div class="min-w-0 flex-1 overflow-hidden">
                <div class="flex min-w-0 items-center gap-1.5">
                  <button
                    type="button"
                    class="min-w-0 truncate text-left font-semibold hover:text-[var(--md-sys-color-primary)]"
                    :class="
                      isCurrentTrack(tr)
                        ? 'text-[var(--md-sys-color-primary)]'
                        : 'text-[var(--md-sys-color-on-surface)]'
                    "
                    :title="tr.title"
                    @click.stop="handlePlayRow(idx)"
                  >
                    {{ tr.title }}
                  </button>
                  <span
                    v-if="tr.lyric_id"
                    class="md-label-small shrink-0 rounded bg-[var(--md-sys-color-primary-container)] px-1 py-0.2 font-bold text-[var(--md-sys-color-on-primary-container)]"
                  >
                    LRC
                  </span>
                </div>
                <div
                  class="mt-0.5 flex min-w-0 flex-wrap items-center gap-x-1.5 gap-y-0.5 text-[11px] text-[var(--md-sys-color-on-surface-variant)]"
                >
                  <span class="min-w-0 truncate">{{ tr.artist || t('online.unknownArtist') }}</span>
                  <span
                    v-if="showSource"
                    class="max-w-[7.5rem] shrink-0 truncate rounded-full bg-[var(--md-sys-color-surface-container-high)] px-1.5 py-0.2 text-[10px] md:hidden"
                    :title="tr.source_name"
                  >
                    {{ tr.source_name }}
                  </span>
                </div>
              </div>
            </div>
          </td>

          <td v-if="showAlbum" class="hidden w-40 px-3 py-2.5 lg:table-cell">
            <span
              class="block truncate text-[var(--md-sys-color-on-surface-variant)]"
              :title="tr.album || undefined"
            >
              {{ tr.album || '—' }}
            </span>
          </td>

          <td v-if="showSource" class="hidden w-36 px-3 py-2.5 md:table-cell">
            <span
              class="inline-flex max-w-full items-center truncate rounded-full bg-[var(--md-sys-color-primary-container)] px-2 py-0.5 text-[10px] font-medium text-[var(--md-sys-color-on-primary-container)]"
              :title="tr.source_name"
            >
              <span class="truncate">{{ tr.source_name }}</span>
            </span>
          </td>

          <td class="hidden w-20 px-3 py-2.5 xl:table-cell">
            <span
              class="inline-flex max-w-full items-center truncate rounded-md bg-[var(--md-sys-color-surface-container-high)] px-1.5 py-0.5 font-mono text-[10px] font-semibold text-[var(--md-sys-color-on-surface-variant)]"
            >
              {{ formatOnlineQuality(tr) }}
            </span>
          </td>

          <td
            class="hidden w-16 px-2 py-2.5 text-right font-mono text-[11px] text-[var(--md-sys-color-on-surface-variant)] sm:table-cell sm:px-3"
          >
            {{ tr.duration_secs != null && tr.duration_secs > 0 ? formatDuration(tr.duration_secs) : '—' }}
          </td>

          <td class="w-24 px-2 py-2.5 text-right sm:w-32 sm:px-3" @click.stop>
            <div class="inline-flex items-center justify-end gap-1">
              <button
                type="button"
                class="md-btn md-btn-sm md-state h-7 rounded-full px-2 text-[11px] sm:px-2.5"
                :class="
                  playerStore.isOnlineTrackInQueue(tr)
                    ? 'bg-[var(--md-sys-color-secondary-container)] text-[var(--md-sys-color-on-secondary-container)]'
                    : 'md-btn-tonal'
                "
                :title="
                  playerStore.isOnlineTrackInQueue(tr) ? t('online.enqueued') : t('online.enqueue')
                "
                :aria-label="
                  playerStore.isOnlineTrackInQueue(tr) ? t('online.enqueued') : t('online.enqueue')
                "
                @click.stop="handleEnqueueTrack(tr)"
              >
                <LuCheck v-if="playerStore.isOnlineTrackInQueue(tr)" class="h-3.5 w-3.5 shrink-0" />
                <LuListPlus v-else class="h-3.5 w-3.5 shrink-0" />
                <span class="hidden sm:inline">
                  {{ playerStore.isOnlineTrackInQueue(tr) ? t('online.enqueued') : t('online.enqueue') }}
                </span>
              </button>

              <button
                type="button"
                class="md-icon-btn md-icon-btn-sm md-state"
                :aria-label="t('common.moreActions')"
                :aria-expanded="activeMenuKey === trackKey(tr)"
                aria-haspopup="menu"
                @click.stop="openRowMenu($event, tr)"
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
        v-if="activeMenuTrack"
        ref="menuRef"
        role="menu"
        :style="menuPos"
        class="fixed z-[var(--z-popover)] w-52 rounded-2xl border border-[var(--md-sys-color-outline-variant)]/50 bg-[var(--md-sys-color-surface-container-high)] p-1.5 text-xs text-[var(--md-sys-color-on-surface)] shadow-[var(--md-sys-elevation-3)]"
      >
        <button
          type="button"
          role="menuitem"
          class="md-state flex w-full items-center gap-2.5 rounded-xl px-3 py-2 text-left"
          @click="handlePlayNext(activeMenuTrack)"
        >
          <LuListPlus class="h-3.5 w-3.5 text-[var(--md-sys-color-on-surface-variant)]" />
          <span>{{ t('online.playNext') }}</span>
        </button>

        <button
          type="button"
          role="menuitem"
          class="md-state flex w-full items-center gap-2.5 rounded-xl px-3 py-2 text-left"
          @click="handleMenuEnqueue(activeMenuTrack)"
        >
          <LuCheck
            v-if="playerStore.isOnlineTrackInQueue(activeMenuTrack)"
            class="h-3.5 w-3.5 text-[var(--md-sys-color-primary)]"
          />
          <LuListPlus v-else class="h-3.5 w-3.5 text-[var(--md-sys-color-on-surface-variant)]" />
          <span>
            {{
              playerStore.isOnlineTrackInQueue(activeMenuTrack)
                ? t('online.enqueued')
                : t('online.enqueue')
            }}
          </span>
        </button>
      </div>
    </Transition>
  </Teleport>
</template>

<script setup lang="ts">
import { ref } from 'vue'
import {
  LuCheck,
  LuDisc3,
  LuDownload,
  LuLibrary,
  LuListPlus,
  LuMoreHorizontal,
  LuPause,
  LuPlay,
  LuVolume2,
} from 'vue-icons-plus/lu'
import { MdButton, MdIconButton, MdMenu, MdMenuItem, MediaCoverImage } from '@/components/ui'
import { useSnackbarStore } from '@/stores/snackbar'
import { formatTime as formatDuration } from '@/utils/format'
import { downloadOnlineTrack } from '../download'
import { useMusicI18n as useI18n } from '../i18n'
import {
  onlineMusicApi,
  resolveOnlineMusicCoverUrl,
  type OnlineMusicTrackDto,
  type TrackOnlineMusicInput,
} from '../onlineApi'
import { buildOnlineTrackKey, usePlayerStore } from '../player'
import { useMusicStore } from '../store'

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
const musicStore = useMusicStore()
const snackbarStore = useSnackbarStore()

const trackingBusy = ref<Record<string, boolean>>({})
const downloadingKeys = ref<Record<string, boolean>>({})
const trackedKeys = ref<Record<string, boolean>>({})
const batchTracking = ref(false)

function trackKey(tr: OnlineMusicTrackDto): string {
  return buildOnlineTrackKey(tr)
}

function isCurrentTrack(tr: OnlineMusicTrackDto): boolean {
  return playerStore.currentTrack?.kind === 'online' && playerStore.currentTrack.id === trackKey(tr)
}

function resolveCoverSrc(tr: OnlineMusicTrackDto): string | null {
  const resolved = resolveOnlineMusicCoverUrl(
    tr.proxy_cover_url,
    tr.cover_url,
    tr.source_id,
    tr.cover_id,
    tr.track_id,
  )
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
  playerStore.enqueueOnlineTrackNext(tr)
  snackbarStore.show(t('online.playNextToast'))
}

async function handleDownloadTrack(tr: OnlineMusicTrackDto): Promise<void> {
  const key = trackKey(tr)
  if (downloadingKeys.value[key]) return
  downloadingKeys.value = { ...downloadingKeys.value, [key]: true }
  try {
    const activeItem =
      playerStore.currentOnlineItem?.key === key ? playerStore.currentOnlineItem : null
    await downloadOnlineTrack(tr, {
      resolved: activeItem?.resolved,
      resolvedAtMs: activeItem?.resolvedAtMs,
      quality: activeItem?.selectedQuality,
    })
    snackbarStore.show(t('online.downloadStartedToast', { title: tr.title }))
  } catch (err) {
    snackbarStore.show({
      message: t('online.downloadFailedToast', {
        message: err instanceof Error ? err.message : String(err),
      }),
      tone: 'error',
    })
  } finally {
    const next = { ...downloadingKeys.value }
    delete next[key]
    downloadingKeys.value = next
  }
}

function toTrackOnlineInput(tr: OnlineMusicTrackDto): TrackOnlineMusicInput {
  return {
    source_id: tr.source_id,
    track_id: tr.track_id,
    title: tr.title,
    artist: tr.artist,
    album: tr.album,
    duration_secs: tr.duration_secs,
    cover_url: tr.cover_url,
    cover_id: tr.cover_id,
    url_id: tr.url_id,
    lyric_id: tr.lyric_id,
    play_url: tr.play_url,
    detail_url: tr.detail_url,
    format: tr.format,
    quality: tr.qualities?.[0] ?? null,
  }
}

async function handleTrackToLibrary(tr: OnlineMusicTrackDto): Promise<void> {
  const key = trackKey(tr)
  if (trackingBusy.value[key]) return
  trackingBusy.value = { ...trackingBusy.value, [key]: true }
  try {
    await onlineMusicApi.trackOnlineTrack(toTrackOnlineInput(tr))
    trackedKeys.value = { ...trackedKeys.value, [key]: true }
    musicStore.bumpCatalogVersion()
    void musicStore.refreshSidebar()
    snackbarStore.show(t('online.trackedToast', { title: tr.title }))
  } catch (err) {
    snackbarStore.show({
      message: err instanceof Error ? err.message : String(err),
      tone: 'error',
    })
  } finally {
    const next = { ...trackingBusy.value }
    delete next[key]
    trackingBusy.value = next
  }
}

async function handleTrackAllToLibrary(): Promise<void> {
  if (batchTracking.value || props.tracks.length === 0) return
  batchTracking.value = true
  try {
    const created = await onlineMusicApi.trackOnlineTracksBatch(
      props.tracks.map((tr) => toTrackOnlineInput(tr)),
    )
    const nextTracked = { ...trackedKeys.value }
    for (const tr of props.tracks) {
      nextTracked[trackKey(tr)] = true
    }
    trackedKeys.value = nextTracked
    musicStore.bumpCatalogVersion()
    void musicStore.refreshSidebar()
    snackbarStore.show(t('online.trackedBatchToast', { count: created.length }))
  } catch (err) {
    snackbarStore.show({
      message: err instanceof Error ? err.message : String(err),
      tone: 'error',
    })
  } finally {
    batchTracking.value = false
  }
}
</script>

<template>
  <div class="overflow-hidden rounded-corner-lg bg-surface-container-low">
    <div
      v-if="tracks.length > 1"
      class="flex items-center justify-end border-b border-outline-variant/50 px-3 py-2"
    >
      <MdButton
        variant="tonal"
        size="sm"
        :icon="LuLibrary"
        :loading="batchTracking"
        :disabled="batchTracking"
        @click="handleTrackAllToLibrary"
      >
        {{
          batchTracking
            ? t('online.trackingAllToLibrary')
            : t('online.trackAllToLibrary', { count: tracks.length })
        }}
      </MdButton>
    </div>
    <table class="w-full table-fixed border-collapse text-left">
      <thead>
        <tr class="text-label-medium border-b border-outline-variant/60 text-on-surface-variant">
          <th class="w-11 px-2 py-3 text-center sm:px-3">{{ t('catalog.colIndex') }}</th>
          <th class="px-2 py-3 sm:px-3">{{ t('catalog.colTitle') }}</th>
          <th v-if="showAlbum" class="hidden w-40 px-3 py-3 lg:table-cell">
            {{ t('catalog.colAlbum') }}
          </th>
          <th v-if="showSource" class="hidden w-32 px-3 py-3 md:table-cell">
            {{ t('online.colSource') }}
          </th>
          <th class="hidden w-20 px-3 py-3 xl:table-cell">{{ t('catalog.colQuality') }}</th>
          <th class="hidden w-16 px-3 py-3 text-right sm:table-cell">
            {{ t('catalog.colDuration') }}
          </th>
          <th class="w-12 px-1.5 py-3 text-right sm:w-32 sm:px-3" />
        </tr>
      </thead>
      <tbody class="divide-y divide-outline-variant/35">
        <tr
          v-for="(tr, idx) in tracks"
          :key="`${trackKey(tr)}-${idx}`"
          class="group cursor-pointer transition-colors duration-150 ease-standard hover:bg-surface-container-high/60"
          :class="isCurrentTrack(tr) ? 'bg-secondary-container/50' : ''"
          @click="handlePlayRow(idx)"
        >
          <td class="w-11 px-2 py-2.5 text-center sm:px-3">
            <button
              type="button"
              :title="
                isCurrentTrack(tr) && playerStore.isPlaying ? t('player.pause') : t('player.play')
              "
              :aria-label="
                isCurrentTrack(tr) && playerStore.isPlaying ? t('player.pause') : t('player.play')
              "
              class="md-touch-target inline-flex h-8 w-8 cursor-pointer items-center justify-center rounded-corner-full"
              @click.stop="handlePlayRow(idx)"
            >
              <LuPause
                v-if="isCurrentTrack(tr) && playerStore.isPlaying"
                class="h-4 w-4 text-primary"
                aria-hidden="true"
              />
              <LuVolume2
                v-else-if="isCurrentTrack(tr)"
                class="h-4 w-4 text-primary"
                aria-hidden="true"
              />
              <template v-else>
                <span
                  class="text-body-small tabular-nums text-on-surface-variant group-focus-within:hidden group-hover:hidden"
                >
                  {{ idx + 1 }}
                </span>
                <LuPlay
                  class="hidden h-4 w-4 fill-current text-on-surface group-focus-within:block group-hover:block"
                  aria-hidden="true"
                />
              </template>
            </button>
          </td>

          <td class="px-2 py-2.5 sm:px-3">
            <div class="flex min-w-0 items-center gap-3">
              <MediaCoverImage
                :src="resolveCoverSrc(tr)"
                :alt="tr.title"
                aspect-class="h-10 w-10 shrink-0"
                rounded="rounded-corner-sm"
                bg-class="bg-surface-container-high"
                :fallback-icon="LuDisc3"
                fallback-icon-class="h-4 w-4 text-on-surface-variant/60"
              />
              <div class="min-w-0 flex-1 overflow-hidden">
                <div class="flex min-w-0 items-center gap-1.5">
                  <button
                    type="button"
                    class="text-title-small min-w-0 cursor-pointer truncate text-left transition-colors duration-150 hover:text-primary"
                    :class="isCurrentTrack(tr) ? 'text-primary' : 'text-on-surface'"
                    :title="tr.title"
                    @click.stop="handlePlayRow(idx)"
                  >
                    {{ tr.title }}
                  </button>
                  <span
                    v-if="tr.lyric_id"
                    class="text-label-small shrink-0 rounded-corner-xs bg-primary-container px-1.5 py-0.5 text-on-primary-container"
                  >
                    LRC
                  </span>
                </div>
                <div class="text-body-small mt-0.5 flex min-w-0 items-center gap-1.5 text-on-surface-variant">
                  <span class="min-w-0 truncate">{{ tr.artist || t('online.unknownArtist') }}</span>
                  <span
                    v-if="showSource"
                    class="text-label-small max-w-28 shrink-0 truncate rounded-corner-xs bg-surface-container-high px-1.5 py-0.5 md:hidden"
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
              class="text-body-small block truncate text-on-surface-variant"
              :title="tr.album || undefined"
            >
              {{ tr.album || '—' }}
            </span>
          </td>

          <td v-if="showSource" class="hidden w-32 px-3 py-2.5 md:table-cell">
            <span
              class="text-label-small inline-flex max-w-full items-center truncate rounded-corner-xs bg-surface-container-high px-2 py-0.5 text-on-surface-variant"
              :title="tr.source_name"
            >
              <span class="truncate">{{ tr.source_name }}</span>
            </span>
          </td>

          <td class="hidden w-20 px-3 py-2.5 xl:table-cell">
            <span
              class="text-label-small inline-flex max-w-full items-center truncate rounded-corner-xs bg-surface-container-high px-1.5 py-0.5 font-mono text-on-surface-variant"
            >
              {{ formatOnlineQuality(tr) }}
            </span>
          </td>

          <td
            class="text-body-small hidden w-16 px-3 py-2.5 text-right font-mono tabular-nums text-on-surface-variant sm:table-cell"
          >
            {{ tr.duration_secs != null && tr.duration_secs > 0 ? formatDuration(tr.duration_secs) : '—' }}
          </td>

          <td class="w-12 px-1.5 py-2.5 text-right sm:w-32 sm:px-3" @click.stop>
            <div class="inline-flex items-center justify-end gap-0.5">
              <span class="hidden sm:inline-flex">
                <MdIconButton
                  size="sm"
                  variant="standard"
                  :selected="Boolean(trackedKeys[trackKey(tr)])"
                  :loading="Boolean(trackingBusy[trackKey(tr)])"
                  :aria-label="
                    trackedKeys[trackKey(tr)] ? t('online.trackedInLibrary') : t('online.trackToLibrary')
                  "
                  :tooltip="
                    trackedKeys[trackKey(tr)] ? t('online.trackedInLibrary') : t('online.trackToLibrary')
                  "
                  @click="handleTrackToLibrary(tr)"
                >
                  <LuCheck v-if="trackedKeys[trackKey(tr)]" class="h-4 w-4 text-primary" />
                  <LuLibrary v-else class="h-4 w-4" />
                </MdIconButton>
              </span>

              <span class="hidden sm:inline-flex">
                <MdIconButton
                  size="sm"
                  variant="standard"
                  :selected="playerStore.isOnlineTrackInQueue(tr)"
                  :aria-label="
                    playerStore.isOnlineTrackInQueue(tr) ? t('online.enqueued') : t('online.enqueue')
                  "
                  :tooltip="
                    playerStore.isOnlineTrackInQueue(tr) ? t('online.enqueued') : t('online.enqueue')
                  "
                  @click="handleEnqueueTrack(tr)"
                >
                  <LuCheck v-if="playerStore.isOnlineTrackInQueue(tr)" class="h-4 w-4" />
                  <LuListPlus v-else class="h-4 w-4" />
                </MdIconButton>
              </span>

              <MdMenu placement="bottom-end" :min-width="208" :aria-label="t('common.moreActions')">
                <template #trigger="{ toggle, triggerAttrs }">
                  <MdIconButton
                    v-bind="triggerAttrs"
                    size="sm"
                    variant="standard"
                    :icon="LuMoreHorizontal"
                    :aria-label="t('common.moreActions')"
                    @click="toggle"
                  />
                </template>

                <div class="sm:hidden">
                  <MdMenuItem
                    :icon="trackedKeys[trackKey(tr)] ? LuCheck : LuLibrary"
                    :label="
                      trackedKeys[trackKey(tr)]
                        ? t('online.trackedInLibrary')
                        : t('online.trackToLibrary')
                    "
                    :disabled="Boolean(trackingBusy[trackKey(tr)])"
                    @select="handleTrackToLibrary(tr)"
                  />
                  <MdMenuItem
                    :icon="playerStore.isOnlineTrackInQueue(tr) ? LuCheck : LuListPlus"
                    :label="
                      playerStore.isOnlineTrackInQueue(tr)
                        ? t('online.enqueued')
                        : t('online.enqueue')
                    "
                    :disabled="playerStore.isOnlineTrackInQueue(tr)"
                    @select="handleEnqueueTrack(tr)"
                  />
                </div>
                <MdMenuItem
                  :icon="LuPlay"
                  :label="t('online.playNext')"
                  @select="handlePlayNext(tr)"
                />
                <MdMenuItem
                  :icon="LuDownload"
                  :label="t('common.download')"
                  :disabled="Boolean(downloadingKeys[trackKey(tr)])"
                  @select="handleDownloadTrack(tr)"
                />
              </MdMenu>
            </div>
          </td>
        </tr>
      </tbody>
    </table>
  </div>
</template>

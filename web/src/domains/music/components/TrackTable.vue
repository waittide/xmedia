<script setup lang="ts">
import { computed, ref } from 'vue'
import { useRouter } from 'vue-router'
import { useI18n as useGlobalI18n } from 'vue-i18n'
import {
  LuCloud,
  LuDisc3,
  LuDownload,
  LuHeart,
  LuListPlus,
  LuMoreHorizontal,
  LuPause,
  LuPlay,
  LuPlus,
  LuTag,
  LuTrash2,
  LuVolume2,
} from 'vue-icons-plus/lu'
import { MdIconButton, MdMenu, MdMenuItem, MediaCoverImage } from '@/components/ui'
import { useAuthStore } from '@/stores/auth'
import { useDialogStore } from '@/stores/dialog'
import { useSnackbarStore } from '@/stores/snackbar'
import { formatTime as formatDuration } from '@/utils/format'
import { api, buildCoverUrl } from '../api'
import { downloadLocalTrack } from '../download'
import { useMusicI18n as useI18n } from '../i18n'
import { onlineMusicApi } from '../onlineApi'
import { usePlayerStore } from '../player'
import { useMusicStore } from '../store'
import type { TrackDto } from '../types'

const MAX_INLINE_TAGS = 2

const props = withDefaults(
  defineProps<{
    tracks: TrackDto[]
    showAlbum?: boolean
    showCover?: boolean
    removable?: boolean
    /** Render inline disc header rows when tracks span multiple discs while keeping a single unified queue. */
    groupByDisc?: boolean
  }>(),
  {
    showAlbum: true,
    showCover: true,
    removable: false,
    groupByDisc: false,
  },
)

const emit = defineEmits<{
  remove: [index: number]
  'tags-updated': [payload: { trackId: string; tags: string[] }]
}>()

const { t } = useI18n()
const { t: globalT } = useGlobalI18n()
const router = useRouter()
const authStore = useAuthStore()
const playerStore = usePlayerStore()
const musicStore = useMusicStore()
const dialogStore = useDialogStore()
const snackbarStore = useSnackbarStore()
const cachingTrackIds = ref<Set<string>>(new Set())
const cachedTrackOverrides = ref<Record<string, boolean>>({})

function isTrackedCached(tr: TrackDto): boolean {
  return Boolean(cachedTrackOverrides.value[tr.id] || tr.online_tracking?.cached)
}

type TableEntry =
  | { kind: 'disc'; disc: number }
  | { kind: 'track'; track: TrackDto; index: number }

const tableEntries = computed<TableEntry[]>(() => {
  if (!props.groupByDisc) {
    return props.tracks.map((track, index) => ({ kind: 'track', track, index }))
  }
  const discNums = new Set(props.tracks.map((tr) => tr.disc_number ?? 1))
  if (discNums.size <= 1) {
    return props.tracks.map((track, index) => ({ kind: 'track', track, index }))
  }
  const out: TableEntry[] = []
  let prevDisc: number | null = null
  props.tracks.forEach((track, index) => {
    const disc = track.disc_number ?? 1
    if (disc !== prevDisc) {
      out.push({ kind: 'disc', disc })
      prevDisc = disc
    }
    out.push({ kind: 'track', track, index })
  })
  return out
})

const columnCount = computed(() => (props.showAlbum ? 6 : 5))

function isCurrentTrack(tr: TrackDto): boolean {
  return playerStore.currentTrack?.kind === 'local' && playerStore.currentTrack.id === tr.id
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

function visibleTags(tr: TrackDto): string[] {
  return (tr.tags ?? []).slice(0, MAX_INLINE_TAGS)
}

function overflowTagCount(tr: TrackDto): number {
  return Math.max(0, (tr.tags?.length ?? 0) - MAX_INLINE_TAGS)
}

function handlePlayRow(idx: number): void {
  const clicked = props.tracks[idx]
  if (!clicked) return
  if (isCurrentTrack(clicked)) {
    void playerStore.togglePlay()
  } else {
    playerStore.playQueue(props.tracks, idx)
  }
}

async function handleToggleStar(track: TrackDto): Promise<void> {
  try {
    await musicStore.toggleTrackStar(track)
  } catch (err) {
    snackbarStore.show({
      message: err instanceof Error ? err.message : String(err),
      tone: 'error',
    })
  }
}

function handleEnqueueNext(track: TrackDto): void {
  playerStore.enqueueNext(track)
  snackbarStore.show(t('online.playNextToast'))
}

async function createPlaylistWithTrack(track: TrackDto): Promise<void> {
  const name = await dialogStore.prompt({
    title: t('playlist.createTitle'),
    message: t('playlist.createPrompt'),
    placeholder: t('playlist.namePlaceholder'),
  })
  if (!name || !name.trim()) return
  try {
    await api.createPlaylist({ name: name.trim(), track_ids: [track.id] })
    await musicStore.refreshSidebar()
  } catch (err) {
    snackbarStore.show({
      message: err instanceof Error ? err.message : String(err),
      tone: 'error',
    })
  }
}

async function addToExistingPlaylist(playlistId: string, track: TrackDto): Promise<void> {
  try {
    await musicStore.addTrackToPlaylist(playlistId, track.id)
    snackbarStore.show(t('common.addedToPlaylist'))
  } catch (err) {
    snackbarStore.show({
      message: err instanceof Error ? err.message : String(err),
      tone: 'error',
    })
  }
}

async function editTrackTags(track: TrackDto): Promise<void> {
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
  try {
    const updated = await api.setTrackTags(track.id, nextTags)
    emit('tags-updated', { trackId: track.id, tags: updated.tags })
    musicStore.bumpCatalogVersion()
    await musicStore.refreshSidebar()
  } catch (err) {
    snackbarStore.show({
      message: err instanceof Error ? err.message : String(err),
      tone: 'error',
    })
  }
}

async function confirmUnbindTrack(track: TrackDto): Promise<void> {
  const confirmed = await dialogStore.confirm({
    title: globalT('pool.unbindConfirmTitle'),
    message: track.online_tracking
      ? globalT('pool.unbindOnlineConfirmMessage', { title: track.title })
      : globalT('pool.unbindConfirmMessage', { title: track.title }),
    confirmText: globalT('pool.unbindBtn'),
    danger: true,
  })
  if (!confirmed) return
  try {
    await api.unbindItem(track.id)
    playerStore.removeByTrackId(track.id)
    musicStore.bumpCatalogVersion()
    await musicStore.refreshSidebar()
    snackbarStore.show({ message: globalT('pool.unboundToast') })
  } catch (err) {
    snackbarStore.show({
      message: err instanceof Error ? err.message : String(err),
      tone: 'error',
    })
  }
}

async function confirmDeleteTrack(track: TrackDto): Promise<void> {
  const confirmed = await dialogStore.confirm({
    title: t('common.deleteTrackConfirmTitle'),
    message: t('common.deleteTrackConfirm', { title: track.title }),
    danger: true,
  })
  if (!confirmed) return
  try {
    await api.deleteTrack(track.id)
    playerStore.removeByTrackId(track.id)
    musicStore.bumpCatalogVersion()
    await musicStore.refreshSidebar()
  } catch (err) {
    snackbarStore.show({
      message: err instanceof Error ? err.message : String(err),
      tone: 'error',
    })
  }
}

async function handleCacheTrackedTrack(track: TrackDto): Promise<void> {
  if (cachingTrackIds.value.has(track.id)) return
  const next = new Set(cachingTrackIds.value)
  next.add(track.id)
  cachingTrackIds.value = next
  snackbarStore.show(t('online.cachingTrackToast', { title: track.title }))
  try {
    await onlineMusicApi.cacheTrackedTrack(track.id)
    cachedTrackOverrides.value = { ...cachedTrackOverrides.value, [track.id]: true }
    musicStore.bumpCatalogVersion()
    void musicStore.refreshSidebar()
    snackbarStore.show(t('online.trackCachedToast', { title: track.title }))
  } catch (err) {
    snackbarStore.show({
      message: err instanceof Error ? err.message : String(err),
      tone: 'error',
    })
  } finally {
    const done = new Set(cachingTrackIds.value)
    done.delete(track.id)
    cachingTrackIds.value = done
  }
}
</script>

<template>
  <div class="overflow-hidden rounded-corner-lg bg-surface-container-low">
    <table class="w-full table-fixed border-collapse text-left">
      <thead>
        <tr class="text-label-medium border-b border-outline-variant/60 text-on-surface-variant">
          <th class="w-11 px-2 py-3 text-center sm:px-3">{{ t('catalog.colIndex') }}</th>
          <th class="px-2 py-3 sm:px-3">{{ t('catalog.colTitle') }}</th>
          <th v-if="showAlbum" class="hidden w-44 px-3 py-3 lg:table-cell">
            {{ t('catalog.colAlbum') }}
          </th>
          <th class="hidden w-28 px-3 py-3 xl:table-cell">{{ t('catalog.colQuality') }}</th>
          <th class="hidden w-16 px-3 py-3 text-right sm:table-cell">
            {{ t('catalog.colDuration') }}
          </th>
          <th class="w-20 px-2 py-3 text-right sm:w-24 sm:px-3" />
        </tr>
      </thead>
      <tbody class="divide-y divide-outline-variant/35">
        <template
          v-for="entry in tableEntries"
          :key="entry.kind === 'disc' ? `disc-${entry.disc}` : `${entry.track.id}-${entry.index}`"
        >
          <tr v-if="entry.kind === 'disc'" class="bg-surface-container/70">
            <td :colspan="columnCount" class="text-label-large px-4 py-2 text-on-surface-variant">
              {{ t('catalog.discHeader', { disc: entry.disc }) }}
            </td>
          </tr>

          <tr
            v-else
            class="group cursor-pointer transition-colors duration-150 ease-standard hover:bg-surface-container-high/60"
            :class="isCurrentTrack(entry.track) ? 'bg-secondary-container/50' : ''"
            @click="handlePlayRow(entry.index)"
          >
            <td class="w-11 px-2 py-2.5 text-center sm:px-3">
              <button
                type="button"
                :aria-label="
                  isCurrentTrack(entry.track) && playerStore.isPlaying
                    ? t('player.pause')
                    : t('player.play')
                "
                class="md-touch-target inline-flex h-8 w-8 cursor-pointer items-center justify-center rounded-corner-full"
                @click.stop="handlePlayRow(entry.index)"
              >
                <LuPause
                  v-if="isCurrentTrack(entry.track) && playerStore.isPlaying"
                  class="h-4 w-4 text-primary"
                  aria-hidden="true"
                />
                <LuVolume2
                  v-else-if="isCurrentTrack(entry.track)"
                  class="h-4 w-4 text-primary"
                  aria-hidden="true"
                />
                <template v-else>
                  <span
                    class="text-body-small tabular-nums text-on-surface-variant group-focus-within:hidden group-hover:hidden"
                  >
                    {{ entry.track.track_number ?? entry.index + 1 }}
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
                  v-if="showCover"
                  :src="entry.track.cover_key ? buildCoverUrl(entry.track.id, 96) : null"
                  :alt="entry.track.title"
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
                      :class="isCurrentTrack(entry.track) ? 'text-primary' : 'text-on-surface'"
                      :title="entry.track.title"
                      @click.stop="handlePlayRow(entry.index)"
                    >
                      {{ entry.track.title }}
                    </button>
                    <span
                      v-if="entry.track.has_lyrics"
                      class="text-label-small shrink-0 rounded-corner-xs bg-primary-container px-1.5 py-0.5 text-on-primary-container"
                    >
                      LRC
                    </span>
                    <span
                      v-if="entry.track.online_tracking"
                      class="text-label-small shrink-0 rounded-corner-xs px-1.5 py-0.5"
                      :class="
                        isTrackedCached(entry.track)
                          ? 'bg-secondary-container text-on-secondary-container'
                          : 'bg-tertiary-container text-on-tertiary-container'
                      "
                    >
                      {{
                        isTrackedCached(entry.track)
                          ? t('online.badgeTrackedCached')
                          : t('online.badgeTrackedOnline')
                      }}
                    </span>
                  </div>
                  <div class="text-body-small mt-0.5 flex min-w-0 items-center gap-1.5 text-on-surface-variant">
                    <button
                      type="button"
                      class="min-w-0 cursor-pointer truncate text-left hover:text-primary hover:underline"
                      @click.stop="
                        router.push({ path: '/music/library', query: { artist: entry.track.artist_id } })
                      "
                    >
                      {{ entry.track.artist_name }}
                    </button>
                    <button
                      v-for="tg in visibleTags(entry.track)"
                      :key="tg"
                      type="button"
                      class="text-label-small hidden shrink-0 cursor-pointer rounded-corner-xs bg-surface-container-high px-1.5 py-0.5 text-on-surface-variant transition-colors hover:bg-secondary-container hover:text-on-secondary-container sm:inline-flex"
                      @click.stop="
                        router.push({ path: '/music/library', query: { view: 'tracks', tag: tg } })
                      "
                    >
                      #{{ tg }}
                    </button>
                    <span
                      v-if="overflowTagCount(entry.track) > 0"
                      class="text-label-small hidden shrink-0 text-on-surface-variant sm:inline"
                    >
                      +{{ overflowTagCount(entry.track) }}
                    </span>
                  </div>
                </div>
              </div>
            </td>

            <td v-if="showAlbum" class="hidden w-44 px-3 py-2.5 lg:table-cell">
              <button
                type="button"
                class="text-body-small block max-w-full cursor-pointer truncate text-left text-on-surface-variant hover:text-primary hover:underline"
                :title="entry.track.album_title"
                @click.stop="router.push(`/music/item/${encodeURIComponent(entry.track.album_id)}`)"
              >
                {{ entry.track.album_title }}
              </button>
            </td>

            <td class="hidden w-28 px-3 py-2.5 xl:table-cell">
              <span
                class="text-label-small inline-flex max-w-full items-center truncate rounded-corner-xs px-1.5 py-0.5 font-mono"
                :class="
                  entry.track.is_lossless
                    ? 'bg-primary-container text-on-primary-container'
                    : 'bg-surface-container-high text-on-surface-variant'
                "
              >
                {{ formatQuality(entry.track) }}
              </span>
            </td>

            <td
              class="text-body-small hidden w-16 px-3 py-2.5 text-right font-mono tabular-nums text-on-surface-variant sm:table-cell"
            >
              {{ formatDuration(entry.track.duration_secs) }}
            </td>

            <td class="w-20 px-2 py-2.5 text-right sm:w-24 sm:px-3" @click.stop>
              <div class="inline-flex items-center justify-end gap-0.5">
                <MdIconButton
                  size="sm"
                  variant="standard"
                  :selected="Boolean(entry.track.starred_at)"
                  :aria-label="
                    entry.track.starred_at ? t('common.unfavorite') : t('common.favorite')
                  "
                  @click="handleToggleStar(entry.track)"
                >
                  <LuHeart
                    class="h-4 w-4"
                    :class="entry.track.starred_at ? 'fill-current text-tertiary' : ''"
                  />
                </MdIconButton>

                <MdMenu placement="bottom-end" :min-width="220" :aria-label="t('common.moreActions')">
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

                  <MdMenuItem
                    :icon="LuListPlus"
                    :label="t('common.addToQueue')"
                    @select="handleEnqueueNext(entry.track)"
                  />

                  <MdMenuItem
                    v-if="authStore.isAdmin"
                    :icon="LuTag"
                    :label="globalT('musicTags.editTags')"
                    @select="editTrackTags(entry.track)"
                  />

                  <MdMenuItem
                    :icon="LuPlus"
                    :label="t('nav.newPlaylist')"
                    @select="createPlaylistWithTrack(entry.track)"
                  />

                  <div
                    v-if="musicStore.playlists.length > 0"
                    class="my-1 max-h-36 overflow-y-auto border-t border-outline-variant pt-1"
                  >
                    <MdMenuItem
                      v-for="pl in musicStore.playlists"
                      :key="pl.id"
                      :label="`+ ${pl.name}`"
                      :trailing-text="String(pl.track_count)"
                      @select="addToExistingPlaylist(pl.id, entry.track)"
                    />
                  </div>

                  <MdMenuItem
                    v-if="entry.track.online_tracking && !isTrackedCached(entry.track)"
                    :icon="LuCloud"
                    :label="t('online.cacheTrackBtn')"
                    :disabled="cachingTrackIds.has(entry.track.id)"
                    @select="handleCacheTrackedTrack(entry.track)"
                  />

                  <MdMenuItem
                    :icon="LuDownload"
                    :label="t('common.download')"
                    @select="downloadLocalTrack(entry.track.id)"
                  />

                  <div class="my-1 border-t border-outline-variant pt-1">
                    <MdMenuItem
                      v-if="removable"
                      :icon="LuTrash2"
                      :label="t('playlist.removeTrack')"
                      destructive
                      @select="emit('remove', entry.index)"
                    />
                    <MdMenuItem
                      :icon="LuTrash2"
                      :label="globalT('pool.unbindBtn')"
                      destructive
                      @select="confirmUnbindTrack(entry.track)"
                    />
                    <MdMenuItem
                      v-if="authStore.isAdmin"
                      :icon="LuTrash2"
                      :label="t('common.delete')"
                      destructive
                      @select="confirmDeleteTrack(entry.track)"
                    />
                  </div>
                </MdMenu>
              </div>
            </td>
          </tr>
        </template>
      </tbody>
    </table>
  </div>
</template>

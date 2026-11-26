<script setup lang="ts">
import { computed, onMounted, ref, watch } from 'vue'
import { useRouter } from 'vue-router'
import { useI18n as useGlobalI18n } from 'vue-i18n'
import {
  LuCheck,
  LuCloud,
  LuDisc3,
  LuDownload,
  LuListPlus,
  LuLoader2,
  LuPause,
  LuPlay,
  LuRefreshCw,
  LuSettings2,
  LuShuffle,
  LuVolume2,
} from 'vue-icons-plus/lu'
import {
  appHostApi,
  type SubsonicAlbumDto,
  type SubsonicAlbumListType,
  type SubsonicSongDto,
} from '@/api/appHost'
import { MediaErrorState, PaginationBar } from '@/components/domain'
import {
  MdButton,
  MdChip,
  MdEmptyState,
  MdIconButton,
  MdSegmentedButton,
  MdSheet,
  MdSkeleton,
  MediaCoverImage,
  type SegmentedOption,
} from '@/components/ui'
import { useAppClientStore } from '@/stores/appServers'
import { useSnackbarStore } from '@/stores/snackbar'
import { useUploadStore } from '@/stores/uploads'
import { formatTime as formatDuration } from '@/utils/format'
import { useMusicI18n as useI18n } from '../i18n'
import { buildRemoteSongKey, usePlayerStore } from '../player'

const ALBUM_PAGE_SIZE = 30

const props = defineProps<{
  keyword: string
  selectedSourceId: string | null
}>()

const emit = defineEmits<{
  'update:sourceId': [id: string | null]
}>()

const { t } = useI18n()
const { t: globalT } = useGlobalI18n()
const router = useRouter()
const appClientStore = useAppClientStore()
const snackbarStore = useSnackbarStore()
const uploadStore = useUploadStore()
const playerStore = usePlayerStore()

const subsonicSources = computed(() => appClientStore.subsonicSources)

const activeSourceId = computed<string>(() => {
  if (props.selectedSourceId && subsonicSources.value.some((s) => s.id === props.selectedSourceId)) {
    return props.selectedSourceId
  }
  return subsonicSources.value[0]?.id ?? ''
})

const activeSourceName = computed(() => {
  const src = subsonicSources.value.find((s) => s.id === activeSourceId.value)
  return src?.name || src?.url || ''
})

const albumSortType = ref<SubsonicAlbumListType>('newest')
const albums = ref<SubsonicAlbumDto[]>([])
const hasMore = ref(false)
const loading = ref(false)
const loadingMore = ref(false)
const error = ref<string | null>(null)

const searchActive = computed(() => props.keyword.trim().length > 0)
const searchAlbums = ref<SubsonicAlbumDto[]>([])
const searchSongs = ref<SubsonicSongDto[]>([])

const albumSheetOpen = ref(false)
const activeAlbum = ref<SubsonicAlbumDto | null>(null)
const albumSongs = ref<SubsonicSongDto[]>([])
const albumDetailLoading = ref(false)
const albumDetailError = ref<string | null>(null)

const importingIds = ref<Record<string, boolean>>({})
const importedIds = ref<Record<string, boolean>>({})

let listSeq = 0
let detailSeq = 0

const sortOptions = computed<SegmentedOption<SubsonicAlbumListType>[]>(() => [
  { value: 'newest', label: globalT('remote.subsonicSortNewest') },
  { value: 'random', label: globalT('remote.subsonicSortRandom') },
  { value: 'alphabeticalByName', label: globalT('remote.subsonicSortName') },
])

function isSongCurrent(song: SubsonicSongDto): boolean {
  return (
    playerStore.currentTrack?.kind === 'remote' &&
    playerStore.currentTrack.id === buildRemoteSongKey(activeSourceId.value, song.id)
  )
}

function importKey(songId: string): string {
  return `${activeSourceId.value}::${songId}`
}

function buildSongFilename(song: SubsonicSongDto): string {
  const parts = [song.artist, song.title].filter(Boolean).join(' - ') || 'track'
  const safeBase = parts.replace(/[\\/:*?"<>|]+/g, '_').trim().slice(0, 120)
  const ext = song.suffix?.replace(/^\.+/, '').toLowerCase()
  return ext ? `${safeBase}.${ext}` : safeBase
}

async function fetchAlbums(reset = true): Promise<void> {
  const sourceId = activeSourceId.value
  if (!sourceId) {
    albums.value = []
    hasMore.value = false
    return
  }
  const seq = ++listSeq
  const offset = reset ? 0 : albums.value.length
  if (reset) {
    loading.value = true
    error.value = null
  } else {
    loadingMore.value = true
  }
  try {
    const res = await appHostApi.listSubsonicAlbums(sourceId, {
      type: albumSortType.value,
      size: ALBUM_PAGE_SIZE,
      offset,
    })
    if (seq !== listSeq) return
    albums.value = reset ? res.albums : [...albums.value, ...res.albums]
    hasMore.value = res.albums.length >= ALBUM_PAGE_SIZE
  } catch (err) {
    if (seq !== listSeq) return
    error.value = err instanceof Error ? err.message : String(err)
  } finally {
    if (seq === listSeq) {
      loading.value = false
      loadingMore.value = false
    }
  }
}

async function runSearch(): Promise<void> {
  const sourceId = activeSourceId.value
  const q = props.keyword.trim()
  if (!sourceId || !q) return
  const seq = ++listSeq
  loading.value = true
  error.value = null
  try {
    const res = await appHostApi.searchSubsonic(sourceId, q)
    if (seq !== listSeq) return
    searchAlbums.value = res.albums
    searchSongs.value = res.songs
  } catch (err) {
    if (seq !== listSeq) return
    error.value = err instanceof Error ? err.message : String(err)
  } finally {
    if (seq === listSeq) {
      loading.value = false
    }
  }
}

function reloadCurrent(): void {
  if (searchActive.value) {
    void runSearch()
  } else {
    void fetchAlbums(true)
  }
}

async function openAlbum(album: SubsonicAlbumDto): Promise<void> {
  const sourceId = activeSourceId.value
  if (!sourceId) return
  activeAlbum.value = album
  albumSongs.value = []
  albumDetailError.value = null
  albumSheetOpen.value = true
  albumDetailLoading.value = true
  const seq = ++detailSeq
  try {
    const res = await appHostApi.getSubsonicAlbum(sourceId, album.id)
    if (seq !== detailSeq) return
    activeAlbum.value = res.album
    albumSongs.value = res.songs
  } catch (err) {
    if (seq !== detailSeq) return
    albumDetailError.value = err instanceof Error ? err.message : String(err)
  } finally {
    if (seq === detailSeq) {
      albumDetailLoading.value = false
    }
  }
}

function playSong(song: SubsonicSongDto, list: SubsonicSongDto[]): void {
  if (!activeSourceId.value) return
  if (isSongCurrent(song)) {
    void playerStore.togglePlay()
    return
  }
  const idx = list.findIndex((s) => s.id === song.id)
  playerStore.playRemoteSongs(activeSourceId.value, activeSourceName.value, list, Math.max(0, idx))
}

function enqueueSong(song: SubsonicSongDto): void {
  if (!activeSourceId.value) return
  if (playerStore.isRemoteSongInQueue(activeSourceId.value, song.id)) return
  playerStore.enqueueRemoteSong(activeSourceId.value, activeSourceName.value, song)
  snackbarStore.show(t('online.queuedToast'))
}

async function handleImportSong(song: SubsonicSongDto): Promise<void> {
  const sourceId = activeSourceId.value
  if (!sourceId) return
  const key = importKey(song.id)
  if (importingIds.value[key]) return
  importingIds.value = { ...importingIds.value, [key]: true }
  try {
    const res = await appHostApi.importSubsonicSong(sourceId, song.id, buildSongFilename(song))
    importedIds.value = { ...importedIds.value, [key]: true }
    uploadStore.notifyCatalogChanged('music')
    snackbarStore.show({
      message: globalT('remote.importSuccess', { title: res.title || song.title }),
      actionLabel: globalT('remote.viewInLibrary'),
      onAction: () => {
        void router.push('/music/library?view=tracks')
      },
    })
  } catch (err) {
    snackbarStore.show({
      message: globalT('remote.importFailed', {
        message: err instanceof Error ? err.message : String(err),
      }),
      tone: 'error',
    })
  } finally {
    const next = { ...importingIds.value }
    delete next[key]
    importingIds.value = next
  }
}

watch([activeSourceId, albumSortType, () => props.keyword], () => {
  albumSheetOpen.value = false
  reloadCurrent()
})

onMounted(() => {
  reloadCurrent()
})

defineExpose({
  reload: reloadCurrent,
})
</script>

<template>
  <div class="space-y-4">
    <!-- Empty Remote Sources -->
    <MdEmptyState
      v-if="subsonicSources.length === 0"
      :icon="LuCloud"
      :title="globalT('remote.subsonicEmptySourcesTitle')"
      :description="globalT('remote.subsonicEmptySourcesHint')"
    >
      <template #action>
        <MdButton
          variant="tonal"
          :icon="LuSettings2"
          :to="{ path: '/music/settings', query: { tab: 'sources', kind: 'remote' } }"
        >
          {{ t('common.manageSources') }}
        </MdButton>
      </template>
    </MdEmptyState>

    <template v-else>
      <!-- Remote Source Chips + Sort Controls -->
      <div class="flex flex-wrap items-center justify-between gap-2">
        <div class="flex flex-wrap items-center gap-1.5">
          <MdChip
            v-for="src in subsonicSources"
            :key="src.id"
            variant="filter"
            :icon="LuCloud"
            :label="src.name || globalT('remote.unnamed')"
            :selected="activeSourceId === src.id"
            @click="emit('update:sourceId', src.id)"
          />
        </div>

        <div class="flex items-center gap-2">
          <MdSegmentedButton
            v-if="!searchActive"
            v-model="albumSortType"
            :options="sortOptions"
            size="sm"
            :show-check="false"
          />
          <MdIconButton
            size="sm"
            variant="standard"
            :icon="LuRefreshCw"
            :loading="loading"
            :aria-label="globalT('common.refresh')"
            @click="reloadCurrent"
          />
        </div>
      </div>

      <!-- Error State -->
      <MediaErrorState
        v-if="error"
        :message="error"
        :retrying="loading"
        @retry="reloadCurrent"
      />

      <!-- Loading Skeleton -->
      <div
        v-else-if="loading"
        class="grid grid-cols-2 gap-5 sm:grid-cols-3 md:grid-cols-4 lg:grid-cols-5 xl:grid-cols-6"
        aria-busy="true"
      >
        <MdSkeleton v-for="idx in 12" :key="`sub-sk-${idx}`" variant="card" aspect="square" />
      </div>

      <!-- Search Results -->
      <template v-else-if="searchActive">
        <MdEmptyState
          v-if="searchSongs.length === 0 && searchAlbums.length === 0"
          compact
          :icon="LuDisc3"
          :title="t('common.noResults')"
        />

        <section v-if="searchSongs.length > 0" class="space-y-3">
          <div class="flex flex-wrap items-center justify-between gap-2">
            <h2 class="text-title-medium text-on-surface">
              {{ globalT('remote.subsonicSearchSongs') }}
            </h2>
            <div class="flex items-center gap-1.5">
              <MdButton
                variant="tonal"
                :icon="LuPlay"
                @click="playerStore.playRemoteSongs(activeSourceId, activeSourceName, searchSongs, 0)"
              >
                {{ t('common.playAllBtn') }}
              </MdButton>
              <MdButton
                variant="text"
                :icon="LuShuffle"
                @click="playerStore.shuffleAndPlayRemoteSongs(activeSourceId, activeSourceName, searchSongs)"
              >
                {{ t('common.shufflePlayBtn') }}
              </MdButton>
            </div>
          </div>

          <div class="space-y-1 overflow-hidden rounded-corner-lg bg-surface-container-low p-1.5">
            <div
              v-for="song in searchSongs"
              :key="song.id"
              class="flex cursor-pointer items-center gap-3 rounded-corner-md px-3.5 py-2.5 transition-colors hover:bg-surface-container-high/60"
              :class="isSongCurrent(song) ? 'bg-secondary-container/50' : ''"
              @click="playSong(song, searchSongs)"
            >
              <MdIconButton
                size="sm"
                variant="standard"
                :selected="isSongCurrent(song)"
                :aria-label="
                  isSongCurrent(song) && playerStore.isPlaying
                    ? t('player.pause')
                    : t('player.play')
                "
                @click.stop="playSong(song, searchSongs)"
              >
                <LuPause v-if="isSongCurrent(song) && playerStore.isPlaying" class="h-4 w-4 text-primary" />
                <LuVolume2 v-else-if="isSongCurrent(song)" class="h-4 w-4 text-primary" />
                <LuPlay v-else class="h-4 w-4 fill-current" />
              </MdIconButton>

              <div class="min-w-0 flex-1">
                <p
                  class="text-title-small truncate"
                  :class="isSongCurrent(song) ? 'text-primary' : 'text-on-surface'"
                >
                  {{ song.title }}
                </p>
                <p class="text-body-small truncate text-on-surface-variant">
                  {{ song.artist }}<span v-if="song.album"> · {{ song.album }}</span>
                </p>
              </div>

              <span
                v-if="song.duration"
                class="text-body-small hidden shrink-0 font-mono tabular-nums text-on-surface-variant sm:inline"
              >
                {{ formatDuration(song.duration) }}
              </span>

              <div class="flex shrink-0 items-center gap-2" @click.stop>
                <MdIconButton
                  size="sm"
                  variant="standard"
                  :selected="playerStore.isRemoteSongInQueue(activeSourceId, song.id)"
                  :aria-label="t('online.enqueue')"
                  @click="enqueueSong(song)"
                >
                  <LuCheck v-if="playerStore.isRemoteSongInQueue(activeSourceId, song.id)" class="h-4 w-4" />
                  <LuListPlus v-else class="h-4 w-4" />
                </MdIconButton>
                <MdIconButton
                  size="sm"
                  variant="standard"
                  :loading="Boolean(importingIds[importKey(song.id)])"
                  :aria-label="
                    importedIds[importKey(song.id)]
                      ? globalT('remote.importedBtn')
                      : globalT('remote.importBtn')
                  "
                  @click="handleImportSong(song)"
                >
                  <LuCheck v-if="importedIds[importKey(song.id)]" class="h-4 w-4 text-success" />
                  <LuDownload v-else class="h-4 w-4" />
                </MdIconButton>
              </div>
            </div>
          </div>
        </section>

        <section v-if="searchAlbums.length > 0" class="space-y-3">
          <h2 class="text-title-medium text-on-surface">
            {{ globalT('remote.subsonicSearchAlbums') }}
          </h2>
          <div class="grid grid-cols-2 gap-5 sm:grid-cols-3 md:grid-cols-4 lg:grid-cols-5 xl:grid-cols-6">
            <button
              v-for="album in searchAlbums"
              :key="album.id"
              type="button"
              class="group flex cursor-pointer flex-col text-left outline-none focus-visible:outline-3 focus-visible:outline-offset-2 focus-visible:outline-secondary"
              @click="openAlbum(album)"
            >
              <MediaCoverImage
                :src="album.cover_url"
                :alt="album.name"
                aspect-class="aspect-square w-full"
                rounded="rounded-corner-md"
                bg-class="bg-surface-container-high"
                :fallback-icon="LuDisc3"
                class="album-shadow transition-shadow duration-200 group-hover:shadow-elevation-1"
              />
              <p class="text-title-small mt-2 truncate text-on-surface group-hover:text-primary">
                {{ album.name }}
              </p>
              <p class="text-body-small truncate text-on-surface-variant">
                {{ album.artist }}
              </p>
            </button>
          </div>
        </section>
      </template>

      <!-- Album Grid -->
      <template v-else>
        <MdEmptyState
          v-if="albums.length === 0"
          compact
          :icon="LuDisc3"
          :title="globalT('remote.subsonicAlbumsEmpty')"
        />

        <template v-else>
          <div class="grid grid-cols-2 gap-5 sm:grid-cols-3 md:grid-cols-4 lg:grid-cols-5 xl:grid-cols-6">
            <button
              v-for="album in albums"
              :key="album.id"
              type="button"
              class="group flex cursor-pointer flex-col text-left outline-none focus-visible:outline-3 focus-visible:outline-offset-2 focus-visible:outline-secondary"
              @click="openAlbum(album)"
            >
              <MediaCoverImage
                :src="album.cover_url"
                :alt="album.name"
                aspect-class="aspect-square w-full"
                rounded="rounded-corner-md"
                bg-class="bg-surface-container-high"
                :fallback-icon="LuDisc3"
                class="album-shadow transition-shadow duration-200 group-hover:shadow-elevation-1"
              />
              <p class="text-title-small mt-2 truncate text-on-surface group-hover:text-primary">
                {{ album.name }}
              </p>
              <p class="text-body-small truncate text-on-surface-variant">
                {{ album.artist }}
              </p>
            </button>
          </div>

          <PaginationBar
            mode="load-more"
            :has-more="hasMore"
            :loading="loadingMore"
            @load-more="fetchAlbums(false)"
          />
        </template>
      </template>
    </template>

    <!-- Subsonic Album Track Sheet -->
    <MdSheet
      :open="albumSheetOpen"
      :title="activeAlbum?.name ?? ''"
      :subtitle="activeAlbum?.artist ?? ''"
      size="md"
      @close="albumSheetOpen = false"
    >
      <template v-if="albumSongs.length > 0" #subheader>
        <div class="flex items-center gap-2 rounded-corner-md bg-surface-container-low px-3 py-2">
          <MdButton
            variant="filled"
            :icon="LuPlay"
            @click="playerStore.playRemoteSongs(activeSourceId, activeSourceName, albumSongs, 0)"
          >
            {{ t('common.playAllBtn') }}
          </MdButton>
          <MdButton
            variant="tonal"
            :icon="LuShuffle"
            @click="playerStore.shuffleAndPlayRemoteSongs(activeSourceId, activeSourceName, albumSongs)"
          >
            {{ t('common.shufflePlayBtn') }}
          </MdButton>
        </div>
      </template>

      <MediaErrorState
        v-if="albumDetailError"
        compact
        :message="albumDetailError"
        @retry="activeAlbum && openAlbum(activeAlbum)"
      />

      <div v-else-if="albumDetailLoading" class="flex items-center justify-center py-12 text-on-surface-variant">
        <LuLoader2 class="mr-2 h-4 w-4 animate-spin" aria-hidden="true" />
        <span class="text-body-medium">{{ t('common.loading') }}</span>
      </div>

      <div v-else class="divide-y divide-outline-variant/35">
        <div
          v-for="(song, idx) in albumSongs"
          :key="song.id"
          class="flex items-center gap-2.5 py-2 transition-colors hover:bg-surface-container-high/50"
          :class="isSongCurrent(song) ? 'text-primary' : ''"
        >
          <button
            type="button"
            class="md-state flex min-w-0 flex-1 cursor-pointer items-center gap-2.5 rounded-corner-sm px-1.5 py-1 text-left"
            @click="playSong(song, albumSongs)"
          >
            <span class="text-body-small flex w-6 shrink-0 items-center justify-center tabular-nums text-on-surface-variant">
              <LuVolume2 v-if="isSongCurrent(song)" class="h-4 w-4 text-primary" aria-hidden="true" />
              <template v-else>{{ song.track ?? idx + 1 }}</template>
            </span>
            <span class="min-w-0 flex-1">
              <span
                class="text-title-small block truncate"
                :class="isSongCurrent(song) ? 'text-primary' : 'text-on-surface'"
              >
                {{ song.title }}
              </span>
              <span class="text-body-small block truncate text-on-surface-variant">
                {{ song.artist }}
              </span>
            </span>
            <span
              v-if="song.duration"
              class="text-body-small shrink-0 font-mono tabular-nums text-on-surface-variant"
            >
              {{ formatDuration(song.duration) }}
            </span>
          </button>
          <div class="flex shrink-0 items-center gap-2">
            <MdIconButton
              size="sm"
              variant="standard"
              :loading="Boolean(importingIds[importKey(song.id)])"
              :aria-label="globalT('remote.importBtn')"
              @click="handleImportSong(song)"
            >
              <LuCheck v-if="importedIds[importKey(song.id)]" class="h-4 w-4 text-success" />
              <LuDownload v-else class="h-4 w-4" />
            </MdIconButton>
          </div>
        </div>
      </div>
    </MdSheet>
  </div>
</template>

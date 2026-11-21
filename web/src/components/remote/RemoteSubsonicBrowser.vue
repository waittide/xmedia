<script setup lang="ts">
import { computed, nextTick, onBeforeUnmount, onMounted, ref, watch } from 'vue'
import { useRouter } from 'vue-router'
import { useI18n } from 'vue-i18n'
import {
  LuAlertCircle,
  LuCheck,
  LuDisc3,
  LuDownload,
  LuLoader2,
  LuMusic,
  LuPause,
  LuPlay,
  LuPlus,
  LuRefreshCw,
  LuSearch,
  LuSkipBack,
  LuSkipForward,
  LuX,
} from 'vue-icons-plus/lu'
import {
  appHostApi,
  type SubsonicAlbumDto,
  type SubsonicAlbumListType,
  type SubsonicSongDto,
} from '@/api/appHost'
import AppSelect, { type SelectOption } from '@/components/ui/AppSelect.vue'
import AppSlider from '@/components/ui/AppSlider.vue'
import MdSegmentedButton, { type SegmentedOption } from '@/components/ui/MdSegmentedButton.vue'
import MdSideSheet from '@/components/ui/MdSideSheet.vue'
import MediaCoverImage from '@/components/ui/MediaCoverImage.vue'
import { usePlayerStore } from '@/domains/music/player'
import { useAppClientStore } from '@/stores/appServers'
import { useSnackbarStore } from '@/stores/snackbar'
import { useUploadStore } from '@/stores/uploads'
import { formatBytes, formatDuration, formatTime } from '@/utils/format'

const ALBUM_PAGE_SIZE = 30

const props = defineProps<{
  selectedSourceId: string
  active: boolean
}>()

const emit = defineEmits<{
  'update:selectedSourceId': [id: string]
  'add-source': []
}>()

const { t } = useI18n()
const router = useRouter()
const appClientStore = useAppClientStore()
const snackbarStore = useSnackbarStore()
const uploadStore = useUploadStore()
const playerStore = usePlayerStore()

const subsonicAlbumType = ref<SubsonicAlbumListType>('newest')
const subsonicAlbums = ref<SubsonicAlbumDto[]>([])
const subsonicHasMore = ref(false)
const subsonicLoading = ref(false)
const subsonicLoadingMore = ref(false)
const subsonicError = ref('')
const subsonicSearchInput = ref('')
const subsonicSearchActive = ref(false)
const subsonicSearchAlbums = ref<SubsonicAlbumDto[]>([])
const subsonicSearchSongs = ref<SubsonicSongDto[]>([])
const subsonicImportingIds = ref<Record<string, boolean>>({})
const subsonicImportedIds = ref<Record<string, boolean>>({})
let subsonicRequestSeq = 0

// Subsonic Album Side Sheet
const albumSheetOpen = ref(false)
const activeAlbum = ref<SubsonicAlbumDto | null>(null)
const albumSongs = ref<SubsonicSongDto[]>([])
const albumDetailLoading = ref(false)
const albumDetailError = ref('')
let albumDetailSeq = 0

// Lightweight MD3 Audio Preview Bar
const previewAudioRef = ref<HTMLAudioElement | null>(null)
const previewQueue = ref<SubsonicSongDto[]>([])
const previewIndex = ref(-1)
const previewPlaying = ref(false)
const previewCurrentTime = ref(0)
const previewDuration = ref(0)
const previewBuffering = ref(false)
let previewSeq = 0

const previewSong = computed<SubsonicSongDto | null>(() => {
  if (previewIndex.value < 0 || previewIndex.value >= previewQueue.value.length) return null
  return previewQueue.value[previewIndex.value] ?? null
})

const subsonicSourceOptions = computed<SelectOption[]>(() =>
  appClientStore.subsonicSources.map((src) => ({
    value: src.id,
    label: src.name || t('remote.unnamed'),
  })),
)

const subsonicSortOptions = computed<SegmentedOption<SubsonicAlbumListType>[]>(() => [
  { value: 'newest', label: t('remote.subsonicSortNewest') },
  { value: 'random', label: t('remote.subsonicSortRandom') },
  { value: 'alphabeticalByName', label: t('remote.subsonicSortName') },
])

async function fetchSubsonicAlbums(reset = true, sourceIdOverride?: string): Promise<void> {
  const sourceId = sourceIdOverride ?? props.selectedSourceId
  if (!sourceId) {
    subsonicAlbums.value = []
    subsonicHasMore.value = false
    return
  }

  const offset = reset ? 0 : subsonicAlbums.value.length
  const seq = ++subsonicRequestSeq
  if (reset) {
    subsonicLoading.value = true
    subsonicError.value = ''
  } else {
    subsonicLoadingMore.value = true
  }

  try {
    const res = await appHostApi.listSubsonicAlbums(sourceId, {
      type: subsonicAlbumType.value,
      size: ALBUM_PAGE_SIZE,
      offset,
    })
    if (seq !== subsonicRequestSeq) return
    subsonicAlbums.value = reset ? res.albums : [...subsonicAlbums.value, ...res.albums]
    subsonicHasMore.value = res.albums.length >= ALBUM_PAGE_SIZE
  } catch (err) {
    if (seq !== subsonicRequestSeq) return
    subsonicError.value = err instanceof Error ? err.message : String(err)
  } finally {
    if (seq === subsonicRequestSeq) {
      subsonicLoading.value = false
      subsonicLoadingMore.value = false
    }
  }
}

function selectSource(nextId: string): void {
  emit('update:selectedSourceId', nextId)
  subsonicSearchInput.value = ''
  subsonicSearchActive.value = false
  subsonicSearchAlbums.value = []
  subsonicSearchSongs.value = []
  albumSheetOpen.value = false
  activeAlbum.value = null
  albumSongs.value = []
  if (nextId) {
    void fetchSubsonicAlbums(true, nextId)
  } else {
    subsonicAlbums.value = []
  }
}

function resetState(refetch = false, sourceId?: string): void {
  subsonicAlbums.value = []
  subsonicSearchInput.value = ''
  subsonicSearchActive.value = false
  subsonicSearchAlbums.value = []
  subsonicSearchSongs.value = []
  albumSheetOpen.value = false
  activeAlbum.value = null
  albumSongs.value = []
  closePreviewBar()
  if (refetch && (sourceId ?? props.selectedSourceId)) {
    void fetchSubsonicAlbums(true, sourceId ?? props.selectedSourceId)
  }
}

function handleSubsonicSortChange(nextType: SubsonicAlbumListType): void {
  subsonicAlbumType.value = nextType
  subsonicSearchActive.value = false
  subsonicSearchInput.value = ''
  void fetchSubsonicAlbums(true)
}

async function handleSubsonicSearchSubmit(): Promise<void> {
  const q = subsonicSearchInput.value.trim()
  if (!q) {
    handleSubsonicClearSearch()
    return
  }
  const sourceId = props.selectedSourceId
  if (!sourceId) return

  const seq = ++subsonicRequestSeq
  subsonicSearchActive.value = true
  subsonicLoading.value = true
  subsonicError.value = ''
  try {
    const res = await appHostApi.searchSubsonic(sourceId, q)
    if (seq !== subsonicRequestSeq) return
    subsonicSearchAlbums.value = res.albums
    subsonicSearchSongs.value = res.songs
  } catch (err) {
    if (seq !== subsonicRequestSeq) return
    subsonicError.value = err instanceof Error ? err.message : String(err)
  } finally {
    if (seq === subsonicRequestSeq) {
      subsonicLoading.value = false
    }
  }
}

function handleSubsonicClearSearch(): void {
  subsonicSearchInput.value = ''
  if (subsonicSearchActive.value) {
    subsonicSearchActive.value = false
    subsonicSearchAlbums.value = []
    subsonicSearchSongs.value = []
    void fetchSubsonicAlbums(true)
  }
}

async function openSubsonicAlbum(album: SubsonicAlbumDto): Promise<void> {
  const sourceId = props.selectedSourceId
  if (!sourceId) return

  activeAlbum.value = album
  albumSongs.value = []
  albumDetailError.value = ''
  albumSheetOpen.value = true
  albumDetailLoading.value = true
  const seq = ++albumDetailSeq

  try {
    const res = await appHostApi.getSubsonicAlbum(sourceId, album.id)
    if (seq !== albumDetailSeq) return
    activeAlbum.value = res.album
    albumSongs.value = res.songs
  } catch (err) {
    if (seq !== albumDetailSeq) return
    albumDetailError.value = err instanceof Error ? err.message : String(err)
  } finally {
    if (seq === albumDetailSeq) {
      albumDetailLoading.value = false
    }
  }
}

function subsonicImportKey(sourceId: string, songId: string): string {
  return `${sourceId}::${songId}`
}

function buildSongFilename(song: SubsonicSongDto): string {
  const parts = [song.artist, song.title].filter(Boolean).join(' - ') || 'track'
  const safeBase = parts.replace(/[\\/:*?"<>|]+/g, '_').trim().slice(0, 120)
  const ext = song.suffix?.replace(/^\.+/, '').toLowerCase()
  return ext ? `${safeBase}.${ext}` : safeBase
}

async function handleImportSong(song: SubsonicSongDto): Promise<void> {
  const sourceId = props.selectedSourceId
  if (!sourceId) return
  const key = subsonicImportKey(sourceId, song.id)
  if (subsonicImportingIds.value[key]) return

  subsonicImportingIds.value = { ...subsonicImportingIds.value, [key]: true }
  try {
    const res = await appHostApi.importSubsonicSong(sourceId, song.id, buildSongFilename(song))
    subsonicImportedIds.value = { ...subsonicImportedIds.value, [key]: true }
    uploadStore.notifyCatalogChanged('music')
    snackbarStore.show({
      message: t('remote.importSuccess', { title: res.title || song.title }),
      actionLabel: t('remote.viewInLibrary'),
      onAction: () => {
        void router.push('/music/songs')
      },
    })
  } catch (err) {
    snackbarStore.show({
      message: t('remote.importFailed', {
        message: err instanceof Error ? err.message : String(err),
      }),
      tone: 'error',
    })
  } finally {
    const next = { ...subsonicImportingIds.value }
    delete next[key]
    subsonicImportingIds.value = next
  }
}

// Audio Preview Bar Controls
function isPreviewingSong(song: SubsonicSongDto): boolean {
  return previewSong.value?.id === song.id && previewPlaying.value
}

function onPreviewAudioError(): void {
  if (!previewSong.value) return
  previewPlaying.value = false
  previewBuffering.value = false
  snackbarStore.show({ message: t('remote.subsonicPreviewFailed'), tone: 'error' })
}

async function playPreviewTrack(song: SubsonicSongDto, queue: SubsonicSongDto[]): Promise<void> {
  if (previewSong.value?.id === song.id && previewAudioRef.value) {
    if (previewPlaying.value) {
      previewAudioRef.value.pause()
    } else {
      const seq = ++previewSeq
      playerStore.pause()
      try {
        await previewAudioRef.value.play()
      } catch (err) {
        if (seq !== previewSeq) return
        if (err instanceof DOMException && err.name === 'AbortError') return
        previewPlaying.value = false
        previewBuffering.value = false
        snackbarStore.show({ message: t('remote.subsonicPreviewFailed'), tone: 'error' })
      }
    }
    return
  }

  const seq = ++previewSeq
  playerStore.pause()
  const idx = queue.findIndex((item) => item.id === song.id)
  previewQueue.value = queue
  previewIndex.value = idx >= 0 ? idx : 0
  previewCurrentTime.value = 0
  previewDuration.value = song.duration ?? 0
  previewBuffering.value = true

  await nextTick()
  if (seq !== previewSeq) return
  const audio = previewAudioRef.value
  if (!audio) return
  audio.src = song.stream_url
  audio.currentTime = 0
  try {
    await audio.play()
  } catch (err) {
    if (seq !== previewSeq) return
    if (err instanceof DOMException && err.name === 'AbortError') return
    previewPlaying.value = false
    previewBuffering.value = false
    snackbarStore.show({ message: t('remote.subsonicPreviewFailed'), tone: 'error' })
  }
}

function togglePreviewPlay(): void {
  const audio = previewAudioRef.value
  if (!audio || !previewSong.value) return
  if (previewPlaying.value) {
    audio.pause()
  } else {
    playerStore.pause()
    void audio.play().catch(() => {
      snackbarStore.show({ message: t('remote.subsonicPreviewFailed'), tone: 'error' })
    })
  }
}

function stepPreviewTrack(delta: -1 | 1): void {
  if (previewQueue.value.length === 0) return
  const nextIdx = previewIndex.value + delta
  if (nextIdx < 0 || nextIdx >= previewQueue.value.length) return
  const nextSong = previewQueue.value[nextIdx]
  if (nextSong) {
    void playPreviewTrack(nextSong, previewQueue.value)
  }
}

function seekPreviewTo(target: number): void {
  if (!Number.isFinite(target) || !previewAudioRef.value) return
  previewAudioRef.value.currentTime = target
  previewCurrentTime.value = target
}

function closePreviewBar(): void {
  previewSeq += 1
  const audio = previewAudioRef.value
  if (audio) {
    audio.pause()
    audio.removeAttribute('src')
    audio.load()
  }
  previewQueue.value = []
  previewIndex.value = -1
  previewPlaying.value = false
  previewCurrentTime.value = 0
  previewDuration.value = 0
  previewBuffering.value = false
}

function closeAllOverlays(): void {
  closePreviewBar()
  albumSheetOpen.value = false
}

function onPreviewTimeUpdate(): void {
  const audio = previewAudioRef.value
  if (!audio) return
  previewCurrentTime.value = audio.currentTime || 0
  if (Number.isFinite(audio.duration) && audio.duration > 0) {
    previewDuration.value = audio.duration
  }
}

function onPreviewEnded(): void {
  if (previewIndex.value + 1 < previewQueue.value.length) {
    stepPreviewTrack(1)
  } else {
    previewPlaying.value = false
  }
}

watch(
  () => props.active,
  (isActive) => {
    if (
      isActive &&
      props.selectedSourceId &&
      subsonicAlbums.value.length === 0 &&
      !subsonicSearchActive.value &&
      !subsonicLoading.value
    ) {
      void fetchSubsonicAlbums(true)
    }
  },
)

watch(
  () => playerStore.isPlaying,
  (playing) => {
    if (playing && previewSong.value) {
      closePreviewBar()
    }
  },
)

onMounted(() => {
  if (
    props.active &&
    props.selectedSourceId &&
    subsonicAlbums.value.length === 0 &&
    !subsonicSearchActive.value &&
    !subsonicLoading.value
  ) {
    void fetchSubsonicAlbums(true)
  }
})

onBeforeUnmount(() => {
  closePreviewBar()
})

defineExpose({
  selectSource,
  resetState,
  closePreviewBar,
  closeAllOverlays,
  hasAlbums: computed(() => subsonicAlbums.value.length > 0),
})
</script>

<template>
  <div>
    <div v-show="active">
      <div
        v-if="appClientStore.subsonicSources.length === 0"
      class="rounded-[24px] bg-[var(--md-sys-color-surface-container-low)] px-6 py-16 text-center"
    >
      <div
        class="mx-auto mb-3 flex h-12 w-12 items-center justify-center rounded-2xl bg-[var(--md-sys-color-primary-container)] text-[var(--md-sys-color-on-primary-container)]"
      >
        <LuDisc3 class="h-6 w-6" />
      </div>
      <p class="text-sm font-semibold text-[var(--md-sys-color-on-surface)]">
        {{ t('remote.subsonicEmptySourcesTitle') }}
      </p>
      <p class="mx-auto mt-1 max-w-md text-xs text-[var(--md-sys-color-on-surface-variant)]">
        {{ t('remote.subsonicEmptySourcesHint') }}
      </p>
      <button
        type="button"
        class="md-btn md-btn-sm md-btn-filled md-state mt-5"
        @click="emit('add-source')"
      >
        <LuPlus class="h-4 w-4" />
        <span>{{ t('remote.subsonicAddBtn') }}</span>
      </button>
    </div>

    <div v-else class="space-y-4">
      <!-- Subsonic Toolbar -->
      <div class="flex flex-wrap items-center gap-2.5">
        <div class="w-48 shrink-0 sm:w-56">
          <AppSelect
            :model-value="selectedSourceId"
            :options="subsonicSourceOptions"
            :placeholder="t('remote.subsonicSelectSource')"
            size="sm"
            @update:model-value="selectSource"
          />
        </div>

        <MdSegmentedButton
          :model-value="subsonicAlbumType"
          :options="subsonicSortOptions"
          size="sm"
          :show-check="false"
          @update:model-value="handleSubsonicSortChange"
        />

        <button
          type="button"
          class="md-icon-btn md-icon-btn-sm md-state"
          :disabled="subsonicLoading"
          :title="t('common.refresh')"
          :aria-label="t('common.refresh')"
          @click="subsonicSearchActive ? handleSubsonicSearchSubmit() : fetchSubsonicAlbums(true)"
        >
          <LuRefreshCw class="h-4 w-4" :class="subsonicLoading ? 'animate-spin' : ''" />
        </button>

        <form
          class="relative ml-auto w-full sm:w-64"
          @submit.prevent="handleSubsonicSearchSubmit"
        >
          <LuSearch
            class="pointer-events-none absolute top-1/2 left-3 h-3.5 w-3.5 -translate-y-1/2 text-[var(--md-sys-color-on-surface-variant)]"
          />
          <input
            v-model="subsonicSearchInput"
            type="text"
            :placeholder="t('remote.subsonicSearchPlaceholder')"
            class="md-input h-9 w-full !pr-8 !pl-8 text-xs"
          />
          <button
            v-if="subsonicSearchInput || subsonicSearchActive"
            type="button"
            class="md-icon-btn md-icon-btn-sm absolute top-1/2 right-1 -translate-y-1/2 !h-7 !w-7"
            :aria-label="t('common.cancel')"
            @click="handleSubsonicClearSearch"
          >
            <LuX class="h-3.5 w-3.5" />
          </button>
        </form>
      </div>

      <!-- Error Banner -->
      <div
        v-if="subsonicError"
        class="flex items-center justify-between gap-3 rounded-2xl bg-[var(--md-sys-color-error-container)] px-4 py-3 text-xs font-medium text-[var(--md-sys-color-on-error-container)]"
      >
        <div class="flex min-w-0 items-center gap-2">
          <LuAlertCircle class="h-4 w-4 shrink-0" />
          <span class="break-all">{{ subsonicError }}</span>
        </div>
        <button
          type="button"
          class="md-btn md-btn-sm md-btn-text shrink-0 !text-[var(--md-sys-color-on-error-container)]"
          @click="subsonicSearchActive ? handleSubsonicSearchSubmit() : fetchSubsonicAlbums(true)"
        >
          {{ t('remote.retryBtn') }}
        </button>
      </div>

      <!-- Loading -->
      <div
        v-else-if="subsonicLoading"
        class="flex items-center justify-center py-20 text-xs text-[var(--md-sys-color-on-surface-variant)]"
      >
        <LuLoader2 class="mr-2 h-4 w-4 animate-spin" />
        <span>{{ t('common.loading') }}</span>
      </div>

      <!-- Search Results View -->
      <template v-else-if="subsonicSearchActive">
        <div
          v-if="subsonicSearchSongs.length === 0 && subsonicSearchAlbums.length === 0"
          class="rounded-[24px] bg-[var(--md-sys-color-surface-container-low)] py-14 text-center text-xs text-[var(--md-sys-color-on-surface-variant)]"
        >
          {{ t('common.noResults') }}
        </div>

        <!-- Matched Songs -->
        <section v-if="subsonicSearchSongs.length > 0" class="space-y-2.5">
          <h2 class="md-label-medium text-[var(--md-sys-color-on-surface-variant)]">
            {{ t('remote.subsonicSearchSongs') }}
          </h2>
          <div class="divide-y divide-[var(--md-sys-color-outline-variant)]/35 overflow-hidden rounded-2xl bg-[var(--md-sys-color-surface-container-low)]">
            <div
              v-for="song in subsonicSearchSongs"
              :key="song.id"
              class="flex items-center gap-3 px-3.5 py-2.5 transition-colors hover:bg-[var(--md-sys-color-surface-container)]"
            >
              <button
                type="button"
                class="md-icon-btn md-icon-btn-sm md-state shrink-0"
                :class="
                  previewSong?.id === song.id
                    ? 'bg-[var(--md-sys-color-primary-container)] text-[var(--md-sys-color-on-primary-container)]'
                    : ''
                "
                :title="isPreviewingSong(song) ? t('remote.subsonicPause') : t('remote.subsonicPlay')"
                @click="playPreviewTrack(song, subsonicSearchSongs)"
              >
                <LuPause v-if="isPreviewingSong(song)" class="h-4 w-4" />
                <LuPlay v-else class="h-4 w-4" />
              </button>

              <div class="min-w-0 flex-1">
                <p class="truncate text-sm font-medium text-[var(--md-sys-color-on-surface)]">
                  {{ song.title }}
                </p>
                <p class="truncate text-xs text-[var(--md-sys-color-on-surface-variant)]">
                  {{ song.artist }}
                  <span v-if="song.album"> · {{ song.album }}</span>
                </p>
              </div>

              <span
                v-if="song.suffix"
                class="hidden shrink-0 rounded-md bg-[var(--md-sys-color-surface-container-high)] px-1.5 py-0.5 font-mono text-[10px] uppercase text-[var(--md-sys-color-on-surface-variant)] sm:inline"
              >
                {{ song.suffix }}
              </span>

              <span
                v-if="song.duration"
                class="shrink-0 font-mono text-xs tabular-nums text-[var(--md-sys-color-on-surface-variant)]"
              >
                {{ formatDuration(song.duration) }}
              </span>

              <button
                type="button"
                class="md-btn md-btn-sm md-state shrink-0"
                :class="
                  subsonicImportedIds[subsonicImportKey(selectedSourceId, song.id)]
                    ? 'md-btn-outlined'
                    : 'md-btn-tonal'
                "
                :disabled="Boolean(subsonicImportingIds[subsonicImportKey(selectedSourceId, song.id)])"
                @click="handleImportSong(song)"
              >
                <LuLoader2
                  v-if="subsonicImportingIds[subsonicImportKey(selectedSourceId, song.id)]"
                  class="h-3.5 w-3.5 animate-spin"
                />
                <LuCheck
                  v-else-if="subsonicImportedIds[subsonicImportKey(selectedSourceId, song.id)]"
                  class="h-3.5 w-3.5"
                />
                <LuDownload v-else class="h-3.5 w-3.5" />
                <span>
                  {{
                    subsonicImportedIds[subsonicImportKey(selectedSourceId, song.id)]
                      ? t('remote.importedBtn')
                      : t('remote.importBtn')
                  }}
                </span>
              </button>
            </div>
          </div>
        </section>

        <!-- Matched Albums -->
        <section v-if="subsonicSearchAlbums.length > 0" class="space-y-3">
          <h2 class="md-label-medium text-[var(--md-sys-color-on-surface-variant)]">
            {{ t('remote.subsonicSearchAlbums') }}
          </h2>
          <div class="grid grid-cols-2 gap-4 sm:grid-cols-3 xl:grid-cols-4">
            <button
              v-for="album in subsonicSearchAlbums"
              :key="album.id"
              type="button"
              class="group flex cursor-pointer flex-col rounded-2xl text-left focus-visible:outline-2 focus-visible:outline-offset-4 focus-visible:outline-[var(--md-sys-color-primary)]"
              @click="openSubsonicAlbum(album)"
            >
              <MediaCoverImage
                :src="album.cover_url"
                :alt="album.name"
                aspect-class="aspect-square w-full"
                rounded="rounded-2xl"
                :fallback-icon="LuDisc3"
                img-class="h-full w-full object-cover transition duration-300 group-hover:scale-[1.03]"
                class="shadow-[var(--md-sys-elevation-1)] transition duration-200 group-hover:-translate-y-1"
              />
              <p
                class="mt-2 truncate text-sm font-semibold text-[var(--md-sys-color-on-surface)] group-hover:text-[var(--md-sys-color-primary)]"
              >
                {{ album.name }}
              </p>
              <p class="truncate text-xs text-[var(--md-sys-color-on-surface-variant)]">
                {{ album.artist }}
              </p>
            </button>
          </div>
        </section>
      </template>

      <!-- Album Grid Browse View -->
      <template v-else>
        <div
          v-if="subsonicAlbums.length === 0"
          class="rounded-[24px] bg-[var(--md-sys-color-surface-container-low)] py-14 text-center text-xs text-[var(--md-sys-color-on-surface-variant)]"
        >
          {{ t('remote.subsonicAlbumsEmpty') }}
        </div>

        <template v-else>
          <div class="grid grid-cols-2 gap-4 sm:grid-cols-3 xl:grid-cols-4">
            <button
              v-for="album in subsonicAlbums"
              :key="album.id"
              type="button"
              class="group flex cursor-pointer flex-col rounded-2xl text-left focus-visible:outline-2 focus-visible:outline-offset-4 focus-visible:outline-[var(--md-sys-color-primary)]"
              @click="openSubsonicAlbum(album)"
            >
              <MediaCoverImage
                :src="album.cover_url"
                :alt="album.name"
                aspect-class="aspect-square w-full"
                rounded="rounded-2xl"
                :fallback-icon="LuDisc3"
                img-class="h-full w-full object-cover transition duration-300 group-hover:scale-[1.03]"
                class="shadow-[var(--md-sys-elevation-1)] transition duration-200 group-hover:-translate-y-1"
              />
              <div class="mt-2 min-w-0 px-0.5">
                <p
                  class="truncate text-sm font-semibold text-[var(--md-sys-color-on-surface)] transition-colors group-hover:text-[var(--md-sys-color-primary)]"
                  :title="album.name"
                >
                  {{ album.name }}
                </p>
                <p class="mt-0.5 truncate text-xs text-[var(--md-sys-color-on-surface-variant)]">
                  {{ album.artist }}
                </p>
                <p
                  v-if="album.year || album.song_count"
                  class="mt-0.5 truncate text-[11px] tabular-nums text-[var(--md-sys-color-on-surface-variant)]/80"
                >
                  <span v-if="album.year">{{ album.year }}</span>
                  <span v-if="album.year && album.song_count"> · </span>
                  <span v-if="album.song_count">
                    {{ t('remote.subsonicTracksCount', { count: album.song_count }) }}
                  </span>
                </p>
              </div>
            </button>
          </div>

          <div v-if="subsonicHasMore" class="flex justify-center pt-2">
            <button
              type="button"
              :disabled="subsonicLoadingMore"
              class="md-btn md-btn-tonal md-btn-sm"
              @click="fetchSubsonicAlbums(false)"
            >
              <LuLoader2 v-if="subsonicLoadingMore" class="h-3.5 w-3.5 animate-spin" />
              <span>{{ t('remote.subsonicLoadMore') }}</span>
            </button>
          </div>
        </template>
      </template>
      </div>
    </div>

    <!-- Built-in MD3 Subsonic Audio Preview Bar -->
    <audio
      ref="previewAudioRef"
      class="hidden"
      @play="previewPlaying = true; previewBuffering = false"
      @pause="previewPlaying = false"
      @waiting="previewBuffering = true"
      @canplay="previewBuffering = false"
      @timeupdate="onPreviewTimeUpdate"
      @loadedmetadata="onPreviewTimeUpdate"
      @ended="onPreviewEnded"
      @error="onPreviewAudioError"
    />

    <Transition name="md-dialog">
      <div
        v-if="previewSong"
        role="region"
        :aria-label="t('remote.subsonicPreviewPlaying')"
        class="sticky bottom-4 z-20 mx-auto mt-4 max-w-3xl rounded-2xl border border-[var(--md-sys-color-outline-variant)]/50 bg-[var(--md-sys-color-surface-container-high)]/95 px-4 py-3 shadow-[var(--md-sys-elevation-3)] backdrop-blur-md"
      >
        <div class="flex flex-wrap items-center gap-3">
          <MediaCoverImage
            :src="previewSong.cover_url"
            :alt="previewSong.title"
            aspect-class="h-10 w-10 shrink-0"
            rounded="rounded-xl"
            :fallback-icon="LuMusic"
          />

          <div class="min-w-0 flex-1">
            <p class="truncate text-xs font-semibold text-[var(--md-sys-color-on-surface)]">
              {{ previewSong.title }}
            </p>
            <p class="truncate text-[11px] text-[var(--md-sys-color-on-surface-variant)]">
              {{ previewSong.artist }}
              <span v-if="previewSong.album"> · {{ previewSong.album }}</span>
            </p>
          </div>

          <!-- Transport Controls -->
          <div class="flex items-center gap-1">
            <button
              type="button"
              class="md-icon-btn md-icon-btn-sm md-state"
              :disabled="previewIndex <= 0"
              :title="t('remote.subsonicPrevTrack')"
              :aria-label="t('remote.subsonicPrevTrack')"
              @click="stepPreviewTrack(-1)"
            >
              <LuSkipBack class="h-4 w-4" />
            </button>

            <button
              type="button"
              class="md-state flex h-9 w-9 cursor-pointer items-center justify-center rounded-full bg-[var(--md-sys-color-primary)] text-[var(--md-sys-color-on-primary)]"
              :title="previewPlaying ? t('remote.subsonicPause') : t('remote.subsonicPlay')"
              :aria-label="previewPlaying ? t('remote.subsonicPause') : t('remote.subsonicPlay')"
              @click="togglePreviewPlay"
            >
              <LuLoader2 v-if="previewBuffering" class="h-4 w-4 animate-spin" />
              <LuPause v-else-if="previewPlaying" class="h-4 w-4" />
              <LuPlay v-else class="h-4 w-4 translate-x-0.5" />
            </button>

            <button
              type="button"
              class="md-icon-btn md-icon-btn-sm md-state"
              :disabled="previewIndex >= previewQueue.length - 1"
              :title="t('remote.subsonicNextTrack')"
              :aria-label="t('remote.subsonicNextTrack')"
              @click="stepPreviewTrack(1)"
            >
              <LuSkipForward class="h-4 w-4" />
            </button>
          </div>

          <div class="flex items-center gap-1">
            <button
              type="button"
              class="md-btn md-btn-sm md-btn-tonal"
              :disabled="
                Boolean(subsonicImportingIds[subsonicImportKey(selectedSourceId, previewSong.id)])
              "
              @click="handleImportSong(previewSong)"
            >
              <LuLoader2
                v-if="subsonicImportingIds[subsonicImportKey(selectedSourceId, previewSong.id)]"
                class="h-3.5 w-3.5 animate-spin"
              />
              <LuCheck
                v-else-if="
                  subsonicImportedIds[subsonicImportKey(selectedSourceId, previewSong.id)]
                "
                class="h-3.5 w-3.5"
              />
              <LuDownload v-else class="h-3.5 w-3.5" />
              <span class="hidden sm:inline">
                {{
                  subsonicImportedIds[subsonicImportKey(selectedSourceId, previewSong.id)]
                    ? t('remote.importedBtn')
                    : t('remote.importBtn')
                }}
              </span>
            </button>

            <button
              type="button"
              class="md-icon-btn md-icon-btn-sm md-state"
              :title="t('remote.subsonicClosePreview')"
              :aria-label="t('remote.subsonicClosePreview')"
              @click="closePreviewBar"
            >
              <LuX class="h-4 w-4" />
            </button>
          </div>
        </div>

        <!-- Seek Bar -->
        <div class="mt-2 flex items-center gap-2.5">
          <span class="w-9 shrink-0 font-mono text-[10px] tabular-nums text-[var(--md-sys-color-on-surface-variant)]">
            {{ formatTime(previewCurrentTime) }}
          </span>
          <div class="min-w-0 flex-1">
            <AppSlider
              :model-value="previewCurrentTime"
              :min="0"
              :max="Math.max(1, previewDuration)"
              :step="0.5"
              :aria-label="previewSong?.title || t('remote.subsonicPreviewPlaying')"
              @update:model-value="seekPreviewTo"
            />
          </div>
          <span class="w-9 shrink-0 text-right font-mono text-[10px] tabular-nums text-[var(--md-sys-color-on-surface-variant)]">
            {{ formatTime(previewDuration) }}
          </span>
        </div>
      </div>
    </Transition>

    <!-- Subsonic Album Tracks Side Sheet -->
    <MdSideSheet
      :open="albumSheetOpen"
      :title="activeAlbum?.name || ''"
      :subtitle="activeAlbum?.artist || ''"
      width-class="w-full max-w-lg"
      @close="albumSheetOpen = false"
    >
      <template v-if="activeAlbum" #subheader>
        <div class="flex items-center gap-3.5 rounded-2xl bg-[var(--md-sys-color-surface-container)] p-3">
          <MediaCoverImage
            :src="activeAlbum.cover_url"
            :alt="activeAlbum.name"
            aspect-class="h-16 w-16 shrink-0"
            rounded="rounded-xl"
            :fallback-icon="LuDisc3"
          />
          <div class="min-w-0 flex-1 space-y-1">
            <p class="truncate text-xs text-[var(--md-sys-color-on-surface-variant)]">
              <span v-if="activeAlbum.year">{{ activeAlbum.year }}</span>
              <span v-if="activeAlbum.year && albumSongs.length"> · </span>
              <span v-if="albumSongs.length">
                {{ t('remote.subsonicTracksCount', { count: albumSongs.length }) }}
              </span>
              <span v-if="activeAlbum.duration">
                · {{ formatDuration(activeAlbum.duration) }}
              </span>
            </p>
            <button
              v-if="albumSongs.length > 0"
              type="button"
              class="md-btn md-btn-filled md-btn-sm"
              @click="playPreviewTrack(albumSongs[0]!, albumSongs)"
            >
              <LuPlay class="h-3.5 w-3.5" />
              <span>{{ t('remote.subsonicPlayAlbum') }}</span>
            </button>
          </div>
        </div>
      </template>

      <div
        v-if="albumDetailLoading"
        class="flex items-center justify-center py-14 text-xs text-[var(--md-sys-color-on-surface-variant)]"
      >
        <LuLoader2 class="mr-2 h-4 w-4 animate-spin" />
        <span>{{ t('common.loading') }}</span>
      </div>

      <div
        v-else-if="albumDetailError"
        class="flex items-center gap-2 rounded-2xl bg-[var(--md-sys-color-error-container)] px-4 py-3 text-xs font-medium text-[var(--md-sys-color-on-error-container)]"
      >
        <LuAlertCircle class="h-4 w-4 shrink-0" />
        <span class="break-all">{{ albumDetailError }}</span>
      </div>

      <div
        v-else-if="albumSongs.length === 0"
        class="py-12 text-center text-xs text-[var(--md-sys-color-on-surface-variant)]"
      >
        {{ t('remote.subsonicTracksEmpty') }}
      </div>

      <div v-else class="space-y-1">
        <div
          v-for="(song, idx) in albumSongs"
          :key="song.id"
          class="flex items-center gap-2.5 rounded-2xl px-2.5 py-2 transition-colors"
          :class="
            previewSong?.id === song.id
              ? 'bg-[var(--md-sys-color-secondary-container)]/55'
              : 'hover:bg-[var(--md-sys-color-surface-container)]'
          "
        >
          <button
            type="button"
            class="md-icon-btn md-icon-btn-sm md-state shrink-0"
            :class="
              previewSong?.id === song.id
                ? 'bg-[var(--md-sys-color-primary)] text-[var(--md-sys-color-on-primary)]'
                : ''
            "
            :title="isPreviewingSong(song) ? t('remote.subsonicPause') : t('remote.subsonicPlay')"
            @click="playPreviewTrack(song, albumSongs)"
          >
            <LuPause v-if="isPreviewingSong(song)" class="h-3.5 w-3.5" />
            <LuPlay v-else class="h-3.5 w-3.5" />
          </button>

          <span class="w-5 shrink-0 text-right font-mono text-[11px] tabular-nums text-[var(--md-sys-color-on-surface-variant)]">
            {{ song.track ?? idx + 1 }}
          </span>

          <div class="min-w-0 flex-1">
            <p class="truncate text-xs font-semibold text-[var(--md-sys-color-on-surface)]">
              {{ song.title }}
            </p>
            <p class="truncate text-[11px] text-[var(--md-sys-color-on-surface-variant)]">
              <span>{{ song.artist }}</span>
              <span v-if="song.suffix" class="ml-1.5 font-mono uppercase opacity-80">
                · {{ song.suffix }}
                <template v-if="song.bit_rate"> {{ song.bit_rate }}k</template>
              </span>
              <span v-if="song.size" class="ml-1 font-mono opacity-75">
                · {{ formatBytes(song.size) }}
              </span>
            </p>
          </div>

          <span
            v-if="song.duration"
            class="shrink-0 font-mono text-[11px] tabular-nums text-[var(--md-sys-color-on-surface-variant)]"
          >
            {{ formatDuration(song.duration) }}
          </span>

          <button
            type="button"
            class="md-btn md-btn-sm md-state shrink-0 !h-7 !px-2.5 !text-[11px]"
            :class="
              subsonicImportedIds[subsonicImportKey(selectedSourceId, song.id)]
                ? 'md-btn-outlined'
                : 'md-btn-tonal'
            "
            :disabled="Boolean(subsonicImportingIds[subsonicImportKey(selectedSourceId, song.id)])"
            @click="handleImportSong(song)"
          >
            <LuLoader2
              v-if="subsonicImportingIds[subsonicImportKey(selectedSourceId, song.id)]"
              class="h-3 w-3 animate-spin"
            />
            <LuCheck
              v-else-if="subsonicImportedIds[subsonicImportKey(selectedSourceId, song.id)]"
              class="h-3 w-3"
            />
            <LuDownload v-else class="h-3 w-3" />
            <span>
              {{
                subsonicImportedIds[subsonicImportKey(selectedSourceId, song.id)]
                  ? t('remote.importedBtn')
                  : t('remote.importBtn')
              }}
            </span>
          </button>
        </div>
      </div>
    </MdSideSheet>
  </div>
</template>

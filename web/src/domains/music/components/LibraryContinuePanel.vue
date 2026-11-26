<script setup lang="ts">
import { computed, onBeforeUnmount, onMounted, ref, watch } from 'vue'
import {
  LuBookmark,
  LuClock,
  LuDisc3,
  LuListMusic,
  LuPause,
  LuPlay,
  LuShuffle,
  LuTrash2,
} from 'vue-icons-plus/lu'
import { MediaErrorState } from '@/components/domain'
import {
  MdButton,
  MdEmptyState,
  MdIconButton,
  MdLinearProgress,
  MdSkeleton,
  MediaCoverImage,
} from '@/components/ui'
import { useDelayedLoading } from '@/composables/useDelayedLoading'
import { useSnackbarStore } from '@/stores/snackbar'
import { formatTime } from '@/utils/format'
import { api, buildCoverUrl } from '../api'
import { useMusicI18n as useI18n } from '../i18n'
import { usePlayerStore } from '../player'
import { useMusicStore } from '../store'
import type { AlbumDto, BookmarkDto, PlayQueueDto, TrackDto } from '../types'
import AlbumCard from './AlbumCard.vue'
import TrackTable from './TrackTable.vue'

const props = withDefaults(
  defineProps<{
    query?: string
  }>(),
  {
    query: '',
  },
)

const { t } = useI18n()
const musicStore = useMusicStore()
const playerStore = usePlayerStore()
const snackbarStore = useSnackbarStore()

const loadingLocal = ref(false)
const localError = ref<string | null>(null)
const savedQueue = ref<PlayQueueDto | null>(null)
const bookmarks = ref<BookmarkDto[]>([])
const recentAlbums = ref<AlbumDto[]>([])
const recentTracks = ref<TrackDto[]>([])

const queryNeedle = computed(() => props.query.trim().toLowerCase())

const filteredBookmarks = computed(() => {
  const needle = queryNeedle.value
  if (!needle) return bookmarks.value
  return bookmarks.value.filter(
    (bm) =>
      bm.track.title.toLowerCase().includes(needle) ||
      bm.track.artist_name.toLowerCase().includes(needle) ||
      (bm.comment?.toLowerCase().includes(needle) ?? false),
  )
})

const filteredRecentAlbums = computed(() => {
  const needle = queryNeedle.value
  if (!needle) return recentAlbums.value
  return recentAlbums.value.filter(
    (al) =>
      al.title.toLowerCase().includes(needle) ||
      al.album_artist_name.toLowerCase().includes(needle),
  )
})

const filteredRecentTracks = computed(() => {
  const needle = queryNeedle.value
  if (!needle) return recentTracks.value
  return recentTracks.value.filter(
    (tr) =>
      tr.title.toLowerCase().includes(needle) ||
      tr.artist_name.toLowerCase().includes(needle) ||
      tr.album_title.toLowerCase().includes(needle),
  )
})

const resumeSession = computed(() => {
  if (playerStore.currentTrack && playerStore.queue.length > 0) {
    const tr = playerStore.currentTrack
    const posSecs = playerStore.currentTimeSecs
    const durSecs = Math.max(1, tr.duration_secs || playerStore.durationSecs || 1)
    const percent = Math.min(100, Math.round((posSecs / durSecs) * 100))
    return {
      source: 'local' as const,
      title: tr.title,
      subtitle: [tr.artist_name, tr.album_title].filter(Boolean).join(' · '),
      coverSrc:
        tr.kind === 'local'
          ? tr.cover_key && tr.cover_id
            ? buildCoverUrl(tr.cover_id, 240)
            : null
          : tr.cover_url,
      queueCount: playerStore.queue.length,
      tracks: [] as TrackDto[],
      index: playerStore.currentIndex,
      posSecs,
      durSecs,
      percent,
    }
  }
  const q = savedQueue.value
  if (!q || q.tracks.length === 0) return null
  const idx = q.current_track_id
    ? Math.max(0, q.tracks.findIndex((tr) => tr.id === q.current_track_id))
    : 0
  const track = q.tracks[idx] ?? q.tracks[0]!
  const posSecs = Math.max(0, (q.position_ms || 0) / 1000)
  const durSecs = Math.max(1, track.duration_secs || 1)
  const percent = Math.min(100, Math.round((posSecs / durSecs) * 100))
  return {
    source: 'remote' as const,
    title: track.title,
    subtitle: [track.artist_name, track.album_title].filter(Boolean).join(' · '),
    coverSrc: track.cover_key ? buildCoverUrl(track.id, 240) : null,
    queueCount: q.tracks.length,
    tracks: q.tracks,
    index: idx,
    posSecs,
    durSecs,
    percent,
  }
})

const isLocalPlaying = computed(
  () => resumeSession.value?.source === 'local' && playerStore.isPlaying,
)

const isLoading = computed(
  () =>
    loadingLocal.value &&
    !resumeSession.value &&
    bookmarks.value.length === 0 &&
    recentTracks.value.length === 0,
)

const hasAnyContent = computed(
  () =>
    resumeSession.value !== null ||
    filteredBookmarks.value.length > 0 ||
    filteredRecentAlbums.value.length > 0 ||
    filteredRecentTracks.value.length > 0,
)

let loadSeq = 0

async function loadLocalContinue(): Promise<void> {
  const seq = ++loadSeq
  loadingLocal.value = true
  localError.value = null
  try {
    const [qRes, bmRes, alRes, trRes] = await Promise.allSettled([
      api.getPlayQueue(),
      api.listBookmarks(),
      api.listAlbums({ type: 'recent', size: 6 }),
      api.recentTracks(50),
    ])
    if (seq !== loadSeq) return
    if (qRes.status === 'fulfilled') savedQueue.value = qRes.value
    if (bmRes.status === 'fulfilled') bookmarks.value = bmRes.value
    if (alRes.status === 'fulfilled') recentAlbums.value = alRes.value.items
    if (trRes.status === 'fulfilled') recentTracks.value = trRes.value
    if (
      qRes.status === 'rejected' &&
      bmRes.status === 'rejected' &&
      alRes.status === 'rejected' &&
      trRes.status === 'rejected'
    ) {
      localError.value =
        trRes.reason instanceof Error ? trRes.reason.message : String(trRes.reason)
    }
  } finally {
    if (seq === loadSeq) {
      loadingLocal.value = false
    }
  }
}

onBeforeUnmount(() => {
  loadSeq += 1
})

async function refresh(): Promise<void> {
  await loadLocalContinue()
}

// Fixes P1-3: Never pause accidentally when already playing; show explicit Pause/Resume toggle
async function handleResumeOrPause(): Promise<void> {
  const s = resumeSession.value
  if (!s) return
  if (s.source === 'local') {
    await playerStore.togglePlay()
  } else {
    playerStore.resumeQueue(s.tracks, s.index, s.posSecs)
  }
}

// Fixes P1-3: Restore remote cloud queue into playerStore before opening QueueDrawer
async function handleOpenQueue(): Promise<void> {
  const s = resumeSession.value
  if (s?.source === 'remote' && playerStore.queue.length === 0) {
    await playerStore.restoreRemoteQueue(true)
  }
  playerStore.queueOpen = true
}

function playBookmark(bm: BookmarkDto): void {
  const posSecs = bm.position_ms / 1000
  if (playerStore.queue.length === 0 && savedQueue.value && savedQueue.value.tracks.length > 0) {
    const idx = savedQueue.value.tracks.findIndex((tr) => tr.id === bm.track.id)
    if (idx >= 0) {
      playerStore.resumeQueue(savedQueue.value.tracks, idx, posSecs)
      return
    }
  }
  playerStore.playTrackAt(bm.track, posSecs)
}

async function deleteBookmark(bm: BookmarkDto): Promise<void> {
  const prev = [...bookmarks.value]
  bookmarks.value = bookmarks.value.filter((item) => item.track.id !== bm.track.id)
  try {
    await api.deleteBookmark(bm.track.id)
    snackbarStore.show({
      message: t('common.bookmarkDeleted'),
      actionLabel: t('common.undo'),
      onAction: async () => {
        try {
          const restored = await api.saveBookmark(
            bm.track.id,
            bm.position_ms,
            bm.comment ?? undefined,
          )
          bookmarks.value = [restored, ...bookmarks.value.filter((x) => x.track.id !== bm.track.id)]
        } catch {
          bookmarks.value = prev
        }
      },
    })
  } catch (err) {
    bookmarks.value = prev
    snackbarStore.show({
      message: err instanceof Error ? err.message : String(err),
      tone: 'error',
    })
  }
}

function bookmarkPercent(bm: BookmarkDto): number {
  const durMs = Math.max(1, bm.track.duration_secs * 1000)
  return Math.min(100, Math.round((bm.position_ms / durMs) * 100))
}

const showSkeleton = useDelayedLoading(isLoading)

onMounted(loadLocalContinue)
watch(() => musicStore.catalogVersion, loadLocalContinue)

defineExpose({ refresh })
</script>

<template>
  <div class="space-y-6">
    <MediaErrorState
      v-if="localError"
      :message="localError"
      :retrying="isLoading"
      @retry="refresh"
    />

    <div v-else-if="isLoading && showSkeleton" class="space-y-4" aria-busy="true">
      <MdSkeleton variant="rect" height="6.5rem" />
      <div class="grid grid-cols-1 gap-3 md:grid-cols-2">
        <MdSkeleton v-for="i in 4" :key="`c-sk-${i}`" variant="rect" height="4.5rem" />
      </div>
    </div>

    <div v-else-if="isLoading" class="min-h-24" aria-busy="true" />

    <MdEmptyState
      v-else-if="!hasAnyContent"
      :icon="LuClock"
      :title="t('continueView.emptyTitle')"
      :description="t('continueView.emptyDesc')"
    />

    <template v-else>
      <!-- 1. Active / Saved Local Queue Resume Card -->
      <section v-if="resumeSession" class="space-y-3">
        <h2 class="text-title-large text-on-surface">
          {{ t('continueView.savedQueueSection') }}
        </h2>
        <div
          class="flex flex-col justify-between gap-4 rounded-corner-xl bg-surface-container-low p-5 sm:flex-row sm:items-center"
        >
          <div class="flex min-w-0 flex-1 items-center gap-4">
            <MediaCoverImage
              :src="resumeSession.coverSrc"
              :alt="resumeSession.title"
              aspect-class="h-20 w-20 shrink-0"
              rounded="rounded-corner-md"
              bg-class="bg-surface-container-high"
              :fallback-icon="LuDisc3"
              class="album-shadow"
            />
            <div class="min-w-0 flex-1 space-y-1.5">
              <div class="flex flex-wrap items-center gap-2">
                <span
                  class="text-label-small inline-flex items-center gap-1 rounded-corner-xs bg-primary-container px-2 py-0.5 text-on-primary-container"
                >
                  <LuListMusic class="h-3 w-3" aria-hidden="true" />
                  <span>{{ t('continueView.queueCountBadge', { count: resumeSession.queueCount }) }}</span>
                </span>
                <span class="text-body-small font-mono tabular-nums text-on-surface-variant">
                  {{
                    t('continueView.resumeAt', {
                      time: formatTime(resumeSession.posSecs),
                      percent: resumeSession.percent,
                    })
                  }}
                </span>
              </div>

              <h3 class="text-title-medium truncate text-on-surface">
                {{ resumeSession.title }}
              </h3>
              <p v-if="resumeSession.subtitle" class="text-body-small truncate text-on-surface-variant">
                {{ resumeSession.subtitle }}
              </p>

              <div class="max-w-md pt-0.5">
                <MdLinearProgress
                  :value="resumeSession.percent"
                  :aria-label="`${resumeSession.title} ${resumeSession.percent}%`"
                />
              </div>
            </div>
          </div>

          <div class="flex shrink-0 items-center gap-2">
            <MdButton
              variant="filled"
              :icon="isLocalPlaying ? LuPause : LuPlay"
              @click="handleResumeOrPause"
            >
              {{ isLocalPlaying ? t('player.pause') : t('continueView.resumeBtn') }}
            </MdButton>
            <MdButton variant="tonal" :icon="LuListMusic" @click="handleOpenQueue">
              {{ t('continueView.openQueueBtn') }}
            </MdButton>
          </div>
        </div>
      </section>

      <!-- 2. Audio Bookmarks (single click target, undo snackbar on delete) -->
      <section v-if="filteredBookmarks.length > 0" class="space-y-3">
        <h2 class="text-title-large text-on-surface">
          {{ t('continueView.bookmarksSection') }}
        </h2>
        <div class="grid grid-cols-1 gap-3 md:grid-cols-2">
          <div
            v-for="bm in filteredBookmarks"
            :key="bm.track.id"
            class="group flex items-center gap-3 rounded-corner-lg bg-surface-container-low p-3 transition-colors hover:bg-surface-container"
          >
            <button
              type="button"
              class="flex min-w-0 flex-1 cursor-pointer items-center gap-3 text-left"
              @click="playBookmark(bm)"
            >
              <MediaCoverImage
                :src="bm.track.cover_key ? buildCoverUrl(bm.track.id, 160) : null"
                :alt="bm.track.title"
                aspect-class="h-14 w-14 shrink-0"
                rounded="rounded-corner-sm"
                bg-class="bg-surface-container-high"
                :fallback-icon="LuDisc3"
              />
              <div class="min-w-0 flex-1">
                <p class="text-title-small truncate text-on-surface group-hover:text-primary">
                  {{ bm.track.title }}
                </p>
                <p class="text-body-small truncate text-on-surface-variant">
                  {{ bm.track.artist_name }}
                </p>
                <div class="mt-1 flex items-center gap-2">
                  <span
                    class="text-label-small inline-flex items-center gap-1 rounded-corner-xs bg-primary-container px-1.5 py-0.5 font-mono text-on-primary-container"
                  >
                    <LuBookmark class="h-3 w-3" aria-hidden="true" />
                    <span>
                      {{
                        t('continueView.resumeAt', {
                          time: formatTime(bm.position_ms / 1000),
                          percent: bookmarkPercent(bm),
                        })
                      }}
                    </span>
                  </span>
                  <span v-if="bm.comment" class="text-body-small truncate text-on-surface-variant">
                    {{ bm.comment }}
                  </span>
                </div>
              </div>
            </button>

            <MdIconButton
              size="sm"
              variant="standard"
              :icon="LuTrash2"
              :aria-label="t('common.delete')"
              @click="deleteBookmark(bm)"
            />
          </div>
        </div>
      </section>

      <!-- 3. Recently Played Albums -->
      <section v-if="filteredRecentAlbums.length > 0" class="space-y-3">
        <h2 class="text-title-large text-on-surface">
          {{ t('continueView.recentAlbumsSection') }}
        </h2>
        <div class="grid grid-cols-2 gap-5 sm:grid-cols-3 md:grid-cols-4 lg:grid-cols-5 xl:grid-cols-6">
          <AlbumCard v-for="al in filteredRecentAlbums" :key="al.id" :album="al" />
        </div>
      </section>

      <!-- 4. Recently Played Tracks -->
      <section v-if="filteredRecentTracks.length > 0" class="space-y-3">
        <div class="flex flex-wrap items-center justify-between gap-2">
          <h2 class="text-title-large text-on-surface">
            {{ t('continueView.recentTracksSection') }}
          </h2>
          <div class="flex items-center gap-1.5">
            <MdButton
              variant="tonal"
              :icon="LuPlay"
              @click="playerStore.playQueue(filteredRecentTracks, 0)"
            >
              {{ t('common.playAllBtn') }}
            </MdButton>
            <MdButton
              variant="text"
              :icon="LuShuffle"
              @click="playerStore.shuffleAndPlay(filteredRecentTracks)"
            >
              {{ t('common.shufflePlayBtn') }}
            </MdButton>
          </div>
        </div>
        <TrackTable :tracks="filteredRecentTracks" />
      </section>
    </template>
  </div>
</template>

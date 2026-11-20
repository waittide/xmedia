<script setup lang="ts">
import { computed, onMounted, ref, watch } from 'vue'
import { useRouter } from 'vue-router'
import {
  LuPlay,
  LuShuffle,
  LuClock,
  LuBookmark,
  LuListMusic,
  LuTrash2,
} from 'vue-icons-plus/lu'
import RefreshButton from '@/components/ui/RefreshButton.vue'
import { api } from '../api'
import AlbumCard from '../components/AlbumCard.vue'
import CoverImage from '../components/CoverImage.vue'
import TrackTable from '../components/TrackTable.vue'
import { useMusicI18n as useI18n } from '../i18n'
import { usePlayerStore } from '../player'
import { useMusicStore } from '../store'
import type { AlbumDto, BookmarkDto, PlayQueueDto, TrackDto } from '../types'

const { t } = useI18n()
const router = useRouter()
const musicStore = useMusicStore()
const playerStore = usePlayerStore()

const loading = ref(true)
const savedQueue = ref<PlayQueueDto | null>(null)
const bookmarks = ref<BookmarkDto[]>([])
const recentAlbums = ref<AlbumDto[]>([])
const recentTracks = ref<TrackDto[]>([])

function formatTime(secs: number): string {
  if (!Number.isFinite(secs) || secs < 0) return '0:00'
  const m = Math.floor(secs / 60)
  const s = Math.floor(secs % 60)
  return `${m}:${String(s).padStart(2, '0')}`
}

const resumeSession = computed(() => {
  if (playerStore.currentTrack && playerStore.queue.length > 0) {
    const tr = playerStore.currentTrack
    const posSecs = playerStore.currentTimeSecs
    const durSecs = Math.max(1, tr.duration_secs || playerStore.durationSecs || 1)
    const percent = Math.min(100, Math.round((posSecs / durSecs) * 100))
    return {
      source: 'local' as const,
      track: tr,
      tracks: playerStore.queue,
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
    track,
    tracks: q.tracks,
    index: idx,
    posSecs,
    durSecs,
    percent,
  }
})

const hasAnyContent = computed(
  () =>
    resumeSession.value !== null ||
    bookmarks.value.length > 0 ||
    recentAlbums.value.length > 0 ||
    recentTracks.value.length > 0,
)

async function loadContinueData(): Promise<void> {
  loading.value = true
  try {
    const [qRes, bmRes, alRes, trRes] = await Promise.allSettled([
      api.getPlayQueue(),
      api.listBookmarks(),
      api.listAlbums({ type: 'recent', size: 6 }),
      api.recentTracks(50),
    ])
    if (qRes.status === 'fulfilled') savedQueue.value = qRes.value
    if (bmRes.status === 'fulfilled') bookmarks.value = bmRes.value
    if (alRes.status === 'fulfilled') recentAlbums.value = alRes.value.items
    if (trRes.status === 'fulfilled') recentTracks.value = trRes.value
  } finally {
    loading.value = false
  }
}

async function handleResumeQueue(): Promise<void> {
  const s = resumeSession.value
  if (!s) return
  if (s.source === 'local') {
    await playerStore.togglePlay()
  } else {
    playerStore.resumeQueue(s.tracks, s.index, s.posSecs)
  }
}

function playBookmark(bm: BookmarkDto): void {
  const posSecs = bm.position_ms / 1000
  if (playerStore.queue.length === 0 && savedQueue.value && savedQueue.value.tracks.length > 0) {
    const idx = savedQueue.value.tracks.findIndex((t) => t.id === bm.track.id)
    if (idx >= 0) {
      playerStore.resumeQueue(savedQueue.value.tracks, idx, posSecs)
      return
    }
  }
  playerStore.playTrackAt(bm.track, posSecs)
}

async function deleteBookmark(trackId: string): Promise<void> {
  await api.deleteBookmark(trackId)
  bookmarks.value = bookmarks.value.filter((bm) => bm.track.id !== trackId)
}

function bookmarkPercent(bm: BookmarkDto): number {
  const durMs = Math.max(1, bm.track.duration_secs * 1000)
  return Math.min(100, Math.round((bm.position_ms / durMs) * 100))
}

onMounted(loadContinueData)
watch(() => musicStore.catalogVersion, loadContinueData)
</script>

<template>
  <div class="mx-auto max-w-6xl space-y-8 px-6 py-7 sm:px-8">
    <!-- Header -->
    <div class="flex flex-wrap items-center justify-between gap-4">
      <div>
        <h1 class="text-2xl font-extrabold tracking-tight text-[var(--md-sys-color-on-surface)]">
          {{ t('continueView.title') }}
        </h1>
        <p class="mt-0.5 text-xs text-[var(--md-sys-color-on-surface-variant)]">
          {{ t('continueView.subtitle') }}
        </p>
      </div>

      <RefreshButton size="md" :action="loadContinueData" :busy="loading" />
    </div>

    <!-- Empty State -->
    <div
      v-if="!loading && !hasAnyContent"
      class="md-card-filled flex flex-col items-center justify-center rounded-3xl px-6 py-16 text-center"
    >
      <div
        class="mb-4 flex h-14 w-14 items-center justify-center rounded-2xl bg-[var(--md-sys-color-primary-container)] text-[var(--md-sys-color-on-primary-container)]"
      >
        <LuClock class="h-7 w-7" />
      </div>
      <h2 class="text-base font-bold text-[var(--md-sys-color-on-surface)]">
        {{ t('continueView.emptyTitle') }}
      </h2>
      <p class="mt-1.5 max-w-md text-xs leading-relaxed text-[var(--md-sys-color-on-surface-variant)]">
        {{ t('continueView.emptyDesc') }}
      </p>
      <button
        type="button"
        class="md-btn md-btn-filled md-state mt-5"
        @click="router.push('/music/songs')"
      >
        <LuPlay class="h-3.5 w-3.5 fill-current" />
        <span>{{ t('continueView.browseCatalog') }}</span>
      </button>
    </div>

    <template v-else-if="hasAnyContent">
      <!-- 1. Saved / Active Play Queue Resume Card -->
      <section v-if="resumeSession">
        <h2 class="mb-3 text-base font-bold text-[var(--md-sys-color-on-surface)]">
          {{ t('continueView.savedQueueSection') }}
        </h2>
        <div
          class="md-card-filled flex flex-col justify-between gap-5 rounded-3xl border border-[var(--md-sys-color-outline-variant)]/50 p-5 sm:flex-row sm:items-center"
        >
          <div class="flex min-w-0 flex-1 items-center gap-4">
            <CoverImage
              :cover-id="resumeSession.track.id"
              :has-cover="Boolean(resumeSession.track.cover_key)"
              :size="240"
              rounded="rounded-2xl"
              class="album-shadow h-20 w-20 shrink-0 sm:h-24 sm:w-24"
            />
            <div class="min-w-0 flex-1 space-y-1.5">
              <div class="flex flex-wrap items-center gap-2">
                <span
                  class="inline-flex items-center gap-1 rounded-full bg-[var(--md-sys-color-primary-container)] px-2.5 py-0.5 text-[11px] font-semibold text-[var(--md-sys-color-on-primary-container)]"
                >
                  <LuListMusic class="h-3 w-3" />
                  {{ t('continueView.queueCountBadge', { count: resumeSession.tracks.length }) }}
                </span>
                <span class="font-mono text-[11px] text-[var(--md-sys-color-on-surface-variant)]">
                  {{
                    t('continueView.resumeAt', {
                      time: formatTime(resumeSession.posSecs),
                      percent: resumeSession.percent,
                    })
                  }}
                </span>
              </div>

              <h3 class="truncate text-base font-extrabold text-[var(--md-sys-color-on-surface)]">
                {{ resumeSession.track.title }}
              </h3>
              <p class="truncate text-xs text-[var(--md-sys-color-on-surface-variant)]">
                {{ resumeSession.track.artist_name }} · {{ resumeSession.track.album_title }}
              </p>

              <div
                class="h-1.5 w-full max-w-md overflow-hidden rounded-full bg-[var(--md-sys-color-surface-container-highest)]"
              >
                <div
                  class="h-full rounded-full bg-[var(--md-sys-color-primary)] transition-all"
                  :style="{ width: `${resumeSession.percent}%` }"
                />
              </div>
            </div>
          </div>

          <div class="flex shrink-0 items-center gap-2.5">
            <button
              type="button"
              class="md-btn md-btn-filled md-state"
              @click="handleResumeQueue"
            >
              <LuPlay class="h-3.5 w-3.5 fill-current" />
              <span>{{ t('continueView.resumeBtn') }}</span>
            </button>

            <button
              type="button"
              class="md-btn md-btn-tonal md-state"
              @click="playerStore.queueOpen = true"
            >
              <LuListMusic class="h-3.5 w-3.5" />
              <span>{{ t('continueView.openQueueBtn') }}</span>
            </button>
          </div>
        </div>
      </section>

      <!-- 2. Audio Bookmarks -->
      <section v-if="bookmarks.length > 0">
        <h2 class="mb-3 text-base font-bold text-[var(--md-sys-color-on-surface)]">
          {{ t('continueView.bookmarksSection') }}
        </h2>
        <div class="grid grid-cols-1 gap-3 md:grid-cols-2">
          <div
            v-for="bm in bookmarks"
            :key="bm.track.id"
            class="group md-card-filled flex items-center justify-between gap-3.5 border border-[var(--md-sys-color-outline-variant)]/50 p-3.5 transition hover:border-[var(--md-sys-color-primary)]/50 hover:bg-[var(--md-sys-color-surface-container)]"
          >
            <button
              type="button"
              class="flex min-w-0 flex-1 items-center gap-3.5 text-left"
              @click="playBookmark(bm)"
            >
              <CoverImage
                :cover-id="bm.track.id"
                :has-cover="Boolean(bm.track.cover_key)"
                :size="160"
                rounded="rounded-xl"
                class="h-14 w-14 shrink-0"
              />
              <div class="min-w-0 flex-1">
                <p
                  class="truncate text-xs font-bold text-[var(--md-sys-color-on-surface)] group-hover:text-[var(--md-sys-color-primary)]"
                >
                  {{ bm.track.title }}
                </p>
                <p class="mt-0.5 truncate text-[11px] text-[var(--md-sys-color-on-surface-variant)]">
                  {{ bm.track.artist_name }}
                </p>
                <div class="mt-1.5 flex items-center gap-2">
                  <span
                    class="inline-flex items-center gap-1 rounded-md bg-[var(--md-sys-color-primary-container)] px-2 py-0.5 font-mono text-[10px] font-semibold text-[var(--md-sys-color-on-primary-container)]"
                  >
                    <LuBookmark class="h-2.5 w-2.5" />
                    {{
                      t('continueView.resumeAt', {
                        time: formatTime(bm.position_ms / 1000),
                        percent: bookmarkPercent(bm),
                      })
                    }}
                  </span>
                  <span
                    v-if="bm.comment"
                    class="truncate text-[11px] text-[var(--md-sys-color-on-surface-variant)]"
                  >
                    {{ bm.comment }}
                  </span>
                </div>
              </div>
            </button>

            <div class="flex shrink-0 items-center gap-1">
              <button
                type="button"
                class="md-icon-btn md-icon-btn-sm md-state bg-[var(--md-sys-color-primary-container)] text-[var(--md-sys-color-on-primary-container)] hover:bg-[var(--md-sys-color-primary)] hover:text-[var(--md-sys-color-on-primary)]"
                :title="t('continueView.resumeBtn')"
                @click="playBookmark(bm)"
              >
                <LuPlay class="h-3.5 w-3.5 fill-current" />
              </button>
              <button
                type="button"
                class="md-icon-btn md-icon-btn-sm md-state hover:text-[var(--md-sys-color-error)]"
                :title="t('common.delete')"
                @click="deleteBookmark(bm.track.id)"
              >
                <LuTrash2 class="h-3.5 w-3.5" />
              </button>
            </div>
          </div>
        </div>
      </section>

      <!-- 3. Recently Played Albums -->
      <section v-if="recentAlbums.length > 0">
        <h2 class="mb-3 text-base font-bold text-[var(--md-sys-color-on-surface)]">
          {{ t('continueView.recentAlbumsSection') }}
        </h2>
        <div class="grid grid-cols-2 gap-3 sm:grid-cols-3 md:grid-cols-4 lg:grid-cols-6">
          <AlbumCard v-for="al in recentAlbums" :key="al.id" :album="al" />
        </div>
      </section>

      <!-- 4. Recently Played Tracks -->
      <section v-if="recentTracks.length > 0">
        <div class="mb-3 flex flex-wrap items-center justify-between gap-3">
          <h2 class="text-base font-bold text-[var(--md-sys-color-on-surface)]">
            {{ t('continueView.recentTracksSection') }}
          </h2>
          <div class="flex items-center gap-2">
            <button
              type="button"
              class="md-btn md-btn-filled md-btn-sm md-state"
              @click="playerStore.playQueue(recentTracks, 0)"
            >
              <LuPlay class="h-3.5 w-3.5 fill-current" />
              <span>{{ t('common.playAll') }}</span>
            </button>
            <button
              type="button"
              class="md-btn md-btn-tonal md-btn-sm md-state"
              @click="playerStore.shuffleAndPlay(recentTracks)"
            >
              <LuShuffle class="h-3.5 w-3.5" />
              <span>{{ t('common.shufflePlay') }}</span>
            </button>
          </div>
        </div>

        <TrackTable :tracks="recentTracks" />
      </section>
    </template>
  </div>
</template>

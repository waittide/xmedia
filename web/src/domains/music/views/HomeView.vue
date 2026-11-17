<script setup lang="ts">
import { onMounted, ref, watch } from 'vue'
import { LuPlay, LuShuffle, LuRefreshCw, LuDisc3, LuMusic, LuMic, LuHeart } from 'vue-icons-plus/lu'
import { useUploadStore } from '@/stores/uploads'
import { api } from '../api'
import AlbumCard from '../components/AlbumCard.vue'
import TrackTable from '../components/TrackTable.vue'
import { useMusicI18n as useI18n } from '../i18n'
import { usePlayerStore } from '../player'
import { useMusicStore } from '../store'
import type { HomeOverviewDto } from '../types'

const { t } = useI18n()
const musicStore = useMusicStore()
const playerStore = usePlayerStore()
const uploadStore = useUploadStore()

const loading = ref(true)
const overview = ref<HomeOverviewDto | null>(null)

async function loadHome(): Promise<void> {
  loading.value = true
  try {
    overview.value = await api.homeOverview()
  } finally {
    loading.value = false
  }
}

async function refreshRandomPicks(): Promise<void> {
  if (!overview.value) return
  overview.value.random_tracks = await api.randomTracks(12)
}

onMounted(loadHome)
watch(() => [musicStore.catalogVersion, uploadStore.versionCounter], loadHome)
</script>

<template>
  <div class="mx-auto max-w-6xl space-y-8 px-6 py-7 sm:px-8">
    <!-- Hero Header & Library Stats -->
    <div class="flex flex-wrap items-end justify-between gap-4">
      <div>
        <h1 class="text-2xl font-extrabold tracking-tight">{{ t('home.welcomeTitle') }}</h1>
        <p class="mt-1 text-xs text-[var(--ctp-subtext0)]">{{ t('app.tagline') }}</p>
      </div>

      <div
        v-if="overview && overview.random_tracks.length > 0"
        class="flex items-center gap-2"
      >
        <button
          type="button"
          class="inline-flex items-center gap-2 rounded-full bg-[var(--ctp-accent)] px-4 py-2 text-xs font-bold text-[var(--ctp-on-accent)] shadow-sm transition hover:opacity-95"
          @click="playerStore.playQueue(overview.random_tracks, 0)"
        >
          <LuPlay class="h-3.5 w-3.5 fill-current" />
          <span>{{ t('common.playAll') }}</span>
        </button>
        <button
          type="button"
          class="inline-flex items-center gap-2 rounded-full border border-[var(--ctp-surface0)] bg-[var(--ctp-shell)] px-4 py-2 text-xs font-semibold text-[var(--ctp-text)] transition hover:bg-[var(--ctp-surface0)]"
          @click="playerStore.shuffleAndPlay(overview.random_tracks)"
        >
          <LuShuffle class="h-3.5 w-3.5" />
          <span>{{ t('common.shufflePlay') }}</span>
        </button>
      </div>
    </div>

    <!-- Stats Row -->
    <div v-if="overview" class="grid grid-cols-2 gap-3 sm:grid-cols-4">
      <div class="card-soft flex items-center gap-3.5 rounded-2xl bg-[var(--ctp-shell)] p-4">
        <div class="flex h-10 w-10 items-center justify-center rounded-xl bg-[var(--ctp-accent-soft)] text-[var(--ctp-accent)]">
          <LuMusic class="h-5 w-5" />
        </div>
        <div>
          <p class="text-lg font-extrabold">{{ overview.stats.total_tracks }}</p>
          <p class="text-[11px] text-[var(--ctp-subtext0)]">{{ t('home.statTracks') }}</p>
        </div>
      </div>

      <div class="card-soft flex items-center gap-3.5 rounded-2xl bg-[var(--ctp-shell)] p-4">
        <div class="flex h-10 w-10 items-center justify-center rounded-xl bg-[var(--ctp-accent-soft)] text-[var(--ctp-accent)]">
          <LuDisc3 class="h-5 w-5" />
        </div>
        <div>
          <p class="text-lg font-extrabold">{{ overview.stats.total_albums }}</p>
          <p class="text-[11px] text-[var(--ctp-subtext0)]">{{ t('home.statAlbums') }}</p>
        </div>
      </div>

      <div class="card-soft flex items-center gap-3.5 rounded-2xl bg-[var(--ctp-shell)] p-4">
        <div class="flex h-10 w-10 items-center justify-center rounded-xl bg-[var(--ctp-accent-soft)] text-[var(--ctp-accent)]">
          <LuMic class="h-5 w-5" />
        </div>
        <div>
          <p class="text-lg font-extrabold">{{ overview.stats.total_artists }}</p>
          <p class="text-[11px] text-[var(--ctp-subtext0)]">{{ t('home.statArtists') }}</p>
        </div>
      </div>

      <div class="card-soft flex items-center gap-3.5 rounded-2xl bg-[var(--ctp-shell)] p-4">
        <div class="flex h-10 w-10 items-center justify-center rounded-xl bg-[var(--ctp-red)]/15 text-[var(--ctp-red)]">
          <LuHeart class="h-5 w-5" />
        </div>
        <div>
          <p class="text-lg font-extrabold">{{ overview.stats.starred_tracks }}</p>
          <p class="text-[11px] text-[var(--ctp-subtext0)]">{{ t('home.statStarred') }}</p>
        </div>
      </div>
    </div>

    <!-- Empty Library Onboarding -->
    <div
      v-if="!loading && overview && overview.stats.total_tracks === 0"
      class="card-soft flex flex-col items-center justify-center rounded-3xl bg-[var(--ctp-shell)] px-6 py-16 text-center"
    >
      <div class="mb-4 flex h-14 w-14 items-center justify-center rounded-2xl bg-[var(--ctp-accent-soft)] text-[var(--ctp-accent)]">
        <LuDisc3 class="h-7 w-7" />
      </div>
      <h2 class="text-base font-bold">{{ t('home.emptyLibraryTitle') }}</h2>
      <p class="mt-1.5 max-w-md text-xs leading-relaxed text-[var(--ctp-subtext0)]">
        {{ t('home.emptyLibraryDesc') }}
      </p>
    </div>

    <template v-else-if="overview">
      <!-- Recently Added Albums -->
      <section v-if="overview.recent_added_albums.length > 0">
        <h2 class="mb-3 text-base font-bold">{{ t('home.recentAddedAlbums') }}</h2>
        <div class="grid grid-cols-2 gap-3 sm:grid-cols-3 md:grid-cols-4 lg:grid-cols-6">
          <AlbumCard
            v-for="al in overview.recent_added_albums"
            :key="al.id"
            :album="al"
          />
        </div>
      </section>

      <!-- Most Played Albums -->
      <section v-if="overview.frequent_albums.length > 0">
        <h2 class="mb-3 text-base font-bold">{{ t('home.frequentAlbums') }}</h2>
        <div class="grid grid-cols-2 gap-3 sm:grid-cols-3 md:grid-cols-4 lg:grid-cols-6">
          <AlbumCard
            v-for="al in overview.frequent_albums"
            :key="`freq-${al.id}`"
            :album="al"
          />
        </div>
      </section>

      <!-- Recently Played Tracks -->
      <section v-if="overview.recent_played_tracks.length > 0">
        <h2 class="mb-3 text-base font-bold">{{ t('home.recentPlayed') }}</h2>
        <TrackTable :tracks="overview.recent_played_tracks" />
      </section>

      <!-- Random Discovery -->
      <section v-if="overview.random_tracks.length > 0">
        <div class="mb-3 flex items-center justify-between">
          <h2 class="text-base font-bold">{{ t('home.randomDiscovery') }}</h2>
          <button
            type="button"
            class="inline-flex items-center gap-1.5 rounded-xl border border-[var(--ctp-surface0)] bg-[var(--ctp-shell)] px-3 py-1.5 text-xs font-medium text-[var(--ctp-subtext1)] transition hover:text-[var(--ctp-text)]"
            @click="refreshRandomPicks"
          >
            <LuRefreshCw class="h-3.5 w-3.5" />
            <span>{{ t('home.refreshRandom') }}</span>
          </button>
        </div>
        <TrackTable :tracks="overview.random_tracks" />
      </section>
    </template>
  </div>
</template>

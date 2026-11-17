<script setup lang="ts">
import { onMounted, ref, watch } from 'vue'
import { LuFilm, LuTv, LuClock, LuFolderHeart } from 'vue-icons-plus/lu'
import { useUploadStore } from '@/stores/uploads'
import { api } from '../api'
import ShowCard from '../components/ShowCard.vue'
import VideoCard from '../components/VideoCard.vue'
import VideoMetadataModal from '../components/VideoMetadataModal.vue'
import { useVideoI18n as useI18n } from '../i18n'
import { useVideoStore } from '../store'
import type { VideoHomeOverviewDto, VideoSummaryDto } from '../types'

const { t } = useI18n()
const videoStore = useVideoStore()
const uploadStore = useUploadStore()

const loading = ref(true)
const overview = ref<VideoHomeOverviewDto | null>(null)
const editingVideo = ref<VideoSummaryDto | null>(null)

async function loadHome(): Promise<void> {
  loading.value = true
  try {
    overview.value = await api.homeOverview()
  } finally {
    loading.value = false
  }
}

onMounted(loadHome)
watch(() => [videoStore.catalogVersion, uploadStore.versionCounter], loadHome)
</script>

<template>
  <div class="mx-auto max-w-6xl space-y-8 px-6 py-7 sm:px-8">
    <!-- Hero Header -->
    <div>
      <h1 class="text-2xl font-extrabold tracking-tight">{{ t('home.welcomeTitle') }}</h1>
      <p class="mt-1 text-xs text-[var(--ctp-subtext0)]">{{ t('app.tagline') }}</p>
    </div>

    <!-- Stats Row -->
    <div v-if="overview" class="grid grid-cols-2 gap-3 sm:grid-cols-4">
      <div class="card-soft flex items-center gap-3.5 rounded-2xl bg-[var(--ctp-shell)] p-4">
        <div class="flex h-10 w-10 items-center justify-center rounded-xl bg-[var(--ctp-accent-soft)] text-[var(--ctp-accent)]">
          <LuFilm class="h-5 w-5" />
        </div>
        <div>
          <p class="text-lg font-extrabold">{{ overview.total_videos }}</p>
          <p class="text-[11px] text-[var(--ctp-subtext0)]">{{ t('home.statVideos') }}</p>
        </div>
      </div>

      <div class="card-soft flex items-center gap-3.5 rounded-2xl bg-[var(--ctp-shell)] p-4">
        <div class="flex h-10 w-10 items-center justify-center rounded-xl bg-[var(--ctp-accent-soft)] text-[var(--ctp-accent)]">
          <LuTv class="h-5 w-5" />
        </div>
        <div>
          <p class="text-lg font-extrabold">{{ overview.total_shows }}</p>
          <p class="text-[11px] text-[var(--ctp-subtext0)]">{{ t('home.statShows') }}</p>
        </div>
      </div>

      <div class="card-soft flex items-center gap-3.5 rounded-2xl bg-[var(--ctp-shell)] p-4">
        <div class="flex h-10 w-10 items-center justify-center rounded-xl bg-[var(--ctp-accent-soft)] text-[var(--ctp-accent)]">
          <LuClock class="h-5 w-5" />
        </div>
        <div>
          <p class="text-lg font-extrabold">{{ overview.continue_watching.length }}</p>
          <p class="text-[11px] text-[var(--ctp-subtext0)]">{{ t('home.statContinue') }}</p>
        </div>
      </div>

      <div class="card-soft flex items-center gap-3.5 rounded-2xl bg-[var(--ctp-shell)] p-4">
        <div class="flex h-10 w-10 items-center justify-center rounded-xl bg-[var(--ctp-red)]/15 text-[var(--ctp-red)]">
          <LuFolderHeart class="h-5 w-5" />
        </div>
        <div>
          <p class="text-lg font-extrabold">{{ videoStore.shelves.length }}</p>
          <p class="text-[11px] text-[var(--ctp-subtext0)]">{{ t('home.statShelves') }}</p>
        </div>
      </div>
    </div>

    <!-- Empty Library Onboarding -->
    <div
      v-if="!loading && overview && overview.total_videos === 0"
      class="card-soft flex flex-col items-center justify-center rounded-3xl bg-[var(--ctp-shell)] px-6 py-16 text-center"
    >
      <div class="mb-4 flex h-14 w-14 items-center justify-center rounded-2xl bg-[var(--ctp-accent-soft)] text-[var(--ctp-accent)]">
        <LuFilm class="h-7 w-7" />
      </div>
      <h2 class="text-base font-bold">{{ t('home.emptyLibraryTitle') }}</h2>
      <p class="mt-1.5 max-w-md text-xs leading-relaxed text-[var(--ctp-subtext0)]">
        {{ t('home.emptyLibraryDesc') }}
      </p>
    </div>

    <template v-else-if="overview">
      <!-- Continue Watching -->
      <section v-if="overview.continue_watching.length > 0">
        <h2 class="mb-3 text-base font-bold">{{ t('home.continueWatching') }}</h2>
        <div class="grid grid-cols-1 gap-3 sm:grid-cols-2 md:grid-cols-3 lg:grid-cols-4">
          <VideoCard
            v-for="v in overview.continue_watching"
            :key="`cont-${v.id}`"
            :video="v"
            @edit="editingVideo = $event"
            @deleted="loadHome"
          />
        </div>
      </section>

      <!-- TV & Anime Series -->
      <section v-if="overview.recent_shows.length > 0">
        <h2 class="mb-3 text-base font-bold">{{ t('home.recentShows') }}</h2>
        <div class="grid grid-cols-2 gap-3 sm:grid-cols-3 md:grid-cols-4 lg:grid-cols-6">
          <ShowCard
            v-for="sh in overview.recent_shows"
            :key="sh.id"
            :show="sh"
          />
        </div>
      </section>

      <!-- Recently Added Videos -->
      <section v-if="overview.recent_videos.length > 0">
        <h2 class="mb-3 text-base font-bold">{{ t('home.recentVideos') }}</h2>
        <div class="grid grid-cols-1 gap-3 sm:grid-cols-2 md:grid-cols-3 lg:grid-cols-4">
          <VideoCard
            v-for="v in overview.recent_videos"
            :key="v.id"
            :video="v"
            @edit="editingVideo = $event"
            @deleted="loadHome"
          />
        </div>
      </section>
    </template>

    <VideoMetadataModal
      :open="editingVideo !== null"
      :video="editingVideo"
      @close="editingVideo = null"
      @saved="loadHome"
    />
  </div>
</template>

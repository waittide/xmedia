<script setup lang="ts">
import { computed, onMounted, ref } from 'vue'
import { LuHeart } from 'vue-icons-plus/lu'
import MdSegmentedButton from '@/components/ui/MdSegmentedButton.vue'
import RefreshButton from '@/components/ui/RefreshButton.vue'
import { api } from '../api'
import ShowCard from '../components/ShowCard.vue'
import VideoCard from '../components/VideoCard.vue'
import VideoMetadataModal from '../components/VideoMetadataModal.vue'
import { useVideoI18n as useI18n } from '../i18n'
import type { VideoFavoritesDto, VideoSummaryDto } from '../types'

const { t } = useI18n()

const activeTab = ref<'videos' | 'shows'>('videos')
const favorites = ref<VideoFavoritesDto>({ videos: [], shows: [] })
const editingVideo = ref<VideoSummaryDto | null>(null)

const tabOptions = computed(() => [
  { value: 'videos' as const, label: `${t('favorites.tabVideos')} (${favorites.value.videos.length})` },
  { value: 'shows' as const, label: `${t('favorites.tabShows')} (${favorites.value.shows.length})` },
])

async function loadFavorites(): Promise<void> {
  favorites.value = await api.listFavorites()
}

onMounted(loadFavorites)
</script>

<template>
  <div class="mx-auto max-w-7xl space-y-6 px-6 py-7 sm:px-8">
    <div class="flex flex-wrap items-center justify-between gap-4">
      <div>
        <h1 class="text-2xl font-extrabold tracking-tight text-[var(--md-sys-color-on-surface)]">
          {{ t('favorites.title') }}
        </h1>
        <p class="mt-0.5 text-xs text-[var(--md-sys-color-on-surface-variant)]">
          {{ t('favorites.subtitle') }}
        </p>
      </div>

      <RefreshButton size="md" :action="loadFavorites" />
    </div>

    <MdSegmentedButton v-model="activeTab" :options="tabOptions" size="md" />

    <div v-if="activeTab === 'videos'">
      <div
        v-if="favorites.videos.length > 0"
        class="grid grid-cols-1 gap-3 sm:grid-cols-2 md:grid-cols-3 lg:grid-cols-4"
      >
        <VideoCard
          v-for="v in favorites.videos"
          :key="v.id"
          :video="v"
          @edit="editingVideo = $event"
          @deleted="loadFavorites"
        />
      </div>
      <div
        v-else
        class="md-card-filled py-16 text-center text-xs text-[var(--md-sys-color-on-surface-variant)]"
      >
        <LuHeart class="mx-auto mb-2 h-6 w-6 opacity-40" />
        {{ t('favorites.empty') }}
      </div>
    </div>

    <div v-else>
      <div
        v-if="favorites.shows.length > 0"
        class="grid grid-cols-2 gap-3 sm:grid-cols-3 md:grid-cols-4 lg:grid-cols-6"
      >
        <ShowCard v-for="sh in favorites.shows" :key="sh.id" :show="sh" />
      </div>
      <div
        v-else
        class="md-card-filled py-16 text-center text-xs text-[var(--md-sys-color-on-surface-variant)]"
      >
        <LuHeart class="mx-auto mb-2 h-6 w-6 opacity-40" />
        {{ t('favorites.empty') }}
      </div>
    </div>

    <VideoMetadataModal
      :open="editingVideo !== null"
      :video="editingVideo"
      @close="editingVideo = null"
      @saved="loadFavorites"
    />
  </div>
</template>

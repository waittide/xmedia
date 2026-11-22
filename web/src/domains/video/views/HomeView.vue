<script setup lang="ts">
import { computed, ref } from 'vue'
import { useRouter } from 'vue-router'
import { useI18n as useGlobalI18n } from 'vue-i18n'
import {
  LuArrowRight,
  LuClapperboard,
  LuClock,
  LuCompass,
  LuFilm,
  LuFolderHeart,
  LuHeart,
  LuLibrary,
  LuPlay,
  LuSparkles,
  LuTag,
  LuTv,
} from 'vue-icons-plus/lu'
import { useHomeOverview } from '@/composables/useHomeOverview'
import { usePreferencesStore } from '@/stores/preferences'
import { useUploadStore } from '@/stores/uploads'
import { formatTime } from '@/utils/format'
import HomeSkeleton from '@/components/HomeSkeleton.vue'
import HomeStatCards, { type StatCardItem } from '@/components/domain/HomeStatCards.vue'
import PosterImage from '../components/PosterImage.vue'
import ShowCard from '../components/ShowCard.vue'
import VideoCard from '../components/VideoCard.vue'
import VideoMetadataModal from '../components/VideoMetadataModal.vue'
import { useVideoI18n as useI18n } from '../i18n'
import { useVideoStore } from '../store'
import type { VideoSummaryDto } from '../types'

defineOptions({ name: 'VideoHomeView' })

const { t } = useI18n()
const { t: globalT } = useGlobalI18n()
const router = useRouter()
const prefsStore = usePreferencesStore()
const videoStore = useVideoStore()
const uploadStore = useUploadStore()

const editingVideo = ref<VideoSummaryDto | null>(null)

const { overview, loading, loadHome, refreshPage } = useHomeOverview('video', {
  refreshSidebar: () => videoStore.refreshSidebar(true),
  watchSources: () => [videoStore.catalogVersion, uploadStore.versionCounter],
})

const statCards = computed<StatCardItem[]>(() => {
  const ov = overview.value
  return prefsStore.workspaceLayout.video.home_cards.map((key) => {
    switch (key) {
      case 'movies':
        return {
          key,
          value: ov?.movies_count ?? 0,
          icon: LuFilm,
          onClick: () =>
            void router.push({ path: '/video/catalog', query: { kind: 'standalone' } }),
        }
      case 'shows':
        return {
          key,
          value: ov?.total_shows ?? 0,
          icon: LuTv,
          onClick: () => void router.push({ path: '/video/catalog', query: { tab: 'shows' } }),
        }
      case 'favorites':
        return {
          key,
          value: ov?.favorites_count ?? 0,
          icon: LuHeart,
          tone: 'rose',
          onClick: () => void router.push('/video/favorites'),
        }
      case 'watching':
        return {
          key,
          value: ov?.watching_count ?? 0,
          icon: LuClock,
          onClick: () => void router.push('/video/continue'),
        }
      case 'shelves':
        return {
          key,
          value: ov?.shelves_count ?? videoStore.shelves.length,
          icon: LuFolderHeart,
          onClick: () => void router.push('/video/shelves'),
        }
      case 'genres':
        return {
          key,
          value: ov?.genres_count ?? videoStore.genres.length,
          icon: LuSparkles,
          onClick: () => void router.push('/video/genres'),
        }
      case 'tags':
        return {
          key,
          value: ov?.tags_count ?? videoStore.tags.length,
          icon: LuTag,
          onClick: () => void router.push('/video/tags'),
        }
      case 'libraries':
        return {
          key,
          value: ov?.libraries_count ?? videoStore.libraries.length,
          icon: LuLibrary,
          onClick: () => void router.push('/video/libraries'),
        }
      case 'all':
      default:
        return {
          key: 'all',
          value: ov?.total_videos ?? 0,
          icon: LuClapperboard,
          onClick: () => void router.push('/video/catalog'),
        }
    }
  })
})

function progressPercent(v: VideoSummaryDto): number {
  const pos = v.progress?.position_secs ?? 0
  const dur = v.progress?.duration_secs || v.duration_secs || 0
  if (dur <= 0) return 0
  return Math.max(3, Math.min(100, Math.round((pos / dur) * 100)))
}

function formatResumeSubtitle(v: VideoSummaryDto): string {
  const pos = v.progress?.position_secs ?? 0
  const dur = v.progress?.duration_secs || v.duration_secs || 0
  const prefix =
    v.show_title && v.episode_number != null
      ? `${v.show_title} S${String(v.season_number ?? 1).padStart(2, '0')}E${String(v.episode_number).padStart(2, '0')} · `
      : ''
  return `${prefix}${formatTime(pos)} / ${formatTime(dur)}`
}
</script>

<template>
  <div class="mx-auto max-w-7xl space-y-10 px-6 py-7 sm:px-8">
    <HomeStatCards
      v-if="overview && overview.total_videos > 0"
      domain="video"
      :cards="statCards"
      :on-refresh="refreshPage"
    />

    <!-- Cold-Load Skeleton -->
    <HomeSkeleton v-if="loading && !overview" />

    <!-- Empty Library Onboarding -->
    <section
      v-else-if="!loading && overview && overview.total_videos === 0"
      class="md-card-filled flex flex-col items-center justify-center rounded-3xl px-6 py-16 text-center"
    >
      <div
        class="mb-4 flex h-14 w-14 items-center justify-center rounded-2xl bg-[var(--md-sys-color-primary-container)] text-[var(--md-sys-color-on-primary-container)]"
      >
        <LuFilm class="h-7 w-7" />
      </div>
      <h2 class="text-lg font-bold text-[var(--md-sys-color-on-surface)]">
        {{ t('home.emptyLibraryTitle') }}
      </h2>
      <p class="mt-1.5 max-w-md text-xs leading-relaxed text-[var(--md-sys-color-on-surface-variant)]">
        {{ t('home.emptyLibraryDesc') }}
      </p>
      <div class="mt-6 flex flex-wrap items-center justify-center gap-3">
        <RouterLink
          :to="{ path: '/video/catalog', query: { scope: 'online' } }"
          class="md-btn md-btn-tonal md-state"
        >
          <LuCompass class="h-4 w-4" />
          <span>{{ globalT('nav.browseOnlineSources') }}</span>
        </RouterLink>
      </div>
    </section>

    <template v-else-if="overview">
      <!-- Continue Watching (Unified Horizontal Cards) -->
      <section v-if="overview.continue_watching.length > 0">
        <div class="mb-5 flex items-end justify-between">
          <div>
            <h2 class="text-lg font-bold tracking-tight text-[var(--md-sys-color-on-surface)]">
              {{ t('home.continueWatching') }}
            </h2>
          </div>
          <RouterLink
            to="/video/continue"
            class="md-btn md-btn-sm md-btn-text md-state"
          >
            <span>{{ t('collections.seeAll') }}</span>
            <LuArrowRight class="h-3.5 w-3.5" />
          </RouterLink>
        </div>

        <div class="grid grid-cols-1 gap-4 md:grid-cols-2 xl:grid-cols-3">
          <RouterLink
            v-for="v in overview.continue_watching.slice(0, 6)"
            :key="`cont-${v.id}`"
            :to="`/video/watch/${v.id}`"
            class="md-card-filled md-state group flex items-center gap-4 p-3.5 transition duration-200 hover:-translate-y-0.5 hover:bg-[var(--md-sys-color-surface-container-high)] hover:shadow-[var(--md-sys-elevation-1)]"
          >
            <PosterImage
              :poster-key="v.poster_key"
              :alt="v.title"
              :size="320"
              aspect="video"
              rounded="rounded-xl"
              class="h-24 w-36 shrink-0 shadow-[var(--md-sys-elevation-1)]"
            />

            <div class="flex min-w-0 flex-1 flex-col justify-center">
              <h3
                class="truncate text-sm font-bold text-[var(--md-sys-color-on-surface)] transition-colors group-hover:text-[var(--md-sys-color-primary)]"
              >
                {{ v.title }}
              </h3>
              <p class="mt-1.5 truncate text-xs text-[var(--md-sys-color-on-surface-variant)]">
                {{ formatResumeSubtitle(v) }}
              </p>

              <div class="mt-4 flex items-center gap-3">
                <div
                  class="h-1.5 flex-1 overflow-hidden rounded-full bg-[var(--md-sys-color-surface-container-highest)]"
                >
                  <div
                    class="h-full rounded-full bg-[var(--md-sys-color-primary)] transition-all duration-300"
                    :style="{ width: `${progressPercent(v)}%` }"
                  />
                </div>
                <span
                  class="flex h-7 w-7 shrink-0 items-center justify-center rounded-full bg-[var(--md-sys-color-primary)] text-[var(--md-sys-color-on-primary)] shadow-[var(--md-sys-elevation-1)] transition group-hover:scale-105"
                >
                  <LuPlay class="ml-0.5 h-3.5 w-3.5 fill-current" />
                </span>
              </div>
            </div>
          </RouterLink>
        </div>
      </section>

      <!-- TV & Anime Series -->
      <section v-if="overview.recent_shows.length > 0">
        <div class="mb-5 flex items-end justify-between">
          <h2 class="text-lg font-bold tracking-tight text-[var(--md-sys-color-on-surface)]">
            {{ t('home.recentShows') }}
          </h2>
          <RouterLink
            :to="{ path: '/video/catalog', query: { tab: 'shows' } }"
            class="md-btn md-btn-sm md-btn-text md-state"
          >
            <span>{{ t('collections.seeAll') }}</span>
            <LuArrowRight class="h-3.5 w-3.5" />
          </RouterLink>
        </div>

        <div class="grid grid-cols-2 gap-4 sm:grid-cols-3 md:grid-cols-4 lg:grid-cols-6">
          <ShowCard v-for="sh in overview.recent_shows" :key="sh.id" :show="sh" />
        </div>
      </section>

      <!-- Recently Added Videos -->
      <section v-if="overview.recent_videos.length > 0">
        <div class="mb-5 flex items-end justify-between">
          <h2 class="text-lg font-bold tracking-tight text-[var(--md-sys-color-on-surface)]">
            {{ t('home.recentVideos') }}
          </h2>
          <RouterLink
            to="/video/catalog"
            class="md-btn md-btn-sm md-btn-text md-state"
          >
            <span>{{ t('collections.seeAll') }}</span>
            <LuArrowRight class="h-3.5 w-3.5" />
          </RouterLink>
        </div>

        <div class="grid grid-cols-1 gap-4 sm:grid-cols-2 md:grid-cols-3 lg:grid-cols-4">
          <VideoCard
            v-for="v in overview.recent_videos"
            :key="v.id"
            :video="v"
            @edit="editingVideo = $event"
            @deleted="() => loadHome(true)"
          />
        </div>
      </section>
    </template>

    <VideoMetadataModal
      :open="editingVideo !== null"
      :video="editingVideo"
      @close="editingVideo = null"
      @saved="() => loadHome(true)"
    />
  </div>
</template>

<script setup lang="ts">
import { computed, onActivated, onMounted, ref, watch, type Component } from 'vue'
import { useRouter } from 'vue-router'
import { useI18n as useGlobalI18n } from 'vue-i18n'
import {
  LuArrowRight,
  LuClapperboard,
  LuClock,
  LuFilm,
  LuFolderHeart,
  LuHeart,
  LuLibrary,
  LuPlay,
  LuSlidersHorizontal,
  LuSparkles,
  LuTag,
  LuTv,
} from 'vue-icons-plus/lu'
import { fetchHomeOverview, peekHomeCache } from '@/config/homeCache'
import { useThemeStore } from '@/stores/theme'
import { useUploadStore } from '@/stores/uploads'
import HomeSkeleton from '@/components/HomeSkeleton.vue'
import LayoutCustomizerModal from '@/components/LayoutCustomizerModal.vue'
import PosterImage from '../components/PosterImage.vue'
import ShowCard from '../components/ShowCard.vue'
import VideoCard from '../components/VideoCard.vue'
import VideoMetadataModal from '../components/VideoMetadataModal.vue'
import { useVideoI18n as useI18n } from '../i18n'
import { useVideoStore } from '../store'
import type { VideoHomeOverviewDto, VideoSummaryDto } from '../types'

defineOptions({ name: 'VideoHomeView' })

const { t } = useI18n()
const { t: globalT } = useGlobalI18n()
const router = useRouter()
const themeStore = useThemeStore()
const videoStore = useVideoStore()
const uploadStore = useUploadStore()

const overview = ref<VideoHomeOverviewDto | null>(peekHomeCache('video'))
const loading = ref(overview.value === null)
const editingVideo = ref<VideoSummaryDto | null>(null)
const customizerOpen = ref(false)

async function loadHome(force = false): Promise<void> {
  if (!overview.value) loading.value = true
  try {
    overview.value = await fetchHomeOverview('video', { force })
  } finally {
    loading.value = false
  }
}

onMounted(() => void loadHome(false))
onActivated(() => void loadHome(false))
watch(
  () => [videoStore.catalogVersion, uploadStore.versionCounter],
  () => void loadHome(true),
)

interface StatCardItem {
  key: string
  label: string
  value: number
  icon: Component
  tone?: 'accent' | 'rose'
  onClick: () => void
}

const statCards = computed<StatCardItem[]>(() => {
  const ov = overview.value
  const keys = themeStore.workspaceLayout.video.home_cards
  return keys.map((key) => {
    const label = globalT(`layoutCustomizer.cards.video.${key}`)
    switch (key) {
      case 'movies':
        return {
          key,
          label,
          value: ov?.movies_count ?? 0,
          icon: LuFilm,
          onClick: () =>
            void router.push({ path: '/video/catalog', query: { kind: 'standalone' } }),
        }
      case 'shows':
        return {
          key,
          label,
          value: ov?.total_shows ?? 0,
          icon: LuTv,
          onClick: () => void router.push({ path: '/video/catalog', query: { tab: 'shows' } }),
        }
      case 'favorites':
        return {
          key,
          label,
          value: ov?.favorites_count ?? 0,
          icon: LuHeart,
          tone: 'rose',
          onClick: () => void router.push('/video/favorites'),
        }
      case 'watching':
        return {
          key,
          label,
          value: ov?.watching_count ?? 0,
          icon: LuClock,
          onClick: () => void router.push('/video/continue'),
        }
      case 'shelves':
        return {
          key,
          label,
          value: ov?.shelves_count ?? videoStore.shelves.length,
          icon: LuFolderHeart,
          onClick: () => void router.push('/video/shelves'),
        }
      case 'genres':
        return {
          key,
          label,
          value: ov?.genres_count ?? videoStore.genres.length,
          icon: LuSparkles,
          onClick: () => void router.push('/video/genres'),
        }
      case 'tags':
        return {
          key,
          label,
          value: ov?.tags_count ?? videoStore.tags.length,
          icon: LuTag,
          onClick: () => void router.push('/video/tags'),
        }
      case 'libraries':
        return {
          key,
          label,
          value: ov?.libraries_count ?? videoStore.libraries.length,
          icon: LuLibrary,
          onClick: () => void router.push('/video/libraries'),
        }
      case 'all':
      default:
        return {
          key: 'all',
          label,
          value: ov?.total_videos ?? 0,
          icon: LuClapperboard,
          onClick: () => void router.push('/video/catalog'),
        }
    }
  })
})

const cardsGridClass = computed(() => {
  const n = statCards.value.length
  if (n <= 2) return 'grid-cols-1 sm:grid-cols-2'
  if (n === 3) return 'grid-cols-1 sm:grid-cols-3'
  return 'grid-cols-2 lg:grid-cols-4'
})

function formatClock(secs: number): string {
  const s = Math.max(0, Math.floor(secs))
  const h = Math.floor(s / 3600)
  const m = Math.floor((s % 3600) / 60)
  const rem = s % 60
  if (h > 0) {
    return `${h}:${String(m).padStart(2, '0')}:${String(rem).padStart(2, '0')}`
  }
  return `${m}:${String(rem).padStart(2, '0')}`
}

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
  return `${prefix}${formatClock(pos)} / ${formatClock(dur)}`
}
</script>

<template>
  <div class="mx-auto max-w-7xl space-y-10 px-6 py-8 sm:px-8">
    <!-- Customizable Video Stat Cards (1..=4) -->
    <section class="space-y-2.5">
      <div class="flex items-center justify-end">
        <button
          type="button"
          class="inline-flex items-center gap-1.5 rounded-lg px-2 py-1 text-[11px] font-medium text-[var(--ctp-subtext0)] transition hover:bg-[var(--ctp-shell)] hover:text-[var(--ctp-text)]"
          @click="customizerOpen = true"
        >
          <LuSlidersHorizontal class="h-3.5 w-3.5" />
          <span>{{ globalT('layoutCustomizer.customizeCards') }}</span>
        </button>
      </div>

      <div class="grid gap-4" :class="cardsGridClass">
        <button
          v-for="card in statCards"
          :key="card.key"
          type="button"
          class="card-soft flex items-center gap-3.5 rounded-2xl bg-[var(--ctp-shell)] p-4 text-left transition hover:border-[var(--ctp-accent)]"
          @click="card.onClick()"
        >
          <div
            class="flex h-10 w-10 shrink-0 items-center justify-center rounded-xl"
            :class="
              card.tone === 'rose'
                ? 'bg-[var(--ctp-red)]/15 text-[var(--ctp-red)]'
                : 'bg-[var(--ctp-accent-soft)] text-[var(--ctp-accent)]'
            "
          >
            <component :is="card.icon" class="h-5 w-5" />
          </div>
          <div class="min-w-0">
            <p class="text-xl font-bold leading-none text-[var(--ctp-text)]">{{ card.value }}</p>
            <p class="mt-1 truncate text-xs text-[var(--ctp-subtext0)]">{{ card.label }}</p>
          </div>
        </button>
      </div>
    </section>

    <!-- Cold-Load Skeleton -->
    <HomeSkeleton v-if="loading && !overview" />

    <!-- Empty Library Onboarding -->
    <section
      v-else-if="!loading && overview && overview.total_videos === 0"
      class="card-soft flex flex-col items-center justify-center rounded-3xl bg-[var(--ctp-shell)] px-6 py-16 text-center"
    >
      <div
        class="mb-4 flex h-14 w-14 items-center justify-center rounded-2xl bg-[var(--ctp-accent-soft)] text-[var(--ctp-accent)]"
      >
        <LuFilm class="h-7 w-7" />
      </div>
      <h2 class="text-lg font-bold">{{ t('home.emptyLibraryTitle') }}</h2>
      <p class="mt-1.5 max-w-md text-xs leading-relaxed text-[var(--ctp-subtext0)]">
        {{ t('home.emptyLibraryDesc') }}
      </p>
    </section>

    <template v-else-if="overview">
      <!-- Continue Watching (Unified Horizontal Cards) -->
      <section v-if="overview.continue_watching.length > 0">
        <div class="mb-5 flex items-end justify-between">
          <div>
            <h2 class="text-lg font-bold tracking-tight text-[var(--ctp-text)]">
              {{ t('home.continueWatching') }}
            </h2>
          </div>
          <RouterLink
            to="/video/continue"
            class="inline-flex items-center gap-1 text-xs font-semibold text-[var(--ctp-accent)] hover:underline"
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
            class="group flex items-center gap-4 rounded-2xl border border-[var(--ctp-surface0)]/85 bg-[var(--ctp-shell)]/90 p-3.5 transition duration-200 hover:-translate-y-0.5 hover:border-[var(--ctp-accent)]/60 hover:bg-[var(--ctp-mantle)]"
          >
            <PosterImage
              :poster-key="v.poster_key"
              :alt="v.title"
              :size="320"
              aspect="video"
              rounded="rounded-xl"
              class="h-24 w-36 shrink-0 shadow-md"
            />

            <div class="flex min-w-0 flex-1 flex-col justify-center">
              <h3
                class="truncate text-sm font-bold text-[var(--ctp-text)] transition-colors group-hover:text-[var(--ctp-accent)]"
              >
                {{ v.title }}
              </h3>
              <p class="mt-1.5 truncate text-xs text-[var(--ctp-subtext0)]">
                {{ formatResumeSubtitle(v) }}
              </p>

              <div class="mt-4 flex items-center gap-3">
                <div class="h-1.5 flex-1 overflow-hidden rounded-full bg-[var(--ctp-canvas)]/90">
                  <div
                    class="h-full rounded-full bg-[var(--ctp-accent)] transition-all duration-300"
                    :style="{ width: `${progressPercent(v)}%` }"
                  />
                </div>
                <span
                  class="flex h-7 w-7 shrink-0 items-center justify-center rounded-full bg-[var(--ctp-accent)] text-[var(--ctp-on-accent)] shadow-sm transition group-hover:scale-105"
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
          <h2 class="text-lg font-bold tracking-tight text-[var(--ctp-text)]">
            {{ t('home.recentShows') }}
          </h2>
          <RouterLink
            :to="{ path: '/video/catalog', query: { tab: 'shows' } }"
            class="inline-flex items-center gap-1 text-xs font-semibold text-[var(--ctp-accent)] hover:underline"
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
          <h2 class="text-lg font-bold tracking-tight text-[var(--ctp-text)]">
            {{ t('home.recentVideos') }}
          </h2>
          <RouterLink
            to="/video/catalog"
            class="inline-flex items-center gap-1 text-xs font-semibold text-[var(--ctp-accent)] hover:underline"
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

    <LayoutCustomizerModal
      :open="customizerOpen"
      domain="video"
      initial-tab="cards"
      @close="customizerOpen = false"
    />
  </div>
</template>

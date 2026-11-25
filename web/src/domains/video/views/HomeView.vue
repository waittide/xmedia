<script setup lang="ts">
import { computed, ref, watch } from 'vue'
import { useI18n as useGlobalI18n } from 'vue-i18n'
import { LuChevronRight } from 'vue-icons-plus/lu'
import { DomainHomeTemplate, MediaItemCard } from '@/components/domain'
import { MdButton } from '@/components/ui'
import { useHomeOverview } from '@/composables/useHomeOverview'
import { libraryRoute, useMergedShelf, useSourceFilter, type MediaItemVM } from '@/sources'
import { useAuthStore } from '@/stores/auth'
import { UPLOAD_EXTENSIONS_BY_KIND, useUploadStore } from '@/stores/uploads'
import { buildPosterUrl } from '../api'
import ShowCard from '../components/ShowCard.vue'
import VideoCard from '../components/VideoCard.vue'
import VideoMetadataModal from '../components/VideoMetadataModal.vue'
import { useVideoI18n as useI18n } from '../i18n'
import { useVideoStore } from '../store'
import type { ShowSummaryDto, VideoSummaryDto } from '../types'

defineOptions({ name: 'VideoHomeView' })

const { t } = useI18n()
const { t: globalT } = useGlobalI18n()
const authStore = useAuthStore()
const videoStore = useVideoStore()
const uploadStore = useUploadStore()

const editingVideo = ref<VideoSummaryDto | null>(null)
const refreshing = ref(false)
const uploadInputRef = ref<HTMLInputElement | null>(null)

const { overview, loading: overviewLoading, error: overviewError, loadHome, refreshPage } =
  useHomeOverview('video', {
    refreshSidebar: () => videoStore.refreshSidebar(true),
    watchSources: () => [videoStore.catalogVersion, uploadStore.versionCounter],
  })

const mergedContinue = useMergedShelf('video', 'continue', { limit: 12 })
const mergedFavorites = useMergedShelf('video', 'favorites', { limit: 12 })
const sourceFilter = useSourceFilter('video')

const videoUploadAccept = UPLOAD_EXTENSIONS_BY_KIND.video.map((ext) => `.${ext}`).join(',')

const recentVideos = computed<VideoSummaryDto[]>(() => overview.value?.recent_videos ?? [])
const recentShows = computed<ShowSummaryDto[]>(() => overview.value?.recent_shows ?? [])

const recentVideoById = computed(() => {
  const map = new Map<string, VideoSummaryDto>()
  for (const v of recentVideos.value) {
    map.set(v.id, v)
  }
  return map
})

const recentShowById = computed(() => {
  const map = new Map<string, ShowSummaryDto>()
  for (const sh of recentShows.value) {
    map.set(sh.id, sh)
  }
  return map
})

function mapVideoToVM(video: VideoSummaryDto): MediaItemVM {
  const encodedId = encodeURIComponent(video.id)
  const playTarget = { path: `/video/play/${encodedId}` }
  const detailTarget = !video.show_id
    ? { path: `/video/item/${encodedId}`, query: { kind: 'movie' } }
    : playTarget
  return {
    ref: { scope: 'local', domain: 'video', id: video.id },
    title: video.title,
    subtitle: video.year ? String(video.year) : undefined,
    cover: buildPosterUrl(video.poster_key, 560) ?? undefined,
    badge: { sourceKind: 'local' },
    to: detailTarget,
    playTo: playTarget,
  }
}

function mapShowToVM(show: ShowSummaryDto): MediaItemVM {
  return {
    ref: { scope: 'local', domain: 'video', id: show.id },
    title: show.title,
    subtitle: show.year ? String(show.year) : undefined,
    cover: buildPosterUrl(show.poster_key, 480) ?? undefined,
    badge: { sourceKind: 'local' },
    to: { path: `/video/item/${encodeURIComponent(show.id)}` },
  }
}

function normalizeShelfItemRoute(item: MediaItemVM): MediaItemVM {
  if (item.ref.scope !== 'local') return item
  const target = item.to
  if (typeof target === 'object' && target && 'path' in target && typeof target.path === 'string') {
    if (target.path.startsWith('/video/play/')) {
      return {
        ...item,
        to: { path: `/video/item/${encodeURIComponent(item.ref.id)}`, query: { kind: 'movie' } },
      }
    }
  }
  return item
}

const normalizedFavorites = computed<MediaItemVM[]>(() =>
  mergedFavorites.items.value.map(normalizeShelfItemRoute),
)

const primaryRecentMode = computed<'videos' | 'shows' | 'favorites'>(() => {
  if (recentVideos.value.length > 0) return 'videos'
  if (recentShows.value.length > 0) return 'shows'
  return 'favorites'
})

const primaryRecentItems = computed<MediaItemVM[]>(() => {
  if (primaryRecentMode.value === 'videos') {
    return recentVideos.value.map(mapVideoToVM)
  }
  if (primaryRecentMode.value === 'shows') {
    return recentShows.value.map(mapShowToVM)
  }
  return normalizedFavorites.value
})

const primaryRecentTitle = computed(() => {
  if (primaryRecentMode.value === 'shows') return t('home.recentShows')
  if (primaryRecentMode.value === 'favorites') return t('home.favoritesSection')
  return t('home.recentVideos')
})

const primaryRecentSeeAllTo = computed(() => {
  if (primaryRecentMode.value === 'shows') return libraryRoute('video', { view: 'shows' })
  if (primaryRecentMode.value === 'favorites') return libraryRoute('video', { tab: 'favorites' })
  return libraryRoute('video')
})

const primaryAspect = computed<'video' | 'poster'>(() =>
  primaryRecentMode.value === 'shows' ? 'poster' : 'video',
)

const showExtraShows = computed(
  () => primaryRecentMode.value === 'videos' && recentShows.value.length > 0,
)

const showExtraFavorites = computed(
  () => primaryRecentMode.value !== 'favorites' && normalizedFavorites.value.length > 0,
)

const summaryText = computed(() => {
  const ov = overview.value
  if (!ov || (ov.total_videos === 0 && ov.total_shows === 0)) return ''
  const parts: string[] = []
  if (ov.total_videos > 0) parts.push(t('common.videosCount', { count: ov.total_videos }))
  if (ov.total_shows > 0) parts.push(t('common.showsCount', { count: ov.total_shows }))
  return parts.join(' · ')
})

const combinedLoading = computed(
  () =>
    (overviewLoading.value && !overview.value) ||
    (mergedContinue.loading.value &&
      mergedContinue.items.value.length === 0 &&
      !overview.value),
)

const combinedError = computed(() => overviewError.value ?? mergedContinue.error.value)

async function handleRefresh(): Promise<void> {
  refreshing.value = true
  try {
    await Promise.allSettled([
      refreshPage(),
      mergedContinue.refresh(),
      mergedFavorites.refresh(),
      sourceFilter.refresh(true),
    ])
  } finally {
    refreshing.value = false
  }
}

function handleOpenUpload(): void {
  uploadInputRef.value?.click()
}

function handleUploadFilesChange(ev: Event): void {
  const input = ev.target as HTMLInputElement
  if (input.files && input.files.length > 0) {
    uploadStore.enqueueFiles(input.files)
  }
  input.value = ''
}

watch(
  () => [videoStore.catalogVersion, uploadStore.versionCounter],
  () => {
    void mergedContinue.refresh()
    void mergedFavorites.refresh()
  },
)
</script>

<template>
  <div class="px-4 py-6 sm:px-6 lg:px-8">
    <input
      ref="uploadInputRef"
      type="file"
      multiple
      :accept="videoUploadAccept"
      class="hidden"
      @change="handleUploadFilesChange"
    />

    <DomainHomeTemplate
      domain="video"
      :summary="summaryText"
      :continue-items="mergedContinue.items.value"
      :recent-items="primaryRecentItems"
      :continue-title="t('home.continueWatching')"
      :recent-title="primaryRecentTitle"
      :continue-see-all-to="libraryRoute('video', { tab: 'continue' })"
      :recent-see-all-to="primaryRecentSeeAllTo"
      :aspect="primaryAspect"
      cover-style="plain"
      :loading="combinedLoading"
      :refreshing="refreshing"
      :error="combinedError"
      :is-admin="authStore.isAdmin"
      :can-upload="authStore.isAdmin"
      :has-online-sources="sourceFilter.hasOnlineSources.value"
      :empty-title="t('home.emptyLibraryTitle')"
      :empty-description="t('home.emptyLibraryDesc')"
      @refresh="handleRefresh"
      @retry="handleRefresh"
      @upload="handleOpenUpload"
    >
      <template #recent-card="{ item }">
        <VideoCard
          v-if="item.ref.scope === 'local' && recentVideoById.has(item.ref.id)"
          :video="recentVideoById.get(item.ref.id)!"
          @edit="editingVideo = $event"
          @deleted="() => loadHome(true)"
          @updated="() => mergedFavorites.refresh()"
        />
        <ShowCard
          v-else-if="item.ref.scope === 'local' && recentShowById.has(item.ref.id)"
          :show="recentShowById.get(item.ref.id)!"
        />
        <MediaItemCard
          v-else
          :item="item"
          :aspect="item.ref.scope === 'local' ? 'video' : 'poster'"
          cover-style="plain"
        />
      </template>

      <template #extra-sections>
        <!-- TV & Anime Shows -->
        <section v-if="showExtraShows" class="space-y-3">
          <div class="flex items-center justify-between gap-2">
            <h2 class="text-title-large text-on-surface">
              {{ t('home.recentShows') }}
            </h2>
            <MdButton
              variant="text"
              size="sm"
              :to="libraryRoute('video', { view: 'shows' })"
            >
              <span>{{ globalT('common.viewAll') }}</span>
              <template #trailing>
                <LuChevronRight class="h-4 w-4" aria-hidden="true" />
              </template>
            </MdButton>
          </div>

          <div class="grid grid-cols-2 gap-5 sm:grid-cols-3 md:grid-cols-4 lg:grid-cols-5 xl:grid-cols-6">
            <ShowCard v-for="sh in recentShows" :key="sh.id" :show="sh" />
          </div>
        </section>

        <!-- Cross-Source Favorites (Local + Online) -->
        <section v-if="showExtraFavorites" class="space-y-3">
          <div class="flex items-center justify-between gap-2">
            <h2 class="text-title-large text-on-surface">
              {{ t('home.favoritesSection') }}
            </h2>
            <MdButton
              variant="text"
              size="sm"
              :to="libraryRoute('video', { tab: 'favorites' })"
            >
              <span>{{ globalT('common.viewAll') }}</span>
              <template #trailing>
                <LuChevronRight class="h-4 w-4" aria-hidden="true" />
              </template>
            </MdButton>
          </div>

          <div class="grid grid-cols-1 gap-5 sm:grid-cols-2 lg:grid-cols-3 xl:grid-cols-4">
            <MediaItemCard
              v-for="(fav, idx) in normalizedFavorites.slice(0, 8)"
              :key="fav.ref.scope === 'local' ? `fav-local-${fav.ref.id}-${idx}` : `fav-online-${fav.ref.sourceId}-${fav.ref.ref}-${idx}`"
              :item="fav"
              aspect="video"
              cover-style="plain"
            />
          </div>
        </section>
      </template>
    </DomainHomeTemplate>

    <VideoMetadataModal
      :open="editingVideo !== null"
      :video="editingVideo"
      @close="editingVideo = null"
      @saved="() => loadHome(true)"
    />
  </div>
</template>

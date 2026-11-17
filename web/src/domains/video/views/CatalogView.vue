<script setup lang="ts">
import { computed, onMounted, ref, watch } from 'vue'
import { useRoute, useRouter } from 'vue-router'
import { LuSearch, LuX } from 'vue-icons-plus/lu'
import AppSelect from '@/components/ui/AppSelect.vue'
import { useUploadStore } from '@/stores/uploads'
import { api } from '../api'
import ShowCard from '../components/ShowCard.vue'
import VideoCard from '../components/VideoCard.vue'
import VideoMetadataModal from '../components/VideoMetadataModal.vue'
import { useVideoI18n as useI18n } from '../i18n'
import { useVideoStore } from '../store'
import type { ShowSummaryDto, VideoKindFilter, VideoSortBy, VideoSummaryDto } from '../types'

type CatalogTab = 'videos' | 'shows'

const { t } = useI18n()
const route = useRoute()
const router = useRouter()
const videoStore = useVideoStore()
const uploadStore = useUploadStore()

function resolveTabFromQuery(): CatalogTab {
  return route.query.tab === 'shows' ? 'shows' : 'videos'
}

const activeTab = ref<CatalogTab>(resolveTabFromQuery())
const selectedLibrary = ref(typeof route.query.library === 'string' ? route.query.library : '')
const selectedTag = ref(typeof route.query.tag === 'string' ? route.query.tag : '')
const search = ref(typeof route.query.q === 'string' ? route.query.q : '')
const kindFilter = ref<VideoKindFilter>('all')
const sortKey = ref('created_at:desc')

const videos = ref<VideoSummaryDto[]>([])
const shows = ref<ShowSummaryDto[]>([])
const loading = ref(false)
const editingVideo = ref<VideoSummaryDto | null>(null)

const activeLibraryName = computed(() => {
  if (!selectedLibrary.value) return ''
  return videoStore.libraries.find((l) => l.id === selectedLibrary.value)?.name ?? ''
})

const pageTitle = computed(() => {
  if (activeLibraryName.value) return activeLibraryName.value
  if (selectedTag.value) return selectedTag.value
  return t('nav.all')
})

const pageSubtitle = computed(() =>
  activeTab.value === 'shows'
    ? t('common.showsCount', { count: shows.value.length })
    : t('common.videosCount', { count: videos.value.length }),
)

const libraryOptions = computed(() => [
  { value: '', label: t('catalog.allLibraries') },
  ...videoStore.libraries.map((l) => ({
    value: l.id,
    label: l.name,
    badge: l.video_count,
  })),
])

const tagOptions = computed(() => [
  { value: '', label: t('catalog.allGenres') },
  ...videoStore.tags.map((g) => ({
    value: g.name,
    label: g.name,
    badge: g.count,
  })),
])

const kindOptions = computed(() => [
  { value: 'all', label: t('catalog.kindAll') },
  { value: 'standalone', label: t('catalog.kindStandalone') },
  { value: 'episodes', label: t('catalog.kindEpisodes') },
])

const sortOptions = computed(() => [
  { value: 'created_at:desc', label: t('catalog.sortCreatedDesc') },
  { value: 'title:asc', label: t('catalog.sortTitleAsc') },
  { value: 'year:desc', label: t('catalog.sortYearDesc') },
  { value: 'duration:desc', label: t('catalog.sortDurationDesc') },
])

function syncRouteQuery(): void {
  const nextQuery: Record<string, string> = {}
  if (activeTab.value !== 'videos') nextQuery.tab = activeTab.value
  if (selectedLibrary.value) nextQuery.library = selectedLibrary.value
  if (selectedTag.value) nextQuery.tag = selectedTag.value
  if (search.value.trim()) nextQuery.q = search.value.trim()
  void router.replace({ path: '/video/catalog', query: nextQuery })
}

function selectTab(tab: CatalogTab): void {
  if (activeTab.value === tab) return
  activeTab.value = tab
  syncRouteQuery()
  void fetchActive()
}

function clearScopeFilters(): void {
  selectedLibrary.value = ''
  selectedTag.value = ''
  search.value = ''
  syncRouteQuery()
  void fetchActive()
}

async function fetchVideos(): Promise<void> {
  loading.value = true
  try {
    const [sortBy, order] = sortKey.value.split(':') as [VideoSortBy, string]
    videos.value = await api.listVideos({
      library_id: selectedLibrary.value || undefined,
      kind: kindFilter.value,
      genre: selectedTag.value || undefined,
      search: search.value.trim() || undefined,
      sort_by: sortBy,
      sort_desc: order === 'desc',
    })
  } finally {
    loading.value = false
  }
}

async function fetchShows(): Promise<void> {
  loading.value = true
  try {
    shows.value = await api.listShows({
      library_id: selectedLibrary.value || undefined,
      genre: selectedTag.value || undefined,
      search: search.value.trim() || undefined,
    })
  } finally {
    loading.value = false
  }
}

async function fetchActive(): Promise<void> {
  if (activeTab.value === 'shows') {
    await fetchShows()
  } else {
    await fetchVideos()
  }
}

function handleFilterChange(): void {
  syncRouteQuery()
  void fetchActive()
}

watch(
  () => [route.query.tab, route.query.library, route.query.tag, route.query.q] as const,
  ([qTab, qLib, qTag, qSearch]) => {
    const nextTab: CatalogTab = qTab === 'shows' ? 'shows' : 'videos'
    const nextLib = typeof qLib === 'string' ? qLib : ''
    const nextTag = typeof qTag === 'string' ? qTag : ''
    const nextSearch = typeof qSearch === 'string' ? qSearch : search.value
    const changed =
      nextTab !== activeTab.value ||
      nextLib !== selectedLibrary.value ||
      nextTag !== selectedTag.value ||
      nextSearch !== search.value
    activeTab.value = nextTab
    selectedLibrary.value = nextLib
    selectedTag.value = nextTag
    search.value = nextSearch
    if (changed) {
      void fetchActive()
    }
  },
)

onMounted(fetchActive)
watch(() => [videoStore.catalogVersion, uploadStore.versionCounter], fetchActive)
</script>

<template>
  <div class="mx-auto max-w-6xl space-y-5 px-6 py-7 sm:px-8">
    <!-- Header -->
    <div class="flex flex-wrap items-center justify-between gap-4">
      <div>
        <div class="flex items-center gap-2">
          <h1 class="text-2xl font-extrabold tracking-tight">{{ pageTitle }}</h1>
          <button
            v-if="selectedLibrary || selectedTag"
            type="button"
            class="inline-flex items-center gap-1 rounded-full border border-[var(--ctp-surface0)] bg-[var(--ctp-shell)] px-2.5 py-0.5 text-[11px] font-medium text-[var(--ctp-subtext1)] transition hover:border-[var(--ctp-accent)] hover:text-[var(--ctp-accent)]"
            @click="clearScopeFilters"
          >
            <span>{{ t('nav.all') }}</span>
            <LuX class="h-3 w-3" />
          </button>
        </div>
        <p class="mt-0.5 text-xs text-[var(--ctp-subtext0)]">{{ pageSubtitle }}</p>
      </div>
    </div>

    <!-- Videos / Shows Tabs -->
    <div class="inline-flex gap-1 rounded-2xl bg-[var(--ctp-shell)] p-1">
      <button
        type="button"
        class="rounded-xl px-4 py-2 text-xs font-semibold transition"
        :class="
          activeTab === 'videos'
            ? 'bg-[var(--ctp-accent)] text-[var(--ctp-on-accent)]'
            : 'text-[var(--ctp-subtext1)] hover:text-[var(--ctp-text)]'
        "
        @click="selectTab('videos')"
      >
        {{ t('nav.moviesAndClips') }}
      </button>
      <button
        type="button"
        class="rounded-xl px-4 py-2 text-xs font-semibold transition"
        :class="
          activeTab === 'shows'
            ? 'bg-[var(--ctp-accent)] text-[var(--ctp-on-accent)]'
            : 'text-[var(--ctp-subtext1)] hover:text-[var(--ctp-text)]'
        "
        @click="selectTab('shows')"
      >
        {{ t('nav.shows') }}
      </button>
    </div>

    <!-- Filters Row -->
    <div
      class="grid grid-cols-1 gap-2.5 sm:grid-cols-2"
      :class="activeTab === 'videos' ? 'lg:grid-cols-5' : 'lg:grid-cols-3'"
    >
      <div class="relative">
        <LuSearch
          class="pointer-events-none absolute left-3 top-1/2 h-3.5 w-3.5 -translate-y-1/2 text-[var(--ctp-subtext0)]"
        />
        <input
          v-model="search"
          type="text"
          :placeholder="t('common.filterPlaceholder')"
          class="h-9 w-full rounded-xl border border-[var(--ctp-surface0)] bg-[var(--ctp-canvas)] pl-8.5 pr-3 text-xs outline-none focus:border-[var(--ctp-accent)]"
          @input="fetchActive"
        />
      </div>

      <AppSelect
        v-model="selectedLibrary"
        :options="libraryOptions"
        @change="handleFilterChange"
      />

      <AppSelect
        v-model="selectedTag"
        :options="tagOptions"
        searchable
        @change="handleFilterChange"
      />

      <template v-if="activeTab === 'videos'">
        <AppSelect
          v-model="kindFilter"
          :options="kindOptions"
          @change="fetchVideos"
        />

        <AppSelect
          v-model="sortKey"
          :options="sortOptions"
          @change="fetchVideos"
        />
      </template>
    </div>

    <!-- Results -->
    <template v-if="activeTab === 'videos'">
      <div
        v-if="videos.length === 0 && !loading"
        class="py-16 text-center text-xs text-[var(--ctp-subtext0)]"
      >
        {{ t('common.noResults') }}
      </div>
      <div v-else class="grid grid-cols-1 gap-3 sm:grid-cols-2 md:grid-cols-3 lg:grid-cols-4">
        <VideoCard
          v-for="v in videos"
          :key="v.id"
          :video="v"
          @edit="editingVideo = $event"
          @deleted="fetchVideos"
        />
      </div>
    </template>

    <template v-else>
      <div
        v-if="shows.length === 0 && !loading"
        class="py-16 text-center text-xs text-[var(--ctp-subtext0)]"
      >
        {{ t('common.noResults') }}
      </div>
      <div v-else class="grid grid-cols-2 gap-3 sm:grid-cols-3 md:grid-cols-4 lg:grid-cols-6">
        <ShowCard v-for="sh in shows" :key="sh.id" :show="sh" />
      </div>
    </template>

    <VideoMetadataModal
      :open="editingVideo !== null"
      :video="editingVideo"
      @close="editingVideo = null"
      @saved="fetchVideos"
    />
  </div>
</template>

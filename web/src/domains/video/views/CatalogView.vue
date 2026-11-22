<script setup lang="ts">
import { computed, onBeforeUnmount, onMounted, ref, watch } from 'vue'
import { useRoute, useRouter } from 'vue-router'
import { LuFilm, LuGlobe, LuSearch, LuX } from 'vue-icons-plus/lu'
import AppSelect from '@/components/ui/AppSelect.vue'
import MdSegmentedButton, { type SegmentedOption } from '@/components/ui/MdSegmentedButton.vue'
import RefreshButton from '@/components/ui/RefreshButton.vue'
import { useSnackbarStore } from '@/stores/snackbar'
import { useUploadStore } from '@/stores/uploads'
import { api } from '../api'
import OnlineVideoSearchSection from '../components/OnlineVideoSearchSection.vue'
import ShowCard from '../components/ShowCard.vue'
import VideoCard from '../components/VideoCard.vue'
import VideoMetadataModal from '../components/VideoMetadataModal.vue'
import { useVideoI18n as useI18n } from '../i18n'
import { useVideoStore } from '../store'
import type { ShowSummaryDto, VideoKindFilter, VideoSortBy, VideoSummaryDto } from '../types'

type CatalogTab = 'videos' | 'shows'
type VideoSearchScope = 'local' | 'online'

const { t } = useI18n()
const route = useRoute()
const router = useRouter()
const videoStore = useVideoStore()
const uploadStore = useUploadStore()
const snackbarStore = useSnackbarStore()
let fetchSeq = 0
let searchDebounceTimer: ReturnType<typeof setTimeout> | null = null
let syncingRouteQuery = false
let syncRouteSeq = 0
let pendingRouteQuery: Record<string, string | string[]> | null = null

const searchScope = ref<VideoSearchScope>(route.query.scope === 'online' ? 'online' : 'local')
const onlineKeyword = ref<string>(typeof route.query.q === 'string' ? route.query.q : '')
const onlineSourceId = computed(() =>
  typeof route.query.source === 'string' && route.query.source.trim()
    ? route.query.source.trim()
    : null,
)

const searchScopeOptions = computed<SegmentedOption<VideoSearchScope>[]>(() => [
  { value: 'local', label: t('online.scopeLocal'), icon: LuFilm },
  { value: 'online', label: t('online.scopeOnline'), icon: LuGlobe },
])

function resolveTabFromQuery(): CatalogTab {
  return route.query.tab === 'shows' ? 'shows' : 'videos'
}

function resolveKindFromQuery(): VideoKindFilter {
  const q = route.query.kind
  if (q === 'standalone' || q === 'episodes') return q
  return 'all'
}

const activeTab = ref<CatalogTab>(resolveTabFromQuery())
const selectedLibrary = ref(typeof route.query.library === 'string' ? route.query.library : '')
const selectedGenre = ref(typeof route.query.genre === 'string' ? route.query.genre : '')
const selectedTag = ref(typeof route.query.tag === 'string' ? route.query.tag : '')
const search = ref(typeof route.query.q === 'string' ? route.query.q : '')
const kindFilter = ref<VideoKindFilter>(resolveKindFromQuery())
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
  if (searchScope.value === 'online') return t('online.scopeOnline')
  if (activeLibraryName.value) return activeLibraryName.value
  if (selectedGenre.value) return selectedGenre.value
  if (selectedTag.value) return `#${selectedTag.value}`
  if (activeTab.value === 'videos' && kindFilter.value === 'standalone') {
    return t('catalog.kindStandalone')
  }
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

const genreOptions = computed(() => [
  { value: '', label: t('catalog.allGenres') },
  ...videoStore.genres.map((g) => ({
    value: g.name,
    label: g.name,
    badge: g.count,
  })),
])

const tagOptions = computed(() => [
  { value: '', label: t('collections.tagsTitle') },
  ...videoStore.tags.map((tg) => ({
    value: tg.name,
    label: `#${tg.name}`,
    badge: tg.count,
  })),
])

const kindOptions = computed(() => [
  { value: 'all', label: t('catalog.kindAll') },
  { value: 'standalone', label: t('catalog.kindStandalone') },
  { value: 'episodes', label: t('catalog.kindEpisodes') },
])

const tabOptions = computed(() => [
  { value: 'videos' as const, label: t('nav.moviesAndClips') },
  { value: 'shows' as const, label: t('nav.shows') },
])

const sortOptions = computed(() => [
  { value: 'created_at:desc', label: t('catalog.sortCreatedDesc') },
  { value: 'title:asc', label: t('catalog.sortTitleAsc') },
  { value: 'year:desc', label: t('catalog.sortYearDesc') },
  { value: 'duration:desc', label: t('catalog.sortDurationDesc') },
])

function queryValuesEqual(a: unknown, b: unknown): boolean {
  if (Array.isArray(a) || Array.isArray(b)) {
    const la = Array.isArray(a) ? a : a ? [a] : []
    const lb = Array.isArray(b) ? b : b ? [b] : []
    return la.length === lb.length && la.every((v, i) => v === lb[i])
  }
  return a === b
}

function buildLocalRouteQuery(nextKw?: string): Record<string, string> {
  const nextQuery: Record<string, string> = {}
  if (activeTab.value !== 'videos') nextQuery.tab = activeTab.value
  if (selectedLibrary.value) nextQuery.library = selectedLibrary.value
  if (selectedGenre.value) nextQuery.genre = selectedGenre.value
  if (selectedTag.value) nextQuery.tag = selectedTag.value
  if (activeTab.value === 'videos' && kindFilter.value !== 'all') nextQuery.kind = kindFilter.value
  const kw = (nextKw ?? search.value).trim()
  if (kw) nextQuery.q = kw
  return nextQuery
}

function commitRouteQuery(nextQuery: Record<string, string | string[]>): void {
  const cur = syncingRouteQuery && pendingRouteQuery ? pendingRouteQuery : route.query
  const same =
    Object.keys(nextQuery).length === Object.keys(cur).length &&
    Object.entries(nextQuery).every(([k, v]) => queryValuesEqual(cur[k], v))
  if (same) return

  const seq = ++syncRouteSeq
  syncingRouteQuery = true
  pendingRouteQuery = nextQuery
  void router.replace({ path: '/video/catalog', query: nextQuery }).finally(() => {
    if (seq === syncRouteSeq) {
      syncingRouteQuery = false
      pendingRouteQuery = null
    }
  })
}

function syncOnlineScopeToRoute(
  nextScope: VideoSearchScope,
  nextKw: string,
  nextSourceId?: string | null,
): void {
  if (nextScope === 'online') {
    const nextQuery: Record<string, string> = { scope: 'online' }
    if (nextSourceId) {
      nextQuery.source = nextSourceId
    }
    if (nextKw.trim()) {
      nextQuery.q = nextKw.trim()
    }
    commitRouteQuery(nextQuery)
    return
  }
  commitRouteQuery(buildLocalRouteQuery(nextKw))
}

function syncRouteQuery(): void {
  if (searchScope.value === 'online') {
    syncOnlineScopeToRoute('online', onlineKeyword.value, onlineSourceId.value)
    return
  }
  commitRouteQuery(buildLocalRouteQuery())
}

function switchSearchScope(next: VideoSearchScope): void {
  searchScope.value = next
  if (next === 'online') {
    if (searchDebounceTimer) {
      clearTimeout(searchDebounceTimer)
      searchDebounceTimer = null
    }
    fetchSeq += 1
    loading.value = false
    onlineKeyword.value = typeof route.query.q === 'string' ? route.query.q : ''
    syncOnlineScopeToRoute('online', onlineKeyword.value, onlineSourceId.value)
    return
  }
  syncOnlineScopeToRoute('local', search.value, null)
  void fetchActive()
}

function handleOnlineKeywordUpdate(nextKw: string): void {
  onlineKeyword.value = nextKw
  syncOnlineScopeToRoute('online', nextKw, onlineSourceId.value)
}

function handleOnlineSourceUpdate(nextSourceId: string | null): void {
  syncOnlineScopeToRoute('online', onlineKeyword.value, nextSourceId)
}

function applyScopeFromRoute(): void {
  searchScope.value = route.query.scope === 'online' ? 'online' : 'local'
  onlineKeyword.value = typeof route.query.q === 'string' ? route.query.q : ''
}

function selectTab(tab: CatalogTab): void {
  if (activeTab.value === tab) return
  activeTab.value = tab
  syncRouteQuery()
  void fetchActive()
}

function clearScopeFilters(): void {
  selectedLibrary.value = ''
  selectedGenre.value = ''
  selectedTag.value = ''
  kindFilter.value = 'all'
  search.value = ''
  syncRouteQuery()
  void fetchActive()
}

async function fetchVideos(): Promise<void> {
  if (searchScope.value === 'online') {
    loading.value = false
    return
  }
  const seq = ++fetchSeq
  loading.value = true
  try {
    const [sortBy, order] = sortKey.value.split(':') as [VideoSortBy, string]
    const list = await api.listVideos({
      library_id: selectedLibrary.value || undefined,
      kind: kindFilter.value,
      genre: selectedGenre.value || undefined,
      tag: selectedTag.value || undefined,
      search: search.value.trim() || undefined,
      sort_by: sortBy,
      sort_desc: order === 'desc',
    })
    if (seq !== fetchSeq) return
    videos.value = list
  } catch (err) {
    if (seq !== fetchSeq) return
    snackbarStore.show({
      message: err instanceof Error ? err.message : String(err),
      tone: 'error',
    })
  } finally {
    if (seq === fetchSeq) {
      loading.value = false
    }
  }
}

async function fetchShows(): Promise<void> {
  if (searchScope.value === 'online') {
    loading.value = false
    return
  }
  const seq = ++fetchSeq
  loading.value = true
  try {
    const list = await api.listShows({
      library_id: selectedLibrary.value || undefined,
      genre: selectedGenre.value || undefined,
      tag: selectedTag.value || undefined,
      search: search.value.trim() || undefined,
    })
    if (seq !== fetchSeq) return
    shows.value = list
  } catch (err) {
    if (seq !== fetchSeq) return
    snackbarStore.show({
      message: err instanceof Error ? err.message : String(err),
      tone: 'error',
    })
  } finally {
    if (seq === fetchSeq) {
      loading.value = false
    }
  }
}

async function fetchActive(): Promise<void> {
  if (searchScope.value === 'online') {
    loading.value = false
    return
  }
  if (activeTab.value === 'shows') {
    await fetchShows()
  } else {
    await fetchVideos()
  }
}

function handleSearchInput(): void {
  if (searchScope.value === 'online') return
  if (searchDebounceTimer) clearTimeout(searchDebounceTimer)
  searchDebounceTimer = setTimeout(() => {
    searchDebounceTimer = null
    if (searchScope.value === 'online') return
    syncRouteQuery()
    void fetchActive()
  }, 220)
}

function handleFilterChange(): void {
  if (searchScope.value === 'online') return
  syncRouteQuery()
  void fetchActive()
}

watch(
  () =>
    [
      route.query.scope,
      route.query.tab,
      route.query.library,
      route.query.genre,
      route.query.tag,
      route.query.kind,
      route.query.q,
    ] as const,
  ([qScope, qTab, qLib, qGenre, qTag, qKind, qSearch]) => {
    if (syncingRouteQuery) return
    const prevScope = searchScope.value
    const nextScope: VideoSearchScope = qScope === 'online' ? 'online' : 'local'
    searchScope.value = nextScope

    if (nextScope === 'online') {
      if (searchDebounceTimer) {
        clearTimeout(searchDebounceTimer)
        searchDebounceTimer = null
      }
      fetchSeq += 1
      loading.value = false
      onlineKeyword.value = typeof qSearch === 'string' ? qSearch : ''
      return
    }

    const nextTab: CatalogTab = qTab === 'shows' ? 'shows' : 'videos'
    const nextLib = typeof qLib === 'string' ? qLib : ''
    const nextGenre = typeof qGenre === 'string' ? qGenre : ''
    const nextTag = typeof qTag === 'string' ? qTag : ''
    const nextKind: VideoKindFilter =
      qKind === 'standalone' || qKind === 'episodes' ? qKind : 'all'
    const nextSearch = typeof qSearch === 'string' ? qSearch : ''
    const changed =
      prevScope !== 'local' ||
      nextTab !== activeTab.value ||
      nextLib !== selectedLibrary.value ||
      nextGenre !== selectedGenre.value ||
      nextTag !== selectedTag.value ||
      nextKind !== kindFilter.value ||
      nextSearch !== search.value
    activeTab.value = nextTab
    selectedLibrary.value = nextLib
    selectedGenre.value = nextGenre
    selectedTag.value = nextTag
    kindFilter.value = nextKind
    search.value = nextSearch
    if (changed) {
      void fetchActive()
    }
  },
)

async function refreshPage(): Promise<void> {
  if (searchScope.value === 'online') return
  await Promise.all([videoStore.refreshSidebar(), fetchActive()])
}

onMounted(() => {
  applyScopeFromRoute()
  if (searchScope.value !== 'online') {
    void fetchActive()
  } else {
    loading.value = false
  }
})
onBeforeUnmount(() => {
  if (searchDebounceTimer) {
    clearTimeout(searchDebounceTimer)
    searchDebounceTimer = null
  }
})
watch(
  () => [videoStore.catalogVersion, uploadStore.versionCounter],
  () => {
    if (searchScope.value !== 'online') {
      void fetchActive()
    }
  },
)
</script>

<template>
  <div class="mx-auto max-w-7xl space-y-5 px-6 py-7 sm:px-8">
    <!-- Header -->
    <div class="flex flex-wrap items-center justify-between gap-4">
      <div>
        <div class="flex items-center gap-2">
          <h1 class="text-2xl font-extrabold tracking-tight text-[var(--md-sys-color-on-surface)]">
            {{ pageTitle }}
          </h1>
          <button
            v-if="
              searchScope === 'local' &&
              (selectedLibrary || selectedGenre || selectedTag || kindFilter !== 'all')
            "
            type="button"
            class="md-chip md-state"
            @click="clearScopeFilters"
          >
            <span>{{ t('nav.all') }}</span>
            <LuX class="h-3 w-3" />
          </button>
        </div>
        <p
          v-if="searchScope === 'local'"
          class="mt-0.5 text-xs text-[var(--md-sys-color-on-surface-variant)]"
        >
          {{ pageSubtitle }}
        </p>
      </div>

      <div class="flex flex-wrap items-center gap-3">
        <MdSegmentedButton
          :model-value="searchScope"
          :options="searchScopeOptions"
          size="sm"
          :show-check="false"
          @update:model-value="switchSearchScope($event)"
        />
        <RefreshButton v-if="searchScope === 'local'" size="md" :action="refreshPage" />
      </div>
    </div>

    <OnlineVideoSearchSection
      v-if="searchScope === 'online'"
      :keyword="onlineKeyword"
      :initial-source-id="onlineSourceId"
      @update:keyword="handleOnlineKeywordUpdate"
      @update:source-id="handleOnlineSourceUpdate"
    />

    <template v-else>
      <!-- Videos / Shows Tabs -->
      <MdSegmentedButton
        :model-value="activeTab"
        :options="tabOptions"
        size="md"
        @update:model-value="selectTab($event)"
      />

      <!-- Filters Row -->
      <div
        class="grid grid-cols-1 gap-2.5 sm:grid-cols-2"
        :class="activeTab === 'videos' ? 'lg:grid-cols-5' : 'lg:grid-cols-3'"
      >
        <div class="relative">
          <LuSearch
            class="pointer-events-none absolute top-1/2 left-3 h-3.5 w-3.5 -translate-y-1/2 text-[var(--md-sys-color-on-surface-variant)]"
          />
          <input
            v-model="search"
            type="text"
            :placeholder="t('common.filterPlaceholder')"
            class="md-input h-9 w-full pl-8.5 pr-3 text-xs"
            @input="handleSearchInput"
          />
        </div>

        <AppSelect
          v-model="selectedLibrary"
          :options="libraryOptions"
          @change="handleFilterChange"
        />

        <AppSelect
          v-model="selectedGenre"
          :options="genreOptions"
          searchable
          @change="handleFilterChange"
        />

        <AppSelect
          v-if="videoStore.tags.length > 0 || selectedTag"
          v-model="selectedTag"
          :options="tagOptions"
          searchable
          @change="handleFilterChange"
        />

        <template v-if="activeTab === 'videos'">
          <AppSelect
            v-model="kindFilter"
            :options="kindOptions"
            @change="handleFilterChange"
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
          class="py-16 text-center text-xs text-[var(--md-sys-color-on-surface-variant)]"
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
          class="py-16 text-center text-xs text-[var(--md-sys-color-on-surface-variant)]"
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
    </template>
  </div>
</template>

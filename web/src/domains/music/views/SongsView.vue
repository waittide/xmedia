<script setup lang="ts">
import { computed, onMounted, ref, watch } from 'vue'
import { useRoute, useRouter } from 'vue-router'
import { useI18n as useGlobalI18n } from 'vue-i18n'
import { LuPlay, LuShuffle, LuSearch, LuHeart, LuX } from 'vue-icons-plus/lu'
import AppSelect from '@/components/ui/AppSelect.vue'
import MdSegmentedButton from '@/components/ui/MdSegmentedButton.vue'
import RefreshButton from '@/components/ui/RefreshButton.vue'
import { useUploadStore } from '@/stores/uploads'
import { api } from '../api'
import AlbumCard from '../components/AlbumCard.vue'
import CoverImage from '../components/CoverImage.vue'
import TrackTable from '../components/TrackTable.vue'
import { useMusicI18n as useI18n } from '../i18n'
import { usePlayerStore } from '../player'
import { useMusicStore } from '../store'
import type {
  AlbumDto,
  AlbumListType,
  ArtistDto,
  ArtistRoleFilter,
  SortOrder,
  TrackDto,
  TrackSortField,
} from '../types'

type CatalogTab = 'songs' | 'albums' | 'artists'

const { t } = useI18n()
const { t: globalT } = useGlobalI18n()
const route = useRoute()
const router = useRouter()
const musicStore = useMusicStore()
const playerStore = usePlayerStore()
const uploadStore = useUploadStore()

function resolveTabFromQuery(): CatalogTab {
  const q = route.query.tab
  if (q === 'albums' || q === 'artists') return q
  return 'songs'
}

const activeTab = ref<CatalogTab>(resolveTabFromQuery())
const selectedLibrary = ref(typeof route.query.library === 'string' ? route.query.library : '')
const selectedGenre = ref(typeof route.query.genre === 'string' ? route.query.genre : '')
const selectedTag = ref(typeof route.query.tag === 'string' ? route.query.tag : '')
const search = ref('')

// Songs state
const tracks = ref<TrackDto[]>([])
const tracksTotal = ref(0)
const tracksPage = ref(1)
const tracksPageSize = 50
const tracksSortKey = ref('created_at:desc')

// Albums state
const albums = ref<AlbumDto[]>([])
const albumsTotal = ref(0)
const albumsPage = ref(1)
const albumsPageSize = 36
const albumListType = ref<AlbumListType>('newest')

// Artists state
const artists = ref<ArtistDto[]>([])
const artistsTotal = ref(0)
const artistRoleFilter = ref<ArtistRoleFilter>('all')

const loading = ref(false)

const activeLibraryName = computed(() => {
  if (!selectedLibrary.value) return ''
  return musicStore.libraries.find((l) => l.id === selectedLibrary.value)?.name ?? ''
})

const pageTitle = computed(() => {
  if (activeLibraryName.value) return activeLibraryName.value
  if (selectedTag.value) return `#${selectedTag.value}`
  if (selectedGenre.value) return selectedGenre.value
  return t('nav.all')
})

const pageSubtitle = computed(() => {
  if (activeTab.value === 'albums') {
    return t('common.albumsCount', { count: albumsTotal.value })
  }
  if (activeTab.value === 'artists') {
    return t('common.artistsCount', { count: artistsTotal.value })
  }
  return t('common.tracksCount', { count: tracksTotal.value })
})

const libraryOptions = computed(() => [
  { value: '', label: t('catalog.allLibraries') },
  ...musicStore.libraries.map((l) => ({
    value: l.id,
    label: l.name,
    badge: l.track_count,
  })),
])

const genreOptions = computed(() => [
  { value: '', label: t('catalog.allGenres') },
  ...musicStore.genres.map((g) => ({
    value: g.name,
    label: g.name,
    badge: activeTab.value === 'albums' ? g.album_count : g.song_count,
  })),
])

const tagOptions = computed(() => [
  { value: '', label: globalT('musicTags.allTagsFilter') },
  ...musicStore.tags.map((tg) => ({
    value: tg.name,
    label: `#${tg.name}`,
    badge: tg.song_count,
  })),
])

const trackSortOptions = computed(() => [
  { value: 'created_at:desc', label: t('catalog.sortCreatedDesc') },
  { value: 'title:asc', label: t('catalog.sortTitleAsc') },
  { value: 'artist:asc', label: t('catalog.sortArtistAsc') },
  { value: 'album:asc', label: t('catalog.sortAlbumAsc') },
  { value: 'year:desc', label: t('catalog.sortYearDesc') },
  { value: 'duration:desc', label: t('catalog.sortDurationDesc') },
])

const albumTypeOptions = computed(() => [
  { value: 'newest', label: t('catalog.albumSortNewest') },
  { value: 'recent', label: t('catalog.albumSortRecent') },
  { value: 'frequent', label: t('catalog.albumSortFrequent') },
  { value: 'random', label: t('catalog.albumSortRandom') },
  { value: 'alphabeticalByName', label: t('catalog.albumSortAlphaName') },
  { value: 'alphabeticalByArtist', label: t('catalog.albumSortAlphaArtist') },
  { value: 'starred', label: t('catalog.albumSortStarred') },
])

const artistRoleOptions = computed(() => [
  { value: 'all', label: t('catalog.artistRoleAll') },
  { value: 'album_artist', label: t('catalog.artistRoleAlbum') },
  { value: 'track_artist', label: t('catalog.artistRoleTrack') },
])

const catalogTabOptions = computed(
  () =>
    [
      { value: 'songs', label: t('nav.songs') },
      { value: 'albums', label: t('nav.albums') },
      { value: 'artists', label: t('nav.artists') },
    ] as const,
)

const tracksTotalPages = computed(() => Math.max(1, Math.ceil(tracksTotal.value / tracksPageSize)))
const albumsTotalPages = computed(() => Math.max(1, Math.ceil(albumsTotal.value / albumsPageSize)))

function syncRouteQuery(): void {
  const nextQuery: Record<string, string> = {}
  if (activeTab.value !== 'songs') nextQuery.tab = activeTab.value
  if (selectedLibrary.value) nextQuery.library = selectedLibrary.value
  if (selectedGenre.value) nextQuery.genre = selectedGenre.value
  if (selectedTag.value) nextQuery.tag = selectedTag.value
  void router.replace({ path: '/music/songs', query: nextQuery })
}

function selectTab(tab: CatalogTab): void {
  if (activeTab.value === tab) return
  activeTab.value = tab
  syncRouteQuery()
  void fetchActiveTab()
}

function clearScopeFilters(): void {
  selectedLibrary.value = ''
  selectedGenre.value = ''
  selectedTag.value = ''
  tracksPage.value = 1
  albumsPage.value = 1
  syncRouteQuery()
  void fetchActiveTab()
}

async function fetchSongs(): Promise<void> {
  loading.value = true
  try {
    const [field, order] = tracksSortKey.value.split(':') as [TrackSortField, SortOrder]
    const res = await api.queryTracks({
      library_id: selectedLibrary.value || undefined,
      genre: selectedGenre.value || undefined,
      tag: selectedTag.value || undefined,
      search: search.value.trim() || undefined,
      sort_by: field,
      sort_order: order,
      page: tracksPage.value,
      page_size: tracksPageSize,
    })
    tracks.value = res.items
    tracksTotal.value = res.total
  } finally {
    loading.value = false
  }
}

async function fetchAlbums(): Promise<void> {
  loading.value = true
  try {
    const res = await api.listAlbums({
      type: albumListType.value,
      library_id: selectedLibrary.value || undefined,
      genre: selectedGenre.value || undefined,
      search: search.value.trim() || undefined,
      offset: (albumsPage.value - 1) * albumsPageSize,
      size: albumsPageSize,
    })
    albums.value = res.items
    albumsTotal.value = res.total
  } finally {
    loading.value = false
  }
}

async function fetchArtists(): Promise<void> {
  loading.value = true
  try {
    const res = await api.listArtists({
      search: search.value.trim() || undefined,
      role: artistRoleFilter.value,
      size: 200,
    })
    artists.value = res.items
    artistsTotal.value = res.total
  } finally {
    loading.value = false
  }
}

async function fetchActiveTab(): Promise<void> {
  if (activeTab.value === 'albums') {
    await fetchAlbums()
  } else if (activeTab.value === 'artists') {
    await fetchArtists()
  } else {
    await fetchSongs()
  }
}

function handleFilterChange(): void {
  tracksPage.value = 1
  albumsPage.value = 1
  syncRouteQuery()
  void fetchActiveTab()
}

watch(
  () => [route.query.tab, route.query.library, route.query.genre, route.query.tag] as const,
  ([qTab, qLib, qGenre, qTag]) => {
    const nextTab: CatalogTab = qTab === 'albums' || qTab === 'artists' ? qTab : 'songs'
    const nextLib = typeof qLib === 'string' ? qLib : ''
    const nextGenre = typeof qGenre === 'string' ? qGenre : ''
    const nextTag = typeof qTag === 'string' ? qTag : ''
    const changed =
      nextTab !== activeTab.value ||
      nextLib !== selectedLibrary.value ||
      nextGenre !== selectedGenre.value ||
      nextTag !== selectedTag.value
    activeTab.value = nextTab
    selectedLibrary.value = nextLib
    selectedGenre.value = nextGenre
    selectedTag.value = nextTag
    if (changed) {
      tracksPage.value = 1
      albumsPage.value = 1
      void fetchActiveTab()
    }
  },
)

async function refreshPage(): Promise<void> {
  await Promise.all([musicStore.refreshSidebar(), fetchActiveTab()])
}

onMounted(fetchActiveTab)
watch(() => [musicStore.catalogVersion, uploadStore.versionCounter], fetchActiveTab)
</script>

<template>
  <div class="mx-auto max-w-7xl space-y-5 px-6 py-7 sm:px-8">
    <!-- Top Header & Quick Play -->
    <div class="flex flex-wrap items-center justify-between gap-4">
      <div class="flex items-center gap-3">
        <div>
          <div class="flex items-center gap-2">
            <h1 class="text-2xl font-extrabold tracking-tight text-[var(--md-sys-color-on-surface)]">
              {{ pageTitle }}
            </h1>
            <button
              v-if="selectedLibrary || selectedGenre || selectedTag"
              type="button"
              class="md-chip md-chip-selected md-state"
              @click="clearScopeFilters"
            >
              <span>{{ t('nav.all') }}</span>
              <LuX class="h-3 w-3" />
            </button>
          </div>
          <p class="mt-0.5 text-xs text-[var(--md-sys-color-on-surface-variant)]">
            {{ pageSubtitle }}
          </p>
        </div>
      </div>

      <div class="flex items-center gap-2">
        <template v-if="activeTab === 'songs' && tracks.length > 0">
          <button
            type="button"
            class="md-btn md-btn-filled md-btn-sm md-state"
            @click="playerStore.playQueue(tracks, 0)"
          >
            <LuPlay class="h-3.5 w-3.5 fill-current" />
            <span>{{ t('common.playAll') }}</span>
          </button>
          <button
            type="button"
            class="md-btn md-btn-tonal md-btn-sm md-state"
            @click="playerStore.shuffleAndPlay(tracks)"
          >
            <LuShuffle class="h-3.5 w-3.5" />
            <span>{{ t('common.shufflePlay') }}</span>
          </button>
        </template>
        <RefreshButton size="md" :action="refreshPage" />
      </div>
    </div>

    <!-- Songs / Albums / Artists Tabs -->
    <div class="flex flex-wrap items-center justify-between gap-3">
      <MdSegmentedButton
        :model-value="activeTab"
        :options="catalogTabOptions"
        size="md"
        @update:model-value="selectTab($event)"
      />
    </div>

    <!-- 1. SONGS TAB -->
    <template v-if="activeTab === 'songs'">
      <div
        class="grid grid-cols-1 gap-2.5 sm:grid-cols-2"
        :class="musicStore.tags.length > 0 || selectedTag ? 'lg:grid-cols-5' : 'lg:grid-cols-4'"
      >
        <div class="relative">
          <LuSearch
            class="pointer-events-none absolute left-3 top-1/2 h-3.5 w-3.5 -translate-y-1/2 text-[var(--md-sys-color-on-surface-variant)]"
          />
          <input
            v-model="search"
            type="text"
            :placeholder="t('common.filterPlaceholder')"
            class="md-input h-9 w-full pl-8.5 pr-3 text-xs placeholder-[var(--md-sys-color-on-surface-variant)]"
            @input="tracksPage = 1; fetchSongs()"
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
          v-if="musicStore.tags.length > 0 || selectedTag"
          v-model="selectedTag"
          :options="tagOptions"
          searchable
          @change="handleFilterChange"
        />

        <AppSelect
          v-model="tracksSortKey"
          :options="trackSortOptions"
          @change="tracksPage = 1; fetchSongs()"
        />
      </div>

      <div
        v-if="tracks.length === 0 && !loading"
        class="py-16 text-center text-xs text-[var(--md-sys-color-on-surface-variant)]"
      >
        {{ t('common.noResults') }}
      </div>
      <TrackTable v-else :tracks="tracks" />

      <div
        v-if="tracksTotalPages > 1"
        class="flex items-center justify-between pt-2 text-xs text-[var(--md-sys-color-on-surface-variant)]"
      >
        <span>
          {{ t('common.pageInfo', { page: tracksPage, totalPages: tracksTotalPages, total: tracksTotal }) }}
        </span>
        <div class="flex items-center gap-2">
          <button
            type="button"
            :disabled="tracksPage <= 1"
            class="md-btn md-btn-outlined md-btn-sm md-state"
            @click="tracksPage -= 1; fetchSongs()"
          >
            {{ t('common.prevPage') }}
          </button>
          <button
            type="button"
            :disabled="tracksPage >= tracksTotalPages"
            class="md-btn md-btn-outlined md-btn-sm md-state"
            @click="tracksPage += 1; fetchSongs()"
          >
            {{ t('common.nextPage') }}
          </button>
        </div>
      </div>
    </template>

    <!-- 2. ALBUMS TAB -->
    <template v-else-if="activeTab === 'albums'">
      <div class="grid grid-cols-1 gap-2.5 sm:grid-cols-2 lg:grid-cols-4">
        <div class="relative">
          <LuSearch
            class="pointer-events-none absolute left-3 top-1/2 h-3.5 w-3.5 -translate-y-1/2 text-[var(--md-sys-color-on-surface-variant)]"
          />
          <input
            v-model="search"
            type="text"
            :placeholder="t('common.filterPlaceholder')"
            class="md-input h-9 w-full pl-8.5 pr-3 text-xs placeholder-[var(--md-sys-color-on-surface-variant)]"
            @input="albumsPage = 1; fetchAlbums()"
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
          v-model="albumListType"
          :options="albumTypeOptions"
          @change="albumsPage = 1; fetchAlbums()"
        />
      </div>

      <div
        v-if="albums.length === 0 && !loading"
        class="py-16 text-center text-xs text-[var(--md-sys-color-on-surface-variant)]"
      >
        {{ t('common.noResults') }}
      </div>
      <div v-else class="grid grid-cols-2 gap-3 sm:grid-cols-3 md:grid-cols-4 lg:grid-cols-6">
        <AlbumCard v-for="al in albums" :key="al.id" :album="al" />
      </div>

      <div
        v-if="albumsTotalPages > 1"
        class="flex items-center justify-between pt-2 text-xs text-[var(--md-sys-color-on-surface-variant)]"
      >
        <span>
          {{ t('common.pageInfo', { page: albumsPage, totalPages: albumsTotalPages, total: albumsTotal }) }}
        </span>
        <div class="flex items-center gap-2">
          <button
            type="button"
            :disabled="albumsPage <= 1"
            class="md-btn md-btn-outlined md-btn-sm md-state"
            @click="albumsPage -= 1; fetchAlbums()"
          >
            {{ t('common.prevPage') }}
          </button>
          <button
            type="button"
            :disabled="albumsPage >= albumsTotalPages"
            class="md-btn md-btn-outlined md-btn-sm md-state"
            @click="albumsPage += 1; fetchAlbums()"
          >
            {{ t('common.nextPage') }}
          </button>
        </div>
      </div>
    </template>

    <!-- 3. ARTISTS TAB -->
    <template v-else>
      <div class="grid grid-cols-1 gap-2.5 sm:grid-cols-2 md:max-w-xl">
        <div class="relative">
          <LuSearch
            class="pointer-events-none absolute left-3 top-1/2 h-3.5 w-3.5 -translate-y-1/2 text-[var(--md-sys-color-on-surface-variant)]"
          />
          <input
            v-model="search"
            type="text"
            :placeholder="t('common.filterPlaceholder')"
            class="md-input h-9 w-full pl-8.5 pr-3 text-xs placeholder-[var(--md-sys-color-on-surface-variant)]"
            @input="fetchArtists"
          />
        </div>

        <AppSelect
          v-model="artistRoleFilter"
          :options="artistRoleOptions"
          @change="fetchArtists"
        />
      </div>

      <div
        v-if="artists.length === 0 && !loading"
        class="py-16 text-center text-xs text-[var(--md-sys-color-on-surface-variant)]"
      >
        {{ t('common.noResults') }}
      </div>

      <div v-else class="grid grid-cols-2 gap-3.5 sm:grid-cols-3 md:grid-cols-4 lg:grid-cols-6">
        <div
          v-for="ar in artists"
          :key="ar.id"
          class="group relative flex cursor-pointer flex-col items-center rounded-2xl p-3 text-center transition hover:bg-[var(--md-sys-color-surface-container)]"
          @click="router.push(`/music/artists/${ar.id}`)"
        >
          <div class="relative mb-3 w-3/4">
            <CoverImage
              :cover-id="ar.id"
              :has-cover="Boolean(ar.cover_key)"
              :size="280"
              rounded="rounded-full"
              class="album-shadow"
            />
            <button
              type="button"
              class="md-icon-btn md-icon-btn-sm md-state absolute right-0 bottom-0 h-7 w-7 border border-[var(--md-sys-color-outline-variant)] bg-[var(--md-sys-color-surface)] shadow-[var(--md-sys-elevation-1)] transition hover:scale-110"
              @click.stop="musicStore.toggleArtistStar(ar)"
            >
              <LuHeart
                class="h-3.5 w-3.5"
                :class="
                  ar.starred_at
                    ? 'fill-[var(--md-sys-color-error)] text-[var(--md-sys-color-error)]'
                    : 'text-[var(--md-sys-color-on-surface-variant)]'
                "
              />
            </button>
          </div>

          <h3 class="w-full truncate text-xs font-bold text-[var(--md-sys-color-on-surface)]">
            {{ ar.name }}
          </h3>
          <p class="mt-0.5 text-[11px] text-[var(--md-sys-color-on-surface-variant)]">
            {{ t('common.albumsCount', { count: ar.album_count }) }} ·
            {{ t('common.tracksCount', { count: ar.track_count }) }}
          </p>
          <div class="mt-1.5 flex flex-wrap justify-center gap-1">
            <span
              v-if="ar.is_album_artist"
              class="rounded-md bg-[var(--md-sys-color-primary-container)] px-1.5 py-0.5 text-[9px] font-semibold text-[var(--md-sys-color-on-primary-container)]"
            >
              {{ t('catalog.badgeAlbumArtist') }}
            </span>
            <span
              v-if="ar.is_track_artist && !ar.is_album_artist"
              class="rounded-md bg-[var(--md-sys-color-surface-container-high)] px-1.5 py-0.5 text-[9px] font-medium text-[var(--md-sys-color-on-surface-variant)]"
            >
              {{ t('catalog.badgeTrackArtist') }}
            </span>
          </div>
        </div>
      </div>
    </template>
  </div>
</template>

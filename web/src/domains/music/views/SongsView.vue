<script setup lang="ts">
import { ref } from 'vue'
import { useI18n as useGlobalI18n } from 'vue-i18n'
import { LuPlay, LuRadio, LuRefreshCw, LuShuffle, LuTag } from 'vue-icons-plus/lu'
import { DomainLibraryTemplate, DomainPoolSection } from '@/components/domain'
import { MdButton, MdChip, MdIconButton } from '@/components/ui'
import type { LibraryTab } from '@/sources'
import AlbumCard from '../components/AlbumCard.vue'
import ArtistCard from '../components/ArtistCard.vue'
import LibraryArtistPanel from '../components/LibraryArtistPanel.vue'
import LibraryCollectionsPanel from '../components/LibraryCollectionsPanel.vue'
import LibraryContinuePanel from '../components/LibraryContinuePanel.vue'
import LibraryFavoritesPanel from '../components/LibraryFavoritesPanel.vue'
import LibraryPlaylistPanel from '../components/LibraryPlaylistPanel.vue'
import MusicCatalogFilterSheet from '../components/MusicCatalogFilterSheet.vue'
import OnlineMusicSearchSection from '../components/OnlineMusicSearchSection.vue'
import RemoteSubsonicSection from '../components/RemoteSubsonicSection.vue'
import TrackTable from '../components/TrackTable.vue'
import {
  useMusicLibraryCatalog,
  type MusicViewMode,
} from '../composables/useMusicLibraryCatalog'
import { useMusicI18n as useI18n } from '../i18n'
import { usePlayerStore } from '../player'
import type { ArtistRoleFilter } from '../types'

const { t } = useI18n()
const { t: globalT } = useGlobalI18n()
const playerStore = usePlayerStore()

const onlineSearchRef = ref<InstanceType<typeof OnlineMusicSearchSection> | null>(null)
const remoteSearchRef = ref<InstanceType<typeof RemoteSubsonicSection> | null>(null)
const continuePanelRef = ref<InstanceType<typeof LibraryContinuePanel> | null>(null)
const favoritesPanelRef = ref<InstanceType<typeof LibraryFavoritesPanel> | null>(null)
const manualRefreshing = ref(false)

const {
  musicStore,
  sourceFilter,
  activeScope,
  activeSourceId,
  activeArtistId,
  activeCollectionId,
  isSubDetailView,
  activeTab,
  activeView,
  selectedLibrary,
  selectedGenre,
  selectedTag,
  committedQuery,
  tracksSortKey,
  albumListType,
  artistRoleFilter,
  searchInput,
  filterSheetOpen,
  loading,
  fetchError,
  tracks,
  albums,
  artists,
  isLocalCatalogMode,
  tabs,
  viewChips,
  activeLibraryName,
  filteredGenres,
  filteredTags,
  pageTitle,
  headerCount,
  activeFilterCount,
  libraryOptions,
  genreOptions,
  tagOptions,
  trackSortOptions,
  albumTypeOptions,
  artistRoleOptions,
  activePagination,
  isLocalCatalogEmpty,
  searchPlaceholder,
  updateQuery,
  clearDebounce,
  handleSearchInputUpdate,
  fetchLocalCatalog,
} = useMusicLibraryCatalog()

function handleTabChange(nextTab: string): void {
  const tab = nextTab as LibraryTab
  updateQuery(
    {
      tab: tab === 'all' ? null : tab,
      artist: null,
      collection: null,
    },
    true,
  )
}

function handleViewChange(nextView: MusicViewMode): void {
  const patch: Record<string, string | null> = {
    view: nextView === 'items' ? null : nextView,
    sort: null,
    order: null,
  }
  if (nextView === 'artists' || nextView === 'genres' || nextView === 'tags') {
    patch.library = null
    patch.genre = null
    patch.tag = null
  } else if (nextView === 'items') {
    patch.tag = null
  }
  updateQuery(patch, true)
}

function handleTrackSortChange(val: string): void {
  const [sort, order] = val.split(':')
  updateQuery(
    {
      sort: sort === 'created_at' && order === 'desc' ? null : sort || null,
      order: sort === 'created_at' && order === 'desc' ? null : order || null,
    },
    true,
  )
}

function handleAlbumSortChange(val: string): void {
  updateQuery({ sort: val === 'newest' ? null : val, order: null }, true)
}

function handleArtistRoleChange(val: string): void {
  artistRoleFilter.value = val as ArtistRoleFilter
  updateQuery({}, true)
  void fetchLocalCatalog()
}

function resetAllFilters(): void {
  updateQuery({ library: null, genre: null, tag: null, sort: null, order: null }, true)
}

function handleSearchSubmit(val: string): void {
  clearDebounce()
  const trimmed = val.trim()
  if (activeScope.value === 'online' && activeTab.value === 'all') {
    updateQuery({ q: trimmed || null }, true)
    void onlineSearchRef.value?.submitSearch(trimmed)
  } else if (trimmed !== committedQuery.value) {
    updateQuery({ q: trimmed || null }, true)
  } else if (activeScope.value === 'remote' && activeTab.value === 'all') {
    remoteSearchRef.value?.reload()
  } else if (isLocalCatalogMode.value) {
    void fetchLocalCatalog()
  }
}

function handleSearchClear(): void {
  clearDebounce()
  searchInput.value = ''
  updateQuery({ q: null }, true)
  if (activeScope.value === 'online' && activeTab.value === 'all') {
    onlineSearchRef.value?.clearSearch()
  }
}

function handleOnlineKeywordUpdate(kw: string): void {
  searchInput.value = kw
  if (kw.trim() !== committedQuery.value) {
    updateQuery({ q: kw.trim() || null })
  }
}

async function handleRefresh(): Promise<void> {
  if (manualRefreshing.value) return
  manualRefreshing.value = true
  try {
    if (activeTab.value === 'continue') {
      await continuePanelRef.value?.refresh()
      return
    }
    if (activeTab.value === 'favorites') {
      await favoritesPanelRef.value?.refresh()
      return
    }
    if (activeScope.value === 'online') {
      await onlineSearchRef.value?.retryInit()
      return
    }
    if (activeScope.value === 'remote') {
      remoteSearchRef.value?.reload()
      return
    }
    await Promise.allSettled([musicStore.refreshSidebar(), fetchLocalCatalog()])
  } finally {
    manualRefreshing.value = false
  }
}

function handleTagsUpdated(payload: { trackId: string; tags: string[] }): void {
  tracks.value = tracks.value.map((tr) =>
    tr.id === payload.trackId ? { ...tr, tags: payload.tags } : tr,
  )
}

function handlePoolChanged(): void {
  musicStore.bumpCatalogVersion()
  void musicStore.refreshSidebar()
}
</script>

<template>
  <div class="px-4 pt-1.5 pb-5 sm:px-6 sm:py-6 lg:px-8">
    <DomainLibraryTemplate
      domain="music"
      :title="pageTitle"
      :count="headerCount"
      :show-scope-bar="true"
      :show-search="!isSubDetailView"
      :search-query="searchInput"
      :search-placeholder="searchPlaceholder"
      :tabs="tabs"
      :active-tab="activeTab"
      :loading="isLocalCatalogMode ? loading : false"
      :skeleton-mode="activeView === 'tracks' ? 'list' : 'grid'"
      skeleton-aspect="square"
      :error="isLocalCatalogMode ? fetchError : null"
      :empty="isLocalCatalogEmpty"
      :page="activePagination.page"
      :page-size="activePagination.pageSize"
      :total="activePagination.total"
      :total-pages="activePagination.totalPages"
      :filter-sheet-open="filterSheetOpen"
      :active-filter-count="activeFilterCount"
      @update:search-query="handleSearchInputUpdate"
      @search-submit="handleSearchSubmit"
      @search-clear="handleSearchClear"
      @update:active-tab="handleTabChange"
      @update:page="updateQuery({ page: $event > 1 ? String($event) : null })"
      @update:filter-sheet-open="filterSheetOpen = $event"
      @reset-filters="resetAllFilters"
      @retry="fetchLocalCatalog"
    >
      <template v-if="isLocalCatalogMode" #toolbar-chips>
        <MdChip
          v-for="chip in viewChips"
          :key="chip.value"
          class="shrink-0"
          variant="filter"
          :icon="chip.icon"
          :label="chip.label"
          :selected="activeView === chip.value"
          hide-check
          @click="handleViewChange(chip.value)"
        />
        <MdChip
          v-if="selectedLibrary && activeLibraryName"
          class="shrink-0"
          variant="input"
          :label="activeLibraryName"
          selected
          removable
          hide-check
          @remove="updateQuery({ library: null }, true)"
        />
        <MdChip
          v-if="selectedGenre"
          class="shrink-0"
          variant="input"
          :label="selectedGenre"
          selected
          removable
          hide-check
          @remove="updateQuery({ genre: null }, true)"
        />
        <MdChip
          v-if="selectedTag"
          class="shrink-0"
          variant="input"
          :label="`#${selectedTag}`"
          selected
          removable
          hide-check
          @remove="updateQuery({ tag: null }, true)"
        />
      </template>

      <template v-if="isLocalCatalogMode" #toolbar-trailing>
        <template v-if="activeView === 'tracks' && tracks.length > 0">
          <MdButton
            size="sm"
            variant="tonal"
            :icon="LuPlay"
            @click="playerStore.playQueue(tracks, 0)"
          >
            {{ t('common.playAllBtn') }}
          </MdButton>
          <MdIconButton
            size="sm"
            variant="standard"
            :icon="LuShuffle"
            :aria-label="t('common.shufflePlay')"
            :tooltip="t('common.shufflePlay')"
            @click="playerStore.shuffleAndPlay(tracks)"
          />
        </template>
        <MdIconButton
          size="sm"
          variant="standard"
          :icon="LuRefreshCw"
          :loading="manualRefreshing"
          :aria-label="globalT('common.refresh')"
          :tooltip="globalT('common.refresh')"
          @click="handleRefresh"
        />
      </template>

      <template
        v-if="
          isLocalCatalogMode &&
          (activeView === 'items' || activeView === 'tracks' || activeView === 'artists')
        "
        #filter-sheet
      >
        <MusicCatalogFilterSheet
          :active-view="activeView"
          :selected-library="selectedLibrary"
          :selected-genre="selectedGenre"
          :selected-tag="selectedTag"
          :tracks-sort-key="tracksSortKey"
          :album-list-type="albumListType"
          :artist-role-filter="artistRoleFilter"
          :library-options="libraryOptions"
          :genre-options="genreOptions"
          :tag-options="tagOptions"
          :track-sort-options="trackSortOptions"
          :album-type-options="albumTypeOptions"
          :artist-role-options="artistRoleOptions"
          @update:library="updateQuery({ library: $event || null }, true)"
          @update:genre="updateQuery({ genre: $event || null }, true)"
          @update:tag="updateQuery({ tag: $event || null }, true)"
          @update:track-sort="handleTrackSortChange"
          @update:album-sort="handleAlbumSortChange"
          @update:artist-role="handleArtistRoleChange"
        />
      </template>

      <LibraryArtistPanel
        v-if="activeArtistId"
        :artist-id="activeArtistId"
        @back="updateQuery({ artist: null, view: 'artists' }, true)"
      />

      <LibraryPlaylistPanel
        v-else-if="activeCollectionId"
        :playlist-id="activeCollectionId"
        @back="updateQuery({ collection: null, tab: 'collections' }, true)"
        @deleted="updateQuery({ collection: null, tab: 'collections' }, true)"
      />

      <LibraryContinuePanel
        v-else-if="activeTab === 'continue'"
        ref="continuePanelRef"
        :query="searchInput"
      />

      <LibraryFavoritesPanel
        v-else-if="activeTab === 'favorites'"
        ref="favoritesPanelRef"
        :query="searchInput"
      />

      <LibraryCollectionsPanel
        v-else-if="activeScope === 'local' && activeTab === 'collections'"
        :query="searchInput"
      />

      <DomainPoolSection
        v-else-if="activeScope === 'pool'"
        domain="music"
        :search-query="searchInput"
        @changed="handlePoolChanged"
      />

      <OnlineMusicSearchSection
        v-else-if="activeScope === 'online'"
        ref="onlineSearchRef"
        :keyword="committedQuery"
        :initial-source-id="activeSourceId"
        :show-search-input="false"
        @update:keyword="handleOnlineKeywordUpdate"
        @update:source-id="sourceFilter.setSource($event)"
      />

      <RemoteSubsonicSection
        v-else-if="activeScope === 'remote'"
        ref="remoteSearchRef"
        :keyword="committedQuery"
        :selected-source-id="activeSourceId"
        @update:source-id="sourceFilter.setSource($event)"
      />

      <template v-else>
        <TrackTable
          v-if="activeView === 'tracks'"
          :tracks="tracks"
          @tags-updated="handleTagsUpdated"
        />

        <div
          v-else-if="activeView === 'items'"
          class="grid grid-cols-2 gap-5 sm:grid-cols-3 md:grid-cols-4 lg:grid-cols-5 xl:grid-cols-6"
        >
          <AlbumCard v-for="al in albums" :key="al.id" :album="al" />
        </div>

        <div
          v-else-if="activeView === 'artists'"
          class="grid grid-cols-2 gap-4 sm:grid-cols-3 md:grid-cols-4 lg:grid-cols-5 xl:grid-cols-6"
        >
          <ArtistCard v-for="ar in artists" :key="ar.id" :artist="ar" />
        </div>

        <div v-else-if="activeView === 'genres'" class="flex flex-wrap gap-2">
          <MdChip
            v-for="genre in filteredGenres"
            :key="genre.name"
            variant="assist"
            :icon="LuRadio"
            :label="`${genre.name} (${genre.song_count})`"
            @click="updateQuery({ genre: genre.name, view: 'items' }, true)"
          />
        </div>

        <div v-else-if="activeView === 'tags'" class="flex flex-wrap gap-2">
          <MdChip
            v-for="tg in filteredTags"
            :key="tg.name"
            variant="assist"
            :icon="LuTag"
            :label="`#${tg.name} (${tg.song_count})`"
            @click="updateQuery({ tag: tg.name, view: 'tracks' }, true)"
          />
        </div>
      </template>
    </DomainLibraryTemplate>
  </div>
</template>

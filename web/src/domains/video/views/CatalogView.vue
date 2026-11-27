<script setup lang="ts">
import { ref } from 'vue'
import { useI18n as useGlobalI18n } from 'vue-i18n'
import {
  LuCheck,
  LuFilm,
  LuRefreshCw,
  LuSparkles,
  LuTag,
  LuX,
} from 'vue-icons-plus/lu'
import { DomainLibraryTemplate, DomainPoolSection, MediaItemCard } from '@/components/domain'
import {
  MdButton,
  MdChip,
  MdIconButton,
  MdMenuItem,
  MdSelect,
} from '@/components/ui'
import VideoCatalogFilterSheet from '../components/catalog/VideoCatalogFilterSheet.vue'
import VideoCollectionSubheader from '../components/catalog/VideoCollectionSubheader.vue'
import VideoWatchlistsGrid from '../components/catalog/VideoWatchlistsGrid.vue'
import OnlineVideoSearchSection from '../components/OnlineVideoSearchSection.vue'
import ShowCard from '../components/ShowCard.vue'
import VideoCard from '../components/VideoCard.vue'
import VideoMetadataModal from '../components/VideoMetadataModal.vue'
import { useVideoLibraryCatalog } from '../composables/useVideoLibraryCatalog'
import { useVideoI18n as useI18n } from '../i18n'

const { t } = useI18n()
const { t: globalT } = useGlobalI18n()

const onlineSearchRef = ref<InstanceType<typeof OnlineVideoSearchSection> | null>(null)

const {
  filterSheetOpen,
  editingVideo,
  videos,
  shows,
  shelfDetail,
  manualRefreshing,
  activeScope,
  onlineSourceId,
  activeCollectionId,
  activeTab,
  activeView,
  selectedLibrary,
  selectedGenre,
  selectedTag,
  kindFilter,
  sortKey,
  routeQueryQ,
  searchInput,
  isShelfTab,
  mergedShelfItems,
  primaryTabs,
  viewChipOptions,
  sortOptions,
  activeLibraryName,
  activeFilterCount,
  showLocalFilterControls,
  showVideoSort,
  filteredGenres,
  filteredTags,
  filteredShelves,
  filteredShelfVideos,
  pageTitle,
  isPoolBrowseMode,
  isOnlineSearchMode,
  pageCount,
  templateLoading,
  templateError,
  templateEmpty,
  emptyTitle,
  emptyDescription,
  skeletonAspect,
  updateLibraryQuery,
  handleScopeChange,
  handleTabChange,
  handleViewChange,
  handleSortChange,
  handleResetFilters,
  handleSearchUpdate,
  handleSearchSubmit,
  handleSearchClear,
  handleOnlineKeywordSync,
  handleOnlineSourceSync,
  fetchActiveData,
  handleRefresh,
  handleRenameShelf,
  handleDeleteShelf,
  handleRemoveVideoFromShelf,
  handlePoolChanged,
  handleMergedRemoveFavorite,
  handleMergedClearContinue,
} = useVideoLibraryCatalog({
  submitOnlineKeyword: (kw) => {
    onlineSearchRef.value?.submitKeyword(kw)
  },
  clearOnlineKeyword: () => {
    onlineSearchRef.value?.clearKeyword()
  },
  refreshOnlineSearch: async () => {
    await onlineSearchRef.value?.refresh()
  },
})
</script>

<template>
  <div class="px-4 pt-1.5 pb-5 sm:px-6 sm:py-6 lg:px-8">
    <DomainLibraryTemplate
      domain="video"
      :title="pageTitle"
      :count="pageCount"
      :scope="activeScope"
      :sync-scope-url="false"
      :show-scope-bar="true"
      :search-query="searchInput"
      :search-placeholder="
        activeScope === 'online' ? t('online.searchPlaceholder') : t('common.filterPlaceholder')
      "
      :tabs="primaryTabs"
      :active-tab="activeTab"
      :loading="templateLoading"
      :skeleton-aspect="skeletonAspect"
      :error="templateError"
      :empty="templateEmpty"
      :empty-title="emptyTitle"
      :empty-description="emptyDescription"
      :filter-sheet-open="filterSheetOpen"
      :active-filter-count="activeFilterCount"
      @scope-change="handleScopeChange"
      @update:active-tab="handleTabChange"
      @update:search-query="handleSearchUpdate"
      @search-submit="handleSearchSubmit"
      @search-clear="handleSearchClear"
      @retry="fetchActiveData"
      @update:filter-sheet-open="filterSheetOpen = $event"
      @reset-filters="handleResetFilters"
    >
      <template
        v-if="activeScope === 'local' && activeTab === 'all' && !activeCollectionId"
        #toolbar-chips
      >
        <MdChip
          v-for="opt in viewChipOptions"
          :key="opt.value"
          class="shrink-0"
          variant="filter"
          :label="opt.label"
          :selected="activeView === opt.value"
          hide-check
          @click="handleViewChange(opt.value)"
        />

        <MdChip
          v-if="activeLibraryName"
          class="shrink-0"
          variant="input"
          :label="activeLibraryName"
          removable
          @remove="updateLibraryQuery({ library: null })"
        />
        <MdChip
          v-if="selectedGenre"
          class="shrink-0"
          variant="input"
          :label="selectedGenre"
          removable
          @remove="updateLibraryQuery({ genre: null })"
        />
        <MdChip
          v-if="selectedTag"
          class="shrink-0"
          variant="input"
          :label="`#${selectedTag}`"
          removable
          @remove="updateLibraryQuery({ tag: null })"
        />
      </template>

      <template v-if="showVideoSort || activeScope !== 'remote'" #toolbar-trailing>
        <div v-if="showVideoSort" class="hidden w-40 sm:block">
          <MdSelect
            :model-value="sortKey"
            :options="sortOptions"
            variant="compact"
            :aria-label="t('catalog.sortLabel')"
            @change="handleSortChange"
          />
        </div>
        <MdIconButton
          v-if="activeScope !== 'remote'"
          variant="standard"
          size="sm"
          :icon="LuRefreshCw"
          :loading="manualRefreshing"
          :aria-label="t('common.refresh')"
          :tooltip="t('common.refresh')"
          @click="handleRefresh"
        />
      </template>

      <template v-if="showLocalFilterControls" #filter-sheet>
        <VideoCatalogFilterSheet
          :selected-library="selectedLibrary"
          :selected-genre="selectedGenre"
          :selected-tag="selectedTag"
          :kind-filter="kindFilter"
          :sort-key="sortKey"
          :sort-options="sortOptions"
          :active-view="activeView"
          :show-video-sort="showVideoSort"
          @patch-query="updateLibraryQuery"
          @sort-change="handleSortChange"
        />
      </template>

      <template #subheader>
        <VideoCollectionSubheader
          :active-scope="activeScope"
          :active-tab="activeTab"
          :active-collection-id="activeCollectionId"
          :shelf-detail="shelfDetail"
          @back="updateLibraryQuery({ collection: null, tab: 'collections' })"
          @rename="handleRenameShelf"
          @delete="handleDeleteShelf"
        />
      </template>

      <DomainPoolSection
        v-if="isPoolBrowseMode"
        domain="video"
        :search-query="searchInput"
        @changed="handlePoolChanged"
      />

      <OnlineVideoSearchSection
        v-else-if="isOnlineSearchMode"
        ref="onlineSearchRef"
        :keyword="routeQueryQ"
        :initial-source-id="onlineSourceId"
        :show-search-input="false"
        @update:keyword="handleOnlineKeywordSync"
        @update:source-id="handleOnlineSourceSync"
      />

      <div
        v-else-if="isShelfTab"
        class="grid grid-cols-1 gap-5 sm:grid-cols-2 lg:grid-cols-3 xl:grid-cols-4"
      >
        <MediaItemCard
          v-for="(item, idx) in mergedShelfItems"
          :key="
            item.ref.scope === 'local'
              ? `shelf-local-${item.ref.id}-${idx}`
              : `shelf-online-${item.ref.sourceId}-${item.ref.ref}-${idx}`
          "
          :item="item"
          aspect="video"
          cover-style="plain"
          :prefer-play-route="activeTab === 'continue'"
          :show-favorite="activeTab === 'favorites'"
          :favorite="activeTab === 'favorites' ? true : undefined"
          @toggle-favorite="handleMergedRemoveFavorite"
        >
          <template v-if="activeTab === 'continue'" #menu="{ close }">
            <MdMenuItem
              :icon="LuCheck"
              :label="t('detail.clearProgress')"
              @select="handleMergedClearContinue(item, close)"
            />
          </template>
        </MediaItemCard>
      </div>

      <div
        v-else-if="activeCollectionId"
        class="grid grid-cols-1 gap-5 sm:grid-cols-2 lg:grid-cols-3 xl:grid-cols-4"
      >
        <VideoCard
          v-for="v in filteredShelfVideos"
          :key="v.id"
          :video="v"
          removable-from-shelf
          @remove-from-shelf="handleRemoveVideoFromShelf"
          @edit="editingVideo = $event"
          @deleted="fetchActiveData"
        />
      </div>

      <VideoWatchlistsGrid
        v-else-if="activeTab === 'collections'"
        :shelves="filteredShelves"
        @open="updateLibraryQuery({ collection: $event, tab: null })"
        @rename="handleRenameShelf"
        @delete="handleDeleteShelf"
      />

      <div
        v-else-if="activeView === 'genres' || activeView === 'tags'"
        class="flex flex-wrap gap-2"
      >
        <MdChip
          v-for="item in activeView === 'genres' ? filteredGenres : filteredTags"
          :key="item.name"
          variant="assist"
          :icon="activeView === 'genres' ? LuSparkles : LuTag"
          :label="`${activeView === 'tags' ? '#' : ''}${item.name} (${item.count})`"
          @click="
            updateLibraryQuery(
              activeView === 'genres'
                ? { genre: item.name, view: null }
                : { tag: item.name, view: null },
            )
          "
        />
      </div>

      <div
        v-else-if="activeView === 'shows'"
        class="grid grid-cols-2 gap-5 sm:grid-cols-3 md:grid-cols-4 lg:grid-cols-5 xl:grid-cols-6"
      >
        <ShowCard v-for="sh in shows" :key="sh.id" :show="sh" @unbound="fetchActiveData" />
      </div>

      <div
        v-else
        class="grid grid-cols-1 gap-5 sm:grid-cols-2 lg:grid-cols-3 xl:grid-cols-4"
      >
        <VideoCard
          v-for="v in videos"
          :key="v.id"
          :video="v"
          @edit="editingVideo = $event"
          @deleted="fetchActiveData"
          @updated="fetchActiveData"
        />
      </div>

      <template v-if="activeFilterCount > 0 || routeQueryQ" #empty-actions>
        <MdButton
          variant="tonal"
          :icon="LuX"
          @click="
            handleSearchClear();
            handleResetFilters()
          "
        >
          {{ globalT('common.clearFilters') }}
        </MdButton>
      </template>

      <template v-else-if="activeTab === 'continue'" #empty-actions>
        <MdButton variant="tonal" :icon="LuFilm" @click="handleTabChange('all')">
          {{ t('continueView.browseCatalog') }}
        </MdButton>
      </template>
    </DomainLibraryTemplate>

    <VideoMetadataModal
      :open="editingVideo !== null"
      :video="editingVideo"
      @close="editingVideo = null"
      @saved="fetchActiveData"
    />
  </div>
</template>

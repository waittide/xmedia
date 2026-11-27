<script setup lang="ts">
import { ref } from 'vue'
import { useI18n as useGlobalI18n } from 'vue-i18n'
import {
  LuBookOpen,
  LuPlus,
  LuRefreshCw,
  LuSettings2,
  LuTag,
  LuX,
} from 'vue-icons-plus/lu'
import { DomainLibraryTemplate, DomainPoolSection, MediaItemCard } from '@/components/domain'
import RemoteOpdsBrowser from '@/components/remote/RemoteOpdsBrowser.vue'
import {
  MdButton,
  MdChip,
  MdEmptyState,
  MdIconButton,
  MdMenuItem,
} from '@/components/ui'
import ComicCatalogFilterSheet from '@/domains/comic/components/catalog/ComicCatalogFilterSheet.vue'
import ComicCollectionsSection from '@/domains/comic/components/catalog/ComicCollectionsSection.vue'
import ComicMergedShelfSection from '@/domains/comic/components/catalog/ComicMergedShelfSection.vue'
import ComicShelfFormDialog from '@/domains/comic/components/catalog/ComicShelfFormDialog.vue'
import OnlineComicSearchSection from '@/domains/comic/components/OnlineComicSearchSection.vue'
import { useComicLibraryCatalog } from '@/domains/comic/composables/useComicLibraryCatalog'
import { useComicI18n as useI18n } from '@/domains/comic/i18n'
import { useAppClientStore } from '@/stores/appServers'
import { useAuthStore } from '@/stores/auth'

const props = defineProps<{
  presetMode?: 'all' | 'continue' | 'favorites'
}>()

const { t } = useI18n()
const { t: globalT } = useGlobalI18n()
const authStore = useAuthStore()
const appClientStore = useAppClientStore()

const onlineSearchRef = ref<InstanceType<typeof OnlineComicSearchSection> | null>(null)

const {
  scope,
  activeSourceId,
  showRemoteScope,
  activeTab,
  activeView,
  queryFromRoute,
  searchInput,
  libraryId,
  collectionId,
  statusFilter,
  readStatusFilter,
  sortBy,
  sortOrder,
  activeCollection,
  isLocalItemsView,
  isTaxonomyView,
  isCollectionsView,
  isMergedTab,
  isPoolBrowseView,
  isOnlineBrowseView,
  isRemoteBrowseView,
  mergedShelf,
  seriesList,
  manualRefreshing,
  selectedRemoteSourceId,
  filterSheetOpen,
  shelfDialogOpen,
  editingShelf,
  tabs,
  viewOptions,
  activeFilterCount,
  activeFilterChips,
  localSeriesVMs,
  taxonomyItems,
  filteredShelves,
  filteredBookmarks,
  headerTitle,
  headerCount,
  templateLoading,
  templateError,
  templateEmpty,
  templateEmptyTitle,
  updateQuery,
  handleScopeChange,
  handleTabChange,
  handleViewChange,
  selectTaxonomyValue,
  handleSearchSubmit,
  handleSearchClear,
  handleOnlineKeywordSync,
  handleOnlineSourceSync,
  removeFilterChip,
  resetLocalFilters,
  refreshActiveView,
  toggleLocalSeriesFavorite,
  handleUnbindLocalSeries,
  handlePoolChanged,
  openCreateShelf,
  openEditShelf,
  handleDeleteShelf,
  openCollection,
  onRemoteSourceSelect,
  goRemoteSettings,
} = useComicLibraryCatalog({
  presetMode: () => props.presetMode,
  submitOnlineSearch: (q) => {
    void onlineSearchRef.value?.submitSearch(q)
  },
  clearOnlineSearch: () => {
    onlineSearchRef.value?.clearSearch()
  },
  refreshOnlineSearch: async () => {
    await onlineSearchRef.value?.refresh()
  },
})
</script>

<template>
  <div class="px-4 pt-1.5 pb-5 sm:px-6 sm:py-6 md:px-8">
    <DomainLibraryTemplate
      domain="comic"
      :title="headerTitle"
      :count="headerCount"
      :scope="scope"
      :sync-scope-url="false"
      :show-scope-bar="true"
      :show-remote-scope="showRemoteScope"
      :search-query="searchInput"
      :search-placeholder="
        isOnlineBrowseView
          ? t('online.searchPlaceholder')
          : isPoolBrowseView
            ? globalT('pool.searchPlaceholder', { domain: globalT('app.domains.comic') })
            : globalT('common.searchComic')
      "
      :show-search="!isRemoteBrowseView"
      :tabs="tabs"
      :active-tab="activeTab"
      :loading="templateLoading"
      skeleton-aspect="poster"
      :error="templateError"
      :empty="templateEmpty"
      :empty-title="templateEmptyTitle"
      :filter-sheet-open="filterSheetOpen"
      :filter-sheet-title="t('library.filterSheetTitle')"
      :active-filter-count="activeFilterCount"
      @scope-change="handleScopeChange"
      @update:active-tab="handleTabChange"
      @update:search-query="searchInput = $event"
      @search-submit="handleSearchSubmit"
      @search-clear="handleSearchClear"
      @update:filter-sheet-open="filterSheetOpen = $event"
      @reset-filters="resetLocalFilters"
      @retry="refreshActiveView"
    >
      <template v-if="scope === 'local' && activeTab === 'all'" #toolbar-chips>
        <template v-if="!collectionId">
          <MdChip
            v-for="opt in viewOptions"
            :key="opt.value"
            class="shrink-0"
            variant="filter"
            :label="opt.label"
            :selected="activeView === opt.value"
            hide-check
            @click="handleViewChange(opt.value)"
          />
        </template>

        <MdChip
          v-for="chip in activeFilterChips"
          :key="chip.key"
          class="shrink-0"
          variant="input"
          :label="chip.label"
          selected
          removable
          hide-check
          @remove="removeFilterChip(chip.key)"
        />

        <MdButton
          v-if="activeFilterChips.length > 0"
          class="shrink-0"
          size="sm"
          variant="text"
          @click="resetLocalFilters"
        >
          {{ t('series.clearFilters') }}
        </MdButton>
      </template>

      <template v-if="isCollectionsView || !isRemoteBrowseView" #toolbar-trailing>
        <MdButton
          v-if="isCollectionsView"
          :class="templateEmpty ? 'hidden sm:inline-flex' : ''"
          size="sm"
          variant="tonal"
          :icon="LuPlus"
          @click="openCreateShelf"
        >
          {{ t('shelves.newShelf') }}
        </MdButton>
        <MdIconButton
          v-if="!isRemoteBrowseView"
          variant="standard"
          size="sm"
          :icon="LuRefreshCw"
          :loading="manualRefreshing"
          :aria-label="globalT('common.refresh')"
          :tooltip="globalT('common.refresh')"
          @click="refreshActiveView"
        />
      </template>

      <template v-if="activeCollection && scope === 'local'" #subheader>
        <ComicCollectionsSection
          mode="subheader"
          :active-collection="activeCollection"
          @back="updateQuery({ collection: null, tab: 'collections' })"
          @edit="openEditShelf"
          @delete="handleDeleteShelf"
        />
      </template>

      <template v-if="isLocalItemsView" #filter-sheet>
        <ComicCatalogFilterSheet
          :library-id="libraryId"
          :status-filter="statusFilter"
          :read-status-filter="readStatusFilter"
          :sort-by="sortBy"
          :sort-order="sortOrder"
          @patch-query="updateQuery"
        />
      </template>

      <template v-if="isCollectionsView" #empty-actions>
        <MdButton variant="tonal" :icon="LuPlus" @click="openCreateShelf">
          {{ t('shelves.newShelf') }}
        </MdButton>
      </template>
      <template v-else-if="isLocalItemsView && activeFilterChips.length > 0" #empty-actions>
        <MdButton variant="tonal" :icon="LuX" @click="resetLocalFilters">
          {{ t('series.clearFilters') }}
        </MdButton>
      </template>

      <DomainPoolSection
        v-if="isPoolBrowseView"
        domain="comic"
        :search-query="searchInput"
        @changed="handlePoolChanged"
      />

      <OnlineComicSearchSection
        v-else-if="isOnlineBrowseView"
        ref="onlineSearchRef"
        :keyword="queryFromRoute"
        :initial-source-id="activeSourceId"
        :show-search-input="false"
        @update:keyword="handleOnlineKeywordSync"
        @update:source-id="handleOnlineSourceSync"
      />

      <template v-else-if="isRemoteBrowseView">
        <RemoteOpdsBrowser
          v-if="appClientStore.isStandaloneApp"
          :selected-source-id="selectedRemoteSourceId"
          :active="true"
          @update:selected-source-id="onRemoteSourceSelect"
          @add-source="goRemoteSettings"
        />
        <MdEmptyState
          v-else
          :icon="LuSettings2"
          :title="t('library.remoteEmptyTitle')"
          :description="t('library.remoteEmptyDesc')"
        >
          <template v-if="authStore.isAdmin" #action>
            <MdButton variant="tonal" :icon="LuSettings2" @click="goRemoteSettings">
              {{ t('library.remoteManageBtn') }}
            </MdButton>
          </template>
        </MdEmptyState>
      </template>

      <ComicMergedShelfSection
        v-else-if="isMergedTab"
        :items="mergedShelf.items.value"
        :bookmarks="filteredBookmarks"
        :prefer-play-route="activeTab === 'continue'"
      />

      <ComicCollectionsSection
        v-else-if="isCollectionsView"
        mode="grid"
        :shelves="filteredShelves"
        @select="openCollection"
        @edit="openEditShelf"
        @delete="handleDeleteShelf"
      />

      <div
        v-else-if="isTaxonomyView"
        class="grid grid-cols-2 gap-3 sm:grid-cols-3 md:grid-cols-4 lg:grid-cols-5"
      >
        <button
          v-for="item in taxonomyItems"
          :key="item.name"
          type="button"
          class="md-state flex cursor-pointer items-center justify-between gap-2 rounded-corner-md bg-surface-container-low px-4 py-3 text-left transition-colors duration-150 ease-standard hover:bg-surface-container-high"
          @click="selectTaxonomyValue(item.name)"
        >
          <span class="text-title-small flex min-w-0 items-center gap-2 text-on-surface">
            <LuTag class="h-4 w-4 shrink-0 text-primary" aria-hidden="true" />
            <span class="truncate">{{ activeView === 'tags' ? `#${item.name}` : item.name }}</span>
          </span>
          <span
            class="text-label-small shrink-0 rounded-corner-full bg-surface-container-highest px-2 py-0.5 tabular-nums text-on-surface-variant"
          >
            {{ item.count }}
          </span>
        </button>
      </div>

      <div
        v-else
        class="grid grid-cols-2 gap-5 sm:grid-cols-3 md:grid-cols-4 lg:grid-cols-5 xl:grid-cols-6"
      >
        <MediaItemCard
          v-for="(vm, idx) in localSeriesVMs"
          :key="vm.ref.scope === 'local' ? vm.ref.id : idx"
          :item="vm"
          aspect="poster"
          cover-style="book"
          :fallback-icon="LuBookOpen"
          :show-favorite="seriesList[idx]?.is_favorite"
          :favorite="seriesList[idx]?.is_favorite ? true : undefined"
          @toggle-favorite="seriesList[idx] && toggleLocalSeriesFavorite(seriesList[idx]!)"
        >
          <template v-if="seriesList[idx]" #menu="{ close }">
            <MdMenuItem
              :icon="LuX"
              :label="globalT('pool.unbindBtn')"
              destructive
              @select="
                close();
                handleUnbindLocalSeries(seriesList[idx]!)
              "
            />
          </template>
        </MediaItemCard>
      </div>
    </DomainLibraryTemplate>

    <ComicShelfFormDialog
      :open="shelfDialogOpen"
      :editing-shelf="editingShelf"
      @close="shelfDialogOpen = false"
    />
  </div>
</template>

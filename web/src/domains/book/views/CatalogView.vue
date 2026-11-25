<script setup lang="ts">
import { computed, ref } from 'vue'
import { useI18n as useGlobalI18n } from 'vue-i18n'
import {
  LuArrowDown,
  LuArrowUp,
  LuLayoutGrid,
  LuList,
  LuPlus,
  LuRefreshCw,
  LuTag,
  LuUpload,
  LuUser,
} from 'vue-icons-plus/lu'
import { DomainLibraryTemplate, DomainPoolSection, MediaItemCard } from '@/components/domain'
import RemoteOpdsBrowser from '@/components/remote/RemoteOpdsBrowser.vue'
import {
  MdButton,
  MdChip,
  MdIconButton,
  MdSegmentedButton,
  MdSelect,
} from '@/components/ui'
import BookCatalogBooksGrid from '@/domains/book/components/catalog/BookCatalogBooksGrid.vue'
import BookCatalogContinueSection from '@/domains/book/components/catalog/BookCatalogContinueSection.vue'
import BookCatalogFilterSheet from '@/domains/book/components/catalog/BookCatalogFilterSheet.vue'
import BookCatalogListTable from '@/domains/book/components/catalog/BookCatalogListTable.vue'
import BookCatalogSelectionBar from '@/domains/book/components/catalog/BookCatalogSelectionBar.vue'
import BookCatalogShelvesSection from '@/domains/book/components/catalog/BookCatalogShelvesSection.vue'
import OnlineBookSearchSection from '@/domains/book/components/OnlineBookSearchSection.vue'
import { useBookLibraryCatalog } from '@/domains/book/composables/useBookLibraryCatalog'
import { useBookI18n as useI18n } from '@/domains/book/i18n'
import { useCatalogStore } from '@/domains/book/store'
import { useAuthStore } from '@/stores/auth'
import { useUploadStore } from '@/stores/uploads'

const { t } = useI18n()
const { t: globalT } = useGlobalI18n()
const authStore = useAuthStore()
const catalogStore = useCatalogStore()
const uploadStore = useUploadStore()

const uploadInputRef = ref<HTMLInputElement | null>(null)
const onlineSearchRef = ref<InstanceType<typeof OnlineBookSearchSection> | null>(null)

const {
  sourceFilter,
  activeScope,
  onlineSourceId,
  activeTab,
  activeView,
  activeCollectionId,
  searchInput,
  filterSheetOpen,
  manualRefreshing,
  isLocalScope,
  isPoolScope,
  isOnlineScope,
  isRemoteScope,
  continueShelf,
  favoritesShelf,
  selectedRemoteSourceId,
  libraryTabs,
  viewModeOptions,
  sortOptions,
  activeShelf,
  isShowingBooksGrid,
  activeFilterCount,
  totalPages,
  filteredAuthors,
  filteredTags,
  filteredShelves,
  filteredBookmarks,
  headerCount,
  templateLoading,
  templateError,
  templateEmpty,
  emptyTitle,
  emptyDescription,
  resolveLibraryName,
  handleCreateShelf,
  handleRenameShelf,
  handleDeleteShelf,
  handleScopeChange,
  handleTabChange,
  handleViewChange,
  handleLayoutModeChange,
  handleSearchSubmit,
  handleSearchClear,
  handleOnlineKeywordSync,
  handleOnlineSourceSync,
  handleRemoteSourceChange,
  goRemoteSettings,
  triggerRefetch,
  handleSortByChange,
  handlePageChange,
  handleResetFilters,
  selectDimensionAuthor,
  selectDimensionTag,
  selectShelfCollection,
  handleBackToShelves,
  removeFormatChip,
  removeLibraryChip,
  removeAuthorChip,
  removeTagChip,
  toggleSortOrder,
  toggleBookFavorite,
  handleCardMutated,
  handlePoolChanged,
  handleSelectionBatchChanged,
  handleRefresh,
} = useBookLibraryCatalog({
  submitOnlineSearch: (q) => {
    void onlineSearchRef.value?.submitSearch(q)
  },
  clearOnlineSearch: () => {
    onlineSearchRef.value?.clearSearch()
  },
})

const layoutOptions = computed(() => [
  {
    value: 'grid' as const,
    icon: LuLayoutGrid,
    title: t('catalog.gridView'),
    ariaLabel: t('catalog.gridView'),
  },
  {
    value: 'list' as const,
    icon: LuList,
    title: t('catalog.tableView'),
    ariaLabel: t('catalog.tableView'),
  },
])

function handleLayoutKeydown(event: KeyboardEvent, currentIdx: number): void {
  if (
    event.key !== 'ArrowLeft' &&
    event.key !== 'ArrowRight' &&
    event.key !== 'ArrowUp' &&
    event.key !== 'ArrowDown'
  ) {
    return
  }
  event.preventDefault()
  const opts = layoutOptions.value
  const delta = event.key === 'ArrowRight' || event.key === 'ArrowDown' ? 1 : -1
  const nextIdx = (currentIdx + delta + opts.length) % opts.length
  const nextOpt = opts[nextIdx]
  if (!nextOpt) return
  handleLayoutModeChange(nextOpt.value)
  const targetEl = event.currentTarget
  const group = targetEl instanceof HTMLElement ? targetEl.parentElement : null
  const buttons = group?.querySelectorAll<HTMLButtonElement>('[role="radio"]')
  buttons?.[nextIdx]?.focus()
}

function triggerUpload(): void {
  if (!authStore.isAdmin) return
  uploadInputRef.value?.click()
}

function onUploadFilesChange(ev: Event): void {
  const input = ev.target
  if (!(input instanceof HTMLInputElement)) return
  if (input.files && input.files.length > 0) {
    uploadStore.enqueueFiles(input.files)
  }
  input.value = ''
}
</script>

<template>
  <div class="px-4 py-5 sm:px-6 sm:py-6">
    <DomainLibraryTemplate
      domain="book"
      :title="t('catalog.title')"
      :count="headerCount"
      :scope="activeScope"
      :sync-scope-url="false"
      :show-scope-bar="true"
      :show-remote-scope="sourceFilter.hasRemoteSources.value || isRemoteScope"
      :search-query="searchInput"
      :search-placeholder="
        isOnlineScope
          ? t('online.searchPlaceholder')
          : isPoolScope
            ? globalT('pool.searchPlaceholder', { domain: globalT('app.domains.book') })
            : undefined
      "
      :show-search="!isRemoteScope"
      :tabs="libraryTabs"
      :active-tab="activeTab"
      :loading="templateLoading"
      :skeleton-mode="isShowingBooksGrid && catalogStore.viewMode === 'list' ? 'list' : 'grid'"
      skeleton-aspect="poster"
      :error="templateError"
      :empty="templateEmpty"
      :empty-title="emptyTitle"
      :empty-description="emptyDescription"
      :page="isShowingBooksGrid ? catalogStore.page : 1"
      :page-size="catalogStore.pageSize"
      :total="isShowingBooksGrid ? catalogStore.total : null"
      :total-pages="isShowingBooksGrid ? totalPages : null"
      v-model:filter-sheet-open="filterSheetOpen"
      :active-filter-count="activeFilterCount"
      @scope-change="handleScopeChange"
      @update:search-query="searchInput = $event"
      @search-submit="handleSearchSubmit"
      @search-clear="handleSearchClear"
      @update:active-tab="handleTabChange"
      @update:page="handlePageChange"
      @retry="handleRefresh"
      @reset-filters="handleResetFilters"
    >
      <template v-if="isLocalScope && authStore.isAdmin" #header-trailing>
        <MdButton variant="tonal" :icon="LuUpload" @click="triggerUpload">
          {{ t('catalog.uploadBooks') }}
        </MdButton>
      </template>

      <template v-if="isLocalScope && activeTab === 'all'" #toolbar-chips>
        <MdSegmentedButton
          v-if="!activeCollectionId"
          :model-value="activeView"
          :options="viewModeOptions"
          size="sm"
          :show-check="false"
          @update:model-value="handleViewChange"
        />

        <template v-if="isShowingBooksGrid">
          <MdChip
            v-if="activeShelf"
            variant="input"
            selected
            :label="activeShelf.name"
            @remove="selectShelfCollection('')"
          />
          <MdChip
            v-for="fmt in catalogStore.filterFormats"
            :key="`fmt-${fmt}`"
            variant="input"
            selected
            :label="fmt.toUpperCase()"
            @remove="removeFormatChip(fmt)"
          />
          <MdChip
            v-for="libId in catalogStore.filterLibraryIds"
            :key="`lib-${libId}`"
            variant="input"
            selected
            :label="resolveLibraryName(libId)"
            @remove="removeLibraryChip(libId)"
          />
          <MdChip
            v-for="author in catalogStore.filterAuthors"
            :key="`au-${author}`"
            variant="input"
            selected
            :label="author"
            @remove="removeAuthorChip(author)"
          />
          <MdChip
            v-if="catalogStore.filterTags.length > 1"
            variant="filter"
            selected
            hide-check
            :label="
              catalogStore.filterTagMode === 'all'
                ? t('search.tagModeAll')
                : t('search.tagModeAny')
            "
            @click="
              catalogStore.filterTagMode = catalogStore.filterTagMode === 'any' ? 'all' : 'any';
              triggerRefetch()
            "
          />
          <MdChip
            v-for="tg in catalogStore.filterTags"
            :key="`tg-${tg}`"
            variant="input"
            selected
            :label="`#${tg}`"
            @remove="removeTagChip(tg)"
          />
        </template>
      </template>

      <template v-if="isLocalScope" #toolbar-trailing>
        <MdButton
          v-if="activeTab === 'collections' && !activeCollectionId"
          variant="tonal"
          :icon="LuPlus"
          @click="handleCreateShelf"
        >
          {{ t('collections.createShelfBtn') }}
        </MdButton>

        <template v-if="isShowingBooksGrid">
          <div class="w-32 sm:w-36">
            <MdSelect
              :model-value="catalogStore.sortBy"
              :options="sortOptions"
              size="sm"
              @update:model-value="handleSortByChange"
            />
          </div>

          <MdIconButton
            variant="standard"
            size="sm"
            :icon="catalogStore.sortOrder === 'desc' ? LuArrowDown : LuArrowUp"
            :aria-label="
              catalogStore.sortOrder === 'desc' ? t('catalog.sortDesc') : t('catalog.sortAsc')
            "
            :tooltip="
              catalogStore.sortOrder === 'desc' ? t('catalog.sortDesc') : t('catalog.sortAsc')
            "
            @click="toggleSortOrder"
          />

          <div
            role="radiogroup"
            :aria-label="t('catalog.gridView')"
            class="inline-flex h-9 items-stretch overflow-hidden rounded-corner-full border border-outline"
          >
            <button
              v-for="(opt, idx) in layoutOptions"
              :key="opt.value"
              type="button"
              role="radio"
              :aria-checked="catalogStore.viewMode === opt.value"
              :aria-label="opt.ariaLabel"
              :title="opt.title"
              :tabindex="catalogStore.viewMode === opt.value ? 0 : -1"
              class="md-state md-touch-target inline-flex cursor-pointer items-center justify-center px-3 transition-colors duration-150 ease-standard focus-visible:outline-3 focus-visible:-outline-offset-2 focus-visible:outline-secondary"
              :class="[
                idx > 0 ? 'border-l border-outline' : '',
                catalogStore.viewMode === opt.value
                  ? 'bg-secondary-container text-on-secondary-container'
                  : 'bg-transparent text-on-surface hover:bg-surface-container-high/50',
              ]"
              @click="handleLayoutModeChange(opt.value)"
              @keydown="handleLayoutKeydown($event, idx)"
            >
              <component :is="opt.icon" class="h-4 w-4" aria-hidden="true" />
            </button>
          </div>
        </template>

        <MdIconButton
          variant="standard"
          size="sm"
          :icon="LuRefreshCw"
          :loading="manualRefreshing"
          :aria-label="globalT('common.refresh')"
          :tooltip="globalT('common.refresh')"
          @click="handleRefresh"
        />
      </template>

      <template v-if="isShowingBooksGrid && catalogStore.selectedBookIds.size > 0" #selection-bar>
        <BookCatalogSelectionBar @changed="handleSelectionBatchChanged" />
      </template>

      <template v-if="isLocalScope && activeShelf" #subheader>
        <BookCatalogShelvesSection
          mode="subheader"
          :active-shelf="activeShelf"
          @back="handleBackToShelves"
          @rename="handleRenameShelf"
          @delete="handleDeleteShelf"
        />
      </template>

      <template v-if="isShowingBooksGrid" #filter-sheet>
        <BookCatalogFilterSheet
          :active-collection-id="activeCollectionId"
          @refetch="triggerRefetch"
          @select-shelf="selectShelfCollection"
        />
      </template>

      <template #empty-actions>
        <MdButton
          v-if="isLocalScope && activeTab === 'collections'"
          variant="filled"
          :icon="LuPlus"
          @click="handleCreateShelf"
        >
          {{ t('collections.createShelfBtn') }}
        </MdButton>
        <template v-else-if="isShowingBooksGrid">
          <MdButton
            v-if="activeFilterCount > 0 || searchInput.trim()"
            variant="tonal"
            @click="handleResetFilters"
          >
            {{ t('search.reset') }}
          </MdButton>
          <MdButton
            v-if="authStore.isAdmin"
            variant="filled"
            :icon="LuUpload"
            @click="triggerUpload"
          >
            {{ t('catalog.uploadBooks') }}
          </MdButton>
        </template>
      </template>

      <DomainPoolSection
        v-if="isPoolScope"
        domain="book"
        :search-query="searchInput"
        @changed="handlePoolChanged"
      />

      <OnlineBookSearchSection
        v-else-if="isOnlineScope"
        ref="onlineSearchRef"
        :keyword="searchInput"
        :initial-source-id="onlineSourceId"
        :show-search-input="false"
        @update:keyword="handleOnlineKeywordSync"
        @update:source-id="handleOnlineSourceSync"
      />

      <RemoteOpdsBrowser
        v-else-if="isRemoteScope"
        :selected-source-id="selectedRemoteSourceId"
        :active="true"
        @update:selected-source-id="handleRemoteSourceChange"
        @add-source="goRemoteSettings"
      />

      <template v-else>
        <BookCatalogContinueSection
          v-if="activeTab === 'continue'"
          :items="continueShelf.items.value"
          :bookmarks="filteredBookmarks"
        />

        <div
          v-else-if="activeTab === 'favorites'"
          class="grid grid-cols-2 gap-5 sm:grid-cols-3 md:grid-cols-4 lg:grid-cols-5 xl:grid-cols-6"
        >
          <MediaItemCard
            v-for="(item, idx) in favoritesShelf.items.value"
            :key="`fav-${idx}-${item.title}`"
            :item="item"
            aspect="poster"
            cover-style="book"
          />
        </div>

        <BookCatalogShelvesSection
          v-else-if="activeTab === 'collections' && !activeCollectionId"
          mode="grid"
          :shelves="filteredShelves"
          @select="selectShelfCollection"
          @rename="handleRenameShelf"
          @delete="handleDeleteShelf"
        />

        <div v-else-if="activeView === 'authors'" class="flex flex-wrap gap-2">
          <MdChip
            v-for="author in filteredAuthors"
            :key="author.name"
            variant="assist"
            :icon="LuUser"
            :label="`${author.name} (${author.count})`"
            @click="selectDimensionAuthor(author.name)"
          />
        </div>

        <div v-else-if="activeView === 'tags'" class="flex flex-wrap gap-2">
          <MdChip
            v-for="tag in filteredTags"
            :key="tag.name"
            variant="assist"
            :icon="LuTag"
            :label="`#${tag.name} (${tag.count})`"
            @click="selectDimensionTag(tag.name)"
          />
        </div>

        <BookCatalogBooksGrid
          v-else-if="catalogStore.viewMode === 'grid'"
          :books="catalogStore.books"
          @toggle-favorite="toggleBookFavorite"
          @mutated="handleCardMutated"
        />

        <BookCatalogListTable
          v-else
          :books="catalogStore.books"
          @toggle-favorite="toggleBookFavorite"
        />
      </template>
    </DomainLibraryTemplate>

    <input
      v-if="authStore.isAdmin"
      ref="uploadInputRef"
      type="file"
      multiple
      accept=".epub,.txt,.pdf,.mobi,.azw3,.fb2"
      class="hidden"
      @change="onUploadFilesChange"
    />
  </div>
</template>

<script setup lang="ts" generic="S extends OnlineSourceSummaryDto">
import { computed, toRef } from 'vue'
import { useI18n } from 'vue-i18n'
import OnlineSourceCard from './OnlineSourceCard.vue'
import OnlineSourceLoginModal from './OnlineSourceLoginModal.vue'
import OnlineSourceOverflowMenu from './OnlineSourceOverflowMenu.vue'
import OnlineSourceToolbar from './OnlineSourceToolbar.vue'
import type { OnlineSourceManagerAdapter, OnlineSourceSummaryDto } from './types'
import { useOnlineSourceManager } from './useOnlineSourceManager'

const props = defineProps<{
  sources: S[]
  adapter: OnlineSourceManagerAdapter<S>
}>()

const emit = defineEmits<{
  refresh: []
}>()

const { t } = useI18n()

const {
  domainLabel,
  showImportBox,
  importJsonText,
  importing,
  seeding,
  importError,
  searchQuery,
  statusFilter,
  groupFilter,
  batchBusy,
  loginModalSource,
  editSheetSource,
  debugSheetSource,
  selectionMode,
  selectedIds,
  localSources,
  draggingId,
  dropTargetId,
  dropPosition,
  canReorder,
  openMenuSourceId,
  menuPos,
  openMenuSource,
  availableGroups,
  enabledCount,
  disabledCount,
  filteredSources,
  selectedCount,
  allFilteredSelected,
  isSourceBusy,
  closeMenu,
  toggleSelectionMode,
  toggleSelectItem,
  toggleSelectAllFiltered,
  invertSelectFiltered,
  toggleSourceMenu,
  handleDragStart,
  handleDragOver,
  handleDragLeave,
  handleDrop,
  handleDragEnd,
  handleBatchEnable,
  handleBatchSetGroup,
  handleBatchExport,
  handleBatchDelete,
  handleFileSelected,
  handleImportSubmit,
  handleImportSeeds,
  handleToggleEnabled,
  handlePinSource,
  handleOpenLogin,
  handleSearchWithSource,
  handleOpenDebug,
  handleDeleteSource,
  handleSourceUpdated,
  handleSavedAndDebug,
  clearFilters,
} = useOnlineSourceManager(
  toRef(props, 'sources'),
  toRef(props, 'adapter'),
  () => emit('refresh'),
)

const canImportSeeds = computed(() => typeof props.adapter.importSeeds === 'function')
const menuExportUrl = computed(() =>
  openMenuSource.value ? props.adapter.exportSourceUrl(openMenuSource.value.id) : '',
)

function closeEditSheet(): void {
  editSheetSource.value = null
}

function closeDebugSheet(): void {
  debugSheetSource.value = null
}
</script>

<template>
  <!-- IMPORTANT: Avoid `.space-y-4` on outer wrapper so E2E `[role="dialog"], form, .space-y-4` never collides -->
  <div class="flex flex-col gap-4">
    <OnlineSourceToolbar
      :domain-label="domainLabel"
      :search-query="searchQuery"
      :status-filter="statusFilter"
      :group-filter="groupFilter"
      :available-groups="availableGroups"
      :total-count="localSources.length"
      :enabled-count="enabledCount"
      :disabled-count="disabledCount"
      :filtered-count="filteredSources.length"
      :selection-mode="selectionMode"
      :selected-count="selectedCount"
      :all-filtered-selected="allFilteredSelected"
      :can-reorder="canReorder"
      :batch-busy="batchBusy"
      :show-import-box="showImportBox"
      :import-json-text="importJsonText"
      :importing="importing"
      :seeding="seeding"
      :can-import-seeds="canImportSeeds"
      :import-error="importError"
      @update:search-query="searchQuery = $event"
      @update:status-filter="statusFilter = $event"
      @update:group-filter="groupFilter = $event"
      @update:show-import-box="showImportBox = $event"
      @update:import-json-text="importJsonText = $event"
      @toggle-selection-mode="toggleSelectionMode"
      @file-selected="handleFileSelected"
      @submit-import="handleImportSubmit"
      @import-seeds="handleImportSeeds"
      @toggle-select-all="toggleSelectAllFiltered"
      @invert-select="invertSelectFiltered"
      @batch-enable="handleBatchEnable"
      @batch-set-group="handleBatchSetGroup"
      @batch-export="handleBatchExport"
      @batch-delete="handleBatchDelete"
    />

    <!-- Empty State -->
    <div
      v-if="localSources.length === 0"
      class="rounded-[24px] bg-[var(--md-sys-color-surface-container-low)] py-14 text-center"
    >
      <p class="text-sm font-semibold text-[var(--md-sys-color-on-surface)]">
        {{ t('onlineSources.panel.emptyTitle', { domain: domainLabel }) }}
      </p>
      <p class="mt-1 text-xs text-[var(--md-sys-color-on-surface-variant)]">
        {{
          canImportSeeds
            ? t('onlineSources.panel.emptyHintWithSeeds', { domain: domainLabel })
            : t('onlineSources.panel.emptyHint', { domain: domainLabel })
        }}
      </p>
    </div>

    <!-- Empty Filter Result -->
    <div
      v-else-if="filteredSources.length === 0"
      class="rounded-[24px] bg-[var(--md-sys-color-surface-container-low)] py-12 text-center"
    >
      <p class="text-sm font-medium text-[var(--md-sys-color-on-surface)]">
        {{ t('onlineSources.panel.noMatchTitle', { domain: domainLabel }) }}
      </p>
      <button
        type="button"
        class="md-btn md-btn-sm md-btn-text md-state mt-2"
        @click="clearFilters"
      >
        {{ t('onlineSources.panel.clearFiltersBtn') }}
      </button>
    </div>

    <!-- MD3 Draggable / Selectable Source Cards -->
    <div v-else class="flex flex-col gap-2">
      <OnlineSourceCard
        v-for="src in filteredSources"
        :key="src.id"
        :domain-label="domainLabel"
        :source="src"
        :total-count="localSources.length"
        :selection-mode="selectionMode"
        :selected="selectedIds.has(src.id)"
        :can-reorder="canReorder"
        :dragging="draggingId === src.id"
        :drop-indicator="dropTargetId === src.id ? dropPosition : null"
        :busy="isSourceBusy(src.id)"
        :menu-open="openMenuSourceId === src.id"
        @toggle-select="toggleSelectItem(src.id, $event)"
        @toggle-enabled="handleToggleEnabled(src, $event)"
        @edit="editSheetSource = src"
        @toggle-menu="toggleSourceMenu(src, $event)"
        @drag-start="handleDragStart(src, $event)"
        @drag-over="handleDragOver(src, $event)"
        @drag-leave="handleDragLeave(src, $event)"
        @drop="handleDrop(src, $event)"
        @drag-end="handleDragEnd"
      />
    </div>

    <OnlineSourceOverflowMenu
      :source="openMenuSource"
      :pos="menuPos"
      :export-url="menuExportUrl"
      @close="closeMenu"
      @pin="handlePinSource"
      @login="handleOpenLogin"
      @search="handleSearchWithSource"
      @debug="handleOpenDebug"
      @delete="handleDeleteSource"
    />

    <OnlineSourceLoginModal
      :open="loginModalSource !== null"
      :domain-label="domainLabel"
      :source="loginModalSource"
      :get-source-variable="adapter.getSourceVariable"
      :run-login-action="adapter.runLoginAction"
      :update-source="adapter.updateSource"
      @close="loginModalSource = null"
      @updated="handleSourceUpdated"
    />

    <slot
      name="edit-sheet"
      :open="editSheetSource !== null"
      :source="editSheetSource"
      :close="closeEditSheet"
      :saved="() => emit('refresh')"
      :saved-and-debug="handleSavedAndDebug"
    />

    <slot
      name="debug-sheet"
      :open="debugSheetSource !== null"
      :source="debugSheetSource"
      :close="closeDebugSheet"
      :tested="() => emit('refresh')"
    />
  </div>
</template>

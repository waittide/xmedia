<script setup lang="ts">
import { toRef } from 'vue'
import type { BookSourceSummaryDto } from '../onlineApi'
import OnlineSourceDebugSheet from './OnlineSourceDebugSheet.vue'
import OnlineSourceEditSheet from './OnlineSourceEditSheet.vue'
import OnlineSourceLoginModal from './OnlineSourceLoginModal.vue'
import OnlineSourceCard from './online-sources/OnlineSourceCard.vue'
import OnlineSourceOverflowMenu from './online-sources/OnlineSourceOverflowMenu.vue'
import OnlineSourceToolbar from './online-sources/OnlineSourceToolbar.vue'
import { useOnlineSourceManager } from './online-sources/useOnlineSourceManager'

const props = defineProps<{
  sources: BookSourceSummaryDto[]
}>()

const emit = defineEmits<{
  refresh: []
}>()

const {
  showImportBox,
  importJsonText,
  importing,
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
  handleToggleEnabled,
  handlePinSource,
  handleOpenLogin,
  handleSearchWithSource,
  handleOpenDebug,
  handleDeleteSource,
  handleSourceUpdated,
  handleSavedAndDebug,
  clearFilters,
} = useOnlineSourceManager(toRef(props, 'sources'), () => emit('refresh'))
</script>

<template>
  <div class="space-y-4">
    <OnlineSourceToolbar
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
      :import-error="importError"
      @update:search-query="searchQuery = $event"
      @update:status-filter="statusFilter = $event"
      @update:group-filter="groupFilter = $event"
      @update:show-import-box="showImportBox = $event"
      @update:import-json-text="importJsonText = $event"
      @toggle-selection-mode="toggleSelectionMode"
      @file-selected="handleFileSelected"
      @submit-import="handleImportSubmit"
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
      <p class="text-sm font-semibold text-[var(--md-sys-color-on-surface)]">尚未导入任何书源</p>
      <p class="mt-1 text-xs text-[var(--md-sys-color-on-surface-variant)]">
        上传或粘贴 Legado JSON 导入书源
      </p>
    </div>

    <!-- Empty Filter Result -->
    <div
      v-else-if="filteredSources.length === 0"
      class="rounded-[24px] bg-[var(--md-sys-color-surface-container-low)] py-12 text-center"
    >
      <p class="text-sm font-medium text-[var(--md-sys-color-on-surface)]">没有匹配的书源</p>
      <button
        type="button"
        class="md-btn md-btn-sm md-btn-text md-state mt-2"
        @click="clearFilters"
      >
        清除筛选条件
      </button>
    </div>

    <!-- Minimal MD3 Draggable / Selectable Source Cards -->
    <div v-else class="space-y-2">
      <OnlineSourceCard
        v-for="src in filteredSources"
        :key="src.id"
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
      @close="closeMenu"
      @pin="handlePinSource"
      @login="handleOpenLogin"
      @search="handleSearchWithSource"
      @debug="handleOpenDebug"
      @delete="handleDeleteSource"
    />

    <OnlineSourceLoginModal
      :open="loginModalSource !== null"
      :source="loginModalSource"
      @close="loginModalSource = null"
      @updated="handleSourceUpdated"
    />

    <OnlineSourceEditSheet
      :open="editSheetSource !== null"
      :source="editSheetSource"
      @close="editSheetSource = null"
      @saved="emit('refresh')"
      @saved-and-debug="handleSavedAndDebug"
    />

    <OnlineSourceDebugSheet
      :open="debugSheetSource !== null"
      :source="debugSheetSource"
      @close="debugSheetSource = null"
      @tested="emit('refresh')"
    />
  </div>
</template>

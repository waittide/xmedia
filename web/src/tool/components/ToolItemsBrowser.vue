<script setup lang="ts">
import { computed } from 'vue'
import { useI18n } from 'vue-i18n'
import {
  LuArrowDown,
  LuArrowUp,
  LuChevronRight,
  LuColumns3,
  LuCornerLeftUp,
  LuFolder,
  LuFolderPlus,
  LuList,
  LuRadio,
  LuSlidersHorizontal,
  LuSparkles,
  LuUndo2,
} from 'vue-icons-plus/lu'
import AppCheckbox from '@/components/ui/AppCheckbox.vue'
import MdButton from '@/components/ui/MdButton.vue'
import MdChip from '@/components/ui/MdChip.vue'
import MdEmptyState from '@/components/ui/MdEmptyState.vue'
import MdIconButton from '@/components/ui/MdIconButton.vue'
import MdSegmentedButton from '@/components/ui/MdSegmentedButton.vue'
import MdSelect from '@/components/ui/MdSelect.vue'
import MdSkeleton from '@/components/ui/MdSkeleton.vue'
import RefreshButton from '@/components/ui/RefreshButton.vue'
import type { ToolCompletenessFilter } from '../completeness'
import { useToolInspectorStore } from '../stores/inspector'
import { useToolScrapeStore } from '../stores/scrape'
import { useToolWorkspaceStore } from '../stores/workspace'
import { useToolTaskStore, type ScrapeSubItem } from '../tasks'
import type { MediaFileEntry, RawFileEntryDto } from '../types'
import ToolFolderManagerView from './browser/ToolFolderManagerView.vue'
import ToolMediaItemRow from './browser/ToolMediaItemRow.vue'
import ToolSelectionActionBar from './browser/ToolSelectionActionBar.vue'
import { PAGE_CHUNK_SIZE, type SortField } from '../stores/selection'

export type { ToolCompletenessFilter }
export type ToolSortField = SortField

const { t } = useI18n()
const workspaceStore = useToolWorkspaceStore()
const inspectorStore = useToolInspectorStore()
const scrapeStore = useToolScrapeStore()
const taskStore = useToolTaskStore()

function fileTaskStatus(file: MediaFileEntry): ScrapeSubItem['status'] | undefined {
  return taskStore.fileStatusMap.get(`${file.root_id}::${file.rel_path}`)
}

const rootNameMap = computed<Map<string, string>>(() => {
  const m = new Map<string, string>()
  for (const r of workspaceStore.domainRoots) {
    m.set(r.id, r.name)
  }
  return m
})

function fileSelectionKey(file: MediaFileEntry): string {
  return workspaceStore.entrySelectionKey(file)
}

const checkedSelectionKeys = computed<string[]>(() =>
  workspaceStore.checkedFiles.map((f) => fileSelectionKey(f)),
)

const effectiveSortOptions = computed(() => {
  if (!workspaceStore.isFolderManagerMode) return workspaceStore.sortOptions
  return workspaceStore.sortOptions.filter(
    (opt) => opt.value === 'name' || opt.value === 'size' || opt.value === 'modified',
  )
})

const effectiveSortField = computed<ToolSortField>(() => {
  if (
    workspaceStore.isFolderManagerMode &&
    workspaceStore.sortField !== 'name' &&
    workspaceStore.sortField !== 'size' &&
    workspaceStore.sortField !== 'modified'
  ) {
    return 'name'
  }
  return workspaceStore.sortField
})

const sortedFolders = computed(() => {
  const folders = workspaceStore.snapshot?.folders ?? []
  const dir = workspaceStore.sortDir === 'desc' ? -1 : 1
  return [...folders].sort((a, b) => {
    if (effectiveSortField.value === 'size') {
      const diff = a.media_file_count - b.media_file_count
      if (diff !== 0) return diff * dir
    }
    return a.name.localeCompare(b.name, undefined, { numeric: true }) * dir
  })
})

const mediaEntryMap = computed<Map<string, MediaFileEntry>>(() => {
  const m = new Map<string, MediaFileEntry>()
  for (const f of workspaceStore.snapshot?.files ?? []) {
    m.set(f.rel_path, f)
  }
  return m
})

const unorganizedCheckedFiles = computed<MediaFileEntry[]>(() =>
  workspaceStore.checkedFiles.filter((f) => !f.completeness.is_organized),
)

const sortedRawFiles = computed<RawFileEntryDto[]>(() => {
  const cur = workspaceStore.currentRelPath
  const snap = workspaceStore.snapshot
  const rawList: RawFileEntryDto[] =
    snap?.raw_files !== undefined
      ? snap.raw_files
      : (snap?.files ?? [])
          .filter((f) => {
            const idx = f.rel_path.lastIndexOf('/')
            const parent = idx === -1 ? '' : f.rel_path.slice(0, idx)
            return parent === cur
          })
          .map((f) => ({
            rel_path: f.rel_path,
            file_name: f.file_name,
            extension: f.extension,
            file_size: f.file_size,
            modified_unix_secs: f.modified_unix_secs,
            is_media: true,
            media_kind: f.media_kind,
          }))
  const dir = workspaceStore.sortDir === 'asc' ? 1 : -1
  return [...rawList].sort((a, b) => {
    if (effectiveSortField.value === 'size') {
      return (a.file_size - b.file_size) * dir
    }
    if (effectiveSortField.value === 'modified') {
      return (a.modified_unix_secs - b.modified_unix_secs) * dir
    }
    return a.file_name.localeCompare(b.file_name, undefined, { numeric: true }) * dir
  })
})

const folderModeSelectablePaths = computed<string[]>(() => [
  ...sortedFolders.value.map((f) => f.rel_path),
  ...sortedRawFiles.value.map((f) => f.rel_path),
])

const allFolderItemsSelected = computed(
  () =>
    folderModeSelectablePaths.value.length > 0 &&
    folderModeSelectablePaths.value.every((p) => workspaceStore.selectedPaths.has(p)),
)

const selectedRawPaths = computed<string[]>(() => Array.from(workspaceStore.selectedPaths))

const folderRelSet = computed<Set<string>>(
  () => new Set(sortedFolders.value.map((f) => f.rel_path)),
)

const selectedFolderCount = computed<number>(() => {
  let n = 0
  for (const p of workspaceStore.selectedPaths) {
    if (folderRelSet.value.has(p)) n += 1
  }
  return n
})

const folderSelectionSummary = computed<string>(() => {
  const dirs = selectedFolderCount.value
  const files = Math.max(0, workspaceStore.selectedPaths.size - dirs)
  const parts: string[] = []
  if (dirs > 0) parts.push(t('tool.browser.selectedDirs', { count: dirs }))
  if (files > 0) parts.push(t('tool.browser.selectedFiles', { count: files }))
  if (parts.length === 0) {
    return t('tool.batch.selectedCount', { count: workspaceStore.selectedPaths.size })
  }
  const base = t('tool.browser.selectedPrefix', { summary: parts.join(' · ') })
  if (dirs > 0 && workspaceStore.checkedFiles.length > 0) {
    return t('tool.browser.selectedWithMedia', {
      base,
      count: workspaceStore.checkedFiles.length,
    })
  }
  return base
})

function isOrigBackupFile(fileName: string): boolean {
  return /\.orig(?:\.[^./]+)?\.epub$/i.test(fileName)
}

function relativeDirBadge(relPath: string): string | null {
  const idx = relPath.lastIndexOf('/')
  const parent = idx === -1 ? '' : relPath.slice(0, idx)
  const cur = workspaceStore.currentRelPath
  if (!parent || parent === cur) return null
  if (cur && parent.startsWith(`${cur}/`)) {
    return parent.slice(cur.length + 1)
  }
  return parent
}
</script>

<template>
  <div class="relative min-h-full p-4 pb-24 sm:p-6 sm:pb-24">
    <!-- Domain Empty State -->
    <MdEmptyState
      v-if="workspaceStore.domainRoots.length === 0"
      :icon="LuFolderPlus"
      :title="workspaceStore.domainLabels.emptyRootsTitle"
      :description="workspaceStore.domainLabels.emptyRootsDesc"
      class="mx-auto mt-10 max-w-md rounded-corner-xl bg-surface-container-low"
    >
      <template #action>
        <MdButton variant="filled" :icon="LuFolderPlus" @click="workspaceStore.openSettingsTab('roots')">
          {{ t('tool.roots.add') }}
        </MdButton>
      </template>
    </MdEmptyState>

    <template v-else>
      <!-- Clean Header: Breadcrumbs + View Segmented Button + Utility Icons -->
      <div class="mb-3 flex flex-wrap items-center justify-between gap-3">
        <div class="flex min-w-0 flex-wrap items-center gap-0.5 text-body-medium">
          <MdIconButton
            v-if="workspaceStore.snapshot && workspaceStore.snapshot.parent_rel_path !== null"
            size="sm"
            class="mr-1"
            :icon="LuCornerLeftUp"
            :tooltip="t('tool.browser.parentDirTitle')"
            :aria-label="t('tool.browser.parentDirTitle')"
            @click="
              workspaceStore.enterFolder(
                workspaceStore.snapshot.parent_rel_path,
                workspaceStore.viewMode === 'folders',
              )
            "
          />

          <template v-for="(bc, idx) in workspaceStore.breadcrumbs" :key="bc.relPath || '__root__'">
            <LuChevronRight
              v-if="idx > 0"
              class="h-4 w-4 shrink-0 text-on-surface-variant"
              aria-hidden="true"
            />
            <button
              type="button"
              class="md-state cursor-pointer rounded-corner-full px-2.5 py-1 transition"
              :class="
                idx === workspaceStore.breadcrumbs.length - 1
                  ? 'text-title-small text-on-surface'
                  : 'text-on-surface-variant hover:text-on-surface'
              "
              @click="workspaceStore.enterFolder(bc.relPath, workspaceStore.viewMode === 'folders')"
            >
              {{ bc.label }}
            </button>
          </template>

          <span
            v-if="workspaceStore.isFolderManagerMode && workspaceStore.snapshot"
            class="ml-2 hidden rounded-corner-full bg-surface-container-high px-2.5 py-0.5 font-mono text-label-small text-on-surface-variant sm:inline-flex"
          >
            <template v-if="workspaceStore.isMultiRootSummary">
              {{ t('tool.browser.mountedRootsCount', { count: workspaceStore.domainRoots.length }) }}
            </template>
            <template v-else>
              {{
                t('tool.browser.folderFileCount', {
                  folders: sortedFolders.length,
                  files: sortedRawFiles.length,
                })
              }}
            </template>
          </span>
        </div>

        <div class="flex flex-wrap items-center gap-2">
          <MdSegmentedButton
            :model-value="workspaceStore.viewMode"
            :options="[
              { value: 'flat', label: t('tool.browser.viewFlat'), icon: LuList, title: t('tool.browser.viewFlatTitle') },
              { value: 'folders', label: t('tool.browser.viewFolders'), icon: LuFolder, title: t('tool.browser.viewFoldersTitle') },
            ]"
            size="sm"
            @update:model-value="workspaceStore.setViewMode($event)"
          />

          <div v-if="workspaceStore.snapshot" class="flex items-center gap-1">
            <div class="w-28">
              <MdSelect
                :model-value="effectiveSortField"
                :options="effectiveSortOptions"
                size="sm"
                @update:model-value="workspaceStore.setSortField($event as ToolSortField)"
              />
            </div>
            <MdIconButton
              size="sm"
              :icon="workspaceStore.sortDir === 'asc' ? LuArrowUp : LuArrowDown"
              :tooltip="workspaceStore.sortDir === 'asc' ? t('tool.sort.asc') : t('tool.sort.desc')"
              :aria-label="workspaceStore.sortDir === 'asc' ? t('tool.sort.asc') : t('tool.sort.desc')"
              @click="workspaceStore.toggleSortDir()"
            />
          </div>

          <MdIconButton
            v-if="workspaceStore.settings?.xmedia_webhook_enabled"
            size="sm"
            :loading="workspaceStore.notifyingRescan"
            :icon="LuRadio"
            :tooltip="t('tool.batch.triggerRescan')"
            :aria-label="t('tool.batch.triggerRescan')"
            @click="workspaceStore.triggerManualRescan()"
          />

          <MdIconButton
            size="sm"
            :icon="LuUndo2"
            :tooltip="t('tool.browser.historyTooltip')"
            :aria-label="t('tool.browser.historyTooltip')"
            @click="workspaceStore.openHistoryModal('metadata')"
          />

          <RefreshButton
            :action="() => workspaceStore.reloadCurrentScope()"
            :busy="workspaceStore.loadingDir"
            :title="t('tool.actions.refresh')"
          />
        </div>
      </div>

      <!-- Focused Unmatched Review Banner -->
      <div
        v-if="workspaceStore.workbenchFocusPaths && workspaceStore.workbenchFocusPaths.size > 0"
        class="mb-3 flex flex-wrap items-center justify-between gap-2 rounded-corner-lg bg-warning-container px-4 py-2.5 text-body-small text-on-warning-container"
      >
        <span class="font-medium">
          {{ t('tool.browser.focusBanner', { count: workspaceStore.filteredFiles.length }) }}
        </span>
        <MdButton variant="text" @click="workspaceStore.workbenchFocusPaths = null">
          {{ t('tool.browser.exitFocus') }}
        </MdButton>
      </div>

      <!-- MD3 Filter Chips Row + Lightweight Primary Action -->
      <div
        v-if="!workspaceStore.isFolderManagerMode"
        class="mb-4 flex flex-wrap items-center justify-between gap-2"
      >
        <div class="flex flex-wrap items-center gap-1.5">
          <MdChip
            v-for="f in workspaceStore.filterItems"
            :key="f.id"
            variant="filter"
            :selected="workspaceStore.completenessFilter === f.id"
            @click="workspaceStore.selectCompletenessFilter(f.id)"
          >
            <span>{{ f.label }}</span>
            <span class="ml-1 font-mono text-label-small opacity-75">{{ f.count }}</span>
          </MdChip>
        </div>

        <div class="flex items-center gap-2">
          <AppCheckbox
            v-if="workspaceStore.filteredFiles.length > 0"
            :model-value="workspaceStore.allFilteredSelected"
            :label="
              workspaceStore.selectedPaths.size > 0
                ? t('tool.batch.selectedCount', { count: workspaceStore.selectedPaths.size })
                : t('tool.actions.selectAll')
            "
            @change="workspaceStore.toggleSelectAllFiltered()"
          />

          <MdButton
            v-if="workspaceStore.filteredFiles.length > 0 && workspaceStore.selectedPaths.size === 0"
            variant="outlined"
            :icon="LuColumns3"
            :title="t('tool.browser.workbenchTooltip')"
            @click="scrapeStore.openScrapeWorkbench()"
          >
            {{ t('tool.browser.workbenchBtn') }}
          </MdButton>

          <div
            v-if="workspaceStore.filteredFiles.length > 0 && workspaceStore.selectedPaths.size === 0"
            class="inline-flex items-center overflow-hidden rounded-corner-full bg-secondary-container text-on-secondary-container"
          >
            <button
              type="button"
              class="md-state inline-flex h-10 cursor-pointer items-center gap-1.5 pr-3 pl-4 text-label-large"
              :title="t('tool.browser.quickScrapeAllTitle', { summary: scrapeStore.lastScrapeSummary })"
              @click="scrapeStore.quickScrapeSelection()"
            >
              <LuSparkles class="h-4 w-4" aria-hidden="true" />
              <span>{{ t('tool.browser.quickScrapeAll', { count: workspaceStore.filteredFiles.length }) }}</span>
            </button>
            <button
              type="button"
              class="md-state inline-flex h-10 cursor-pointer items-center bg-on-secondary-container/8 px-3 text-label-large"
              :title="t('tool.browser.configureScrapeTitle', { summary: scrapeStore.lastScrapeSummary })"
              :aria-label="t('tool.browser.configureScrapeTitle', { summary: scrapeStore.lastScrapeSummary })"
              @click="scrapeStore.configureScrapeSelection()"
            >
              <LuSlidersHorizontal class="h-4 w-4" aria-hidden="true" />
            </button>
          </div>
        </div>
      </div>

      <div
        v-if="workspaceStore.dirError"
        role="alert"
        class="mb-4 rounded-corner-lg bg-error-container px-4 py-3 text-body-small text-on-error-container"
      >
        {{ workspaceStore.dirError }}
      </div>

      <!-- Loading Skeleton -->
      <div v-if="workspaceStore.loadingDir && !workspaceStore.snapshot" class="space-y-2">
        <MdSkeleton
          v-for="n in 6"
          :key="n"
          height="4rem"
          class="rounded-corner-lg"
        />
      </div>

      <!-- Unified File Manager View -->
      <ToolFolderManagerView
        v-else-if="workspaceStore.isFolderManagerMode"
        :is-multi-root-summary="workspaceStore.isMultiRootSummary"
        :domain-roots="workspaceStore.domainRoots"
        :root-file-counts="workspaceStore.rootFileCounts"
        :snapshot="workspaceStore.snapshot"
        :sorted-folders="sortedFolders"
        :sorted-raw-files="sortedRawFiles"
        :media-entry-map="mediaEntryMap"
        :folder-mode-selectable-paths="folderModeSelectablePaths"
        :all-folder-items-selected="allFolderItemsSelected"
        :selected-paths="workspaceStore.selectedPaths"
        :inspecting-path="inspectorStore.inspectingPath"
        @select-root="(rid, mode) => workspaceStore.selectRoot(rid, mode)"
        @enter-folder="(rel, sw) => workspaceStore.enterFolder(rel, sw)"
        @toggle-select-all-raw="workspaceStore.toggleSelectAllRaw($event)"
        @toggle-select-file="(rel, shift, ordered) => workspaceStore.toggleSelectFile(rel, shift, ordered)"
        @inspect-file="workspaceStore.inspectEntry($event)"
        @open-rename="workspaceStore.openRenameModal($event)"
        @request-delete-files="(paths, del) => workspaceStore.requestDeleteFiles(paths, del)"
      />

      <!-- Empty File List (Flat / Filtered Mode) -->
      <MdEmptyState
        v-else-if="workspaceStore.filteredFiles.length === 0"
        :icon="LuFolder"
        :title="
          workspaceStore.hasActiveSearchOrFilter
            ? t('tool.browser.emptyFilteredTitle')
            : t('tool.browser.emptyDirTitle')
        "
        :description="
          workspaceStore.hasActiveSearchOrFilter
            ? t('tool.browser.emptyFilteredDesc')
            : t('tool.browser.emptyDirDesc')
        "
        class="rounded-corner-xl bg-surface-container-low"
      />

      <!-- MD3 Tonal Media List -->
      <div v-else class="space-y-3">
        <div
          class="divide-y divide-outline-variant/35 overflow-hidden rounded-corner-lg-increased bg-surface-container-low"
        >
          <ToolMediaItemRow
            v-for="file in workspaceStore.visibleFiles"
            :key="fileSelectionKey(file)"
            :file="file"
            :selection-key="fileSelectionKey(file)"
            :is-inspecting="inspectorStore.inspectingPath === fileSelectionKey(file)"
            :is-selected="workspaceStore.selectedPaths.has(fileSelectionKey(file))"
            :root-badge="workspaceStore.isMultiRootSummary ? rootNameMap.get(file.root_id) : undefined"
            :dir-badge="relativeDirBadge(file.rel_path)"
            :task-status="fileTaskStatus(file)"
            :artwork-version="inspectorStore.getFileArtworkVersion(file.root_id, file.rel_path)"
            :is-orig-backup="isOrigBackupFile(file.file_name)"
            @inspect-file="workspaceStore.inspectEntry($event)"
            @toggle-select-file="(key, shift) => workspaceStore.toggleSelectFile(key, shift)"
            @open-book-content-editor="workspaceStore.openBookContentEditor($event)"
            @open-book-normalize-epub="workspaceStore.openBookNormalizeModal($event)"
            @open-comic-page-manager="workspaceStore.openComicPageManager($event)"
            @open-music-toolbox="workspaceStore.openMusicToolbox($event)"
            @open-video-toolbox="workspaceStore.openVideoToolbox($event)"
            @open-rename="workspaceStore.openRenameModal($event)"
            @enqueue-scrape-files="scrapeStore.enqueueScrapeFiles($event)"
            @request-delete-files="(paths, del) => workspaceStore.requestDeleteFiles(paths, del)"
          />
        </div>

        <!-- Load More Bar -->
        <div
          v-if="workspaceStore.filteredFiles.length > workspaceStore.visibleFiles.length"
          class="flex flex-wrap items-center justify-between gap-2 rounded-corner-lg bg-surface-container-low px-4 py-2.5 text-body-small text-on-surface-variant"
        >
          <span>
            {{
              t('tool.batch.showingOf', {
                shown: workspaceStore.visibleFiles.length,
                total: workspaceStore.filteredFiles.length,
              })
            }}
          </span>
          <div class="flex items-center gap-2">
            <MdButton
              variant="tonal"
              @click="workspaceStore.visibleLimit += PAGE_CHUNK_SIZE"
            >
              {{
                t('tool.batch.loadMore', {
                  remaining: workspaceStore.filteredFiles.length - workspaceStore.visibleFiles.length,
                })
              }}
            </MdButton>
            <MdButton
              variant="text"
              @click="workspaceStore.visibleLimit = workspaceStore.filteredFiles.length"
            >
              {{ t('tool.batch.showAll') }}
            </MdButton>
          </div>
        </div>
      </div>
    </template>

    <!-- MD3 Contextual Floating Action Bar -->
    <ToolSelectionActionBar
      :selected-count="workspaceStore.selectedPaths.size"
      :is-folder-manager-mode="workspaceStore.isFolderManagerMode"
      :folder-selection-summary="folderSelectionSummary"
      :checked-files="workspaceStore.checkedFiles"
      :unorganized-checked-files="unorganizedCheckedFiles"
      :selected-book-entries="workspaceStore.selectedBookEntries"
      :selected-comic-entries="workspaceStore.selectedComicEntries"
      :selected-music-entries="workspaceStore.selectedMusicEntries"
      :selected-video-entries="workspaceStore.selectedVideoEntries"
      :selected-raw-paths="selectedRawPaths"
      :selected-folder-count="selectedFolderCount"
      :checked-selection-keys="checkedSelectionKeys"
      :last-scrape-summary="scrapeStore.lastScrapeSummary"
      @clear-selection="workspaceStore.clearSelection()"
      @quick-scrape-selection="scrapeStore.quickScrapeSelection()"
      @configure-scrape-selection="scrapeStore.configureScrapeSelection()"
      @open-scrape-workbench="scrapeStore.openScrapeWorkbench(undefined, $event)"
      @open-batch-metadata="workspaceStore.openBatchMetadataModal()"
      @open-rename="workspaceStore.openRenameModal($event)"
      @open-dry-run="workspaceStore.openDryRunModal()"
      @open-book-normalize-epub="workspaceStore.openBookNormalizeModal($event)"
      @open-comic-series-tools="workspaceStore.openComicSeriesToolsModal()"
      @open-music-toolbox="workspaceStore.openMusicToolbox($event)"
      @open-video-toolbox="workspaceStore.openVideoToolbox($event)"
      @request-delete-files="(paths, del) => workspaceStore.requestDeleteFiles(paths, del)"
    />
  </div>
</template>

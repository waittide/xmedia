<script setup lang="ts">
import { computed, type Component } from 'vue'
import { useI18n } from 'vue-i18n'
import {
  LuArrowDown,
  LuArrowUp,
  LuCheck,
  LuChevronRight,
  LuColumns3,
  LuCornerLeftUp,
  LuFolder,
  LuFolderPlus,
  LuList,
  LuRadio,
  LuRefreshCw,
  LuSlidersHorizontal,
  LuSparkles,
  LuUndo2,
} from 'vue-icons-plus/lu'
import AppCheckbox from '@/components/ui/AppCheckbox.vue'
import MdSelect from '@/components/ui/MdSelect.vue'
import MdSegmentedButton from '@/components/ui/MdSegmentedButton.vue'
import RefreshButton from '@/components/ui/RefreshButton.vue'
import type { MediaKind } from '@/types/api'
import type { ToolCompletenessFilter } from '../completeness'
import { useToolTaskStore, type ScrapeSubItem } from '../tasks'
import type {
  DirectorySnapshot,
  ManagedRootDto,
  MediaFileEntry,
  RawFileEntryDto,
} from '../types'
import ToolFolderManagerView from './browser/ToolFolderManagerView.vue'
import ToolMediaItemRow from './browser/ToolMediaItemRow.vue'
import ToolSelectionActionBar from './browser/ToolSelectionActionBar.vue'

export type { ToolCompletenessFilter }
export type ToolSortField = 'name' | 'title' | 'incomplete' | 'size' | 'modified'

const props = defineProps<{
  activeDomain: MediaKind
  activeRootId: string
  currentRelPath: string
  domainRoots: ManagedRootDto[]
  rootFileCounts: Record<string, number>
  domainLabels: {
    emptyRootsTitle: string
    emptyRootsDesc: string
  }
  breadcrumbs: Array<{ label: string; relPath: string }>
  viewMode: 'flat' | 'folders'
  hasActiveSearchOrFilter: boolean
  completenessFilter: ToolCompletenessFilter
  filterItems: Array<{
    id: ToolCompletenessFilter
    label: string
    count: number
    icon: Component
  }>
  focusPaths: Set<string> | null
  filteredFiles: MediaFileEntry[]
  visibleFiles: MediaFileEntry[]
  allFilteredSelected: boolean
  selectedPaths: Set<string>
  sortField: ToolSortField
  sortDir: 'asc' | 'desc'
  sortOptions: Array<{ value: string; label: string }>
  checkedFiles: MediaFileEntry[]
  selectedComicEntries: MediaFileEntry[]
  selectedMusicEntries: MediaFileEntry[]
  selectedVideoEntries: MediaFileEntry[]
  selectedEntriesForRename: MediaFileEntry[]
  webhookEnabled: boolean
  notifyingRescan: boolean
  loadingDir: boolean
  dirError: string | null
  snapshot: DirectorySnapshot | null
  inspectingPath: string | null
  artworkVersion: number
  artworkVersions?: Record<string, number>
  lastScrapeSummary: string
}>()

const emit = defineEmits<{
  openSettingsRoots: []
  selectRoot: [rootId: string, preferredViewMode?: 'flat' | 'folders']
  enterFolder: [relPath: string, switchToItems?: boolean]
  setViewMode: [mode: 'flat' | 'folders']
  selectCompletenessFilter: [filter: ToolCompletenessFilter]
  clearFocusPaths: []
  toggleSelectAll: []
  toggleSelectAllRaw: [paths: string[]]
  clearSelection: []
  setSortField: [field: ToolSortField]
  toggleSortDir: []
  quickScrapeSelection: []
  configureScrapeSelection: []
  openScrapeWorkbench: [entries?: MediaFileEntry[]]
  openComicSeriesTools: []
  openMusicToolbox: [entries: MediaFileEntry[]]
  openVideoToolbox: [entries: MediaFileEntry[]]
  openDryRun: []
  openBatchMetadata: []
  openHistory: []
  openRename: [entries?: MediaFileEntry[]]
  requestDeleteFiles: [paths: string[], deleteCompanions: boolean]
  triggerRescan: []
  refreshDir: []
  inspectFile: [entry: MediaFileEntry]
  toggleSelectFile: [relPath: string, shiftKey?: boolean, orderedPaths?: string[]]
  openBookContentEditor: [entry: MediaFileEntry]
  openBookNormalizeEpub: [entries: MediaFileEntry[]]
  openComicPageManager: [entry: MediaFileEntry]
  enqueueScrapeFiles: [entries: MediaFileEntry[]]
  loadMore: []
  showAll: []
}>()

const { t } = useI18n()
const taskStore = useToolTaskStore()

function fileTaskStatus(file: MediaFileEntry): ScrapeSubItem['status'] | undefined {
  return taskStore.fileStatusMap.get(`${file.root_id}::${file.rel_path}`)
}

const selectedBookEntries = computed(() =>
  props.checkedFiles.filter(
    (f) =>
      f.media_kind === 'book' &&
      (f.extension === 'txt' || f.extension === 'epub' || f.extension === 'pdf'),
  ),
)

const isMultiRootSummary = computed(
  () => !props.activeRootId && props.domainRoots.length > 1,
)

const rootNameMap = computed<Map<string, string>>(() => {
  const m = new Map<string, string>()
  for (const r of props.domainRoots) {
    m.set(r.id, r.name)
  }
  return m
})

function fileSelectionKey(file: MediaFileEntry): string {
  return isMultiRootSummary.value ? `${file.root_id}::${file.rel_path}` : file.rel_path
}

const checkedSelectionKeys = computed<string[]>(() =>
  props.checkedFiles.map((f) => fileSelectionKey(f)),
)

const isFolderManagerMode = computed(
  () =>
    props.viewMode === 'folders' &&
    !props.hasActiveSearchOrFilter &&
    !(props.focusPaths && props.focusPaths.size > 0),
)

const effectiveSortOptions = computed(() => {
  if (!isFolderManagerMode.value) return props.sortOptions
  return props.sortOptions.filter(
    (opt) => opt.value === 'name' || opt.value === 'size' || opt.value === 'modified',
  )
})

const effectiveSortField = computed<ToolSortField>(() => {
  if (
    isFolderManagerMode.value &&
    props.sortField !== 'name' &&
    props.sortField !== 'size' &&
    props.sortField !== 'modified'
  ) {
    return 'name'
  }
  return props.sortField
})

const sortedFolders = computed(() => {
  const folders = props.snapshot?.folders ?? []
  const dir = props.sortDir === 'desc' ? -1 : 1
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
  for (const f of props.snapshot?.files ?? []) {
    m.set(f.rel_path, f)
  }
  return m
})

const unorganizedCheckedFiles = computed<MediaFileEntry[]>(() =>
  props.checkedFiles.filter((f) => !f.completeness.is_organized),
)

const sortedRawFiles = computed<RawFileEntryDto[]>(() => {
  const cur = props.currentRelPath
  const rawList: RawFileEntryDto[] =
    props.snapshot?.raw_files !== undefined
      ? props.snapshot.raw_files
      : (props.snapshot?.files ?? [])
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
  const dir = props.sortDir === 'asc' ? 1 : -1
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
    folderModeSelectablePaths.value.every((p) => props.selectedPaths.has(p)),
)

const selectedRawPaths = computed<string[]>(() => Array.from(props.selectedPaths))

const folderRelSet = computed<Set<string>>(
  () => new Set(sortedFolders.value.map((f) => f.rel_path)),
)

const selectedFolderCount = computed<number>(() => {
  let n = 0
  for (const p of props.selectedPaths) {
    if (folderRelSet.value.has(p)) n += 1
  }
  return n
})

const folderSelectionSummary = computed<string>(() => {
  const dirs = selectedFolderCount.value
  const files = Math.max(0, props.selectedPaths.size - dirs)
  const parts: string[] = []
  if (dirs > 0) parts.push(t('tool.browser.selectedDirs', { count: dirs }))
  if (files > 0) parts.push(t('tool.browser.selectedFiles', { count: files }))
  if (parts.length === 0) return t('tool.batch.selectedCount', { count: props.selectedPaths.size })
  const base = t('tool.browser.selectedPrefix', { summary: parts.join(' · ') })
  if (dirs > 0 && props.checkedFiles.length > 0) {
    return t('tool.browser.selectedWithMedia', { base, count: props.checkedFiles.length })
  }
  return base
})

function isOrigBackupFile(fileName: string): boolean {
  return /\.orig(?:\.[^./]+)?\.epub$/i.test(fileName)
}

function relativeDirBadge(relPath: string): string | null {
  const idx = relPath.lastIndexOf('/')
  const parent = idx === -1 ? '' : relPath.slice(0, idx)
  const cur = props.currentRelPath
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
    <div
      v-if="domainRoots.length === 0"
      class="mx-auto mt-14 max-w-md rounded-[28px] bg-[var(--md-sys-color-surface-container-low)] p-8 text-center"
    >
      <div
        class="mx-auto mb-4 flex h-14 w-14 items-center justify-center rounded-2xl bg-[var(--md-sys-color-primary-container)] text-[var(--md-sys-color-on-primary-container)]"
      >
        <LuFolderPlus class="h-6 w-6" />
      </div>
      <h2 class="text-base font-semibold text-[var(--md-sys-color-on-surface)]">
        {{ domainLabels.emptyRootsTitle }}
      </h2>
      <p class="mt-2 text-xs leading-relaxed text-[var(--md-sys-color-on-surface-variant)]">
        {{ domainLabels.emptyRootsDesc }}
      </p>
      <button
        type="button"
        class="md-btn md-btn-filled md-state mt-6"
        @click="emit('openSettingsRoots')"
      >
        <LuFolderPlus class="h-4 w-4" />
        <span>{{ t('tool.roots.add') }}</span>
      </button>
    </div>

    <template v-else>
      <!-- Clean Calm Header: Breadcrumbs + View Segmented Button + Utility Icons -->
      <div class="mb-3 flex flex-wrap items-center justify-between gap-3">
        <div class="flex min-w-0 flex-wrap items-center gap-0.5 text-sm">
          <button
            v-if="snapshot && snapshot.parent_rel_path !== null"
            type="button"
            class="md-icon-btn md-icon-btn-sm md-state mr-1"
            :title="t('tool.browser.parentDirTitle')"
            :aria-label="t('tool.browser.parentDirTitle')"
            @click="emit('enterFolder', snapshot.parent_rel_path, viewMode === 'folders')"
          >
            <LuCornerLeftUp class="h-4 w-4" />
          </button>

          <template v-for="(bc, idx) in breadcrumbs" :key="bc.relPath || '__root__'">
            <LuChevronRight
              v-if="idx > 0"
              class="h-4 w-4 shrink-0 text-on-surface-variant"
            />
            <button
              type="button"
              class="md-state rounded-full px-2.5 py-1 transition"
              :class="
                idx === breadcrumbs.length - 1
                  ? 'font-semibold text-on-surface'
                  : 'text-on-surface-variant hover:text-on-surface'
              "
              @click="emit('enterFolder', bc.relPath, viewMode === 'folders')"
            >
              {{ bc.label }}
            </button>
          </template>

          <span
            v-if="isFolderManagerMode && snapshot"
            class="ml-2 hidden rounded-full bg-surface-container-high px-2.5 py-0.5 font-mono text-[11px] text-on-surface-variant sm:inline-flex"
          >
            <template v-if="isMultiRootSummary">
              {{ t('tool.browser.mountedRootsCount', { count: domainRoots.length }) }}
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
            :model-value="viewMode"
            :options="[
              { value: 'flat', label: t('tool.browser.viewFlat'), icon: LuList, title: t('tool.browser.viewFlatTitle') },
              { value: 'folders', label: t('tool.browser.viewFolders'), icon: LuFolder, title: t('tool.browser.viewFoldersTitle') },
            ]"
            size="sm"
            @update:model-value="emit('setViewMode', $event)"
          />

          <div v-if="snapshot" class="flex items-center gap-1">
            <div class="w-28">
              <MdSelect
                :model-value="effectiveSortField"
                :options="effectiveSortOptions"
                size="sm"
                @update:model-value="emit('setSortField', $event as ToolSortField)"
              />
            </div>
            <button
              type="button"
              class="md-icon-btn md-icon-btn-sm md-state"
              :title="sortDir === 'asc' ? t('tool.sort.asc') : t('tool.sort.desc')"
              :aria-label="sortDir === 'asc' ? t('tool.sort.asc') : t('tool.sort.desc')"
              @click="emit('toggleSortDir')"
            >
              <LuArrowUp v-if="sortDir === 'asc'" class="h-4 w-4" />
              <LuArrowDown v-else class="h-4 w-4" />
            </button>
          </div>

          <button
            v-if="webhookEnabled"
            type="button"
            :disabled="notifyingRescan"
            class="md-icon-btn md-icon-btn-sm md-state"
            :title="t('tool.batch.triggerRescan')"
            :aria-label="t('tool.batch.triggerRescan')"
            @click="emit('triggerRescan')"
          >
            <LuRefreshCw v-if="notifyingRescan" class="h-4 w-4 animate-spin text-primary" />
            <LuRadio v-else class="h-4 w-4" />
          </button>

          <button
            type="button"
            class="md-icon-btn md-icon-btn-sm md-state"
            :title="t('tool.browser.historyTooltip')"
            :aria-label="t('tool.browser.historyTooltip')"
            @click="emit('openHistory')"
          >
            <LuUndo2 class="h-4 w-4" />
          </button>

          <RefreshButton
            :action="() => emit('refreshDir')"
            :busy="loadingDir"
            :title="t('tool.actions.refresh')"
          />
        </div>
      </div>

      <!-- Focused Unmatched Review Banner -->
      <div
        v-if="focusPaths && focusPaths.size > 0"
        class="mb-3 flex flex-wrap items-center justify-between gap-2 rounded-2xl bg-warning-container px-4 py-2.5 text-xs text-on-warning-container"
      >
        <span class="font-semibold">
          {{ t('tool.browser.focusBanner', { count: filteredFiles.length }) }}
        </span>
        <button
          type="button"
          class="md-state rounded-full px-3 py-1 text-xs font-semibold"
          @click="emit('clearFocusPaths')"
        >
          {{ t('tool.browser.exitFocus') }}
        </button>
      </div>

      <!-- MD3 Filter Chips Row + Lightweight Primary Action -->
      <div v-if="!isFolderManagerMode" class="mb-4 flex flex-wrap items-center justify-between gap-2">
        <div class="flex flex-wrap items-center gap-1.5">
          <button
            v-for="f in filterItems"
            :key="f.id"
            type="button"
            class="md-chip md-state"
            :class="completenessFilter === f.id ? 'md-chip-selected' : ''"
            @click="emit('selectCompletenessFilter', f.id)"
          >
            <LuCheck v-if="completenessFilter === f.id" class="h-3.5 w-3.5 shrink-0" />
            <span>{{ f.label }}</span>
            <span class="font-mono text-[10px] opacity-75">{{ f.count }}</span>
          </button>
        </div>

        <div class="flex items-center gap-2">
          <AppCheckbox
            v-if="filteredFiles.length > 0"
            :model-value="allFilteredSelected"
            :label="
              selectedPaths.size > 0
                ? t('tool.batch.selectedCount', { count: selectedPaths.size })
                : t('tool.actions.selectAll')
            "
            @change="emit('toggleSelectAll')"
          />

          <button
            v-if="filteredFiles.length > 0 && selectedPaths.size === 0"
            type="button"
            class="md-btn md-btn-sm md-btn-outlined md-state"
            :title="t('tool.browser.workbenchTooltip')"
            @click="emit('openScrapeWorkbench')"
          >
            <LuColumns3 class="h-3.5 w-3.5" />
            <span>{{ t('tool.browser.workbenchBtn') }}</span>
          </button>

          <div
            v-if="filteredFiles.length > 0 && selectedPaths.size === 0"
            class="inline-flex items-center overflow-hidden rounded-full bg-secondary-container text-on-secondary-container"
          >
            <button
              type="button"
              class="md-state inline-flex h-8 items-center gap-1.5 pr-2.5 pl-3.5 text-xs font-semibold"
              :title="t('tool.browser.quickScrapeAllTitle', { summary: lastScrapeSummary })"
              @click="emit('quickScrapeSelection')"
            >
              <LuSparkles class="h-3.5 w-3.5" />
              <span>{{ t('tool.browser.quickScrapeAll', { count: filteredFiles.length }) }}</span>
            </button>
            <button
              type="button"
              class="md-state inline-flex h-8 items-center border-l border-outline-variant/50 px-2.5 text-xs"
              :title="t('tool.browser.configureScrapeTitle', { summary: lastScrapeSummary })"
              :aria-label="t('tool.browser.configureScrapeTitle', { summary: lastScrapeSummary })"
              @click="emit('configureScrapeSelection')"
            >
              <LuSlidersHorizontal class="h-3.5 w-3.5" />
            </button>
          </div>
        </div>
      </div>

      <div
        v-if="dirError"
        class="mb-4 rounded-2xl bg-[var(--md-sys-color-error-container)] px-4 py-3 text-xs text-[var(--md-sys-color-on-error-container)]"
      >
        {{ dirError }}
      </div>

      <!-- Loading Skeleton -->
      <div v-if="loadingDir && !snapshot" class="space-y-2">
        <div
          v-for="n in 6"
          :key="n"
          class="h-16 animate-pulse rounded-2xl bg-[var(--md-sys-color-surface-container-low)]"
        />
      </div>

      <!-- Unified File Manager View -->
      <ToolFolderManagerView
        v-else-if="isFolderManagerMode"
        :is-multi-root-summary="isMultiRootSummary"
        :domain-roots="domainRoots"
        :root-file-counts="rootFileCounts"
        :snapshot="snapshot"
        :sorted-folders="sortedFolders"
        :sorted-raw-files="sortedRawFiles"
        :media-entry-map="mediaEntryMap"
        :folder-mode-selectable-paths="folderModeSelectablePaths"
        :all-folder-items-selected="allFolderItemsSelected"
        :selected-paths="selectedPaths"
        :inspecting-path="inspectingPath"
        @select-root="(rid, mode) => emit('selectRoot', rid, mode)"
        @enter-folder="(rel, sw) => emit('enterFolder', rel, sw)"
        @toggle-select-all-raw="emit('toggleSelectAllRaw', $event)"
        @toggle-select-file="(rel, shift, ordered) => emit('toggleSelectFile', rel, shift, ordered)"
        @inspect-file="emit('inspectFile', $event)"
        @open-rename="emit('openRename', $event)"
        @request-delete-files="(paths, del) => emit('requestDeleteFiles', paths, del)"
      />

      <!-- Empty File List (Flat / Filtered Mode) -->
      <div
        v-else-if="filteredFiles.length === 0"
        class="rounded-[24px] bg-surface-container-low py-16 text-center"
      >
        <p class="text-sm font-semibold text-on-surface">
          {{ hasActiveSearchOrFilter ? t('tool.browser.emptyFilteredTitle') : t('tool.browser.emptyDirTitle') }}
        </p>
        <p class="mt-1 text-xs text-on-surface-variant">
          {{ hasActiveSearchOrFilter ? t('tool.browser.emptyFilteredDesc') : t('tool.browser.emptyDirDesc') }}
        </p>
      </div>

      <!-- MD3 Tonal Media List -->
      <div v-else class="space-y-3">
        <div
          class="divide-y divide-outline-variant/35 overflow-hidden rounded-[20px] bg-surface-container-low"
        >
          <ToolMediaItemRow
            v-for="file in visibleFiles"
            :key="fileSelectionKey(file)"
            :file="file"
            :selection-key="fileSelectionKey(file)"
            :is-inspecting="inspectingPath === fileSelectionKey(file)"
            :is-selected="selectedPaths.has(fileSelectionKey(file))"
            :root-badge="isMultiRootSummary ? rootNameMap.get(file.root_id) : undefined"
            :dir-badge="relativeDirBadge(file.rel_path)"
            :task-status="fileTaskStatus(file)"
            :artwork-version="artworkVersion + (artworkVersions?.[`${file.root_id}::${file.rel_path}`] ?? 0)"
            :is-orig-backup="isOrigBackupFile(file.file_name)"
            @inspect-file="emit('inspectFile', $event)"
            @toggle-select-file="(key, shift) => emit('toggleSelectFile', key, shift)"
            @open-book-content-editor="emit('openBookContentEditor', $event)"
            @open-book-normalize-epub="emit('openBookNormalizeEpub', $event)"
            @open-comic-page-manager="emit('openComicPageManager', $event)"
            @open-music-toolbox="emit('openMusicToolbox', $event)"
            @open-video-toolbox="emit('openVideoToolbox', $event)"
            @open-rename="emit('openRename', $event)"
            @enqueue-scrape-files="emit('enqueueScrapeFiles', $event)"
            @request-delete-files="(paths, del) => emit('requestDeleteFiles', paths, del)"
          />
        </div>

        <!-- Load More Bar -->
        <div
          v-if="filteredFiles.length > visibleFiles.length"
          class="flex flex-wrap items-center justify-between gap-2 rounded-2xl bg-surface-container-low px-4 py-2.5 text-xs text-on-surface-variant"
        >
          <span>
            {{
              t('tool.batch.showingOf', {
                shown: visibleFiles.length,
                total: filteredFiles.length,
              })
            }}
          </span>
          <div class="flex items-center gap-2">
            <button
              type="button"
              class="md-btn md-btn-sm md-btn-tonal md-state"
              @click="emit('loadMore')"
            >
              {{
                t('tool.batch.loadMore', {
                  remaining: filteredFiles.length - visibleFiles.length,
                })
              }}
            </button>
            <button
              type="button"
              class="md-btn md-btn-sm md-btn-text md-state"
              @click="emit('showAll')"
            >
              {{ t('tool.batch.showAll') }}
            </button>
          </div>
        </div>
      </div>
    </template>

    <!-- MD3 Contextual Floating Action Bar -->
    <ToolSelectionActionBar
      :selected-count="selectedPaths.size"
      :is-folder-manager-mode="isFolderManagerMode"
      :folder-selection-summary="folderSelectionSummary"
      :checked-files="checkedFiles"
      :unorganized-checked-files="unorganizedCheckedFiles"
      :selected-book-entries="selectedBookEntries"
      :selected-comic-entries="selectedComicEntries"
      :selected-music-entries="selectedMusicEntries"
      :selected-video-entries="selectedVideoEntries"
      :selected-raw-paths="selectedRawPaths"
      :selected-folder-count="selectedFolderCount"
      :checked-selection-keys="checkedSelectionKeys"
      :last-scrape-summary="lastScrapeSummary"
      @clear-selection="emit('clearSelection')"
      @quick-scrape-selection="emit('quickScrapeSelection')"
      @configure-scrape-selection="emit('configureScrapeSelection')"
      @open-scrape-workbench="emit('openScrapeWorkbench', $event)"
      @open-batch-metadata="emit('openBatchMetadata')"
      @open-rename="emit('openRename', $event)"
      @open-dry-run="emit('openDryRun')"
      @open-book-normalize-epub="emit('openBookNormalizeEpub', $event)"
      @open-comic-series-tools="emit('openComicSeriesTools')"
      @open-music-toolbox="emit('openMusicToolbox', $event)"
      @open-video-toolbox="emit('openVideoToolbox', $event)"
      @request-delete-files="(paths, del) => emit('requestDeleteFiles', paths, del)"
    />
  </div>
</template>

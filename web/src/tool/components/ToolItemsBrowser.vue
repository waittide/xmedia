<script setup lang="ts">
import { computed, type Component } from 'vue'
import {
  LuArrowDown,
  LuArrowUp,
  LuBookOpen,
  LuCheck,
  LuChevronRight,
  LuClapperboard,
  LuColumns3,
  LuCornerLeftUp,
  LuDisc3,
  LuFile,
  LuFileSignature,
  LuFileText,
  LuFilm,
  LuFolder,
  LuFolderPlus,
  LuImage,
  LuLayers,
  LuList,
  LuLoader2,
  LuMusic,
  LuPackageCheck,
  LuPencil,
  LuRadio,
  LuRefreshCw,
  LuSparkles,
  LuTrash2,
  LuUndo2,
  LuX,
} from 'vue-icons-plus/lu'
import AppCheckbox from '@/components/ui/AppCheckbox.vue'
import AppSelect from '@/components/ui/AppSelect.vue'
import MdSegmentedButton from '@/components/ui/MdSegmentedButton.vue'
import RefreshButton from '@/components/ui/RefreshButton.vue'
import type { MediaKind } from '@/types/api'
import { buildArtworkUrl } from '../api'
import { formatBytes, useToolText } from '../i18n'
import { useToolTaskStore, type ScrapeSubItem } from '../tasks'
import type {
  DirectorySnapshot,
  ManagedRootDto,
  MediaFileEntry,
  RawFileEntryDto,
} from '../types'
import type { ToolCompletenessFilter } from './ToolSidebar.vue'

export type ToolSortField = 'name' | 'title' | 'incomplete' | 'size' | 'modified'

const props = defineProps<{
  activeDomain: MediaKind
  currentRelPath: string
  domainRoots: ManagedRootDto[]
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
}>()

const emit = defineEmits<{
  openSettingsRoots: []
  enterFolder: [relPath: string, switchToItems?: boolean]
  setViewMode: [mode: 'flat' | 'folders']
  selectCompletenessFilter: [filter: ToolCompletenessFilter]
  clearFocusPaths: []
  toggleSelectAll: []
  toggleSelectAllRaw: [paths: string[]]
  clearSelection: []
  setSortField: [field: ToolSortField]
  toggleSortDir: []
  enqueueScrapeSelection: []
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
  toggleSelectFile: [relPath: string, shiftKey?: boolean]
  openBookContentEditor: [entry: MediaFileEntry]
  openBookNormalizeEpub: [entries: MediaFileEntry[]]
  openComicPageManager: [entry: MediaFileEntry]
  enqueueScrapeFiles: [entries: MediaFileEntry[]]
  loadMore: []
  showAll: []
}>()

const txt = useToolText()
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

const isFolderManagerMode = computed(
  () =>
    props.viewMode === 'folders' &&
    !props.hasActiveSearchOrFilter &&
    !(props.focusPaths && props.focusPaths.size > 0),
)

const sortedFolders = computed(() => {
  const folders = props.snapshot?.folders ?? []
  const dir = props.sortField === 'name' && props.sortDir === 'desc' ? -1 : 1
  return [...folders].sort(
    (a, b) => a.name.localeCompare(b.name, undefined, { numeric: true }) * dir,
  )
})

const mediaEntryMap = computed<Map<string, MediaFileEntry>>(() => {
  const m = new Map<string, MediaFileEntry>()
  for (const f of props.snapshot?.files ?? []) {
    m.set(f.rel_path, f)
  }
  return m
})

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
    if (props.sortField === 'size') {
      return (a.file_size - b.file_size) * dir
    }
    if (props.sortField === 'modified') {
      return (a.modified_unix_secs - b.modified_unix_secs) * dir
    }
    return a.file_name.localeCompare(b.file_name, undefined, { numeric: true }) * dir
  })
})

const allRawFilesSelected = computed(
  () =>
    sortedRawFiles.value.length > 0 &&
    sortedRawFiles.value.every((f) => props.selectedPaths.has(f.rel_path)),
)

const selectedRawPaths = computed<string[]>(() => Array.from(props.selectedPaths))

function rawFileCategory(
  raw: RawFileEntryDto,
): 'image' | 'sidecar' | 'book' | 'comic' | 'music' | 'video' | 'other' {
  const ext = raw.extension.toLowerCase()
  if (['jpg', 'jpeg', 'png', 'webp', 'gif', 'bmp', 'avif', 'svg'].includes(ext)) {
    return 'image'
  }
  if (['opf', 'nfo', 'xml', 'lrc', 'srt', 'ass', 'ssa', 'vtt', 'json', 'cue'].includes(ext)) {
    return 'sidecar'
  }
  if (raw.media_kind === 'book') return 'book'
  if (raw.media_kind === 'comic') return 'comic'
  if (raw.media_kind === 'music') return 'music'
  if (raw.media_kind === 'video') return 'video'
  return 'other'
}

function rawFileKindLabel(raw: RawFileEntryDto): string {
  const ext = raw.extension.toLowerCase()
  if (ext === 'epub' && isOrigBackupFile(raw.file_name)) return '原件备份'
  if (['opf', 'nfo', 'xml'].includes(ext)) return '元数据文件'
  if (['lrc', 'srt', 'ass', 'ssa', 'vtt'].includes(ext)) return '歌词 / 字幕'
  if (['jpg', 'jpeg', 'png', 'webp', 'gif', 'bmp', 'avif', 'svg'].includes(ext)) return '图片'
  if (raw.is_media && raw.media_kind === 'book') return '图书'
  if (raw.is_media && raw.media_kind === 'comic') return '漫画'
  if (raw.is_media && raw.media_kind === 'music') return '音频'
  if (raw.is_media && raw.media_kind === 'video') return '视频'
  return '普通文件'
}

function handleRawFileRowClick(raw: RawFileEntryDto): void {
  const media = mediaEntryMap.value.get(raw.rel_path)
  if (media) {
    emit('inspectFile', media)
  }
}

function formatModifiedDate(unixSecs: number): string {
  if (!unixSecs) return '—'
  const d = new Date(unixSecs * 1000)
  const y = d.getFullYear()
  const m = String(d.getMonth() + 1).padStart(2, '0')
  const day = String(d.getDate()).padStart(2, '0')
  return `${y}-${m}-${day}`
}

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
        <span>{{ txt.roots.add }}</span>
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
            title="返回上级目录"
            @click="emit('enterFolder', snapshot.parent_rel_path, viewMode === 'folders')"
          >
            <LuCornerLeftUp class="h-4 w-4" />
          </button>

          <template v-for="(bc, idx) in breadcrumbs" :key="bc.relPath || '__root__'">
            <LuChevronRight
              v-if="idx > 0"
              class="h-4 w-4 shrink-0 text-[var(--md-sys-color-on-surface-variant)]"
            />
            <button
              type="button"
              class="md-state rounded-full px-2.5 py-1 transition"
              :class="
                idx === breadcrumbs.length - 1
                  ? 'font-semibold text-[var(--md-sys-color-on-surface)]'
                  : 'text-[var(--md-sys-color-on-surface-variant)] hover:text-[var(--md-sys-color-on-surface)]'
              "
              @click="emit('enterFolder', bc.relPath, viewMode === 'folders')"
            >
              {{ bc.label }}
            </button>
          </template>

          <span
            v-if="isFolderManagerMode && snapshot"
            class="ml-2 hidden rounded-full bg-[var(--md-sys-color-surface-container-high)] px-2.5 py-0.5 font-mono text-[11px] text-[var(--md-sys-color-on-surface-variant)] sm:inline-flex"
          >
            {{ sortedFolders.length }} 文件夹 · {{ sortedRawFiles.length }} 文件
          </span>
        </div>

        <div class="flex flex-wrap items-center gap-2">
          <MdSegmentedButton
            :model-value="viewMode"
            :options="[
              { value: 'flat', label: '全部媒体', icon: LuList, title: '平铺展示当前目录及子目录内所有媒体' },
              { value: 'folders', label: '文件夹', icon: LuFolder, title: '按文件夹层级浏览' },
            ]"
            size="sm"
            @update:model-value="emit('setViewMode', $event)"
          />

          <div v-if="filteredFiles.length > 1" class="flex items-center gap-1">
            <div class="w-28">
              <AppSelect
                :model-value="sortField"
                :options="sortOptions"
                size="sm"
                @update:model-value="emit('setSortField', $event as ToolSortField)"
              />
            </div>
            <button
              type="button"
              class="md-icon-btn md-icon-btn-sm md-state"
              :title="sortDir === 'asc' ? txt.sort.asc : txt.sort.desc"
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
            :title="txt.batch.triggerRescan"
            @click="emit('triggerRescan')"
          >
            <LuRefreshCw v-if="notifyingRescan" class="h-4 w-4 animate-spin text-[var(--md-sys-color-primary)]" />
            <LuRadio v-else class="h-4 w-4" />
          </button>

          <button
            type="button"
            class="md-icon-btn md-icon-btn-sm md-state"
            title="变更历史与一键撤销"
            @click="emit('openHistory')"
          >
            <LuUndo2 class="h-4 w-4" />
          </button>

          <RefreshButton
            :action="() => emit('refreshDir')"
            :busy="loadingDir"
            :title="txt.actions.refresh"
          />
        </div>
      </div>

      <!-- Focused Unmatched Review Banner (From Tasks Center) -->
      <div
        v-if="focusPaths && focusPaths.size > 0"
        class="mb-3 flex flex-wrap items-center justify-between gap-2 rounded-2xl bg-[var(--md-sys-color-warning-container)] px-4 py-2.5 text-xs text-[var(--md-sys-color-on-warning-container)]"
      >
        <span class="font-semibold">
          正在聚焦查看任务结果（{{ filteredFiles.length }} 项）· 点击条目即可在右侧精修元信息或对照在线候选
        </span>
        <button
          type="button"
          class="md-state rounded-full px-3 py-1 text-xs font-semibold"
          @click="emit('clearFocusPaths')"
        >
          退出聚焦
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
                ? txt.batch.selectedCount.replace('{count}', String(selectedPaths.size))
                : txt.actions.selectAll
            "
            @change="emit('toggleSelectAll')"
          />

          <button
            v-if="filteredFiles.length > 0 && selectedPaths.size === 0"
            type="button"
            class="md-btn md-btn-sm md-btn-outlined md-state"
            title="进入三栏刮削工作台（左本地 · 中刮削源 · 右对照编辑）"
            @click="emit('openScrapeWorkbench')"
          >
            <LuColumns3 class="h-3.5 w-3.5" />
            <span>刮削工作台</span>
          </button>

          <button
            v-if="filteredFiles.length > 0 && selectedPaths.size === 0"
            type="button"
            class="md-btn md-btn-sm md-btn-tonal md-state"
            title="对当前列表批量配置并启动自动刮削"
            @click="emit('enqueueScrapeSelection')"
          >
            <LuSparkles class="h-3.5 w-3.5" />
            <span>自动刮削全部 ({{ filteredFiles.length }})</span>
          </button>
        </div>
      </div>

      <div
        v-else
        class="mb-3 flex flex-wrap items-center justify-between gap-2 text-xs text-[var(--md-sys-color-on-surface-variant)]"
      >
        <span>
          文件视图 · 展示当前目录原始文件（含 .opf / .nfo / 封面 / 字幕等外挂），删除仅移除所选文件本身
        </span>
        <AppCheckbox
          v-if="sortedRawFiles.length > 0"
          :model-value="allRawFilesSelected"
          :label="
            selectedPaths.size > 0
              ? txt.batch.selectedCount.replace('{count}', String(selectedPaths.size))
              : txt.actions.selectAll
          "
          @change="emit('toggleSelectAllRaw', sortedRawFiles.map((f) => f.rel_path))"
        />
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

      <!-- Unified File Manager View (Folders Mode without active search/filter) -->
      <div v-else-if="isFolderManagerMode" class="space-y-3">
        <div
          v-if="
            sortedFolders.length === 0 &&
            filteredFiles.length === 0 &&
            (!snapshot || snapshot.parent_rel_path === null)
          "
          class="rounded-[24px] bg-[var(--md-sys-color-surface-container-low)] py-16 text-center"
        >
          <p class="text-sm font-semibold text-[var(--md-sys-color-on-surface)]">
            当前文件夹暂无内容
          </p>
          <p class="mt-1 text-xs text-[var(--md-sys-color-on-surface-variant)]">
            请从左侧切换其他媒体目录，或先往该目录添加媒体文件
          </p>
        </div>

        <div
          v-else
          class="overflow-hidden rounded-[20px] bg-[var(--md-sys-color-surface-container-low)]"
        >
          <!-- File Manager Table Header -->
          <div
            class="flex items-center gap-3.5 border-b border-[var(--md-sys-color-outline-variant)]/45 bg-[var(--md-sys-color-surface-container)]/60 px-4 py-2 text-[11px] font-semibold text-[var(--md-sys-color-on-surface-variant)]"
          >
            <div class="flex w-5 shrink-0 items-center justify-center">
              <AppCheckbox
                v-if="sortedRawFiles.length > 0"
                :model-value="allRawFilesSelected"
                @change="emit('toggleSelectAllRaw', sortedRawFiles.map((f) => f.rel_path))"
              />
            </div>
            <div class="w-10 shrink-0" />
            <div class="min-w-0 flex-1">文件名</div>
            <div class="hidden w-28 shrink-0 text-right md:block">大小 / 项数</div>
            <div class="hidden w-24 shrink-0 text-right xl:block">修改日期</div>
            <div class="hidden w-32 shrink-0 text-right sm:block">类型</div>
            <div class="w-24 shrink-0 text-right">操作</div>
          </div>

          <div class="divide-y divide-[var(--md-sys-color-outline-variant)]/35">
            <!-- Parent Directory Row (..) -->
            <button
              v-if="snapshot && snapshot.parent_rel_path !== null"
              type="button"
              class="md-state flex w-full items-center gap-3.5 px-4 py-2.5 text-left"
              @click="emit('enterFolder', snapshot.parent_rel_path, true)"
            >
              <div class="w-5 shrink-0" />
              <div
                class="flex h-10 w-10 shrink-0 items-center justify-center rounded-xl bg-[var(--md-sys-color-surface-container-high)] text-[var(--md-sys-color-on-surface-variant)]"
              >
                <LuCornerLeftUp class="h-4 w-4" />
              </div>
              <div class="min-w-0 flex-1">
                <div class="flex items-center gap-2">
                  <span class="font-mono text-[13px] font-semibold text-[var(--md-sys-color-on-surface)]">
                    ..
                  </span>
                  <span class="text-xs text-[var(--md-sys-color-on-surface-variant)]">
                    返回上级目录
                  </span>
                </div>
              </div>
              <div class="hidden w-28 shrink-0 text-right text-xs text-[var(--md-sys-color-on-surface-variant)] md:block">
                —
              </div>
              <div class="hidden w-24 shrink-0 text-right text-xs text-[var(--md-sys-color-on-surface-variant)] xl:block">
                —
              </div>
              <div class="hidden w-32 shrink-0 sm:block" />
              <div class="flex w-24 shrink-0 justify-end text-[var(--md-sys-color-on-surface-variant)]">
                <LuChevronRight class="h-4 w-4 opacity-50" />
              </div>
            </button>

            <!-- Subfolder Rows -->
            <button
              v-for="folder in sortedFolders"
              :key="`fm-dir-${folder.rel_path}`"
              type="button"
              class="md-state group flex w-full items-center gap-3.5 px-4 py-2.5 text-left"
              @click="emit('enterFolder', folder.rel_path, true)"
            >
              <div class="w-5 shrink-0" />
              <div
                class="flex h-10 w-10 shrink-0 items-center justify-center rounded-xl bg-[var(--md-sys-color-primary-container)]/85 text-[var(--md-sys-color-on-primary-container)]"
              >
                <LuFolder class="h-4 w-4" />
              </div>
              <div class="min-w-0 flex-1">
                <div class="flex items-center gap-2">
                  <span class="truncate text-[13px] font-semibold text-[var(--md-sys-color-on-surface)]">
                    {{ folder.name }}
                  </span>
                  <span
                    class="shrink-0 rounded-full bg-[var(--md-sys-color-surface-container-high)] px-2 py-0.5 text-[10px] font-medium text-[var(--md-sys-color-on-surface-variant)]"
                  >
                    文件夹
                  </span>
                </div>
                <div class="mt-0.5 text-[11px] text-[var(--md-sys-color-on-surface-variant)] md:hidden">
                  <template v-if="folder.media_file_count > 0 || folder.child_dir_count > 0">
                    {{ folder.media_file_count }} 项
                    <template v-if="folder.child_dir_count > 0">
                      · {{ folder.child_dir_count }} 子目录
                    </template>
                  </template>
                  <template v-else>空目录</template>
                </div>
              </div>
              <div class="hidden w-28 shrink-0 text-right font-mono text-[11px] text-[var(--md-sys-color-on-surface-variant)] md:block">
                <template v-if="folder.media_file_count > 0 || folder.child_dir_count > 0">
                  <span>{{ folder.media_file_count }} 项</span>
                  <span v-if="folder.child_dir_count > 0" class="opacity-75">
                    / {{ folder.child_dir_count }} 目录
                  </span>
                </template>
                <span v-else class="opacity-60">空目录</span>
              </div>
              <div class="hidden w-24 shrink-0 text-right font-mono text-[11px] text-[var(--md-sys-color-on-surface-variant)] opacity-60 xl:block">
                —
              </div>
              <div class="hidden w-32 shrink-0 justify-end sm:flex">
                <span class="rounded-full bg-[var(--md-sys-color-surface-container-high)] px-2 py-0.5 text-[10px] text-[var(--md-sys-color-on-surface-variant)]">
                  文件夹
                </span>
              </div>
              <div class="flex w-24 shrink-0 justify-end text-[var(--md-sys-color-on-surface-variant)]">
                <LuChevronRight class="h-4 w-4 opacity-60 transition group-hover:translate-x-0.5 group-hover:opacity-100" />
              </div>
            </button>

            <!-- Empty Subfolder Inline Notice (keeps '..' row clickable) -->
            <div
              v-if="sortedFolders.length === 0 && sortedRawFiles.length === 0"
              class="py-12 text-center text-xs text-[var(--md-sys-color-on-surface-variant)]"
            >
              此文件夹为空
            </div>

            <!-- Raw File Rows in Current Folder -->
            <div
              v-for="raw in sortedRawFiles"
              :key="`fm-raw-${raw.rel_path}`"
              class="md-state group flex items-center gap-3.5 px-4 py-2.5"
              :class="[
                mediaEntryMap.has(raw.rel_path) ? 'cursor-pointer' : 'cursor-default',
                inspectingPath === raw.rel_path
                  ? 'bg-[var(--md-sys-color-secondary-container)]/75'
                  : selectedPaths.has(raw.rel_path)
                    ? 'bg-[var(--md-sys-color-primary-container)]/35'
                    : '',
              ]"
              @click="handleRawFileRowClick(raw)"
            >
              <div
                class="flex w-5 shrink-0 items-center justify-center"
                @click.stop="emit('toggleSelectFile', raw.rel_path, $event.shiftKey)"
              >
                <AppCheckbox :model-value="selectedPaths.has(raw.rel_path)" />
              </div>

              <!-- Raw File Type Icon -->
              <div
                class="flex h-10 w-10 shrink-0 items-center justify-center rounded-xl bg-[var(--md-sys-color-surface-container-high)] text-[var(--md-sys-color-on-surface-variant)]"
              >
                <LuImage v-if="rawFileCategory(raw) === 'image'" class="h-4 w-4 text-sky-400" />
                <LuFileText v-else-if="rawFileCategory(raw) === 'sidecar'" class="h-4 w-4 text-amber-400" />
                <LuBookOpen v-else-if="rawFileCategory(raw) === 'book'" class="h-4 w-4 text-[var(--md-sys-color-primary)]" />
                <LuLayers v-else-if="rawFileCategory(raw) === 'comic'" class="h-4 w-4 text-[var(--md-sys-color-primary)]" />
                <LuDisc3 v-else-if="rawFileCategory(raw) === 'music'" class="h-4 w-4 text-[var(--md-sys-color-primary)]" />
                <LuClapperboard v-else-if="rawFileCategory(raw) === 'video'" class="h-4 w-4 text-[var(--md-sys-color-primary)]" />
                <LuFile v-else class="h-4 w-4 opacity-70" />
              </div>

              <!-- Raw File Name -->
              <div class="min-w-0 flex-1">
                <div class="flex items-center gap-2">
                  <span class="truncate font-mono text-[13px] font-medium text-[var(--md-sys-color-on-surface)]">
                    {{ raw.file_name }}
                  </span>
                  <span
                    v-if="raw.extension"
                    class="shrink-0 rounded-full bg-[var(--md-sys-color-surface-container-high)] px-2 py-0.5 font-mono text-[10px] font-semibold uppercase text-[var(--md-sys-color-on-surface-variant)]"
                  >
                    {{ raw.extension }}
                  </span>
                </div>
                <div class="mt-0.5 flex items-center gap-2 text-[11px] text-[var(--md-sys-color-on-surface-variant)] md:hidden">
                  <span class="font-mono">{{ formatBytes(raw.file_size) }}</span>
                  <span>·</span>
                  <span>{{ rawFileKindLabel(raw) }}</span>
                </div>
              </div>

              <!-- Size Column -->
              <div class="hidden w-28 shrink-0 text-right font-mono text-[11px] text-[var(--md-sys-color-on-surface-variant)] md:block">
                {{ formatBytes(raw.file_size) }}
              </div>

              <!-- Modified Date Column -->
              <div class="hidden w-24 shrink-0 text-right font-mono text-[11px] text-[var(--md-sys-color-on-surface-variant)] opacity-80 xl:block">
                {{ formatModifiedDate(raw.modified_unix_secs) }}
              </div>

              <!-- File Kind Column -->
              <div class="hidden w-32 shrink-0 items-center justify-end sm:flex">
                <span
                  class="rounded-full px-2 py-0.5 text-[10px] font-medium"
                  :class="
                    raw.is_media
                      ? 'bg-[var(--md-sys-color-primary-container)]/75 text-[var(--md-sys-color-on-primary-container)]'
                      : 'bg-[var(--md-sys-color-surface-container-high)] text-[var(--md-sys-color-on-surface-variant)]'
                  "
                >
                  {{ rawFileKindLabel(raw) }}
                </span>
              </div>

              <!-- Raw File Quick Actions -->
              <div class="flex w-24 shrink-0 items-center justify-end gap-0.5 opacity-70 transition group-hover:opacity-100">
                <button
                  v-if="mediaEntryMap.get(raw.rel_path)"
                  type="button"
                  class="md-icon-btn md-icon-btn-sm md-state"
                  title="按模板整理归档此媒体文件"
                  @click.stop="emit('openRename', [mediaEntryMap.get(raw.rel_path)!])"
                >
                  <LuFileSignature class="h-4 w-4" />
                </button>

                <button
                  type="button"
                  class="md-icon-btn md-icon-btn-sm md-state text-red-400 hover:bg-red-500/10"
                  title="仅删除此原始文件（不删除同名外挂）"
                  @click.stop="emit('requestDeleteFiles', [raw.rel_path], false)"
                >
                  <LuTrash2 class="h-4 w-4" />
                </button>
              </div>
            </div>
          </div>
        </div>
      </div>

      <!-- Empty File List (Flat / Filtered Mode) -->
      <div
        v-else-if="filteredFiles.length === 0"
        class="rounded-[24px] bg-[var(--md-sys-color-surface-container-low)] py-16 text-center"
      >
        <p class="text-sm font-semibold text-[var(--md-sys-color-on-surface)]">
          {{ hasActiveSearchOrFilter ? '当前筛选条件下没有匹配文件' : '当前目录暂无支持的媒体文件' }}
        </p>
        <p class="mt-1 text-xs text-[var(--md-sys-color-on-surface-variant)]">
          {{ hasActiveSearchOrFilter ? '尝试切换筛选标签或清空搜索关键词' : '切换至文件夹视图浏览目录，或从左侧切换其他媒体目录' }}
        </p>
      </div>

      <!-- MD3 Tonal Media List (Calm single container, subtle dividers, tonal active selection) -->
      <div v-else class="space-y-3">
        <div
          class="overflow-hidden rounded-[20px] bg-[var(--md-sys-color-surface-container-low)] divide-y divide-[var(--md-sys-color-outline-variant)]/35"
        >
          <div
            v-for="file in visibleFiles"
            :key="file.rel_path"
            class="md-state group flex cursor-pointer items-center gap-3.5 px-4 py-3"
            :class="
              inspectingPath === file.rel_path
                ? 'bg-[var(--md-sys-color-secondary-container)]/75'
                : selectedPaths.has(file.rel_path)
                  ? 'bg-[var(--md-sys-color-primary-container)]/35'
                  : ''
            "
            @click="emit('inspectFile', file)"
          >
            <div @click.stop="emit('toggleSelectFile', file.rel_path, $event.shiftKey)">
              <AppCheckbox :model-value="selectedPaths.has(file.rel_path)" />
            </div>

            <!-- Artwork Thumbnail -->
            <div
              class="relative shrink-0 overflow-hidden rounded-xl bg-[var(--md-sys-color-surface-container-high)]"
              :class="file.media_kind === 'music' ? 'h-12 w-12' : 'h-14 w-10'"
            >
              <img
                v-if="file.completeness.has_cover"
                :src="
                  buildArtworkUrl(
                    file.root_id,
                    file.rel_path,
                    file.media_kind,
                    artworkVersion,
                    file.media_kind === 'video' ? 'poster' : 'cover',
                  )
                "
                alt=""
                loading="lazy"
                class="h-full w-full object-cover"
              />
              <div
                v-else
                class="flex h-full w-full items-center justify-center text-[var(--md-sys-color-on-surface-variant)]"
              >
                <LuBookOpen v-if="file.media_kind === 'book'" class="h-4 w-4 opacity-55" />
                <LuLayers v-else-if="file.media_kind === 'comic'" class="h-4 w-4 opacity-55" />
                <LuDisc3 v-else-if="file.media_kind === 'music'" class="h-4 w-4 opacity-55" />
                <LuClapperboard v-else class="h-4 w-4 opacity-55" />
              </div>
            </div>

            <!-- Title, Subtitle & File Meta -->
            <div class="min-w-0 flex-1">
              <div class="flex items-center gap-2">
                <span class="truncate text-[13px] font-semibold text-[var(--md-sys-color-on-surface)]">
                  {{ file.display_title }}
                </span>
                <span
                  class="shrink-0 rounded-full bg-[var(--md-sys-color-surface-container-high)] px-2 py-0.5 font-mono text-[10px] font-semibold uppercase text-[var(--md-sys-color-on-surface-variant)]"
                >
                  {{ file.extension }}
                </span>
                <span
                  v-if="fileTaskStatus(file) === 'running'"
                  class="inline-flex shrink-0 items-center gap-1 rounded-full bg-[var(--md-sys-color-primary-container)] px-2 py-0.5 text-[10px] font-semibold text-[var(--md-sys-color-on-primary-container)]"
                >
                  <LuLoader2 class="h-2.5 w-2.5 animate-spin" />
                  <span>刮削中</span>
                </span>
                <span
                  v-else-if="fileTaskStatus(file) === 'queued'"
                  class="shrink-0 rounded-full bg-[var(--md-sys-color-surface-container-highest)] px-2 py-0.5 text-[10px] font-medium text-[var(--md-sys-color-on-surface-variant)]"
                >
                  排队中
                </span>
                <span
                  v-else-if="fileTaskStatus(file) === 'unmatched'"
                  class="shrink-0 rounded-full bg-amber-500/15 px-2 py-0.5 text-[10px] font-semibold text-amber-500"
                >
                  未匹配
                </span>
                <span
                  v-else-if="fileTaskStatus(file) === 'failed'"
                  class="shrink-0 rounded-full bg-[var(--md-sys-color-error-container)] px-2 py-0.5 text-[10px] font-semibold text-[var(--md-sys-color-on-error-container)]"
                >
                  失败
                </span>
                <span
                  v-if="file.year"
                  class="shrink-0 font-mono text-[11px] text-[var(--md-sys-color-on-surface-variant)]"
                >
                  {{ file.year }}
                </span>
              </div>
              <div class="mt-0.5 flex flex-wrap items-center gap-x-2 gap-y-0.5 text-[11px] text-[var(--md-sys-color-on-surface-variant)]">
                <span
                  v-if="relativeDirBadge(file.rel_path)"
                  class="inline-flex max-w-[14rem] items-center gap-1 truncate rounded-full bg-[var(--md-sys-color-surface-container-high)] px-2 py-0.5 text-[10px]"
                  :title="relativeDirBadge(file.rel_path) ?? undefined"
                >
                  <LuFolder class="h-2.5 w-2.5 shrink-0 text-[var(--md-sys-color-primary)]" />
                  <span class="truncate">{{ relativeDirBadge(file.rel_path) }}</span>
                </span>
                <span v-if="file.display_subtitle" class="truncate">
                  {{ file.display_subtitle }}
                </span>
                <span class="truncate font-mono opacity-70">{{ file.file_name }}</span>
                <span class="shrink-0 font-mono opacity-70">{{ formatBytes(file.file_size) }}</span>
              </div>
            </div>

            <!-- Subtle Completeness Pills -->
            <div class="hidden shrink-0 items-center gap-1 sm:flex">
              <span
                class="rounded-full px-2 py-0.5 text-[10px] font-medium"
                :class="
                  file.completeness.has_cover
                    ? 'bg-[var(--md-sys-color-success-container)] text-[var(--md-sys-color-on-success-container)]'
                    : 'bg-[var(--md-sys-color-warning-container)] text-[var(--md-sys-color-on-warning-container)]'
                "
              >
                {{ file.completeness.has_cover ? '有封面' : '缺封面' }}
              </span>
              <span
                class="rounded-full px-2 py-0.5 text-[10px] font-medium"
                :class="
                  file.completeness.has_summary_or_lyrics
                    ? 'bg-[var(--md-sys-color-success-container)] text-[var(--md-sys-color-on-success-container)]'
                    : 'bg-[var(--md-sys-color-surface-container-high)] text-[var(--md-sys-color-on-surface-variant)]'
                "
              >
                {{
                  file.media_kind === 'music'
                    ? file.completeness.has_summary_or_lyrics
                      ? '有歌词'
                      : '缺歌词'
                    : file.completeness.has_summary_or_lyrics
                      ? '有简介'
                      : '缺简介'
                }}
              </span>
              <span
                class="rounded-full px-2 py-0.5 text-[10px] font-medium"
                :class="
                  file.completeness.has_nfo_or_tag_block
                    ? 'bg-[var(--md-sys-color-success-container)] text-[var(--md-sys-color-on-success-container)]'
                    : 'bg-[var(--md-sys-color-surface-container-high)] text-[var(--md-sys-color-on-surface-variant)]'
                "
              >
                {{ file.completeness.has_nfo_or_tag_block ? '有标签' : '缺标签' }}
              </span>
              <span
                v-if="file.media_kind === 'book' && file.extension === 'epub' && isOrigBackupFile(file.file_name)"
                class="rounded-full bg-[var(--md-sys-color-surface-container-high)] px-2 py-0.5 text-[10px] font-medium text-[var(--md-sys-color-on-surface-variant)]"
                title="规范化保留的原始 EPUB 备份"
              >
                原件备份
              </span>
              <span
                v-else-if="file.media_kind === 'book' && file.completeness.is_normalized"
                class="rounded-full bg-[var(--md-sys-color-primary-container)] px-2 py-0.5 text-[10px] font-medium text-[var(--md-sys-color-on-primary-container)]"
                title="已完成 EPUB 3.0 规范化"
              >
                已规范化
              </span>
            </div>

            <!-- Row Quick Icon Actions (Subtle until hovered) -->
            <div class="flex shrink-0 items-center gap-0.5 opacity-65 transition group-hover:opacity-100">
              <button
                v-if="
                  file.media_kind === 'book' &&
                  (file.extension === 'txt' || file.extension === 'epub')
                "
                type="button"
                class="md-icon-btn md-icon-btn-sm md-state"
                :title="file.extension === 'txt' ? '正文编辑与转 EPUB' : '编辑目录与正文'"
                @click.stop="emit('openBookContentEditor', file)"
              >
                <LuFileText class="h-4 w-4" />
              </button>

              <button
                v-if="
                  file.media_kind === 'book' &&
                  (file.extension === 'txt' || file.extension === 'epub' || file.extension === 'pdf') &&
                  !isOrigBackupFile(file.file_name)
                "
                type="button"
                class="md-icon-btn md-icon-btn-sm md-state"
                :class="file.completeness.is_normalized ? 'text-emerald-500' : ''"
                :title="
                  file.completeness.is_normalized
                    ? '已规范化 EPUB 3.0（点击可按新参数重新规范化）'
                    : '转为规范化压缩 EPUB 3.0'
                "
                @click.stop="emit('openBookNormalizeEpub', [file])"
              >
                <LuPackageCheck class="h-4 w-4" />
              </button>

              <button
                v-if="file.media_kind === 'comic'"
                type="button"
                class="md-icon-btn md-icon-btn-sm md-state"
                title="漫画页面管理与瘦身"
                @click.stop="emit('openComicPageManager', file)"
              >
                <LuImage class="h-4 w-4" />
              </button>

              <button
                v-if="file.media_kind === 'music'"
                type="button"
                class="md-icon-btn md-icon-btn-sm md-state"
                title="音频工具箱"
                @click.stop="emit('openMusicToolbox', [file])"
              >
                <LuMusic class="h-4 w-4" />
              </button>

              <button
                v-if="file.media_kind === 'video'"
                type="button"
                class="md-icon-btn md-icon-btn-sm md-state"
                title="视频字幕与剧集工具箱"
                @click.stop="emit('openVideoToolbox', [file])"
              >
                <LuFilm class="h-4 w-4" />
              </button>

              <button
                type="button"
                class="md-icon-btn md-icon-btn-sm md-state"
                title="按模板整理归档此条目"
                @click.stop="emit('openRename', [file])"
              >
                <LuFileSignature class="h-4 w-4" />
              </button>

              <button
                type="button"
                class="md-icon-btn md-icon-btn-sm md-state"
                title="快速自动刮削此条目"
                @click.stop="emit('enqueueScrapeFiles', [file])"
              >
                <LuSparkles class="h-4 w-4 text-[var(--md-sys-color-primary)]" />
              </button>

              <button
                type="button"
                class="md-icon-btn md-icon-btn-sm md-state text-red-400 hover:bg-red-500/10"
                title="删除此条目及同名外挂文件"
                @click.stop="emit('requestDeleteFiles', [file.rel_path], true)"
              >
                <LuTrash2 class="h-4 w-4" />
              </button>
            </div>
          </div>
        </div>

        <!-- Load More Bar -->
        <div
          v-if="filteredFiles.length > visibleFiles.length"
          class="flex flex-wrap items-center justify-between gap-2 rounded-2xl bg-[var(--md-sys-color-surface-container-low)] px-4 py-2.5 text-xs text-[var(--md-sys-color-on-surface-variant)]"
        >
          <span>
            {{
              txt.batch.showingOf
                .replace('{shown}', String(visibleFiles.length))
                .replace('{total}', String(filteredFiles.length))
            }}
          </span>
          <div class="flex items-center gap-2">
            <button
              type="button"
              class="md-btn md-btn-sm md-btn-tonal md-state"
              @click="emit('loadMore')"
            >
              {{
                txt.batch.loadMore.replace(
                  '{remaining}',
                  String(filteredFiles.length - visibleFiles.length),
                )
              }}
            </button>
            <button
              type="button"
              class="md-btn md-btn-sm md-btn-text md-state"
              @click="emit('showAll')"
            >
              {{ txt.batch.showAll }}
            </button>
          </div>
        </div>
      </div>
    </template>

    <!-- MD3 Contextual Floating Action Bar (Slides up smoothly ONLY when items are checked) -->
    <Teleport to="body">
      <Transition name="md-bar-up">
        <div
          v-if="selectedPaths.size > 0"
          class="fixed bottom-5 left-1/2 z-40 flex max-w-[94vw] -translate-x-1/2 flex-wrap items-center gap-1.5 rounded-full bg-[var(--md-sys-color-surface-container-high)] py-2 pr-3 pl-2.5 shadow-[var(--md-sys-elevation-3)]"
        >
          <button
            type="button"
            class="md-icon-btn md-icon-btn-sm md-state"
            title="取消全选"
            @click="emit('clearSelection')"
          >
            <LuX class="h-4 w-4" />
          </button>

          <span class="pr-1 text-xs font-semibold text-[var(--md-sys-color-on-surface)]">
            已选 {{ selectedPaths.size }} 项
          </span>

          <div class="mx-1 h-5 w-px bg-[var(--md-sys-color-outline-variant)]" />

          <template v-if="isFolderManagerMode">
            <button
              v-if="checkedFiles.length > 0"
              type="button"
              class="md-btn md-btn-sm md-btn-tonal md-state"
              title="按命名模板整理归档所选媒体文件"
              @click="emit('openRename', checkedFiles)"
            >
              <LuFileSignature class="h-3.5 w-3.5" />
              <span>整理归档 ({{ checkedFiles.length }})</span>
            </button>

            <button
              v-if="checkedFiles.length > 0"
              type="button"
              class="md-btn md-btn-sm md-btn-tonal md-state"
              @click="emit('enqueueScrapeSelection')"
            >
              <LuSparkles class="h-3.5 w-3.5" />
              <span>自动刮削 ({{ checkedFiles.length }})</span>
            </button>

            <button
              type="button"
              class="md-btn md-btn-sm md-btn-outlined md-state !border-red-500/45 !text-red-400 hover:!bg-red-500/10"
              title="仅删除所选原始文件本身（不删除同名外挂）"
              @click="emit('requestDeleteFiles', selectedRawPaths, false)"
            >
              <LuTrash2 class="h-3.5 w-3.5" />
              <span>删除文件 ({{ selectedRawPaths.length }})</span>
            </button>
          </template>

          <template v-else>
            <button
              type="button"
              class="md-btn md-btn-sm md-btn-filled md-state"
              @click="emit('enqueueScrapeSelection')"
            >
              <LuSparkles class="h-3.5 w-3.5" />
              <span>自动刮削</span>
            </button>

            <button
              type="button"
              class="md-btn md-btn-sm md-btn-tonal md-state"
              title="在三栏刮削工作台中逐项对照与编辑已选条目"
              @click="emit('openScrapeWorkbench', checkedFiles)"
            >
              <LuColumns3 class="h-3.5 w-3.5" />
              <span>刮削工作台</span>
            </button>

            <button
              type="button"
              class="md-btn md-btn-sm md-btn-tonal md-state"
              @click="emit('openBatchMetadata')"
            >
              <LuPencil class="h-3.5 w-3.5" />
              <span>批量编辑</span>
            </button>

            <button
              type="button"
              class="md-btn md-btn-sm md-btn-outlined md-state"
              title="按元信息模板重命名并归档目录"
              @click="emit('openRename', checkedFiles)"
            >
              <LuFileSignature class="h-3.5 w-3.5" />
              <span>整理归档</span>
            </button>

            <button
              type="button"
              class="md-btn md-btn-sm md-btn-text md-state"
              @click="emit('openDryRun')"
            >
              <span>置信度预演</span>
            </button>

            <button
              v-if="selectedBookEntries.length > 0"
              type="button"
              class="md-btn md-btn-sm md-btn-text md-state"
              title="将所选 TXT / EPUB / PDF 批量转为规范化压缩 EPUB 3.0"
              @click="emit('openBookNormalizeEpub', selectedBookEntries)"
            >
              <LuPackageCheck class="h-3.5 w-3.5" />
              <span>转规范化 EPUB</span>
            </button>

            <button
              v-if="selectedComicEntries.length > 0"
              type="button"
              class="md-btn md-btn-sm md-btn-text md-state"
              @click="emit('openComicSeriesTools')"
            >
              <LuLayers class="h-3.5 w-3.5" />
              <span>漫画工具</span>
            </button>

            <button
              v-if="selectedMusicEntries.length > 0"
              type="button"
              class="md-btn md-btn-sm md-btn-text md-state"
              @click="emit('openMusicToolbox', selectedMusicEntries)"
            >
              <LuMusic class="h-3.5 w-3.5" />
              <span>音频工具</span>
            </button>

            <button
              v-if="selectedVideoEntries.length > 0"
              type="button"
              class="md-btn md-btn-sm md-btn-text md-state"
              @click="emit('openVideoToolbox', selectedVideoEntries)"
            >
              <LuFilm class="h-3.5 w-3.5" />
              <span>视频工具</span>
            </button>

            <button
              type="button"
              class="md-btn md-btn-sm md-btn-outlined md-state !border-red-500/45 !text-red-400 hover:!bg-red-500/10"
              title="删除所选媒体条目及同名外挂文件"
              @click="emit('requestDeleteFiles', checkedFiles.map((f) => f.rel_path), true)"
            >
              <LuTrash2 class="h-3.5 w-3.5" />
              <span>删除</span>
            </button>
          </template>
        </div>
      </Transition>
    </Teleport>
  </div>
</template>

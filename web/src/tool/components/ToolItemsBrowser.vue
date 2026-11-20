<script setup lang="ts">
import type { Component } from 'vue'
import {
  LuArrowDown,
  LuArrowUp,
  LuBookOpen,
  LuCheck,
  LuChevronRight,
  LuClapperboard,
  LuDisc3,
  LuFileSignature,
  LuFileText,
  LuFilm,
  LuFolder,
  LuFolderOpen,
  LuFolderPlus,
  LuImage,
  LuLayers,
  LuList,
  LuMusic,
  LuPencil,
  LuRadio,
  LuRefreshCw,
  LuSparkles,
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
import type { DirectorySnapshot, ManagedRootDto, MediaFileEntry } from '../types'
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
  clearSelection: []
  setSortField: [field: ToolSortField]
  toggleSortDir: []
  enqueueScrapeSelection: []
  openComicSeriesTools: []
  openMusicToolbox: [entries: MediaFileEntry[]]
  openVideoToolbox: [entries: MediaFileEntry[]]
  openDryRun: []
  openBatchMetadata: []
  openHistory: []
  openRename: []
  triggerRescan: []
  refreshDir: []
  inspectFile: [entry: MediaFileEntry]
  toggleSelectFile: [relPath: string, shiftKey?: boolean]
  openBookContentEditor: [entry: MediaFileEntry]
  openComicPageManager: [entry: MediaFileEntry]
  enqueueScrapeFiles: [entries: MediaFileEntry[]]
  loadMore: []
  showAll: []
}>()

const txt = useToolText()

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
      <div class="mb-4 flex flex-wrap items-center justify-between gap-2">
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
        v-if="dirError"
        class="mb-4 rounded-2xl bg-[var(--md-sys-color-error-container)] px-4 py-3 text-xs text-[var(--md-sys-color-on-error-container)]"
      >
        {{ dirError }}
      </div>

      <!-- Subfolder Tonal Cards (Folder View) -->
      <div
        v-if="
          viewMode === 'folders' &&
          !hasActiveSearchOrFilter &&
          snapshot &&
          snapshot.folders.length > 0
        "
        class="mb-4 grid grid-cols-2 gap-2.5 sm:grid-cols-3 lg:grid-cols-4"
      >
        <button
          v-for="folder in snapshot.folders"
          :key="`grid-${folder.rel_path}`"
          type="button"
          class="md-card-filled md-state flex items-center gap-3 p-3.5 text-left"
          @click="emit('enterFolder', folder.rel_path, true)"
        >
          <div
            class="flex h-10 w-10 shrink-0 items-center justify-center rounded-xl bg-[var(--md-sys-color-primary-container)] text-[var(--md-sys-color-on-primary-container)]"
          >
            <LuFolderOpen class="h-4 w-4" />
          </div>
          <div class="min-w-0 flex-1">
            <div class="truncate text-xs font-semibold text-[var(--md-sys-color-on-surface)]">
              {{ folder.name }}
            </div>
            <div class="mt-0.5 text-[11px] text-[var(--md-sys-color-on-surface-variant)]">
              {{ folder.media_file_count }} 项
              <template v-if="folder.child_dir_count > 0">
                · {{ folder.child_dir_count }} 目录
              </template>
            </div>
          </div>
        </button>
      </div>

      <!-- Loading Skeleton -->
      <div v-if="loadingDir && !snapshot" class="space-y-2">
        <div
          v-for="n in 6"
          :key="n"
          class="h-16 animate-pulse rounded-2xl bg-[var(--md-sys-color-surface-container-low)]"
        />
      </div>

      <!-- Empty File List -->
      <div
        v-else-if="filteredFiles.length === 0"
        class="rounded-[24px] bg-[var(--md-sys-color-surface-container-low)] py-16 text-center"
      >
        <p class="text-sm font-semibold text-[var(--md-sys-color-on-surface)]">
          {{ hasActiveSearchOrFilter ? '当前筛选条件下没有匹配文件' : '当前目录暂无支持的媒体文件' }}
        </p>
        <p class="mt-1 text-xs text-[var(--md-sys-color-on-surface-variant)]">
          {{ hasActiveSearchOrFilter ? '尝试切换筛选标签或清空搜索关键词' : '点击上方子文件夹或左侧切换其他目录' }}
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
                title="快速自动刮削此条目"
                @click.stop="emit('enqueueScrapeFiles', [file])"
              >
                <LuSparkles class="h-4 w-4 text-[var(--md-sys-color-primary)]" />
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
            @click="emit('openBatchMetadata')"
          >
            <LuPencil class="h-3.5 w-3.5" />
            <span>批量编辑</span>
          </button>

          <button
            type="button"
            class="md-btn md-btn-sm md-btn-outlined md-state"
            @click="emit('openRename')"
          >
            <LuFileSignature class="h-3.5 w-3.5" />
            <span>重命名</span>
          </button>

          <button
            type="button"
            class="md-btn md-btn-sm md-btn-text md-state"
            @click="emit('openDryRun')"
          >
            <span>置信度预演</span>
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
        </div>
      </Transition>
    </Teleport>
  </div>
</template>

<script setup lang="ts">
import {
  LuArrowDown,
  LuArrowUp,
  LuBookOpen,
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
  LuRadio,
  LuRefreshCw,
  LuSparkles,
  LuX,
} from 'vue-icons-plus/lu'
import AppCheckbox from '@/components/ui/AppCheckbox.vue'
import AppSelect from '@/components/ui/AppSelect.vue'
import RefreshButton from '@/components/ui/RefreshButton.vue'
import type { MediaKind } from '@/types/api'
import { buildArtworkUrl } from '../api'
import { formatBytes, useToolText } from '../i18n'
import type { DirectorySnapshot, ManagedRootDto, MediaFileEntry } from '../types'

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
  batchSummaryBanner: string | null
  snapshot: DirectorySnapshot | null
  inspectingPath: string | null
  artworkVersion: number
}>()

const emit = defineEmits<{
  openSettingsRoots: []
  enterFolder: [relPath: string, switchToItems?: boolean]
  setViewMode: [mode: 'flat' | 'folders']
  toggleSelectAll: []
  clearSelection: []
  setSortField: [field: ToolSortField]
  toggleSortDir: []
  enqueueScrapeSelection: []
  openComicSeriesTools: []
  openMusicToolbox: [entries: MediaFileEntry[]]
  openVideoToolbox: [entries: MediaFileEntry[]]
  openDryRun: []
  openRename: []
  triggerRescan: []
  refreshDir: []
  dismissBanner: []
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
  if (cur && parent.startsWith(`${cur}/`) ) {
    return parent.slice(cur.length + 1)
  }
  return parent
}
</script>

<template>
  <div class="p-4 sm:p-6">
  <!-- Domain Empty State -->
  <div
    v-if="domainRoots.length === 0"
    class="mx-auto mt-12 max-w-lg rounded-3xl border border-dashed border-[var(--ctp-surface1)] bg-[var(--ctp-mantle)]/45 p-8 text-center"
  >
    <div
      class="mx-auto mb-4 flex h-12 w-12 items-center justify-center rounded-2xl bg-[var(--ctp-accent-soft)] text-[var(--ctp-accent)]"
    >
      <LuFolderPlus class="h-6 w-6" />
    </div>
    <h2 class="text-base font-bold text-[var(--ctp-text)]">
      {{ domainLabels.emptyRootsTitle }}
    </h2>
    <p class="mt-2 text-xs leading-relaxed text-[var(--ctp-subtext0)]">
      {{ domainLabels.emptyRootsDesc }}
    </p>
    <button
      type="button"
      class="mt-5 inline-flex items-center gap-2 rounded-xl bg-[var(--ctp-accent)] px-5 py-2.5 text-xs font-semibold text-[var(--ctp-on-accent)] shadow-sm transition hover:opacity-95"
      @click="emit('openSettingsRoots')"
    >
      <LuFolderPlus class="h-4 w-4" />
      <span>{{ txt.roots.add }}</span>
    </button>
  </div>

  <template v-else>
    <!-- Breadcrumb + Batch Action Toolbar -->
    <div
      class="mb-4 flex flex-wrap items-center justify-between gap-3 rounded-2xl border border-[var(--ctp-surface0)]/75 bg-[var(--ctp-mantle)]/45 px-4 py-2.5"
    >
      <div class="flex min-w-0 flex-wrap items-center gap-1 text-xs">
        <template v-for="(bc, idx) in breadcrumbs" :key="bc.relPath || '__root__'">
          <LuChevronRight
            v-if="idx > 0"
            class="h-3.5 w-3.5 shrink-0 text-[var(--ctp-subtext0)]"
          />
          <button
            type="button"
            class="rounded-lg px-2 py-1 font-medium transition hover:bg-[var(--ctp-surface0)]/65"
            :class="
              idx === breadcrumbs.length - 1
                ? 'font-bold text-[var(--ctp-text)]'
                : 'text-[var(--ctp-subtext1)]'
            "
            @click="emit('enterFolder', bc.relPath)"
          >
            {{ bc.label }}
          </button>
        </template>
      </div>

      <div class="flex flex-wrap items-center gap-2">
        <!-- View Mode Switcher: Flat All-Media vs Folder Hierarchy -->
        <div
          class="flex items-center gap-0.5 rounded-xl border border-[var(--ctp-surface0)]/80 bg-[var(--ctp-canvas)] p-0.5"
        >
          <button
            type="button"
            class="inline-flex items-center gap-1 rounded-lg px-2.5 py-1 text-[11px] font-semibold transition"
            :class="
              viewMode === 'flat'
                ? 'bg-[var(--ctp-accent)] text-[var(--ctp-on-accent)]'
                : 'text-[var(--ctp-subtext1)] hover:text-[var(--ctp-text)]'
            "
            title="平铺展示当前目录及所有子目录内的媒体文件"
            @click="emit('setViewMode', 'flat')"
          >
            <LuList class="h-3.5 w-3.5" />
            <span>全部媒体</span>
          </button>
          <button
            type="button"
            class="inline-flex items-center gap-1 rounded-lg px-2.5 py-1 text-[11px] font-semibold transition"
            :class="
              viewMode === 'folders'
                ? 'bg-[var(--ctp-accent)] text-[var(--ctp-on-accent)]'
                : 'text-[var(--ctp-subtext1)] hover:text-[var(--ctp-text)]'
            "
            title="按文件夹目录层级逐级浏览"
            @click="emit('setViewMode', 'folders')"
          >
            <LuFolder class="h-3.5 w-3.5" />
            <span>文件夹</span>
          </button>
        </div>

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
          v-if="selectedPaths.size > 0"
          type="button"
          class="rounded-xl border border-[var(--ctp-surface0)] bg-[var(--ctp-canvas)] px-2.5 py-1 text-[11px] font-medium text-[var(--ctp-subtext1)] transition hover:border-[var(--ctp-accent)] hover:text-[var(--ctp-text)]"
          @click="emit('clearSelection')"
        >
          {{ txt.actions.clearSelect }}
        </button>

        <!-- Sort Controls -->
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
            class="flex h-8 w-8 items-center justify-center rounded-xl border border-[var(--ctp-surface0)] bg-[var(--ctp-canvas)] text-[var(--ctp-subtext1)] transition hover:border-[var(--ctp-accent)] hover:text-[var(--ctp-text)]"
            :title="sortDir === 'asc' ? txt.sort.asc : txt.sort.desc"
            @click="emit('toggleSortDir')"
          >
            <LuArrowUp v-if="sortDir === 'asc'" class="h-3.5 w-3.5" />
            <LuArrowDown v-else class="h-3.5 w-3.5" />
          </button>
        </div>

        <!-- Scrape Button: Adds Background Scrape Task (Local -> High Priority -> Low Priority) -->
        <button
          type="button"
          :disabled="filteredFiles.length === 0"
          class="inline-flex items-center gap-1.5 rounded-xl bg-[var(--ctp-accent-soft)] px-3 py-1.5 text-xs font-semibold text-[var(--ctp-accent)] transition hover:opacity-90 disabled:opacity-45"
          title="添加后台刮削任务（按 本地已有非空信息 → 高优先级源 → 低优先级源 自动补齐）"
          @click="emit('enqueueScrapeSelection')"
        >
          <LuSparkles class="h-3.5 w-3.5" />
          <span>
            {{
              checkedFiles.length > 0
                ? `${txt.actions.scrape} (${checkedFiles.length})`
                : txt.batch.scrapeAllLabel.replace('{count}', String(filteredFiles.length))
            }}
          </span>
        </button>

        <button
          v-if="selectedComicEntries.length > 0"
          type="button"
          class="inline-flex items-center gap-1.5 rounded-xl border border-[var(--ctp-surface0)] bg-[var(--ctp-canvas)] px-3 py-1.5 text-xs font-semibold text-[var(--ctp-text)] transition hover:border-[var(--ctp-accent)]"
          @click="emit('openComicSeriesTools')"
        >
          <LuLayers class="h-3.5 w-3.5 text-[var(--ctp-accent)]" />
          <span>{{ txt.batch.comicSeriesTools }} ({{ selectedComicEntries.length }})</span>
        </button>

        <button
          v-if="selectedMusicEntries.length > 0"
          type="button"
          class="inline-flex items-center gap-1.5 rounded-xl border border-[var(--ctp-surface0)] bg-[var(--ctp-canvas)] px-3 py-1.5 text-xs font-semibold text-[var(--ctp-text)] transition hover:border-[var(--ctp-accent)]"
          @click="emit('openMusicToolbox', selectedMusicEntries)"
        >
          <LuMusic class="h-3.5 w-3.5 text-[var(--ctp-accent)]" />
          <span>{{ txt.batch.musicTools }} ({{ selectedMusicEntries.length }})</span>
        </button>

        <button
          v-if="selectedVideoEntries.length > 0"
          type="button"
          class="inline-flex items-center gap-1.5 rounded-xl border border-[var(--ctp-surface0)] bg-[var(--ctp-canvas)] px-3 py-1.5 text-xs font-semibold text-[var(--ctp-text)] transition hover:border-[var(--ctp-accent)]"
          @click="emit('openVideoToolbox', selectedVideoEntries)"
        >
          <LuFilm class="h-3.5 w-3.5 text-[var(--ctp-accent)]" />
          <span>{{ txt.batch.videoTools }} ({{ selectedVideoEntries.length }})</span>
        </button>

        <button
          v-if="selectedEntriesForRename.length > 0"
          type="button"
          class="inline-flex items-center gap-1.5 rounded-xl border border-[var(--ctp-surface0)] bg-[var(--ctp-canvas)] px-3 py-1.5 text-xs font-semibold text-[var(--ctp-text)] transition hover:border-[var(--ctp-accent)]"
          title="只读预检拟匹配结果与置信度，低置信度项自动标黄供人工确认"
          @click="emit('openDryRun')"
        >
          <LuSparkles class="h-3.5 w-3.5 text-amber-500" />
          <span>{{ txt.batch.dryRunScrape }} ({{ selectedEntriesForRename.length }})</span>
        </button>

        <button
          type="button"
          :disabled="selectedEntriesForRename.length === 0"
          class="inline-flex items-center gap-1.5 rounded-xl border border-[var(--ctp-surface0)] bg-[var(--ctp-canvas)] px-3 py-1.5 text-xs font-semibold text-[var(--ctp-text)] transition hover:border-[var(--ctp-accent)] disabled:opacity-45"
          @click="emit('openRename')"
        >
          <LuFileSignature class="h-3.5 w-3.5 text-[var(--ctp-accent)]" />
          <span>{{ txt.actions.batchRename }}</span>
        </button>

        <button
          v-if="webhookEnabled"
          type="button"
          :disabled="notifyingRescan"
          class="inline-flex items-center gap-1.5 rounded-xl border border-[var(--ctp-surface0)] bg-[var(--ctp-canvas)] px-3 py-1.5 text-xs font-semibold text-[var(--ctp-text)] transition hover:border-[var(--ctp-accent)] disabled:opacity-50"
          :title="txt.batch.triggerRescan"
          @click="emit('triggerRescan')"
        >
          <LuRefreshCw v-if="notifyingRescan" class="h-3.5 w-3.5 animate-spin text-[var(--ctp-accent)]" />
          <LuRadio v-else class="h-3.5 w-3.5 text-[var(--ctp-accent)]" />
          <span>{{ notifyingRescan ? txt.batch.rescanning : txt.batch.triggerRescan }}</span>
        </button>

        <RefreshButton
          :action="() => emit('refreshDir')"
          :busy="loadingDir"
          :title="txt.actions.refresh"
        />
      </div>
    </div>

    <div
      v-if="batchSummaryBanner"
      class="mb-4 flex items-center justify-between gap-2 rounded-2xl border border-emerald-500/35 bg-emerald-500/10 px-4 py-2.5 text-xs font-medium text-emerald-500"
    >
      <span>{{ batchSummaryBanner }}</span>
      <button
        type="button"
        class="rounded-lg p-1 hover:bg-emerald-500/15"
        @click="emit('dismissBanner')"
      >
        <LuX class="h-3.5 w-3.5" />
      </button>
    </div>

    <div
      v-if="dirError"
      class="mb-4 rounded-2xl border border-red-500/35 bg-red-500/10 px-4 py-3 text-xs text-red-400"
    >
      {{ dirError }}
    </div>

    <!-- Subfolder Cards (Folder View) -->
    <div
      v-if="viewMode === 'folders' && !hasActiveSearchOrFilter && snapshot && snapshot.folders.length > 0"
      class="mb-4 grid grid-cols-2 gap-2.5 sm:grid-cols-3 lg:grid-cols-4"
    >
      <button
        v-for="folder in snapshot.folders"
        :key="`grid-${folder.rel_path}`"
        type="button"
        class="card-soft flex items-center gap-3 rounded-2xl bg-[var(--ctp-mantle)]/45 p-3 text-left transition hover:border-[var(--ctp-accent)]"
        @click="emit('enterFolder', folder.rel_path, true)"
      >
        <div
          class="flex h-9 w-9 shrink-0 items-center justify-center rounded-xl bg-[var(--ctp-accent-soft)] text-[var(--ctp-accent)]"
        >
          <LuFolderOpen class="h-4 w-4" />
        </div>
        <div class="min-w-0 flex-1">
          <div class="truncate text-xs font-semibold text-[var(--ctp-text)]">
            {{ folder.name }}
          </div>
          <div class="mt-0.5 text-[11px] text-[var(--ctp-subtext0)]">
            {{ folder.media_file_count }} 个媒体文件
          </div>
        </div>
      </button>
    </div>

    <!-- Media Files List -->
    <div
      v-if="loadingDir && !snapshot"
      class="py-16 text-center text-xs text-[var(--ctp-subtext0)]"
    >
      正在读取当前领域本地目录结构…
    </div>

    <div
      v-else-if="filteredFiles.length === 0"
      class="rounded-3xl border border-dashed border-[var(--ctp-surface1)] py-16 text-center text-xs text-[var(--ctp-subtext0)]"
    >
      当前目录与筛选条件下暂无匹配的{{ txt.domains[activeDomain] }}文件
    </div>

    <div v-else class="space-y-2">
      <div
        v-for="file in visibleFiles"
        :key="file.rel_path"
        class="group flex cursor-pointer items-center gap-3 rounded-2xl border px-3.5 py-2.5 transition"
        :class="
          inspectingPath === file.rel_path
            ? 'border-[var(--ctp-accent)] bg-[var(--ctp-accent-soft)]/55'
            : 'border-[var(--ctp-surface0)]/75 bg-[var(--ctp-mantle)]/35 hover:border-[var(--ctp-surface1)] hover:bg-[var(--ctp-mantle)]/65'
        "
        @click="emit('inspectFile', file)"
      >
        <div
          @click.stop.prevent="emit('toggleSelectFile', file.rel_path, $event.shiftKey)"
        >
          <AppCheckbox
            :model-value="selectedPaths.has(file.rel_path)"
            class="pointer-events-none"
          />
        </div>

        <!-- Artwork Thumbnail -->
        <div
          class="relative flex h-12 w-12 shrink-0 items-center justify-center overflow-hidden rounded-xl border border-[var(--ctp-surface0)] bg-[var(--ctp-canvas)]"
        >
          <img
            v-if="file.completeness.has_cover"
            :src="buildArtworkUrl(file.root_id, file.rel_path, file.media_kind, artworkVersion)"
            :alt="file.display_title"
            loading="lazy"
            class="h-full w-full object-cover"
          />
          <LuBookOpen
            v-else-if="file.media_kind === 'book'"
            class="h-5 w-5 text-[var(--ctp-subtext0)]"
          />
          <LuLayers
            v-else-if="file.media_kind === 'comic'"
            class="h-5 w-5 text-[var(--ctp-subtext0)]"
          />
          <LuDisc3
            v-else-if="file.media_kind === 'music'"
            class="h-5 w-5 text-[var(--ctp-subtext0)]"
          />
          <LuClapperboard v-else class="h-5 w-5 text-[var(--ctp-subtext0)]" />
        </div>

        <!-- Title + Subtitle + Folder Badge -->
        <div class="min-w-0 flex-1">
          <div class="flex items-center gap-2">
            <span class="truncate text-xs font-bold text-[var(--ctp-text)]">
              {{ file.display_title }}
            </span>
            <span
              v-if="file.year"
              class="shrink-0 font-mono text-[11px] text-[var(--ctp-subtext0)]"
            >
              ({{ file.year }})
            </span>
            <span
              class="shrink-0 rounded-md bg-[var(--ctp-surface0)]/80 px-1.5 py-0.5 font-mono text-[10px] font-semibold uppercase text-[var(--ctp-subtext1)]"
            >
              {{ file.extension }}
            </span>
          </div>
          <div class="mt-0.5 flex items-center gap-2 text-[11px] text-[var(--ctp-subtext0)]">
            <span class="truncate">{{ file.display_subtitle }}</span>
            <span>·</span>
            <span
              v-if="relativeDirBadge(file.rel_path)"
              class="inline-flex max-w-[14rem] shrink-0 items-center gap-1 truncate rounded-md bg-[var(--ctp-surface0)]/65 px-1.5 py-0.5 font-mono text-[10px] text-[var(--ctp-subtext1)]"
            >
              <LuFolder class="h-2.5 w-2.5 shrink-0 text-[var(--ctp-accent)]" />
              <span class="truncate">{{ relativeDirBadge(file.rel_path) }}</span>
            </span>
            <span class="truncate font-mono text-[10px] opacity-80">
              {{ file.file_name }}
            </span>
          </div>
        </div>

        <!-- Completeness Badges -->
        <div class="hidden shrink-0 items-center gap-1.5 sm:flex">
          <span
            class="rounded-full px-2 py-0.5 text-[10px] font-medium"
            :class="
              file.completeness.has_cover
                ? 'bg-emerald-500/15 text-emerald-500'
                : 'bg-amber-500/15 text-amber-500'
            "
          >
            {{ file.completeness.has_cover ? '封面✓' : '缺封面' }}
          </span>
          <span
            class="rounded-full px-2 py-0.5 text-[10px] font-medium"
            :class="
              file.completeness.has_summary_or_lyrics
                ? 'bg-emerald-500/15 text-emerald-500'
                : 'bg-[var(--ctp-surface0)]/80 text-[var(--ctp-subtext0)]'
            "
          >
            {{
              file.media_kind === 'music'
                ? file.completeness.has_summary_or_lyrics
                  ? '歌词✓'
                  : '无歌词'
                : file.completeness.has_summary_or_lyrics
                  ? '简介✓'
                  : '无简介'
            }}
          </span>
          <span
            class="rounded-full px-2 py-0.5 text-[10px] font-medium"
            :class="
              file.completeness.has_nfo_or_tag_block
                ? 'bg-emerald-500/15 text-emerald-500'
                : 'bg-[var(--ctp-surface0)]/80 text-[var(--ctp-subtext0)]'
            "
          >
            {{
              file.media_kind === 'video'
                ? file.completeness.has_nfo_or_tag_block
                  ? 'NFO✓'
                  : '无NFO'
                : file.media_kind === 'comic'
                  ? file.completeness.has_nfo_or_tag_block
                    ? 'ComicInfo✓'
                    : '无XML'
                  : file.completeness.has_nfo_or_tag_block
                    ? '元数据✓'
                    : '待完善'
            }}
          </span>
        </div>

        <div class="hidden w-16 shrink-0 text-right font-mono text-[11px] text-[var(--ctp-subtext0)] lg:block">
          {{ formatBytes(file.file_size) }}
        </div>

        <button
          v-if="
            file.media_kind === 'book' &&
            (file.extension === 'txt' || file.extension === 'epub')
          "
          type="button"
          class="shrink-0 rounded-xl p-2 text-[var(--ctp-subtext0)] opacity-80 transition hover:bg-[var(--ctp-accent-soft)] hover:text-[var(--ctp-accent)]"
          :title="
            file.extension === 'txt'
              ? '编辑目录与正文 / 转为 EPUB'
              : '编辑目录与正文'
          "
          @click.stop="emit('openBookContentEditor', file)"
        >
          <LuFileText class="h-4 w-4" />
        </button>

        <button
          v-if="file.media_kind === 'comic'"
          type="button"
          class="shrink-0 rounded-xl p-2 text-[var(--ctp-subtext0)] opacity-80 transition hover:bg-[var(--ctp-accent-soft)] hover:text-[var(--ctp-accent)]"
          title="漫画页面管理与体积瘦身"
          @click.stop="emit('openComicPageManager', file)"
        >
          <LuImage class="h-4 w-4" />
        </button>

        <button
          v-if="file.media_kind === 'music'"
          type="button"
          class="shrink-0 rounded-xl p-2 text-[var(--ctp-subtext0)] opacity-80 transition hover:bg-[var(--ctp-accent-soft)] hover:text-[var(--ctp-accent)]"
          title="歌词精校 / 响度 / 封面 / CUE 工具箱"
          @click.stop="emit('openMusicToolbox', [file])"
        >
          <LuMusic class="h-4 w-4" />
        </button>

        <button
          v-if="file.media_kind === 'video'"
          type="button"
          class="shrink-0 rounded-xl p-2 text-[var(--ctp-subtext0)] opacity-80 transition hover:bg-[var(--ctp-accent-soft)] hover:text-[var(--ctp-accent)]"
          title="字幕精校 / 内封字幕抽取 / 剧集映射工具箱"
          @click.stop="emit('openVideoToolbox', [file])"
        >
          <LuFilm class="h-4 w-4" />
        </button>

        <button
          type="button"
          class="shrink-0 rounded-xl p-2 text-[var(--ctp-subtext0)] opacity-80 transition hover:bg-[var(--ctp-accent-soft)] hover:text-[var(--ctp-accent)]"
          title="添加刮削任务（本地信息 → 高优先级源 → 低优先级源）"
          @click.stop="emit('enqueueScrapeFiles', [file])"
        >
          <LuSparkles class="h-4 w-4" />
        </button>
      </div>

      <!-- Incremental Load More Bar for Large Directories -->
      <div
        v-if="filteredFiles.length > visibleFiles.length"
        class="flex flex-wrap items-center justify-between gap-2 rounded-2xl border border-[var(--ctp-surface0)]/70 bg-[var(--ctp-mantle)]/40 px-4 py-2.5 text-xs text-[var(--ctp-subtext0)]"
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
            class="rounded-xl border border-[var(--ctp-surface0)] bg-[var(--ctp-canvas)] px-3 py-1 font-semibold text-[var(--ctp-text)] transition hover:border-[var(--ctp-accent)]"
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
            class="rounded-xl px-2.5 py-1 font-medium text-[var(--ctp-subtext1)] transition hover:text-[var(--ctp-text)]"
            @click="emit('showAll')"
          >
            {{ txt.batch.showAll }}
          </button>
        </div>
      </div>
    </div>
  </template>
  </div>
</template>

<script setup lang="ts">
import {
  LuColumns3,
  LuFileSignature,
  LuFilm,
  LuLayers,
  LuMusic,
  LuPackageCheck,
  LuPencil,
  LuSlidersHorizontal,
  LuSparkles,
  LuTrash2,
  LuX,
} from 'vue-icons-plus/lu'
import type { MediaFileEntry } from '../../types'

defineProps<{
  selectedCount: number
  isFolderManagerMode: boolean
  folderSelectionSummary: string
  checkedFiles: MediaFileEntry[]
  unorganizedCheckedFiles: MediaFileEntry[]
  selectedBookEntries: MediaFileEntry[]
  selectedComicEntries: MediaFileEntry[]
  selectedMusicEntries: MediaFileEntry[]
  selectedVideoEntries: MediaFileEntry[]
  selectedRawPaths: string[]
  selectedFolderCount: number
  checkedSelectionKeys: string[]
  lastScrapeSummary: string
}>()

const emit = defineEmits<{
  clearSelection: []
  quickScrapeSelection: []
  configureScrapeSelection: []
  openScrapeWorkbench: [entries: MediaFileEntry[]]
  openBatchMetadata: []
  openRename: [entries: MediaFileEntry[]]
  openDryRun: []
  openBookNormalizeEpub: [entries: MediaFileEntry[]]
  openComicSeriesTools: []
  openMusicToolbox: [entries: MediaFileEntry[]]
  openVideoToolbox: [entries: MediaFileEntry[]]
  requestDeleteFiles: [paths: string[], deleteCompanions: boolean]
}>()
</script>

<template>
  <Teleport to="body">
    <Transition name="md-bar-up">
      <div
        v-if="selectedCount > 0"
        class="fixed bottom-[calc(4.75rem+env(safe-area-inset-bottom,0px))] left-1/2 z-40 flex max-w-[94vw] -translate-x-1/2 flex-wrap items-center gap-1.5 rounded-full bg-[var(--md-sys-color-surface-container-high)] py-2 pr-3 pl-2.5 shadow-[var(--md-sys-elevation-3)] md:bottom-5"
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
          {{ isFolderManagerMode ? folderSelectionSummary : `已选 ${selectedCount} 项` }}
        </span>

        <div class="mx-1 h-5 w-px bg-[var(--md-sys-color-outline-variant)]" />

        <template v-if="checkedFiles.length > 0">
          <div
            class="inline-flex items-center overflow-hidden rounded-full bg-[var(--md-sys-color-primary)] text-[var(--md-sys-color-on-primary)]"
          >
            <button
              type="button"
              class="md-state inline-flex h-8 items-center gap-1.5 pr-2.5 pl-3.5 text-xs font-semibold"
              :title="`按上次策略一键快刮 (${lastScrapeSummary})`"
              @click="emit('quickScrapeSelection')"
            >
              <LuSparkles class="h-3.5 w-3.5" />
              <span>
                {{ isFolderManagerMode ? `按上次策略快刮 (${checkedFiles.length})` : '按上次策略快刮' }}
              </span>
            </button>
            <button
              type="button"
              class="md-state inline-flex h-8 items-center border-l border-[var(--md-sys-color-on-primary)]/25 px-2.5 text-xs"
              :title="`配置自动刮削策略（当前：${lastScrapeSummary}）`"
              @click="emit('configureScrapeSelection')"
            >
              <LuSlidersHorizontal class="h-3.5 w-3.5" />
            </button>
          </div>

          <button
            type="button"
            class="md-btn md-btn-sm md-btn-tonal md-state"
            title="在三栏刮削工作台中逐项对照与编辑所选媒体条目"
            @click="emit('openScrapeWorkbench', checkedFiles)"
          >
            <LuColumns3 class="h-3.5 w-3.5" />
            <span>刮削工作台</span>
          </button>

          <button
            type="button"
            class="md-btn md-btn-sm md-btn-tonal md-state"
            title="批量修改所选媒体条目的元信息"
            @click="emit('openBatchMetadata')"
          >
            <LuPencil class="h-3.5 w-3.5" />
            <span>批量编辑</span>
          </button>

          <button
            type="button"
            :disabled="unorganizedCheckedFiles.length === 0"
            class="md-btn md-btn-sm md-btn-outlined md-state disabled:cursor-not-allowed disabled:opacity-45"
            :title="
              unorganizedCheckedFiles.length === 0
                ? '所选条目均已按模板整理归档，无需重复整理'
                : unorganizedCheckedFiles.length < checkedFiles.length
                  ? `按命名模板整理归档（将自动跳过 ${checkedFiles.length - unorganizedCheckedFiles.length} 项已归档条目）`
                  : '按命名模板整理归档所选媒体条目'
            "
            @click="emit('openRename', checkedFiles)"
          >
            <LuFileSignature class="h-3.5 w-3.5" />
            <span>
              {{
                unorganizedCheckedFiles.length > 0 && unorganizedCheckedFiles.length < checkedFiles.length
                  ? `整理归档 (${unorganizedCheckedFiles.length})`
                  : '整理归档'
              }}
            </span>
          </button>

          <button
            v-if="!isFolderManagerMode"
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
        </template>

        <button
          type="button"
          class="md-btn md-btn-sm md-btn-outlined md-state !border-[var(--md-sys-color-error)]/45 !text-[var(--md-sys-color-error)] hover:!bg-[var(--md-sys-color-error-container)]/40"
          :title="
            isFolderManagerMode
              ? selectedFolderCount > 0
                ? '永久删除所选文件夹（含内部全部文件）及所选原始文件'
                : '仅删除所选原始文件本身（不删除同名外挂）'
              : '删除所选媒体条目及同名外挂文件'
          "
          @click="
            isFolderManagerMode
              ? emit('requestDeleteFiles', selectedRawPaths, false)
              : emit('requestDeleteFiles', checkedSelectionKeys, true)
          "
        >
          <LuTrash2 class="h-3.5 w-3.5" />
          <span>
            {{
              isFolderManagerMode
                ? selectedFolderCount > 0
                  ? `删除所选 (${selectedRawPaths.length})`
                  : `删除文件 (${selectedRawPaths.length})`
                : '删除'
            }}
          </span>
        </button>
      </div>
    </Transition>
  </Teleport>
</template>

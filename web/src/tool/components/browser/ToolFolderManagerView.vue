<script setup lang="ts">
import { computed } from 'vue'
import {
  LuBookOpen,
  LuChevronRight,
  LuClapperboard,
  LuDisc3,
  LuFile,
  LuFileSignature,
  LuFileText,
  LuFolder,
  LuHardDrive,
  LuImage,
  LuLayers,
  LuTrash2,
} from 'vue-icons-plus/lu'
import AppCheckbox from '@/components/ui/AppCheckbox.vue'
import { formatBytes } from '../../i18n'
import type {
  DirectorySnapshot,
  FolderEntryDto,
  ManagedRootDto,
  MediaFileEntry,
  RawFileEntryDto,
} from '../../types'

const props = defineProps<{
  isMultiRootSummary: boolean
  domainRoots: ManagedRootDto[]
  rootFileCounts: Record<string, number>
  snapshot: DirectorySnapshot | null
  sortedFolders: FolderEntryDto[]
  sortedRawFiles: RawFileEntryDto[]
  mediaEntryMap: Map<string, MediaFileEntry>
  folderModeSelectablePaths: string[]
  allFolderItemsSelected: boolean
  selectedPaths: Set<string>
  inspectingPath: string | null
}>()

const emit = defineEmits<{
  selectRoot: [rootId: string, preferredViewMode?: 'flat' | 'folders']
  enterFolder: [relPath: string, switchToItems?: boolean]
  toggleSelectAllRaw: [paths: string[]]
  toggleSelectFile: [relPath: string, shiftKey?: boolean, orderedPaths?: string[]]
  inspectFile: [entry: MediaFileEntry]
  openRename: [entries?: MediaFileEntry[]]
  requestDeleteFiles: [paths: string[], deleteCompanions: boolean]
}>()

const isEmptyFolder = computed(
  () => props.sortedFolders.length === 0 && props.sortedRawFiles.length === 0,
)

function isOrigBackupFile(fileName: string): boolean {
  return /\.orig(?:\.[^./]+)?\.epub$/i.test(fileName)
}

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
  const media = props.mediaEntryMap.get(raw.rel_path)
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
</script>

<template>
  <div class="space-y-3">
    <!-- Multi-root Summary: list mounted directories so user can drill into any specific mount -->
    <div
      v-if="isMultiRootSummary"
      class="divide-y divide-[var(--md-sys-color-outline-variant)]/35 overflow-hidden rounded-[20px] bg-[var(--md-sys-color-surface-container-low)]"
    >
      <button
        v-for="r in domainRoots"
        :key="`mount-dir-${r.id}`"
        type="button"
        class="md-state group flex w-full items-center gap-3.5 px-4 py-3 text-left"
        @click="emit('selectRoot', r.id, 'folders')"
      >
        <div
          class="flex h-10 w-10 shrink-0 items-center justify-center rounded-xl bg-[var(--md-sys-color-primary-container)]/85 text-[var(--md-sys-color-on-primary-container)]"
        >
          <LuHardDrive class="h-4 w-4" />
        </div>
        <div class="min-w-0 flex-1">
          <div class="flex items-center gap-2">
            <span class="truncate text-[13px] font-semibold text-[var(--md-sys-color-on-surface)]">
              {{ r.name }}
            </span>
            <span
              class="shrink-0 rounded-full bg-[var(--md-sys-color-surface-container-high)] px-2 py-0.5 text-[10px] font-medium text-[var(--md-sys-color-on-surface-variant)]"
            >
              挂载目录
            </span>
          </div>
          <div class="mt-0.5 truncate font-mono text-[11px] text-[var(--md-sys-color-on-surface-variant)]">
            {{ r.path }}
          </div>
        </div>
        <div class="hidden shrink-0 font-mono text-xs text-[var(--md-sys-color-on-surface-variant)] sm:block">
          {{ rootFileCounts[r.id] ?? 0 }} 项媒体
        </div>
        <LuChevronRight
          class="h-4 w-4 shrink-0 text-[var(--md-sys-color-on-surface-variant)] opacity-60 transition group-hover:translate-x-0.5 group-hover:opacity-100"
        />
      </button>
    </div>

    <div
      v-else-if="isEmptyFolder"
      class="rounded-[24px] bg-[var(--md-sys-color-surface-container-low)] py-16 text-center"
    >
      <p class="text-sm font-semibold text-[var(--md-sys-color-on-surface)]">
        当前文件夹暂无内容
      </p>
      <p class="mt-1 text-xs text-[var(--md-sys-color-on-surface-variant)]">
        {{
          snapshot && snapshot.parent_rel_path !== null
            ? '点击上方路径栏可返回上级目录，或从左侧切换其他媒体目录'
            : '请从左侧切换其他媒体目录，或先往该目录添加媒体文件'
        }}
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
            v-if="folderModeSelectablePaths.length > 0"
            :model-value="allFolderItemsSelected"
            @change="emit('toggleSelectAllRaw', folderModeSelectablePaths)"
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
        <!-- Subfolder Rows -->
        <div
          v-for="folder in sortedFolders"
          :key="`fm-dir-${folder.rel_path}`"
          class="md-state group flex w-full cursor-pointer items-center gap-3.5 px-4 py-2.5 text-left"
          :class="
            selectedPaths.has(folder.rel_path)
              ? 'bg-[var(--md-sys-color-primary-container)]/35'
              : ''
          "
          @click="emit('enterFolder', folder.rel_path, true)"
        >
          <div
            class="flex w-5 shrink-0 items-center justify-center"
            @click.stop="
              emit('toggleSelectFile', folder.rel_path, $event.shiftKey, folderModeSelectablePaths)
            "
          >
            <AppCheckbox :model-value="selectedPaths.has(folder.rel_path)" />
          </div>
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
          <div
            class="hidden w-28 shrink-0 text-right font-mono text-[11px] text-[var(--md-sys-color-on-surface-variant)] md:block"
          >
            <template v-if="folder.media_file_count > 0 || folder.child_dir_count > 0">
              <span>{{ folder.media_file_count }} 项</span>
              <span v-if="folder.child_dir_count > 0" class="opacity-75">
                / {{ folder.child_dir_count }} 目录
              </span>
            </template>
            <span v-else class="opacity-60">空目录</span>
          </div>
          <div
            class="hidden w-24 shrink-0 text-right font-mono text-[11px] text-[var(--md-sys-color-on-surface-variant)] opacity-60 xl:block"
          >
            —
          </div>
          <div class="hidden w-32 shrink-0 justify-end sm:flex">
            <span
              class="rounded-full bg-[var(--md-sys-color-surface-container-high)] px-2 py-0.5 text-[10px] text-[var(--md-sys-color-on-surface-variant)]"
            >
              文件夹
            </span>
          </div>
          <div
            class="flex w-24 shrink-0 items-center justify-end gap-0.5 text-[var(--md-sys-color-on-surface-variant)]"
          >
            <button
              type="button"
              class="md-icon-btn md-icon-btn-sm md-state text-[var(--md-sys-color-error)] opacity-0 transition group-hover:opacity-100 hover:bg-[var(--md-sys-color-error-container)]/50"
              title="删除整个文件夹及其内部所有文件"
              @click.stop="emit('requestDeleteFiles', [folder.rel_path], false)"
            >
              <LuTrash2 class="h-4 w-4" />
            </button>
            <LuChevronRight
              class="h-4 w-4 opacity-60 transition group-hover:translate-x-0.5 group-hover:opacity-100"
            />
          </div>
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
            @click.stop="
              emit('toggleSelectFile', raw.rel_path, $event.shiftKey, folderModeSelectablePaths)
            "
          >
            <AppCheckbox :model-value="selectedPaths.has(raw.rel_path)" />
          </div>

          <div
            class="flex h-10 w-10 shrink-0 items-center justify-center rounded-xl bg-[var(--md-sys-color-surface-container-high)] text-[var(--md-sys-color-on-surface-variant)]"
          >
            <LuImage v-if="rawFileCategory(raw) === 'image'" class="h-4 w-4 text-[var(--md-sys-color-primary)]" />
            <LuFileText
              v-else-if="rawFileCategory(raw) === 'sidecar'"
              class="h-4 w-4 text-[var(--md-sys-color-warning)]"
            />
            <LuBookOpen
              v-else-if="rawFileCategory(raw) === 'book'"
              class="h-4 w-4 text-[var(--md-sys-color-primary)]"
            />
            <LuLayers
              v-else-if="rawFileCategory(raw) === 'comic'"
              class="h-4 w-4 text-[var(--md-sys-color-primary)]"
            />
            <LuDisc3
              v-else-if="rawFileCategory(raw) === 'music'"
              class="h-4 w-4 text-[var(--md-sys-color-primary)]"
            />
            <LuClapperboard
              v-else-if="rawFileCategory(raw) === 'video'"
              class="h-4 w-4 text-[var(--md-sys-color-primary)]"
            />
            <LuFile v-else class="h-4 w-4 opacity-70" />
          </div>

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
            <div
              class="mt-0.5 flex items-center gap-2 text-[11px] text-[var(--md-sys-color-on-surface-variant)] md:hidden"
            >
              <span class="font-mono">{{ formatBytes(raw.file_size) }}</span>
              <span>·</span>
              <span>{{ rawFileKindLabel(raw) }}</span>
            </div>
          </div>

          <div
            class="hidden w-28 shrink-0 text-right font-mono text-[11px] text-[var(--md-sys-color-on-surface-variant)] md:block"
          >
            {{ formatBytes(raw.file_size) }}
          </div>

          <div
            class="hidden w-24 shrink-0 text-right font-mono text-[11px] text-[var(--md-sys-color-on-surface-variant)] opacity-80 xl:block"
          >
            {{ formatModifiedDate(raw.modified_unix_secs) }}
          </div>

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

          <div
            class="flex w-24 shrink-0 items-center justify-end gap-0.5 opacity-70 transition group-hover:opacity-100"
          >
            <button
              v-if="mediaEntryMap.get(raw.rel_path) && !mediaEntryMap.get(raw.rel_path)!.completeness.is_organized"
              type="button"
              class="md-icon-btn md-icon-btn-sm md-state"
              title="按模板整理归档此媒体文件"
              @click.stop="emit('openRename', [mediaEntryMap.get(raw.rel_path)!])"
            >
              <LuFileSignature class="h-4 w-4" />
            </button>

            <button
              type="button"
              class="md-icon-btn md-icon-btn-sm md-state text-[var(--md-sys-color-error)] hover:bg-[var(--md-sys-color-error-container)]/50"
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
</template>

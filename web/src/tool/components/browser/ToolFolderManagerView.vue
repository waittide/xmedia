<script setup lang="ts">
import { computed } from 'vue'
import { useI18n } from 'vue-i18n'
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
import MdIconButton from '@/components/ui/MdIconButton.vue'
import { formatBytes } from '@/utils/format'
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

const { t } = useI18n()

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
  if (ext === 'epub' && isOrigBackupFile(raw.file_name)) return t('tool.folderView.kindOrigBackup')
  if (['opf', 'nfo', 'xml'].includes(ext)) return t('tool.folderView.kindMeta')
  if (['lrc', 'srt', 'ass', 'ssa', 'vtt'].includes(ext)) return t('tool.folderView.kindSubLyrics')
  if (['jpg', 'jpeg', 'png', 'webp', 'gif', 'bmp', 'avif', 'svg'].includes(ext)) {
    return t('tool.folderView.kindImage')
  }
  if (raw.is_media && raw.media_kind === 'book') return t('tool.domains.book')
  if (raw.is_media && raw.media_kind === 'comic') return t('tool.domains.comic')
  if (raw.is_media && raw.media_kind === 'music') return t('tool.folderView.kindAudio')
  if (raw.is_media && raw.media_kind === 'video') return t('tool.folderView.kindVideo')
  return t('tool.folderView.kindFile')
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
    <!-- Multi-root Summary -->
    <div
      v-if="isMultiRootSummary"
      class="divide-y divide-outline-variant/35 overflow-hidden rounded-corner-lg-increased bg-surface-container-low"
    >
      <button
        v-for="r in domainRoots"
        :key="`mount-dir-${r.id}`"
        type="button"
        class="md-state group flex w-full cursor-pointer items-center gap-3.5 px-4 py-3 text-left"
        @click="emit('selectRoot', r.id, 'folders')"
      >
        <div
          class="flex h-10 w-10 shrink-0 items-center justify-center rounded-corner-md bg-primary-container text-on-primary-container"
        >
          <LuHardDrive class="h-4 w-4" aria-hidden="true" />
        </div>
        <div class="min-w-0 flex-1">
          <div class="flex items-center gap-2">
            <span class="text-title-small truncate text-on-surface">
              {{ r.name }}
            </span>
            <span
              class="text-label-small shrink-0 rounded-corner-full bg-surface-container-high px-2 py-0.5 text-on-surface-variant"
            >
              {{ t('tool.folderView.mountedRootBadge') }}
            </span>
          </div>
          <div class="text-body-small mt-0.5 truncate font-mono text-on-surface-variant">
            {{ r.path }}
          </div>
        </div>
        <div class="text-body-small hidden shrink-0 font-mono text-on-surface-variant sm:block">
          {{ t('tool.folderView.mediaItemsCount', { count: rootFileCounts[r.id] ?? 0 }) }}
        </div>
        <LuChevronRight
          class="h-4 w-4 shrink-0 text-on-surface-variant opacity-60 transition group-hover:translate-x-0.5 group-hover:opacity-100"
          aria-hidden="true"
        />
      </button>
    </div>

    <div
      v-else-if="isEmptyFolder"
      class="rounded-corner-xl bg-surface-container-low py-16 text-center"
    >
      <p class="text-title-small text-on-surface">
        {{ t('tool.folderView.emptyTitle') }}
      </p>
      <p class="text-body-small mt-1 text-on-surface-variant">
        {{
          snapshot && snapshot.parent_rel_path !== null
            ? t('tool.folderView.emptyDescSub')
            : t('tool.folderView.emptyDescRoot')
        }}
      </p>
    </div>

    <div
      v-else
      class="overflow-hidden rounded-corner-lg-increased bg-surface-container-low"
    >
      <!-- File Manager Table Header -->
      <div
        class="text-label-medium flex items-center gap-3.5 border-b border-outline-variant/45 bg-surface-container/60 px-4 py-2 text-on-surface-variant"
      >
        <div class="flex w-5 shrink-0 items-center justify-center">
          <AppCheckbox
            v-if="folderModeSelectablePaths.length > 0"
            :model-value="allFolderItemsSelected"
            :aria-label="t('tool.actions.selectAll')"
            @change="emit('toggleSelectAllRaw', folderModeSelectablePaths)"
          />
        </div>
        <div class="w-10 shrink-0" />
        <div class="min-w-0 flex-1">{{ t('tool.folderView.colName') }}</div>
        <div class="hidden w-28 shrink-0 text-right md:block">{{ t('tool.folderView.colSize') }}</div>
        <div class="hidden w-24 shrink-0 text-right xl:block">{{ t('tool.folderView.colModified') }}</div>
        <div class="hidden w-32 shrink-0 text-right sm:block">{{ t('tool.folderView.colKind') }}</div>
        <div class="w-24 shrink-0 text-right">{{ t('tool.folderView.colActions') }}</div>
      </div>

      <div class="divide-y divide-outline-variant/35">
        <!-- Subfolder Rows (Non-Interactive Container + Sibling Checkbox, Trigger Button & Action Button) -->
        <div
          v-for="folder in sortedFolders"
          :key="`fm-dir-${folder.rel_path}`"
          class="group flex w-full items-center gap-3.5 px-4 py-2.5 transition-colors duration-150"
          :class="
            selectedPaths.has(folder.rel_path)
              ? 'bg-primary-container/35'
              : 'hover:bg-surface-container/60'
          "
        >
          <div
            class="flex w-5 shrink-0 items-center justify-center"
            @click="
              emit('toggleSelectFile', folder.rel_path, $event.shiftKey, folderModeSelectablePaths)
            "
          >
            <AppCheckbox
              :model-value="selectedPaths.has(folder.rel_path)"
              :aria-label="folder.name"
            />
          </div>

          <button
            type="button"
            class="md-state flex min-w-0 flex-1 cursor-pointer items-center gap-3.5 rounded-corner-md py-0.5 text-left outline-none focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-secondary"
            @click="emit('enterFolder', folder.rel_path, true)"
          >
            <div
              class="flex h-10 w-10 shrink-0 items-center justify-center rounded-corner-md bg-primary-container text-on-primary-container"
            >
              <LuFolder class="h-4 w-4" aria-hidden="true" />
            </div>
            <div class="min-w-0 flex-1">
              <div class="flex items-center gap-2">
                <span class="text-title-small truncate text-on-surface">
                  {{ folder.name }}
                </span>
                <span
                  class="text-label-small shrink-0 rounded-corner-full bg-surface-container-high px-2 py-0.5 text-on-surface-variant sm:hidden"
                >
                  {{ t('tool.folderView.folderBadge') }}
                </span>
              </div>
              <div class="text-body-small mt-0.5 text-on-surface-variant md:hidden">
                <template v-if="folder.media_file_count > 0 || folder.child_dir_count > 0">
                  {{ t('tool.folderView.itemsCount', { count: folder.media_file_count }) }}
                  <template v-if="folder.child_dir_count > 0">
                    · {{ t('tool.folderView.subfoldersCount', { count: folder.child_dir_count }) }}
                  </template>
                </template>
                <template v-else>{{ t('tool.folderView.emptyFolder') }}</template>
              </div>
            </div>
            <div
              class="text-label-small hidden w-28 shrink-0 text-right font-mono text-on-surface-variant md:block"
            >
              <template v-if="folder.media_file_count > 0 || folder.child_dir_count > 0">
                <span>{{ t('tool.folderView.itemsCount', { count: folder.media_file_count }) }}</span>
                <span v-if="folder.child_dir_count > 0" class="opacity-75">
                  {{ t('tool.folderView.dirsSlashCount', { count: folder.child_dir_count }) }}
                </span>
              </template>
              <span v-else class="opacity-80">{{ t('tool.folderView.emptyFolder') }}</span>
            </div>
            <div
              class="text-label-small hidden w-24 shrink-0 text-right font-mono text-on-surface-variant opacity-80 xl:block"
            >
              —
            </div>
            <div class="hidden w-32 shrink-0 justify-end sm:flex">
              <span
                class="text-label-small rounded-corner-full bg-surface-container-high px-2 py-0.5 text-on-surface-variant"
              >
                {{ t('tool.folderView.folderBadge') }}
              </span>
            </div>
          </button>

          <div class="flex w-24 shrink-0 items-center justify-end gap-0.5 text-on-surface-variant">
            <MdIconButton
              size="sm"
              class="opacity-0 transition group-hover:opacity-100 group-focus-within:opacity-100 focus-visible:opacity-100"
              :icon="LuTrash2"
              :tooltip="t('tool.folderView.deleteFolderTitle')"
              :aria-label="t('tool.folderView.deleteFolderTitle')"
              @click="emit('requestDeleteFiles', [folder.rel_path], false)"
            />
            <LuChevronRight
              class="h-4 w-4 opacity-75 transition group-hover:translate-x-0.5 group-hover:opacity-100"
              aria-hidden="true"
            />
          </div>
        </div>

        <!-- Raw File Rows in Current Folder (Non-Interactive Container + Sibling Controls) -->
        <div
          v-for="raw in sortedRawFiles"
          :key="`fm-raw-${raw.rel_path}`"
          class="group flex items-center gap-3.5 px-4 py-2.5 transition-colors duration-150"
          :class="
            inspectingPath === raw.rel_path
              ? 'bg-secondary-container/75'
              : selectedPaths.has(raw.rel_path)
                ? 'bg-primary-container/35'
                : 'hover:bg-surface-container/60'
          "
        >
          <div
            class="flex w-5 shrink-0 items-center justify-center"
            @click="
              emit('toggleSelectFile', raw.rel_path, $event.shiftKey, folderModeSelectablePaths)
            "
          >
            <AppCheckbox
              :model-value="selectedPaths.has(raw.rel_path)"
              :aria-label="raw.file_name"
            />
          </div>

          <component
            :is="mediaEntryMap.has(raw.rel_path) ? 'button' : 'div'"
            :type="mediaEntryMap.has(raw.rel_path) ? 'button' : undefined"
            class="flex min-w-0 flex-1 items-center gap-3.5 rounded-corner-md py-0.5 text-left outline-none focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-secondary"
            :class="mediaEntryMap.has(raw.rel_path) ? 'md-state cursor-pointer' : ''"
            @click="mediaEntryMap.has(raw.rel_path) ? handleRawFileRowClick(raw) : undefined"
          >
            <div
              class="flex h-10 w-10 shrink-0 items-center justify-center rounded-corner-md bg-surface-container-high text-on-surface-variant"
            >
              <LuImage v-if="rawFileCategory(raw) === 'image'" class="h-4 w-4 text-primary" aria-hidden="true" />
              <LuFileText
                v-else-if="rawFileCategory(raw) === 'sidecar'"
                class="h-4 w-4 text-warning"
                aria-hidden="true"
              />
              <LuBookOpen
                v-else-if="rawFileCategory(raw) === 'book'"
                class="h-4 w-4 text-primary"
                aria-hidden="true"
              />
              <LuLayers
                v-else-if="rawFileCategory(raw) === 'comic'"
                class="h-4 w-4 text-primary"
                aria-hidden="true"
              />
              <LuDisc3
                v-else-if="rawFileCategory(raw) === 'music'"
                class="h-4 w-4 text-primary"
                aria-hidden="true"
              />
              <LuClapperboard
                v-else-if="rawFileCategory(raw) === 'video'"
                class="h-4 w-4 text-primary"
                aria-hidden="true"
              />
              <LuFile v-else class="h-4 w-4 opacity-70" aria-hidden="true" />
            </div>

            <div class="min-w-0 flex-1">
              <div class="flex items-center gap-2">
                <span class="text-body-medium truncate font-mono font-medium text-on-surface">
                  {{ raw.file_name }}
                </span>
                <span
                  v-if="raw.extension"
                  class="text-label-small shrink-0 rounded-corner-full bg-surface-container-high px-2 py-0.5 font-mono uppercase text-on-surface-variant"
                >
                  {{ raw.extension }}
                </span>
              </div>
              <div class="text-body-small mt-0.5 flex items-center gap-2 text-on-surface-variant md:hidden">
                <span class="font-mono">{{ formatBytes(raw.file_size) }}</span>
                <span>·</span>
                <span>{{ rawFileKindLabel(raw) }}</span>
              </div>
            </div>

            <div class="text-label-small hidden w-28 shrink-0 text-right font-mono text-on-surface-variant md:block">
              {{ formatBytes(raw.file_size) }}
            </div>

            <div class="text-label-small hidden w-24 shrink-0 text-right font-mono text-on-surface-variant opacity-80 xl:block">
              {{ formatModifiedDate(raw.modified_unix_secs) }}
            </div>

            <div class="hidden w-32 shrink-0 items-center justify-end sm:flex">
              <span
                class="text-label-small rounded-corner-full px-2 py-0.5"
                :class="
                  raw.is_media
                    ? 'bg-primary-container/75 text-on-primary-container'
                    : 'bg-surface-container-high text-on-surface-variant'
                "
              >
                {{ rawFileKindLabel(raw) }}
              </span>
            </div>
          </component>

          <div class="flex w-24 shrink-0 items-center justify-end gap-0.5">
            <MdIconButton
              v-if="
                mediaEntryMap.get(raw.rel_path) &&
                !mediaEntryMap.get(raw.rel_path)!.completeness.is_organized
              "
              size="sm"
              :icon="LuFileSignature"
              :tooltip="t('tool.folderView.organizeMediaTitle')"
              :aria-label="t('tool.folderView.organizeMediaTitle')"
              @click="emit('openRename', [mediaEntryMap.get(raw.rel_path)!])"
            />

            <MdIconButton
              size="sm"
              :icon="LuTrash2"
              :tooltip="t('tool.folderView.deleteRawFileTitle')"
              :aria-label="t('tool.folderView.deleteRawFileTitle')"
              @click="emit('requestDeleteFiles', [raw.rel_path], false)"
            />
          </div>
        </div>
      </div>
    </div>
  </div>
</template>

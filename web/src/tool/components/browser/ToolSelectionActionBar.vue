<script setup lang="ts">
import { computed } from 'vue'
import { useI18n } from 'vue-i18n'
import {
  LuColumns3,
  LuMoreHorizontal,
  LuFileSignature,
  LuFilm,
  LuLayers,
  LuMusic,
  LuPackageCheck,
  LuPencil,
  LuSearchCheck,
  LuSlidersHorizontal,
  LuSparkles,
  LuTrash2,
  LuX,
} from 'vue-icons-plus/lu'
import MdButton from '@/components/ui/MdButton.vue'
import MdIconButton from '@/components/ui/MdIconButton.vue'
import MdMenu from '@/components/ui/MdMenu.vue'
import MdMenuItem from '@/components/ui/MdMenuItem.vue'
import type { MediaFileEntry } from '../../types'

const props = defineProps<{
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

const { t } = useI18n()

const deleteLabel = computed(() => {
  if (props.isFolderManagerMode) {
    return props.selectedFolderCount > 0
      ? t('tool.selectionBar.deleteSelectedCount', { count: props.selectedRawPaths.length })
      : t('tool.selectionBar.deleteFilesCount', { count: props.selectedRawPaths.length })
  }
  return t('tool.selectionBar.delete')
})

const organizeLabel = computed(() => {
  if (
    props.unorganizedCheckedFiles.length > 0 &&
    props.unorganizedCheckedFiles.length < props.checkedFiles.length
  ) {
    return t('tool.selectionBar.organizeCount', { count: props.unorganizedCheckedFiles.length })
  }
  return t('tool.selectionBar.organize')
})

function triggerDelete(): void {
  if (props.isFolderManagerMode) {
    emit('requestDeleteFiles', props.selectedRawPaths, false)
  } else {
    emit('requestDeleteFiles', props.checkedSelectionKeys, true)
  }
}
</script>

<template>
  <Teleport to="body">
    <Transition name="md-bar-up">
      <div
        v-if="selectedCount > 0"
        class="fixed bottom-[calc(5.75rem+env(safe-area-inset-bottom,0px))] left-1/2 z-40 flex max-w-[94vw] -translate-x-1/2 flex-nowrap items-center gap-1.5 rounded-corner-full bg-surface-container-high py-1.5 pr-2.5 pl-2 shadow-elevation-3 sm:bottom-5"
      >
        <MdIconButton
          size="sm"
          :icon="LuX"
          :tooltip="t('tool.selectionBar.clearSelect')"
          :aria-label="t('tool.selectionBar.clearSelect')"
          @click="emit('clearSelection')"
        />

        <span class="text-label-large truncate pr-1 whitespace-nowrap text-on-surface">
          {{
            isFolderManagerMode
              ? folderSelectionSummary
              : t('tool.batch.selectedCount', { count: selectedCount })
          }}
        </span>

        <div class="mx-0.5 h-5 w-px shrink-0 bg-outline-variant" />

        <!-- Primary Actions (2-3 buttons max + Overflow MdMenu so bar NEVER wraps) -->
        <template v-if="checkedFiles.length > 0">
          <MdButton
            variant="filled"
            size="sm"
            class="shrink-0 whitespace-nowrap"
            :icon="LuSparkles"
            :title="t('tool.selectionBar.quickScrapeTitle', { summary: lastScrapeSummary })"
            @click="emit('quickScrapeSelection')"
          >
            {{
              isFolderManagerMode
                ? t('tool.selectionBar.quickScrapeCount', { count: checkedFiles.length })
                : t('tool.selectionBar.quickScrape')
            }}
          </MdButton>

          <MdButton
            variant="tonal"
            size="sm"
            class="hidden shrink-0 whitespace-nowrap sm:inline-flex"
            :icon="LuColumns3"
            :title="t('tool.selectionBar.workbenchTitle')"
            @click="emit('openScrapeWorkbench', checkedFiles)"
          >
            {{ t('tool.selectionBar.workbench') }}
          </MdButton>

          <MdButton
            v-if="unorganizedCheckedFiles.length > 0"
            variant="outlined"
            size="sm"
            class="hidden shrink-0 whitespace-nowrap md:inline-flex"
            :icon="LuFileSignature"
            :title="
              unorganizedCheckedFiles.length < checkedFiles.length
                ? t('tool.selectionBar.organizePartialTitle', {
                    skipped: checkedFiles.length - unorganizedCheckedFiles.length,
                  })
                : t('tool.selectionBar.organizeTitle')
            "
            @click="emit('openRename', checkedFiles)"
          >
            {{ organizeLabel }}
          </MdButton>

          <MdMenu
            placement="top-end"
            :min-width="230"
            :aria-label="t('tool.selectionBar.moreActions')"
          >
            <template #trigger="{ toggle, triggerAttrs }">
              <MdButton
                variant="tonal"
                size="sm"
                class="shrink-0 whitespace-nowrap"
                :icon="LuMoreHorizontal"
                v-bind="triggerAttrs"
                @click="toggle"
              >
                {{ t('tool.common.more') }}
              </MdButton>
            </template>

            <MdMenuItem
              class="sm:hidden"
              :icon="LuColumns3"
              :label="t('tool.selectionBar.workbench')"
              @select="emit('openScrapeWorkbench', checkedFiles)"
            />
            <MdMenuItem
              :icon="LuSlidersHorizontal"
              :label="t('tool.selectionBar.configureStrategy')"
              @select="emit('configureScrapeSelection')"
            />
            <MdMenuItem
              :icon="LuPencil"
              :label="t('tool.selectionBar.batchEdit')"
              @select="emit('openBatchMetadata')"
            />
            <MdMenuItem
              :icon="LuFileSignature"
              :disabled="unorganizedCheckedFiles.length === 0"
              :label="organizeLabel"
              @select="emit('openRename', checkedFiles)"
            />
            <MdMenuItem
              v-if="!isFolderManagerMode"
              :icon="LuSearchCheck"
              :label="t('tool.selectionBar.dryRun')"
              @select="emit('openDryRun')"
            />
            <MdMenuItem
              v-if="selectedBookEntries.length > 0"
              :icon="LuPackageCheck"
              :label="t('tool.selectionBar.normalizeEpub')"
              @select="emit('openBookNormalizeEpub', selectedBookEntries)"
            />
            <MdMenuItem
              v-if="selectedComicEntries.length > 0"
              :icon="LuLayers"
              :label="t('tool.selectionBar.comicTools')"
              @select="emit('openComicSeriesTools')"
            />
            <MdMenuItem
              v-if="selectedMusicEntries.length > 0"
              :icon="LuMusic"
              :label="t('tool.selectionBar.musicTools')"
              @select="emit('openMusicToolbox', selectedMusicEntries)"
            />
            <MdMenuItem
              v-if="selectedVideoEntries.length > 0"
              :icon="LuFilm"
              :label="t('tool.selectionBar.videoTools')"
              @select="emit('openVideoToolbox', selectedVideoEntries)"
            />
            <MdMenuItem
              destructive
              :icon="LuTrash2"
              :label="deleteLabel"
              @select="triggerDelete"
            />
          </MdMenu>
        </template>

        <!-- Non-media raw files / empty folders selected -->
        <MdButton
          v-else
          variant="tonal"
          tone="error"
          size="sm"
          class="shrink-0 whitespace-nowrap"
          :icon="LuTrash2"
          @click="triggerDelete"
        >
          {{ deleteLabel }}
        </MdButton>
      </div>
    </Transition>
  </Teleport>
</template>

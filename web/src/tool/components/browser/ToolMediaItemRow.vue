<script setup lang="ts">
import { computed } from 'vue'
import { useI18n } from 'vue-i18n'
import {
  LuBookOpen,
  LuClapperboard,
  LuDisc3,
  LuMoreVertical,
  LuFileSignature,
  LuFileText,
  LuFilm,
  LuFolder,
  LuImage,
  LuLayers,
  LuLoader2,
  LuMusic,
  LuPackageCheck,
  LuSparkles,
  LuTrash2,
} from 'vue-icons-plus/lu'
import AppCheckbox from '@/components/ui/AppCheckbox.vue'
import MdIconButton from '@/components/ui/MdIconButton.vue'
import MdMenu from '@/components/ui/MdMenu.vue'
import MdMenuItem from '@/components/ui/MdMenuItem.vue'
import { formatBytes } from '@/utils/format'
import { buildArtworkUrl } from '../../api'
import type { ScrapeSubItem } from '../../tasks'
import type { MediaFileEntry } from '../../types'

const props = defineProps<{
  file: MediaFileEntry
  selectionKey: string
  isInspecting: boolean
  isSelected: boolean
  rootBadge: string | undefined
  dirBadge: string | null
  taskStatus: ScrapeSubItem['status'] | undefined
  artworkVersion: number
  isOrigBackup: boolean
}>()

const emit = defineEmits<{
  inspectFile: [entry: MediaFileEntry]
  toggleSelectFile: [key: string, shiftKey?: boolean]
  openBookContentEditor: [entry: MediaFileEntry]
  openBookNormalizeEpub: [entries: MediaFileEntry[]]
  openComicPageManager: [entry: MediaFileEntry]
  openMusicToolbox: [entries: MediaFileEntry[]]
  openVideoToolbox: [entries: MediaFileEntry[]]
  openRename: [entries?: MediaFileEntry[]]
  enqueueScrapeFiles: [entries: MediaFileEntry[]]
  requestDeleteFiles: [paths: string[], deleteCompanions: boolean]
}>()

const { t } = useI18n()

const hasMissingChips = computed(
  () =>
    !props.file.completeness.has_cover ||
    !props.file.completeness.has_summary_or_lyrics ||
    !props.file.completeness.has_nfo_or_tag_block ||
    (props.file.media_kind === 'book' && props.file.extension === 'epub' && props.isOrigBackup),
)
</script>

<template>
  <div
    class="group flex items-center gap-3 px-4 py-2.5 transition-colors duration-150"
    :class="
      isInspecting
        ? 'bg-secondary-container/75'
        : isSelected
          ? 'bg-primary-container/35'
          : 'hover:bg-surface-container/60'
    "
  >
    <div
      class="shrink-0"
      @click="emit('toggleSelectFile', selectionKey, $event.shiftKey)"
    >
      <AppCheckbox
        :model-value="isSelected"
        :aria-label="file.display_title || file.file_name"
      />
    </div>

    <!-- Primary Inspect Trigger (Sibling to Checkbox & Action Buttons — Zero Nested Interactive Roles) -->
    <button
      type="button"
      class="md-state flex min-w-0 flex-1 cursor-pointer items-center gap-3.5 rounded-corner-md py-0.5 text-left outline-none focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-secondary"
      @click="emit('inspectFile', file)"
    >
      <!-- Artwork Thumbnail -->
      <div
        class="relative shrink-0 overflow-hidden rounded-corner-md bg-surface-container-high"
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
          referrerpolicy="no-referrer"
          class="h-full w-full object-cover"
        />
        <div
          v-else
          class="flex h-full w-full items-center justify-center text-on-surface-variant"
        >
          <LuBookOpen v-if="file.media_kind === 'book'" class="h-4 w-4 opacity-55" aria-hidden="true" />
          <LuLayers v-else-if="file.media_kind === 'comic'" class="h-4 w-4 opacity-55" aria-hidden="true" />
          <LuDisc3 v-else-if="file.media_kind === 'music'" class="h-4 w-4 opacity-55" aria-hidden="true" />
          <LuClapperboard v-else class="h-4 w-4 opacity-55" aria-hidden="true" />
        </div>
      </div>

      <!-- Title, Subtitle & File Meta -->
      <div class="min-w-0 flex-1">
        <div class="flex items-center gap-2">
          <span class="text-title-small truncate text-on-surface">
            {{ file.display_title }}
          </span>
          <span
            class="text-label-small shrink-0 rounded-corner-full bg-surface-container-high px-2 py-0.5 font-mono uppercase text-on-surface-variant"
          >
            {{ file.extension }}
          </span>
          <span
            v-if="rootBadge"
            class="text-label-small hidden shrink-0 rounded-corner-full bg-secondary-container/75 px-2 py-0.5 text-on-secondary-container sm:inline-flex"
          >
            {{ rootBadge }}
          </span>
          <span
            v-if="taskStatus === 'running'"
            class="text-label-small inline-flex shrink-0 items-center gap-1 rounded-corner-full bg-primary-container px-2 py-0.5 text-on-primary-container"
          >
            <LuLoader2 class="h-3 w-3 animate-spin" aria-hidden="true" />
            <span>{{ t('tool.row.statusScraping') }}</span>
          </span>
          <span
            v-else-if="taskStatus === 'queued'"
            class="text-label-small shrink-0 rounded-corner-full bg-surface-container-highest px-2 py-0.5 text-on-surface-variant"
          >
            {{ t('tool.row.statusQueued') }}
          </span>
          <span
            v-else-if="taskStatus === 'unmatched'"
            class="text-label-small shrink-0 rounded-corner-full bg-warning-container px-2 py-0.5 text-on-warning-container"
          >
            {{ t('tool.row.statusUnmatched') }}
          </span>
          <span
            v-else-if="taskStatus === 'failed'"
            class="text-label-small shrink-0 rounded-corner-full bg-error-container px-2 py-0.5 text-on-error-container"
          >
            {{ t('tool.row.statusFailed') }}
          </span>
          <span
            v-if="file.year != null"
            class="text-label-small shrink-0 font-mono text-on-surface-variant"
          >
            {{ file.year }}
          </span>
        </div>

        <div
          class="text-body-small mt-0.5 flex flex-wrap items-center gap-x-2 gap-y-0.5 text-on-surface-variant"
        >
          <span
            v-if="dirBadge"
            class="text-label-small inline-flex max-w-[14rem] items-center gap-1 truncate rounded-corner-full bg-surface-container-high px-2 py-0.5"
            :title="dirBadge"
          >
            <LuFolder class="h-3 w-3 shrink-0 text-primary" aria-hidden="true" />
            <span class="truncate">{{ dirBadge }}</span>
          </span>
          <span v-if="file.display_subtitle" class="truncate">
            {{ file.display_subtitle }}
          </span>
          <span class="truncate font-mono opacity-75">{{ file.file_name }}</span>
          <span class="shrink-0 font-mono opacity-75">{{ formatBytes(file.file_size) }}</span>
        </div>
      </div>

      <!-- Missing / Anomaly Chips Only (No Green Normal Noise) -->
      <div v-if="hasMissingChips" class="hidden shrink-0 items-center gap-1 sm:flex">
        <span
          v-if="!file.completeness.has_cover"
          class="text-label-small rounded-corner-full bg-warning-container px-2 py-0.5 text-on-warning-container"
        >
          {{ t('tool.row.missingCover') }}
        </span>
        <span
          v-if="!file.completeness.has_summary_or_lyrics"
          class="text-label-small rounded-corner-full bg-surface-container-high px-2 py-0.5 text-on-surface-variant"
        >
          {{
            file.media_kind === 'music'
              ? t('tool.row.missingLyrics')
              : t('tool.row.missingSummary')
          }}
        </span>
        <span
          v-if="!file.completeness.has_nfo_or_tag_block"
          class="text-label-small rounded-corner-full bg-surface-container-high px-2 py-0.5 text-on-surface-variant"
        >
          {{ t('tool.row.missingTags') }}
        </span>
        <span
          v-if="file.media_kind === 'book' && file.extension === 'epub' && isOrigBackup"
          class="text-label-small rounded-corner-full bg-surface-container-high px-2 py-0.5 text-on-surface-variant"
          :title="t('tool.row.origBackupHint')"
        >
          {{ t('tool.row.origBackup') }}
        </span>
      </div>
    </button>

    <!-- Row Actions: 1 Quick Scrape Icon + Overflow MdMenu -->
    <div class="flex shrink-0 items-center gap-0.5">
      <MdIconButton
        size="sm"
        :icon="LuSparkles"
        :tooltip="t('tool.row.quickScrape')"
        :aria-label="t('tool.row.quickScrape')"
        @click="emit('enqueueScrapeFiles', [file])"
      />

      <MdMenu placement="bottom-end" :min-width="210" :aria-label="t('tool.row.moreActions')">
        <template #trigger="{ toggle, triggerAttrs }">
          <MdIconButton
            size="sm"
            :icon="LuMoreVertical"
            :tooltip="t('tool.row.moreActions')"
            :aria-label="t('tool.row.moreActions')"
            v-bind="triggerAttrs"
            @click="toggle"
          />
        </template>

        <MdMenuItem
          v-if="file.media_kind === 'book' && (file.extension === 'txt' || file.extension === 'epub')"
          :icon="LuFileText"
          :label="file.extension === 'txt' ? t('tool.row.editTxtConvert') : t('tool.row.editContent')"
          @select="emit('openBookContentEditor', file)"
        />
        <MdMenuItem
          v-if="
            file.media_kind === 'book' &&
            (file.extension === 'txt' || file.extension === 'epub' || file.extension === 'pdf') &&
            !isOrigBackup
          "
          :icon="LuPackageCheck"
          :label="
            file.completeness.is_normalized
              ? t('tool.row.reNormalizeEpub')
              : t('tool.row.normalizeEpub')
          "
          @select="emit('openBookNormalizeEpub', [file])"
        />
        <MdMenuItem
          v-if="file.media_kind === 'comic'"
          :icon="LuImage"
          :label="t('tool.row.comicPages')"
          @select="emit('openComicPageManager', file)"
        />
        <MdMenuItem
          v-if="file.media_kind === 'music'"
          :icon="LuMusic"
          :label="t('tool.row.musicToolbox')"
          @select="emit('openMusicToolbox', [file])"
        />
        <MdMenuItem
          v-if="file.media_kind === 'video'"
          :icon="LuFilm"
          :label="t('tool.row.videoToolbox')"
          @select="emit('openVideoToolbox', [file])"
        />
        <MdMenuItem
          v-if="!file.completeness.is_organized"
          :icon="LuFileSignature"
          :label="t('tool.row.organizeItem')"
          @select="emit('openRename', [file])"
        />
        <MdMenuItem
          destructive
          :icon="LuTrash2"
          :label="t('tool.row.deleteWithCompanions')"
          @select="emit('requestDeleteFiles', [selectionKey], true)"
        />
      </MdMenu>
    </div>
  </div>
</template>

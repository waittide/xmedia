<script setup lang="ts">
import { useI18n } from 'vue-i18n'
import {
  LuFileArchive,
  LuFileSignature,
  LuImage,
  LuLock,
  LuMoreVertical,
  LuPackageCheck,
  LuTrash2,
  LuUnlock,
  LuUpload,
  LuWand2,
} from 'vue-icons-plus/lu'
import MdIconButton from '@/components/ui/MdIconButton.vue'
import MdMenu from '@/components/ui/MdMenu.vue'
import MdMenuItem from '@/components/ui/MdMenuItem.vue'
import type { MediaFileDetail, MediaFileEntry } from '../../types'

export type InspectorUploadSlot = 'cover' | 'series_cover' | 'poster' | 'fanart' | 'thumb'

defineProps<{
  detail: MediaFileDetail
  uploadingArtwork: boolean
  polishingBook: boolean
  bookCoverLocked: boolean
  videoPosterLocked: boolean
}>()

const emit = defineEmits<{
  uploadArtwork: [slot: InspectorUploadSlot]
  toggleBookCoverLock: []
  toggleVideoPosterLock: []
  openNormalizeEpub: [entry: MediaFileEntry]
  polishBook: []
  openComicOptimize: [entry: MediaFileEntry]
  openRename: [entry: MediaFileEntry]
  requestDelete: [entry: MediaFileEntry]
}>()

const { t } = useI18n()
</script>

<template>
  <MdMenu
    placement="bottom-end"
    :min-width="220"
    :aria-label="t('tool.inspector.moreActions')"
  >
    <template #trigger="{ toggle, triggerAttrs }">
      <MdIconButton
        variant="tonal"
        :icon="LuMoreVertical"
        :tooltip="t('tool.inspector.moreActions')"
        :aria-label="t('tool.inspector.moreActions')"
        v-bind="triggerAttrs"
        @click="toggle"
      />
    </template>

    <template v-if="detail.entry.media_kind !== 'video'">
      <MdMenuItem
        :icon="LuUpload"
        :disabled="uploadingArtwork"
        :label="
          detail.entry.media_kind === 'comic'
            ? t('tool.inspector.chapterCover')
            : t('tool.actions.uploadCover')
        "
        @select="emit('uploadArtwork', 'cover')"
      />
      <MdMenuItem
        v-if="detail.entry.media_kind === 'comic' && detail.entry.rel_path.includes('/')"
        :icon="LuImage"
        :disabled="uploadingArtwork"
        :label="t('tool.inspector.seriesCover')"
        @select="emit('uploadArtwork', 'series_cover')"
      />
      <MdMenuItem
        v-if="
          detail.entry.media_kind === 'comic' &&
          (detail.entry.extension === 'cbz' || detail.entry.extension === 'zip')
        "
        :icon="LuFileArchive"
        :label="t('tool.inspector.optimizeComic')"
        @select="emit('openComicOptimize', detail.entry)"
      />
      <MdMenuItem
        v-if="detail.metadata.kind === 'book' && detail.metadata.data.writable"
        :icon="bookCoverLocked ? LuUnlock : LuLock"
        :label="bookCoverLocked ? t('tool.inspector.coverLocked') : t('tool.inspector.lockCover')"
        @select="emit('toggleBookCoverLock')"
      />
      <MdMenuItem
        v-if="
          detail.entry.media_kind === 'book' &&
          (detail.entry.extension === 'txt' ||
            detail.entry.extension === 'epub' ||
            detail.entry.extension === 'pdf')
        "
        :icon="LuPackageCheck"
        :label="
          detail.entry.completeness.is_normalized
            ? t('tool.inspector.normalizedEpub')
            : t('tool.inspector.normalizeToEpub')
        "
        @select="emit('openNormalizeEpub', detail.entry)"
      />
      <MdMenuItem
        v-if="detail.entry.media_kind === 'book' && detail.entry.extension === 'epub'"
        :icon="LuWand2"
        :disabled="polishingBook"
        :label="t('tool.inspector.polishBook')"
        @select="emit('polishBook')"
      />
    </template>

    <template v-else>
      <MdMenuItem
        :icon="LuImage"
        :disabled="uploadingArtwork"
        :label="t('tool.actions.uploadPoster')"
        @select="emit('uploadArtwork', 'poster')"
      />
      <MdMenuItem
        :icon="LuUpload"
        :disabled="uploadingArtwork"
        :label="t('tool.actions.uploadFanart')"
        @select="emit('uploadArtwork', 'fanart')"
      />
      <MdMenuItem
        :icon="videoPosterLocked ? LuUnlock : LuLock"
        :label="
          videoPosterLocked ? t('tool.inspector.posterLocked') : t('tool.inspector.lockPoster')
        "
        @select="emit('toggleVideoPosterLock')"
      />
    </template>

    <MdMenuItem
      :icon="LuFileSignature"
      :disabled="Boolean(detail.entry.completeness.is_organized)"
      :label="
        detail.entry.completeness.is_organized
          ? t('tool.inspector.organized')
          : t('tool.inspector.organize')
      "
      @select="emit('openRename', detail.entry)"
    />
    <MdMenuItem
      destructive
      :icon="LuTrash2"
      :label="t('tool.inspector.deleteEntry')"
      @select="emit('requestDelete', detail.entry)"
    />
  </MdMenu>
</template>

<script setup lang="ts">
import { computed } from 'vue'
import { useI18n } from 'vue-i18n'
import {
  LuBookOpen,
  LuClapperboard,
  LuDisc3,
  LuFileText,
  LuFilm,
  LuImage,
  LuLayers,
  LuMusic,
  LuUpload,
} from 'vue-icons-plus/lu'
import MdButton from '@/components/ui/MdButton.vue'
import MdSegmentedButton from '@/components/ui/MdSegmentedButton.vue'
import { formatDuration } from '@/utils/format'
import type { MediaFileDetail, MediaFileEntry } from '../../types'
import InspectorQuickActionsMenu, {
  type InspectorUploadSlot,
} from './InspectorQuickActionsMenu.vue'

const props = defineProps<{
  detail: MediaFileDetail
  artworkSrc: string | null
  hasPendingCover: boolean
  isDirty: boolean
  showDirtyBadge?: boolean
  videoArtworkSlot: 'poster' | 'fanart' | 'thumb'
  uploadingArtwork: boolean
  polishingBook: boolean
  bookCoverLocked: boolean
  videoPosterLocked: boolean
}>()

const emit = defineEmits<{
  'update:videoArtworkSlot': [slot: 'poster' | 'fanart' | 'thumb']
  uploadArtwork: [slot: InspectorUploadSlot]
  toggleBookCoverLock: []
  toggleVideoPosterLock: []
  openContentEditor: [entry: MediaFileEntry]
  openNormalizeEpub: [entry: MediaFileEntry]
  polishBook: []
  openComicPages: [entry: MediaFileEntry]
  openComicOptimize: [entry: MediaFileEntry]
  openMusicTools: [entry: MediaFileEntry]
  openVideoTools: [entry: MediaFileEntry]
  openRename: [entry: MediaFileEntry]
  requestDelete: [entry: MediaFileEntry]
}>()

const { t } = useI18n()

const videoSlotOptions = computed(() => [
  { value: 'poster' as const, label: t('tool.inspector.slotPoster') },
  { value: 'fanart' as const, label: t('tool.inspector.slotFanart') },
  { value: 'thumb' as const, label: t('tool.inspector.slotThumb') },
])
</script>

<template>
  <div class="flex gap-3.5 rounded-corner-lg bg-surface p-3.5">
    <div class="flex shrink-0 flex-col items-center gap-1.5">
      <div
        class="relative overflow-hidden rounded-corner-md bg-surface-container-high"
        :class="
          detail.entry.media_kind === 'music' ? 'h-24 w-24 album-shadow' : 'h-32 w-24 book-shadow'
        "
      >
        <img
          v-if="artworkSrc"
          :src="artworkSrc"
          alt="artwork"
          referrerpolicy="no-referrer"
          class="h-full w-full object-cover"
        />
        <div
          v-else
          class="flex h-full w-full flex-col items-center justify-center gap-1 text-on-surface-variant"
        >
          <LuBookOpen
            v-if="detail.entry.media_kind === 'book'"
            class="h-6 w-6 opacity-55"
            aria-hidden="true"
          />
          <LuLayers
            v-else-if="detail.entry.media_kind === 'comic'"
            class="h-6 w-6 opacity-55"
            aria-hidden="true"
          />
          <LuDisc3
            v-else-if="detail.entry.media_kind === 'music'"
            class="h-6 w-6 opacity-55"
            aria-hidden="true"
          />
          <LuClapperboard v-else class="h-6 w-6 opacity-55" aria-hidden="true" />
          <span class="text-label-small">{{ t('tool.inspector.noCover') }}</span>
        </div>
        <span
          v-if="hasPendingCover"
          class="text-label-small absolute inset-x-0 bottom-0 bg-primary/90 py-0.5 text-center text-on-primary"
        >
          {{ t('tool.inspector.pendingCover') }}
        </span>
      </div>

      <MdSegmentedButton
        v-if="detail.metadata.kind === 'video'"
        :model-value="videoArtworkSlot"
        :options="videoSlotOptions"
        size="sm"
        @update:model-value="emit('update:videoArtworkSlot', $event)"
      />
    </div>

    <div class="flex min-w-0 flex-1 flex-col justify-between gap-2">
      <div>
        <div class="flex flex-wrap items-center gap-1.5">
          <span
            class="text-label-small rounded-corner-full bg-primary-container px-2.5 py-0.5 font-mono uppercase text-on-primary-container"
          >
            {{ detail.entry.extension }}
          </span>
          <span
            v-if="showDirtyBadge && isDirty"
            class="text-label-small rounded-corner-full bg-warning-container px-2 py-0.5 text-on-warning-container"
          >
            {{ t('tool.inspector.unsaved') }}
          </span>

          <template v-if="detail.metadata.kind === 'music'">
            <span
              class="text-label-small rounded-corner-full bg-surface-container px-2 py-0.5 text-on-surface-variant"
            >
              {{ formatDuration(detail.metadata.data.duration_secs, '—') }}
            </span>
            <span
              v-if="detail.metadata.data.bit_rate_kbps"
              class="text-label-small rounded-corner-full bg-surface-container px-2 py-0.5 font-mono text-on-surface-variant"
            >
              {{ detail.metadata.data.bit_rate_kbps }} kbps
            </span>
          </template>

          <template v-else-if="detail.metadata.kind === 'comic'">
            <span
              class="text-label-small rounded-corner-full bg-surface-container px-2 py-0.5 text-on-surface-variant"
            >
              {{ t('tool.inspector.pagesCount', { count: detail.metadata.data.page_count }) }}
            </span>
            <span
              class="text-label-small rounded-corner-full bg-surface-container px-2 py-0.5 text-on-surface-variant"
            >
              {{
                detail.metadata.data.has_comic_info_xml
                  ? t('tool.inspector.hasComicInfo')
                  : t('tool.inspector.noComicInfo')
              }}
            </span>
          </template>

          <template v-else-if="detail.metadata.kind === 'video'">
            <span
              v-if="detail.metadata.data.width && detail.metadata.data.height"
              class="text-label-small rounded-corner-full bg-surface-container px-2 py-0.5 font-mono text-on-surface-variant"
            >
              {{ detail.metadata.data.width }}×{{ detail.metadata.data.height }}
            </span>
            <span
              v-if="detail.metadata.data.video_codec"
              class="text-label-small rounded-corner-full bg-surface-container px-2 py-0.5 font-mono uppercase text-on-surface-variant"
            >
              {{ detail.metadata.data.video_codec }}
            </span>
          </template>

          <template v-else-if="detail.metadata.kind === 'book'">
            <span
              class="text-label-small rounded-corner-full bg-surface-container px-2 py-0.5 text-on-surface-variant"
            >
              {{
                detail.metadata.data.format === 'epub'
                  ? t('tool.inspector.epubEmbedded')
                  : t('tool.inspector.sidecarOpf', {
                      format: detail.metadata.data.format.toUpperCase(),
                    })
              }}
            </span>
          </template>
        </div>

        <div
          v-if="detail.metadata.kind === 'video'"
          class="text-body-small mt-2 space-y-0.5 text-on-surface-variant"
        >
          <div v-if="detail.metadata.data.audio_streams.length > 0" class="truncate">
            {{
              t('tool.inspector.audioStreams', {
                summary: detail.metadata.data.audio_streams
                  .map((a) => `${a.codec.toUpperCase()}${a.language ? ` [${a.language}]` : ''}`)
                  .join(' · '),
              })
            }}
          </div>
          <div
            v-if="
              detail.metadata.data.subtitle_streams.length > 0 ||
              detail.metadata.data.sidecar_subtitles.length > 0
            "
            class="truncate"
          >
            {{
              t('tool.inspector.subtitlesSummary', {
                embedded: detail.metadata.data.subtitle_streams.length,
                sidecar: detail.metadata.data.sidecar_subtitles.length,
              })
            }}
          </div>
        </div>
      </div>

      <div class="flex items-center gap-2">
        <MdButton
          v-if="
            detail.entry.media_kind === 'book' &&
            (detail.entry.extension === 'txt' || detail.entry.extension === 'epub')
          "
          variant="tonal"
          :icon="LuFileText"
          @click="emit('openContentEditor', detail.entry)"
        >
          {{
            detail.entry.extension === 'txt'
              ? t('tool.inspector.txtConvertEpub')
              : t('tool.inspector.editContent')
          }}
        </MdButton>
        <MdButton
          v-else-if="detail.entry.media_kind === 'comic'"
          variant="tonal"
          :icon="LuImage"
          @click="emit('openComicPages', detail.entry)"
        >
          {{ t('tool.inspector.comicPages') }}
        </MdButton>
        <MdButton
          v-else-if="detail.entry.media_kind === 'music'"
          variant="tonal"
          :icon="LuMusic"
          @click="emit('openMusicTools', detail.entry)"
        >
          {{ t('tool.inspector.musicToolbox') }}
        </MdButton>
        <MdButton
          v-else-if="detail.entry.media_kind === 'video'"
          variant="tonal"
          :icon="LuFilm"
          @click="emit('openVideoTools', detail.entry)"
        >
          {{ t('tool.inspector.videoToolbox') }}
        </MdButton>
        <MdButton
          v-else
          variant="tonal"
          :icon="LuUpload"
          :loading="uploadingArtwork"
          @click="emit('uploadArtwork', 'cover')"
        >
          {{ t('tool.actions.uploadCoverBtn') }}
        </MdButton>

        <InspectorQuickActionsMenu
          :detail="detail"
          :uploading-artwork="uploadingArtwork"
          :polishing-book="polishingBook"
          :book-cover-locked="bookCoverLocked"
          :video-poster-locked="videoPosterLocked"
          @upload-artwork="emit('uploadArtwork', $event)"
          @toggle-book-cover-lock="emit('toggleBookCoverLock')"
          @toggle-video-poster-lock="emit('toggleVideoPosterLock')"
          @open-normalize-epub="emit('openNormalizeEpub', $event)"
          @polish-book="emit('polishBook')"
          @open-comic-optimize="emit('openComicOptimize', $event)"
          @open-rename="emit('openRename', $event)"
          @request-delete="emit('requestDelete', $event)"
        />
      </div>
    </div>
  </div>
</template>

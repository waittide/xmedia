<script setup lang="ts">
import type { MediaFileDetail, MediaFileEntry } from '../../types'
import InspectorArtworkCard from './InspectorArtworkCard.vue'
import InspectorBookForm from './InspectorBookForm.vue'
import InspectorComicForm from './InspectorComicForm.vue'
import InspectorMusicForm from './InspectorMusicForm.vue'
import type { InspectorUploadSlot } from './InspectorQuickActionsMenu.vue'
import InspectorVideoForm from './InspectorVideoForm.vue'
import type {
  BookInspectorForm,
  ComicInspectorForm,
  MusicInspectorForm,
  VideoInspectorForm,
} from './types'

defineProps<{
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
  errorMsg: string | null
  bookForm: BookInspectorForm
  comicForm: ComicInspectorForm
  musicForm: MusicInspectorForm
  videoForm: VideoInspectorForm
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
</script>

<template>
  <div class="space-y-4">
    <InspectorArtworkCard
      :detail="detail"
      :artwork-src="artworkSrc"
      :has-pending-cover="hasPendingCover"
      :is-dirty="isDirty"
      :show-dirty-badge="showDirtyBadge"
      :video-artwork-slot="videoArtworkSlot"
      :uploading-artwork="uploadingArtwork"
      :polishing-book="polishingBook"
      :book-cover-locked="bookCoverLocked"
      :video-poster-locked="videoPosterLocked"
      @update:video-artwork-slot="emit('update:videoArtworkSlot', $event)"
      @upload-artwork="emit('uploadArtwork', $event)"
      @toggle-book-cover-lock="emit('toggleBookCoverLock')"
      @toggle-video-poster-lock="emit('toggleVideoPosterLock')"
      @open-content-editor="emit('openContentEditor', $event)"
      @open-normalize-epub="emit('openNormalizeEpub', $event)"
      @polish-book="emit('polishBook')"
      @open-comic-pages="emit('openComicPages', $event)"
      @open-comic-optimize="emit('openComicOptimize', $event)"
      @open-music-tools="emit('openMusicTools', $event)"
      @open-video-tools="emit('openVideoTools', $event)"
      @open-rename="emit('openRename', $event)"
      @request-delete="emit('requestDelete', $event)"
    />

    <div
      v-if="errorMsg"
      role="alert"
      class="text-body-small rounded-corner-lg bg-error-container px-3.5 py-2.5 text-on-error-container"
    >
      {{ errorMsg }}
    </div>

    <InspectorMusicForm
      v-if="detail.metadata.kind === 'music'"
      :form="musicForm"
      :sidecar-lrc-filename="detail.metadata.data.sidecar_lrc_filename"
      :file-key="`${detail.entry.root_id}:${detail.entry.rel_path}`"
    />
    <InspectorVideoForm v-else-if="detail.metadata.kind === 'video'" :form="videoForm" />
    <InspectorComicForm v-else-if="detail.metadata.kind === 'comic'" :form="comicForm" />
    <InspectorBookForm
      v-else-if="detail.metadata.kind === 'book'"
      :form="bookForm"
      :book-format="detail.metadata.data.format"
      :writable="detail.metadata.data.writable"
      :is-epub="detail.entry.extension === 'epub'"
      :has-cover="detail.metadata.data.has_cover"
      :polishing-book="polishingBook"
      @polish="emit('polishBook')"
    />
  </div>
</template>

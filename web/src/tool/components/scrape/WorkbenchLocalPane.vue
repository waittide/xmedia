<script setup lang="ts">
import { computed, ref } from 'vue'
import { useI18n } from 'vue-i18n'
import {
  LuBookOpen,
  LuClapperboard,
  LuDisc3,
  LuEye,
  LuLayers,
  LuPencil,
  LuSave,
  LuUpload,
} from 'vue-icons-plus/lu'
import AppCheckbox from '@/components/ui/AppCheckbox.vue'
import MdButton from '@/components/ui/MdButton.vue'
import MdSegmentedButton from '@/components/ui/MdSegmentedButton.vue'
import { formatBytes, formatDuration } from '@/utils/format'
import { entryCompositeKey } from '../../completeness'
import type { MediaFileDetail } from '../../types'
import InspectorBookForm from '../inspector/InspectorBookForm.vue'
import InspectorComicForm from '../inspector/InspectorComicForm.vue'
import InspectorMusicForm from '../inspector/InspectorMusicForm.vue'
import InspectorVideoForm from '../inspector/InspectorVideoForm.vue'
import type { useMetadataForm } from '../inspector/useMetadataForm'

type MetadataFormInstance = ReturnType<typeof useMetadataForm>

interface ReadOnlyFieldItem {
  id: string
  label: string
  value: string
  half?: boolean
  multiline?: boolean
}

const props = defineProps<{
  detail: MediaFileDetail
  savedKeys: Set<string>
  localEditMode: boolean
  canSaveMetadata: boolean
  artworkSrc: string
  videoArtworkSlot: 'poster' | 'fanart' | 'thumb'
  uploadingArtwork: boolean
  polishingBook: boolean
  savingLocal: boolean
  errorMsg: string | null
  localForm: MetadataFormInstance
}>()

const emit = defineEmits<{
  'update:videoArtworkSlot': [slot: 'poster' | 'fanart' | 'thumb']
  'enter-edit': []
  'exit-edit': []
  'upload-artwork': []
  'polish-book': []
  'save-local': [organizeAfterSave: boolean]
}>()

const { t } = useI18n()
const organizeLocalAfterSave = ref(false)

const videoSlotOptions = computed(() => {
  if (props.detail.metadata.kind !== 'video') return []
  const d = props.detail.metadata.data
  return [
    { value: 'poster' as const, label: d.has_poster ? `${t('tool.inspector.slotPoster')}·` : t('tool.inspector.slotPoster') },
    { value: 'fanart' as const, label: d.has_fanart ? `${t('tool.inspector.slotFanart')}·` : t('tool.inspector.slotFanart') },
    { value: 'thumb' as const, label: d.has_thumb ? `${t('tool.inspector.slotThumb')}·` : t('tool.inspector.slotThumb') },
  ]
})

const hasMissingBadges = computed(
  () =>
    !props.detail.entry.completeness.has_cover ||
    !props.detail.entry.completeness.has_summary_or_lyrics ||
    !props.detail.entry.completeness.has_nfo_or_tag_block,
)

const localReadOnlyFields = computed<ReadOnlyFieldItem[]>(() => {
  const m = props.detail.metadata
  if (m.kind === 'book') {
    const b = m.data
    const seriesVal = [b.series, b.series_index ? `#${b.series_index}` : '']
      .filter(Boolean)
      .join(' ')
    return [
      { id: 'title', label: t('tool.forms.book.title'), value: b.title },
      { id: 'authors', label: t('tool.forms.book.authors'), value: b.authors.join(' / ') },
      { id: 'series', label: t('tool.forms.book.series'), value: seriesVal },
      { id: 'isbn', label: t('tool.forms.book.isbn'), value: b.isbn ?? '', half: true },
      { id: 'publisher', label: t('tool.forms.book.publisher'), value: b.publisher ?? '', half: true },
      { id: 'published_at', label: t('tool.forms.book.publishedAt'), value: b.published_at ?? '', half: true },
      { id: 'language', label: t('tool.forms.book.language'), value: b.language ?? '', half: true },
      { id: 'tags', label: t('tool.forms.book.tags'), value: b.tags.join(' / ') },
      {
        id: 'description',
        label: t('tool.forms.book.description'),
        value: b.description ?? '',
        multiline: true,
      },
    ]
  }
  if (m.kind === 'comic') {
    const c = m.data
    const seriesVal = [
      c.series,
      c.volume != null ? `Vol.${c.volume}` : '',
      c.number ? `#${c.number}` : '',
    ]
      .filter(Boolean)
      .join(' ')
    return [
      { id: 'title', label: t('tool.forms.comic.title'), value: c.title ?? '' },
      { id: 'series', label: t('tool.forms.comic.series'), value: seriesVal },
      { id: 'writers', label: t('tool.forms.comic.writers'), value: c.writers.join(' / '), half: true },
      { id: 'pencillers', label: t('tool.forms.comic.pencillers'), value: c.pencillers.join(' / '), half: true },
      { id: 'publisher', label: t('tool.forms.common.publisher'), value: c.publisher ?? '', half: true },
      { id: 'year', label: t('tool.forms.common.year'), value: c.year != null ? String(c.year) : '', half: true },
      { id: 'language', label: t('tool.forms.common.language'), value: c.language_iso ?? '', half: true },
      { id: 'genres', label: t('tool.forms.comic.genres'), value: c.genres.join(' / '), half: true },
      { id: 'tags', label: t('tool.forms.comic.tags'), value: c.tags.join(' / ') },
      { id: 'summary', label: t('tool.forms.comic.summary'), value: c.summary ?? '', multiline: true },
    ]
  }
  if (m.kind === 'music') {
    const mu = m.data
    return [
      { id: 'title', label: t('tool.forms.music.title'), value: mu.title },
      { id: 'artist', label: t('tool.forms.music.artist'), value: mu.artist, half: true },
      { id: 'album_artist', label: t('tool.forms.music.albumArtist'), value: mu.album_artist ?? '', half: true },
      { id: 'album', label: t('tool.forms.music.album'), value: mu.album },
      {
        id: 'disc',
        label: t('tool.forms.music.disc'),
        value: mu.disc_number != null ? String(mu.disc_number) : '',
        half: true,
      },
      {
        id: 'track',
        label: t('tool.forms.music.track'),
        value: mu.track_number != null ? String(mu.track_number) : '',
        half: true,
      },
      { id: 'year', label: t('tool.forms.music.year'), value: mu.year != null ? String(mu.year) : '', half: true },
      { id: 'genre', label: t('tool.forms.music.genre'), value: mu.genre ?? '', half: true },
      {
        id: 'lyrics',
        label: mu.sidecar_lrc
          ? t('tool.workbench.local.lyricsWithSidecar', { filename: mu.sidecar_lrc_filename })
          : t('tool.workbench.local.embeddedLyrics'),
        value: mu.embedded_lyrics || mu.sidecar_lrc || '',
        multiline: true,
      },
    ]
  }
  const v = m.data
  return [
    { id: 'title', label: t('tool.forms.video.movieTitle'), value: v.title },
    { id: 'original_title', label: t('tool.workbench.local.originalTitle'), value: v.original_title ?? '' },
    { id: 'show_title', label: t('tool.workbench.local.showOrSeries'), value: v.show_title ?? '' },
    {
      id: 'season',
      label: t('tool.forms.video.season'),
      value: v.season_number != null ? String(v.season_number) : '',
      half: true,
    },
    {
      id: 'episode',
      label: t('tool.forms.video.episode'),
      value: v.episode_number != null ? String(v.episode_number) : '',
      half: true,
    },
    { id: 'year', label: t('tool.forms.video.year'), value: v.year != null ? String(v.year) : '', half: true },
    { id: 'rating', label: t('tool.forms.video.rating'), value: v.rating != null ? String(v.rating) : '', half: true },
    { id: 'studio', label: t('tool.workbench.local.studioOrPublisher'), value: v.studio ?? '' },
    { id: 'directors', label: t('tool.forms.video.directors'), value: v.directors.join(' / ') },
    { id: 'actors', label: t('tool.workbench.local.cast'), value: v.actors.join(' / ') },
    { id: 'genres', label: t('tool.forms.video.genres'), value: v.genres.join(' / ') },
    { id: 'tags', label: t('tool.forms.video.tags'), value: v.tags.join(' / ') },
    { id: 'summary', label: t('tool.forms.video.plot'), value: v.summary ?? '', multiline: true },
  ]
})
</script>

<template>
  <div class="flex min-h-0 flex-1 flex-col overflow-hidden">
    <div
      class="flex shrink-0 items-center justify-between gap-2 border-b border-outline-variant/45 px-4 py-2"
    >
      <div class="flex min-w-0 items-center gap-1.5">
        <span
          class="text-label-small rounded-corner-full px-2 py-0.5"
          :class="
            localEditMode
              ? 'bg-primary-container text-on-primary-container'
              : 'bg-surface-container-high text-on-surface'
          "
        >
          {{
            localEditMode
              ? t('tool.workbench.local.editingBadge')
              : t('tool.workbench.local.infoBadge')
          }}
        </span>
        <span class="text-body-small truncate text-on-surface-variant">
          {{ formatBytes(detail.entry.file_size) }}
        </span>
        <span
          v-if="savedKeys.has(entryCompositeKey(detail.entry))"
          class="text-label-small shrink-0 rounded-corner-full bg-success-container px-2 py-0.5 text-on-success-container"
        >
          {{ t('tool.workbench.local.updatedBadge') }}
        </span>
      </div>

      <div class="flex shrink-0 items-center gap-1.5">
        <MdButton
          v-if="canSaveMetadata && !localEditMode"
          variant="tonal"
          size="sm"
          :icon="LuPencil"
          :title="t('tool.workbench.local.directEditHint')"
          @click="emit('enter-edit')"
        >
          {{ t('tool.workbench.local.directEdit') }}
        </MdButton>
        <MdButton
          v-else-if="localEditMode"
          variant="outlined"
          size="sm"
          :icon="LuEye"
          @click="emit('exit-edit')"
        >
          {{ t('tool.workbench.local.exitEdit') }}
        </MdButton>
      </div>
    </div>

    <div class="flex-1 space-y-3.5 overflow-y-auto px-4 pt-3 pb-5">
      <!-- Local Persisted Cover & Summary Card -->
      <div class="flex gap-3.5 rounded-corner-lg bg-surface p-3.5">
        <div class="flex shrink-0 flex-col items-center gap-1.5">
          <div
            class="relative overflow-hidden rounded-corner-md bg-surface-container-high"
            :class="
              detail.entry.media_kind === 'music'
                ? 'h-24 w-24 album-shadow'
                : 'h-32 w-24 book-shadow'
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
              <LuBookOpen v-if="detail.entry.media_kind === 'book'" class="h-6 w-6 opacity-55" aria-hidden="true" />
              <LuLayers v-else-if="detail.entry.media_kind === 'comic'" class="h-6 w-6 opacity-55" aria-hidden="true" />
              <LuDisc3 v-else-if="detail.entry.media_kind === 'music'" class="h-6 w-6 opacity-55" aria-hidden="true" />
              <LuClapperboard v-else class="h-6 w-6 opacity-55" aria-hidden="true" />
              <span class="text-label-small">{{ t('tool.inspector.noCover') }}</span>
            </div>
          </div>

          <MdSegmentedButton
            v-if="detail.metadata.kind === 'video' && videoSlotOptions.length > 0"
            :model-value="videoArtworkSlot"
            :options="videoSlotOptions"
            size="sm"
            @update:model-value="emit('update:videoArtworkSlot', $event)"
          />
        </div>

        <div class="flex min-w-0 flex-1 flex-col justify-between gap-2">
          <div class="space-y-1">
            <div class="text-title-small line-clamp-2 text-on-surface">
              {{ detail.entry.display_title || detail.entry.file_name }}
            </div>
            <div
              class="text-label-small truncate font-mono text-on-surface-variant"
              :title="detail.entry.rel_path"
            >
              {{ detail.entry.rel_path }}
            </div>
          </div>

          <div class="flex flex-wrap items-center gap-1.5">
            <span
              class="text-label-small rounded-corner-full bg-primary-container px-2 py-0.5 font-mono uppercase text-on-primary-container"
            >
              {{ detail.entry.extension }}
            </span>

            <template v-if="detail.metadata.kind === 'music'">
              <span
                v-if="detail.metadata.data.duration_secs"
                class="text-label-small rounded-corner-full bg-surface-container px-2 py-0.5 font-mono text-on-surface-variant"
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
              <span class="text-label-small rounded-corner-full bg-surface-container px-2 py-0.5 text-on-surface-variant">
                {{ t('tool.inspector.pagesCount', { count: detail.metadata.data.page_count }) }}
              </span>
            </template>

            <template v-else-if="detail.metadata.kind === 'video'">
              <span
                v-if="detail.metadata.data.width && detail.metadata.data.height"
                class="text-label-small rounded-corner-full bg-surface-container px-2 py-0.5 font-mono text-on-surface-variant"
              >
                {{ detail.metadata.data.width }}×{{ detail.metadata.data.height }}
              </span>
            </template>

            <template v-else-if="detail.metadata.kind === 'book'">
              <span class="text-label-small rounded-corner-full bg-surface-container px-2 py-0.5 text-on-surface-variant">
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

          <div v-if="hasMissingBadges || localEditMode" class="flex flex-wrap items-center gap-1">
            <span
              v-if="!detail.entry.completeness.has_cover"
              class="text-label-small rounded-corner-full bg-warning-container px-2 py-0.5 text-on-warning-container"
            >
              {{ t('tool.row.missingCover') }}
            </span>
            <span
              v-if="!detail.entry.completeness.has_summary_or_lyrics"
              class="text-label-small rounded-corner-full bg-surface-container-high px-2 py-0.5 text-on-surface-variant"
            >
              {{
                detail.entry.media_kind === 'music'
                  ? t('tool.row.missingLyrics')
                  : t('tool.row.missingSummary')
              }}
            </span>
            <span
              v-if="!detail.entry.completeness.has_nfo_or_tag_block"
              class="text-label-small rounded-corner-full bg-surface-container-high px-2 py-0.5 text-on-surface-variant"
            >
              {{ t('tool.row.missingTags') }}
            </span>

            <MdButton
              v-if="localEditMode"
              variant="outlined"
              size="sm"
              class="ml-auto"
              :icon="LuUpload"
              :loading="uploadingArtwork"
              @click="emit('upload-artwork')"
            >
              {{
                uploadingArtwork
                  ? t('tool.workbench.local.uploading')
                  : t('tool.workbench.local.uploadImage')
              }}
            </MdButton>
          </div>
        </div>
      </div>

      <div
        v-if="errorMsg"
        role="alert"
        class="text-body-small rounded-corner-lg bg-error-container px-3.5 py-2.5 text-on-error-container"
      >
        {{ errorMsg }}
      </div>

      <!-- Inline Domain Metadata Form -->
      <template v-if="localEditMode">
        <InspectorMusicForm
          v-if="detail.metadata.kind === 'music'"
          :form="localForm.musicForm"
          :sidecar-lrc-filename="detail.metadata.data.sidecar_lrc_filename"
          :file-key="entryCompositeKey(detail.entry)"
        />
        <InspectorVideoForm
          v-else-if="detail.metadata.kind === 'video'"
          :form="localForm.videoForm"
        />
        <InspectorComicForm
          v-else-if="detail.metadata.kind === 'comic'"
          :form="localForm.comicForm"
        />
        <InspectorBookForm
          v-else-if="detail.metadata.kind === 'book'"
          :form="localForm.bookForm"
          :book-format="detail.metadata.data.format"
          :writable="detail.metadata.data.writable"
          :is-epub="detail.entry.extension === 'epub'"
          :has-cover="detail.metadata.data.has_cover"
          :polishing-book="polishingBook"
          @polish="emit('polish-book')"
        />
      </template>

      <!-- Read-Only Structured Local Metadata Grid (Borderless Tonal Surface Blocks — NOT Input Lookalikes) -->
      <dl v-else class="grid grid-cols-2 gap-2.5">
        <div
          v-for="field in localReadOnlyFields"
          :key="field.id"
          :class="field.half ? 'col-span-1' : 'col-span-2'"
          class="rounded-corner-md bg-surface px-3 py-2"
          :title="canSaveMetadata ? t('tool.workbench.local.doubleClickHint') : undefined"
          @dblclick="canSaveMetadata && emit('enter-edit')"
        >
          <dt class="text-label-small text-on-surface-variant">
            {{ field.label }}
          </dt>
          <dd
            v-if="field.multiline"
            class="text-body-small mt-1 max-h-56 overflow-y-auto leading-relaxed whitespace-pre-wrap select-text"
            :class="field.value ? 'text-on-surface' : 'text-on-surface-variant/55'"
          >
            {{ field.value || '—' }}
          </dd>
          <dd
            v-else
            class="text-body-medium mt-0.5 break-words select-text"
            :class="field.value ? 'text-on-surface' : 'text-on-surface-variant/55'"
          >
            {{ field.value || '—' }}
          </dd>
        </div>
      </dl>
    </div>

    <!-- Local Pane Inline Edit Footer -->
    <div
      v-if="localEditMode"
      class="flex shrink-0 flex-wrap items-center justify-between gap-2 border-t border-outline-variant/45 bg-surface-container/75 px-4 py-2.5"
    >
      <AppCheckbox
        v-model="organizeLocalAfterSave"
        :disabled="savingLocal"
        :label="t('tool.inspector.saveAndArchive')"
      />

      <div class="flex items-center gap-1.5">
        <MdButton variant="text" size="sm" :disabled="savingLocal" @click="emit('exit-edit')">
          {{ t('tool.common.cancel') }}
        </MdButton>
        <MdButton
          variant="filled"
          size="sm"
          :icon="LuSave"
          :loading="savingLocal"
          @click="emit('save-local', organizeLocalAfterSave)"
        >
          {{ savingLocal ? t('tool.common.saving') : t('tool.workbench.local.saveChanges') }}
        </MdButton>
      </div>
    </div>
  </div>
</template>

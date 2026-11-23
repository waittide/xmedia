<script setup lang="ts">
import { computed } from 'vue'
import {
  LuBookOpen,
  LuClapperboard,
  LuDisc3,
  LuEye,
  LuFileSignature,
  LuLayers,
  LuLoader2,
  LuPencil,
  LuSave,
  LuUpload,
} from 'vue-icons-plus/lu'
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

const videoSlotOptions = computed(() => {
  if (props.detail.metadata.kind !== 'video') return []
  const d = props.detail.metadata.data
  return [
    { value: 'poster' as const, label: d.has_poster ? '海报·' : '海报' },
    { value: 'fanart' as const, label: d.has_fanart ? '背景·' : '背景' },
    { value: 'thumb' as const, label: d.has_thumb ? '剧照·' : '剧照' },
  ]
})

const localReadOnlyFields = computed<ReadOnlyFieldItem[]>(() => {
  const m = props.detail.metadata
  if (m.kind === 'book') {
    const b = m.data
    const seriesVal = [b.series, b.series_index ? `#${b.series_index}` : '']
      .filter(Boolean)
      .join(' ')
    return [
      { id: 'title', label: '书名 (dc:title)', value: b.title },
      { id: 'authors', label: '作者 (dc:creator)', value: b.authors.join(' / ') },
      { id: 'series', label: '系列 / 丛书 (Series)', value: seriesVal },
      { id: 'isbn', label: 'ISBN', value: b.isbn ?? '', half: true },
      { id: 'publisher', label: '出版社 (dc:publisher)', value: b.publisher ?? '', half: true },
      { id: 'published_at', label: '出版日期 (dc:date)', value: b.published_at ?? '', half: true },
      { id: 'language', label: '语言 (dc:language)', value: b.language ?? '', half: true },
      { id: 'tags', label: '标签 / 主题 (dc:subject)', value: b.tags.join(' / ') },
      {
        id: 'description',
        label: '内容简介 (dc:description)',
        value: b.description ?? '',
        multiline: true,
      },
    ]
  }
  if (m.kind === 'comic') {
    const c = m.data
    const seriesVal = [
      c.series,
      c.volume !== null ? `Vol.${c.volume}` : '',
      c.number ? `#${c.number}` : '',
    ]
      .filter(Boolean)
      .join(' ')
    return [
      { id: 'title', label: '标题', value: c.title ?? '' },
      { id: 'series', label: '系列 / 丛书', value: seriesVal },
      { id: 'writers', label: '作者 / 编剧', value: c.writers.join(' / '), half: true },
      { id: 'pencillers', label: '画师 / 作画', value: c.pencillers.join(' / '), half: true },
      { id: 'publisher', label: '出版社', value: c.publisher ?? '', half: true },
      { id: 'year', label: '年份', value: c.year ? String(c.year) : '', half: true },
      { id: 'language', label: '语言', value: c.language_iso ?? '', half: true },
      { id: 'genres', label: '流派', value: c.genres.join(' / '), half: true },
      { id: 'tags', label: '标签', value: c.tags.join(' / ') },
      { id: 'summary', label: '简介', value: c.summary ?? '', multiline: true },
    ]
  }
  if (m.kind === 'music') {
    const mu = m.data
    return [
      { id: 'title', label: '曲目标题', value: mu.title },
      { id: 'artist', label: '歌手 / 艺人', value: mu.artist, half: true },
      { id: 'album_artist', label: '专辑艺术家', value: mu.album_artist ?? '', half: true },
      { id: 'album', label: '专辑名称', value: mu.album },
      {
        id: 'disc',
        label: '碟号',
        value: mu.disc_number ? String(mu.disc_number) : '',
        half: true,
      },
      {
        id: 'track',
        label: '音轨号',
        value: mu.track_number ? String(mu.track_number) : '',
        half: true,
      },
      { id: 'year', label: '年份', value: mu.year ? String(mu.year) : '', half: true },
      { id: 'genre', label: '流派', value: mu.genre ?? '', half: true },
      {
        id: 'lyrics',
        label: mu.sidecar_lrc ? `歌词（含 ${mu.sidecar_lrc_filename}）` : '内嵌歌词',
        value: mu.embedded_lyrics || mu.sidecar_lrc || '',
        multiline: true,
      },
    ]
  }
  const v = m.data
  return [
    { id: 'title', label: '标题', value: v.title },
    { id: 'original_title', label: '原作名', value: v.original_title ?? '' },
    { id: 'show_title', label: '所属剧集 / 系列', value: v.show_title ?? '' },
    {
      id: 'season',
      label: '季号',
      value: v.season_number !== null ? String(v.season_number) : '',
      half: true,
    },
    {
      id: 'episode',
      label: '集号',
      value: v.episode_number !== null ? String(v.episode_number) : '',
      half: true,
    },
    { id: 'year', label: '首播 / 年份', value: v.year ? String(v.year) : '', half: true },
    { id: 'rating', label: '评分', value: v.rating ? String(v.rating) : '', half: true },
    { id: 'studio', label: '制片 / 发行商', value: v.studio ?? '' },
    { id: 'directors', label: '导演', value: v.directors.join(' / ') },
    { id: 'actors', label: '主演', value: v.actors.join(' / ') },
    { id: 'genres', label: '流派', value: v.genres.join(' / ') },
    { id: 'tags', label: '标签', value: v.tags.join(' / ') },
    { id: 'summary', label: '剧情简介', value: v.summary ?? '', multiline: true },
  ]
})
</script>

<template>
  <div class="flex min-h-0 flex-1 flex-col overflow-hidden">
    <div
      class="flex shrink-0 items-center justify-between gap-2 border-b border-[var(--md-sys-color-outline-variant)]/45 px-4 py-2"
    >
      <div class="flex min-w-0 items-center gap-1.5">
        <span
          class="rounded-full px-2 py-0.5 text-[10px] font-semibold"
          :class="
            localEditMode
              ? 'bg-[var(--md-sys-color-primary-container)] text-[var(--md-sys-color-on-primary-container)]'
              : 'bg-[var(--md-sys-color-surface-container-high)] text-[var(--md-sys-color-on-surface)]'
          "
        >
          {{ localEditMode ? '直接编辑中' : '当前条目信息' }}
        </span>
        <span class="truncate text-[11px] text-[var(--md-sys-color-on-surface-variant)]">
          {{ formatBytes(detail.entry.file_size) }}
        </span>
        <span
          v-if="savedKeys.has(entryCompositeKey(detail.entry))"
          class="shrink-0 rounded-full bg-[var(--md-sys-color-success-container)] px-2 py-0.5 text-[10px] font-semibold text-[var(--md-sys-color-on-success-container)]"
        >
          已更新
        </span>
      </div>

      <div class="flex shrink-0 items-center gap-1.5">
        <button
          v-if="canSaveMetadata && !localEditMode"
          type="button"
          class="md-btn md-btn-sm md-btn-tonal md-state !h-7 !px-2.5 !text-[11px]"
          title="直接在左栏内联编辑当前条目字段"
          @click="emit('enter-edit')"
        >
          <LuPencil class="h-3 w-3" />
          <span>直接编辑</span>
        </button>
        <button
          v-else-if="localEditMode"
          type="button"
          class="md-btn md-btn-sm md-btn-outlined md-state !h-7 !px-2.5 !text-[11px]"
          @click="emit('exit-edit')"
        >
          <LuEye class="h-3 w-3" />
          <span>退出编辑</span>
        </button>
      </div>
    </div>

    <div class="flex-1 space-y-3.5 overflow-y-auto px-4 pt-3 pb-5">
      <!-- Local Persisted Cover & Summary Card -->
      <div class="flex gap-3.5 rounded-2xl bg-[var(--md-sys-color-surface)] p-3.5">
        <div class="flex shrink-0 flex-col items-center gap-1.5">
          <div
            class="relative overflow-hidden rounded-xl bg-[var(--md-sys-color-surface-container-high)]"
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
              class="flex h-full w-full flex-col items-center justify-center gap-1 text-[var(--md-sys-color-on-surface-variant)]"
            >
              <LuBookOpen v-if="detail.entry.media_kind === 'book'" class="h-6 w-6 opacity-55" />
              <LuLayers v-else-if="detail.entry.media_kind === 'comic'" class="h-6 w-6 opacity-55" />
              <LuDisc3 v-else-if="detail.entry.media_kind === 'music'" class="h-6 w-6 opacity-55" />
              <LuClapperboard v-else class="h-6 w-6 opacity-55" />
              <span class="text-[10px]">无封面</span>
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
            <div class="line-clamp-2 text-[13px] font-semibold text-[var(--md-sys-color-on-surface)]">
              {{ detail.entry.display_title || detail.entry.file_name }}
            </div>
            <div
              class="truncate font-mono text-[10px] text-[var(--md-sys-color-on-surface-variant)]"
              :title="detail.entry.rel_path"
            >
              {{ detail.entry.rel_path }}
            </div>
          </div>

          <div class="flex flex-wrap items-center gap-1.5">
            <span
              class="rounded-full bg-[var(--md-sys-color-primary-container)] px-2 py-0.5 font-mono text-[10px] font-semibold uppercase text-[var(--md-sys-color-on-primary-container)]"
            >
              {{ detail.entry.extension }}
            </span>

            <template v-if="detail.metadata.kind === 'music'">
              <span
                v-if="detail.metadata.data.duration_secs"
                class="rounded-full bg-[var(--md-sys-color-surface-container)] px-2 py-0.5 font-mono text-[10px] text-[var(--md-sys-color-on-surface-variant)]"
              >
                {{ formatDuration(detail.metadata.data.duration_secs, '—') }}
              </span>
              <span
                v-if="detail.metadata.data.bit_rate_kbps"
                class="rounded-full bg-[var(--md-sys-color-surface-container)] px-2 py-0.5 font-mono text-[10px] text-[var(--md-sys-color-on-surface-variant)]"
              >
                {{ detail.metadata.data.bit_rate_kbps }} kbps
              </span>
            </template>

            <template v-else-if="detail.metadata.kind === 'comic'">
              <span
                class="rounded-full bg-[var(--md-sys-color-surface-container)] px-2 py-0.5 text-[10px] text-[var(--md-sys-color-on-surface-variant)]"
              >
                {{ detail.metadata.data.page_count }} 页
              </span>
            </template>

            <template v-else-if="detail.metadata.kind === 'video'">
              <span
                v-if="detail.metadata.data.width && detail.metadata.data.height"
                class="rounded-full bg-[var(--md-sys-color-surface-container)] px-2 py-0.5 text-[10px] text-[var(--md-sys-color-on-surface-variant)]"
              >
                {{ detail.metadata.data.width }}×{{ detail.metadata.data.height }}
              </span>
            </template>

            <template v-else-if="detail.metadata.kind === 'book'">
              <span
                class="rounded-full bg-[var(--md-sys-color-surface-container)] px-2 py-0.5 text-[10px] text-[var(--md-sys-color-on-surface-variant)]"
              >
                {{
                  detail.metadata.data.format === 'epub'
                    ? 'EPUB 内封'
                    : `${detail.metadata.data.format.toUpperCase()} 旁路 .opf`
                }}
              </span>
            </template>
          </div>

          <div class="flex flex-wrap items-center gap-1">
            <span
              class="rounded-full px-2 py-0.5 text-[10px] font-medium"
              :class="
                detail.entry.completeness.has_cover
                  ? 'bg-[var(--md-sys-color-success-container)] text-[var(--md-sys-color-on-success-container)]'
                  : 'bg-[var(--md-sys-color-warning-container)] text-[var(--md-sys-color-on-warning-container)]'
              "
            >
              {{ detail.entry.completeness.has_cover ? '有封面' : '缺封面' }}
            </span>
            <span
              class="rounded-full px-2 py-0.5 text-[10px] font-medium"
              :class="
                detail.entry.completeness.has_summary_or_lyrics
                  ? 'bg-[var(--md-sys-color-success-container)] text-[var(--md-sys-color-on-success-container)]'
                  : 'bg-[var(--md-sys-color-surface-container-high)] text-[var(--md-sys-color-on-surface-variant)]'
              "
            >
              {{
                detail.entry.media_kind === 'music'
                  ? detail.entry.completeness.has_summary_or_lyrics
                    ? '有歌词'
                    : '缺歌词'
                  : detail.entry.completeness.has_summary_or_lyrics
                    ? '有简介'
                    : '缺简介'
              }}
            </span>
            <span
              class="rounded-full px-2 py-0.5 text-[10px] font-medium"
              :class="
                detail.entry.completeness.has_nfo_or_tag_block
                  ? 'bg-[var(--md-sys-color-success-container)] text-[var(--md-sys-color-on-success-container)]'
                  : 'bg-[var(--md-sys-color-surface-container-high)] text-[var(--md-sys-color-on-surface-variant)]'
              "
            >
              {{ detail.entry.completeness.has_nfo_or_tag_block ? '有标签' : '缺标签' }}
            </span>

            <button
              v-if="localEditMode"
              type="button"
              :disabled="uploadingArtwork"
              class="md-chip md-state ml-auto !h-6 !rounded-full !px-2 !text-[10px]"
              @click="emit('upload-artwork')"
            >
              <LuUpload class="h-2.5 w-2.5" />
              <span>{{ uploadingArtwork ? '上传中…' : '上传图片' }}</span>
            </button>
          </div>
        </div>
      </div>

      <div
        v-if="errorMsg"
        class="rounded-2xl bg-[var(--md-sys-color-error-container)] px-3.5 py-2.5 text-xs text-[var(--md-sys-color-on-error-container)]"
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

      <!-- Read-Only Structured Local Metadata Grid -->
      <div v-else class="grid grid-cols-2 gap-2.5">
        <div
          v-for="field in localReadOnlyFields"
          :key="field.id"
          :class="field.half ? 'col-span-1' : 'col-span-2'"
          class="space-y-1"
          :title="canSaveMetadata ? '双击直接进入内联编辑' : undefined"
          @dblclick="canSaveMetadata && emit('enter-edit')"
        >
          <div class="text-[11px] font-medium text-[var(--md-sys-color-on-surface-variant)]">
            {{ field.label }}
          </div>
          <div
            v-if="field.multiline"
            class="min-h-[84px] max-h-56 overflow-y-auto rounded-xl border border-[var(--md-sys-color-outline-variant)]/45 bg-[var(--md-sys-color-surface)]/80 px-2.5 py-2 text-xs leading-relaxed whitespace-pre-wrap select-text"
            :class="
              field.value
                ? 'text-[var(--md-sys-color-on-surface)]'
                : 'text-[var(--md-sys-color-on-surface-variant)]/55'
            "
          >
            {{ field.value || '—' }}
          </div>
          <div
            v-else
            class="flex min-h-9 items-center rounded-xl border border-[var(--md-sys-color-outline-variant)]/45 bg-[var(--md-sys-color-surface)]/80 px-2.5 py-1.5 text-xs break-words select-text"
            :class="
              field.value
                ? 'text-[var(--md-sys-color-on-surface)]'
                : 'text-[var(--md-sys-color-on-surface-variant)]/55'
            "
          >
            {{ field.value || '—' }}
          </div>
        </div>
      </div>
    </div>

    <!-- Left Pane Inline Edit Footer -->
    <div
      v-if="localEditMode"
      class="flex shrink-0 flex-wrap items-center justify-between gap-2 border-t border-[var(--md-sys-color-outline-variant)]/45 bg-[var(--md-sys-color-surface-container)]/75 px-4 py-2.5"
    >
      <button
        type="button"
        class="md-btn md-btn-sm md-btn-text md-state"
        @click="emit('exit-edit')"
      >
        取消
      </button>
      <div class="flex items-center gap-1.5">
        <button
          type="button"
          :disabled="savingLocal"
          class="md-btn md-btn-sm md-btn-outlined md-state"
          title="保存修改并按默认模板重命名归档"
          @click="emit('save-local', true)"
        >
          <LuFileSignature class="h-3.5 w-3.5" />
          <span>保存并归档</span>
        </button>
        <button
          type="button"
          :disabled="savingLocal"
          class="md-btn md-btn-sm md-btn-filled md-state"
          title="直接写入当前原文件 (⌘S)"
          @click="emit('save-local', false)"
        >
          <LuLoader2 v-if="savingLocal" class="h-3.5 w-3.5 animate-spin" />
          <LuSave v-else class="h-3.5 w-3.5" />
          <span>{{ savingLocal ? '保存中…' : '保存修改' }}</span>
        </button>
      </div>
    </div>
  </div>
</template>

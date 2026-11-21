<script setup lang="ts">
import {
  LuBookOpen,
  LuClapperboard,
  LuDisc3,
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
import { buildArtworkUrl } from '../../api'
import { formatBytes } from '../../i18n'
import type { ScrapeSubItem } from '../../tasks'
import type { MediaFileEntry } from '../../types'

defineProps<{
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
</script>

<template>
  <div
    class="md-state group flex cursor-pointer items-center gap-3.5 px-4 py-3"
    :class="
      isInspecting
        ? 'bg-[var(--md-sys-color-secondary-container)]/75'
        : isSelected
          ? 'bg-[var(--md-sys-color-primary-container)]/35'
          : ''
    "
    @click="emit('inspectFile', file)"
  >
    <div @click.stop="emit('toggleSelectFile', selectionKey, $event.shiftKey)">
      <AppCheckbox :model-value="isSelected" />
    </div>

    <!-- Artwork Thumbnail -->
    <div
      class="relative shrink-0 overflow-hidden rounded-xl bg-[var(--md-sys-color-surface-container-high)]"
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
        class="h-full w-full object-cover"
      />
      <div
        v-else
        class="flex h-full w-full items-center justify-center text-[var(--md-sys-color-on-surface-variant)]"
      >
        <LuBookOpen v-if="file.media_kind === 'book'" class="h-4 w-4 opacity-55" />
        <LuLayers v-else-if="file.media_kind === 'comic'" class="h-4 w-4 opacity-55" />
        <LuDisc3 v-else-if="file.media_kind === 'music'" class="h-4 w-4 opacity-55" />
        <LuClapperboard v-else class="h-4 w-4 opacity-55" />
      </div>
    </div>

    <!-- Title, Subtitle & File Meta -->
    <div class="min-w-0 flex-1">
      <div class="flex items-center gap-2">
        <span class="truncate text-[13px] font-semibold text-[var(--md-sys-color-on-surface)]">
          {{ file.display_title }}
        </span>
        <span
          class="shrink-0 rounded-full bg-[var(--md-sys-color-surface-container-high)] px-2 py-0.5 font-mono text-[10px] font-semibold uppercase text-[var(--md-sys-color-on-surface-variant)]"
        >
          {{ file.extension }}
        </span>
        <span
          v-if="rootBadge"
          class="hidden shrink-0 rounded-full bg-[var(--md-sys-color-secondary-container)]/75 px-2 py-0.5 text-[10px] font-medium text-[var(--md-sys-color-on-secondary-container)] sm:inline-flex"
        >
          {{ rootBadge }}
        </span>
        <span
          v-if="taskStatus === 'running'"
          class="inline-flex shrink-0 items-center gap-1 rounded-full bg-[var(--md-sys-color-primary-container)] px-2 py-0.5 text-[10px] font-semibold text-[var(--md-sys-color-on-primary-container)]"
        >
          <LuLoader2 class="h-2.5 w-2.5 animate-spin" />
          <span>刮削中</span>
        </span>
        <span
          v-else-if="taskStatus === 'queued'"
          class="shrink-0 rounded-full bg-[var(--md-sys-color-surface-container-highest)] px-2 py-0.5 text-[10px] font-medium text-[var(--md-sys-color-on-surface-variant)]"
        >
          排队中
        </span>
        <span
          v-else-if="taskStatus === 'unmatched'"
          class="shrink-0 rounded-full bg-[var(--md-sys-color-warning-container)] px-2 py-0.5 text-[10px] font-semibold text-[var(--md-sys-color-on-warning-container)]"
        >
          未匹配
        </span>
        <span
          v-else-if="taskStatus === 'failed'"
          class="shrink-0 rounded-full bg-[var(--md-sys-color-error-container)] px-2 py-0.5 text-[10px] font-semibold text-[var(--md-sys-color-on-error-container)]"
        >
          失败
        </span>
        <span
          v-if="file.year"
          class="shrink-0 font-mono text-[11px] text-[var(--md-sys-color-on-surface-variant)]"
        >
          {{ file.year }}
        </span>
      </div>
      <div
        class="mt-0.5 flex flex-wrap items-center gap-x-2 gap-y-0.5 text-[11px] text-[var(--md-sys-color-on-surface-variant)]"
      >
        <span
          v-if="dirBadge"
          class="inline-flex max-w-[14rem] items-center gap-1 truncate rounded-full bg-[var(--md-sys-color-surface-container-high)] px-2 py-0.5 text-[10px]"
          :title="dirBadge"
        >
          <LuFolder class="h-2.5 w-2.5 shrink-0 text-[var(--md-sys-color-primary)]" />
          <span class="truncate">{{ dirBadge }}</span>
        </span>
        <span v-if="file.display_subtitle" class="truncate">
          {{ file.display_subtitle }}
        </span>
        <span class="truncate font-mono opacity-70">{{ file.file_name }}</span>
        <span class="shrink-0 font-mono opacity-70">{{ formatBytes(file.file_size) }}</span>
      </div>
    </div>

    <!-- Subtle Completeness Pills -->
    <div class="hidden shrink-0 items-center gap-1 sm:flex">
      <span
        class="rounded-full px-2 py-0.5 text-[10px] font-medium"
        :class="
          file.completeness.has_cover
            ? 'bg-[var(--md-sys-color-success-container)] text-[var(--md-sys-color-on-success-container)]'
            : 'bg-[var(--md-sys-color-warning-container)] text-[var(--md-sys-color-on-warning-container)]'
        "
      >
        {{ file.completeness.has_cover ? '有封面' : '缺封面' }}
      </span>
      <span
        class="rounded-full px-2 py-0.5 text-[10px] font-medium"
        :class="
          file.completeness.has_summary_or_lyrics
            ? 'bg-[var(--md-sys-color-success-container)] text-[var(--md-sys-color-on-success-container)]'
            : 'bg-[var(--md-sys-color-surface-container-high)] text-[var(--md-sys-color-on-surface-variant)]'
        "
      >
        {{
          file.media_kind === 'music'
            ? file.completeness.has_summary_or_lyrics
              ? '有歌词'
              : '缺歌词'
            : file.completeness.has_summary_or_lyrics
              ? '有简介'
              : '缺简介'
        }}
      </span>
      <span
        class="rounded-full px-2 py-0.5 text-[10px] font-medium"
        :class="
          file.completeness.has_nfo_or_tag_block
            ? 'bg-[var(--md-sys-color-success-container)] text-[var(--md-sys-color-on-success-container)]'
            : 'bg-[var(--md-sys-color-surface-container-high)] text-[var(--md-sys-color-on-surface-variant)]'
        "
      >
        {{ file.completeness.has_nfo_or_tag_block ? '有标签' : '缺标签' }}
      </span>
      <span
        v-if="file.media_kind === 'book' && file.extension === 'epub' && isOrigBackup"
        class="rounded-full bg-[var(--md-sys-color-surface-container-high)] px-2 py-0.5 text-[10px] font-medium text-[var(--md-sys-color-on-surface-variant)]"
        title="规范化保留的原始 EPUB 备份"
      >
        原件备份
      </span>
      <span
        v-else-if="file.media_kind === 'book' && file.completeness.is_normalized"
        class="rounded-full bg-[var(--md-sys-color-primary-container)] px-2 py-0.5 text-[10px] font-medium text-[var(--md-sys-color-on-primary-container)]"
        title="已完成 EPUB 3.0 规范化"
      >
        已规范化
      </span>
      <span
        v-if="file.completeness.is_organized"
        class="rounded-full bg-[var(--md-sys-color-success-container)] px-2 py-0.5 text-[10px] font-medium text-[var(--md-sys-color-on-success-container)]"
        title="当前路径已符合默认命名模板，无需重复整理归档"
      >
        已归档
      </span>
    </div>

    <!-- Row Quick Icon Actions -->
    <div class="flex shrink-0 items-center gap-0.5 opacity-65 transition group-hover:opacity-100">
      <button
        v-if="file.media_kind === 'book' && (file.extension === 'txt' || file.extension === 'epub')"
        type="button"
        class="md-icon-btn md-icon-btn-sm md-state"
        :title="file.extension === 'txt' ? '正文编辑与转 EPUB' : '编辑目录与正文'"
        @click.stop="emit('openBookContentEditor', file)"
      >
        <LuFileText class="h-4 w-4" />
      </button>

      <button
        v-if="
          file.media_kind === 'book' &&
          (file.extension === 'txt' || file.extension === 'epub' || file.extension === 'pdf') &&
          !isOrigBackup
        "
        type="button"
        class="md-icon-btn md-icon-btn-sm md-state"
        :class="file.completeness.is_normalized ? 'text-[var(--md-sys-color-success)]' : ''"
        :title="
          file.completeness.is_normalized
            ? '已规范化 EPUB 3.0（点击可按新参数重新规范化）'
            : '转为规范化压缩 EPUB 3.0'
        "
        @click.stop="emit('openBookNormalizeEpub', [file])"
      >
        <LuPackageCheck class="h-4 w-4" />
      </button>

      <button
        v-if="file.media_kind === 'comic'"
        type="button"
        class="md-icon-btn md-icon-btn-sm md-state"
        title="漫画页面管理与瘦身"
        @click.stop="emit('openComicPageManager', file)"
      >
        <LuImage class="h-4 w-4" />
      </button>

      <button
        v-if="file.media_kind === 'music'"
        type="button"
        class="md-icon-btn md-icon-btn-sm md-state"
        title="音频工具箱"
        @click.stop="emit('openMusicToolbox', [file])"
      >
        <LuMusic class="h-4 w-4" />
      </button>

      <button
        v-if="file.media_kind === 'video'"
        type="button"
        class="md-icon-btn md-icon-btn-sm md-state"
        title="视频字幕与剧集工具箱"
        @click.stop="emit('openVideoToolbox', [file])"
      >
        <LuFilm class="h-4 w-4" />
      </button>

      <button
        v-if="!file.completeness.is_organized"
        type="button"
        class="md-icon-btn md-icon-btn-sm md-state"
        title="按模板整理归档此条目"
        @click.stop="emit('openRename', [file])"
      >
        <LuFileSignature class="h-4 w-4" />
      </button>

      <button
        type="button"
        class="md-icon-btn md-icon-btn-sm md-state"
        title="快速自动刮削此条目"
        @click.stop="emit('enqueueScrapeFiles', [file])"
      >
        <LuSparkles class="h-4 w-4 text-[var(--md-sys-color-primary)]" />
      </button>

      <button
        type="button"
        class="md-icon-btn md-icon-btn-sm md-state text-[var(--md-sys-color-error)] hover:bg-[var(--md-sys-color-error-container)]/50"
        title="删除此条目及同名外挂文件"
        @click.stop="emit('requestDeleteFiles', [selectionKey], true)"
      >
        <LuTrash2 class="h-4 w-4" />
      </button>
    </div>
  </div>
</template>

<script setup lang="ts">
import {
  LuArrowLeft,
  LuBookMarked,
  LuDownload,
  LuList,
  LuMinus,
  LuPlus,
  LuRefreshCw,
  LuRepeat,
  LuSlidersHorizontal,
} from 'vue-icons-plus/lu'
import ThemePopover from '@/components/ThemePopover.vue'
import type { ReaderContext } from './chapterMatch'

defineProps<{
  readerCtx: ReaderContext | null
  chapterTitle: string
  activeIndex: number
  chaptersLength: number
  contentCached: boolean
  fontSize: number
  typographyOpen: boolean
  savingShelf: boolean
  loadingContent: boolean
}>()

const emit = defineEmits<{
  back: []
  'update:fontSize': [value: number]
  'toggle-typography': []
  'save-shelf': []
  refresh: []
  'open-switch-source': []
  'open-download': []
  'toggle-toc': []
}>()
</script>

<template>
  <header
    class="flex h-14 shrink-0 items-center justify-between gap-2 border-b border-[var(--md-sys-color-outline-variant)]/45 bg-[var(--md-sys-color-surface-container)]/90 px-3 sm:px-5"
  >
    <div class="flex min-w-0 items-center gap-2">
      <button
        type="button"
        class="md-icon-btn md-icon-btn-sm md-state"
        title="返回在线搜索"
        aria-label="返回在线搜索"
        @click="emit('back')"
      >
        <LuArrowLeft class="h-4 w-4" />
      </button>

      <div v-if="readerCtx" class="min-w-0">
        <div class="flex items-center gap-2">
          <span class="truncate text-sm font-semibold">{{ readerCtx.name }}</span>
          <span
            class="hidden rounded-full bg-[var(--md-sys-color-primary-container)] px-2 py-0.5 text-[10px] font-medium text-[var(--md-sys-color-on-primary-container)] sm:inline"
          >
            {{ readerCtx.sourceName }}
          </span>
        </div>
        <p class="truncate text-[11px] text-[var(--md-sys-color-on-surface-variant)]">
          {{ chapterTitle || '加载中…' }}
          <span v-if="chaptersLength > 0">
            · {{ activeIndex + 1 }} / {{ chaptersLength }}
          </span>
          <span v-if="contentCached" class="ml-1 opacity-75">· 已缓存</span>
        </p>
      </div>
    </div>

    <div class="flex shrink-0 items-center gap-1">
      <div
        class="hidden items-center gap-0.5 rounded-full bg-[var(--md-sys-color-surface-container-high)] px-1.5 py-0.5 md:flex"
      >
        <button
          type="button"
          class="md-icon-btn md-icon-btn-sm md-state !h-7 !w-7"
          title="减小字号"
          aria-label="减小字号"
          @click="emit('update:fontSize', fontSize - 1)"
        >
          <LuMinus class="h-3.5 w-3.5" />
        </button>
        <span class="px-1 font-mono text-[11px]">{{ fontSize }}</span>
        <button
          type="button"
          class="md-icon-btn md-icon-btn-sm md-state !h-7 !w-7"
          title="增大字号"
          aria-label="增大字号"
          @click="emit('update:fontSize', fontSize + 1)"
        >
          <LuPlus class="h-3.5 w-3.5" />
        </button>
      </div>

      <button
        type="button"
        class="md-icon-btn md-icon-btn-sm md-state"
        :class="
          typographyOpen
            ? '!bg-[var(--md-sys-color-secondary-container)] !text-[var(--md-sys-color-on-secondary-container)]'
            : ''
        "
        title="排版设置"
        @click="emit('toggle-typography')"
      >
        <LuSlidersHorizontal class="h-4 w-4" />
      </button>

      <ThemePopover />

      <button
        v-if="readerCtx && !readerCtx.shelfId"
        type="button"
        :disabled="savingShelf"
        class="md-btn md-btn-sm md-btn-tonal md-state"
        @click="emit('save-shelf')"
      >
        <LuBookMarked class="h-3.5 w-3.5" />
        <span class="hidden sm:inline">加入书架</span>
      </button>

      <button
        type="button"
        class="md-icon-btn md-icon-btn-sm md-state"
        title="重新拉取本章正文（忽略缓存）"
        :disabled="loadingContent"
        @click="emit('refresh')"
      >
        <LuRefreshCw class="h-4 w-4" :class="loadingContent ? 'animate-spin' : ''" />
      </button>

      <button
        type="button"
        class="md-icon-btn md-icon-btn-sm md-state"
        title="换源"
        @click="emit('open-switch-source')"
      >
        <LuRepeat class="h-4 w-4" />
      </button>

      <button
        type="button"
        class="md-icon-btn md-icon-btn-sm md-state"
        title="下载 / 打包入库"
        @click="emit('open-download')"
      >
        <LuDownload class="h-4 w-4" />
      </button>

      <button
        type="button"
        class="md-btn md-btn-sm md-btn-tonal md-state"
        @click="emit('toggle-toc')"
      >
        <LuList class="h-3.5 w-3.5" />
        <span class="hidden sm:inline">目录</span>
      </button>
    </div>
  </header>
</template>

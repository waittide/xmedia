<script setup lang="ts">
import { computed } from 'vue'
import { LuLoader2, LuRepeat, LuSearch } from 'vue-icons-plus/lu'
import AppSlider from '@/components/ui/AppSlider.vue'
import MdDialog from '@/components/ui/MdDialog.vue'
import MdSegmentedButton, { type SegmentedOption } from '@/components/ui/MdSegmentedButton.vue'
import MdSideSheet from '@/components/ui/MdSideSheet.vue'
import OnlineDownloadModal, { type OnlineDownloadTarget } from '../OnlineDownloadModal.vue'
import type { AggregatedSearchItemDto, OnlineChapterItem } from '../../onlineApi'
import type { ReaderContext } from './chapterMatch'

const props = defineProps<{
  readerCtx: ReaderContext | null
  chapters: OnlineChapterItem[]
  activeIndex: number
  typographyOpen: boolean
  fontFamily: string
  fontSize: number
  lineHeight: number
  tocOpen: boolean
  tocQuery: string
  switchSourceOpen: boolean
  searchingAlts: boolean
  altCandidates: AggregatedSearchItemDto[]
  switchingKey: string | null
  downloadOpen: boolean
  downloadTarget: OnlineDownloadTarget | null
}>()

const emit = defineEmits<{
  'close-typography': []
  'update:fontFamily': [value: string]
  'update:fontSize': [value: number]
  'update:lineHeight': [value: number]
  'close-toc': []
  'update:tocQuery': [value: string]
  'select-chapter': [pos: number]
  'close-switch-source': []
  'select-alt-source': [item: AggregatedSearchItemDto]
  'close-download': []
}>()

const fontOptions: SegmentedOption<string>[] = [
  { value: 'system-ui', label: '无衬线' },
  { value: 'serif', label: '衬线体' },
  { value: 'mono', label: '等宽' },
]

const filteredToc = computed(() => {
  const q = props.tocQuery.trim().toLowerCase()
  const entries = props.chapters.map((chapter, pos) => ({ chapter, pos }))
  if (!q) return entries
  return entries.filter((entry) => entry.chapter.title.toLowerCase().includes(q))
})
</script>

<template>
  <!-- Typography Side Sheet (synced with themeStore) -->
  <MdSideSheet
    :open="typographyOpen"
    title="排版设置"
    subtitle="与本地阅读器共享字体、字号与行高偏好"
    width-class="w-full max-w-xs"
    @close="emit('close-typography')"
  >
    <div class="space-y-5 text-xs">
      <div>
        <label class="mb-2 block font-semibold text-[var(--md-sys-color-on-surface-variant)]">
          字体
        </label>
        <MdSegmentedButton
          :model-value="fontFamily"
          :options="fontOptions"
          size="sm"
          full-width
          @update:model-value="(v) => emit('update:fontFamily', v)"
        />
      </div>

      <div>
        <div class="mb-1.5 flex justify-between">
          <span class="font-semibold text-[var(--md-sys-color-on-surface-variant)]">字号</span>
          <span class="font-mono">{{ fontSize }} px</span>
        </div>
        <AppSlider
          :model-value="fontSize"
          :min="13"
          :max="30"
          :step="1"
          aria-label="字号"
          @update:model-value="(v) => emit('update:fontSize', v)"
        />
      </div>

      <div>
        <div class="mb-1.5 flex justify-between">
          <span class="font-semibold text-[var(--md-sys-color-on-surface-variant)]">行高</span>
          <span class="font-mono">{{ lineHeight.toFixed(2) }}</span>
        </div>
        <AppSlider
          :model-value="lineHeight"
          :min="1.3"
          :max="2.4"
          :step="0.05"
          aria-label="行高"
          @update:model-value="(v) => emit('update:lineHeight', v)"
        />
      </div>
    </div>
  </MdSideSheet>

  <!-- TOC Side Sheet -->
  <MdSideSheet
    :open="tocOpen"
    :title="`目录 (${chapters.length})`"
    width-class="w-full max-w-sm"
    @close="emit('close-toc')"
  >
    <template #subheader>
      <div class="relative">
        <LuSearch
          class="pointer-events-none absolute top-1/2 left-3 h-3.5 w-3.5 -translate-y-1/2 text-[var(--md-sys-color-on-surface-variant)]"
        />
        <input
          :value="tocQuery"
          type="text"
          placeholder="筛选章节标题…"
          class="md-input h-9 w-full !pl-9 text-xs"
          @input="emit('update:tocQuery', ($event.target as HTMLInputElement).value)"
        />
      </div>
    </template>

    <div class="space-y-0.5">
      <button
        v-for="entry in filteredToc"
        :key="`toc-${entry.pos}-${entry.chapter.index}`"
        type="button"
        :disabled="entry.chapter.is_volume || !entry.chapter.url"
        class="flex w-full items-center justify-between gap-2 rounded-xl px-3 py-2 text-left text-xs transition"
        :class="
          entry.chapter.is_volume
            ? 'mt-2 font-bold text-[var(--md-sys-color-primary)] opacity-90'
            : entry.pos === activeIndex
              ? 'bg-[var(--md-sys-color-secondary-container)] font-semibold text-[var(--md-sys-color-on-secondary-container)]'
              : 'text-[var(--md-sys-color-on-surface-variant)] hover:bg-[var(--md-sys-color-surface-container-high)] hover:text-[var(--md-sys-color-on-surface)]'
        "
        @click="emit('select-chapter', entry.pos)"
      >
        <span class="truncate">{{ entry.chapter.title }}</span>
      </button>
    </div>
  </MdSideSheet>

  <!-- Switch Source Dialog -->
  <MdDialog
    :open="switchSourceOpen"
    :title="`换源 · ${readerCtx?.name ?? ''}`"
    subtitle="切换书源后将按当前章节标题自动模糊定位对应章节"
    max-width="max-w-lg"
    @close="emit('close-switch-source')"
  >
    <template #icon>
      <LuRepeat class="h-5 w-5" />
    </template>

    <div class="space-y-2 py-1 text-xs">
      <div
        v-if="searchingAlts"
        class="flex items-center justify-center py-10 text-[var(--md-sys-color-on-surface-variant)]"
      >
        <LuLoader2 class="mr-2 h-4 w-4 animate-spin" />
        <span>正在聚合搜索其他书源…</span>
      </div>
      <div
        v-else-if="altCandidates.length === 0"
        class="py-10 text-center text-[var(--md-sys-color-on-surface-variant)]"
      >
        暂未搜到其他书源的匹配条目
      </div>
      <button
        v-for="(item, idx) in altCandidates"
        :key="`alt-${item.source_id}-${idx}`"
        type="button"
        :disabled="switchingKey !== null"
        class="md-state flex w-full items-center justify-between gap-3 rounded-2xl bg-[var(--md-sys-color-surface-container)] p-3 text-left transition hover:brightness-105"
        @click="emit('select-alt-source', item)"
      >
        <div class="min-w-0 flex-1">
          <div class="flex items-center gap-2">
            <span class="truncate font-semibold text-[var(--md-sys-color-on-surface)]">
              {{ item.book.name }}
            </span>
            <span
              class="shrink-0 rounded-full bg-[var(--md-sys-color-primary-container)] px-2 py-0.5 text-[10px] font-medium text-[var(--md-sys-color-on-primary-container)]"
            >
              {{ item.source_name }}
            </span>
          </div>
          <p class="mt-0.5 truncate text-[11px] text-[var(--md-sys-color-on-surface-variant)]">
            {{ item.book.author || '佚名' }}
            <span v-if="item.book.last_chapter"> · {{ item.book.last_chapter }}</span>
          </p>
        </div>
        <LuLoader2
          v-if="switchingKey === `${item.source_id}:${item.book.book_url}`"
          class="h-4 w-4 shrink-0 animate-spin"
        />
      </button>
    </div>
  </MdDialog>

  <OnlineDownloadModal
    :open="downloadOpen"
    :target="downloadTarget"
    @close="emit('close-download')"
  />
</template>

<script setup lang="ts">
import { computed, ref, watch } from 'vue'
import {
  LuAlertTriangle,
  LuChevronDown,
  LuChevronRight,
  LuFolderPlus,
  LuGripVertical,
  LuListTree,
  LuMerge,
  LuPencil,
  LuPlus,
  LuRefreshCw,
  LuSearch,
  LuTrash2,
  LuX,
} from 'vue-icons-plus/lu'

export interface TocChapterSummary {
  key: string
  title: string
  level: number
  charCount: number
  hasRichMedia: boolean
}

const props = defineProps<{
  chapters: TocChapterSummary[]
  activeIndex: number
  activeCharCount: number
  docFormat: string
  resegmenting: boolean
  resetKey: number
}>()

const emit = defineEmits<{
  select: [idx: number]
  insertBelow: [idx: number]
  insertVolumeBelow: [idx: number]
  toggleLevel: [idx: number]
  rename: [idx: number, newTitle: string]
  mergePrev: [idx: number]
  delete: [idx: number]
  reorder: [fromIdx: number, targetIdx: number, pos: 'before' | 'after']
  autoDetectHierarchy: []
  flattenLevels: []
  syncTitlesFromBody: []
  resegmentTxt: [chapterRegex?: string, volumeRegex?: string]
}>()

const tocSearchQuery = ref('')
const onlyShowAnomalies = ref(false)
const tocToolsOpen = ref(false)
const renamingIdx = ref<number | null>(null)
const renameDraft = ref('')
const draggingIdx = ref<number | null>(null)
const dragOverIdx = ref<number | null>(null)
const dragDropPos = ref<'before' | 'after'>('after')

const regexPanelOpen = ref(false)
const chapterRegexInput = ref('')
const volumeRegexInput = ref('')

const regexPresets = [
  { label: '默认中英卷章', chapter: '', volume: '' },
  { label: '数字序号 (1. / 01)', chapter: '^\\s*\\d{1,4}[\\.、\\s]\\s*\\S+.*$', volume: '' },
  {
    label: '第X章/回/话',
    chapter: '^\\s*第[零一二三四五六七八九十百千万两0-9]+[章节回话集].*$',
    volume: '^\\s*第[零一二三四五六七八九十百千万两0-9]+[卷部篇册].*$',
  },
  {
    label: 'English Chapters',
    chapter: '(?i)^\\s*(?:chapter|section|prologue|epilogue)\\b.*$',
    volume: '(?i)^\\s*(?:volume|book|part)\\b.*$',
  },
]

watch(
  () => props.resetKey,
  () => {
    tocSearchQuery.value = ''
    onlyShowAnomalies.value = false
    tocToolsOpen.value = false
    regexPanelOpen.value = false
    renamingIdx.value = null
  },
)

const VOLUME_TITLE_RE =
  /^\s*(?:第\s*[零一二三四五六七八九十百千万两〇廿卅卌壹贰叁肆伍陆柒捌玖拾佰仟0-9０-９]+\s*[卷部集册篇](?:[\s：:、—\-].*)?|卷\s*[零一二三四五六七八九十百千万两〇廿卅卌壹贰叁肆伍陆柒捌玖拾佰仟0-9０-９]+.*|(?:volume|vol\.?|book|part)\s+[0-9ivxlcdm]+.*)$/i

function isVolumeEntry(ch: TocChapterSummary, idx: number): boolean {
  if (ch.level !== 0 || ch.title === '书籍简介') return false
  const next = props.chapters[idx + 1]
  return Boolean(next && next.level > 0) || VOLUME_TITLE_RE.test(ch.title.trim())
}

function getChapterAnomaly(ch: TocChapterSummary, idx: number): 'short' | 'long' | null {
  const count = idx === props.activeIndex ? props.activeCharCount : ch.charCount
  if (count > 20000) return 'long'
  if (count < 80 && !ch.hasRichMedia && ch.title !== '书籍简介' && !isVolumeEntry(ch, idx)) {
    return 'short'
  }
  return null
}

const anomalyCount = computed(() =>
  props.chapters.reduce((acc, ch, idx) => (getChapterAnomaly(ch, idx) ? acc + 1 : acc), 0),
)

const filteredTocItems = computed(() => {
  const q = tocSearchQuery.value.trim().toLowerCase()
  const all = props.chapters.map((ch, idx) => ({
    ch,
    idx,
    anomaly: getChapterAnomaly(ch, idx),
  }))
  return all.filter(({ ch, idx, anomaly }) => {
    if (onlyShowAnomalies.value && !anomaly) return false
    if (!q) return true
    const label = (ch.title || `第 ${idx + 1} 章`).toLowerCase()
    return label.includes(q)
  })
})

function startInlineRename(idx: number): void {
  const target = props.chapters[idx]
  if (!target) return
  renamingIdx.value = idx
  renameDraft.value = target.title
}

function commitInlineRename(): void {
  if (renamingIdx.value === null) return
  const idx = renamingIdx.value
  const cleanTitle = renameDraft.value.trim()
  renamingIdx.value = null
  if (cleanTitle) {
    emit('rename', idx, cleanTitle)
  }
}

function onTocDragStart(idx: number, ev: DragEvent): void {
  draggingIdx.value = idx
  if (ev.dataTransfer) {
    ev.dataTransfer.effectAllowed = 'move'
    ev.dataTransfer.setData('text/plain', String(idx))
  }
}

function onTocDragOver(idx: number, ev: DragEvent): void {
  if (draggingIdx.value === null || draggingIdx.value === idx) return
  const rect = (ev.currentTarget as HTMLElement).getBoundingClientRect()
  dragOverIdx.value = idx
  dragDropPos.value = ev.clientY < rect.top + rect.height / 2 ? 'before' : 'after'
}

function onTocDragEnd(): void {
  draggingIdx.value = null
  dragOverIdx.value = null
}

function onTocDrop(targetIdx: number): void {
  const fromIdx = draggingIdx.value
  const pos = dragDropPos.value
  draggingIdx.value = null
  dragOverIdx.value = null
  if (fromIdx === null || fromIdx === targetIdx) return
  emit('reorder', fromIdx, targetIdx, pos)
}
</script>

<template>
<!-- COLUMN 1: Fixed-Height Non-Jumping TOC & Chapter Tree -->
<aside
  class="flex w-68 shrink-0 flex-col border-r border-[var(--md-sys-color-outline-variant)] bg-[var(--md-sys-color-surface-container-low)] select-none lg:w-76"
>
  <!-- TOC Header + Search -->
  <div class="space-y-2 border-b border-[var(--md-sys-color-outline-variant)]/70 p-2.5">
    <div class="flex items-center justify-between gap-1.5">
      <span class="px-1 text-xs font-bold text-[var(--md-sys-color-on-surface)]">
        目录树 ({{ chapters.length }})
      </span>

      <div class="flex items-center gap-1">
        <button
          type="button"
          class="md-state inline-flex items-center gap-1 rounded-lg border px-2 py-1 text-[11px] font-medium transition"
          :class="
            tocToolsOpen
              ? 'border-[var(--md-sys-color-primary)] bg-[var(--md-sys-color-primary-container)] text-[var(--md-sys-color-on-primary-container)]'
              : 'border-[var(--md-sys-color-outline-variant)] bg-[var(--md-sys-color-surface-container-lowest)] text-[var(--md-sys-color-on-surface-variant)] hover:border-[var(--md-sys-color-primary)] hover:text-[var(--md-sys-color-on-surface)]'
          "
          title="目录层级批量整理与正文标题同步"
          @click="tocToolsOpen = !tocToolsOpen"
        >
          <LuListTree class="h-3 w-3" />
          <span>整理</span>
        </button>

        <button
          v-if="docFormat === 'txt'"
          type="button"
          class="md-state inline-flex items-center gap-1 rounded-lg border border-[var(--md-sys-color-outline-variant)] bg-[var(--md-sys-color-surface-container-lowest)] px-2 py-1 text-[11px] font-medium text-[var(--md-sys-color-on-surface-variant)] transition hover:border-[var(--md-sys-color-primary)] hover:text-[var(--md-sys-color-on-surface)]"
          @click="regexPanelOpen = !regexPanelOpen"
        >
          <LuChevronDown v-if="regexPanelOpen" class="h-3 w-3" />
          <LuChevronRight v-else class="h-3 w-3" />
          <span>分章规则</span>
        </button>

      </div>
    </div>

    <div class="grid grid-cols-2 gap-1.5">
      <button
        type="button"
        class="md-btn md-btn-sm md-btn-tonal md-state w-full"
        title="在当前项下方插入顶层分卷 (L0)，并自动将其后同级章节归入该卷"
        @click="emit('insertVolumeBelow', activeIndex)"
      >
        <LuFolderPlus class="h-3.5 w-3.5" />
        <span>新建卷</span>
      </button>
      <button
        type="button"
        class="md-btn md-btn-sm md-btn-tonal md-state w-full"
        title="在当前项下方插入新章节"
        @click="emit('insertBelow', activeIndex)"
      >
        <LuPlus class="h-3.5 w-3.5" />
        <span>新建章</span>
      </button>
    </div>

    <div class="flex items-center gap-1.5">
      <div class="relative min-w-0 flex-1">
        <LuSearch
          class="pointer-events-none absolute top-1/2 left-2.5 h-3.5 w-3.5 -translate-y-1/2 text-[var(--md-sys-color-on-surface-variant)]"
        />
        <input
          v-model="tocSearchQuery"
          type="text"
          placeholder="搜索定位章节标题…"
          class="md-input h-8 w-full pr-7 pl-8 text-xs"
        />
        <button
          v-if="tocSearchQuery"
          type="button"
          class="absolute top-1/2 right-2 -translate-y-1/2 text-[var(--md-sys-color-on-surface-variant)] hover:text-[var(--md-sys-color-on-surface)]"
          @click="tocSearchQuery = ''"
        >
          <LuX class="h-3 w-3" />
        </button>
      </div>

      <button
        v-if="anomalyCount > 0 || onlyShowAnomalies"
        type="button"
        class="md-state inline-flex h-8 shrink-0 items-center gap-1 rounded-xl border px-2 text-[11px] font-semibold transition"
        :class="
          onlyShowAnomalies
            ? 'border-[var(--md-sys-color-warning)] bg-[var(--md-sys-color-warning-container)] text-[var(--md-sys-color-on-warning-container)]'
            : 'border-[var(--md-sys-color-warning)]/40 bg-[var(--md-sys-color-warning-container)]/60 text-[var(--md-sys-color-on-warning-container)] hover:bg-[var(--md-sys-color-warning-container)]'
        "
        title="筛选过短残章 (<80字) 或超长疑似漏分章 (>20000字)"
        @click="onlyShowAnomalies = !onlyShowAnomalies"
      >
        <LuAlertTriangle class="h-3 w-3" />
        <span>{{ anomalyCount }}</span>
      </button>
    </div>
  </div>

  <!-- Collapsible Batch TOC Hierarchy & Sync Panel -->
  <div
    v-if="tocToolsOpen"
    class="space-y-1.5 border-b border-[var(--md-sys-color-outline-variant)]/75 bg-[var(--md-sys-color-surface-container)] p-2 text-[11px]"
  >
    <div class="grid grid-cols-2 gap-1.5">
      <button
        type="button"
        class="md-state rounded-lg border border-[var(--md-sys-color-outline-variant)] bg-[var(--md-sys-color-surface-container-lowest)] px-2 py-1.5 font-medium text-[var(--md-sys-color-on-surface)] transition hover:border-[var(--md-sys-color-primary)] hover:text-[var(--md-sys-color-primary)]"
        @click="emit('autoDetectHierarchy')"
      >
        自动识别卷/章层级
      </button>
      <button
        type="button"
        class="md-state rounded-lg border border-[var(--md-sys-color-outline-variant)] bg-[var(--md-sys-color-surface-container-lowest)] px-2 py-1.5 font-medium text-[var(--md-sys-color-on-surface)] transition hover:border-[var(--md-sys-color-primary)] hover:text-[var(--md-sys-color-primary)]"
        @click="emit('flattenLevels')"
      >
        全部平铺为一级
      </button>
    </div>
    <button
      type="button"
      class="md-state w-full rounded-lg border border-[var(--md-sys-color-outline-variant)] bg-[var(--md-sys-color-surface-container-lowest)] px-2 py-1.5 font-medium text-[var(--md-sys-color-on-surface)] transition hover:border-[var(--md-sys-color-primary)] hover:text-[var(--md-sys-color-primary)]"
      @click="emit('syncTitlesFromBody')"
    >
      从各章正文 H1~H3 标题同步目录名
    </button>
  </div>

  <!-- Collapsible TXT Regex Panel -->
  <div
    v-if="docFormat === 'txt' && regexPanelOpen"
    class="space-y-2 border-b border-[var(--md-sys-color-outline-variant)]/75 bg-[var(--md-sys-color-surface-container)] p-2.5 text-xs"
  >
    <div class="flex flex-wrap gap-1">
      <button
        v-for="p in regexPresets"
        :key="p.label"
        type="button"
        class="md-state rounded-md border border-[var(--md-sys-color-outline-variant)] bg-[var(--md-sys-color-surface-container-lowest)] px-2 py-0.5 text-[10px] font-medium text-[var(--md-sys-color-on-surface-variant)] transition hover:border-[var(--md-sys-color-primary)] hover:text-[var(--md-sys-color-primary)]"
        @click="chapterRegexInput = p.chapter; volumeRegexInput = p.volume"
      >
        {{ p.label }}
      </button>
    </div>
    <input
      v-model="chapterRegexInput"
      type="text"
      placeholder="章节标题正则（留空默认）"
      class="md-input h-7 w-full px-2 font-mono text-[11px]"
    />
    <input
      v-model="volumeRegexInput"
      type="text"
      placeholder="分卷标题正则（可选）"
      class="md-input h-7 w-full px-2 font-mono text-[11px]"
    />
    <button
      type="button"
      :disabled="resegmenting"
      class="md-btn md-btn-sm md-btn-filled md-state w-full disabled:opacity-50"
      @click="emit('resegmentTxt', chapterRegexInput.trim() || undefined, volumeRegexInput.trim() || undefined)"
    >
      <LuRefreshCw class="h-3 w-3" :class="resegmenting ? 'animate-spin' : ''" />
      <span>按规则重新分章</span>
    </button>
  </div>

  <!-- Fixed-Height Draggable Chapter Rows (Zero Vertical Jump) -->
  <div class="flex-1 space-y-0.5 overflow-y-auto p-1.5">
    <div
      v-for="{ ch, idx, anomaly } in filteredTocItems"
      :key="ch.key"
      :draggable="!tocSearchQuery && !onlyShowAnomalies && renamingIdx !== idx"
      class="group relative flex h-9 items-center gap-1 rounded-xl px-2 transition"
      :class="[
        ch.level > 0 ? 'pl-5' : '',
        activeIndex === idx
          ? 'bg-[var(--md-sys-color-secondary-container)] text-[var(--md-sys-color-on-secondary-container)]'
          : 'text-[var(--md-sys-color-on-surface-variant)] hover:bg-[var(--md-sys-color-surface-container-high)] hover:text-[var(--md-sys-color-on-surface)]',
        dragOverIdx === idx && dragDropPos === 'before'
          ? 'border-t-2 border-[var(--md-sys-color-primary)]'
          : '',
        dragOverIdx === idx && dragDropPos === 'after'
          ? 'border-b-2 border-[var(--md-sys-color-primary)]'
          : '',
      ]"
      @dragstart="onTocDragStart(idx, $event)"
      @dragover.prevent="onTocDragOver(idx, $event)"
      @dragend="onTocDragEnd"
      @drop.prevent="onTocDrop(idx)"
      @click="emit('select', idx)"
      @dblclick.stop="startInlineRename(idx)"
    >
      <LuGripVertical
        v-if="!tocSearchQuery"
        class="h-3.5 w-3.5 shrink-0 cursor-grab opacity-35 group-hover:opacity-80"
      />

      <button
        type="button"
        class="shrink-0 rounded-md px-1.5 py-0.5 text-[10px] font-semibold transition hover:opacity-90"
        :class="
          ch.level > 0
            ? 'bg-[var(--md-sys-color-surface-container)] text-[var(--md-sys-color-on-surface-variant)]'
            : isVolumeEntry(ch, idx)
              ? 'bg-[var(--md-sys-color-primary-container)] text-[var(--md-sys-color-on-primary-container)]'
              : 'bg-[var(--md-sys-color-surface-container-high)] text-[var(--md-sys-color-on-surface-variant)]'
        "
        title="点击切换顶层（卷/章）与子章层级"
        @click.stop="emit('toggleLevel', idx)"
      >
        {{ ch.level > 0 ? '子章' : isVolumeEntry(ch, idx) ? '卷' : '章' }}
      </button>

      <!-- Inline Rename Input vs Title Label -->
      <input
        v-if="renamingIdx === idx"
        v-model="renameDraft"
        type="text"
        autofocus
        class="h-6 min-w-0 flex-1 rounded border border-[var(--md-sys-color-primary)] bg-[var(--md-sys-color-surface-container-lowest)] px-1.5 text-xs text-[var(--md-sys-color-on-surface)] outline-none"
        @click.stop
        @keydown.enter.prevent="commitInlineRename"
        @keydown.esc.prevent="renamingIdx = null"
        @blur="commitInlineRename"
      />
      <span
        v-else
        class="min-w-0 flex-1 truncate text-xs"
        :class="ch.level === 0 ? 'font-semibold' : 'font-normal'"
        :title="`${ch.title}（双击可直接重命名，拖拽可排序）`"
      >
        {{ ch.title || `第 ${idx + 1} 章` }}
      </span>

      <span
        v-if="anomaly && renamingIdx !== idx"
        class="shrink-0 rounded px-1 py-0.2 text-[9px] font-bold group-hover:hidden"
        :class="
          anomaly === 'short'
            ? 'bg-[var(--md-sys-color-warning-container)] text-[var(--md-sys-color-on-warning-container)]'
            : 'bg-[var(--md-sys-color-error-container)] text-[var(--md-sys-color-on-error-container)]'
        "
        :title="anomaly === 'short' ? '疑似残章/空章 (<80字)' : '疑似漏分超长章 (>20000字)'"
      >
        {{ anomaly === 'short' ? '短' : '长' }}
      </span>

      <!-- Hover Compact Inline Actions (No Height Shift) -->
      <div
        v-if="renamingIdx !== idx"
        class="hidden shrink-0 items-center gap-0.5 group-hover:flex"
        @click.stop
      >
        <button
          type="button"
          class="rounded p-1 text-[var(--md-sys-color-on-surface-variant)] hover:bg-[var(--md-sys-color-surface-container-lowest)] hover:text-[var(--md-sys-color-on-surface)]"
          title="重命名章节标题"
          @click="startInlineRename(idx)"
        >
          <LuPencil class="h-3 w-3" />
        </button>
        <button
          v-if="idx > 0"
          type="button"
          class="rounded p-1 text-[var(--md-sys-color-on-surface-variant)] hover:bg-[var(--md-sys-color-surface-container-lowest)] hover:text-[var(--md-sys-color-primary)]"
          title="合并到上一章"
          @click="emit('mergePrev', idx)"
        >
          <LuMerge class="h-3 w-3" />
        </button>
        <button
          type="button"
          class="rounded p-1 text-[var(--md-sys-color-on-surface-variant)] hover:bg-[var(--md-sys-color-surface-container-lowest)] hover:text-[var(--md-sys-color-error)]"
          title="删除章节"
          @click="emit('delete', idx)"
        >
          <LuTrash2 class="h-3 w-3" />
        </button>
      </div>
    </div>
  </div>
</aside>
</template>

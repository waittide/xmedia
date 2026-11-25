<script setup lang="ts">
import { computed, nextTick, ref, watch } from 'vue'
import { useI18n } from 'vue-i18n'
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
import MdButton from '@/components/ui/MdButton.vue'
import MdChip from '@/components/ui/MdChip.vue'
import MdIconButton from '@/components/ui/MdIconButton.vue'
import MdTextField from '@/components/ui/MdTextField.vue'

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

const { t } = useI18n()

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

const regexPresets = computed(() => [
  { label: t('tool.bookEditor.toc.presetDefault'), chapter: '', volume: '' },
  {
    label: t('tool.bookEditor.toc.presetNumeric'),
    chapter: '^\\s*\\d{1,4}[\\.、\\s]\\s*\\S+.*$',
    volume: '',
  },
  {
    label: t('tool.bookEditor.toc.presetCnChapter'),
    chapter: '^\\s*第[零一二三四五六七八九十百千万两0-9]+[章节回话集].*$',
    volume: '^\\s*第[零一二三四五六七八九十百千万两0-9]+[卷部篇册].*$',
  },
  {
    label: t('tool.bookEditor.toc.presetEnChapter'),
    chapter: '(?i)^\\s*(?:chapter|section|prologue|epilogue)\\b.*$',
    volume: '(?i)^\\s*(?:volume|book|part)\\b.*$',
  },
])

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
    const label = (ch.title || `#${idx + 1}`).toLowerCase()
    return label.includes(q)
  })
})

const renameInputRef = ref<HTMLInputElement[] | HTMLInputElement | null>(null)

function startInlineRename(idx: number): void {
  const target = props.chapters[idx]
  if (!target) return
  renamingIdx.value = idx
  renameDraft.value = target.title
  void nextTick(() => {
    const el = Array.isArray(renameInputRef.value) ? renameInputRef.value[0] : renameInputRef.value
    el?.focus()
    el?.select()
  })
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
  <aside
    class="flex w-68 shrink-0 flex-col border-r border-outline-variant/50 bg-surface-container-low select-none lg:w-76"
  >
    <!-- TOC Header + Search -->
    <div class="space-y-2 border-b border-outline-variant/40 p-2.5">
      <div class="flex items-center justify-between gap-1.5">
        <span class="px-1 text-label-large font-semibold text-on-surface">
          {{ t('tool.bookEditor.toc.heading', { count: chapters.length }) }}
        </span>

        <div class="flex items-center gap-1">
          <MdChip
            variant="filter"
            :selected="tocToolsOpen"
            :label="t('tool.bookEditor.toc.organize')"
            :title="t('tool.bookEditor.toc.organizeHint')"
            @click="tocToolsOpen = !tocToolsOpen"
          >
            <template #icon>
              <LuListTree class="h-3.5 w-3.5" />
            </template>
          </MdChip>

          <MdChip
            v-if="docFormat === 'txt'"
            variant="filter"
            :selected="regexPanelOpen"
            :label="t('tool.bookEditor.toc.splitRules')"
            @click="regexPanelOpen = !regexPanelOpen"
          >
            <template #icon>
              <LuChevronDown v-if="regexPanelOpen" class="h-3.5 w-3.5" />
              <LuChevronRight v-else class="h-3.5 w-3.5" />
            </template>
          </MdChip>
        </div>
      </div>

      <div class="grid grid-cols-2 gap-1.5">
        <MdButton
          variant="tonal"
          size="sm"
          class="w-full"
          @click="emit('insertVolumeBelow', activeIndex)"
        >
          <LuFolderPlus class="h-3.5 w-3.5" />
          <span>{{ t('tool.bookEditor.toc.newVolume') }}</span>
        </MdButton>
        <MdButton
          variant="tonal"
          size="sm"
          class="w-full"
          @click="emit('insertBelow', activeIndex)"
        >
          <LuPlus class="h-3.5 w-3.5" />
          <span>{{ t('tool.bookEditor.toc.newChapter') }}</span>
        </MdButton>
      </div>

      <div class="flex items-center gap-1.5">
        <div class="min-w-0 flex-1">
          <MdTextField
            v-model="tocSearchQuery"
            size="sm"
            :aria-label="t('tool.bookEditor.toc.searchPlaceholder')"
            :placeholder="t('tool.bookEditor.toc.searchPlaceholder')"
          >
            <template #leading>
              <LuSearch class="h-4 w-4" />
            </template>
            <template v-if="tocSearchQuery" #trailing>
              <MdIconButton
                size="sm"
                :aria-label="t('tool.actions.close')"
                @click="tocSearchQuery = ''"
              >
                <LuX class="h-3.5 w-3.5" />
              </MdIconButton>
            </template>
          </MdTextField>
        </div>

        <MdChip
          v-if="anomalyCount > 0 || onlyShowAnomalies"
          variant="filter"
          :selected="onlyShowAnomalies"
          :label="String(anomalyCount)"
          :title="t('tool.bookEditor.toc.anomalyHint')"
          @click="onlyShowAnomalies = !onlyShowAnomalies"
        >
          <template #icon>
            <LuAlertTriangle class="h-3.5 w-3.5" />
          </template>
        </MdChip>
      </div>
    </div>

    <!-- Collapsible Batch TOC Hierarchy & Sync Panel -->
    <div
      v-if="tocToolsOpen"
      class="space-y-1.5 border-b border-outline-variant/40 bg-surface-container p-2.5"
    >
      <div class="grid grid-cols-2 gap-1.5">
        <MdButton variant="outlined" size="sm" class="w-full" @click="emit('autoDetectHierarchy')">
          {{ t('tool.bookEditor.toc.autoHierarchy') }}
        </MdButton>
        <MdButton variant="outlined" size="sm" class="w-full" @click="emit('flattenLevels')">
          {{ t('tool.bookEditor.toc.flattenLevels') }}
        </MdButton>
      </div>
      <MdButton variant="tonal" size="sm" class="w-full" @click="emit('syncTitlesFromBody')">
        {{ t('tool.bookEditor.toc.syncFromHeadings') }}
      </MdButton>
    </div>

    <!-- Collapsible TXT Regex Panel -->
    <div
      v-if="docFormat === 'txt' && regexPanelOpen"
      class="space-y-2 border-b border-outline-variant/40 bg-surface-container p-2.5"
    >
      <div class="flex flex-wrap gap-1">
        <MdChip
          v-for="p in regexPresets"
          :key="p.label"
          variant="suggestion"
          :label="p.label"
          @click="chapterRegexInput = p.chapter; volumeRegexInput = p.volume"
        />
      </div>
      <MdTextField
        v-model="chapterRegexInput"
        size="sm"
        :label="t('tool.bookEditor.toc.chapterRegexLabel')"
      />
      <MdTextField
        v-model="volumeRegexInput"
        size="sm"
        :label="t('tool.bookEditor.toc.volumeRegexLabel')"
      />
      <MdButton
        variant="filled"
        size="sm"
        class="w-full"
        :disabled="resegmenting"
        @click="emit('resegmentTxt', chapterRegexInput.trim() || undefined, volumeRegexInput.trim() || undefined)"
      >
        <LuRefreshCw class="h-3.5 w-3.5" :class="resegmenting ? 'animate-spin' : ''" />
        <span>{{ t('tool.bookEditor.toc.resegmentBtn') }}</span>
      </MdButton>
    </div>

    <!-- Draggable Chapter Rows (Sibling interactive controls — zero nested button/option A11y violation) -->
    <nav
      class="flex-1 space-y-0.5 overflow-y-auto p-1.5"
      :aria-label="t('tool.bookEditor.toc.heading', { count: chapters.length })"
    >
      <div
        v-for="{ ch, idx, anomaly } in filteredTocItems"
        :key="ch.key"
        :draggable="!tocSearchQuery && !onlyShowAnomalies && renamingIdx !== idx"
        class="group relative flex min-h-10 items-center gap-1 rounded-corner-md px-2 transition"
        :class="[
          ch.level > 0 ? 'pl-5' : '',
          activeIndex === idx
            ? 'bg-secondary-container text-on-secondary-container'
            : 'text-on-surface-variant hover:bg-surface-container-high hover:text-on-surface',
          dragOverIdx === idx && dragDropPos === 'before' ? 'border-t-2 border-primary' : '',
          dragOverIdx === idx && dragDropPos === 'after' ? 'border-b-2 border-primary' : '',
        ]"
        @dragstart="onTocDragStart(idx, $event)"
        @dragover.prevent="onTocDragOver(idx, $event)"
        @dragend="onTocDragEnd"
        @drop.prevent="onTocDrop(idx)"
      >
        <LuGripVertical
          v-if="!tocSearchQuery"
          aria-hidden="true"
          class="h-3.5 w-3.5 shrink-0 cursor-grab opacity-35 group-hover:opacity-80"
        />

        <button
          type="button"
          class="shrink-0 rounded-corner-xs px-1.5 py-0.5 text-label-small font-semibold transition hover:opacity-90"
          :class="
            ch.level > 0
              ? 'bg-surface-container text-on-surface-variant'
              : isVolumeEntry(ch, idx)
                ? 'bg-primary-container text-on-primary-container'
                : 'bg-surface-container-high text-on-surface-variant'
          "
          @click="emit('toggleLevel', idx)"
        >
          {{
            ch.level > 0
              ? t('tool.bookEditor.toc.subChapter')
              : isVolumeEntry(ch, idx)
                ? t('tool.bookEditor.toc.volume')
                : t('tool.bookEditor.toc.chapter')
          }}
        </button>

        <input
          v-if="renamingIdx === idx"
          ref="renameInputRef"
          v-model="renameDraft"
          type="text"
          :aria-label="t('tool.bookEditor.toc.renameAria')"
          class="h-7 min-w-0 flex-1 rounded-corner-xs border border-primary bg-surface-container-lowest px-2 text-body-small text-on-surface outline-none"
          @keydown.enter.prevent="commitInlineRename"
          @keydown.esc.prevent="renamingIdx = null"
          @blur="commitInlineRename"
        />
        <button
          v-else
          type="button"
          :aria-current="activeIndex === idx ? 'true' : undefined"
          class="flex min-w-0 flex-1 items-center gap-1.5 py-1.5 text-left outline-none focus-visible:underline"
          @click="emit('select', idx)"
          @dblclick="startInlineRename(idx)"
        >
          <span
            class="min-w-0 flex-1 truncate text-body-small"
            :class="ch.level === 0 ? 'font-semibold' : 'font-normal'"
          >
            {{ ch.title || `#${idx + 1}` }}
          </span>
          <span
            v-if="anomaly"
            class="shrink-0 rounded-corner-xs px-1.5 py-0.5 text-label-small font-semibold group-hover:hidden"
            :class="
              anomaly === 'short'
                ? 'bg-warning-container text-on-warning-container'
                : 'bg-error-container text-on-error-container'
            "
          >
            {{ anomaly === 'short' ? t('tool.bookEditor.toc.anomalyShort') : t('tool.bookEditor.toc.anomalyLong') }}
          </span>
        </button>

        <div
          v-if="renamingIdx !== idx"
          class="hidden shrink-0 items-center gap-0.5 group-hover:flex group-focus-within:flex"
        >
          <MdIconButton
            size="sm"
            :title="t('tool.bookEditor.toc.renameAria')"
            :aria-label="t('tool.bookEditor.toc.renameAria')"
            @click="startInlineRename(idx)"
          >
            <LuPencil class="h-3.5 w-3.5" />
          </MdIconButton>
          <MdIconButton
            v-if="idx > 0"
            size="sm"
            :title="t('tool.bookEditor.toc.mergePrev')"
            :aria-label="t('tool.bookEditor.toc.mergePrev')"
            @click="emit('mergePrev', idx)"
          >
            <LuMerge class="h-3.5 w-3.5" />
          </MdIconButton>
          <MdIconButton
            size="sm"
            :title="t('tool.bookEditor.toc.deleteChapter')"
            :aria-label="t('tool.bookEditor.toc.deleteChapter')"
            @click="emit('delete', idx)"
          >
            <LuTrash2 class="h-3.5 w-3.5 text-error" />
          </MdIconButton>
        </div>
      </div>
    </nav>
  </aside>
</template>

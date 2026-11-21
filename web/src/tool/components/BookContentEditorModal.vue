<script setup lang="ts">
import {
  computed,
  nextTick,
  onBeforeUnmount,
  ref,
  shallowRef,
  toRef,
  triggerRef,
  watch,
} from 'vue'
import { LuLoader2, LuX } from 'vue-icons-plus/lu'
import MdDialog from '@/components/ui/MdDialog.vue'
import { useDialogStore } from '@/stores/dialog'
import { useSnackbarStore } from '@/stores/snackbar'
import { useThemeStore } from '@/stores/theme'
import BookEditorCssPanel from './book-editor/BookEditorCssPanel.vue'
import BookEditorFindReplaceBar from './book-editor/BookEditorFindReplaceBar.vue'
import BookEditorHeader from './book-editor/BookEditorHeader.vue'
import BookEditorProofreadPanel from './book-editor/BookEditorProofreadPanel.vue'
import BookEditorTocSidebar from './book-editor/BookEditorTocSidebar.vue'
import BookEditorToolbar from './book-editor/BookEditorToolbar.vue'
import { buildLivePreviewSrcdoc as buildPreviewDoc } from './book-editor/previewDoc'
import {
  useBookEditorChapters,
  type ChapterRecord,
  type SurfaceMode,
} from './book-editor/useBookEditorChapters'
import { useBookEditorDocument } from './book-editor/useBookEditorDocument'
import { useBookEditorFindReplace } from './book-editor/useBookEditorFindReplace'
import {
  countNonWhitespace,
  escapeXml,
  extractPlainFromXhtml,
  hydrateXhtmlForBrowser as hydrateXhtmlForBrowserRaw,
  serializeContainerToValidXhtml,
} from './book-editor/xhtmlUtils'
import type { MediaFileDetail, MediaFileEntry } from '../types'

const props = defineProps<{
  open: boolean
  entry: MediaFileEntry | null
}>()

const emit = defineEmits<{
  close: []
  saved: [detail: MediaFileDetail]
  converted: [detail: MediaFileDetail, deletedSourceTxt: boolean]
}>()

const dialogStore = useDialogStore()
const snackbar = useSnackbarStore()
const themeStore = useThemeStore()

const errorMsg = ref<string | null>(null)
const docFormat = ref<'txt' | 'epub'>('epub')
const docTitle = ref('')
const docAuthors = ref<string[]>([])
const docStylesheets = shallowRef<string[]>([])
const forceHorizontalSpine = ref(false)
const primaryCssDraft = ref('')

// Shallow array avoids deep reactive proxy overhead on 1,000+ chapters
const chapters = shallowRef<ChapterRecord[]>([])
const activeIndex = ref(0)
const hasUnsavedEdits = ref(false)

// Active chapter isolated buffers for 0ms keystroke latency
const surfaceMode = ref<SurfaceMode>('wysiwyg')
const showLivePreview = ref(false)
const indentParagraphs = ref(true)
const activeTitle = ref('')
const activeCharCount = ref(0)
const sourceBuffer = ref('')
const previewSrcdoc = ref('')

const wysiwygRef = ref<HTMLDivElement | null>(null)
const sourceTextareaRef = ref<HTMLTextAreaElement | null>(null)
const findBarRef = ref<InstanceType<typeof BookEditorFindReplaceBar> | null>(null)
const epubImageInputRef = ref<HTMLInputElement | null>(null)

let isComposing = false
let idleSyncTimer: ReturnType<typeof setTimeout> | null = null

const tocResetKey = ref(0)
const deleteSourceTxt = ref(false)
const proofreadPanelOpen = ref(false)
const cssPanelOpen = ref(false)
const transformScope = ref<'book' | 'chapter'>('book')

function showToast(msg: string): void {
  snackbar.show(msg)
}

function epubAssetContext() {
  const entry = props.entry
  return entry && docFormat.value === 'epub'
    ? { rootId: entry.root_id, relPath: entry.rel_path }
    : null
}

function hydrateXhtmlForBrowser(xhtmlBody: string, sourceHref: string | null): string {
  return hydrateXhtmlForBrowserRaw(xhtmlBody, sourceHref, epubAssetContext())
}

function buildLivePreviewSrcdoc(xhtmlBody: string, sourceHref: string | null): string {
  return buildPreviewDoc({
    xhtmlBody,
    sourceHref,
    epubAssetContext: epubAssetContext(),
    stylesheets: docStylesheets.value,
    indentParagraphs: indentParagraphs.value,
  })
}

const totalCharCount = computed(() =>
  chapters.value.reduce((acc, ch, idx) => {
    if (idx === activeIndex.value) return acc + activeCharCount.value
    return acc + ch.charCount
  }, 0),
)

const currentRecord = computed<ChapterRecord | null>(
  () => chapters.value[activeIndex.value] ?? null,
)

function clearIdleSyncTimer(): void {
  if (idleSyncTimer) {
    clearTimeout(idleSyncTimer)
    idleSyncTimer = null
  }
}

function flushActiveSurfaceToRecord(): void {
  clearIdleSyncTimer()
  const rec = chapters.value[activeIndex.value]
  if (!rec) return

  rec.title = activeTitle.value.trim() || rec.title
  if (surfaceMode.value === 'wysiwyg') {
    const el = wysiwygRef.value
    if (el) {
      const serialized = serializeContainerToValidXhtml(el)
      rec.xhtmlBody = serialized || `  <p><br/></p>`
      const plain = extractPlainFromXhtml(rec.xhtmlBody, rec.title)
      rec.plainCache = plain
      rec.charCount = countNonWhitespace(plain)
      rec.hasRichMedia = /<(?:img|svg|image|table|video|audio|figure)\b/i.test(rec.xhtmlBody)
      activeCharCount.value = rec.charCount
    }
  } else {
    rec.xhtmlBody = sourceBuffer.value
    const plain = extractPlainFromXhtml(rec.xhtmlBody, rec.title)
    rec.plainCache = plain
    rec.charCount = countNonWhitespace(plain)
    rec.hasRichMedia = /<(?:img|svg|image|table|video|audio|figure)\b/i.test(rec.xhtmlBody)
    activeCharCount.value = rec.charCount
  }
  triggerRef(chapters)
}

async function mountActiveChapterIntoSurface(): Promise<void> {
  const rec = chapters.value[activeIndex.value]
  if (!rec) return

  activeTitle.value = rec.title
  activeCharCount.value = rec.charCount
  sourceBuffer.value = rec.xhtmlBody

  if (showLivePreview.value) {
    previewSrcdoc.value = buildLivePreviewSrcdoc(rec.xhtmlBody, rec.sourceHref)
  }

  await nextTick()
  if (surfaceMode.value === 'wysiwyg' && wysiwygRef.value) {
    wysiwygRef.value.innerHTML = hydrateXhtmlForBrowser(rec.xhtmlBody, rec.sourceHref)
  }
}

const {
  mapDtoList,
  cleanChapterTypography,
  splitChapterAtCursor,
  handleRenameChapter,
  toggleLevel,
  autoDetectTocHierarchy,
  flattenAllTocLevels,
  syncTocTitlesFromBodyHeadings,
  insertVolumeBelow,
  insertChapterBelow,
  mergeWithPrevious,
  deleteChapterAt,
  handleReorderChapter,
} = useBookEditorChapters({
  chapters,
  activeIndex,
  activeTitle,
  hasUnsavedEdits,
  surfaceMode,
  sourceBuffer,
  wysiwygRef,
  sourceTextareaRef,
  currentRecord,
  flushActiveSurfaceToRecord,
  mountActiveChapterIntoSurface,
  clearIdleSyncTimer,
  confirmDelete: (title) =>
    dialogStore.confirm({
      title: '删除章节',
      message: `确定删除章节「${title}」吗？`,
      danger: true,
    }),
  showToast,
})

const {
  findBarOpen,
  findQuery,
  replaceQuery,
  findScope,
  findUseRegex,
  findCaseSensitive,
  searchMatchSummary,
  handleReplaceSingle,
  handleReplaceAll,
} = useBookEditorFindReplace({
  chapters,
  activeIndex,
  currentRecord,
  surfaceMode,
  sourceBuffer,
  hasUnsavedEdits,
  flushActiveSurfaceToRecord,
  mountActiveChapterIntoSurface,
  showToast,
})

const {
  loading,
  saving,
  converting,
  resegmenting,
  transforming,
  polishing,
  uploadingImage,
  savedBadge,
  applyCssPreset,
  syncPrimaryCssDraftToStylesheets,
  runBackendTextTransform,
  onEpubImagePicked,
  handlePolishInEditor,
  handleResegmentTxt,
  handleSaveInPlace,
  handleConvertToEpub,
} = useBookEditorDocument({
  open: toRef(props, 'open'),
  entry: toRef(props, 'entry'),
  docFormat,
  docTitle,
  docAuthors,
  docStylesheets,
  forceHorizontalSpine,
  primaryCssDraft,
  chapters,
  activeIndex,
  activeTitle,
  hasUnsavedEdits,
  surfaceMode,
  showLivePreview,
  sourceBuffer,
  previewSrcdoc,
  wysiwygRef,
  sourceTextareaRef,
  currentRecord,
  tocResetKey,
  deleteSourceTxt,
  transformScope,
  errorMsg,
  mapDtoList,
  flushActiveSurfaceToRecord,
  mountActiveChapterIntoSurface,
  buildLivePreviewSrcdoc,
  confirmResegment: () =>
    dialogStore.confirm({
      title: '重新切分章节',
      message: '按新规则重新切分原 TXT 将覆盖当前未保存的修改，确定继续吗？',
      danger: true,
    }),
  showToast,
  onSaved: (detail) => emit('saved', detail),
  onConverted: (detail, deleted) => emit('converted', detail, deleted),
})

function scheduleIdleMetricsUpdate(): void {
  clearIdleSyncTimer()
  idleSyncTimer = setTimeout(() => {
    idleSyncTimer = null
    const rec = chapters.value[activeIndex.value]
    if (!rec) return
    if (surfaceMode.value === 'wysiwyg' && wysiwygRef.value) {
      const text = wysiwygRef.value.innerText ?? ''
      activeCharCount.value = countNonWhitespace(text)
      if (showLivePreview.value) {
        const xhtml = serializeContainerToValidXhtml(wysiwygRef.value)
        previewSrcdoc.value = buildLivePreviewSrcdoc(xhtml, rec.sourceHref)
      }
    } else if (surfaceMode.value === 'xhtml') {
      const plain = extractPlainFromXhtml(sourceBuffer.value, activeTitle.value)
      activeCharCount.value = countNonWhitespace(plain)
      if (showLivePreview.value) {
        previewSrcdoc.value = buildLivePreviewSrcdoc(sourceBuffer.value, rec.sourceHref)
      }
    }
  }, 160)
}

function onWysiwygFocus(): void {
  try {
    document.execCommand('defaultParagraphSeparator', false, 'p')
  } catch {
    // ignore browser differences
  }
}

function onWysiwygInput(): void {
  hasUnsavedEdits.value = true
  if (isComposing) return
  scheduleIdleMetricsUpdate()
}

function onWysiwygCompositionEnd(): void {
  isComposing = false
  hasUnsavedEdits.value = true
  scheduleIdleMetricsUpdate()
}

function onSourceTextareaInput(): void {
  hasUnsavedEdits.value = true
  scheduleIdleMetricsUpdate()
}

function onSourceKeydown(ev: KeyboardEvent): void {
  if (ev.key === 'Tab') {
    ev.preventDefault()
    const ta = sourceTextareaRef.value
    if (!ta) return
    const start = ta.selectionStart
    const end = ta.selectionEnd
    sourceBuffer.value = `${sourceBuffer.value.slice(0, start)}  ${sourceBuffer.value.slice(end)}`
    hasUnsavedEdits.value = true
    void nextTick(() => {
      ta.selectionStart = start + 2
      ta.selectionEnd = start + 2
    })
  }
}

function onActiveTitleInput(val: string): void {
  activeTitle.value = val
  const rec = chapters.value[activeIndex.value]
  if (rec) {
    rec.title = val
    triggerRef(chapters)
  }
  hasUnsavedEdits.value = true
}

async function selectChapter(idx: number): Promise<void> {
  if (idx === activeIndex.value || idx < 0 || idx >= chapters.value.length) return
  flushActiveSurfaceToRecord()
  activeIndex.value = idx
  await mountActiveChapterIntoSurface()
}

async function switchSurfaceMode(next: SurfaceMode): Promise<void> {
  if (surfaceMode.value === next) return
  flushActiveSurfaceToRecord()
  surfaceMode.value = next
  await mountActiveChapterIntoSurface()
}

function toggleLivePreview(): void {
  flushActiveSurfaceToRecord()
  showLivePreview.value = !showLivePreview.value
  if (showLivePreview.value && currentRecord.value) {
    previewSrcdoc.value = buildLivePreviewSrcdoc(
      currentRecord.value.xhtmlBody,
      currentRecord.value.sourceHref,
    )
  }
}

watch(
  () => [indentParagraphs.value, themeStore.isDark] as const,
  () => {
    if (showLivePreview.value && currentRecord.value) {
      flushActiveSurfaceToRecord()
      previewSrcdoc.value = buildLivePreviewSrcdoc(
        currentRecord.value.xhtmlBody,
        currentRecord.value.sourceHref,
      )
    }
  },
)

function execWysiwygCmd(command: string, value?: string): void {
  if (surfaceMode.value !== 'wysiwyg' || !wysiwygRef.value) return
  wysiwygRef.value.focus()
  document.execCommand(command, false, value)
  hasUnsavedEdits.value = true
  scheduleIdleMetricsUpdate()
}

function formatBlockTag(tag: 'P' | 'H1' | 'H2' | 'H3' | 'BLOCKQUOTE'): void {
  if (surfaceMode.value === 'wysiwyg') {
    execWysiwygCmd('formatBlock', `<${tag.toLowerCase()}>`)
  } else if (sourceTextareaRef.value) {
    const ta = sourceTextareaRef.value
    const start = ta.selectionStart
    const end = ta.selectionEnd
    const sel = sourceBuffer.value.slice(start, end) || '文本内容'
    const lower = tag.toLowerCase()
    const wrapped = `<${lower}>${sel}</${lower}>`
    sourceBuffer.value = `${sourceBuffer.value.slice(0, start)}${wrapped}${sourceBuffer.value.slice(end)}`
    hasUnsavedEdits.value = true
    scheduleIdleMetricsUpdate()
  }
}

function wrapInlineCode(): void {
  if (surfaceMode.value === 'wysiwyg') {
    const sel = window.getSelection()
    if (!sel || sel.rangeCount === 0) return
    const text = sel.toString()
    if (!text) return
    execWysiwygCmd('insertHTML', `<code>${escapeXml(text)}</code>`)
  } else if (sourceTextareaRef.value) {
    const ta = sourceTextareaRef.value
    const s = ta.selectionStart
    const e = ta.selectionEnd
    const text = sourceBuffer.value.slice(s, e)
    sourceBuffer.value = `${sourceBuffer.value.slice(0, s)}<code>${text}</code>${sourceBuffer.value.slice(e)}`
    hasUnsavedEdits.value = true
  }
}

function insertHorizontalRule(): void {
  if (surfaceMode.value === 'wysiwyg') {
    execWysiwygCmd('insertHorizontalRule')
  } else if (sourceTextareaRef.value) {
    const ta = sourceTextareaRef.value
    const pos = ta.selectionStart
    sourceBuffer.value = `${sourceBuffer.value.slice(0, pos)}\n  <hr/>\n${sourceBuffer.value.slice(pos)}`
    hasUnsavedEdits.value = true
    scheduleIdleMetricsUpdate()
  }
}

function triggerEpubImageUpload(): void {
  if (docFormat.value !== 'epub' || uploadingImage.value) return
  epubImageInputRef.value?.click()
}

async function requestClose(): Promise<void> {
  if (hasUnsavedEdits.value) {
    const ok = await dialogStore.confirm({
      title: '放弃未保存修改',
      message: '当前有尚未保存的目录或正文修改，确定退出工作台吗？',
      danger: true,
    })
    if (!ok) return
  }
  emit('close')
}

function handleWorkbenchKeydown(ev: KeyboardEvent): void {
  if (!props.open) return
  const mod = ev.metaKey || ev.ctrlKey
  const key = ev.key.toLowerCase()

  if (mod && key === 's') {
    ev.preventDefault()
    void handleSaveInPlace()
  } else if (mod && key === 'f') {
    ev.preventDefault()
    findBarOpen.value = true
    void nextTick(() => findBarRef.value?.focusFind())
  } else if (mod && ev.shiftKey && ev.key === 'Enter') {
    ev.preventDefault()
    void splitChapterAtCursor()
  } else if (ev.altKey && ev.key === 'ArrowUp') {
    ev.preventDefault()
    if (activeIndex.value > 0) void selectChapter(activeIndex.value - 1)
  } else if (ev.altKey && ev.key === 'ArrowDown') {
    ev.preventDefault()
    if (activeIndex.value < chapters.value.length - 1) void selectChapter(activeIndex.value + 1)
  }
}

watch(
  () => props.open,
  (isOpen) => {
    if (isOpen) {
      window.addEventListener('keydown', handleWorkbenchKeydown)
    } else {
      window.removeEventListener('keydown', handleWorkbenchKeydown)
    }
  },
  { immediate: true },
)

onBeforeUnmount(() => {
  window.removeEventListener('keydown', handleWorkbenchKeydown)
  clearIdleSyncTimer()
})
</script>

<template>
  <MdDialog
    :open="open && Boolean(entry)"
    max-width="max-w-[98vw] h-[94vh] !max-h-[94vh]"
    :close-on-backdrop="false"
    :hide-close="true"
    @close="requestClose"
  >
    <div
      v-if="entry"
      class="-mx-6 -my-2 flex h-[calc(100%+1rem)] flex-col overflow-hidden bg-[var(--md-sys-color-surface)] text-[var(--md-sys-color-on-surface)]"
    >
      <BookEditorHeader
        :title="docTitle || entry.display_title"
        :doc-format="docFormat"
        :chapter-count="chapters.length"
        :total-char-count="totalCharCount"
        :saved-badge="savedBadge"
        :has-unsaved-edits="hasUnsavedEdits"
        :saving="saving"
        :converting="converting"
        :polishing="polishing"
        :delete-source-txt="deleteSourceTxt"
        @update:delete-source-txt="deleteSourceTxt = $event"
        @save="handleSaveInPlace"
        @convert-to-epub="handleConvertToEpub"
        @polish="handlePolishInEditor"
        @close="requestClose"
      />

      <input
        ref="epubImageInputRef"
        type="file"
        accept="image/*"
        class="hidden"
        @change="onEpubImagePicked"
      />

      <div
        v-if="errorMsg"
        class="flex shrink-0 items-center justify-between gap-2 border-b border-[var(--md-sys-color-error)]/30 bg-[var(--md-sys-color-error-container)] px-4 py-2 text-xs text-[var(--md-sys-color-on-error-container)]"
      >
        <span>{{ errorMsg }}</span>
        <button
          type="button"
          class="md-state rounded p-0.5 text-[var(--md-sys-color-on-error-container)]"
          @click="errorMsg = null"
        >
          <LuX class="h-3.5 w-3.5" />
        </button>
      </div>

      <div
        v-if="loading"
        class="flex flex-1 flex-col items-center justify-center gap-2.5 text-xs text-[var(--md-sys-color-on-surface-variant)]"
      >
        <LuLoader2 class="h-6 w-6 animate-spin text-[var(--md-sys-color-primary)]" />
        <span>正在加载书籍目录结构与样式…</span>
      </div>

      <div v-else class="relative flex min-h-0 flex-1 overflow-hidden">
        <BookEditorTocSidebar
          :chapters="chapters"
          :active-index="activeIndex"
          :active-char-count="activeCharCount"
          :doc-format="docFormat"
          :resegmenting="resegmenting"
          :reset-key="tocResetKey"
          @select="selectChapter"
          @insert-below="insertChapterBelow"
          @insert-volume-below="insertVolumeBelow"
          @toggle-level="toggleLevel"
          @rename="handleRenameChapter"
          @merge-prev="mergeWithPrevious"
          @delete="deleteChapterAt"
          @reorder="handleReorderChapter"
          @auto-detect-hierarchy="autoDetectTocHierarchy"
          @flatten-levels="flattenAllTocLevels"
          @sync-titles-from-body="syncTocTitlesFromBodyHeadings"
          @resegment-txt="handleResegmentTxt"
        />

        <div class="flex min-w-0 flex-1 flex-col bg-[var(--md-sys-color-surface)]">
          <BookEditorToolbar
            :surface-mode="surfaceMode"
            :doc-format="docFormat"
            :uploading-image="uploadingImage"
            :proofread-panel-open="proofreadPanelOpen"
            :css-panel-open="cssPanelOpen"
            :find-bar-open="findBarOpen"
            :indent-paragraphs="indentParagraphs"
            :show-live-preview="showLivePreview"
            @exec-cmd="execWysiwygCmd"
            @format-block="formatBlockTag"
            @wrap-inline-code="wrapInlineCode"
            @insert-hr="insertHorizontalRule"
            @split-chapter="splitChapterAtCursor"
            @clean-typography="cleanChapterTypography"
            @upload-image="triggerEpubImageUpload"
            @toggle-proofread="proofreadPanelOpen = !proofreadPanelOpen; if (proofreadPanelOpen) cssPanelOpen = false"
            @toggle-css="cssPanelOpen = !cssPanelOpen; if (cssPanelOpen) proofreadPanelOpen = false"
            @toggle-find="findBarOpen = !findBarOpen"
            @toggle-indent="indentParagraphs = !indentParagraphs"
            @switch-surface-mode="switchSurfaceMode"
            @toggle-live-preview="toggleLivePreview"
          />

          <BookEditorProofreadPanel
            v-if="proofreadPanelOpen"
            :transform-scope="transformScope"
            :transforming="transforming"
            @update:transform-scope="transformScope = $event"
            @run-transform="(opts, label) => runBackendTextTransform(opts, label)"
            @close="proofreadPanelOpen = false"
          />

          <BookEditorCssPanel
            v-if="cssPanelOpen"
            :model-value="primaryCssDraft"
            @update:model-value="primaryCssDraft = $event; syncPrimaryCssDraftToStylesheets()"
            @apply-preset="(css, label) => applyCssPreset(css, label)"
            @close="cssPanelOpen = false"
          />

          <BookEditorFindReplaceBar
            v-if="findBarOpen"
            ref="findBarRef"
            :find-query="findQuery"
            :replace-query="replaceQuery"
            :find-scope="findScope"
            :find-case-sensitive="findCaseSensitive"
            :find-use-regex="findUseRegex"
            :search-match-summary="searchMatchSummary"
            @update:find-query="findQuery = $event"
            @update:replace-query="replaceQuery = $event"
            @update:find-scope="findScope = $event"
            @update:find-case-sensitive="findCaseSensitive = $event"
            @update:find-use-regex="findUseRegex = $event"
            @replace-single="handleReplaceSingle"
            @replace-all="handleReplaceAll"
            @close="findBarOpen = false"
          />

          <div class="flex shrink-0 items-center gap-3 border-b border-[var(--md-sys-color-outline-variant)]/60 px-6 py-2.5">
            <span class="shrink-0 font-mono text-[11px] text-[var(--md-sys-color-on-surface-variant)]">
              #{{ activeIndex + 1 }}
            </span>
            <input
              :value="activeTitle"
              type="text"
              placeholder="输入目录章节标题…"
              class="min-w-0 flex-1 bg-transparent text-base font-bold text-[var(--md-sys-color-on-surface)] outline-none placeholder:text-[var(--md-sys-color-on-surface-variant)]/50"
              @input="onActiveTitleInput(($event.target as HTMLInputElement).value)"
            />
            <span class="shrink-0 font-mono text-[11px] text-[var(--md-sys-color-on-surface-variant)]">
              本章 {{ activeCharCount.toLocaleString() }} 字
            </span>
          </div>

          <div class="relative min-h-0 flex-1 overflow-y-auto">
            <div v-show="surfaceMode === 'wysiwyg'" class="mx-auto max-w-3xl px-6 py-6 sm:px-10">
              <div
                ref="wysiwygRef"
                contenteditable="true"
                spellcheck="false"
                class="epub-wysiwyg-surface min-h-[65vh] outline-none"
                :class="indentParagraphs ? 'indent-paras' : ''"
                @focus="onWysiwygFocus"
                @input="onWysiwygInput"
                @compositionstart="isComposing = true"
                @compositionend="onWysiwygCompositionEnd"
              />
            </div>

            <textarea
              v-show="surfaceMode === 'xhtml'"
              ref="sourceTextareaRef"
              v-model="sourceBuffer"
              spellcheck="false"
              placeholder="在此编辑章节 <body> 内的 XHTML 源码…"
              class="h-full w-full resize-none bg-transparent p-5 font-mono text-xs leading-relaxed text-[var(--md-sys-color-on-surface)] outline-none"
              @input="onSourceTextareaInput"
              @keydown="onSourceKeydown"
            />
          </div>
        </div>

        <aside
          v-if="showLivePreview"
          class="hidden w-[38%] max-w-xl shrink-0 flex-col border-l border-[var(--md-sys-color-outline-variant)] bg-[var(--md-sys-color-surface-container-low)] lg:flex"
        >
          <div class="flex h-9 shrink-0 items-center justify-between border-b border-[var(--md-sys-color-outline-variant)]/70 px-3.5 text-xs">
            <span class="font-semibold text-[var(--md-sys-color-on-surface)]">阅读器实时排版预览</span>
            <span class="text-[10px] text-[var(--md-sys-color-on-surface-variant)]">已加载包内 CSS 与原图</span>
          </div>
          <iframe
            :srcdoc="previewSrcdoc"
            title="EPUB Live Preview"
            sandbox=""
            class="min-h-0 w-full flex-1 border-0 bg-[var(--md-sys-color-surface)]"
          />
        </aside>
      </div>
    </div>
  </MdDialog>
</template>

<style scoped>
@reference "tailwindcss";

.epub-wysiwyg-surface {
  font-family: "Noto Serif CJK SC", "Songti SC", "PingFang SC", "Microsoft YaHei", Georgia, serif;
  font-size: 16px;
  line-height: 1.9;
  color: var(--md-sys-color-on-surface);
  word-break: break-word;
}

.epub-wysiwyg-surface :deep(h1) {
  font-size: 1.6em;
  font-weight: 800;
  line-height: 1.35;
  margin: 0.6em 0 0.75em;
}

.epub-wysiwyg-surface :deep(h2) {
  font-size: 1.35em;
  font-weight: 700;
  line-height: 1.4;
  margin: 0.6em 0 0.7em;
}

.epub-wysiwyg-surface :deep(h3) {
  font-size: 1.15em;
  font-weight: 700;
  line-height: 1.45;
  margin: 0.6em 0 0.6em;
}

.epub-wysiwyg-surface :deep(p),
.epub-wysiwyg-surface :deep(div) {
  margin: 0 0 0.85em;
  text-align: justify;
}

.epub-wysiwyg-surface.indent-paras :deep(p) {
  text-indent: 2em;
}

.epub-wysiwyg-surface :deep(blockquote) {
  margin: 1em 0;
  padding: 0.5em 1em;
  border-left: 3px solid var(--md-sys-color-primary);
  background: var(--md-sys-color-surface-container-low);
  border-radius: 0 0.5rem 0.5rem 0;
}

.epub-wysiwyg-surface :deep(code) {
  font-family: ui-monospace, SFMono-Regular, Menlo, Monaco, Consolas, monospace;
  font-size: 0.88em;
  padding: 0.15em 0.4em;
  border-radius: 0.35rem;
  background: var(--md-sys-color-surface-container-high);
}

.epub-wysiwyg-surface :deep(hr) {
  border: none;
  border-top: 1px solid var(--md-sys-color-outline-variant);
  margin: 1.5em 0;
}

.epub-wysiwyg-surface :deep(img),
.epub-wysiwyg-surface :deep(svg) {
  max-width: 100%;
  height: auto;
  border-radius: 0.5rem;
  margin: 0.75em auto;
  display: block;
}
</style>

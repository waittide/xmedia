<script setup lang="ts">
import {
  computed,
  nextTick,
  onBeforeUnmount,
  ref,
  shallowRef,
  triggerRef,
  watch,
} from 'vue'
import {
  LuBookOpen,
  LuBold,
  LuCheck,
  LuCode,
  LuEraser,
  LuEye,
  LuEyeOff,
  LuHeading1,
  LuHeading2,
  LuHeading3,
  LuImage,
  LuItalic,
  LuKeyboard,
  LuLanguages,
  LuLoader2,
  LuMinus,
  LuPalette,
  LuPilcrow,
  LuQuote,
  LuRedo2,
  LuRefreshCw,
  LuSave,
  LuScissors,
  LuSearch,
  LuSparkles,
  LuUnderline,
  LuUndo2,
  LuWand2,
  LuX,
} from 'vue-icons-plus/lu'
import AppCheckbox from '@/components/ui/AppCheckbox.vue'
import MdDialog from '@/components/ui/MdDialog.vue'
import MdSegmentedButton from '@/components/ui/MdSegmentedButton.vue'
import { useDialogStore } from '@/stores/dialog'
import { useSnackbarStore } from '@/stores/snackbar'
import { useThemeStore } from '@/stores/theme'
import { buildEpubAssetUrl, toolApi } from '../api'
import BookEditorCssPanel, { cssPresets } from './book-editor/BookEditorCssPanel.vue'
import BookEditorFindReplaceBar from './book-editor/BookEditorFindReplaceBar.vue'
import BookEditorProofreadPanel from './book-editor/BookEditorProofreadPanel.vue'
import BookEditorTocSidebar from './book-editor/BookEditorTocSidebar.vue'
import {
  countNonWhitespace,
  escapeXml,
  extractPlainFromXhtml,
  hydrateXhtmlForBrowser as hydrateXhtmlForBrowserRaw,
  plainToCleanXhtml,
  replaceInTextNodes,
  serializeContainerToValidXhtml,
} from './book-editor/xhtmlUtils'
import type {
  BookTextTransformOptions,
  EditableBookChapterDto,
  MediaFileDetail,
  MediaFileEntry,
  SaveBookChapterInput,
} from '../types'

interface ChapterRecord {
  key: string
  title: string
  level: number
  xhtmlBody: string
  plainCache: string
  charCount: number
  hasRichMedia: boolean
  sourceHref: string | null
}

type SurfaceMode = 'wysiwyg' | 'xhtml'

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

const surfaceModeOptions = [
  { value: 'wysiwyg', label: '所见即所得' },
  { value: 'xhtml', label: 'XHTML 源码' },
] as const

const SHORTCUT_HELP_TEXT =
  '快捷键：Ctrl+S 原地保存 · Ctrl+F 查找替换 · Ctrl+Shift+Enter 光标拆章 · Alt+↑/↓ 切换章节 · Esc 退出'

const loading = ref(false)
const saving = ref(false)
const converting = ref(false)
const resegmenting = ref(false)
const transforming = ref(false)
const polishing = ref(false)
const uploadingImage = ref(false)
const errorMsg = ref<string | null>(null)
const savedBadge = ref(false)

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
let isComposing = false
let idleSyncTimer: ReturnType<typeof setTimeout> | null = null
let savedBadgeTimer: ReturnType<typeof setTimeout> | null = null
let keySeq = 1

// TOC sidebar reset counter (incremented on document reload)
const tocResetKey = ref(0)
const deleteSourceTxt = ref(false)

// Find & Replace Bar state
const findBarOpen = ref(false)
const findQuery = ref('')
const replaceQuery = ref('')
const findScope = ref<'chapter' | 'book'>('chapter')
const findUseRegex = ref(false)
const findCaseSensitive = ref(false)
const findBarRef = ref<InstanceType<typeof BookEditorFindReplaceBar> | null>(null)

// Proofreading & Chinese Conversion Toolbox + CSS Theme Drawer + Image Upload
const proofreadPanelOpen = ref(false)
const cssPanelOpen = ref(false)
const transformScope = ref<'book' | 'chapter'>('book')
const epubImageInputRef = ref<HTMLInputElement | null>(null)
function showToast(msg: string): void {
  snackbar.show(msg)
}

function readRootCssToken(name: string, fallback: string): string {
  if (typeof window === 'undefined') return fallback
  const val = window.getComputedStyle(document.documentElement).getPropertyValue(name).trim()
  return val || fallback
}

function hydrateXhtmlForBrowser(xhtmlBody: string, sourceHref: string | null): string {
  const entry = props.entry
  return hydrateXhtmlForBrowserRaw(
    xhtmlBody,
    sourceHref,
    entry && docFormat.value === 'epub' ? { rootId: entry.root_id, relPath: entry.rel_path } : null,
  )
}
function buildLivePreviewSrcdoc(xhtmlBody: string, sourceHref: string | null): string {
  const hydrated = hydrateXhtmlForBrowser(xhtmlBody, sourceHref)
  const bg = readRootCssToken('--md-sys-color-surface', 'Canvas')
  const fg = readRootCssToken('--md-sys-color-on-surface', 'CanvasText')
  const primary = readRootCssToken('--md-sys-color-primary', 'Highlight')
  const outline = readRootCssToken('--md-sys-color-outline-variant', 'GrayText')
  const epubCss = docStylesheets.value.join('\n')
  const indentRule = indentParagraphs.value ? 'text-indent: 2em;' : 'text-indent: 0;'

  return `<!DOCTYPE html>
<html>
<head>
<meta charset="utf-8"/>
<style>
  html, body {
    margin: 0;
    padding: 20px 26px 48px;
    background: ${bg};
    color: ${fg};
    font-family: "Noto Serif CJK SC", "Songti SC", "PingFang SC", Georgia, serif;
    font-size: 16px;
    line-height: 1.85;
    word-break: break-word;
  }
  h1, h2, h3, h4 { line-height: 1.35; margin: 1em 0 0.7em; }
  p { margin: 0 0 0.8em; ${indentRule} text-align: justify; }
  img, svg { max-width: 100%; height: auto; border-radius: 6px; }
  blockquote { margin: 1em 0; padding: 0.4em 1em; border-left: 3px solid ${primary}; opacity: 0.9; }
  hr { border: none; border-top: 1px solid ${outline}; margin: 1.5em 0; }
  ${epubCss}
</style>
</head>
<body>${hydrated}</body>
</html>`
}

function mapDtoList(dtos: EditableBookChapterDto[]): ChapterRecord[] {
  return dtos.map((c) => {
    const xhtml = c.xhtml_body?.trim() ? c.xhtml_body : plainToCleanXhtml(c.title, c.plain_text)
    const plain = c.plain_text || extractPlainFromXhtml(xhtml, c.title)
    return {
      key: `ch_${keySeq++}_${c.id}`,
      title: c.title,
      level: c.level > 0 ? 1 : 0,
      xhtmlBody: xhtml,
      plainCache: plain,
      charCount: countNonWhitespace(plain),
      hasRichMedia: c.has_rich_media,
      sourceHref: c.source_href,
    }
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

/**
 * Flushes the active editor DOM / textarea buffer back into `chapters.value[activeIndex]`
 * without re-mounting the DOM.
 */
function flushActiveSurfaceToRecord(): void {
  if (idleSyncTimer) {
    clearTimeout(idleSyncTimer)
    idleSyncTimer = null
  }
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

function scheduleIdleMetricsUpdate(): void {
  if (idleSyncTimer) clearTimeout(idleSyncTimer)
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

// ==================== WYSIWYG Formatting Commands ====================

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

/**
 * One-click Clean Typography:
 * Removes empty paragraphs, trims leading full-width spaces `　　` / junk spaces, and normalizes paragraphs.
 */
async function cleanChapterTypography(): Promise<void> {
  const rec = currentRecord.value
  if (!rec) return
  flushActiveSurfaceToRecord()

  const doc = new DOMParser().parseFromString(`<body>${rec.xhtmlBody}</body>`, 'text/html')
  const body = doc.body
  if (!body) return

  body.querySelectorAll('p, div, li').forEach((el) => {
    // Strip leading full-width / half-width whitespace on first text node
    const first = el.firstChild
    if (first && first.nodeType === Node.TEXT_NODE && first.nodeValue) {
      first.nodeValue = first.nodeValue.replace(/^[\s\u3000\u00A0]+/, '')
    }
    const last = el.lastChild
    if (last && last.nodeType === Node.TEXT_NODE && last.nodeValue) {
      last.nodeValue = last.nodeValue.replace(/[\s\u3000\u00A0]+$/, '')
    }
    const hasMedia = el.querySelector('img, svg, image, hr, table') !== null
    if (!hasMedia && !(el.textContent ?? '').trim()) {
      el.remove()
    }
  })

  rec.xhtmlBody = serializeContainerToValidXhtml(body) || '  <p><br/></p>'
  rec.plainCache = extractPlainFromXhtml(rec.xhtmlBody, rec.title)
  rec.charCount = countNonWhitespace(rec.plainCache)
  hasUnsavedEdits.value = true
  triggerRef(chapters)
  await mountActiveChapterIntoSurface()
  showToast('已完成本章排版净化（清理空段与段首冗余空格）')
}

// ==================== Split Chapter at Cursor (WYSIWYG & XHTML) ====================

async function splitChapterAtCursor(): Promise<void> {
  const rec = currentRecord.value
  if (!rec) return

  if (surfaceMode.value === 'wysiwyg' && wysiwygRef.value) {
    const root = wysiwygRef.value
    const sel = window.getSelection()
    if (!sel || sel.rangeCount === 0 || !root.contains(sel.anchorNode)) {
      showToast('请先在正文中点击需要拆分章节的光标位置')
      return
    }
    const caretRange = sel.getRangeAt(0)
    const tailRange = document.createRange()
    tailRange.setStart(caretRange.startContainer, caretRange.startOffset)
    tailRange.setEnd(root, root.childNodes.length)

    const tailFragment = tailRange.extractContents()
    const headXhtml = serializeContainerToValidXhtml(root)
    const tempDiv = document.createElement('div')
    tempDiv.appendChild(tailFragment)
    const tailXhtml = serializeContainerToValidXhtml(tempDiv)

    if (!tailXhtml.trim()) {
      // Restore if nothing after cursor
      await mountActiveChapterIntoSurface()
      showToast('光标后没有可拆分的内容')
      return
    }

    // Infer title if first block of tail is a short heading/paragraph
    const firstEl = tempDiv.firstElementChild
    let nextTitle = `${rec.title} (续)`
    if (firstEl && /^H[1-4]$/.test(firstEl.tagName)) {
      const headingText = (firstEl.textContent ?? '').trim()
      if (headingText && headingText.length <= 60) {
        nextTitle = headingText
      }
    }

    rec.xhtmlBody = headXhtml || '  <p><br/></p>'
    rec.plainCache = extractPlainFromXhtml(rec.xhtmlBody, rec.title)
    rec.charCount = countNonWhitespace(rec.plainCache)

    const tailPlain = extractPlainFromXhtml(tailXhtml, nextTitle)
    const newRec: ChapterRecord = {
      key: `ch_split_${keySeq++}`,
      title: nextTitle,
      level: rec.level,
      xhtmlBody: tailXhtml,
      plainCache: tailPlain,
      charCount: countNonWhitespace(tailPlain),
      hasRichMedia: /<(?:img|svg|image|table)\b/i.test(tailXhtml),
      sourceHref: null,
    }

    const nextList = [...chapters.value]
    nextList.splice(activeIndex.value + 1, 0, newRec)
    chapters.value = nextList
    activeIndex.value += 1
    hasUnsavedEdits.value = true
    await mountActiveChapterIntoSurface()
    showToast(`已拆分为新章节「${nextTitle}」`)
    return
  }

  if (surfaceMode.value === 'xhtml' && sourceTextareaRef.value) {
    const ta = sourceTextareaRef.value
    const pos = ta.selectionStart ?? 0
    if (pos <= 0 || pos >= sourceBuffer.value.length) {
      showToast('请先将光标放在源码中间需要拆分的位置')
      return
    }
    const before = sourceBuffer.value.slice(0, pos).trimEnd()
    const after = sourceBuffer.value.slice(pos).trimStart()
    rec.xhtmlBody = before || '  <p><br/></p>'
    rec.plainCache = extractPlainFromXhtml(rec.xhtmlBody, rec.title)
    rec.charCount = countNonWhitespace(rec.plainCache)

    const nextTitle = `${rec.title} (续)`
    const tailPlain = extractPlainFromXhtml(after, nextTitle)
    const newRec: ChapterRecord = {
      key: `ch_split_${keySeq++}`,
      title: nextTitle,
      level: rec.level,
      xhtmlBody: after,
      plainCache: tailPlain,
      charCount: countNonWhitespace(tailPlain),
      hasRichMedia: /<(?:img|svg|image|table)\b/i.test(after),
      sourceHref: null,
    }

    const nextList = [...chapters.value]
    nextList.splice(activeIndex.value + 1, 0, newRec)
    chapters.value = nextList
    activeIndex.value += 1
    hasUnsavedEdits.value = true
    await mountActiveChapterIntoSurface()
    showToast(`已拆分为新章节「${nextTitle}」`)
  }
}

// ==================== TOC Tree Operations & Drag-and-Drop ====================

function handleRenameChapter(idx: number, newTitle: string): void {
  const rec = chapters.value[idx]
  const clean = newTitle.trim()
  if (rec && clean && clean !== rec.title) {
    rec.title = clean
    if (idx === activeIndex.value) {
      activeTitle.value = clean
    }
    hasUnsavedEdits.value = true
    triggerRef(chapters)
  }
}

function toggleLevel(idx: number): void {
  const rec = chapters.value[idx]
  if (!rec) return
  rec.level = rec.level === 0 ? 1 : 0
  hasUnsavedEdits.value = true
  triggerRef(chapters)
}

const VOLUME_TITLE_RE =
  /^\s*(?:第\s*[零一二三四五六七八九十百千万两〇廿卅卌壹贰叁肆伍陆柒捌玖拾佰仟0-9０-９]+\s*[卷部集册篇](?:[\s：:、—\-].*)?|卷\s*[零一二三四五六七八九十百千万两〇廿卅卌壹贰叁肆伍陆柒捌玖拾佰仟0-9０-９]+.*|(?:volume|vol\.?|book|part)\s+[0-9ivxlcdm]+.*)$/i

function isVolumeRecord(list: ChapterRecord[], idx: number): boolean {
  const rec = list[idx]
  if (!rec || rec.level !== 0 || rec.title === '书籍简介') return false
  const next = list[idx + 1]
  return Boolean(next && next.level > 0) || VOLUME_TITLE_RE.test(rec.title.trim())
}

function autoDetectTocHierarchy(): void {
  flushActiveSurfaceToRecord()
  let seenVol = false
  let volCount = 0
  for (const ch of chapters.value) {
    if (ch.title === '书籍简介') {
      ch.level = 0
      continue
    }
    if (VOLUME_TITLE_RE.test(ch.title.trim())) {
      ch.level = 0
      seenVol = true
      volCount += 1
    } else {
      ch.level = seenVol ? 1 : 0
    }
  }
  hasUnsavedEdits.value = true
  triggerRef(chapters)
  showToast(
    volCount > 0
      ? `已自动识别 ${volCount} 个分卷并缩进下属章节`
      : '未检测到显式「第X卷/部」标题，已保持一级平铺',
  )
}

function flattenAllTocLevels(): void {
  flushActiveSurfaceToRecord()
  for (const ch of chapters.value) {
    ch.level = 0
  }
  hasUnsavedEdits.value = true
  triggerRef(chapters)
  showToast('已将全部章节平铺为一级目录')
}

function syncTocTitlesFromBodyHeadings(): void {
  flushActiveSurfaceToRecord()
  let updated = 0
  for (let i = 0; i < chapters.value.length; i += 1) {
    const ch = chapters.value[i]
    if (!ch) continue
    const doc = new DOMParser().parseFromString(`<body>${ch.xhtmlBody}</body>`, 'text/html')
    const h = doc.body.querySelector('h1, h2, h3')
    const text = (h?.textContent ?? '').replace(/\s+/g, ' ').trim()
    if (text && text.length <= 80 && text !== ch.title) {
      ch.title = text
      if (i === activeIndex.value) {
        activeTitle.value = text
      }
      updated += 1
    }
  }
  if (updated > 0) {
    hasUnsavedEdits.value = true
    triggerRef(chapters)
  }
  showToast(updated > 0 ? `已从正文标题同步更新 ${updated} 个章节名` : '目录标题已与正文标题一致')
}

function applyCssPreset(cssCode: string, label: string): void {
  primaryCssDraft.value = cssCode
  syncPrimaryCssDraftToStylesheets()
  showToast(`已应用「${label}」排版样式预设`)
}

function syncPrimaryCssDraftToStylesheets(): void {
  const next = [...docStylesheets.value]
  if (next.length === 0) {
    next.push(primaryCssDraft.value)
  } else {
    next[0] = primaryCssDraft.value
  }
  docStylesheets.value = next
  hasUnsavedEdits.value = true
  if (showLivePreview.value && currentRecord.value) {
    flushActiveSurfaceToRecord()
    previewSrcdoc.value = buildLivePreviewSrcdoc(
      currentRecord.value.xhtmlBody,
      currentRecord.value.sourceHref,
    )
  }
}

async function runBackendTextTransform(
  options: BookTextTransformOptions,
  summaryLabel: string,
): Promise<void> {
  if (transforming.value || chapters.value.length === 0) return
  flushActiveSurfaceToRecord()
  transforming.value = true
  errorMsg.value = null
  try {
    const targetIndices =
      transformScope.value === 'chapter' && !options.vertical_to_horizontal
        ? [activeIndex.value]
        : chapters.value.map((_, i) => i)

    const inputDtos: EditableBookChapterDto[] = targetIndices.map((idx) => {
      const ch = chapters.value[idx]!
      return {
        id: ch.key,
        title: ch.title,
        level: ch.level,
        plain_text: ch.plainCache,
        xhtml_body: ch.xhtmlBody,
        has_rich_media: ch.hasRichMedia,
        source_href: ch.sourceHref,
      }
    })

    const res = await toolApi.transformBookContent(inputDtos, docStylesheets.value, options)
    res.chapters.forEach((dto, pos) => {
      const idx = targetIndices[pos]
      const rec = idx !== undefined ? chapters.value[idx] : undefined
      if (!rec) return
      rec.title = dto.title
      rec.xhtmlBody = dto.xhtml_body
      rec.plainCache = extractPlainFromXhtml(dto.xhtml_body, dto.title)
      rec.charCount = countNonWhitespace(rec.plainCache)
      if (idx === activeIndex.value) {
        activeTitle.value = rec.title
      }
    })

    if (options.vertical_to_horizontal) {
      docStylesheets.value = res.stylesheets
      primaryCssDraft.value = res.stylesheets[0] ?? primaryCssDraft.value
      forceHorizontalSpine.value = true
    }

    hasUnsavedEdits.value = true
    triggerRef(chapters)
    await mountActiveChapterIntoSurface()
    showToast(`${summaryLabel}完成（共更新 ${res.changed_chapters} 章）`)
  } catch (err) {
    errorMsg.value = err instanceof Error ? err.message : String(err)
  } finally {
    transforming.value = false
  }
}

function triggerEpubImageUpload(): void {
  if (docFormat.value !== 'epub' || uploadingImage.value) return
  epubImageInputRef.value?.click()
}

async function onEpubImagePicked(ev: Event): Promise<void> {
  const input = ev.target as HTMLInputElement
  const file = input.files?.[0]
  input.value = ''
  if (!file || !props.entry || !currentRecord.value) return

  uploadingImage.value = true
  errorMsg.value = null
  try {
    flushActiveSurfaceToRecord()
    const inserted = await toolApi.uploadBookEpubImage(
      props.entry.root_id,
      props.entry.rel_path,
      file,
      currentRecord.value.sourceHref,
    )
    const browserUrl = buildEpubAssetUrl(
      props.entry.root_id,
      props.entry.rel_path,
      inserted.asset_path,
    )
    const altText = escapeXml(file.name.replace(/\.[^.]+$/, '') || '插图')

    if (surfaceMode.value === 'wysiwyg' && wysiwygRef.value) {
      wysiwygRef.value.focus()
      document.execCommand(
        'insertHTML',
        false,
        `<figure><img src="${escapeXml(browserUrl)}" data-epub-src="${escapeXml(inserted.chapter_rel_href)}" alt="${altText}"/></figure><p><br/></p>`,
      )
      flushActiveSurfaceToRecord()
    } else {
      const snippet = `\n  <figure><img src="${escapeXml(inserted.chapter_rel_href)}" alt="${altText}"/></figure>\n`
      if (sourceTextareaRef.value) {
        const pos = sourceTextareaRef.value.selectionStart ?? sourceBuffer.value.length
        sourceBuffer.value = `${sourceBuffer.value.slice(0, pos)}${snippet}${sourceBuffer.value.slice(pos)}`
      } else {
        sourceBuffer.value = `${sourceBuffer.value}${snippet}`
      }
      flushActiveSurfaceToRecord()
    }

    currentRecord.value.hasRichMedia = true
    hasUnsavedEdits.value = true
    triggerRef(chapters)
    if (showLivePreview.value) {
      previewSrcdoc.value = buildLivePreviewSrcdoc(
        currentRecord.value.xhtmlBody,
        currentRecord.value.sourceHref,
      )
    }
    showToast('图片已写入 EPUB 资源包并插入当前章节')
  } catch (err) {
    errorMsg.value = err instanceof Error ? err.message : String(err)
  } finally {
    uploadingImage.value = false
  }
}

async function handlePolishInEditor(): Promise<void> {
  if (!props.entry || docFormat.value !== 'epub' || polishing.value || saving.value) return
  polishing.value = true
  errorMsg.value = null
  try {
    if (hasUnsavedEdits.value && chapters.value.length > 0) {
      await toolApi.saveBookContent(
        props.entry.root_id,
        props.entry.rel_path,
        buildSavePayload(),
        docStylesheets.value.length > 0 ? docStylesheets.value : undefined,
        forceHorizontalSpine.value,
      )
      hasUnsavedEdits.value = false
      forceHorizontalSpine.value = false
    }
    const res = await toolApi.polishBook(props.entry.root_id, props.entry.rel_path)
    emit('saved', res.detail)
    await loadDocument()
    showToast(
      res.removed_orphan_files > 0
        ? `抛光完成：已生成内封简介页并清理 ${res.removed_orphan_files} 个冗余资源`
        : '抛光完成：已更新内封简介页并同步双目录',
    )
  } catch (err) {
    errorMsg.value = err instanceof Error ? err.message : String(err)
  } finally {
    polishing.value = false
  }
}

async function insertVolumeBelow(idx: number): Promise<void> {
  flushActiveSurfaceToRecord()
  const currentList = chapters.value
  let existingVolCount = 0
  for (let i = 0; i < currentList.length; i += 1) {
    if (isVolumeRecord(currentList, i)) existingVolCount += 1
  }
  const title = `第${existingVolCount + 1}卷`
  const newRec: ChapterRecord = {
    key: `vol_new_${keySeq++}`,
    title,
    level: 0,
    xhtmlBody: `  <h1>${escapeXml(title)}</h1>\n  <p><br/></p>`,
    plainCache: '',
    charCount: 0,
    hasRichMedia: false,
    sourceHref: null,
  }
  const nextList = [...currentList]
  const insertAt =
    nextList.length === 0 ? 0 : Math.min(nextList.length, Math.max(0, idx + 1))
  nextList.splice(insertAt, 0, newRec)

  // Automatically nest subsequent flat chapters until the next volume header
  let adoptedCount = 0
  for (let i = insertAt + 1; i < nextList.length; i += 1) {
    const ch = nextList[i]!
    if (ch.title === '书籍简介' || isVolumeRecord(nextList, i)) break
    if (ch.level === 0) {
      ch.level = 1
      adoptedCount += 1
    }
  }

  chapters.value = nextList
  activeIndex.value = insertAt
  hasUnsavedEdits.value = true
  await mountActiveChapterIntoSurface()
  showToast(
    adoptedCount > 0
      ? `已插入「${title}」，并将后续 ${adoptedCount} 章归入该卷`
      : `已插入「${title}」（可在目录双击重命名或拖拽排序）`,
  )
}

async function insertChapterBelow(idx: number): Promise<void> {
  flushActiveSurfaceToRecord()
  const cur = chapters.value[idx]
  const refLevel = cur
    ? isVolumeRecord(chapters.value, idx)
      ? 1
      : cur.level
    : 0
  const title = `新章节 ${chapters.value.length + 1}`
  const newRec: ChapterRecord = {
    key: `ch_new_${keySeq++}`,
    title,
    level: refLevel,
    xhtmlBody: `  <h2>${escapeXml(title)}</h2>\n  <p><br/></p>`,
    plainCache: '',
    charCount: 0,
    hasRichMedia: false,
    sourceHref: null,
  }
  const nextList = [...chapters.value]
  const insertAt =
    nextList.length === 0 ? 0 : Math.min(nextList.length, Math.max(0, idx + 1))
  nextList.splice(insertAt, 0, newRec)
  chapters.value = nextList
  activeIndex.value = insertAt
  hasUnsavedEdits.value = true
  await mountActiveChapterIntoSurface()
}

async function mergeWithPrevious(idx: number): Promise<void> {
  if (idx <= 0 || idx >= chapters.value.length) return
  flushActiveSurfaceToRecord()
  const prev = chapters.value[idx - 1]!
  const cur = chapters.value[idx]!

  prev.xhtmlBody = [prev.xhtmlBody.trim(), cur.xhtmlBody.trim()].filter(Boolean).join('\n\n')
  prev.plainCache = extractPlainFromXhtml(prev.xhtmlBody, prev.title)
  prev.charCount = countNonWhitespace(prev.plainCache)
  prev.hasRichMedia = prev.hasRichMedia || cur.hasRichMedia

  const nextList = [...chapters.value]
  nextList.splice(idx, 1)
  chapters.value = nextList
  if (activeIndex.value >= idx) {
    activeIndex.value = Math.max(0, activeIndex.value - 1)
  }
  hasUnsavedEdits.value = true
  await mountActiveChapterIntoSurface()
  showToast(`已将「${cur.title}」合并至上一章`)
}

async function deleteChapterAt(idx: number): Promise<void> {
  if (chapters.value.length <= 1) {
    showToast('图书至少需要保留一个章节')
    return
  }
  const target = chapters.value[idx]
  if (!target) return
  const ok = await dialogStore.confirm({
    title: '删除章节',
    message: `确定删除章节「${target.title}」吗？`,
    danger: true,
  })
  if (!ok) return
  flushActiveSurfaceToRecord()
  const nextList = [...chapters.value]
  nextList.splice(idx, 1)
  chapters.value = nextList
  if (activeIndex.value >= nextList.length) {
    activeIndex.value = nextList.length - 1
  }
  hasUnsavedEdits.value = true
  await mountActiveChapterIntoSurface()
}

async function handleReorderChapter(
  from: number,
  targetIdx: number,
  pos: 'before' | 'after',
): Promise<void> {
  if (from === targetIdx) return

  flushActiveSurfaceToRecord()
  const activeKey = chapters.value[activeIndex.value]?.key
  const nextList = [...chapters.value]
  const [moved] = nextList.splice(from, 1)
  if (!moved) return

  let insertAt = targetIdx
  if (from < targetIdx) insertAt -= 1
  if (pos === 'after') insertAt += 1
  insertAt = Math.max(0, Math.min(nextList.length, insertAt))

  nextList.splice(insertAt, 0, moved)
  chapters.value = nextList
  if (activeKey) {
    const newActive = nextList.findIndex((c) => c.key === activeKey)
    if (newActive >= 0) activeIndex.value = newActive
  }
  hasUnsavedEdits.value = true
}

// ==================== Find & Replace (Chapter or Whole Book) ====================

function buildSearchRegex(): RegExp | null {
  const q = findQuery.value
  if (!q) return null
  try {
    const pattern = findUseRegex.value ? q : q.replace(/[.*+?^${}()|[\]\\]/g, '\\$&')
    const flags = findCaseSensitive.value ? 'g' : 'gi'
    return new RegExp(pattern, flags)
  } catch {
    return null
  }
}

const searchMatchSummary = computed<string>(() => {
  const re = buildSearchRegex()
  if (!re) return ''
  if (findScope.value === 'chapter') {
    const rec = currentRecord.value
    if (!rec) return '0 处'
    const hay =
      surfaceMode.value === 'xhtml'
        ? sourceBuffer.value
        : (wysiwygRef.value?.innerText ?? rec.plainCache)
    const count = Array.from(hay.matchAll(re)).length
    return `本章 ${count} 处`
  }
  let total = 0
  let chapHits = 0
  for (const ch of chapters.value) {
    const target = surfaceMode.value === 'xhtml' ? ch.xhtmlBody : ch.plainCache
    const m = Array.from(target.matchAll(re)).length
    if (m > 0) {
      total += m
      chapHits += 1
    }
  }
  return `全书 ${chapHits} 章共 ${total} 处`
})

async function handleReplaceSingle(): Promise<void> {
  const re = buildSearchRegex()
  if (!re || !currentRecord.value) return
  flushActiveSurfaceToRecord()

  if (surfaceMode.value === 'xhtml') {
    const singleRe = new RegExp(re.source, re.flags.replace('g', ''))
    const next = sourceBuffer.value.replace(singleRe, replaceQuery.value)
    if (next !== sourceBuffer.value) {
      sourceBuffer.value = next
      flushActiveSurfaceToRecord()
      hasUnsavedEdits.value = true
      showToast('已替换 1 处')
    }
    return
  }

  const doc = new DOMParser().parseFromString(`<body>${currentRecord.value.xhtmlBody}</body>`, 'text/html')
  const count = replaceInTextNodes(doc.body, re, replaceQuery.value, true)
  if (count > 0) {
    currentRecord.value.xhtmlBody = serializeContainerToValidXhtml(doc.body) || '  <p><br/></p>'
    currentRecord.value.plainCache = extractPlainFromXhtml(
      currentRecord.value.xhtmlBody,
      currentRecord.value.title,
    )
    currentRecord.value.charCount = countNonWhitespace(currentRecord.value.plainCache)
    hasUnsavedEdits.value = true
    triggerRef(chapters)
    await mountActiveChapterIntoSurface()
    showToast('已替换 1 处')
  }
}

async function handleReplaceAll(): Promise<void> {
  const re = buildSearchRegex()
  if (!re) return
  flushActiveSurfaceToRecord()

  const indices =
    findScope.value === 'chapter'
      ? [activeIndex.value]
      : chapters.value.map((_, idx) => idx)

  let totalReplaced = 0
  for (const idx of indices) {
    const ch = chapters.value[idx]
    if (!ch) continue
    if (surfaceMode.value === 'xhtml') {
      const matches = Array.from(ch.xhtmlBody.matchAll(re)).length
      if (matches > 0) {
        ch.xhtmlBody = ch.xhtmlBody.replace(re, replaceQuery.value)
        ch.plainCache = extractPlainFromXhtml(ch.xhtmlBody, ch.title)
        ch.charCount = countNonWhitespace(ch.plainCache)
        totalReplaced += matches
      }
    } else {
      const doc = new DOMParser().parseFromString(`<body>${ch.xhtmlBody}</body>`, 'text/html')
      const count = replaceInTextNodes(doc.body, re, replaceQuery.value, false)
      if (count > 0) {
        ch.xhtmlBody = serializeContainerToValidXhtml(doc.body) || '  <p><br/></p>'
        ch.plainCache = extractPlainFromXhtml(ch.xhtmlBody, ch.title)
        ch.charCount = countNonWhitespace(ch.plainCache)
        totalReplaced += count
      }
    }
  }

  if (totalReplaced > 0) {
    hasUnsavedEdits.value = true
    triggerRef(chapters)
    await mountActiveChapterIntoSurface()
  }
  showToast(totalReplaced > 0 ? `共替换 ${totalReplaced} 处匹配内容` : '未找到可替换的匹配项')
}

// ==================== Load, Save (In-Place Ctrl+S), Resegment & Convert ====================

async function loadDocument(): Promise<void> {
  if (!props.entry) return
  loading.value = true
  errorMsg.value = null
  hasUnsavedEdits.value = false
  savedBadge.value = false
  tocResetKey.value += 1
  forceHorizontalSpine.value = false

  try {
    const doc = await toolApi.getBookContent(props.entry.root_id, props.entry.rel_path)
    docFormat.value = doc.format
    docTitle.value = doc.title
    docAuthors.value = doc.authors
    docStylesheets.value = doc.stylesheets ?? []
    primaryCssDraft.value = doc.stylesheets?.[0] ?? cssPresets[0]!.css
    chapters.value = mapDtoList(doc.chapters)
    activeIndex.value = 0
    surfaceMode.value = 'wysiwyg'
    loading.value = false
    await mountActiveChapterIntoSurface()
  } catch (err) {
    errorMsg.value = err instanceof Error ? err.message : String(err)
    chapters.value = []
  } finally {
    loading.value = false
  }
}

watch(
  () => [props.open, props.entry?.rel_path] as const,
  ([isOpen]) => {
    if (isOpen && props.entry) {
      deleteSourceTxt.value = false
      void loadDocument()
    }
  },
  { immediate: true },
)

async function handleResegmentTxt(
  chapterRegex?: string,
  volumeRegex?: string,
): Promise<void> {
  if (!props.entry || resegmenting.value) return
  if (hasUnsavedEdits.value) {
    const ok = await dialogStore.confirm({
      title: '重新切分章节',
      message: '按新规则重新切分原 TXT 将覆盖当前未保存的修改，确定继续吗？',
      danger: true,
    })
    if (!ok) return
  }
  resegmenting.value = true
  errorMsg.value = null
  try {
    const res = await toolApi.resegmentTxtBook(
      props.entry.root_id,
      props.entry.rel_path,
      chapterRegex,
      volumeRegex,
    )
    chapters.value = mapDtoList(res)
    activeIndex.value = 0
    hasUnsavedEdits.value = true
    await mountActiveChapterIntoSurface()
    showToast(`已重新切分为 ${res.length} 个章节`)
  } catch (err) {
    errorMsg.value = err instanceof Error ? err.message : String(err)
  } finally {
    resegmenting.value = false
  }
}

function buildSavePayload(): SaveBookChapterInput[] {
  flushActiveSurfaceToRecord()
  return chapters.value.map((ch, idx) => ({
    title: ch.title.trim() || `第 ${idx + 1} 章`,
    level: ch.level,
    mode: 'xhtml',
    plain_text: ch.plainCache,
    xhtml_body: ch.xhtmlBody,
    source_href: ch.sourceHref,
  }))
}

async function handleSaveInPlace(): Promise<void> {
  if (!props.entry || saving.value || chapters.value.length === 0) return
  saving.value = true
  errorMsg.value = null
  try {
    const updated = await toolApi.saveBookContent(
      props.entry.root_id,
      props.entry.rel_path,
      buildSavePayload(),
      docStylesheets.value.length > 0 ? docStylesheets.value : undefined,
      forceHorizontalSpine.value,
    )
    hasUnsavedEdits.value = false
    forceHorizontalSpine.value = false
    savedBadge.value = true
    if (savedBadgeTimer) clearTimeout(savedBadgeTimer)
    savedBadgeTimer = setTimeout(() => {
      savedBadge.value = false
    }, 2200)
    emit('saved', updated)
  } catch (err) {
    errorMsg.value = err instanceof Error ? err.message : String(err)
  } finally {
    saving.value = false
  }
}

async function handleConvertToEpub(): Promise<void> {
  if (!props.entry || converting.value || chapters.value.length === 0) return
  converting.value = true
  errorMsg.value = null
  try {
    const res = await toolApi.convertTxtToEpub(
      props.entry.root_id,
      props.entry.rel_path,
      buildSavePayload(),
      deleteSourceTxt.value,
      docStylesheets.value.length > 0 ? docStylesheets.value : undefined,
    )
    hasUnsavedEdits.value = false
    emit('converted', res.detail, res.deleted_source_txt)
    showToast(`已打包生成 EPUB：${res.detail.entry.file_name}`)
  } catch (err) {
    errorMsg.value = err instanceof Error ? err.message : String(err)
  } finally {
    converting.value = false
  }
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
  if (idleSyncTimer) clearTimeout(idleSyncTimer)
  if (savedBadgeTimer) clearTimeout(savedBadgeTimer)
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
      <!-- ==================== TOP WORKBENCH HEADER ==================== -->
      <header
        class="flex h-14 shrink-0 items-center justify-between gap-3 border-b border-[var(--md-sys-color-outline-variant)] bg-[var(--md-sys-color-surface-container-low)] px-4 select-none"
      >
        <div class="flex min-w-0 items-center gap-2.5">
          <div
            class="flex h-8 w-8 shrink-0 items-center justify-center rounded-xl bg-[var(--md-sys-color-primary-container)] text-[var(--md-sys-color-on-primary-container)]"
          >
            <LuBookOpen class="h-4 w-4" />
          </div>
          <div class="min-w-0">
            <div class="flex items-center gap-2">
              <h1 class="truncate text-sm font-bold text-[var(--md-sys-color-on-surface)]">
                {{ docTitle || entry.display_title }}
              </h1>
              <span
                class="shrink-0 rounded-md bg-[var(--md-sys-color-secondary-container)] px-1.5 py-0.5 font-mono text-[10px] font-bold uppercase text-[var(--md-sys-color-on-secondary-container)]"
              >
                {{ docFormat }}
              </span>
              <span
                v-if="savedBadge"
                class="inline-flex shrink-0 items-center gap-1 rounded-full bg-[var(--md-sys-color-success-container)] px-2 py-0.5 text-[10px] font-semibold text-[var(--md-sys-color-on-success-container)]"
              >
                <LuCheck class="h-3 w-3" />
                <span>已保存</span>
              </span>
              <span
                v-else-if="hasUnsavedEdits"
                class="shrink-0 rounded-full bg-[var(--md-sys-color-warning-container)] px-2 py-0.5 text-[10px] font-semibold text-[var(--md-sys-color-on-warning-container)]"
              >
                未保存
              </span>
            </div>
            <p class="truncate text-[11px] text-[var(--md-sys-color-on-surface-variant)]">
              {{ chapters.length }} 章 · 全书约 {{ totalCharCount.toLocaleString() }} 字
            </p>
          </div>
        </div>

        <!-- Right Save / Convert / Close Actions -->
        <div class="flex shrink-0 items-center gap-2">
          <div class="group relative">
            <button
              type="button"
              class="md-icon-btn md-icon-btn-sm md-state text-[var(--md-sys-color-on-surface-variant)]"
              :title="SHORTCUT_HELP_TEXT"
              aria-label="编辑器快捷键说明"
            >
              <LuKeyboard class="h-4 w-4" />
            </button>
            <div
              class="pointer-events-none invisible absolute top-full right-0 z-20 mt-1.5 w-64 rounded-2xl border border-[var(--md-sys-color-outline-variant)] bg-[var(--md-sys-color-surface-container-high)] p-3 text-[11px] text-[var(--md-sys-color-on-surface)] opacity-0 shadow-[var(--md-sys-elevation-2)] transition group-focus-within:visible group-focus-within:opacity-100 group-hover:visible group-hover:opacity-100"
            >
              <p class="mb-1.5 font-semibold text-[var(--md-sys-color-on-surface)]">快捷键一览</p>
              <ul class="space-y-1 text-[var(--md-sys-color-on-surface-variant)]">
                <li class="flex justify-between gap-2"><span>原地保存</span><kbd class="font-mono text-[var(--md-sys-color-on-surface)]">Ctrl+S</kbd></li>
                <li class="flex justify-between gap-2"><span>查找替换</span><kbd class="font-mono text-[var(--md-sys-color-on-surface)]">Ctrl+F</kbd></li>
                <li class="flex justify-between gap-2"><span>光标拆章</span><kbd class="font-mono text-[var(--md-sys-color-on-surface)]">Ctrl+Shift+Enter</kbd></li>
                <li class="flex justify-between gap-2"><span>上下章节</span><kbd class="font-mono text-[var(--md-sys-color-on-surface)]">Alt+↑ / ↓</kbd></li>
                <li class="flex justify-between gap-2"><span>退出工作台</span><kbd class="font-mono text-[var(--md-sys-color-on-surface)]">Esc</kbd></li>
              </ul>
            </div>
          </div>

          <template v-if="docFormat === 'txt'">
            <div class="hidden sm:block">
              <AppCheckbox v-model="deleteSourceTxt" label="转后删原 TXT" />
            </div>

            <button
              type="button"
              :disabled="saving || converting || chapters.length === 0"
              class="md-btn md-btn-sm md-btn-outlined md-state disabled:opacity-50"
              title="保存修改回原 TXT 文件 (Ctrl+S)"
              @click="handleSaveInPlace"
            >
              <LuSave class="h-3.5 w-3.5 text-[var(--md-sys-color-primary)]" />
              <span>{{ saving ? '保存中…' : '保存 TXT' }}</span>
            </button>

            <button
              type="button"
              :disabled="converting || saving || chapters.length === 0"
              class="md-btn md-btn-sm md-btn-filled md-state disabled:opacity-50"
              @click="handleConvertToEpub"
            >
              <LuBookOpen class="h-3.5 w-3.5" />
              <span>{{ converting ? '打包中…' : '转为 EPUB' }}</span>
            </button>
          </template>

          <template v-else>
            <button
              type="button"
              :disabled="polishing || saving || chapters.length === 0"
              class="md-btn md-btn-sm md-btn-tonal md-state disabled:opacity-50"
              title="自动生成/刷新内封简介页、同步 EPUB2/3 双目录并清理未引用冗余资源"
              @click="handlePolishInEditor"
            >
              <LuRefreshCw v-if="polishing" class="h-3.5 w-3.5 animate-spin" />
              <LuWand2 v-else class="h-3.5 w-3.5" />
              <span class="hidden sm:inline">{{ polishing ? '抛光中…' : '书籍抛光' }}</span>
            </button>

            <button
              type="button"
              :disabled="saving || polishing || chapters.length === 0"
              class="md-btn md-btn-sm md-btn-filled md-state disabled:opacity-50"
              title="原地保存目录与正文到 EPUB，不关闭编辑器 (Ctrl+S)"
              @click="handleSaveInPlace"
            >
              <LuSave class="h-3.5 w-3.5" />
              <span>{{ saving ? '保存中…' : '保存 (Ctrl+S)' }}</span>
            </button>
          </template>

          <input
            ref="epubImageInputRef"
            type="file"
            accept="image/*"
            class="hidden"
            @change="onEpubImagePicked"
          />

          <button
            type="button"
            class="md-icon-btn md-icon-btn-sm md-state text-[var(--md-sys-color-on-surface-variant)]"
            title="退出工作台"
            aria-label="退出工作台"
            @click="requestClose"
          >
            <LuX class="h-4 w-4" />
          </button>
        </div>
      </header>

      <!-- Error Banner -->
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

      <!-- Loading Skeleton -->
      <div
        v-if="loading"
        class="flex flex-1 flex-col items-center justify-center gap-2.5 text-xs text-[var(--md-sys-color-on-surface-variant)]"
      >
        <LuLoader2 class="h-6 w-6 animate-spin text-[var(--md-sys-color-primary)]" />
        <span>正在加载书籍目录结构与样式…</span>
      </div>

      <!-- ==================== 3-COLUMN WORKBENCH BODY ==================== -->
      <div v-else class="relative flex min-h-0 flex-1 overflow-hidden">
        <!-- COLUMN 1: Fixed-Height Non-Jumping TOC & Chapter Tree -->
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

        <!-- COLUMN 2: Zero-Latency WYSIWYG / XHTML Editor -->
        <div class="flex min-w-0 flex-1 flex-col bg-[var(--md-sys-color-surface)]">
          <!-- Formatting & View Mode Toolbar -->
          <div
            class="flex shrink-0 flex-wrap items-center justify-between gap-2 border-b border-[var(--md-sys-color-outline-variant)] bg-[var(--md-sys-color-surface-container-low)] px-3 py-2 select-none"
          >
            <!-- Left Formatting Buttons (`@mousedown.prevent` keeps caret & selection intact!) -->
            <div class="flex flex-wrap items-center gap-0.5">
              <template v-if="surfaceMode === 'wysiwyg'">
                <button
                  type="button"
                  class="tb-btn"
                  title="撤销 (Ctrl+Z)"
                  @mousedown.prevent="execWysiwygCmd('undo')"
                >
                  <LuUndo2 class="h-3.5 w-3.5" />
                </button>
                <button
                  type="button"
                  class="tb-btn"
                  title="重做 (Ctrl+Y)"
                  @mousedown.prevent="execWysiwygCmd('redo')"
                >
                  <LuRedo2 class="h-3.5 w-3.5" />
                </button>

                <span class="mx-1 h-4 w-px bg-[var(--md-sys-color-outline-variant)]" />
              </template>

              <button
                type="button"
                class="tb-btn"
                title="正文段落 (<p>)"
                @mousedown.prevent="formatBlockTag('P')"
              >
                <LuPilcrow class="h-3.5 w-3.5" />
              </button>
              <button
                type="button"
                class="tb-btn"
                title="一级标题 (<h1>)"
                @mousedown.prevent="formatBlockTag('H1')"
              >
                <LuHeading1 class="h-3.5 w-3.5" />
              </button>
              <button
                type="button"
                class="tb-btn"
                title="二级章节标题 (<h2>)"
                @mousedown.prevent="formatBlockTag('H2')"
              >
                <LuHeading2 class="h-3.5 w-3.5" />
              </button>
              <button
                type="button"
                class="tb-btn"
                title="三级小节标题 (<h3>)"
                @mousedown.prevent="formatBlockTag('H3')"
              >
                <LuHeading3 class="h-3.5 w-3.5" />
              </button>
              <button
                type="button"
                class="tb-btn"
                title="引用块 (<blockquote>)"
                @mousedown.prevent="formatBlockTag('BLOCKQUOTE')"
              >
                <LuQuote class="h-3.5 w-3.5" />
              </button>

              <span class="mx-1 h-4 w-px bg-[var(--md-sys-color-outline-variant)]" />

              <template v-if="surfaceMode === 'wysiwyg'">
                <button
                  type="button"
                  class="tb-btn"
                  title="加粗 (Ctrl+B)"
                  @mousedown.prevent="execWysiwygCmd('bold')"
                >
                  <LuBold class="h-3.5 w-3.5" />
                </button>
                <button
                  type="button"
                  class="tb-btn"
                  title="斜体 (Ctrl+I)"
                  @mousedown.prevent="execWysiwygCmd('italic')"
                >
                  <LuItalic class="h-3.5 w-3.5" />
                </button>
                <button
                  type="button"
                  class="tb-btn"
                  title="下划线 (Ctrl+U)"
                  @mousedown.prevent="execWysiwygCmd('underline')"
                >
                  <LuUnderline class="h-3.5 w-3.5" />
                </button>
                <button
                  type="button"
                  class="tb-btn"
                  title="清除选中文字格式"
                  @mousedown.prevent="execWysiwygCmd('removeFormat')"
                >
                  <LuEraser class="h-3.5 w-3.5" />
                </button>
              </template>

              <button
                type="button"
                class="tb-btn"
                title="行内代码 (<code>)"
                @mousedown.prevent="wrapInlineCode"
              >
                <LuCode class="h-3.5 w-3.5" />
              </button>
              <button
                type="button"
                class="tb-btn"
                title="插入水平分割线 (<hr/>)"
                @mousedown.prevent="insertHorizontalRule"
              >
                <LuMinus class="h-3.5 w-3.5" />
              </button>

              <span class="mx-1 h-4 w-px bg-[var(--md-sys-color-outline-variant)]" />

              <button
                type="button"
                class="md-state inline-flex h-7 items-center gap-1 rounded-lg px-2 text-[11px] font-medium text-[var(--md-sys-color-on-surface-variant)] transition hover:bg-[var(--md-sys-color-surface-container-high)] hover:text-[var(--md-sys-color-on-surface)]"
                title="在当前光标位置拆分为上下两章 (Ctrl+Shift+Enter)"
                @mousedown.prevent="splitChapterAtCursor"
              >
                <LuScissors class="h-3.5 w-3.5 text-[var(--md-sys-color-primary)]" />
                <span>光标拆章</span>
              </button>

              <button
                type="button"
                class="md-state inline-flex h-7 items-center gap-1 rounded-lg px-2 text-[11px] font-medium text-[var(--md-sys-color-on-surface-variant)] transition hover:bg-[var(--md-sys-color-surface-container-high)] hover:text-[var(--md-sys-color-on-surface)]"
                title="自动清理本章空段落与段首全角/半角冗余空格"
                @click="cleanChapterTypography"
              >
                <LuSparkles class="h-3.5 w-3.5 text-[var(--md-sys-color-primary)]" />
                <span>一键净版</span>
              </button>

              <button
                v-if="docFormat === 'epub'"
                type="button"
                :disabled="uploadingImage"
                class="md-state inline-flex h-7 items-center gap-1 rounded-lg px-2 text-[11px] font-medium text-[var(--md-sys-color-on-surface-variant)] transition hover:bg-[var(--md-sys-color-surface-container-high)] hover:text-[var(--md-sys-color-on-surface)] disabled:opacity-50"
                title="上传图片写入 EPUB 资源包并插入当前光标处"
                @mousedown.prevent="triggerEpubImageUpload"
              >
                <LuImage class="h-3.5 w-3.5 text-[var(--md-sys-color-primary)]" />
                <span>{{ uploadingImage ? '上传中…' : '插入图片' }}</span>
              </button>

              <button
                type="button"
                class="md-state inline-flex h-7 items-center gap-1 rounded-lg px-2 text-[11px] font-medium transition"
                :class="
                  proofreadPanelOpen
                    ? 'bg-[var(--md-sys-color-secondary-container)] text-[var(--md-sys-color-on-secondary-container)]'
                    : 'text-[var(--md-sys-color-on-surface-variant)] hover:bg-[var(--md-sys-color-surface-container-high)] hover:text-[var(--md-sys-color-on-surface)]'
                "
                title="简繁转换、台版竖排转横排、网文求票净化与中文标点智能规整"
                @click="proofreadPanelOpen = !proofreadPanelOpen; if (proofreadPanelOpen) cssPanelOpen = false"
              >
                <LuLanguages class="h-3.5 w-3.5" />
                <span>简繁与精校</span>
              </button>

              <button
                type="button"
                class="md-state inline-flex h-7 items-center gap-1 rounded-lg px-2 text-[11px] font-medium transition"
                :class="
                  cssPanelOpen
                    ? 'bg-[var(--md-sys-color-secondary-container)] text-[var(--md-sys-color-on-secondary-container)]'
                    : 'text-[var(--md-sys-color-on-surface-variant)] hover:bg-[var(--md-sys-color-surface-container-high)] hover:text-[var(--md-sys-color-on-surface)]'
                "
                title="切换中文排版 CSS 预设与编辑全局样式表"
                @click="cssPanelOpen = !cssPanelOpen; if (cssPanelOpen) proofreadPanelOpen = false"
              >
                <LuPalette class="h-3.5 w-3.5" />
                <span>排版样式</span>
              </button>

              <button
                type="button"
                class="md-state inline-flex h-7 items-center gap-1 rounded-lg px-2 text-[11px] font-medium transition"
                :class="
                  findBarOpen
                    ? 'bg-[var(--md-sys-color-secondary-container)] text-[var(--md-sys-color-on-secondary-container)]'
                    : 'text-[var(--md-sys-color-on-surface-variant)] hover:bg-[var(--md-sys-color-surface-container-high)] hover:text-[var(--md-sys-color-on-surface)]'
                "
                title="查找与批量替换 (Ctrl+F)"
                @click="findBarOpen = !findBarOpen"
              >
                <LuSearch class="h-3.5 w-3.5" />
                <span>查找替换</span>
              </button>
            </div>

            <!-- Right Mode Switcher + Live Preview Split Toggle -->
            <div class="flex items-center gap-1.5">
              <button
                v-if="surfaceMode === 'wysiwyg'"
                type="button"
                class="md-state h-8 rounded-full border px-2.5 text-[11px] font-medium transition"
                :class="
                  indentParagraphs
                    ? 'border-[var(--md-sys-color-primary)]/40 bg-[var(--md-sys-color-secondary-container)] text-[var(--md-sys-color-on-secondary-container)]'
                    : 'border-[var(--md-sys-color-outline-variant)] bg-[var(--md-sys-color-surface-container-lowest)] text-[var(--md-sys-color-on-surface-variant)]'
                "
                title="切换段落首行缩进两字符显示"
                @click="indentParagraphs = !indentParagraphs"
              >
                首行缩进
              </button>

              <MdSegmentedButton
                :model-value="surfaceMode"
                :options="surfaceModeOptions"
                size="sm"
                @update:model-value="switchSurfaceMode($event)"
              />

              <button
                type="button"
                class="md-state inline-flex h-8 items-center gap-1 rounded-full border px-3 text-[11px] font-semibold transition"
                :class="
                  showLivePreview
                    ? 'border-[var(--md-sys-color-primary)] bg-[var(--md-sys-color-secondary-container)] text-[var(--md-sys-color-on-secondary-container)]'
                    : 'border-[var(--md-sys-color-outline-variant)] bg-[var(--md-sys-color-surface-container-lowest)] text-[var(--md-sys-color-on-surface-variant)] hover:text-[var(--md-sys-color-on-surface)]'
                "
                title="开启/关闭右侧阅读器真实样式对照预览窗"
                @click="toggleLivePreview"
              >
                <LuEye v-if="showLivePreview" class="h-3.5 w-3.5" />
                <LuEyeOff v-else class="h-3.5 w-3.5" />
                <span class="hidden sm:inline">预览分屏</span>
              </button>
            </div>
          </div>

          <!-- Collapsible Proofreading & Zh/Vertical Conversion Toolbox -->
          <BookEditorProofreadPanel
            v-if="proofreadPanelOpen"
            :transform-scope="transformScope"
            :transforming="transforming"
            @update:transform-scope="transformScope = $event"
            @run-transform="(opts, label) => runBackendTextTransform(opts, label)"
            @close="proofreadPanelOpen = false"
          />

          <!-- Collapsible Global EPUB CSS Presets & Stylesheet Editor -->
          <BookEditorCssPanel
            v-if="cssPanelOpen"
            :model-value="primaryCssDraft"
            @update:model-value="primaryCssDraft = $event; syncPrimaryCssDraftToStylesheets()"
            @apply-preset="(css, label) => applyCssPreset(css, label)"
            @close="cssPanelOpen = false"
          />

          <!-- Collapsible Find & Replace Bar -->
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

          <!-- Chapter Title Header Input -->
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

          <!-- Editor Surface: Direct Click-to-Edit WYSIWYG vs XHTML Source -->
          <div class="relative min-h-0 flex-1 overflow-y-auto">
            <div
              v-show="surfaceMode === 'wysiwyg'"
              class="mx-auto max-w-3xl px-6 py-6 sm:px-10"
            >
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

        <!-- COLUMN 3: Toggleable Real EPUB Stylesheet & Image Live Preview -->
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
            sandbox="allow-same-origin"
            class="min-h-0 w-full flex-1 border-0 bg-[var(--md-sys-color-surface)]"
          />
        </aside>
      </div>
    </div>
  </MdDialog>
</template>

<style scoped>
@reference "tailwindcss";

.tb-btn {
  @apply flex h-7 w-7 items-center justify-center rounded-lg text-[var(--md-sys-color-on-surface-variant)] transition hover:bg-[var(--md-sys-color-surface-container-high)] hover:text-[var(--md-sys-color-on-surface)];
}

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

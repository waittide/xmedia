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
  LuAlertTriangle,
  LuBookOpen,
  LuBold,
  LuCheck,
  LuChevronDown,
  LuChevronRight,
  LuCode,
  LuEraser,
  LuEye,
  LuEyeOff,
  LuGripVertical,
  LuHeading1,
  LuHeading2,
  LuHeading3,
  LuImage,
  LuItalic,
  LuLanguages,
  LuListTree,
  LuLoader2,
  LuMerge,
  LuMinus,
  LuPalette,
  LuPencil,
  LuPilcrow,
  LuPlus,
  LuQuote,
  LuRedo2,
  LuRefreshCw,
  LuReplace,
  LuReplaceAll,
  LuSave,
  LuScissors,
  LuSearch,
  LuSparkles,
  LuTrash2,
  LuUnderline,
  LuUndo2,
  LuWand2,
  LuX,
} from 'vue-icons-plus/lu'
import AppCheckbox from '@/components/ui/AppCheckbox.vue'
import { useDialogStore } from '@/stores/dialog'
import { useThemeStore } from '@/stores/theme'
import { buildEpubAssetUrl, toolApi } from '../api'
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
const themeStore = useThemeStore()

const loading = ref(false)
const saving = ref(false)
const converting = ref(false)
const resegmenting = ref(false)
const transforming = ref(false)
const polishing = ref(false)
const uploadingImage = ref(false)
const errorMsg = ref<string | null>(null)
const savedBadge = ref(false)
const statusToast = ref<string | null>(null)

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
let toastTimer: ReturnType<typeof setTimeout> | null = null
let keySeq = 1

// Left TOC Sidebar state
const tocSearchQuery = ref('')
const onlyShowAnomalies = ref(false)
const tocToolsOpen = ref(false)
const renamingIdx = ref<number | null>(null)
const renameDraft = ref('')
const dragFromIdx = ref<number | null>(null)
const dragOverIdx = ref<number | null>(null)
const dragDropPos = ref<'before' | 'after'>('after')

// TXT regex re-segmentation controls
const regexPanelOpen = ref(false)
const chapterRegexInput = ref('')
const volumeRegexInput = ref('')
const deleteSourceTxt = ref(false)

// Find & Replace Bar state
const findBarOpen = ref(false)
const findQuery = ref('')
const replaceQuery = ref('')
const findScope = ref<'chapter' | 'book'>('chapter')
const findUseRegex = ref(false)
const findCaseSensitive = ref(false)
const findInputRef = ref<HTMLInputElement | null>(null)

// Proofreading & Chinese Conversion Toolbox + CSS Theme Drawer + Image Upload
const proofreadPanelOpen = ref(false)
const cssPanelOpen = ref(false)
const transformScope = ref<'book' | 'chapter'>('book')
const cleanWebnovelAds = ref(true)
const normalizePunctuation = ref(true)
const convertCornerQuotes = ref(false)
const customStripRulesOpen = ref(false)
const CUSTOM_RULES_STORAGE_KEY = 'xmedia.book.customStripPatterns.v1'
const customStripRulesText = ref(
  typeof window !== 'undefined' ? window.localStorage.getItem(CUSTOM_RULES_STORAGE_KEY) ?? '' : '',
)
const epubImageInputRef = ref<HTMLInputElement | null>(null)

watch(customStripRulesText, (val) => {
  if (typeof window !== 'undefined') {
    window.localStorage.setItem(CUSTOM_RULES_STORAGE_KEY, val)
  }
})

const cssPresets = [
  {
    id: 'balanced',
    label: '默认均衡衬线',
    desc: '正文宋体/思源宋体 · 1.85 行高 · 首行缩进 2 字',
    css: `body {\n  font-family: "Noto Serif CJK SC", "Songti SC", "PingFang SC", Georgia, serif;\n  line-height: 1.85;\n  margin: 0;\n  padding: 1.2em 1.4em;\n  word-wrap: break-word;\n}\nh1, h2, h3 {\n  line-height: 1.35;\n  margin: 1.2em 0 0.8em;\n  font-weight: 700;\n}\np {\n  margin: 0 0 0.85em;\n  text-indent: 2em;\n  text-align: justify;\n}\nfigure, .cover-wrap {\n  text-align: center;\n  margin: 1em 0;\n}\nimg {\n  max-width: 100%;\n  height: auto;\n}`,
  },
  {
    id: 'classic',
    label: '古典宣纸宋刻',
    desc: '章题底纹装饰 · 宽松舒朗段距 · 典籍与古风长篇优选',
    css: `body {\n  font-family: "Songti SC", "STSong", "Noto Serif CJK SC", "SimSun", serif;\n  line-height: 2.0;\n  letter-spacing: 0.03em;\n  margin: 0;\n  padding: 1.4em 1.6em;\n}\nh1, h2 {\n  font-weight: 700;\n  text-align: center;\n  line-height: 1.4;\n  margin: 1.5em 0 1.1em;\n  padding-bottom: 0.45em;\n  border-bottom: 1px solid rgba(128, 128, 128, 0.28);\n}\np {\n  margin: 0 0 0.95em;\n  text-indent: 2em;\n  text-align: justify;\n}\nblockquote {\n  font-family: "Kaiti SC", "STKaiti", "FangSong", serif;\n  margin: 1em 1.2em;\n  padding-left: 0.8em;\n  border-left: 2px solid rgba(128, 128, 128, 0.4);\n}`,
  },
  {
    id: 'modern',
    label: '现代屏显黑体',
    desc: '苹方/鸿蒙黑体 · 无衬线高清晰屏显 · 社科技术图书优选',
    css: `body {\n  font-family: -apple-system, BlinkMacSystemFont, "PingFang SC", "HarmonyOS Sans SC", "Microsoft YaHei", sans-serif;\n  line-height: 1.8;\n  margin: 0;\n  padding: 1.2em 1.5em;\n}\nh1, h2, h3 {\n  font-weight: 700;\n  line-height: 1.35;\n  margin: 1.3em 0 0.75em;\n}\np {\n  margin: 0 0 0.9em;\n  text-indent: 2em;\n  text-align: justify;\n}\ncode, pre {\n  font-family: ui-monospace, SFMono-Regular, Menlo, Consolas, monospace;\n  font-size: 0.9em;\n}`,
  },
  {
    id: 'lightnovel',
    label: '轻小说插图全屏',
    desc: '插图自适应整屏防截断 · 紧凑对话节奏 · 日轻/网文优选',
    css: `body {\n  font-family: "PingFang SC", "Noto Serif CJK SC", "Hiragino Sans GB", serif;\n  line-height: 1.82;\n  margin: 0;\n  padding: 1em 1.25em;\n}\nh1, h2 {\n  font-weight: 700;\n  line-height: 1.35;\n  margin: 1.1em 0 0.8em;\n}\np {\n  margin: 0 0 0.7em;\n  text-indent: 2em;\n}\nfigure {\n  margin: 0.8em auto;\n  text-align: center;\n  page-break-inside: avoid;\n  break-inside: avoid;\n}\nfigure img, img {\n  max-width: 100%;\n  max-height: 95vh;\n  object-fit: contain;\n  display: block;\n  margin: 0 auto;\n}`,
  },
]

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

function showToast(msg: string): void {
  statusToast.value = msg
  if (toastTimer) clearTimeout(toastTimer)
  toastTimer = setTimeout(() => {
    statusToast.value = null
  }, 2600)
}

function escapeXml(raw: string): string {
  return raw
    .replace(/&/g, '&amp;')
    .replace(/</g, '&lt;')
    .replace(/>/g, '&gt;')
    .replace(/"/g, '&quot;')
    .replace(/\u00A0/g, '&#160;')
}

function plainToCleanXhtml(title: string, plain: string): string {
  const out: string[] = []
  const t = title.trim()
  if (t) {
    out.push(`  <h2>${escapeXml(t)}</h2>`)
  }
  for (const line of plain.split('\n')) {
    const trimmed = line.trim()
    if (!trimmed) continue
    out.push(`  <p>${escapeXml(trimmed)}</p>`)
  }
  if (out.length === 0) {
    out.push('  <p><br/></p>')
  }
  return out.join('\n')
}

function countNonWhitespace(text: string): number {
  let count = 0
  for (let i = 0; i < text.length; i += 1) {
    const code = text.charCodeAt(i)
    if (code > 32 && code !== 160 && code !== 12288) {
      count += 1
    }
  }
  return count
}

function extractPlainFromXhtml(xhtml: string, stripHeading?: string): string {
  const doc = new DOMParser().parseFromString(`<body>${xhtml}</body>`, 'text/html')
  const body = doc.body
  if (!body) return ''
  body.querySelectorAll('br').forEach((br) => br.replaceWith('\n'))
  body
    .querySelectorAll('p, div, h1, h2, h3, h4, h5, h6, li, blockquote, section, article, tr')
    .forEach((el) => {
      el.append('\n')
    })
  const paras = (body.textContent ?? '')
    .split('\n')
    .map((l) => l.replace(/\s+/g, ' ').trim())
    .filter(Boolean)

  if (stripHeading && paras[0] === stripHeading.trim()) {
    paras.shift()
  }
  return paras.join('\n')
}

function resolveZipRelativePath(baseFileHref: string | null, relTarget: string): string | null {
  const trimmed = relTarget.trim()
  if (
    !trimmed ||
    trimmed.startsWith('http://') ||
    trimmed.startsWith('https://') ||
    trimmed.startsWith('data:') ||
    trimmed.startsWith('blob:') ||
    trimmed.startsWith('/api/')
  ) {
    return null
  }
  const cleanRel = trimmed.split('#')[0]?.split('?')[0] ?? trimmed
  const baseParts = baseFileHref ? baseFileHref.split('/').slice(0, -1) : []
  const stack = [...baseParts.filter(Boolean)]
  for (const seg of cleanRel.split('/')) {
    if (!seg || seg === '.') continue
    if (seg === '..') {
      stack.pop()
    } else {
      stack.push(seg)
    }
  }
  return stack.join('/')
}

/**
 * Converts stored EPUB XHTML into safe HTML for the WYSIWYG surface,
 * rewriting internal EPUB image paths to authenticated `/books/epub-asset` URLs.
 */
function hydrateXhtmlForBrowser(xhtmlBody: string, sourceHref: string | null): string {
  const entry = props.entry
  const raw = xhtmlBody.trim() || '<p><br/></p>'
  const doc = new DOMParser().parseFromString(`<body>${raw}</body>`, 'text/html')
  const body = doc.body
  if (!body) return raw

  if (entry && docFormat.value === 'epub') {
    body.querySelectorAll('img').forEach((img) => {
      const origSrc = img.getAttribute('src')
      if (!origSrc) return
      const resolved = resolveZipRelativePath(sourceHref, origSrc)
      if (resolved) {
        img.setAttribute('data-epub-src', origSrc)
        img.setAttribute('src', buildEpubAssetUrl(entry.root_id, entry.rel_path, resolved))
      }
    })
    body.querySelectorAll('image').forEach((svgImg) => {
      const origHref = svgImg.getAttribute('href') || svgImg.getAttribute('xlink:href')
      if (!origHref) return
      const resolved = resolveZipRelativePath(sourceHref, origHref)
      if (resolved) {
        const apiUrl = buildEpubAssetUrl(entry.root_id, entry.rel_path, resolved)
        svgImg.setAttribute('data-epub-href', origHref)
        svgImg.setAttribute('href', apiUrl)
      }
    })
  }

  return body.innerHTML || '<p><br></p>'
}

const VOID_TAGS = new Set(['br', 'hr', 'img', 'col', 'meta', 'link', 'input'])
const BLOCK_TAGS = new Set([
  'p',
  'h1',
  'h2',
  'h3',
  'h4',
  'h5',
  'h6',
  'blockquote',
  'ul',
  'ol',
  'li',
  'pre',
  'hr',
  'figure',
  'figcaption',
  'table',
  'thead',
  'tbody',
  'tr',
  'th',
  'td',
  'section',
  'article',
])
const ALLOWED_ATTRS = new Set([
  'class',
  'id',
  'href',
  'src',
  'alt',
  'title',
  'colspan',
  'rowspan',
  'width',
  'height',
  'viewBox',
  'xmlns',
])

function serializeNodeToValidXhtml(node: Node, topLevel = false): string {
  if (node.nodeType === Node.TEXT_NODE) {
    const text = node.nodeValue ?? ''
    if (topLevel) {
      const trimmed = text.trim()
      return trimmed ? `  <p>${escapeXml(trimmed)}</p>` : ''
    }
    return escapeXml(text)
  }

  if (node.nodeType !== Node.ELEMENT_NODE) {
    return ''
  }

  const el = node as HTMLElement
  let tag = el.tagName.toLowerCase()
  if (tag === 'script' || tag === 'style') return ''
  if (tag === 'b') tag = 'strong'
  if (tag === 'i') tag = 'em'
  // Normalize browser contenteditable `<div>` paragraphs into semantic `<p>` unless it wraps blocks
  if (tag === 'div') {
    const hasBlockChild = Array.from(el.children).some((c) =>
      BLOCK_TAGS.has(c.tagName.toLowerCase()),
    )
    if (!hasBlockChild) {
      tag = 'p'
    }
  }

  const attrs: string[] = []
  const origEpubSrc = el.getAttribute('data-epub-src')
  const origEpubHref = el.getAttribute('data-epub-href')

  for (const attr of Array.from(el.attributes)) {
    const name = attr.name
    if (name === 'data-epub-src' || name === 'data-epub-href') continue
    if (name === 'contenteditable' || name === 'draggable') continue
    if (tag === 'img' && name === 'src' && origEpubSrc) {
      attrs.push(`src="${escapeXml(origEpubSrc)}"`)
      continue
    }
    if (tag === 'image' && (name === 'href' || name === 'xlink:href') && origEpubHref) {
      attrs.push(`href="${escapeXml(origEpubHref)}"`)
      continue
    }
    if (ALLOWED_ATTRS.has(name) || name.startsWith('epub:')) {
      attrs.push(`${name}="${escapeXml(attr.value)}"`)
    }
  }
  if (tag === 'img' && origEpubSrc && !attrs.some((a) => a.startsWith('src='))) {
    attrs.push(`src="${escapeXml(origEpubSrc)}"`)
  }

  const attrStr = attrs.length > 0 ? ` ${attrs.join(' ')}` : ''
  const indent = topLevel ? '  ' : ''

  if (VOID_TAGS.has(tag)) {
    return `${indent}<${tag}${attrStr}/>`
  }

  const childMarkup = Array.from(el.childNodes)
    .map((child) => serializeNodeToValidXhtml(child, false))
    .join('')

  if (topLevel && tag === 'p' && !childMarkup.trim()) {
    return ''
  }

  return `${indent}<${tag}${attrStr}>${childMarkup}</${tag}>`
}

function serializeContainerToValidXhtml(container: ParentNode): string {
  const lines: string[] = []
  for (const child of Array.from(container.childNodes)) {
    const serialized = serializeNodeToValidXhtml(child, true)
    if (serialized.trim()) {
      lines.push(serialized)
    }
  }
  return lines.join('\n')
}

function buildLivePreviewSrcdoc(xhtmlBody: string, sourceHref: string | null): string {
  const hydrated = hydrateXhtmlForBrowser(xhtmlBody, sourceHref)
  const isDark = themeStore.isDark
  const bg = isDark ? '#1e1e2e' : '#ffffff'
  const fg = isDark ? '#cdd6f4' : '#1f2328'
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
  blockquote { margin: 1em 0; padding: 0.4em 1em; border-left: 3px solid #89b4fa; opacity: 0.9; }
  hr { border: none; border-top: 1px solid rgba(128,128,128,0.3); margin: 1.5em 0; }
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

function getChapterAnomaly(ch: ChapterRecord, idx: number): 'short' | 'long' | null {
  const count = idx === activeIndex.value ? activeCharCount.value : ch.charCount
  if (count > 20000) return 'long'
  if (count < 80 && !ch.hasRichMedia && ch.title !== '书籍简介') return 'short'
  return null
}

const anomalyCount = computed(() =>
  chapters.value.reduce((acc, ch, idx) => (getChapterAnomaly(ch, idx) ? acc + 1 : acc), 0),
)

const filteredTocItems = computed(() => {
  const q = tocSearchQuery.value.trim().toLowerCase()
  const all = chapters.value.map((ch, idx) => ({
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

watch(indentParagraphs, () => {
  if (showLivePreview.value && currentRecord.value) {
    flushActiveSurfaceToRecord()
    previewSrcdoc.value = buildLivePreviewSrcdoc(
      currentRecord.value.xhtmlBody,
      currentRecord.value.sourceHref,
    )
  }
})

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

function startInlineRename(idx: number): void {
  const target = chapters.value[idx]
  if (!target) return
  renamingIdx.value = idx
  renameDraft.value = target.title
}

function commitInlineRename(): void {
  if (renamingIdx.value === null) return
  const idx = renamingIdx.value
  const rec = chapters.value[idx]
  const clean = renameDraft.value.trim()
  if (rec && clean && clean !== rec.title) {
    rec.title = clean
    if (idx === activeIndex.value) {
      activeTitle.value = clean
    }
    hasUnsavedEdits.value = true
    triggerRef(chapters)
  }
  renamingIdx.value = null
}

function toggleLevel(idx: number): void {
  const rec = chapters.value[idx]
  if (!rec) return
  rec.level = rec.level === 0 ? 1 : 0
  hasUnsavedEdits.value = true
  triggerRef(chapters)
}

function autoDetectTocHierarchy(): void {
  flushActiveSurfaceToRecord()
  const volRe =
    /^\s*(?:第[零一二三四五六七八九十百千万两〇廿卅卌壹贰叁肆伍陆柒捌玖拾佰仟0-9０-９]+[卷部集册篇](?:[\s：:、—\-].*)?|卷[零一二三四五六七八九十百千万两〇廿卅卌壹贰叁肆伍陆柒捌玖拾佰仟0-9０-９]+.*|(?:volume|vol\.?|book|part)\s+[0-9ivxlcdm]+.*)$/i
  let seenVol = false
  let volCount = 0
  for (const ch of chapters.value) {
    if (ch.title === '书籍简介') {
      ch.level = 0
      continue
    }
    if (volRe.test(ch.title.trim())) {
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

function handleRunProofreadClean(): void {
  const customPatterns = customStripRulesText.value
    .split('\n')
    .map((l) => l.trim())
    .filter(Boolean)
  void runBackendTextTransform(
    {
      clean_webnovel_ads: cleanWebnovelAds.value,
      normalize_punctuation: normalizePunctuation.value,
      convert_quotes_to_horizontal: convertCornerQuotes.value,
      custom_strip_patterns: customPatterns,
    },
    '精校净化',
  )
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

async function insertChapterBelow(idx: number): Promise<void> {
  flushActiveSurfaceToRecord()
  const refLevel = chapters.value[idx]?.level ?? 0
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
  nextList.splice(idx + 1, 0, newRec)
  chapters.value = nextList
  activeIndex.value = idx + 1
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

function onTocDragStart(idx: number, ev: DragEvent): void {
  dragFromIdx.value = idx
  if (ev.dataTransfer) {
    ev.dataTransfer.effectAllowed = 'move'
    ev.dataTransfer.setData('text/plain', String(idx))
  }
}

function onTocDragOver(idx: number, ev: DragEvent): void {
  if (dragFromIdx.value === null || dragFromIdx.value === idx) return
  const rect = (ev.currentTarget as HTMLElement).getBoundingClientRect()
  dragOverIdx.value = idx
  dragDropPos.value = ev.clientY < rect.top + rect.height / 2 ? 'before' : 'after'
}

function onTocDragEnd(): void {
  dragFromIdx.value = null
  dragOverIdx.value = null
}

async function onTocDrop(targetIdx: number): Promise<void> {
  const from = dragFromIdx.value
  const pos = dragDropPos.value
  onTocDragEnd()
  if (from === null || from === targetIdx) return

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

function replaceInTextNodes(root: Node, re: RegExp, replacement: string, limitOne: boolean): number {
  let replaced = 0
  const walker = document.createTreeWalker(root, NodeFilter.SHOW_TEXT)
  const textNodes: Text[] = []
  let cur = walker.nextNode()
  while (cur) {
    textNodes.push(cur as Text)
    cur = walker.nextNode()
  }
  for (const tn of textNodes) {
    const val = tn.nodeValue ?? ''
    re.lastIndex = 0
    if (!re.test(val)) continue
    re.lastIndex = 0
    if (limitOne) {
      const singleFlags = re.flags.replace('g', '')
      const singleRe = new RegExp(re.source, singleFlags)
      tn.nodeValue = val.replace(singleRe, replacement)
      return 1
    }
    const matches = Array.from(val.matchAll(re)).length
    replaced += matches
    tn.nodeValue = val.replace(re, replacement)
  }
  return replaced
}

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
  tocSearchQuery.value = ''
  onlyShowAnomalies.value = false
  tocToolsOpen.value = false
  regexPanelOpen.value = false
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

async function handleResegmentTxt(): Promise<void> {
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
      chapterRegexInput.value.trim() || undefined,
      volumeRegexInput.value.trim() || undefined,
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
    void nextTick(() => findInputRef.value?.focus())
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
  if (toastTimer) clearTimeout(toastTimer)
})
</script>

<template>
  <Teleport to="body">
    <div
      v-if="open && entry"
      class="fixed inset-0 z-50 flex flex-col overflow-hidden bg-[var(--ctp-canvas)] text-[var(--ctp-text)]"
    >
      <!-- ==================== TOP WORKBENCH HEADER ==================== -->
      <header
        class="flex h-14 shrink-0 items-center justify-between gap-3 border-b border-[var(--ctp-surface0)]/85 bg-[var(--ctp-shell)] px-4 select-none"
      >
        <div class="flex min-w-0 items-center gap-2.5">
          <div
            class="flex h-8 w-8 shrink-0 items-center justify-center rounded-xl bg-[var(--ctp-accent)] text-[var(--ctp-on-accent)] shadow-xs"
          >
            <LuBookOpen class="h-4 w-4" />
          </div>
          <div class="min-w-0">
            <div class="flex items-center gap-2">
              <h1 class="truncate text-sm font-bold text-[var(--ctp-text)]">
                {{ docTitle || entry.display_title }}
              </h1>
              <span
                class="shrink-0 rounded-md bg-[var(--ctp-accent-soft)] px-1.5 py-0.5 font-mono text-[10px] font-bold uppercase text-[var(--ctp-accent)]"
              >
                {{ docFormat }}
              </span>
              <span
                v-if="savedBadge"
                class="inline-flex shrink-0 items-center gap-1 rounded-full bg-emerald-500/15 px-2 py-0.5 text-[10px] font-semibold text-emerald-500"
              >
                <LuCheck class="h-3 w-3" />
                <span>已保存</span>
              </span>
              <span
                v-else-if="hasUnsavedEdits"
                class="shrink-0 rounded-full bg-amber-500/15 px-2 py-0.5 text-[10px] font-semibold text-amber-500"
              >
                未保存
              </span>
            </div>
            <p class="truncate text-[11px] text-[var(--ctp-subtext0)]">
              {{ chapters.length }} 章 · 全书约 {{ totalCharCount.toLocaleString() }} 字 ·
              快捷键：Ctrl+S 保存 · Ctrl+F 查找替换 · Ctrl+Shift+Enter 拆章
            </p>
          </div>
        </div>

        <!-- Right Save / Convert / Close Actions -->
        <div class="flex shrink-0 items-center gap-2">
          <template v-if="docFormat === 'txt'">
            <div class="hidden sm:block">
              <AppCheckbox v-model="deleteSourceTxt" label="转后删原 TXT" />
            </div>

            <button
              type="button"
              :disabled="saving || converting || chapters.length === 0"
              class="inline-flex h-8 items-center gap-1.5 rounded-xl border border-[var(--ctp-surface0)] bg-[var(--ctp-canvas)] px-3 text-xs font-semibold text-[var(--ctp-text)] transition hover:border-[var(--ctp-accent)] disabled:opacity-50"
              title="保存修改回原 TXT 文件 (Ctrl+S)"
              @click="handleSaveInPlace"
            >
              <LuSave class="h-3.5 w-3.5 text-[var(--ctp-accent)]" />
              <span>{{ saving ? '保存中…' : '保存 TXT' }}</span>
            </button>

            <button
              type="button"
              :disabled="converting || saving || chapters.length === 0"
              class="inline-flex h-8 items-center gap-1.5 rounded-xl bg-[var(--ctp-accent)] px-3.5 text-xs font-semibold text-[var(--ctp-on-accent)] shadow-xs transition hover:opacity-95 disabled:opacity-50"
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
              class="inline-flex h-8 items-center gap-1.5 rounded-xl border border-[var(--ctp-accent)]/35 bg-[var(--ctp-canvas)] px-3 text-xs font-semibold text-[var(--ctp-accent)] transition hover:bg-[var(--ctp-accent-soft)] disabled:opacity-50"
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
              class="inline-flex h-8 items-center gap-1.5 rounded-xl bg-[var(--ctp-accent)] px-4 text-xs font-semibold text-[var(--ctp-on-accent)] shadow-xs transition hover:opacity-95 disabled:opacity-50"
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
            class="flex h-8 w-8 items-center justify-center rounded-xl text-[var(--ctp-subtext0)] transition hover:bg-[var(--ctp-surface0)]/70 hover:text-[var(--ctp-text)]"
            title="退出工作台"
            @click="requestClose"
          >
            <LuX class="h-4 w-4" />
          </button>
        </div>
      </header>

      <!-- Error Banner -->
      <div
        v-if="errorMsg"
        class="flex shrink-0 items-center justify-between gap-2 border-b border-red-500/30 bg-red-500/10 px-4 py-2 text-xs text-red-400"
      >
        <span>{{ errorMsg }}</span>
        <button type="button" class="rounded p-0.5 hover:bg-red-500/15" @click="errorMsg = null">
          <LuX class="h-3.5 w-3.5" />
        </button>
      </div>

      <!-- Loading Skeleton -->
      <div
        v-if="loading"
        class="flex flex-1 flex-col items-center justify-center gap-2.5 text-xs text-[var(--ctp-subtext0)]"
      >
        <LuLoader2 class="h-6 w-6 animate-spin text-[var(--ctp-accent)]" />
        <span>正在加载电子书目录结构与样式…</span>
      </div>

      <!-- ==================== 3-COLUMN WORKBENCH BODY ==================== -->
      <div v-else class="relative flex min-h-0 flex-1 overflow-hidden">
        <!-- COLUMN 1: Fixed-Height Non-Jumping TOC & Chapter Tree -->
        <aside
          class="flex w-68 shrink-0 flex-col border-r border-[var(--ctp-surface0)]/80 bg-[var(--ctp-shell)] select-none lg:w-76"
        >
          <!-- TOC Header + Search -->
          <div class="space-y-2 border-b border-[var(--ctp-surface0)]/65 p-2.5">
            <div class="flex items-center justify-between gap-1.5">
              <span class="px-1 text-xs font-bold text-[var(--ctp-text)]">
                目录树 ({{ chapters.length }})
              </span>

              <div class="flex items-center gap-1">
                <button
                  type="button"
                  class="inline-flex items-center gap-1 rounded-lg border px-2 py-1 text-[11px] font-medium transition"
                  :class="
                    tocToolsOpen
                      ? 'border-[var(--ctp-accent)] bg-[var(--ctp-accent-soft)] text-[var(--ctp-accent)]'
                      : 'border-[var(--ctp-surface0)] bg-[var(--ctp-canvas)] text-[var(--ctp-subtext1)] hover:border-[var(--ctp-accent)] hover:text-[var(--ctp-text)]'
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
                  class="inline-flex items-center gap-1 rounded-lg border border-[var(--ctp-surface0)] bg-[var(--ctp-canvas)] px-2 py-1 text-[11px] font-medium text-[var(--ctp-subtext1)] transition hover:border-[var(--ctp-accent)] hover:text-[var(--ctp-text)]"
                  @click="regexPanelOpen = !regexPanelOpen"
                >
                  <LuChevronDown v-if="regexPanelOpen" class="h-3 w-3" />
                  <LuChevronRight v-else class="h-3 w-3" />
                  <span>分章规则</span>
                </button>

                <button
                  type="button"
                  class="inline-flex items-center gap-1 rounded-lg bg-[var(--ctp-accent-soft)] px-2 py-1 text-[11px] font-semibold text-[var(--ctp-accent)] transition hover:opacity-90"
                  title="在当前章节下方插入新章"
                  @click="insertChapterBelow(activeIndex)"
                >
                  <LuPlus class="h-3 w-3" />
                  <span>新建章</span>
                </button>
              </div>
            </div>

            <div class="flex items-center gap-1.5">
              <div class="relative min-w-0 flex-1">
                <LuSearch
                  class="pointer-events-none absolute top-1/2 left-2.5 h-3.5 w-3.5 -translate-y-1/2 text-[var(--ctp-subtext0)]"
                />
                <input
                  v-model="tocSearchQuery"
                  type="text"
                  placeholder="搜索定位章节标题…"
                  class="h-8 w-full rounded-xl border border-[var(--ctp-surface0)] bg-[var(--ctp-canvas)] pr-7 pl-8 text-xs text-[var(--ctp-text)] outline-none focus:border-[var(--ctp-accent)]"
                />
                <button
                  v-if="tocSearchQuery"
                  type="button"
                  class="absolute top-1/2 right-2 -translate-y-1/2 text-[var(--ctp-subtext0)] hover:text-[var(--ctp-text)]"
                  @click="tocSearchQuery = ''"
                >
                  <LuX class="h-3 w-3" />
                </button>
              </div>

              <button
                v-if="anomalyCount > 0 || onlyShowAnomalies"
                type="button"
                class="inline-flex h-8 shrink-0 items-center gap-1 rounded-xl border px-2 text-[11px] font-semibold transition"
                :class="
                  onlyShowAnomalies
                    ? 'border-amber-500 bg-amber-500/15 text-amber-500'
                    : 'border-amber-500/35 bg-amber-500/8 text-amber-500 hover:bg-amber-500/15'
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
            class="space-y-1.5 border-b border-[var(--ctp-surface0)]/70 bg-[var(--ctp-mantle)]/65 p-2 text-[11px]"
          >
            <div class="grid grid-cols-2 gap-1.5">
              <button
                type="button"
                class="rounded-lg border border-[var(--ctp-surface0)] bg-[var(--ctp-canvas)] px-2 py-1.5 font-medium text-[var(--ctp-text)] transition hover:border-[var(--ctp-accent)] hover:text-[var(--ctp-accent)]"
                @click="autoDetectTocHierarchy"
              >
                自动识别卷/章层级
              </button>
              <button
                type="button"
                class="rounded-lg border border-[var(--ctp-surface0)] bg-[var(--ctp-canvas)] px-2 py-1.5 font-medium text-[var(--ctp-text)] transition hover:border-[var(--ctp-accent)] hover:text-[var(--ctp-accent)]"
                @click="flattenAllTocLevels"
              >
                全部平铺为一级
              </button>
            </div>
            <button
              type="button"
              class="w-full rounded-lg border border-[var(--ctp-surface0)] bg-[var(--ctp-canvas)] px-2 py-1.5 font-medium text-[var(--ctp-text)] transition hover:border-[var(--ctp-accent)] hover:text-[var(--ctp-accent)]"
              @click="syncTocTitlesFromBodyHeadings"
            >
              从各章正文 H1~H3 标题同步目录名
            </button>
          </div>

          <!-- Collapsible TXT Regex Panel -->
          <div
            v-if="docFormat === 'txt' && regexPanelOpen"
            class="space-y-2 border-b border-[var(--ctp-surface0)]/70 bg-[var(--ctp-mantle)]/65 p-2.5 text-xs"
          >
            <div class="flex flex-wrap gap-1">
              <button
                v-for="p in regexPresets"
                :key="p.label"
                type="button"
                class="rounded-md border border-[var(--ctp-surface0)] bg-[var(--ctp-canvas)] px-2 py-0.5 text-[10px] font-medium text-[var(--ctp-subtext1)] transition hover:border-[var(--ctp-accent)] hover:text-[var(--ctp-accent)]"
                @click="chapterRegexInput = p.chapter; volumeRegexInput = p.volume"
              >
                {{ p.label }}
              </button>
            </div>
            <input
              v-model="chapterRegexInput"
              type="text"
              placeholder="章节标题正则（留空默认）"
              class="h-7 w-full rounded-lg border border-[var(--ctp-surface0)] bg-[var(--ctp-canvas)] px-2 font-mono text-[11px] outline-none focus:border-[var(--ctp-accent)]"
            />
            <input
              v-model="volumeRegexInput"
              type="text"
              placeholder="分卷标题正则（可选）"
              class="h-7 w-full rounded-lg border border-[var(--ctp-surface0)] bg-[var(--ctp-canvas)] px-2 font-mono text-[11px] outline-none focus:border-[var(--ctp-accent)]"
            />
            <button
              type="button"
              :disabled="resegmenting"
              class="inline-flex w-full items-center justify-center gap-1.5 rounded-lg bg-[var(--ctp-accent)] py-1.5 text-[11px] font-semibold text-[var(--ctp-on-accent)] transition hover:opacity-95 disabled:opacity-50"
              @click="handleResegmentTxt"
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
                  ? 'bg-[var(--ctp-accent-soft)] text-[var(--ctp-accent)]'
                  : 'text-[var(--ctp-subtext1)] hover:bg-[var(--ctp-surface0)]/55 hover:text-[var(--ctp-text)]',
                dragOverIdx === idx && dragDropPos === 'before'
                  ? 'border-t-2 border-[var(--ctp-accent)]'
                  : '',
                dragOverIdx === idx && dragDropPos === 'after'
                  ? 'border-b-2 border-[var(--ctp-accent)]'
                  : '',
              ]"
              @dragstart="onTocDragStart(idx, $event)"
              @dragover.prevent="onTocDragOver(idx, $event)"
              @dragend="onTocDragEnd"
              @drop.prevent="onTocDrop(idx)"
              @click="selectChapter(idx)"
              @dblclick.stop="startInlineRename(idx)"
            >
              <LuGripVertical
                v-if="!tocSearchQuery"
                class="h-3.5 w-3.5 shrink-0 cursor-grab opacity-35 group-hover:opacity-80"
              />

              <button
                type="button"
                class="shrink-0 rounded px-1 py-0.2 font-mono text-[9px] font-bold transition hover:bg-[var(--ctp-canvas)]"
                :class="
                  ch.level === 0
                    ? 'bg-[var(--ctp-surface0)]/75 text-[var(--ctp-text)]'
                    : 'bg-[var(--ctp-mantle)] text-[var(--ctp-subtext0)]'
                "
                title="点击切换顶层卷/章 (L0) 与子章 (L1)"
                @click.stop="toggleLevel(idx)"
              >
                {{ ch.level === 0 ? '卷/章' : '子章' }}
              </button>

              <!-- Inline Rename Input vs Title Label -->
              <input
                v-if="renamingIdx === idx"
                v-model="renameDraft"
                type="text"
                autofocus
                class="h-6 min-w-0 flex-1 rounded border border-[var(--ctp-accent)] bg-[var(--ctp-canvas)] px-1.5 text-xs text-[var(--ctp-text)] outline-none"
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
                    ? 'bg-amber-500/15 text-amber-500'
                    : 'bg-rose-500/15 text-rose-400'
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
                  class="rounded p-1 text-[var(--ctp-subtext0)] hover:bg-[var(--ctp-canvas)] hover:text-[var(--ctp-text)]"
                  title="重命名章节标题"
                  @click="startInlineRename(idx)"
                >
                  <LuPencil class="h-3 w-3" />
                </button>
                <button
                  v-if="idx > 0"
                  type="button"
                  class="rounded p-1 text-[var(--ctp-subtext0)] hover:bg-[var(--ctp-canvas)] hover:text-[var(--ctp-accent)]"
                  title="合并到上一章"
                  @click="mergeWithPrevious(idx)"
                >
                  <LuMerge class="h-3 w-3" />
                </button>
                <button
                  type="button"
                  class="rounded p-1 text-[var(--ctp-subtext0)] hover:bg-[var(--ctp-canvas)] hover:text-red-500"
                  title="删除章节"
                  @click="deleteChapterAt(idx)"
                >
                  <LuTrash2 class="h-3 w-3" />
                </button>
              </div>
            </div>
          </div>
        </aside>

        <!-- COLUMN 2: Zero-Latency WYSIWYG / XHTML Editor -->
        <div class="flex min-w-0 flex-1 flex-col bg-[var(--ctp-canvas)]">
          <!-- Formatting & View Mode Toolbar -->
          <div
            class="flex shrink-0 flex-wrap items-center justify-between gap-2 border-b border-[var(--ctp-surface0)]/80 bg-[var(--ctp-mantle)]/45 px-3 py-2 select-none"
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

                <span class="mx-1 h-4 w-px bg-[var(--ctp-surface0)]" />
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

              <span class="mx-1 h-4 w-px bg-[var(--ctp-surface0)]" />

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

              <span class="mx-1 h-4 w-px bg-[var(--ctp-surface0)]" />

              <button
                type="button"
                class="inline-flex h-7 items-center gap-1 rounded-lg px-2 text-[11px] font-medium text-[var(--ctp-subtext1)] transition hover:bg-[var(--ctp-surface0)]/70 hover:text-[var(--ctp-text)]"
                title="在当前光标位置拆分为上下两章 (Ctrl+Shift+Enter)"
                @mousedown.prevent="splitChapterAtCursor"
              >
                <LuScissors class="h-3.5 w-3.5 text-[var(--ctp-accent)]" />
                <span>光标拆章</span>
              </button>

              <button
                type="button"
                class="inline-flex h-7 items-center gap-1 rounded-lg px-2 text-[11px] font-medium text-[var(--ctp-subtext1)] transition hover:bg-[var(--ctp-surface0)]/70 hover:text-[var(--ctp-text)]"
                title="自动清理本章空段落与段首全角/半角冗余空格"
                @click="cleanChapterTypography"
              >
                <LuSparkles class="h-3.5 w-3.5 text-[var(--ctp-accent)]" />
                <span>一键净版</span>
              </button>

              <button
                v-if="docFormat === 'epub'"
                type="button"
                :disabled="uploadingImage"
                class="inline-flex h-7 items-center gap-1 rounded-lg px-2 text-[11px] font-medium text-[var(--ctp-subtext1)] transition hover:bg-[var(--ctp-surface0)]/70 hover:text-[var(--ctp-text)] disabled:opacity-50"
                title="上传图片写入 EPUB 资源包并插入当前光标处"
                @mousedown.prevent="triggerEpubImageUpload"
              >
                <LuImage class="h-3.5 w-3.5 text-[var(--ctp-accent)]" />
                <span>{{ uploadingImage ? '上传中…' : '插入图片' }}</span>
              </button>

              <button
                type="button"
                class="inline-flex h-7 items-center gap-1 rounded-lg px-2 text-[11px] font-medium transition"
                :class="
                  proofreadPanelOpen
                    ? 'bg-[var(--ctp-accent-soft)] text-[var(--ctp-accent)]'
                    : 'text-[var(--ctp-subtext1)] hover:bg-[var(--ctp-surface0)]/70 hover:text-[var(--ctp-text)]'
                "
                title="简繁转换、台版竖排转横排、网文求票净化与中文标点智能规整"
                @click="proofreadPanelOpen = !proofreadPanelOpen; if (proofreadPanelOpen) cssPanelOpen = false"
              >
                <LuLanguages class="h-3.5 w-3.5" />
                <span>简繁与精校</span>
              </button>

              <button
                type="button"
                class="inline-flex h-7 items-center gap-1 rounded-lg px-2 text-[11px] font-medium transition"
                :class="
                  cssPanelOpen
                    ? 'bg-[var(--ctp-accent-soft)] text-[var(--ctp-accent)]'
                    : 'text-[var(--ctp-subtext1)] hover:bg-[var(--ctp-surface0)]/70 hover:text-[var(--ctp-text)]'
                "
                title="切换中文排版 CSS 预设与编辑全局样式表"
                @click="cssPanelOpen = !cssPanelOpen; if (cssPanelOpen) proofreadPanelOpen = false"
              >
                <LuPalette class="h-3.5 w-3.5" />
                <span>排版样式</span>
              </button>

              <button
                type="button"
                class="inline-flex h-7 items-center gap-1 rounded-lg px-2 text-[11px] font-medium transition"
                :class="
                  findBarOpen
                    ? 'bg-[var(--ctp-accent-soft)] text-[var(--ctp-accent)]'
                    : 'text-[var(--ctp-subtext1)] hover:bg-[var(--ctp-surface0)]/70 hover:text-[var(--ctp-text)]'
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
                class="rounded-lg border px-2 py-1 text-[11px] font-medium transition"
                :class="
                  indentParagraphs
                    ? 'border-[var(--ctp-accent)]/40 bg-[var(--ctp-accent-soft)] text-[var(--ctp-accent)]'
                    : 'border-[var(--ctp-surface0)] bg-[var(--ctp-canvas)] text-[var(--ctp-subtext0)]'
                "
                title="切换段落首行缩进两字符显示"
                @click="indentParagraphs = !indentParagraphs"
              >
                首行缩进
              </button>

              <div
                class="inline-flex items-center gap-0.5 rounded-xl border border-[var(--ctp-surface0)] bg-[var(--ctp-shell)] p-0.5"
              >
                <button
                  type="button"
                  class="rounded-lg px-2.5 py-1 text-[11px] font-semibold transition"
                  :class="
                    surfaceMode === 'wysiwyg'
                      ? 'bg-[var(--ctp-accent)] text-[var(--ctp-on-accent)]'
                      : 'text-[var(--ctp-subtext1)] hover:text-[var(--ctp-text)]'
                  "
                  @click="switchSurfaceMode('wysiwyg')"
                >
                  所见即所得
                </button>
                <button
                  type="button"
                  class="rounded-lg px-2.5 py-1 text-[11px] font-semibold transition"
                  :class="
                    surfaceMode === 'xhtml'
                      ? 'bg-[var(--ctp-accent)] text-[var(--ctp-on-accent)]'
                      : 'text-[var(--ctp-subtext1)] hover:text-[var(--ctp-text)]'
                  "
                  @click="switchSurfaceMode('xhtml')"
                >
                  XHTML 源码
                </button>
              </div>

              <button
                type="button"
                class="inline-flex h-7 items-center gap-1 rounded-xl border px-2.5 text-[11px] font-semibold transition"
                :class="
                  showLivePreview
                    ? 'border-[var(--ctp-accent)] bg-[var(--ctp-accent-soft)] text-[var(--ctp-accent)]'
                    : 'border-[var(--ctp-surface0)] bg-[var(--ctp-canvas)] text-[var(--ctp-subtext1)] hover:text-[var(--ctp-text)]'
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
          <div
            v-if="proofreadPanelOpen"
            class="space-y-2 border-b border-[var(--ctp-surface0)]/80 bg-[var(--ctp-shell)] px-4 py-2.5 text-xs"
          >
            <div class="flex flex-wrap items-center justify-between gap-2">
              <div class="flex flex-wrap items-center gap-1.5">
                <div class="inline-flex rounded-lg border border-[var(--ctp-surface0)] bg-[var(--ctp-canvas)] p-0.5">
                  <button
                    type="button"
                    class="rounded px-2 py-0.5 text-[11px] font-semibold"
                    :class="transformScope === 'book' ? 'bg-[var(--ctp-accent)] text-[var(--ctp-on-accent)]' : 'text-[var(--ctp-subtext1)]'"
                    @click="transformScope = 'book'"
                  >
                    全书范围
                  </button>
                  <button
                    type="button"
                    class="rounded px-2 py-0.5 text-[11px] font-semibold"
                    :class="transformScope === 'chapter' ? 'bg-[var(--ctp-accent)] text-[var(--ctp-on-accent)]' : 'text-[var(--ctp-subtext1)]'"
                    @click="transformScope = 'chapter'"
                  >
                    仅当前章
                  </button>
                </div>

                <button
                  type="button"
                  :disabled="transforming"
                  class="rounded-lg border border-[var(--ctp-surface0)] bg-[var(--ctp-canvas)] px-2.5 py-1 text-[11px] font-semibold text-[var(--ctp-text)] transition hover:border-[var(--ctp-accent)] hover:text-[var(--ctp-accent)] disabled:opacity-50"
                  title="OpenCC 级繁转简（含台港词汇转换，保护 XHTML 标签）"
                  @click="runBackendTextTransform({ zh_direction: 't2s' }, '繁转简')"
                >
                  繁 → 简 (台港词库)
                </button>
                <button
                  type="button"
                  :disabled="transforming"
                  class="rounded-lg border border-[var(--ctp-surface0)] bg-[var(--ctp-canvas)] px-2.5 py-1 text-[11px] font-semibold text-[var(--ctp-text)] transition hover:border-[var(--ctp-accent)] hover:text-[var(--ctp-accent)] disabled:opacity-50"
                  title="简转繁（内置多音字词组消歧，避免后/发/干/里误转）"
                  @click="runBackendTextTransform({ zh_direction: 's2t' }, '简转繁')"
                >
                  简 → 繁 (词组消歧)
                </button>
                <button
                  type="button"
                  :disabled="transforming"
                  class="rounded-lg border border-[var(--ctp-surface0)] bg-[var(--ctp-canvas)] px-2.5 py-1 text-[11px] font-semibold text-[var(--ctp-text)] transition hover:border-[var(--ctp-accent)] hover:text-[var(--ctp-accent)] disabled:opacity-50"
                  title="将台版/日版竖排 CSS 改为横排、直角引号「」改为弯引号，并在保存时同步 LTR 翻页方向"
                  @click="runBackendTextTransform({ vertical_to_horizontal: true, convert_quotes_to_horizontal: true }, '竖排转横排')"
                >
                  竖排 → 横排 (CSS + 引号)
                </button>
              </div>

              <button
                type="button"
                class="rounded p-1 text-[var(--ctp-subtext0)] hover:text-[var(--ctp-text)]"
                @click="proofreadPanelOpen = false"
              >
                <LuX class="h-3.5 w-3.5" />
              </button>
            </div>

            <div class="flex flex-wrap items-center justify-between gap-2 border-t border-[var(--ctp-surface0)]/55 pt-2">
              <div class="flex flex-wrap items-center gap-3">
                <AppCheckbox v-model="cleanWebnovelAds" label="去网文求票/域名水印行" />
                <AppCheckbox v-model="normalizePunctuation" label="中文标点规整 (弯引号/省略号/破折号)" />
                <AppCheckbox v-model="convertCornerQuotes" label="直角引号「」→“”" />
                <button
                  type="button"
                  class="text-[11px] font-medium text-[var(--ctp-accent)] hover:underline"
                  @click="customStripRulesOpen = !customStripRulesOpen"
                >
                  {{ customStripRulesOpen ? '收起自定义正则' : '自定义净化正则' }}
                </button>
              </div>

              <button
                type="button"
                :disabled="transforming"
                class="inline-flex items-center gap-1 rounded-lg bg-[var(--ctp-accent)] px-3 py-1 text-[11px] font-semibold text-[var(--ctp-on-accent)] transition hover:opacity-95 disabled:opacity-50"
                @click="handleRunProofreadClean"
              >
                <LuSparkles class="h-3 w-3" />
                <span>{{ transforming ? '处理中…' : '执行精校净化' }}</span>
              </button>
            </div>

            <div v-if="customStripRulesOpen" class="pt-1">
              <textarea
                v-model="customStripRulesText"
                rows="3"
                placeholder="每行输入一条整行匹配剔除正则（自动保存在浏览器本地），例如：^.*本书由某某论坛整理.*$"
                class="w-full rounded-lg border border-[var(--ctp-surface0)] bg-[var(--ctp-canvas)] p-2 font-mono text-[11px] text-[var(--ctp-text)] outline-none focus:border-[var(--ctp-accent)]"
              />
            </div>
          </div>

          <!-- Collapsible Global EPUB CSS Presets & Stylesheet Editor -->
          <div
            v-if="cssPanelOpen"
            class="space-y-2 border-b border-[var(--ctp-surface0)]/80 bg-[var(--ctp-shell)] px-4 py-2.5 text-xs"
          >
            <div class="flex flex-wrap items-center justify-between gap-2">
              <div class="flex flex-wrap items-center gap-1.5">
                <span class="font-semibold text-[var(--ctp-subtext1)]">中文排版预设：</span>
                <button
                  v-for="preset in cssPresets"
                  :key="preset.id"
                  type="button"
                  class="rounded-lg border border-[var(--ctp-surface0)] bg-[var(--ctp-canvas)] px-2.5 py-1 text-[11px] font-medium text-[var(--ctp-text)] transition hover:border-[var(--ctp-accent)] hover:text-[var(--ctp-accent)]"
                  :title="preset.desc"
                  @click="applyCssPreset(preset.css, preset.label)"
                >
                  {{ preset.label }}
                </button>
              </div>
              <button
                type="button"
                class="rounded p-1 text-[var(--ctp-subtext0)] hover:text-[var(--ctp-text)]"
                @click="cssPanelOpen = false"
              >
                <LuX class="h-3.5 w-3.5" />
              </button>
            </div>
            <textarea
              v-model="primaryCssDraft"
              rows="5"
              spellcheck="false"
              placeholder="编辑全局 EPUB CSS 样式表（保存 EPUB 或 TXT 转 EPUB 时自动写入样式文件，右侧预览分屏实时生效）…"
              class="w-full rounded-lg border border-[var(--ctp-surface0)] bg-[var(--ctp-canvas)] p-2 font-mono text-[11px] leading-relaxed text-[var(--ctp-text)] outline-none focus:border-[var(--ctp-accent)]"
              @input="syncPrimaryCssDraftToStylesheets"
            />
          </div>

          <!-- Collapsible Find & Replace Bar -->
          <div
            v-if="findBarOpen"
            class="flex shrink-0 flex-wrap items-center gap-2 border-b border-[var(--ctp-surface0)]/80 bg-[var(--ctp-shell)] px-4 py-2 text-xs"
          >
            <input
              ref="findInputRef"
              v-model="findQuery"
              type="text"
              placeholder="查找内容…"
              class="h-7 w-44 rounded-lg border border-[var(--ctp-surface0)] bg-[var(--ctp-canvas)] px-2.5 text-xs outline-none focus:border-[var(--ctp-accent)]"
            />
            <input
              v-model="replaceQuery"
              type="text"
              placeholder="替换为…"
              class="h-7 w-44 rounded-lg border border-[var(--ctp-surface0)] bg-[var(--ctp-canvas)] px-2.5 text-xs outline-none focus:border-[var(--ctp-accent)]"
            />

            <div class="inline-flex rounded-lg border border-[var(--ctp-surface0)] bg-[var(--ctp-canvas)] p-0.5">
              <button
                type="button"
                class="rounded px-2 py-0.5 text-[11px] font-semibold"
                :class="
                  findScope === 'chapter'
                    ? 'bg-[var(--ctp-accent)] text-[var(--ctp-on-accent)]'
                    : 'text-[var(--ctp-subtext1)]'
                "
                @click="findScope = 'chapter'"
              >
                当前章
              </button>
              <button
                type="button"
                class="rounded px-2 py-0.5 text-[11px] font-semibold"
                :class="
                  findScope === 'book'
                    ? 'bg-[var(--ctp-accent)] text-[var(--ctp-on-accent)]'
                    : 'text-[var(--ctp-subtext1)]'
                "
                @click="findScope = 'book'"
              >
                全书
              </button>
            </div>

            <button
              type="button"
              class="rounded-lg border px-2 py-0.5 font-mono text-[11px] font-bold"
              :class="
                findCaseSensitive
                  ? 'border-[var(--ctp-accent)] bg-[var(--ctp-accent-soft)] text-[var(--ctp-accent)]'
                  : 'border-[var(--ctp-surface0)] text-[var(--ctp-subtext0)]'
              "
              title="区分大小写"
              @click="findCaseSensitive = !findCaseSensitive"
            >
              Aa
            </button>
            <button
              type="button"
              class="rounded-lg border px-2 py-0.5 font-mono text-[11px] font-bold"
              :class="
                findUseRegex
                  ? 'border-[var(--ctp-accent)] bg-[var(--ctp-accent-soft)] text-[var(--ctp-accent)]'
                  : 'border-[var(--ctp-surface0)] text-[var(--ctp-subtext0)]'
              "
              title="使用正则表达式"
              @click="findUseRegex = !findUseRegex"
            >
              .*
            </button>

            <span v-if="findQuery" class="font-mono text-[11px] text-[var(--ctp-subtext0)]">
              {{ searchMatchSummary }}
            </span>

            <div class="ml-auto flex items-center gap-1.5">
              <button
                type="button"
                :disabled="!findQuery"
                class="inline-flex h-7 items-center gap-1 rounded-lg border border-[var(--ctp-surface0)] bg-[var(--ctp-canvas)] px-2.5 text-[11px] font-medium text-[var(--ctp-text)] hover:border-[var(--ctp-accent)] disabled:opacity-45"
                @click="handleReplaceSingle"
              >
                <LuReplace class="h-3 w-3" />
                <span>替换当前</span>
              </button>
              <button
                type="button"
                :disabled="!findQuery"
                class="inline-flex h-7 items-center gap-1 rounded-lg bg-[var(--ctp-accent)] px-2.5 text-[11px] font-semibold text-[var(--ctp-on-accent)] hover:opacity-95 disabled:opacity-45"
                @click="handleReplaceAll"
              >
                <LuReplaceAll class="h-3 w-3" />
                <span>全部替换</span>
              </button>
              <button
                type="button"
                class="rounded p-1 text-[var(--ctp-subtext0)] hover:text-[var(--ctp-text)]"
                @click="findBarOpen = false"
              >
                <LuX class="h-3.5 w-3.5" />
              </button>
            </div>
          </div>

          <!-- Chapter Title Header Input -->
          <div class="flex shrink-0 items-center gap-3 border-b border-[var(--ctp-surface0)]/55 px-6 py-2.5">
            <span class="shrink-0 font-mono text-[11px] text-[var(--ctp-subtext0)]">
              #{{ activeIndex + 1 }}
            </span>
            <input
              :value="activeTitle"
              type="text"
              placeholder="输入目录章节标题…"
              class="min-w-0 flex-1 bg-transparent text-base font-bold text-[var(--ctp-text)] outline-none placeholder:text-[var(--ctp-subtext0)]/50"
              @input="onActiveTitleInput(($event.target as HTMLInputElement).value)"
            />
            <span class="shrink-0 font-mono text-[11px] text-[var(--ctp-subtext0)]">
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
              class="h-full w-full resize-none bg-transparent p-5 font-mono text-xs leading-relaxed text-[var(--ctp-text)] outline-none"
              @input="onSourceTextareaInput"
              @keydown="onSourceKeydown"
            />
          </div>
        </div>

        <!-- COLUMN 3: Toggleable Real EPUB Stylesheet & Image Live Preview -->
        <aside
          v-if="showLivePreview"
          class="hidden w-[38%] max-w-xl shrink-0 flex-col border-l border-[var(--ctp-surface0)]/80 bg-[var(--ctp-shell)] lg:flex"
        >
          <div class="flex h-9 shrink-0 items-center justify-between border-b border-[var(--ctp-surface0)]/65 px-3.5 text-xs">
            <span class="font-semibold text-[var(--ctp-subtext1)]">阅读器实时排版预览</span>
            <span class="text-[10px] text-[var(--ctp-subtext0)]">已加载包内 CSS 与原图</span>
          </div>
          <iframe
            :srcdoc="previewSrcdoc"
            title="EPUB Live Preview"
            sandbox="allow-same-origin"
            class="min-h-0 w-full flex-1 border-0 bg-[var(--ctp-canvas)]"
          />
        </aside>

        <!-- Floating Status Toast -->
        <div
          v-if="statusToast"
          class="pointer-events-none fixed bottom-6 left-1/2 z-50 -translate-x-1/2 rounded-full border border-[var(--ctp-accent)]/35 bg-[var(--ctp-shell)]/95 px-4 py-2 text-xs font-semibold text-[var(--ctp-text)] shadow-xl backdrop-blur-md"
        >
          {{ statusToast }}
        </div>
      </div>
    </div>
  </Teleport>
</template>

<style scoped>
@reference "tailwindcss";

.tb-btn {
  @apply flex h-7 w-7 items-center justify-center rounded-lg text-[var(--ctp-subtext1)] transition hover:bg-[var(--ctp-surface0)]/70 hover:text-[var(--ctp-text)];
}

.epub-wysiwyg-surface {
  font-family: "Noto Serif CJK SC", "Songti SC", "PingFang SC", "Microsoft YaHei", Georgia, serif;
  font-size: 16px;
  line-height: 1.9;
  color: var(--ctp-text);
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
  border-left: 3px solid var(--ctp-accent);
  background: color-mix(in srgb, var(--ctp-mantle) 60%, transparent);
  border-radius: 0 0.5rem 0.5rem 0;
}

.epub-wysiwyg-surface :deep(code) {
  font-family: ui-monospace, SFMono-Regular, Menlo, Monaco, Consolas, monospace;
  font-size: 0.88em;
  padding: 0.15em 0.4em;
  border-radius: 0.35rem;
  background: var(--ctp-surface0);
}

.epub-wysiwyg-surface :deep(hr) {
  border: none;
  border-top: 1px solid var(--ctp-surface1);
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

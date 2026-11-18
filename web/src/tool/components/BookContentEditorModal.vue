<script setup lang="ts">
import { computed, nextTick, ref, watch } from 'vue'
import {
  LuAlertTriangle,
  LuArrowDown,
  LuArrowUp,
  LuBookOpen,
  LuChevronDown,
  LuChevronRight,
  LuFileText,
  LuLoader2,
  LuPlus,
  LuRefreshCw,
  LuSave,
  LuTrash2,
  LuX,
} from 'vue-icons-plus/lu'
import AppCheckbox from '@/components/ui/AppCheckbox.vue'
import { useDialogStore } from '@/stores/dialog'
import { toolApi } from '../api'
import type {
  ChapterEditMode,
  EditableBookChapterDto,
  MediaFileDetail,
  MediaFileEntry,
  SaveBookChapterInput,
} from '../types'

interface LocalChapterState {
  key: string
  title: string
  level: number
  mode: ChapterEditMode
  plainText: string
  xhtmlBody: string
  hasRichMedia: boolean
  sourceHref: string | null
  plainDirtySinceSync: boolean
  xhtmlDirtySinceSync: boolean
}

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

const loading = ref(false)
const saving = ref(false)
const converting = ref(false)
const resegmenting = ref(false)
const errorMsg = ref<string | null>(null)

const docFormat = ref<'txt' | 'epub'>('txt')
const docTitle = ref('')
const docAuthors = ref<string[]>([])
const chapters = ref<LocalChapterState[]>([])
const activeIndex = ref(0)
const hasUnsavedEdits = ref(false)

// TXT regex re-segmentation controls
const regexPanelOpen = ref(false)
const chapterRegexInput = ref('')
const volumeRegexInput = ref('')
const deleteSourceTxt = ref(false)

const editorTextareaRef = ref<HTMLTextAreaElement | null>(null)
let localKeySeq = 1

const regexPresets = [
  {
    label: '默认中英卷章',
    chapter: '',
    volume: '',
  },
  {
    label: '数字序号标题 (1. / 01)',
    chapter: '^\\s*\\d{1,4}[\\.、\\s]\\s*\\S+.*$',
    volume: '',
  },
  {
    label: '全第X章/回/话',
    chapter: '^\\s*第[零一二三四五六七八九十百千万两0-9]+[章节回话集].*$',
    volume: '^\\s*第[零一二三四五六七八九十百千万两0-9]+[卷部篇册].*$',
  },
  {
    label: 'English Chapters',
    chapter: '(?i)^\\s*(?:chapter|section|prologue|epilogue)\\b.*$',
    volume: '(?i)^\\s*(?:volume|book|part)\\b.*$',
  },
]

function escapeXmlText(raw: string): string {
  return raw
    .replace(/&/g, '&amp;')
    .replace(/</g, '&lt;')
    .replace(/>/g, '&gt;')
    .replace(/"/g, '&quot;')
}

function buildXhtmlFromPlain(title: string, plain: string): string {
  const lines: string[] = []
  const t = title.trim()
  if (t) {
    lines.push(`  <h2>${escapeXmlText(t)}</h2>`)
  }
  for (const rawLine of plain.split('\n')) {
    const trimmed = rawLine.trim()
    if (!trimmed) continue
    lines.push(`  <p>${escapeXmlText(trimmed)}</p>`)
  }
  return lines.join('\n')
}

function extractPlainFromXhtml(xhtml: string, stripHeading?: string): string {
  const doc = new DOMParser().parseFromString(`<body>${xhtml}</body>`, 'text/html')
  const body = doc.body
  if (!body) return xhtml
  body.querySelectorAll('br').forEach((br) => br.replaceWith('\n'))
  body
    .querySelectorAll('p, div, h1, h2, h3, h4, h5, h6, li, blockquote, section, article, tr')
    .forEach((el) => {
      el.append('\n')
    })
  const raw = body.textContent ?? ''
  const paras = raw
    .split('\n')
    .map((l) => l.replace(/\s+/g, ' ').trim())
    .filter(Boolean)

  if (stripHeading && paras[0] === stripHeading.trim()) {
    paras.shift()
  }
  return paras.join('\n')
}

function mapDtoToLocal(dtoList: EditableBookChapterDto[]): LocalChapterState[] {
  return dtoList.map((c) => ({
    key: `local_${localKeySeq++}_${c.id}`,
    title: c.title,
    level: c.level > 0 ? 1 : 0,
    mode: c.has_rich_media ? 'xhtml' : 'plain',
    plainText: c.plain_text,
    xhtmlBody: c.xhtml_body || buildXhtmlFromPlain(c.title, c.plain_text),
    hasRichMedia: c.has_rich_media,
    sourceHref: c.source_href,
    plainDirtySinceSync: false,
    xhtmlDirtySinceSync: false,
  }))
}

async function loadDocument(): Promise<void> {
  if (!props.entry) return
  loading.value = true
  errorMsg.value = null
  hasUnsavedEdits.value = false
  regexPanelOpen.value = false
  try {
    const doc = await toolApi.getBookContent(props.entry.root_id, props.entry.rel_path)
    docFormat.value = doc.format
    docTitle.value = doc.title
    docAuthors.value = doc.authors
    chapters.value = mapDtoToLocal(doc.chapters)
    activeIndex.value = 0
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

const currentChapter = computed<LocalChapterState | null>(
  () => chapters.value[activeIndex.value] ?? null,
)

const totalCharCount = computed(() =>
  chapters.value.reduce(
    (sum, ch) =>
      sum +
      (ch.mode === 'plain' ? ch.plainText.replace(/\s+/g, '').length : ch.xhtmlBody.length),
    0,
  ),
)

const currentCharCount = computed(() => {
  const ch = currentChapter.value
  if (!ch) return 0
  return ch.mode === 'plain'
    ? ch.plainText.replace(/\s+/g, '').length
    : ch.xhtmlBody.length
})

function switchChapterMode(nextMode: ChapterEditMode): void {
  const ch = currentChapter.value
  if (!ch || ch.mode === nextMode) return

  if (nextMode === 'xhtml') {
    if (ch.plainDirtySinceSync || !ch.xhtmlBody.trim()) {
      ch.xhtmlBody = buildXhtmlFromPlain(ch.title, ch.plainText)
      ch.plainDirtySinceSync = false
    }
    ch.mode = 'xhtml'
  } else {
    if (ch.xhtmlDirtySinceSync) {
      ch.plainText = extractPlainFromXhtml(ch.xhtmlBody, ch.title)
      ch.xhtmlDirtySinceSync = false
    }
    ch.mode = 'plain'
  }
}

function onTitleInput(val: string): void {
  const ch = currentChapter.value
  if (!ch) return
  ch.title = val
  hasUnsavedEdits.value = true
}

function onBodyInput(val: string): void {
  const ch = currentChapter.value
  if (!ch) return
  if (ch.mode === 'plain') {
    ch.plainText = val
    ch.plainDirtySinceSync = true
  } else {
    ch.xhtmlBody = val
    ch.xhtmlDirtySinceSync = true
  }
  hasUnsavedEdits.value = true
}

function toggleChapterLevel(idx = activeIndex.value): void {
  const ch = chapters.value[idx]
  if (!ch) return
  ch.level = ch.level === 0 ? 1 : 0
  hasUnsavedEdits.value = true
}

function moveChapter(idx: number, delta: -1 | 1): void {
  const target = idx + delta
  if (target < 0 || target >= chapters.value.length) return
  const list = [...chapters.value]
  const [moved] = list.splice(idx, 1)
  list.splice(target, 0, moved!)
  chapters.value = list
  activeIndex.value = target
  hasUnsavedEdits.value = true
}

function insertChapterAfter(idx: number): void {
  const refLevel = chapters.value[idx]?.level ?? 0
  const newChap: LocalChapterState = {
    key: `local_new_${localKeySeq++}`,
    title: `新章节 ${chapters.value.length + 1}`,
    level: refLevel,
    mode: 'plain',
    plainText: '',
    xhtmlBody: '',
    hasRichMedia: false,
    sourceHref: null,
    plainDirtySinceSync: true,
    xhtmlDirtySinceSync: false,
  }
  const list = [...chapters.value]
  list.splice(idx + 1, 0, newChap)
  chapters.value = list
  activeIndex.value = idx + 1
  hasUnsavedEdits.value = true
}

function mergeIntoPrevious(idx: number): void {
  if (idx <= 0 || idx >= chapters.value.length) return
  const prev = chapters.value[idx - 1]!
  const cur = chapters.value[idx]!

  if (prev.mode === 'xhtml' || cur.mode === 'xhtml') {
    const prevX =
      prev.mode === 'xhtml' ? prev.xhtmlBody : buildXhtmlFromPlain(prev.title, prev.plainText)
    const curX =
      cur.mode === 'xhtml' ? cur.xhtmlBody : buildXhtmlFromPlain(cur.title, cur.plainText)
    prev.mode = 'xhtml'
    prev.xhtmlBody = [prevX.trim(), curX.trim()].filter(Boolean).join('\n\n')
    prev.xhtmlDirtySinceSync = true
    prev.hasRichMedia = prev.hasRichMedia || cur.hasRichMedia
  } else {
    const mergedParts = [prev.plainText.trim(), cur.title.trim(), cur.plainText.trim()].filter(
      Boolean,
    )
    prev.plainText = mergedParts.join('\n\n')
    prev.plainDirtySinceSync = true
  }

  const list = [...chapters.value]
  list.splice(idx, 1)
  chapters.value = list
  activeIndex.value = idx - 1
  hasUnsavedEdits.value = true
}

async function deleteChapter(idx: number): Promise<void> {
  if (chapters.value.length <= 1) {
    errorMsg.value = '图书至少需要保留一个章节'
    return
  }
  const target = chapters.value[idx]
  if (!target) return
  const ok = await dialogStore.confirm({
    title: '删除章节',
    message: `确定删除章节「${target.title}」及其正文内容吗？`,
    danger: true,
  })
  if (!ok) return
  const list = [...chapters.value]
  list.splice(idx, 1)
  chapters.value = list
  if (activeIndex.value >= list.length) {
    activeIndex.value = list.length - 1
  }
  hasUnsavedEdits.value = true
}

async function splitChapterAtCursor(): Promise<void> {
  const ch = currentChapter.value
  const ta = editorTextareaRef.value
  if (!ch || !ta) return

  const pos = ta.selectionStart ?? 0
  const full = ch.mode === 'plain' ? ch.plainText : ch.xhtmlBody
  if (pos <= 0 || pos >= full.length) {
    errorMsg.value = '请先将文本光标放在正文中间需要拆分的位置'
    return
  }
  errorMsg.value = null

  const before = full.slice(0, pos).trimEnd()
  const after = full.slice(pos).trimStart()

  // Infer next chapter title from the first line of `after` when in plain mode
  let nextTitle = `${ch.title} (续)`
  let nextBody = after
  if (ch.mode === 'plain') {
    const firstNl = after.indexOf('\n')
    const firstLine = (firstNl >= 0 ? after.slice(0, firstNl) : after).trim()
    if (firstLine.length > 0 && firstLine.length <= 50 && firstNl >= 0) {
      nextTitle = firstLine
      nextBody = after.slice(firstNl + 1).trimStart()
    }
    ch.plainText = before
    ch.plainDirtySinceSync = true
  } else {
    ch.xhtmlBody = before
    ch.xhtmlDirtySinceSync = true
  }

  const created: LocalChapterState = {
    key: `local_split_${localKeySeq++}`,
    title: nextTitle,
    level: ch.level,
    mode: ch.mode,
    plainText: ch.mode === 'plain' ? nextBody : extractPlainFromXhtml(nextBody),
    xhtmlBody: ch.mode === 'xhtml' ? nextBody : buildXhtmlFromPlain(nextTitle, nextBody),
    hasRichMedia: ch.mode === 'xhtml' && ch.hasRichMedia,
    sourceHref: null,
    plainDirtySinceSync: ch.mode === 'plain',
    xhtmlDirtySinceSync: ch.mode === 'xhtml',
  }

  const list = [...chapters.value]
  list.splice(activeIndex.value + 1, 0, created)
  chapters.value = list
  activeIndex.value += 1
  hasUnsavedEdits.value = true
  await nextTick()
  editorTextareaRef.value?.focus()
}

async function handleResegmentTxt(): Promise<void> {
  if (!props.entry || resegmenting.value) return
  if (hasUnsavedEdits.value) {
    const ok = await dialogStore.confirm({
      title: '重新切分章节',
      message: '按新规则重新读取并切分原 TXT 将覆盖当前未保存的手工修改，确定继续吗？',
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
    chapters.value = mapDtoToLocal(res)
    activeIndex.value = 0
    hasUnsavedEdits.value = true
  } catch (err) {
    errorMsg.value = err instanceof Error ? err.message : String(err)
  } finally {
    resegmenting.value = false
  }
}

function buildSavePayload(): SaveBookChapterInput[] {
  return chapters.value.map((ch, idx) => {
    const cleanTitle = ch.title.trim() || `第 ${idx + 1} 章`
    return {
      title: cleanTitle,
      level: ch.level,
      mode: ch.mode,
      plain_text: ch.plainText,
      xhtml_body:
        ch.mode === 'xhtml'
          ? ch.xhtmlBody
          : buildXhtmlFromPlain(cleanTitle, ch.plainText),
      source_href: ch.sourceHref,
    }
  })
}

async function handleSaveBack(): Promise<void> {
  if (!props.entry || saving.value || chapters.value.length === 0) return
  saving.value = true
  errorMsg.value = null
  try {
    const updated = await toolApi.saveBookContent(
      props.entry.root_id,
      props.entry.rel_path,
      buildSavePayload(),
    )
    hasUnsavedEdits.value = false
    emit('saved', updated)
    emit('close')
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
    )
    hasUnsavedEdits.value = false
    emit('converted', res.detail, res.deleted_source_txt)
    emit('close')
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
      message: '当前章节目录或正文有未保存的修改，确定关闭编辑器吗？',
      danger: true,
    })
    if (!ok) return
  }
  emit('close')
}
</script>

<template>
  <Teleport to="body">
    <div
      v-if="open && entry"
      class="fixed inset-0 z-50 flex items-center justify-center bg-black/60 p-2 backdrop-blur-xs sm:p-5"
      @click.self="requestClose"
    >
      <div
        class="flex h-[92vh] w-full max-w-6xl flex-col overflow-hidden rounded-3xl border border-[var(--ctp-surface0)] bg-[var(--ctp-canvas)] shadow-2xl"
      >
        <!-- Top Header -->
        <div
          class="flex shrink-0 items-center justify-between gap-3 border-b border-[var(--ctp-surface0)]/80 bg-[var(--ctp-shell)] px-5 py-3.5"
        >
          <div class="flex min-w-0 items-center gap-2.5">
            <div
              class="flex h-8 w-8 shrink-0 items-center justify-center rounded-xl bg-[var(--ctp-accent-soft)] text-[var(--ctp-accent)]"
            >
              <LuBookOpen class="h-4 w-4" />
            </div>
            <div class="min-w-0">
              <div class="flex items-center gap-2">
                <h2 class="truncate text-sm font-bold text-[var(--ctp-text)]">
                  {{ docTitle || entry.display_title }}
                </h2>
                <span
                  class="shrink-0 rounded-md bg-[var(--ctp-accent-soft)] px-2 py-0.5 font-mono text-[10px] font-bold uppercase text-[var(--ctp-accent)]"
                >
                  {{ docFormat }}
                </span>
                <span
                  v-if="hasUnsavedEdits"
                  class="shrink-0 rounded-full bg-amber-500/15 px-2 py-0.5 text-[10px] font-semibold text-amber-500"
                >
                  已修改
                </span>
              </div>
              <p class="mt-0.5 truncate text-[11px] text-[var(--ctp-subtext0)]">
                共 {{ chapters.length }} 章 · 约 {{ totalCharCount.toLocaleString() }} 字 ·
                {{ entry.rel_path }}
              </p>
            </div>
          </div>

          <button
            type="button"
            class="flex h-8 w-8 items-center justify-center rounded-xl text-[var(--ctp-subtext0)] transition hover:bg-[var(--ctp-surface0)]/70 hover:text-[var(--ctp-text)]"
            @click="requestClose"
          >
            <LuX class="h-4 w-4" />
          </button>
        </div>

        <!-- Error Banner -->
        <div
          v-if="errorMsg"
          class="flex shrink-0 items-center justify-between gap-2 border-b border-red-500/30 bg-red-500/10 px-5 py-2 text-xs text-red-400"
        >
          <span>{{ errorMsg }}</span>
          <button type="button" class="rounded p-0.5 hover:bg-red-500/15" @click="errorMsg = null">
            <LuX class="h-3.5 w-3.5" />
          </button>
        </div>

        <!-- Loading State -->
        <div
          v-if="loading"
          class="flex flex-1 flex-col items-center justify-center gap-2.5 text-xs text-[var(--ctp-subtext0)]"
        >
          <LuLoader2 class="h-6 w-6 animate-spin text-[var(--ctp-accent)]" />
          <span>正在解析书籍章节目录与正文…</span>
        </div>

        <!-- Dual-Pane Body: Left TOC Organizer + Right Dual-Mode Content Editor -->
        <div v-else class="flex min-h-0 flex-1 flex-col md:flex-row">
          <!-- LEFT: TOC & Chapter Tree Organizer -->
          <aside
            class="flex max-h-60 w-full shrink-0 flex-col border-b border-[var(--ctp-surface0)]/80 bg-[var(--ctp-shell)]/70 md:max-h-none md:w-80 md:border-r md:border-b-0"
          >
            <div class="flex items-center justify-between gap-2 border-b border-[var(--ctp-surface0)]/60 px-3.5 py-2.5">
              <span class="text-xs font-bold text-[var(--ctp-text)]">
                章节目录 ({{ chapters.length }})
              </span>

              <div class="flex items-center gap-1">
                <button
                  v-if="docFormat === 'txt'"
                  type="button"
                  class="inline-flex items-center gap-1 rounded-lg border border-[var(--ctp-surface0)] bg-[var(--ctp-canvas)] px-2 py-1 text-[11px] font-medium text-[var(--ctp-subtext1)] transition hover:border-[var(--ctp-accent)] hover:text-[var(--ctp-text)]"
                  @click="regexPanelOpen = !regexPanelOpen"
                >
                  <LuChevronDown v-if="regexPanelOpen" class="h-3 w-3" />
                  <LuChevronRight v-else class="h-3 w-3" />
                  <span>正则分章</span>
                </button>

                <button
                  type="button"
                  class="inline-flex items-center gap-1 rounded-lg bg-[var(--ctp-accent-soft)] px-2 py-1 text-[11px] font-semibold text-[var(--ctp-accent)] transition hover:opacity-90"
                  title="在当前选中章下方新增一章"
                  @click="insertChapterAfter(activeIndex)"
                >
                  <LuPlus class="h-3 w-3" />
                  <span>新增章</span>
                </button>
              </div>
            </div>

            <!-- Collapsible TXT Regex Segmentation Panel -->
            <div
              v-if="docFormat === 'txt' && regexPanelOpen"
              class="space-y-2 border-b border-[var(--ctp-surface0)]/70 bg-[var(--ctp-mantle)]/65 p-3 text-xs"
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

              <div>
                <label class="mb-1 block text-[10px] font-medium text-[var(--ctp-subtext0)]">
                  章节标题正则（留空使用内置规则）
                </label>
                <input
                  v-model="chapterRegexInput"
                  type="text"
                  placeholder="如 ^\s*第[0-9一二三四五六七八九十百千]+章.*$"
                  class="h-8 w-full rounded-lg border border-[var(--ctp-surface0)] bg-[var(--ctp-canvas)] px-2.5 font-mono text-[11px] text-[var(--ctp-text)] outline-none focus:border-[var(--ctp-accent)]"
                />
              </div>

              <div>
                <label class="mb-1 block text-[10px] font-medium text-[var(--ctp-subtext0)]">
                  分卷标题正则（可选）
                </label>
                <input
                  v-model="volumeRegexInput"
                  type="text"
                  placeholder="如 ^\s*第[0-9一二三四五六七八九十]+卷.*$"
                  class="h-8 w-full rounded-lg border border-[var(--ctp-surface0)] bg-[var(--ctp-canvas)] px-2.5 font-mono text-[11px] text-[var(--ctp-text)] outline-none focus:border-[var(--ctp-accent)]"
                />
              </div>

              <button
                type="button"
                :disabled="resegmenting"
                class="inline-flex w-full items-center justify-center gap-1.5 rounded-lg bg-[var(--ctp-accent)] py-1.5 text-[11px] font-semibold text-[var(--ctp-on-accent)] transition hover:opacity-95 disabled:opacity-50"
                @click="handleResegmentTxt"
              >
                <LuRefreshCw class="h-3 w-3" :class="resegmenting ? 'animate-spin' : ''" />
                <span>按规则重新切分章节</span>
              </button>
            </div>

            <!-- Scrollable TOC Items -->
            <div class="flex-1 space-y-0.5 overflow-y-auto p-2">
              <div
                v-for="(ch, idx) in chapters"
                :key="ch.key"
                class="group rounded-xl px-2.5 py-2 transition"
                :class="[
                  ch.level > 0 ? 'ml-4' : '',
                  activeIndex === idx
                    ? 'bg-[var(--ctp-accent-soft)] text-[var(--ctp-accent)]'
                    : 'text-[var(--ctp-subtext1)] hover:bg-[var(--ctp-surface0)]/55 hover:text-[var(--ctp-text)]',
                ]"
              >
                <div class="flex items-center justify-between gap-1.5">
                  <button
                    type="button"
                    class="flex min-w-0 flex-1 items-center gap-1.5 text-left"
                    @click="activeIndex = idx"
                  >
                    <span
                      class="shrink-0 rounded px-1 py-0.2 font-mono text-[9px] font-bold"
                      :class="
                        ch.level === 0
                          ? 'bg-[var(--ctp-surface0)]/80 text-[var(--ctp-text)]'
                          : 'bg-[var(--ctp-mantle)] text-[var(--ctp-subtext0)]'
                      "
                    >
                      {{ ch.level === 0 ? 'L0' : 'L1' }}
                    </span>
                    <span
                      class="truncate text-xs"
                      :class="ch.level === 0 ? 'font-bold' : 'font-medium'"
                    >
                      {{ ch.title || `第 ${idx + 1} 章` }}
                    </span>
                  </button>

                  <span
                    v-if="ch.hasRichMedia"
                    class="shrink-0 rounded bg-amber-500/15 px-1 py-0.2 text-[9px] font-semibold text-amber-500"
                    title="包含图片或富文本标签"
                  >
                    富文本
                  </span>
                </div>

                <!-- Active Chapter Quick Reorder / Merge / Delete Controls -->
                <div
                  v-if="activeIndex === idx"
                  class="mt-1.5 flex flex-wrap items-center justify-between gap-1 border-t border-[var(--ctp-accent)]/20 pt-1.5 text-[10px]"
                >
                  <div class="flex items-center gap-1">
                    <button
                      type="button"
                      :disabled="idx === 0"
                      class="rounded bg-[var(--ctp-canvas)]/80 p-1 text-[var(--ctp-text)] hover:text-[var(--ctp-accent)] disabled:opacity-35"
                      title="上移章节"
                      @click="moveChapter(idx, -1)"
                    >
                      <LuArrowUp class="h-3 w-3" />
                    </button>
                    <button
                      type="button"
                      :disabled="idx === chapters.length - 1"
                      class="rounded bg-[var(--ctp-canvas)]/80 p-1 text-[var(--ctp-text)] hover:text-[var(--ctp-accent)] disabled:opacity-35"
                      title="下移章节"
                      @click="moveChapter(idx, 1)"
                    >
                      <LuArrowDown class="h-3 w-3" />
                    </button>
                    <button
                      type="button"
                      class="rounded bg-[var(--ctp-canvas)]/80 px-1.5 py-0.5 font-medium text-[var(--ctp-text)] hover:text-[var(--ctp-accent)]"
                      title="切换顶层卷/章 (L0) 与卷下子章 (L1)"
                      @click="toggleChapterLevel(idx)"
                    >
                      {{ ch.level === 0 ? '缩进为子章' : '升为顶层' }}
                    </button>
                  </div>

                  <div class="flex items-center gap-1">
                    <button
                      v-if="idx > 0"
                      type="button"
                      class="rounded bg-[var(--ctp-canvas)]/80 px-1.5 py-0.5 font-medium text-[var(--ctp-subtext1)] hover:text-[var(--ctp-accent)]"
                      title="将本章合并追加到上一章末尾"
                      @click="mergeIntoPrevious(idx)"
                    >
                      合并上章
                    </button>
                    <button
                      type="button"
                      class="rounded bg-[var(--ctp-canvas)]/80 p-1 text-[var(--ctp-subtext1)] hover:text-red-500"
                      title="删除此章节"
                      @click="deleteChapter(idx)"
                    >
                      <LuTrash2 class="h-3 w-3" />
                    </button>
                  </div>
                </div>
              </div>
            </div>
          </aside>

          <!-- RIGHT: Chapter Title + Dual-Mode Body Editor -->
          <div v-if="currentChapter" class="flex min-w-0 flex-1 flex-col bg-[var(--ctp-canvas)]">
            <!-- Chapter Editor Toolbar -->
            <div
              class="flex flex-wrap items-center justify-between gap-2.5 border-b border-[var(--ctp-surface0)]/75 bg-[var(--ctp-mantle)]/35 px-4 py-2.5"
            >
              <div class="flex min-w-0 flex-1 items-center gap-2">
                <span class="shrink-0 text-[11px] font-semibold text-[var(--ctp-subtext0)]">
                  章节标题
                </span>
                <input
                  :value="currentChapter.title"
                  type="text"
                  placeholder="输入章节或分卷标题…"
                  class="h-8 min-w-[12rem] flex-1 rounded-xl border border-[var(--ctp-surface0)] bg-[var(--ctp-canvas)] px-3 text-xs font-semibold text-[var(--ctp-text)] outline-none focus:border-[var(--ctp-accent)]"
                  @input="onTitleInput(($event.target as HTMLInputElement).value)"
                />
              </div>

              <div class="flex flex-wrap items-center gap-2">
                <button
                  type="button"
                  class="inline-flex h-8 items-center gap-1 rounded-xl border border-[var(--ctp-surface0)] bg-[var(--ctp-canvas)] px-2.5 text-[11px] font-medium text-[var(--ctp-subtext1)] transition hover:border-[var(--ctp-accent)] hover:text-[var(--ctp-text)]"
                  title="在下方编辑器当前光标位置将本章拆分为上下两章"
                  @click="splitChapterAtCursor"
                >
                  <LuFileText class="h-3.5 w-3.5 text-[var(--ctp-accent)]" />
                  <span>从光标处拆章</span>
                </button>

                <!-- Dual-Mode Switcher: 纯文本段落 / XHTML 源码 -->
                <div
                  class="inline-flex items-center gap-0.5 rounded-xl border border-[var(--ctp-surface0)] bg-[var(--ctp-shell)] p-0.5"
                >
                  <button
                    type="button"
                    class="rounded-lg px-2.5 py-1 text-[11px] font-semibold transition"
                    :class="
                      currentChapter.mode === 'plain'
                        ? 'bg-[var(--ctp-accent)] text-[var(--ctp-on-accent)]'
                        : 'text-[var(--ctp-subtext1)] hover:text-[var(--ctp-text)]'
                    "
                    @click="switchChapterMode('plain')"
                  >
                    纯文本段落
                  </button>
                  <button
                    type="button"
                    class="rounded-lg px-2.5 py-1 text-[11px] font-semibold transition"
                    :class="
                      currentChapter.mode === 'xhtml'
                        ? 'bg-[var(--ctp-accent)] text-[var(--ctp-on-accent)]'
                        : 'text-[var(--ctp-subtext1)] hover:text-[var(--ctp-text)]'
                    "
                    @click="switchChapterMode('xhtml')"
                  >
                    XHTML 源码
                  </button>
                </div>
              </div>
            </div>

            <!-- Rich Media Guard Notice when switched to Plain Mode on an illustrated EPUB chapter -->
            <div
              v-if="currentChapter.hasRichMedia && currentChapter.mode === 'plain'"
              class="flex items-center gap-2 border-b border-amber-500/30 bg-amber-500/10 px-4 py-1.5 text-[11px] text-amber-500"
            >
              <LuAlertTriangle class="h-3.5 w-3.5 shrink-0" />
              <span>
                本章原包含图片或富文本标签（<code>&lt;img&gt;</code> / <code>&lt;svg&gt;</code>），以「纯文本段落」模式保存将重写为纯段落标签；如需保留插图请切换至「XHTML 源码」模式。
              </span>
            </div>

            <!-- Main Textarea -->
            <div class="relative min-h-0 flex-1">
              <textarea
                ref="editorTextareaRef"
                :value="
                  currentChapter.mode === 'plain'
                    ? currentChapter.plainText
                    : currentChapter.xhtmlBody
                "
                :placeholder="
                  currentChapter.mode === 'plain'
                    ? '在此编辑章节正文（每行对应一个自然段）…'
                    : '在此编辑 <body> 内的 XHTML 源码（如 <h2>标题</h2><p>正文</p>）…'
                "
                class="h-full w-full resize-none bg-transparent p-4 text-[var(--ctp-text)] outline-none sm:p-5"
                :class="
                  currentChapter.mode === 'xhtml'
                    ? 'font-mono text-xs leading-relaxed'
                    : 'text-sm leading-7'
                "
                @input="onBodyInput(($event.target as HTMLTextAreaElement).value)"
              />

              <div
                class="pointer-events-none absolute right-4 bottom-3 rounded-lg bg-[var(--ctp-shell)]/85 px-2.5 py-1 font-mono text-[10px] text-[var(--ctp-subtext0)] backdrop-blur-xs"
              >
                本章 {{ currentCharCount.toLocaleString() }}
                {{ currentChapter.mode === 'plain' ? '字' : '字符' }}
              </div>
            </div>
          </div>
        </div>

        <!-- Bottom Action Footer -->
        <div
          class="flex shrink-0 flex-wrap items-center justify-between gap-3 border-t border-[var(--ctp-surface0)]/80 bg-[var(--ctp-shell)] px-5 py-3.5"
        >
          <div>
            <AppCheckbox
              v-if="docFormat === 'txt'"
              v-model="deleteSourceTxt"
              label="转换成功后删除原 TXT 文件"
            />
            <span v-else class="text-[11px] text-[var(--ctp-subtext0)]">
              保存后将原子更新 EPUB 包内章节文件、OPF 阅读顺序及 EPUB3 / NCX 双目录树
            </span>
          </div>

          <div class="flex flex-wrap items-center gap-2">
            <button
              type="button"
              class="h-9 rounded-xl border border-[var(--ctp-surface0)] bg-[var(--ctp-canvas)] px-4 text-xs font-semibold text-[var(--ctp-subtext1)] transition hover:text-[var(--ctp-text)]"
              @click="requestClose"
            >
              取消
            </button>

            <template v-if="docFormat === 'txt'">
              <button
                type="button"
                :disabled="saving || converting || chapters.length === 0"
                class="inline-flex h-9 items-center gap-1.5 rounded-xl border border-[var(--ctp-surface0)] bg-[var(--ctp-canvas)] px-4 text-xs font-semibold text-[var(--ctp-text)] transition hover:border-[var(--ctp-accent)] disabled:opacity-50"
                @click="handleSaveBack"
              >
                <LuSave class="h-3.5 w-3.5 text-[var(--ctp-accent)]" />
                <span>{{ saving ? '保存中…' : '保存回 TXT' }}</span>
              </button>

              <button
                type="button"
                :disabled="converting || saving || chapters.length === 0"
                class="inline-flex h-9 items-center gap-1.5 rounded-xl bg-[var(--ctp-accent)] px-5 text-xs font-semibold text-[var(--ctp-on-accent)] shadow-sm transition hover:opacity-95 disabled:opacity-50"
                @click="handleConvertToEpub"
              >
                <LuBookOpen class="h-3.5 w-3.5" />
                <span>{{ converting ? '正在打包 EPUB…' : '转为 EPUB' }}</span>
              </button>
            </template>

            <template v-else>
              <button
                type="button"
                :disabled="saving || chapters.length === 0"
                class="inline-flex h-9 items-center gap-1.5 rounded-xl bg-[var(--ctp-accent)] px-5 text-xs font-semibold text-[var(--ctp-on-accent)] shadow-sm transition hover:opacity-95 disabled:opacity-50"
                @click="handleSaveBack"
              >
                <LuSave class="h-3.5 w-3.5" />
                <span>{{ saving ? '正在写回 EPUB…' : '保存目录与正文到 EPUB' }}</span>
              </button>
            </template>
          </div>
        </div>
      </div>
    </div>
  </Teleport>
</template>

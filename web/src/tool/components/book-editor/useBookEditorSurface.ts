import {
  nextTick,
  triggerRef,
  watch,
  type ComputedRef,
  type Ref,
  type ShallowRef,
} from 'vue'
import type { ChapterRecord, SurfaceMode } from './useBookEditorChapters'
import {
  formatSelectionBlock,
  insertHorizontalRuleAtSelection,
  removeInlineFormatAtSelection,
  toggleInlineTagAtSelection,
} from './wysiwygDom'
import {
  countNonWhitespace,
  extractPlainFromXhtml,
  hydrateXhtmlForBrowser,
  serializeContainerToValidXhtml,
} from './xhtmlUtils'

const MAX_UNDO_HISTORY = 50

export interface UseBookEditorSurfaceOptions {
  chapters: ShallowRef<ChapterRecord[]>
  activeIndex: Ref<number>
  currentRecord: ComputedRef<ChapterRecord | null>
  hasUnsavedEdits: Ref<boolean>
  surfaceMode: Ref<SurfaceMode>
  showLivePreview: Ref<boolean>
  indentParagraphs: Ref<boolean>
  activeTitle: Ref<string>
  activeCharCount: Ref<number>
  sourceBuffer: Ref<string>
  previewSrcdoc: Ref<string>
  wysiwygRef: Ref<HTMLDivElement | null>
  sourceTextareaRef: Ref<HTMLTextAreaElement | null>
  epubAssetContext: () => { rootId: string; relPath: string } | null
  buildLivePreviewSrcdoc: (xhtmlBody: string, sourceHref: string | null) => string
  themeDeps: () => readonly unknown[]
}

export function useBookEditorSurface(opts: UseBookEditorSurfaceOptions) {
  let isComposing = false
  let idleSyncTimer: ReturnType<typeof setTimeout> | null = null
  let undoStack: string[] = []
  let redoStack: string[] = []
  let lastRecordedHtml = ''

  function clearIdleSyncTimer(): void {
    if (idleSyncTimer) {
      clearTimeout(idleSyncTimer)
      idleSyncTimer = null
    }
  }

  function flushActiveSurfaceToRecord(): void {
    clearIdleSyncTimer()
    const rec = opts.chapters.value[opts.activeIndex.value]
    if (!rec) return

    rec.title = opts.activeTitle.value.trim() || rec.title
    if (opts.surfaceMode.value === 'wysiwyg') {
      const el = opts.wysiwygRef.value
      if (el) {
        const serialized = serializeContainerToValidXhtml(el)
        rec.xhtmlBody = serialized || `  <p><br/></p>`
        const plain = extractPlainFromXhtml(rec.xhtmlBody, rec.title)
        rec.plainCache = plain
        rec.charCount = countNonWhitespace(plain)
        rec.hasRichMedia = /<(?:img|svg|image|table|video|audio|figure)\b/i.test(rec.xhtmlBody)
        opts.activeCharCount.value = rec.charCount
      }
    } else {
      rec.xhtmlBody = opts.sourceBuffer.value
      const plain = extractPlainFromXhtml(rec.xhtmlBody, rec.title)
      rec.plainCache = plain
      rec.charCount = countNonWhitespace(plain)
      rec.hasRichMedia = /<(?:img|svg|image|table|video|audio|figure)\b/i.test(rec.xhtmlBody)
      opts.activeCharCount.value = rec.charCount
    }
    triggerRef(opts.chapters)
  }

  async function mountActiveChapterIntoSurface(): Promise<void> {
    const rec = opts.chapters.value[opts.activeIndex.value]
    if (!rec) return

    opts.activeTitle.value = rec.title
    opts.activeCharCount.value = rec.charCount
    opts.sourceBuffer.value = rec.xhtmlBody

    if (opts.showLivePreview.value) {
      opts.previewSrcdoc.value = opts.buildLivePreviewSrcdoc(rec.xhtmlBody, rec.sourceHref)
    }

    await nextTick()
    if (opts.surfaceMode.value === 'wysiwyg' && opts.wysiwygRef.value) {
      opts.wysiwygRef.value.innerHTML = hydrateXhtmlForBrowser(
        rec.xhtmlBody,
        rec.sourceHref,
        opts.epubAssetContext(),
      )
    }
  }

  function scheduleIdleMetricsUpdate(): void {
    clearIdleSyncTimer()
    idleSyncTimer = setTimeout(() => {
      idleSyncTimer = null
      const rec = opts.chapters.value[opts.activeIndex.value]
      if (!rec) return
      if (opts.surfaceMode.value === 'wysiwyg' && opts.wysiwygRef.value) {
        const text = opts.wysiwygRef.value.innerText ?? ''
        opts.activeCharCount.value = countNonWhitespace(text)
        if (opts.showLivePreview.value) {
          const xhtml = serializeContainerToValidXhtml(opts.wysiwygRef.value)
          opts.previewSrcdoc.value = opts.buildLivePreviewSrcdoc(xhtml, rec.sourceHref)
        }
      } else if (opts.surfaceMode.value === 'xhtml') {
        const plain = extractPlainFromXhtml(opts.sourceBuffer.value, opts.activeTitle.value)
        opts.activeCharCount.value = countNonWhitespace(plain)
        if (opts.showLivePreview.value) {
          opts.previewSrcdoc.value = opts.buildLivePreviewSrcdoc(
            opts.sourceBuffer.value,
            rec.sourceHref,
          )
        }
      }
    }, 160)
  }

  function resetWysiwygHistory(): void {
    undoStack = []
    redoStack = []
    lastRecordedHtml = opts.wysiwygRef.value?.innerHTML ?? ''
  }

  function pushUndo(beforeHtml: string): void {
    undoStack.push(beforeHtml)
    if (undoStack.length > MAX_UNDO_HISTORY) undoStack.shift()
    redoStack = []
    lastRecordedHtml = opts.wysiwygRef.value?.innerHTML ?? ''
  }

  function recordWysiwygUndoSnapshot(): void {
    if (!opts.wysiwygRef.value) return
    const current = opts.wysiwygRef.value.innerHTML
    if (lastRecordedHtml && current !== lastRecordedHtml) {
      undoStack.push(lastRecordedHtml)
      if (undoStack.length > MAX_UNDO_HISTORY) undoStack.shift()
      redoStack = []
    }
    lastRecordedHtml = current
  }

  function onWysiwygFocus(): void {
    lastRecordedHtml = opts.wysiwygRef.value?.innerHTML ?? ''
  }

  function onWysiwygCompositionStart(): void {
    isComposing = true
  }

  function onWysiwygInput(): void {
    opts.hasUnsavedEdits.value = true
    if (isComposing) return
    recordWysiwygUndoSnapshot()
    scheduleIdleMetricsUpdate()
  }

  function onWysiwygCompositionEnd(): void {
    isComposing = false
    opts.hasUnsavedEdits.value = true
    recordWysiwygUndoSnapshot()
    scheduleIdleMetricsUpdate()
  }

  function onSourceTextareaInput(): void {
    opts.hasUnsavedEdits.value = true
    scheduleIdleMetricsUpdate()
  }

  function onSourceKeydown(ev: KeyboardEvent): void {
    if (ev.key === 'Tab') {
      ev.preventDefault()
      const ta = opts.sourceTextareaRef.value
      if (!ta) return
      const start = ta.selectionStart
      const end = ta.selectionEnd
      opts.sourceBuffer.value = `${opts.sourceBuffer.value.slice(0, start)}  ${opts.sourceBuffer.value.slice(end)}`
      opts.hasUnsavedEdits.value = true
      void nextTick(() => {
        ta.selectionStart = start + 2
        ta.selectionEnd = start + 2
      })
    }
  }

  function onActiveTitleInput(val: string): void {
    opts.activeTitle.value = val
    const rec = opts.chapters.value[opts.activeIndex.value]
    if (rec) {
      rec.title = val
      triggerRef(opts.chapters)
    }
    opts.hasUnsavedEdits.value = true
  }

  async function selectChapter(idx: number): Promise<void> {
    if (idx === opts.activeIndex.value || idx < 0 || idx >= opts.chapters.value.length) return
    flushActiveSurfaceToRecord()
    opts.activeIndex.value = idx
    await mountActiveChapterIntoSurface()
    resetWysiwygHistory()
  }

  async function switchSurfaceMode(next: SurfaceMode): Promise<void> {
    if (opts.surfaceMode.value === next) return
    flushActiveSurfaceToRecord()
    opts.surfaceMode.value = next
    await mountActiveChapterIntoSurface()
    resetWysiwygHistory()
  }

  function toggleLivePreview(): void {
    flushActiveSurfaceToRecord()
    opts.showLivePreview.value = !opts.showLivePreview.value
    if (opts.showLivePreview.value && opts.currentRecord.value) {
      opts.previewSrcdoc.value = opts.buildLivePreviewSrcdoc(
        opts.currentRecord.value.xhtmlBody,
        opts.currentRecord.value.sourceHref,
      )
    }
  }

  watch(opts.themeDeps, async () => {
    await nextTick()
    if (opts.showLivePreview.value && opts.currentRecord.value) {
      flushActiveSurfaceToRecord()
      opts.previewSrcdoc.value = opts.buildLivePreviewSrcdoc(
        opts.currentRecord.value.xhtmlBody,
        opts.currentRecord.value.sourceHref,
      )
    }
  })

  function execWysiwygCmd(command: string): void {
    if (opts.surfaceMode.value !== 'wysiwyg' || !opts.wysiwygRef.value) return
    const container = opts.wysiwygRef.value
    container.focus()

    if (command === 'undo') {
      const prev = undoStack.pop()
      if (prev === undefined) return
      redoStack.push(container.innerHTML)
      container.innerHTML = prev
      lastRecordedHtml = prev
      opts.hasUnsavedEdits.value = true
      scheduleIdleMetricsUpdate()
      return
    }
    if (command === 'redo') {
      const next = redoStack.pop()
      if (next === undefined) return
      undoStack.push(container.innerHTML)
      container.innerHTML = next
      lastRecordedHtml = next
      opts.hasUnsavedEdits.value = true
      scheduleIdleMetricsUpdate()
      return
    }

    const before = container.innerHTML
    let changed = false
    if (command === 'bold') changed = toggleInlineTagAtSelection(container, 'strong')
    else if (command === 'italic') changed = toggleInlineTagAtSelection(container, 'em')
    else if (command === 'underline') changed = toggleInlineTagAtSelection(container, 'u')
    else if (command === 'removeFormat') changed = removeInlineFormatAtSelection(container)

    if (changed) {
      pushUndo(before)
      opts.hasUnsavedEdits.value = true
      scheduleIdleMetricsUpdate()
    }
  }

  function formatBlockTag(tag: 'P' | 'H1' | 'H2' | 'H3' | 'BLOCKQUOTE'): void {
    if (opts.surfaceMode.value === 'wysiwyg' && opts.wysiwygRef.value) {
      const container = opts.wysiwygRef.value
      container.focus()
      const before = container.innerHTML
      const lower = tag.toLowerCase() as 'p' | 'h1' | 'h2' | 'h3' | 'blockquote'
      if (formatSelectionBlock(container, lower)) {
        pushUndo(before)
        opts.hasUnsavedEdits.value = true
        scheduleIdleMetricsUpdate()
      }
    } else if (opts.sourceTextareaRef.value) {
      const ta = opts.sourceTextareaRef.value
      const start = ta.selectionStart
      const end = ta.selectionEnd
      const sel = opts.sourceBuffer.value.slice(start, end) || opts.activeTitle.value || '...'
      const lower = tag.toLowerCase()
      const wrapped = `<${lower}>${sel}</${lower}>`
      opts.sourceBuffer.value = `${opts.sourceBuffer.value.slice(0, start)}${wrapped}${opts.sourceBuffer.value.slice(end)}`
      opts.hasUnsavedEdits.value = true
      scheduleIdleMetricsUpdate()
    }
  }

  function wrapInlineCode(): void {
    if (opts.surfaceMode.value === 'wysiwyg' && opts.wysiwygRef.value) {
      const container = opts.wysiwygRef.value
      container.focus()
      const before = container.innerHTML
      if (toggleInlineTagAtSelection(container, 'code')) {
        pushUndo(before)
        opts.hasUnsavedEdits.value = true
        scheduleIdleMetricsUpdate()
      }
    } else if (opts.sourceTextareaRef.value) {
      const ta = opts.sourceTextareaRef.value
      const s = ta.selectionStart
      const e = ta.selectionEnd
      const text = opts.sourceBuffer.value.slice(s, e)
      opts.sourceBuffer.value = `${opts.sourceBuffer.value.slice(0, s)}<code>${text}</code>${opts.sourceBuffer.value.slice(e)}`
      opts.hasUnsavedEdits.value = true
    }
  }

  function insertHorizontalRule(): void {
    if (opts.surfaceMode.value === 'wysiwyg' && opts.wysiwygRef.value) {
      const container = opts.wysiwygRef.value
      container.focus()
      const before = container.innerHTML
      insertHorizontalRuleAtSelection(container)
      pushUndo(before)
      opts.hasUnsavedEdits.value = true
      scheduleIdleMetricsUpdate()
    } else if (opts.sourceTextareaRef.value) {
      const ta = opts.sourceTextareaRef.value
      const pos = ta.selectionStart
      opts.sourceBuffer.value = `${opts.sourceBuffer.value.slice(0, pos)}\n  <hr/>\n${opts.sourceBuffer.value.slice(pos)}`
      opts.hasUnsavedEdits.value = true
      scheduleIdleMetricsUpdate()
    }
  }

  return {
    clearIdleSyncTimer,
    flushActiveSurfaceToRecord,
    mountActiveChapterIntoSurface,
    onWysiwygFocus,
    onWysiwygCompositionStart,
    onWysiwygInput,
    onWysiwygCompositionEnd,
    onSourceTextareaInput,
    onSourceKeydown,
    onActiveTitleInput,
    selectChapter,
    switchSurfaceMode,
    toggleLivePreview,
    execWysiwygCmd,
    formatBlockTag,
    wrapInlineCode,
    insertHorizontalRule,
  }
}

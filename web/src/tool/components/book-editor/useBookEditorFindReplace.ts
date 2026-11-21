import { computed, ref, triggerRef, type ComputedRef, type Ref, type ShallowRef } from 'vue'
import type { ChapterRecord, SurfaceMode } from './useBookEditorChapters'
import {
  countNonWhitespace,
  extractPlainFromXhtml,
  replaceInTextNodes,
  serializeContainerToValidXhtml,
} from './xhtmlUtils'

export interface UseBookEditorFindReplaceOptions {
  chapters: ShallowRef<ChapterRecord[]>
  activeIndex: Ref<number>
  currentRecord: ComputedRef<ChapterRecord | null>
  surfaceMode: Ref<SurfaceMode>
  sourceBuffer: Ref<string>
  hasUnsavedEdits: Ref<boolean>
  flushActiveSurfaceToRecord: () => void
  mountActiveChapterIntoSurface: () => Promise<void>
  showToast: (msg: string) => void
}

function countTextNodeMatches(xhtmlBody: string, re: RegExp): number {
  const doc = new DOMParser().parseFromString(`<body>${xhtmlBody}</body>`, 'text/html')
  if (!doc.body) return 0
  let count = 0
  const walker = document.createTreeWalker(doc.body, NodeFilter.SHOW_TEXT)
  let cur = walker.nextNode()
  while (cur) {
    const val = cur.nodeValue ?? ''
    if (val) {
      re.lastIndex = 0
      count += Array.from(val.matchAll(re)).length
    }
    cur = walker.nextNode()
  }
  return count
}

export function useBookEditorFindReplace(opts: UseBookEditorFindReplaceOptions) {
  const findBarOpen = ref(false)
  const findQuery = ref('')
  const replaceQuery = ref('')
  const findScope = ref<'chapter' | 'book'>('chapter')
  const findUseRegex = ref(false)
  const findCaseSensitive = ref(false)

  function buildSearchRegexResult(): { regex: RegExp | null; error: string | null } {
    const q = findQuery.value
    if (!q) return { regex: null, error: null }
    try {
      const pattern = findUseRegex.value ? q : q.replace(/[.*+?^${}()|[\]\\]/g, '\\$&')
      const flags = findCaseSensitive.value ? 'g' : 'gi'
      return { regex: new RegExp(pattern, flags), error: null }
    } catch (err) {
      return { regex: null, error: err instanceof Error ? err.message : '正则表达式语法错误' }
    }
  }

  const searchMatchSummary = computed<string>(() => {
    const { regex: re, error } = buildSearchRegexResult()
    if (error) return `正则错误：${error}`
    if (!re) return ''
    if (findScope.value === 'chapter') {
      const rec = opts.currentRecord.value
      if (!rec) return '0 处'
      const count = countTextNodeMatches(
        opts.surfaceMode.value === 'xhtml' ? opts.sourceBuffer.value : rec.xhtmlBody,
        re,
      )
      return `本章 ${count} 处`
    }
    let total = 0
    let chapHits = 0
    for (const ch of opts.chapters.value) {
      const m = countTextNodeMatches(ch.xhtmlBody, re)
      if (m > 0) {
        total += m
        chapHits += 1
      }
    }
    return `全书 ${chapHits} 章共 ${total} 处`
  })

  async function handleReplaceSingle(): Promise<void> {
    const { regex: re, error } = buildSearchRegexResult()
    if (error) {
      opts.showToast(`正则语法错误：${error}`)
      return
    }
    if (!re || !opts.currentRecord.value) return
    opts.flushActiveSurfaceToRecord()

    const doc = new DOMParser().parseFromString(
      `<body>${opts.currentRecord.value.xhtmlBody}</body>`,
      'text/html',
    )
    const count = replaceInTextNodes(doc.body, re, replaceQuery.value, true, !findUseRegex.value)
    if (count > 0) {
      opts.currentRecord.value.xhtmlBody = serializeContainerToValidXhtml(doc.body) || '  <p><br/></p>'
      opts.currentRecord.value.plainCache = extractPlainFromXhtml(
        opts.currentRecord.value.xhtmlBody,
        opts.currentRecord.value.title,
      )
      opts.currentRecord.value.charCount = countNonWhitespace(opts.currentRecord.value.plainCache)
      opts.hasUnsavedEdits.value = true
      triggerRef(opts.chapters)
      await opts.mountActiveChapterIntoSurface()
      opts.showToast('已替换 1 处')
    }
  }

  async function handleReplaceAll(): Promise<void> {
    const { regex: re, error } = buildSearchRegexResult()
    if (error) {
      opts.showToast(`正则语法错误：${error}`)
      return
    }
    if (!re) return
    opts.flushActiveSurfaceToRecord()

    const indices =
      findScope.value === 'chapter'
        ? [opts.activeIndex.value]
        : opts.chapters.value.map((_, idx) => idx)

    let totalReplaced = 0
    for (const idx of indices) {
      const ch = opts.chapters.value[idx]
      if (!ch) continue
      const doc = new DOMParser().parseFromString(`<body>${ch.xhtmlBody}</body>`, 'text/html')
      const count = replaceInTextNodes(
        doc.body,
        re,
        replaceQuery.value,
        false,
        !findUseRegex.value,
      )
      if (count > 0) {
        ch.xhtmlBody = serializeContainerToValidXhtml(doc.body) || '  <p><br/></p>'
        ch.plainCache = extractPlainFromXhtml(ch.xhtmlBody, ch.title)
        ch.charCount = countNonWhitespace(ch.plainCache)
        totalReplaced += count
      }
    }

    if (totalReplaced > 0) {
      opts.hasUnsavedEdits.value = true
      triggerRef(opts.chapters)
      await opts.mountActiveChapterIntoSurface()
    }
    opts.showToast(
      totalReplaced > 0 ? `共替换 ${totalReplaced} 处匹配内容` : '未找到可替换的匹配项',
    )
  }

  return {
    findBarOpen,
    findQuery,
    replaceQuery,
    findScope,
    findUseRegex,
    findCaseSensitive,
    searchMatchSummary,
    handleReplaceSingle,
    handleReplaceAll,
  }
}

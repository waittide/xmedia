import { triggerRef, type ComputedRef, type Ref, type ShallowRef } from 'vue'
import type { EditableBookChapterDto } from '../../types'
import {
  countNonWhitespace,
  escapeXml,
  extractPlainFromXhtml,
  plainToCleanXhtml,
  rewriteChapterRelativeAssetPaths,
  serializeContainerToValidXhtml,
} from './xhtmlUtils'

export interface ChapterRecord {
  key: string
  title: string
  level: number
  xhtmlBody: string
  plainCache: string
  charCount: number
  hasRichMedia: boolean
  sourceHref: string | null
}

export type SurfaceMode = 'wysiwyg' | 'xhtml'

const VOLUME_TITLE_RE =
  /^\s*(?:第\s*[零一二三四五六七八九十百千万两〇廿卅卌壹贰叁肆伍陆柒捌玖拾佰仟0-9０-９]+\s*[卷部集册篇](?:[\s：:、—\-].*)?|卷\s*[零一二三四五六七八九十百千万两〇廿卅卌壹贰叁肆伍陆柒捌玖拾佰仟0-9０-９]+.*|(?:volume|vol\.?|book|part)\s+[0-9ivxlcdm]+.*)$/i

export function isVolumeRecord(list: ChapterRecord[], idx: number): boolean {
  const rec = list[idx]
  if (!rec || rec.level !== 0 || rec.title === '书籍简介') return false
  const next = list[idx + 1]
  return Boolean(next && next.level > 0) || VOLUME_TITLE_RE.test(rec.title.trim())
}

export interface UseBookEditorChaptersOptions {
  chapters: ShallowRef<ChapterRecord[]>
  activeIndex: Ref<number>
  activeTitle: Ref<string>
  hasUnsavedEdits: Ref<boolean>
  surfaceMode: Ref<SurfaceMode>
  sourceBuffer: Ref<string>
  wysiwygRef: Ref<HTMLDivElement | null>
  sourceTextareaRef: Ref<HTMLTextAreaElement | null>
  currentRecord: ComputedRef<ChapterRecord | null>
  flushActiveSurfaceToRecord: () => void
  mountActiveChapterIntoSurface: () => Promise<void>
  clearIdleSyncTimer: () => void
  confirmDelete: (title: string) => Promise<boolean>
  showToast: (msg: string) => void
}

export function useBookEditorChapters(opts: UseBookEditorChaptersOptions) {
  let keySeq = 1

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

  /**
   * One-click Clean Typography:
   * Removes empty paragraphs, trims leading full-width spaces `　　` / junk spaces, and normalizes paragraphs.
   */
  async function cleanChapterTypography(): Promise<void> {
    const rec = opts.currentRecord.value
    if (!rec) return
    opts.flushActiveSurfaceToRecord()

    const doc = new DOMParser().parseFromString(`<body>${rec.xhtmlBody}</body>`, 'text/html')
    const body = doc.body
    if (!body) return

    body.querySelectorAll('p, div, li').forEach((el) => {
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
    opts.hasUnsavedEdits.value = true
    triggerRef(opts.chapters)
    await opts.mountActiveChapterIntoSurface()
    opts.showToast('已完成本章排版净化（清理空段与段首冗余空格）')
  }

  async function splitChapterAtCursor(): Promise<void> {
    const rec = opts.currentRecord.value
    if (!rec) return
    opts.clearIdleSyncTimer()
    opts.flushActiveSurfaceToRecord()

    if (opts.surfaceMode.value === 'wysiwyg' && opts.wysiwygRef.value) {
      const root = opts.wysiwygRef.value
      const sel = window.getSelection()
      if (!sel || sel.rangeCount === 0 || !root.contains(sel.anchorNode)) {
        opts.showToast('请先在正文中点击需要拆分章节的光标位置')
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
        await opts.mountActiveChapterIntoSurface()
        opts.showToast('光标后没有可拆分的内容')
        return
      }

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
        sourceHref: rec.sourceHref,
      }

      const nextList = [...opts.chapters.value]
      nextList.splice(opts.activeIndex.value + 1, 0, newRec)
      opts.chapters.value = nextList
      opts.activeIndex.value += 1
      opts.hasUnsavedEdits.value = true
      await opts.mountActiveChapterIntoSurface()
      opts.showToast(`已拆分为新章节「${nextTitle}」`)
      return
    }

    if (opts.surfaceMode.value === 'xhtml' && opts.sourceTextareaRef.value) {
      const ta = opts.sourceTextareaRef.value
      const pos = ta.selectionStart ?? 0
      if (pos <= 0 || pos >= opts.sourceBuffer.value.length) {
        opts.showToast('请先将光标放在源码中间需要拆分的位置')
        return
      }
      const rawBefore = opts.sourceBuffer.value.slice(0, pos).trimEnd()
      const rawAfter = opts.sourceBuffer.value.slice(pos).trimStart()
      const beforeDoc = new DOMParser().parseFromString(`<body>${rawBefore}</body>`, 'text/html')
      const afterDoc = new DOMParser().parseFromString(`<body>${rawAfter}</body>`, 'text/html')
      const before = serializeContainerToValidXhtml(beforeDoc.body) || '  <p><br/></p>'
      const after = serializeContainerToValidXhtml(afterDoc.body)
      if (!after.trim()) {
        opts.showToast('光标后没有可拆分的内容')
        return
      }
      rec.xhtmlBody = before
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
        sourceHref: rec.sourceHref,
      }

      const nextList = [...opts.chapters.value]
      nextList.splice(opts.activeIndex.value + 1, 0, newRec)
      opts.chapters.value = nextList
      opts.activeIndex.value += 1
      opts.hasUnsavedEdits.value = true
      await opts.mountActiveChapterIntoSurface()
      opts.showToast(`已拆分为新章节「${nextTitle}」`)
    }
  }

  function handleRenameChapter(idx: number, newTitle: string): void {
    const rec = opts.chapters.value[idx]
    const clean = newTitle.trim()
    if (rec && clean && clean !== rec.title) {
      rec.title = clean
      if (idx === opts.activeIndex.value) {
        opts.activeTitle.value = clean
      }
      opts.hasUnsavedEdits.value = true
      triggerRef(opts.chapters)
    }
  }

  function toggleLevel(idx: number): void {
    const rec = opts.chapters.value[idx]
    if (!rec) return
    rec.level = rec.level === 0 ? 1 : 0
    opts.hasUnsavedEdits.value = true
    triggerRef(opts.chapters)
  }

  function autoDetectTocHierarchy(): void {
    opts.flushActiveSurfaceToRecord()
    let seenVol = false
    let volCount = 0
    for (const ch of opts.chapters.value) {
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
    opts.hasUnsavedEdits.value = true
    triggerRef(opts.chapters)
    opts.showToast(
      volCount > 0
        ? `已自动识别 ${volCount} 个分卷并缩进下属章节`
        : '未检测到显式「第X卷/部」标题，已保持一级平铺',
    )
  }

  function flattenAllTocLevels(): void {
    opts.flushActiveSurfaceToRecord()
    for (const ch of opts.chapters.value) {
      ch.level = 0
    }
    opts.hasUnsavedEdits.value = true
    triggerRef(opts.chapters)
    opts.showToast('已将全部章节平铺为一级目录')
  }

  function syncTocTitlesFromBodyHeadings(): void {
    opts.flushActiveSurfaceToRecord()
    let updated = 0
    for (let i = 0; i < opts.chapters.value.length; i += 1) {
      const ch = opts.chapters.value[i]
      if (!ch) continue
      const doc = new DOMParser().parseFromString(`<body>${ch.xhtmlBody}</body>`, 'text/html')
      const h = doc.body.querySelector('h1, h2, h3')
      const text = (h?.textContent ?? '').replace(/\s+/g, ' ').trim()
      if (text && text.length <= 80 && text !== ch.title) {
        ch.title = text
        if (i === opts.activeIndex.value) {
          opts.activeTitle.value = text
        }
        updated += 1
      }
    }
    if (updated > 0) {
      opts.hasUnsavedEdits.value = true
      triggerRef(opts.chapters)
    }
    opts.showToast(
      updated > 0 ? `已从正文标题同步更新 ${updated} 个章节名` : '目录标题已与正文标题一致',
    )
  }

  async function insertVolumeBelow(idx: number): Promise<void> {
    opts.flushActiveSurfaceToRecord()
    const currentList = opts.chapters.value
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

    let adoptedCount = 0
    for (let i = insertAt + 1; i < nextList.length; i += 1) {
      const ch = nextList[i]!
      if (ch.title === '书籍简介' || isVolumeRecord(nextList, i)) break
      if (ch.level === 0) {
        ch.level = 1
        adoptedCount += 1
      }
    }

    opts.chapters.value = nextList
    opts.activeIndex.value = insertAt
    opts.hasUnsavedEdits.value = true
    await opts.mountActiveChapterIntoSurface()
    opts.showToast(
      adoptedCount > 0
        ? `已插入「${title}」，并将后续 ${adoptedCount} 章归入该卷`
        : `已插入「${title}」（可在目录双击重命名或拖拽排序）`,
    )
  }

  async function insertChapterBelow(idx: number): Promise<void> {
    opts.flushActiveSurfaceToRecord()
    const cur = opts.chapters.value[idx]
    const refLevel = cur
      ? isVolumeRecord(opts.chapters.value, idx)
        ? 1
        : cur.level
      : 0
    const title = `新章节 ${opts.chapters.value.length + 1}`
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
    const nextList = [...opts.chapters.value]
    const insertAt =
      nextList.length === 0 ? 0 : Math.min(nextList.length, Math.max(0, idx + 1))
    nextList.splice(insertAt, 0, newRec)
    opts.chapters.value = nextList
    opts.activeIndex.value = insertAt
    opts.hasUnsavedEdits.value = true
    await opts.mountActiveChapterIntoSurface()
  }

  async function mergeWithPrevious(idx: number): Promise<void> {
    if (idx <= 0 || idx >= opts.chapters.value.length) return
    opts.flushActiveSurfaceToRecord()
    const prev = opts.chapters.value[idx - 1]!
    const cur = opts.chapters.value[idx]!

    const rewrittenCurBody = rewriteChapterRelativeAssetPaths(
      cur.xhtmlBody,
      cur.sourceHref,
      prev.sourceHref,
    )
    prev.xhtmlBody = [prev.xhtmlBody.trim(), rewrittenCurBody.trim()].filter(Boolean).join('\n\n')
    prev.plainCache = extractPlainFromXhtml(prev.xhtmlBody, prev.title)
    prev.charCount = countNonWhitespace(prev.plainCache)
    prev.hasRichMedia = prev.hasRichMedia || cur.hasRichMedia

    const nextList = [...opts.chapters.value]
    nextList.splice(idx, 1)
    opts.chapters.value = nextList
    if (opts.activeIndex.value >= idx) {
      opts.activeIndex.value = Math.max(0, opts.activeIndex.value - 1)
    }
    opts.hasUnsavedEdits.value = true
    await opts.mountActiveChapterIntoSurface()
    opts.showToast(`已将「${cur.title}」合并至上一章`)
  }

  async function deleteChapterAt(idx: number): Promise<void> {
    if (opts.chapters.value.length <= 1) {
      opts.showToast('图书至少需要保留一个章节')
      return
    }
    const target = opts.chapters.value[idx]
    if (!target) return
    const ok = await opts.confirmDelete(target.title)
    if (!ok) return
    opts.flushActiveSurfaceToRecord()
    const nextList = [...opts.chapters.value]
    nextList.splice(idx, 1)
    opts.chapters.value = nextList
    if (opts.activeIndex.value >= nextList.length) {
      opts.activeIndex.value = nextList.length - 1
    }
    opts.hasUnsavedEdits.value = true
    await opts.mountActiveChapterIntoSurface()
  }

  async function handleReorderChapter(
    from: number,
    targetIdx: number,
    pos: 'before' | 'after',
  ): Promise<void> {
    if (from === targetIdx) return

    opts.flushActiveSurfaceToRecord()
    const activeKey = opts.chapters.value[opts.activeIndex.value]?.key
    const nextList = [...opts.chapters.value]
    const [moved] = nextList.splice(from, 1)
    if (!moved) return

    let insertAt = targetIdx
    if (from < targetIdx) insertAt -= 1
    if (pos === 'after') insertAt += 1
    insertAt = Math.max(0, Math.min(nextList.length, insertAt))

    nextList.splice(insertAt, 0, moved)
    opts.chapters.value = nextList
    if (activeKey) {
      const newActive = nextList.findIndex((c) => c.key === activeKey)
      if (newActive >= 0) opts.activeIndex.value = newActive
    }
    opts.hasUnsavedEdits.value = true
  }

  return {
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
  }
}

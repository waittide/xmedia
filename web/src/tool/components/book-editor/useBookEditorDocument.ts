import { onBeforeUnmount, ref, triggerRef, watch, type Ref, type ShallowRef } from 'vue'
import { buildEpubAssetUrl, toolApi } from '../../api'
import type {
  BookTextTransformOptions,
  EditableBookChapterDto,
  MediaFileDetail,
  MediaFileEntry,
  SaveBookChapterInput,
} from '../../types'
import { cssPresets } from './BookEditorCssPanel.vue'
import type { ChapterRecord, SurfaceMode } from './useBookEditorChapters'
import { countNonWhitespace, escapeXml, extractPlainFromXhtml } from './xhtmlUtils'

export interface UseBookEditorDocumentOptions {
  open: Ref<boolean>
  entry: Ref<MediaFileEntry | null>
  docFormat: Ref<'txt' | 'epub'>
  docTitle: Ref<string>
  docAuthors: Ref<string[]>
  docStylesheets: ShallowRef<string[]>
  forceHorizontalSpine: Ref<boolean>
  primaryCssDraft: Ref<string>
  chapters: ShallowRef<ChapterRecord[]>
  activeIndex: Ref<number>
  activeTitle: Ref<string>
  hasUnsavedEdits: Ref<boolean>
  surfaceMode: Ref<SurfaceMode>
  showLivePreview: Ref<boolean>
  sourceBuffer: Ref<string>
  previewSrcdoc: Ref<string>
  wysiwygRef: Ref<HTMLDivElement | null>
  sourceTextareaRef: Ref<HTMLTextAreaElement | null>
  currentRecord: Ref<ChapterRecord | null>
  tocResetKey: Ref<number>
  deleteSourceTxt: Ref<boolean>
  transformScope: Ref<'book' | 'chapter'>
  errorMsg: Ref<string | null>
  mapDtoList: (list: EditableBookChapterDto[]) => ChapterRecord[]
  flushActiveSurfaceToRecord: () => void
  mountActiveChapterIntoSurface: () => Promise<void>
  buildLivePreviewSrcdoc: (xhtmlBody: string, sourceHref: string | null) => string
  confirmResegment: () => Promise<boolean>
  showToast: (msg: string) => void
  onSaved: (detail: MediaFileDetail) => void
  onConverted: (detail: MediaFileDetail, deletedSourceTxt: boolean) => void
}

export function useBookEditorDocument(opts: UseBookEditorDocumentOptions) {
  const loading = ref(false)
  const saving = ref(false)
  const converting = ref(false)
  const resegmenting = ref(false)
  const transforming = ref(false)
  const polishing = ref(false)
  const uploadingImage = ref(false)
  const savedBadge = ref(false)

  let loadDocSeq = 0
  let savedBadgeTimer: ReturnType<typeof setTimeout> | null = null
  let cssPreviewSyncTimer: ReturnType<typeof setTimeout> | null = null

  function applyCssPreset(cssCode: string, label: string): void {
    opts.primaryCssDraft.value = cssCode
    syncPrimaryCssDraftToStylesheets()
    opts.showToast(`已应用「${label}」排版样式预设`)
  }

  function syncPrimaryCssDraftToStylesheets(): void {
    const next = [...opts.docStylesheets.value]
    if (next.length === 0) {
      next.push(opts.primaryCssDraft.value)
    } else {
      next[0] = opts.primaryCssDraft.value
    }
    opts.docStylesheets.value = next
    opts.hasUnsavedEdits.value = true
    if (cssPreviewSyncTimer) clearTimeout(cssPreviewSyncTimer)
    cssPreviewSyncTimer = setTimeout(() => {
      cssPreviewSyncTimer = null
      if (opts.showLivePreview.value && opts.currentRecord.value && opts.open.value) {
        opts.flushActiveSurfaceToRecord()
        opts.previewSrcdoc.value = opts.buildLivePreviewSrcdoc(
          opts.currentRecord.value.xhtmlBody,
          opts.currentRecord.value.sourceHref,
        )
      }
    }, 160)
  }

  async function runBackendTextTransform(
    options: BookTextTransformOptions,
    summaryLabel: string,
  ): Promise<void> {
    if (transforming.value || opts.chapters.value.length === 0) return
    opts.flushActiveSurfaceToRecord()
    transforming.value = true
    opts.errorMsg.value = null
    try {
      const targetIndices =
        opts.transformScope.value === 'chapter' && !options.vertical_to_horizontal
          ? [opts.activeIndex.value]
          : opts.chapters.value.map((_, i) => i)

      const inputDtos: EditableBookChapterDto[] = targetIndices.map((idx) => {
        const ch = opts.chapters.value[idx]!
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

      const res = await toolApi.transformBookContent(inputDtos, opts.docStylesheets.value, options)
      res.chapters.forEach((dto, pos) => {
        const idx = targetIndices[pos]
        const rec = idx !== undefined ? opts.chapters.value[idx] : undefined
        if (!rec) return
        rec.title = dto.title
        rec.xhtmlBody = dto.xhtml_body
        rec.plainCache = extractPlainFromXhtml(dto.xhtml_body, dto.title)
        rec.charCount = countNonWhitespace(rec.plainCache)
        if (idx === opts.activeIndex.value) {
          opts.activeTitle.value = rec.title
        }
      })

      if (options.vertical_to_horizontal) {
        opts.docStylesheets.value = res.stylesheets
        opts.primaryCssDraft.value = res.stylesheets[0] ?? opts.primaryCssDraft.value
        opts.forceHorizontalSpine.value = true
      }

      opts.hasUnsavedEdits.value = true
      triggerRef(opts.chapters)
      await opts.mountActiveChapterIntoSurface()
      opts.showToast(`${summaryLabel}完成（共更新 ${res.changed_chapters} 章）`)
    } catch (err) {
      opts.errorMsg.value = err instanceof Error ? err.message : String(err)
    } finally {
      transforming.value = false
    }
  }

  async function onEpubImagePicked(ev: Event): Promise<void> {
    const input = ev.target as HTMLInputElement
    const file = input.files?.[0]
    input.value = ''
    if (!file || !opts.entry.value || !opts.currentRecord.value) return

    uploadingImage.value = true
    opts.errorMsg.value = null
    try {
      opts.flushActiveSurfaceToRecord()
      const inserted = await toolApi.uploadBookEpubImage(
        opts.entry.value.root_id,
        opts.entry.value.rel_path,
        file,
        opts.currentRecord.value.sourceHref,
      )
      const browserUrl = buildEpubAssetUrl(
        opts.entry.value.root_id,
        opts.entry.value.rel_path,
        inserted.asset_path,
      )
      const altText = escapeXml(file.name.replace(/\.[^.]+$/, '') || '插图')

      if (opts.surfaceMode.value === 'wysiwyg' && opts.wysiwygRef.value) {
        opts.wysiwygRef.value.focus()
        document.execCommand(
          'insertHTML',
          false,
          `<figure><img src="${escapeXml(browserUrl)}" data-epub-src="${escapeXml(inserted.chapter_rel_href)}" alt="${altText}"/></figure><p><br/></p>`,
        )
        opts.flushActiveSurfaceToRecord()
      } else {
        const snippet = `\n  <figure><img src="${escapeXml(inserted.chapter_rel_href)}" alt="${altText}"/></figure>\n`
        if (opts.sourceTextareaRef.value) {
          const pos = opts.sourceTextareaRef.value.selectionStart ?? opts.sourceBuffer.value.length
          opts.sourceBuffer.value = `${opts.sourceBuffer.value.slice(0, pos)}${snippet}${opts.sourceBuffer.value.slice(pos)}`
        } else {
          opts.sourceBuffer.value = `${opts.sourceBuffer.value}${snippet}`
        }
        opts.flushActiveSurfaceToRecord()
      }

      opts.currentRecord.value.hasRichMedia = true
      opts.hasUnsavedEdits.value = true
      triggerRef(opts.chapters)
      if (opts.showLivePreview.value) {
        opts.previewSrcdoc.value = opts.buildLivePreviewSrcdoc(
          opts.currentRecord.value.xhtmlBody,
          opts.currentRecord.value.sourceHref,
        )
      }
      opts.showToast('图片已写入 EPUB 资源包并插入当前章节')
    } catch (err) {
      opts.errorMsg.value = err instanceof Error ? err.message : String(err)
    } finally {
      uploadingImage.value = false
    }
  }

  function buildSavePayload(): SaveBookChapterInput[] {
    opts.flushActiveSurfaceToRecord()
    return opts.chapters.value.map((ch, idx) => ({
      title: ch.title.trim() || `第 ${idx + 1} 章`,
      level: ch.level,
      mode: 'xhtml',
      plain_text: ch.plainCache,
      xhtml_body: ch.xhtmlBody,
      source_href: ch.sourceHref,
    }))
  }

  async function loadDocument(): Promise<void> {
    if (!opts.entry.value) return
    const seq = ++loadDocSeq
    loading.value = true
    opts.errorMsg.value = null
    opts.hasUnsavedEdits.value = false
    savedBadge.value = false
    opts.tocResetKey.value += 1
    opts.forceHorizontalSpine.value = false

    try {
      const doc = await toolApi.getBookContent(opts.entry.value.root_id, opts.entry.value.rel_path)
      if (seq !== loadDocSeq || !opts.open.value) return
      opts.docFormat.value = doc.format
      opts.docTitle.value = doc.title
      opts.docAuthors.value = doc.authors
      opts.docStylesheets.value = doc.stylesheets ?? []
      opts.primaryCssDraft.value = doc.stylesheets?.[0] ?? cssPresets[0]!.css
      opts.chapters.value = opts.mapDtoList(doc.chapters)
      opts.activeIndex.value = 0
      opts.surfaceMode.value = 'wysiwyg'
      loading.value = false
      await opts.mountActiveChapterIntoSurface()
    } catch (err) {
      if (seq !== loadDocSeq) return
      opts.errorMsg.value = err instanceof Error ? err.message : String(err)
      opts.chapters.value = []
    } finally {
      if (seq === loadDocSeq) loading.value = false
    }
  }

  async function handlePolishInEditor(): Promise<void> {
    if (!opts.entry.value || opts.docFormat.value !== 'epub' || polishing.value || saving.value) {
      return
    }
    polishing.value = true
    opts.errorMsg.value = null
    try {
      if (opts.hasUnsavedEdits.value && opts.chapters.value.length > 0) {
        await toolApi.saveBookContent(
          opts.entry.value.root_id,
          opts.entry.value.rel_path,
          buildSavePayload(),
          opts.docStylesheets.value.length > 0 ? opts.docStylesheets.value : undefined,
          opts.forceHorizontalSpine.value,
        )
        opts.hasUnsavedEdits.value = false
        opts.forceHorizontalSpine.value = false
      }
      const res = await toolApi.polishBook(opts.entry.value.root_id, opts.entry.value.rel_path)
      opts.onSaved(res.detail)
      await loadDocument()
      opts.showToast(
        res.removed_orphan_files > 0
          ? `抛光完成：已生成内封简介页并清理 ${res.removed_orphan_files} 个冗余资源`
          : '抛光完成：已更新内封简介页并同步双目录',
      )
    } catch (err) {
      opts.errorMsg.value = err instanceof Error ? err.message : String(err)
    } finally {
      polishing.value = false
    }
  }

  async function handleResegmentTxt(chapterRegex?: string, volumeRegex?: string): Promise<void> {
    if (!opts.entry.value || resegmenting.value) return
    if (opts.hasUnsavedEdits.value && !(await opts.confirmResegment())) return
    resegmenting.value = true
    opts.errorMsg.value = null
    try {
      const res = await toolApi.resegmentTxtBook(
        opts.entry.value.root_id,
        opts.entry.value.rel_path,
        chapterRegex,
        volumeRegex,
      )
      opts.chapters.value = opts.mapDtoList(res)
      opts.activeIndex.value = 0
      opts.hasUnsavedEdits.value = true
      await opts.mountActiveChapterIntoSurface()
      opts.showToast(`已重新切分为 ${res.length} 个章节`)
    } catch (err) {
      opts.errorMsg.value = err instanceof Error ? err.message : String(err)
    } finally {
      resegmenting.value = false
    }
  }

  async function handleSaveInPlace(): Promise<void> {
    if (!opts.entry.value || saving.value || opts.chapters.value.length === 0) return
    saving.value = true
    opts.errorMsg.value = null
    try {
      const updated = await toolApi.saveBookContent(
        opts.entry.value.root_id,
        opts.entry.value.rel_path,
        buildSavePayload(),
        opts.docStylesheets.value.length > 0 ? opts.docStylesheets.value : undefined,
        opts.forceHorizontalSpine.value,
      )
      opts.hasUnsavedEdits.value = false
      opts.forceHorizontalSpine.value = false
      savedBadge.value = true
      if (savedBadgeTimer) clearTimeout(savedBadgeTimer)
      savedBadgeTimer = setTimeout(() => {
        savedBadge.value = false
      }, 2200)
      opts.onSaved(updated)
    } catch (err) {
      opts.errorMsg.value = err instanceof Error ? err.message : String(err)
    } finally {
      saving.value = false
    }
  }

  async function handleConvertToEpub(): Promise<void> {
    if (!opts.entry.value || converting.value || opts.chapters.value.length === 0) return
    converting.value = true
    opts.errorMsg.value = null
    try {
      const res = await toolApi.convertTxtToEpub(
        opts.entry.value.root_id,
        opts.entry.value.rel_path,
        buildSavePayload(),
        opts.deleteSourceTxt.value,
        opts.docStylesheets.value.length > 0 ? opts.docStylesheets.value : undefined,
      )
      opts.hasUnsavedEdits.value = false
      opts.onConverted(res.detail, res.deleted_source_txt)
      opts.showToast(`已打包生成 EPUB：${res.detail.entry.file_name}`)
    } catch (err) {
      opts.errorMsg.value = err instanceof Error ? err.message : String(err)
    } finally {
      converting.value = false
    }
  }

  watch(
    () => [opts.open.value, opts.entry.value?.rel_path] as const,
    ([isOpen]) => {
      if (isOpen && opts.entry.value) {
        opts.deleteSourceTxt.value = false
        void loadDocument()
      } else if (!isOpen) {
        loadDocSeq += 1
        if (cssPreviewSyncTimer) {
          clearTimeout(cssPreviewSyncTimer)
          cssPreviewSyncTimer = null
        }
        opts.chapters.value = []
        opts.docStylesheets.value = []
        opts.previewSrcdoc.value = ''
        opts.sourceBuffer.value = ''
      }
    },
    { immediate: true },
  )

  onBeforeUnmount(() => {
    if (savedBadgeTimer) clearTimeout(savedBadgeTimer)
    if (cssPreviewSyncTimer) clearTimeout(cssPreviewSyncTimer)
  })

  return {
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
  }
}

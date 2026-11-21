import { computed, ref, type ComputedRef, type Ref } from 'vue'
import { useSnackbarStore } from '@/stores/snackbar'
import type { MediaKind } from '@/types/api'
import { toolApi } from '../../api'
import {
  formatAutoScrapeSummary,
  loadLastAutoScrapeOptions,
  saveLastAutoScrapeOptions,
} from '../../autoScrapePrefs'
import type {
  AutoScrapeOptionsDto,
  BatchMetadataEditReportDto,
  DirectorySnapshot,
  ManagedRootDto,
  MediaFileDetail,
  MediaFileEntry,
  NormalizeBooksBatchReportDto,
} from '../../types'

export interface UseWorkspaceModalsOptions {
  activeDomain: Ref<MediaKind>
  activeRootId: Ref<string>
  domainRoots: ComputedRef<ManagedRootDto[]>
  snapshot: Ref<DirectorySnapshot | null>
  filteredFiles: ComputedRef<MediaFileEntry[]>
  checkedFiles: ComputedRef<MediaFileEntry[]>
  selectedEntriesForRename: ComputedRef<MediaFileEntry[]>
  selectedPaths: Ref<Set<string>>
  inspectingPath: Ref<string | null>
  activeDetail: Ref<MediaFileDetail | null>
  artworkVersion: Ref<number>
  deleteDialogOpen: Ref<boolean>
  tasksDrawerOpen: Ref<boolean>
  resolveModalRootId: (entries: MediaFileEntry[]) => string | null
  entrySelectionKey: (entry: Pick<MediaFileEntry, 'root_id' | 'rel_path'>) => string
  parseSelectionKey: (key: string) => { rootId: string; relPath: string }
  bumpFileArtworkVersion: (rootId: string, relPath: string) => void
  clearActiveDetail: () => void
  reloadCurrentScope: () => Promise<void>
  inspectEntry: (entry: MediaFileEntry) => Promise<void>
  handleDetailUpdated: (updated: MediaFileDetail, bumpArt?: boolean) => Promise<void>
  openScrapeWorkbench: (
    targetEntry?: MediaFileEntry,
    scopedEntries?: MediaFileEntry[],
  ) => Promise<void>
  enqueueScrapeFiles: (
    files: MediaFileEntry[],
    labelHint?: string,
    options?: AutoScrapeOptionsDto,
  ) => void
  bumpScrapeStrategyRev: () => void
}

export function useWorkspaceModals(opts: UseWorkspaceModalsOptions) {
  const snackbar = useSnackbarStore()

  const renameModalOpen = ref(false)
  const renameTargetEntries = ref<MediaFileEntry[] | null>(null)
  const renameSkippedOrganizedCount = ref(0)
  const batchMetadataModalOpen = ref(false)
  const historyModalOpen = ref(false)
  const bookEditorOpen = ref(false)
  const bookEditorEntry = ref<MediaFileEntry | null>(null)
  const bookNormalizeOpen = ref(false)
  const bookNormalizeEntries = ref<MediaFileEntry[]>([])
  const comicPageManagerOpen = ref(false)
  const comicPageManagerEntry = ref<MediaFileEntry | null>(null)
  const comicSeriesToolsOpen = ref(false)
  const musicToolsOpen = ref(false)
  const musicToolsEntries = ref<MediaFileEntry[]>([])
  const videoToolsOpen = ref(false)
  const videoToolsEntries = ref<MediaFileEntry[]>([])
  const dryRunModalOpen = ref(false)
  const dryRunTargetEntries = ref<MediaFileEntry[] | null>(null)
  const dryRunScrapeOptions = ref<AutoScrapeOptionsDto | undefined>(undefined)
  const autoScrapeModalOpen = ref(false)
  const autoScrapeTargetEntries = ref<MediaFileEntry[]>([])

  const selectedComicEntries = computed(() =>
    opts.selectedEntriesForRename.value.filter((e) => e.media_kind === 'comic'),
  )
  const selectedMusicEntries = computed(() =>
    opts.selectedEntriesForRename.value.filter((e) => e.media_kind === 'music'),
  )
  const selectedVideoEntries = computed(() =>
    opts.selectedEntriesForRename.value.filter((e) => e.media_kind === 'video'),
  )

  const fallbackRootId = computed<string>(
    () => opts.activeRootId.value || opts.snapshot.value?.root_id || opts.domainRoots.value[0]?.id || '',
  )

  const effectiveModalRootId = computed<string>(
    () =>
      opts.resolveModalRootId(renameTargetEntries.value ?? opts.selectedEntriesForRename.value) ||
      fallbackRootId.value,
  )
  const effectiveBookNormalizeRootId = computed<string>(
    () => opts.resolveModalRootId(bookNormalizeEntries.value) || fallbackRootId.value,
  )
  const effectiveMusicRootId = computed<string>(
    () => opts.resolveModalRootId(musicToolsEntries.value) || fallbackRootId.value,
  )
  const effectiveVideoRootId = computed<string>(
    () => opts.resolveModalRootId(videoToolsEntries.value) || fallbackRootId.value,
  )
  const effectiveComicRootId = computed<string>(
    () => opts.resolveModalRootId(selectedComicEntries.value) || fallbackRootId.value,
  )
  const effectiveDryRunRootId = computed<string>(
    () =>
      opts.resolveModalRootId(dryRunTargetEntries.value ?? opts.selectedEntriesForRename.value) ||
      fallbackRootId.value,
  )
  const effectiveHistoryRootId = computed<string>(
    () =>
      opts.activeRootId.value ||
      opts.activeDetail.value?.entry.root_id ||
      opts.snapshot.value?.root_id ||
      opts.domainRoots.value[0]?.id ||
      '',
  )

  const anyModalOpen = computed(
    () =>
      opts.tasksDrawerOpen.value ||
      renameModalOpen.value ||
      opts.deleteDialogOpen.value ||
      batchMetadataModalOpen.value ||
      historyModalOpen.value ||
      autoScrapeModalOpen.value ||
      bookEditorOpen.value ||
      bookNormalizeOpen.value ||
      comicPageManagerOpen.value ||
      comicSeriesToolsOpen.value ||
      musicToolsOpen.value ||
      videoToolsOpen.value ||
      dryRunModalOpen.value,
  )

  function openRenameModal(entries?: MediaFileEntry[]): void {
    const targets = entries && entries.length > 0 ? entries : opts.selectedEntriesForRename.value
    const pending = targets.filter((e) => !e.completeness.is_organized)
    const skippedCount = targets.length - pending.length
    if (targets.length > 0 && pending.length === 0) {
      snackbar.show({
        message: '所选条目均已按默认模板整理归档，无需重复整理',
      })
      return
    }
    if (pending.length > 0 && !opts.resolveModalRootId(pending)) {
      snackbar.show({
        message: '所选条目跨越多个挂载目录，请先从左侧选择单个挂载目录再整理归档',
        tone: 'error',
      })
      return
    }
    renameSkippedOrganizedCount.value = skippedCount
    renameTargetEntries.value = pending
    renameModalOpen.value = true
  }

  function closeRenameModal(): void {
    renameModalOpen.value = false
    renameTargetEntries.value = null
    renameSkippedOrganizedCount.value = 0
  }

  function openBatchMetadataModal(): void {
    if (
      opts.selectedEntriesForRename.value.length > 0 &&
      !opts.resolveModalRootId(opts.selectedEntriesForRename.value)
    ) {
      snackbar.show({
        message: '所选条目跨越多个挂载目录，请先从左侧选择单个挂载目录再批量编辑',
        tone: 'error',
      })
      return
    }
    batchMetadataModalOpen.value = true
  }

  function openDryRunModal(): void {
    if (
      opts.selectedEntriesForRename.value.length > 0 &&
      !opts.resolveModalRootId(opts.selectedEntriesForRename.value)
    ) {
      snackbar.show({
        message: '所选条目跨越多个挂载目录，请先从左侧选择单个挂载目录再进行置信度预演',
        tone: 'error',
      })
      return
    }
    dryRunTargetEntries.value = null
    dryRunScrapeOptions.value = undefined
    dryRunModalOpen.value = true
  }

  function closeDryRunModal(): void {
    dryRunModalOpen.value = false
    dryRunTargetEntries.value = null
    dryRunScrapeOptions.value = undefined
  }

  function openComicSeriesToolsModal(): void {
    if (
      selectedComicEntries.value.length > 0 &&
      !opts.resolveModalRootId(selectedComicEntries.value)
    ) {
      snackbar.show({
        message: '所选漫画跨越多个挂载目录，请先从左侧选择单个挂载目录再使用漫画工具',
        tone: 'error',
      })
      return
    }
    comicSeriesToolsOpen.value = true
  }

  function openBookContentEditor(entry: MediaFileEntry): void {
    bookEditorEntry.value = entry
    bookEditorOpen.value = true
  }

  function openComicPageManager(entry: MediaFileEntry): void {
    comicPageManagerEntry.value = entry
    comicPageManagerOpen.value = true
  }

  function openMusicToolbox(entries: MediaFileEntry[]): void {
    if (entries.length > 0 && !opts.resolveModalRootId(entries)) {
      snackbar.show({
        message: '所选音频跨越多个挂载目录，请先从左侧选择单个挂载目录再使用音频工具',
        tone: 'error',
      })
      return
    }
    musicToolsEntries.value = entries
    musicToolsOpen.value = true
  }

  function openVideoToolbox(entries: MediaFileEntry[]): void {
    if (entries.length > 0 && !opts.resolveModalRootId(entries)) {
      snackbar.show({
        message: '所选视频跨越多个挂载目录，请先从左侧选择单个挂载目录再使用视频工具',
        tone: 'error',
      })
      return
    }
    videoToolsEntries.value = entries
    videoToolsOpen.value = true
  }

  function openBookNormalizeModal(entries: MediaFileEntry[]): void {
    const valid = entries.filter(
      (e) =>
        e.media_kind === 'book' &&
        (e.extension === 'txt' || e.extension === 'epub' || e.extension === 'pdf'),
    )
    if (valid.length === 0) return
    if (!opts.resolveModalRootId(valid)) {
      snackbar.show({
        message: '所选图书跨越多个挂载目录，请先从左侧选择单个挂载目录再转为规范化 EPUB',
        tone: 'error',
      })
      return
    }
    bookNormalizeEntries.value = valid
    bookNormalizeOpen.value = true
  }

  function quickScrapeSelection(): void {
    const targets =
      opts.checkedFiles.value.length > 0
        ? [...opts.checkedFiles.value]
        : [...opts.filteredFiles.value]
    if (targets.length === 0) return
    const autoOpts = loadLastAutoScrapeOptions(opts.activeDomain.value)
    opts.enqueueScrapeFiles(targets, undefined, autoOpts)
    snackbar.show({
      message: `已按「${formatAutoScrapeSummary(autoOpts)}」策略提交 ${targets.length} 项快刮任务`,
      actionLabel: '调整策略',
      onAction: () => {
        autoScrapeTargetEntries.value = targets
        autoScrapeModalOpen.value = true
      },
    })
  }

  function configureScrapeSelection(): void {
    const targets =
      opts.checkedFiles.value.length > 0
        ? [...opts.checkedFiles.value]
        : [...opts.filteredFiles.value]
    if (targets.length === 0) return
    autoScrapeTargetEntries.value = targets
    autoScrapeModalOpen.value = true
  }

  function handleAutoScrapeConfirmed(
    entries: MediaFileEntry[],
    options: AutoScrapeOptionsDto,
  ): void {
    autoScrapeModalOpen.value = false
    saveLastAutoScrapeOptions(opts.activeDomain.value, options)
    opts.bumpScrapeStrategyRev()
    opts.enqueueScrapeFiles(entries, undefined, options)
  }

  function handleAutoScrapeToDryRun(
    entries: MediaFileEntry[],
    options: AutoScrapeOptionsDto,
  ): void {
    autoScrapeModalOpen.value = false
    saveLastAutoScrapeOptions(opts.activeDomain.value, options)
    opts.bumpScrapeStrategyRev()
    if (entries.length > 0 && !opts.resolveModalRootId(entries)) {
      snackbar.show({
        message: '所选条目跨越多个挂载目录，请先从左侧选择单个挂载目录再进行置信度预演',
        tone: 'error',
      })
      return
    }
    opts.selectedPaths.value = new Set(entries.map((e) => opts.entrySelectionKey(e)))
    dryRunTargetEntries.value = entries
    dryRunScrapeOptions.value = options
    dryRunModalOpen.value = true
  }

  async function handleDryRunManualScrape(entries: MediaFileEntry[]): Promise<void> {
    dryRunModalOpen.value = false
    if (entries.length === 0) return
    await opts.openScrapeWorkbench(entries[0], entries)
  }

  async function handleRenamed(
    movedCount: number,
    movedMap: Record<string, string>,
  ): Promise<void> {
    const prevActive = opts.activeDetail.value
    const mappedRel = prevActive ? movedMap[prevActive.entry.rel_path] : undefined
    const activeEntryRootId = prevActive?.entry.root_id || effectiveModalRootId.value
    if (mappedRel && prevActive) {
      opts.inspectingPath.value = opts.entrySelectionKey({
        root_id: activeEntryRootId,
        rel_path: mappedRel,
      })
    }
    if (movedCount > 0 && opts.selectedPaths.value.size > 0) {
      const nextSel = new Set<string>()
      for (const p of opts.selectedPaths.value) {
        const { relPath } = opts.parseSelectionKey(p)
        if (!(relPath in movedMap)) {
          nextSel.add(p)
        }
      }
      opts.selectedPaths.value = nextSel
    }
    renameTargetEntries.value = null
    renameSkippedOrganizedCount.value = 0
    if (movedCount > 0) {
      await opts.reloadCurrentScope()
      snackbar.show({
        message: `已完成 ${movedCount} 个文件的整理与归档`,
        actionLabel: '撤销历史',
        onAction: () => {
          historyModalOpen.value = true
        },
      })
    }
    if (mappedRel && activeEntryRootId) {
      try {
        const refreshed = await toolApi.inspectFile(
          activeEntryRootId,
          mappedRel,
          opts.activeDomain.value,
        )
        opts.activeDetail.value = refreshed
        opts.inspectingPath.value = opts.entrySelectionKey(refreshed.entry)
      } catch {
        // Fall back safely if renamed file lives outside filtered branch
      }
    }
  }

  async function handleHistoryRestored(): Promise<void> {
    opts.artworkVersion.value += 1
    await opts.reloadCurrentScope()
    if (opts.activeDetail.value) {
      const stillHere = (opts.snapshot.value?.files ?? []).find(
        (f) =>
          f.root_id === opts.activeDetail.value?.entry.root_id &&
          f.rel_path === opts.activeDetail.value?.entry.rel_path,
      )
      if (stillHere) {
        void opts.inspectEntry(stillHere)
      } else {
        opts.clearActiveDetail()
      }
    }
  }

  async function handleBatchMetadataCompleted(
    report: BatchMetadataEditReportDto,
  ): Promise<void> {
    const targeted = opts.selectedEntriesForRename.value
    if (targeted.length > 0) {
      for (const entry of targeted) {
        opts.bumpFileArtworkVersion(entry.root_id, entry.rel_path)
      }
    } else {
      opts.artworkVersion.value += 1
    }
    await opts.reloadCurrentScope()
    if (opts.activeDetail.value) {
      const stillHere = (opts.snapshot.value?.files ?? []).find(
        (f) =>
          f.root_id === opts.activeDetail.value?.entry.root_id &&
          f.rel_path === opts.activeDetail.value?.entry.rel_path,
      )
      if (stillHere) {
        void opts.inspectEntry(stillHere)
      }
    }
    const batchId = report.batch_id
    snackbar.show({
      message: `批量编辑完成：更新 ${report.updated_count} 项${
        report.skipped_count > 0 ? `，跳过 ${report.skipped_count} 项` : ''
      }${report.failed_count > 0 ? `，失败 ${report.failed_count} 项` : ''}`,
      actionLabel: batchId ? '撤销' : '历史',
      onAction: async () => {
        if (batchId) {
          const res = await toolApi.undoMetadataBatch(batchId)
          await handleHistoryRestored()
          snackbar.show(`已撤销本次批量修改，恢复 ${res.restored_count} 个文件`)
        } else {
          historyModalOpen.value = true
        }
      },
    })
  }

  async function handleBookContentSaved(updated: MediaFileDetail): Promise<void> {
    await opts.handleDetailUpdated(updated, true)
    snackbar.show(`已保存章节目录与正文：${updated.entry.file_name}`)
  }

  async function handleTxtConvertedToEpub(
    epubDetail: MediaFileDetail,
    deletedSourceTxt: boolean,
  ): Promise<void> {
    opts.bumpFileArtworkVersion(epubDetail.entry.root_id, epubDetail.entry.rel_path)
    await opts.reloadCurrentScope()
    opts.activeDetail.value = epubDetail
    opts.inspectingPath.value = opts.entrySelectionKey(epubDetail.entry)
    bookEditorEntry.value = epubDetail.entry
    snackbar.show(
      deletedSourceTxt
        ? `已转换为 EPUB 并移除原 TXT：${epubDetail.entry.file_name}`
        : `已生成标准 EPUB：${epubDetail.entry.file_name}`,
    )
  }

  async function handleBookNormalizeCompleted(
    report: NormalizeBooksBatchReportDto,
  ): Promise<void> {
    const rootId = effectiveBookNormalizeRootId.value
    for (const it of report.items) {
      if (!it.error) {
        opts.bumpFileArtworkVersion(rootId, it.output_rel_path || it.source_rel_path)
      }
    }
    await opts.reloadCurrentScope()
    if (opts.activeDetail.value) {
      const matchedItem = report.items.find(
        (it) => !it.error && it.source_rel_path === opts.activeDetail.value?.entry.rel_path,
      )
      const targetRel = matchedItem?.output_rel_path ?? opts.activeDetail.value.entry.rel_path
      const refreshed = (opts.snapshot.value?.files ?? []).find(
        (f) => f.root_id === opts.activeDetail.value?.entry.root_id && f.rel_path === targetRel,
      )
      if (refreshed) {
        opts.activeDetail.value = null
        await opts.inspectEntry(refreshed)
      }
    }
    snackbar.show(
      report.failed > 0
        ? `规范化 EPUB 完成：成功 ${report.succeeded} 本，失败 ${report.failed} 本`
        : `规范化 EPUB 完成：共处理 ${report.succeeded} 本`,
    )
  }

  return {
    renameModalOpen,
    renameTargetEntries,
    renameSkippedOrganizedCount,
    batchMetadataModalOpen,
    historyModalOpen,
    bookEditorOpen,
    bookEditorEntry,
    bookNormalizeOpen,
    bookNormalizeEntries,
    comicPageManagerOpen,
    comicPageManagerEntry,
    comicSeriesToolsOpen,
    musicToolsOpen,
    musicToolsEntries,
    videoToolsOpen,
    videoToolsEntries,
    dryRunModalOpen,
    dryRunTargetEntries,
    dryRunScrapeOptions,
    autoScrapeModalOpen,
    autoScrapeTargetEntries,
    selectedComicEntries,
    selectedMusicEntries,
    selectedVideoEntries,
    effectiveModalRootId,
    effectiveBookNormalizeRootId,
    effectiveMusicRootId,
    effectiveVideoRootId,
    effectiveComicRootId,
    effectiveDryRunRootId,
    effectiveHistoryRootId,
    anyModalOpen,
    openRenameModal,
    closeRenameModal,
    openBatchMetadataModal,
    openDryRunModal,
    closeDryRunModal,
    openComicSeriesToolsModal,
    openBookContentEditor,
    openComicPageManager,
    openMusicToolbox,
    openVideoToolbox,
    openBookNormalizeModal,
    quickScrapeSelection,
    configureScrapeSelection,
    handleAutoScrapeConfirmed,
    handleAutoScrapeToDryRun,
    handleDryRunManualScrape,
    handleRenamed,
    handleHistoryRestored,
    handleBatchMetadataCompleted,
    handleBookContentSaved,
    handleTxtConvertedToEpub,
    handleBookNormalizeCompleted,
  }
}

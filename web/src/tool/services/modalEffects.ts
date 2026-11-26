import { useSnackbarStore } from '@/stores/snackbar'
import { toolApi } from '../api'
import type { useToolDirectoryStore } from '../stores/directory'
import type { useToolInspectorStore } from '../stores/inspector'
import type { useToolNavigationStore } from '../stores/navigation'
import type { useToolSelectionStore } from '../stores/selection'
import type {
  BatchMetadataEditReportDto,
  MediaFileDetail,
  MediaFileEntry,
  NormalizeBooksBatchReportDto,
} from '../types'

export interface ModalEffectsContext {
  directoryStore: ReturnType<typeof useToolDirectoryStore>
  selectionStore: ReturnType<typeof useToolSelectionStore>
  inspectorStore: ReturnType<typeof useToolInspectorStore>
  navigationStore: ReturnType<typeof useToolNavigationStore>
  getEffectiveModalRootId: () => string
  getEffectiveBookNormalizeRootId: () => string
  resetRenameTargets: () => void
  setBookEditorEntry: (entry: MediaFileEntry) => void
  openHistoryModal: (tab: 'metadata' | 'rename') => void
}

export function createModalCompletionHandlers(ctx: ModalEffectsContext) {
  const snackbar = useSnackbarStore()
  const { directoryStore, selectionStore, inspectorStore, navigationStore } = ctx

  async function handleRenamed(movedCount: number, movedMap: Record<string, string>): Promise<void> {
    const prevActive = inspectorStore.activeDetail
    const mappedRel = prevActive ? movedMap[prevActive.entry.rel_path] : undefined
    const activeEntryRootId = prevActive?.entry.root_id || ctx.getEffectiveModalRootId()
    if (mappedRel && prevActive) {
      inspectorStore.inspectingPath = directoryStore.entrySelectionKey({
        root_id: activeEntryRootId,
        rel_path: mappedRel,
      })
    }
    if (movedCount > 0 && selectionStore.selectedPaths.size > 0) {
      selectionStore.selectedPaths = new Set(
        [...selectionStore.selectedPaths].filter(
          (p) => !(directoryStore.parseSelectionKey(p).relPath in movedMap),
        ),
      )
    }
    ctx.resetRenameTargets()
    if (movedCount > 0) {
      await directoryStore.reloadCurrentScope()
      snackbar.show({
        message: `已完成 ${movedCount} 个文件的整理与归档`,
        actionLabel: '历史',
        onAction: () => ctx.openHistoryModal('rename'),
      })
    }
    if (mappedRel && activeEntryRootId) {
      try {
        const refreshed = await toolApi.inspectFile(
          activeEntryRootId,
          mappedRel,
          directoryStore.activeDomain,
        )
        inspectorStore.activeDetail = refreshed
        inspectorStore.inspectingPath = directoryStore.entrySelectionKey(refreshed.entry)
        directoryStore.notifyRouteSync(true)
      } catch {
        // Fall back safely if renamed file lives outside filtered branch
      }
    }
  }

  async function handleHistoryRestored(): Promise<void> {
    inspectorStore.artworkVersion += 1
    await directoryStore.reloadCurrentScope()
    if (inspectorStore.activeDetail) {
      const stillHere = (directoryStore.snapshot?.files ?? []).find(
        (f) =>
          f.root_id === inspectorStore.activeDetail?.entry.root_id &&
          f.rel_path === inspectorStore.activeDetail?.entry.rel_path,
      )
      if (stillHere) {
        inspectorStore.activeDetail = null
        await navigationStore.inspectEntry(stillHere, { skipDirtyCheck: true, replaceRoute: true })
      } else {
        navigationStore.closeInspectorAndSync()
      }
    }
  }

  async function handleBatchMetadataCompleted(report: BatchMetadataEditReportDto): Promise<void> {
    const targeted = selectionStore.selectedEntriesForRename
    if (targeted.length > 0) {
      for (const entry of targeted) {
        inspectorStore.bumpFileArtworkVersion(entry.root_id, entry.rel_path)
      }
    } else {
      inspectorStore.artworkVersion += 1
    }
    await directoryStore.reloadCurrentScope()
    if (inspectorStore.activeDetail) {
      const stillHere = (directoryStore.snapshot?.files ?? []).find(
        (f) =>
          f.root_id === inspectorStore.activeDetail?.entry.root_id &&
          f.rel_path === inspectorStore.activeDetail?.entry.rel_path,
      )
      if (stillHere) {
        inspectorStore.activeDetail = null
        void navigationStore.inspectEntry(stillHere, { skipDirtyCheck: true, replaceRoute: true })
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
          ctx.openHistoryModal('metadata')
        }
      },
    })
  }

  async function handleTxtConvertedToEpub(
    epubDetail: MediaFileDetail,
    deletedSourceTxt: boolean,
  ): Promise<void> {
    inspectorStore.bumpFileArtworkVersion(epubDetail.entry.root_id, epubDetail.entry.rel_path)
    await directoryStore.reloadCurrentScope()
    inspectorStore.activeDetail = epubDetail
    inspectorStore.inspectingPath = directoryStore.entrySelectionKey(epubDetail.entry)
    ctx.setBookEditorEntry(epubDetail.entry)
    directoryStore.notifyRouteSync(true)
    snackbar.show(
      deletedSourceTxt
        ? `已转换为 EPUB 并移除原 TXT：${epubDetail.entry.file_name}`
        : `已生成标准 EPUB：${epubDetail.entry.file_name}`,
    )
  }

  async function handleBookNormalizeCompleted(report: NormalizeBooksBatchReportDto): Promise<void> {
    const rootId = ctx.getEffectiveBookNormalizeRootId()
    for (const it of report.items) {
      if (!it.error) {
        inspectorStore.bumpFileArtworkVersion(rootId, it.output_rel_path || it.source_rel_path)
      }
    }
    await directoryStore.reloadCurrentScope()
    if (inspectorStore.activeDetail) {
      const matchedItem = report.items.find(
        (it) => !it.error && it.source_rel_path === inspectorStore.activeDetail?.entry.rel_path,
      )
      const targetRel = matchedItem?.output_rel_path ?? inspectorStore.activeDetail.entry.rel_path
      const refreshed = (directoryStore.snapshot?.files ?? []).find(
        (f) => f.root_id === inspectorStore.activeDetail?.entry.root_id && f.rel_path === targetRel,
      )
      if (refreshed) {
        inspectorStore.activeDetail = null
        await navigationStore.inspectEntry(refreshed, { skipDirtyCheck: true, replaceRoute: true })
      }
    }
    snackbar.show(
      report.failed > 0
        ? `规范化 EPUB 完成：成功 ${report.succeeded} 本，失败 ${report.failed} 本`
        : `规范化 EPUB 完成：共处理 ${report.succeeded} 本`,
    )
  }

  return {
    handleRenamed,
    handleHistoryRestored,
    handleBatchMetadataCompleted,
    handleTxtConvertedToEpub,
    handleBookNormalizeCompleted,
  }
}

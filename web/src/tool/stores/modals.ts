import { defineStore } from 'pinia'
import { computed, ref } from 'vue'
import { useDialogStore } from '@/stores/dialog'
import { useSnackbarStore } from '@/stores/snackbar'
import { toolApi } from '../api'
import { createModalCompletionHandlers } from '../services/modalEffects'
import type { AutoScrapeOptionsDto, MediaFileEntry } from '../types'
import { useToolDirectoryStore } from './directory'
import { useToolInspectorStore } from './inspector'
import { useToolNavigationStore } from './navigation'
import { useToolSelectionStore } from './selection'

export type ComicSeriesModalTab = 'series' | 'merge' | 'optimize'

export const useToolModalsStore = defineStore('toolModals', () => {
  const directoryStore = useToolDirectoryStore()
  const selectionStore = useToolSelectionStore()
  const inspectorStore = useToolInspectorStore()
  const navigationStore = useToolNavigationStore()
  const dialogStore = useDialogStore()
  const snackbar = useSnackbarStore()

  const tasksDrawerOpen = ref(false)
  const renameModalOpen = ref(false)
  const renameTargetEntries = ref<MediaFileEntry[] | null>(null)
  const renameSkippedOrganizedCount = ref(0)
  const batchMetadataModalOpen = ref(false)
  const historyModalOpen = ref(false)
  const historyInitialTab = ref<'metadata' | 'rename'>('metadata')
  const bookEditorOpen = ref(false)
  const bookEditorEntry = ref<MediaFileEntry | null>(null)
  const bookNormalizeOpen = ref(false)
  const bookNormalizeEntries = ref<MediaFileEntry[]>([])
  const comicPageManagerOpen = ref(false)
  const comicPageManagerEntry = ref<MediaFileEntry | null>(null)
  const comicSeriesToolsOpen = ref(false)
  const comicSeriesToolsEntries = ref<MediaFileEntry[] | null>(null)
  const comicSeriesInitialTab = ref<ComicSeriesModalTab>('series')
  const musicToolsOpen = ref(false)
  const musicToolsEntries = ref<MediaFileEntry[]>([])
  const videoToolsOpen = ref(false)
  const videoToolsEntries = ref<MediaFileEntry[]>([])
  const dryRunModalOpen = ref(false)
  const dryRunTargetEntries = ref<MediaFileEntry[] | null>(null)
  const dryRunScrapeOptions = ref<AutoScrapeOptionsDto | undefined>(undefined)
  const autoScrapeModalOpen = ref(false)
  const autoScrapeTargetEntries = ref<MediaFileEntry[]>([])

  const effectiveModalRootId = computed<string>(
    () =>
      directoryStore.resolveModalRootId(
        renameTargetEntries.value ?? selectionStore.selectedEntriesForRename,
      ) || directoryStore.fallbackRootId,
  )
  const effectiveBookNormalizeRootId = computed<string>(
    () =>
      directoryStore.resolveModalRootId(bookNormalizeEntries.value) ||
      directoryStore.fallbackRootId,
  )
  const effectiveMusicRootId = computed<string>(
    () =>
      directoryStore.resolveModalRootId(musicToolsEntries.value) || directoryStore.fallbackRootId,
  )
  const effectiveVideoRootId = computed<string>(
    () =>
      directoryStore.resolveModalRootId(videoToolsEntries.value) || directoryStore.fallbackRootId,
  )
  const effectiveComicEntries = computed<MediaFileEntry[]>(
    () => comicSeriesToolsEntries.value ?? selectionStore.selectedComicEntries,
  )
  const effectiveComicRootId = computed<string>(
    () =>
      directoryStore.resolveModalRootId(effectiveComicEntries.value) ||
      directoryStore.fallbackRootId,
  )
  const effectiveDryRunRootId = computed<string>(
    () =>
      directoryStore.resolveModalRootId(
        dryRunTargetEntries.value ?? selectionStore.selectedEntriesForRename,
      ) || directoryStore.fallbackRootId,
  )
  const effectiveHistoryRootId = computed<string>(
    () =>
      directoryStore.activeRootId ||
      inspectorStore.activeDetail?.entry.root_id ||
      directoryStore.fallbackRootId,
  )

  const anyModalOpen = computed(
    () =>
      tasksDrawerOpen.value ||
      Boolean(dialogStore.active) ||
      renameModalOpen.value ||
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

  async function requestDeleteFiles(paths: string[], deleteCompanions: boolean): Promise<void> {
    const unique = Array.from(new Set(paths.map((p) => p.trim()).filter((p) => p.length > 0)))
    if (unique.length === 0) return

    const folderCount = unique.filter((p) => directoryStore.currentFolderMap.has(p)).length
    const title = deleteCompanions
      ? '确认删除媒体条目与同名外挂？'
      : folderCount > 0
        ? folderCount === unique.length
          ? '确认递归删除所选文件夹？'
          : '确认删除所选文件夹与文件？'
        : '确认删除原始文件？'

    const modeHint = deleteCompanions
      ? '将同步删除主文件及其专属同名外挂（.opf / .nfo / .lrc / .srt / .ass / 同名封面）。'
      : folderCount > 0
        ? `包含 ${folderCount} 个文件夹，将递归删除目录及其内部全部文件。`
        : '仅删除所选原始文件本身，不会牵连删除其他外挂文件。'

    const previewList = unique
      .slice(0, 6)
      .map((p) => `• ${directoryStore.parseSelectionKey(p).relPath}`)
      .join('\n')
    const moreHint = unique.length > 6 ? `\n……等共 ${unique.length} 项` : ''

    const confirmed = await dialogStore.confirm({
      title,
      message: `${modeHint}\n此操作不可撤销：\n${previewList}${moreHint}`,
      confirmText: `删除 (${unique.length})`,
      cancelText: '取消',
      danger: true,
    })
    if (!confirmed) return

    try {
      const grouped = new Map<string, string[]>()
      for (const rawKey of unique) {
        const { rootId, relPath } = directoryStore.parseSelectionKey(rawKey)
        if (!rootId || !relPath) continue
        const list = grouped.get(rootId) ?? []
        list.push(relPath)
        grouped.set(rootId, list)
      }

      let totalDeleted = 0
      let totalCompanions = 0
      const allFailed: Array<{ rel_path: string; error: string }> = []
      const removedKeys = new Set<string>()
      const removedDirKeys: Array<{ rootId: string; prefix: string }> = []

      for (const [rid, relPaths] of grouped.entries()) {
        const report = await toolApi.deleteFiles(rid, relPaths, deleteCompanions)
        totalDeleted += report.deleted_count
        totalCompanions += report.deleted_companions.length
        allFailed.push(...report.failed)
        for (const p of [...report.deleted_paths, ...report.deleted_companions]) {
          removedKeys.add(p)
          removedKeys.add(`${rid}::${p}`)
        }
        for (const p of report.deleted_paths) {
          if (directoryStore.currentFolderMap.has(p)) {
            removedDirKeys.push({ rootId: rid, prefix: `${p}/` })
          }
        }
      }

      const isKeyRemoved = (k: string): boolean => {
        if (removedKeys.has(k)) return true
        const { rootId, relPath } = directoryStore.parseSelectionKey(k)
        return removedDirKeys.some((d) => d.rootId === rootId && relPath.startsWith(d.prefix))
      }

      if (selectionStore.selectedPaths.size > 0) {
        selectionStore.selectedPaths = new Set(
          [...selectionStore.selectedPaths].filter((p) => !isKeyRemoved(p)),
        )
      }
      if (
        inspectorStore.activeDetail &&
        isKeyRemoved(directoryStore.entrySelectionKey(inspectorStore.activeDetail.entry))
      ) {
        navigationStore.closeInspectorAndSync()
      }
      if (allFailed.length > 0 && totalDeleted === 0) {
        snackbar.show({
          message: allFailed.map((f) => `${f.rel_path}: ${f.error}`).join('；'),
          tone: 'error',
        })
        return
      }
      await directoryStore.reloadCurrentScope()
      const compHint = totalCompanions > 0 ? `（同步清理 ${totalCompanions} 个同名外挂）` : ''
      const failHint = allFailed.length > 0 ? `，${allFailed.length} 项失败` : ''
      const unit = removedDirKeys.length > 0 ? '项' : '个文件'
      snackbar.show(`已删除 ${totalDeleted} ${unit}${compHint}${failHint}`)
    } catch (err) {
      snackbar.show({
        message: err instanceof Error ? err.message : String(err),
        tone: 'error',
      })
    }
  }

  function openHistoryModal(tab: 'metadata' | 'rename' = 'metadata'): void {
    historyInitialTab.value = tab
    historyModalOpen.value = true
  }

  function openRenameModal(entries?: MediaFileEntry[]): void {
    const targets =
      entries && entries.length > 0 ? entries : selectionStore.selectedEntriesForRename
    const pending = targets.filter((e) => !e.completeness.is_organized)
    const skippedCount = targets.length - pending.length
    if (targets.length > 0 && pending.length === 0) {
      snackbar.show({ message: '所选条目均已按默认模板整理归档，无需重复整理' })
      return
    }
    if (pending.length > 0 && !directoryStore.resolveModalRootId(pending)) {
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
      selectionStore.selectedEntriesForRename.length > 0 &&
      !directoryStore.resolveModalRootId(selectionStore.selectedEntriesForRename)
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
      selectionStore.selectedEntriesForRename.length > 0 &&
      !directoryStore.resolveModalRootId(selectionStore.selectedEntriesForRename)
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

  function openComicSeriesToolsModal(
    entries?: MediaFileEntry[],
    initialTab: ComicSeriesModalTab = 'series',
  ): void {
    const targets =
      entries && entries.length > 0 ? entries : selectionStore.selectedComicEntries
    if (targets.length > 0 && !directoryStore.resolveModalRootId(targets)) {
      snackbar.show({
        message: '所选漫画跨越多个挂载目录，请先从左侧选择单个挂载目录再使用漫画工具',
        tone: 'error',
      })
      return
    }
    comicSeriesToolsEntries.value = entries && entries.length > 0 ? entries : null
    comicSeriesInitialTab.value = initialTab
    comicSeriesToolsOpen.value = true
  }

  function closeComicSeriesToolsModal(): void {
    comicSeriesToolsOpen.value = false
    comicSeriesToolsEntries.value = null
    comicSeriesInitialTab.value = 'series'
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
    if (entries.length > 0 && !directoryStore.resolveModalRootId(entries)) {
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
    if (entries.length > 0 && !directoryStore.resolveModalRootId(entries)) {
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
    if (!directoryStore.resolveModalRootId(valid)) {
      snackbar.show({
        message: '所选图书跨越多个挂载目录，请先从左侧选择单个挂载目录再转为规范化 EPUB',
        tone: 'error',
      })
      return
    }
    bookNormalizeEntries.value = valid
    bookNormalizeOpen.value = true
  }

  const completionHandlers = createModalCompletionHandlers({
    directoryStore,
    selectionStore,
    inspectorStore,
    navigationStore,
    getEffectiveModalRootId: () => effectiveModalRootId.value,
    getEffectiveBookNormalizeRootId: () => effectiveBookNormalizeRootId.value,
    resetRenameTargets: () => {
      renameTargetEntries.value = null
      renameSkippedOrganizedCount.value = 0
    },
    setBookEditorEntry: (entry) => {
      bookEditorEntry.value = entry
    },
    openHistoryModal,
  })

  return {
    tasksDrawerOpen,
    renameModalOpen,
    renameTargetEntries,
    renameSkippedOrganizedCount,
    batchMetadataModalOpen,
    historyModalOpen,
    historyInitialTab,
    bookEditorOpen,
    bookEditorEntry,
    bookNormalizeOpen,
    bookNormalizeEntries,
    comicPageManagerOpen,
    comicPageManagerEntry,
    comicSeriesToolsOpen,
    comicSeriesToolsEntries,
    comicSeriesInitialTab,
    musicToolsOpen,
    musicToolsEntries,
    videoToolsOpen,
    videoToolsEntries,
    dryRunModalOpen,
    dryRunTargetEntries,
    dryRunScrapeOptions,
    autoScrapeModalOpen,
    autoScrapeTargetEntries,
    effectiveModalRootId,
    effectiveBookNormalizeRootId,
    effectiveMusicRootId,
    effectiveVideoRootId,
    effectiveComicEntries,
    effectiveComicRootId,
    effectiveDryRunRootId,
    effectiveHistoryRootId,
    anyModalOpen,
    requestDeleteFiles,
    openHistoryModal,
    openRenameModal,
    closeRenameModal,
    openBatchMetadataModal,
    openDryRunModal,
    closeDryRunModal,
    openComicSeriesToolsModal,
    closeComicSeriesToolsModal,
    openBookContentEditor,
    openComicPageManager,
    openMusicToolbox,
    openVideoToolbox,
    openBookNormalizeModal,
    ...completionHandlers,
  }
})

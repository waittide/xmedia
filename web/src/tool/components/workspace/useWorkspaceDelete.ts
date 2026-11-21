import { computed, ref, type ComputedRef, type Ref } from 'vue'
import { useSnackbarStore } from '@/stores/snackbar'
import { toolApi } from '../../api'
import type { MediaFileDetail, MediaFileEntry } from '../../types'

export interface UseWorkspaceDeleteOptions {
  currentFolderMap: ComputedRef<
    Map<string, { name: string; media_file_count: number; child_dir_count: number }>
  >
  selectedPaths: Ref<Set<string>>
  activeDetail: Ref<MediaFileDetail | null>
  parseSelectionKey: (key: string) => { rootId: string; relPath: string }
  entrySelectionKey: (entry: Pick<MediaFileEntry, 'root_id' | 'rel_path'>) => string
  clearActiveDetail: () => void
  reloadCurrentScope: () => Promise<void>
}

export function useWorkspaceDelete(opts: UseWorkspaceDeleteOptions) {
  const snackbar = useSnackbarStore()

  const deleteDialogOpen = ref(false)
  const deleteDialogPaths = ref<string[]>([])
  const deleteDialogCompanions = ref(false)
  const deletingFiles = ref(false)
  const deleteDialogError = ref<string | null>(null)

  const deleteDialogFolderCount = computed<number>(
    () => deleteDialogPaths.value.filter((p) => opts.currentFolderMap.value.has(p)).length,
  )

  const deleteDialogTitle = computed<string>(() => {
    if (deleteDialogCompanions.value) return '确认删除媒体条目与同名外挂？'
    if (deleteDialogFolderCount.value > 0) {
      return deleteDialogFolderCount.value === deleteDialogPaths.value.length
        ? '确认递归删除所选文件夹？'
        : '确认删除所选文件夹与文件？'
    }
    return '确认删除原始文件？'
  })

  const deleteDialogSubtitle = computed<string>(() => {
    if (deleteDialogCompanions.value) {
      return '主视图删除模式：同步删除主文件及其专属同名外挂（.opf / .nfo / .lrc / .srt / .ass / 同名封面）'
    }
    if (deleteDialogFolderCount.value > 0) {
      return `文件夹视图删除模式：包含 ${deleteDialogFolderCount.value} 个文件夹，将递归删除目录及其内部全部文件；单个文件仅删除本身`
    }
    return '文件视图删除模式：仅删除所选原始文件本身，不会牵连删除其他外挂文件'
  })

  function requestDeleteFiles(paths: string[], deleteCompanions: boolean): void {
    const unique = Array.from(new Set(paths.map((p) => p.trim()).filter((p) => p.length > 0)))
    if (unique.length === 0) return
    deleteDialogPaths.value = unique
    deleteDialogCompanions.value = deleteCompanions
    deleteDialogError.value = null
    deleteDialogOpen.value = true
  }

  async function confirmDeleteFiles(): Promise<void> {
    if (deleteDialogPaths.value.length === 0 || deletingFiles.value) return
    deletingFiles.value = true
    deleteDialogError.value = null
    try {
      const grouped = new Map<string, string[]>()
      for (const rawKey of deleteDialogPaths.value) {
        const { rootId, relPath } = opts.parseSelectionKey(rawKey)
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

      for (const [rid, paths] of grouped.entries()) {
        const report = await toolApi.deleteFiles(rid, paths, deleteDialogCompanions.value)
        totalDeleted += report.deleted_count
        totalCompanions += report.deleted_companions.length
        allFailed.push(...report.failed)
        for (const p of [...report.deleted_paths, ...report.deleted_companions]) {
          removedKeys.add(p)
          removedKeys.add(`${rid}::${p}`)
        }
        for (const p of report.deleted_paths) {
          if (opts.currentFolderMap.value.has(p)) {
            removedDirKeys.push({ rootId: rid, prefix: `${p}/` })
          }
        }
      }

      const isKeyRemoved = (k: string): boolean => {
        if (removedKeys.has(k)) return true
        const { rootId, relPath } = opts.parseSelectionKey(k)
        return removedDirKeys.some((d) => d.rootId === rootId && relPath.startsWith(d.prefix))
      }

      if (opts.selectedPaths.value.size > 0) {
        const nextSel = new Set<string>()
        for (const p of opts.selectedPaths.value) {
          if (!isKeyRemoved(p)) nextSel.add(p)
        }
        opts.selectedPaths.value = nextSel
      }
      if (
        opts.activeDetail.value &&
        isKeyRemoved(opts.entrySelectionKey(opts.activeDetail.value.entry))
      ) {
        opts.clearActiveDetail()
      }
      if (allFailed.length > 0 && totalDeleted === 0) {
        deleteDialogError.value = allFailed.map((f) => `${f.rel_path}: ${f.error}`).join('；')
        return
      }
      deleteDialogOpen.value = false
      await opts.reloadCurrentScope()
      const compHint =
        totalCompanions > 0 ? `（同步清理 ${totalCompanions} 个同名外挂）` : ''
      const failHint = allFailed.length > 0 ? `，${allFailed.length} 项失败` : ''
      const unit = removedDirKeys.length > 0 ? '项' : '个文件'
      snackbar.show(`已删除 ${totalDeleted} ${unit}${compHint}${failHint}`)
    } catch (err) {
      deleteDialogError.value = err instanceof Error ? err.message : String(err)
    } finally {
      deletingFiles.value = false
    }
  }

  return {
    deleteDialogOpen,
    deleteDialogPaths,
    deleteDialogCompanions,
    deletingFiles,
    deleteDialogError,
    deleteDialogFolderCount,
    deleteDialogTitle,
    deleteDialogSubtitle,
    requestDeleteFiles,
    confirmDeleteFiles,
  }
}

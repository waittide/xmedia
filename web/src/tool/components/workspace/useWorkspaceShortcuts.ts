import { onBeforeUnmount, onMounted, type ComputedRef, type Ref } from 'vue'
import { useToolTaskStore } from '../../tasks'
import type { MediaFileDetail, MediaFileEntry } from '../../types'
import { PAGE_CHUNK_SIZE, isTypingTarget, type PrimaryNavKey } from './hashState'

export interface UseWorkspaceShortcutsOptions {
  activeNav: Ref<PrimaryNavKey>
  scrapeWorkbenchActive: Ref<boolean>
  mobileMenuOpen: Ref<boolean>
  activeDetail: Ref<MediaFileDetail | null>
  selectedPaths: Ref<Set<string>>
  inspectingPath: Ref<string | null>
  visibleLimit: Ref<number>
  filteredFiles: ComputedRef<MediaFileEntry[]>
  anyModalOpen: ComputedRef<boolean>
  focusSearch: () => void
  requestCloseInspector: () => void
  isInspectorDirty: () => boolean
  isWorkbenchDirty: () => boolean
  entrySelectionKey: (entry: Pick<MediaFileEntry, 'root_id' | 'rel_path'>) => string
  inspectEntry: (entry: MediaFileEntry) => Promise<void>
}

export function useWorkspaceShortcuts(opts: UseWorkspaceShortcutsOptions) {
  const taskStore = useToolTaskStore()

  function handleKeydown(ev: KeyboardEvent): void {
    if (opts.anyModalOpen.value) return
    if (
      typeof document !== 'undefined' &&
      document.querySelector('[aria-modal="true"], [role="menu"], [role="listbox"]')
    ) {
      return
    }
    if ((ev.metaKey || ev.ctrlKey) && ev.key.toLowerCase() === 'k') {
      ev.preventDefault()
      opts.focusSearch()
      return
    }
    if (ev.key === 'Escape') {
      if (opts.mobileMenuOpen.value) {
        ev.preventDefault()
        opts.mobileMenuOpen.value = false
      } else if (opts.activeDetail.value && !opts.scrapeWorkbenchActive.value) {
        ev.preventDefault()
        opts.requestCloseInspector()
      } else if (opts.selectedPaths.value.size > 0) {
        ev.preventDefault()
        opts.selectedPaths.value = new Set()
      }
      return
    }
    if (isTypingTarget(ev.target)) return
    if (
      opts.activeNav.value === 'items' &&
      !opts.scrapeWorkbenchActive.value &&
      (ev.key === 'ArrowDown' || ev.key === 'ArrowUp')
    ) {
      const list = opts.filteredFiles.value
      if (list.length === 0) return
      ev.preventDefault()
      const curIdx = opts.inspectingPath.value
        ? list.findIndex((f) => opts.entrySelectionKey(f) === opts.inspectingPath.value)
        : -1
      const nextIdx =
        ev.key === 'ArrowDown'
          ? Math.min(list.length - 1, curIdx + 1)
          : Math.max(0, curIdx <= 0 ? 0 : curIdx - 1)
      if (nextIdx >= opts.visibleLimit.value) {
        opts.visibleLimit.value = nextIdx + PAGE_CHUNK_SIZE
      }
      const target = list[nextIdx]
      if (target) {
        void opts.inspectEntry(target)
      }
    }
  }

  function handleBeforeUnload(ev: BeforeUnloadEvent): void {
    if (taskStore.activeCount > 0 || opts.isInspectorDirty() || opts.isWorkbenchDirty()) {
      ev.preventDefault()
      ev.returnValue = ''
    }
  }

  onMounted(() => {
    window.addEventListener('keydown', handleKeydown)
    window.addEventListener('beforeunload', handleBeforeUnload)
  })

  onBeforeUnmount(() => {
    window.removeEventListener('keydown', handleKeydown)
    window.removeEventListener('beforeunload', handleBeforeUnload)
  })
}

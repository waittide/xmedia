import { onBeforeUnmount, onMounted } from 'vue'
import { useToolInspectorStore } from '../../stores/inspector'
import { useToolScrapeStore } from '../../stores/scrape'
import { useToolWorkspaceStore } from '../../stores/workspace'
import { useToolTaskStore } from '../../tasks'
import { PAGE_CHUNK_SIZE, isTypingTarget } from './hashState'

export interface UseWorkspaceShortcutsOptions {
  focusSearch: () => void
}

export function useWorkspaceShortcuts(opts: UseWorkspaceShortcutsOptions) {
  const workspaceStore = useToolWorkspaceStore()
  const inspectorStore = useToolInspectorStore()
  const scrapeStore = useToolScrapeStore()
  const taskStore = useToolTaskStore()

  function handleKeydown(ev: KeyboardEvent): void {
    if (workspaceStore.anyModalOpen) return
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
      if (workspaceStore.mobileMenuOpen) {
        ev.preventDefault()
        workspaceStore.mobileMenuOpen = false
      } else if (inspectorStore.activeDetail && !scrapeStore.scrapeWorkbenchActive) {
        ev.preventDefault()
        void inspectorStore.requestCloseInspector()
      } else if (workspaceStore.selectedPaths.size > 0) {
        ev.preventDefault()
        workspaceStore.clearSelection()
      }
      return
    }
    if (isTypingTarget(ev.target)) return
    if (
      workspaceStore.activeNav === 'items' &&
      !scrapeStore.scrapeWorkbenchActive &&
      (ev.key === 'ArrowDown' || ev.key === 'ArrowUp')
    ) {
      const list = workspaceStore.filteredFiles
      if (list.length === 0) return
      ev.preventDefault()
      const curIdx = inspectorStore.inspectingPath
        ? list.findIndex((f) => workspaceStore.entrySelectionKey(f) === inspectorStore.inspectingPath)
        : -1
      const nextIdx =
        ev.key === 'ArrowDown'
          ? Math.min(list.length - 1, curIdx + 1)
          : Math.max(0, curIdx <= 0 ? 0 : curIdx - 1)
      if (nextIdx >= workspaceStore.visibleLimit) {
        workspaceStore.visibleLimit = nextIdx + PAGE_CHUNK_SIZE
      }
      const target = list[nextIdx]
      if (target) {
        void workspaceStore.inspectEntry(target)
      }
    }
  }

  function handleBeforeUnload(ev: BeforeUnloadEvent): void {
    if (
      taskStore.activeCount > 0 ||
      inspectorStore.isInspectorDirty() ||
      inspectorStore.isWorkbenchDirty()
    ) {
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

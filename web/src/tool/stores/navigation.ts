import { defineStore } from 'pinia'
import type { MediaKind } from '@/types/api'
import { toolApi } from '../api'
import type { ToolSettingsTab } from '../components/ToolSettingsView.vue'
import {
  VALID_DOMAINS,
  buildToolHash,
  parseToolHash,
  type BrowseViewMode,
} from '../components/workspace/hashState'
import type { MediaFileEntry } from '../types'
import { useToolDirectoryStore } from './directory'
import { useToolInspectorStore } from './inspector'
import { useToolScrapeStore } from './scrape'
import { useToolSelectionStore } from './selection'

export const useToolNavigationStore = defineStore('toolNavigation', () => {
  const directoryStore = useToolDirectoryStore()
  const selectionStore = useToolSelectionStore()
  const inspectorStore = useToolInspectorStore()

  let suppressHashSync = false

  function syncHashFromState(replace = false): void {
    if (suppressHashSync || typeof window === 'undefined') return
    const scrapeStore = useToolScrapeStore()
    const nextHash = buildToolHash({
      domain: directoryStore.activeDomain,
      nav: directoryStore.activeNav,
      rootId: directoryStore.activeRootId,
      relPath: directoryStore.currentRelPath,
      settingsTab: directoryStore.settingsInitialTab,
      workbench: scrapeStore.scrapeWorkbenchActive,
      item: inspectorStore.inspectingPath,
    })
    if (window.location.hash !== nextHash) {
      if (replace) window.history.replaceState(null, '', nextHash)
      else window.history.pushState(null, '', nextHash)
    }
  }

  directoryStore.registerRouteSync(syncHashFromState)

  async function inspectEntry(
    entry: MediaFileEntry,
    options?: { skipDirtyCheck?: boolean; replaceHash?: boolean },
  ): Promise<void> {
    const key = directoryStore.entrySelectionKey(entry)
    if (
      inspectorStore.activeDetail?.entry.root_id === entry.root_id &&
      inspectorStore.activeDetail?.entry.rel_path === entry.rel_path
    ) {
      return
    }
    if (!options?.skipDirtyCheck && !(await inspectorStore.confirmActiveEditorClean())) return
    const seq = inspectorStore.nextInspectSeq()
    inspectorStore.activeDetail = null
    inspectorStore.inspectingPath = key
    inspectorStore.loadingInspector = true
    syncHashFromState(options?.replaceHash ?? true)
    try {
      const detail = await toolApi.inspectFile(
        entry.root_id,
        entry.rel_path,
        directoryStore.activeDomain,
      )
      if (seq !== inspectorStore.currentInspectSeq()) return
      inspectorStore.activeDetail = detail
    } catch (err) {
      if (seq === inspectorStore.currentInspectSeq()) {
        inspectorStore.inspectingPath = null
        directoryStore.dirError = err instanceof Error ? err.message : String(err)
        syncHashFromState(true)
      }
    } finally {
      if (seq === inspectorStore.currentInspectSeq()) inspectorStore.loadingInspector = false
    }
  }

  function closeInspectorAndSync(): void {
    inspectorStore.clearActiveDetail()
    syncHashFromState(true)
  }

  function toggleSidebar(isDesktop?: boolean): void {
    const desktop =
      isDesktop ??
      (typeof window !== 'undefined' ? window.matchMedia('(min-width: 600px)').matches : true)
    if (!desktop) {
      directoryStore.mobileMenuOpen = !directoryStore.mobileMenuOpen
      return
    }
    directoryStore.collapsed = !directoryStore.collapsed
    localStorage.setItem('xmedia_tool.sidebar_collapsed', directoryStore.collapsed ? '1' : '0')
  }

  async function navigatePrimary(nav: 'home' | 'items', keepFilter = false): Promise<void> {
    const scrapeStore = useToolScrapeStore()
    if (
      (directoryStore.activeNav !== nav || scrapeStore.scrapeWorkbenchActive) &&
      (nav === 'home' || directoryStore.activeNav === 'settings') &&
      !(await inspectorStore.confirmActiveEditorClean())
    ) {
      return
    }
    if (nav !== 'items') {
      scrapeStore.scrapeWorkbenchActive = false
      scrapeStore.scrapeWorkbenchPaths = null
    }
    const wasScopedToSingleRoot =
      Boolean(directoryStore.activeRootId) || Boolean(directoryStore.currentRelPath)
    directoryStore.activeNav = nav
    directoryStore.previousWorkspaceNav = nav
    localStorage.setItem('xmedia_tool.active_nav', nav)
    directoryStore.mobileMenuOpen = false
    if (!keepFilter && nav === 'items') {
      selectionStore.completenessFilter = 'all'
      selectionStore.workbenchFocusPaths = null
    }
    if (directoryStore.activeRootId || directoryStore.currentRelPath || !directoryStore.snapshot) {
      directoryStore.activeRootId = ''
      directoryStore.currentRelPath = ''
      selectionStore.clearSelection()
      if (wasScopedToSingleRoot || !directoryStore.snapshot) {
        inspectorStore.clearActiveDetail()
        syncHashFromState()
        await directoryStore.loadAllDomainRoots()
        return
      }
    }
    syncHashFromState()
  }

  async function switchDomain(next: MediaKind): Promise<void> {
    if (directoryStore.activeDomain === next) {
      directoryStore.mobileMenuOpen = false
      return
    }
    if (!(await inspectorStore.confirmActiveEditorClean())) return
    const scrapeStore = useToolScrapeStore()
    scrapeStore.scrapeWorkbenchActive = false
    scrapeStore.scrapeWorkbenchPaths = null
    directoryStore.activeDomain = next
    directoryStore.mobileMenuOpen = false
    localStorage.setItem('xmedia_tool.active_domain', next)
    await directoryStore.activateDomainWorkspace(next)
    syncHashFromState()
  }

  async function selectRoot(rootId: string, preferredViewMode?: BrowseViewMode): Promise<void> {
    if (!(await inspectorStore.confirmActiveEditorClean())) return
    const scrapeStore = useToolScrapeStore()
    scrapeStore.scrapeWorkbenchActive = false
    scrapeStore.scrapeWorkbenchPaths = null
    directoryStore.mobileMenuOpen = false
    selectionStore.completenessFilter = 'all'
    selectionStore.workbenchFocusPaths = null
    if (preferredViewMode) selectionStore.setViewMode(preferredViewMode)
    directoryStore.activeNav = 'items'
    directoryStore.previousWorkspaceNav = 'items'
    localStorage.setItem('xmedia_tool.active_nav', 'items')

    const alreadyLoaded =
      directoryStore.activeRootId === rootId &&
      directoryStore.currentRelPath === '' &&
      directoryStore.snapshot?.root_id === rootId
    directoryStore.activeRootId = rootId
    directoryStore.currentRelPath = ''
    selectionStore.clearSelection()
    if (alreadyLoaded) {
      syncHashFromState()
      return
    }
    inspectorStore.clearActiveDetail()
    syncHashFromState()
    await directoryStore.loadDirectory(rootId, '')
  }

  async function enterFolder(relPath: string, forceFoldersView = true): Promise<void> {
    if (!(await inspectorStore.confirmActiveEditorClean())) return
    const scrapeStore = useToolScrapeStore()
    scrapeStore.scrapeWorkbenchActive = false
    scrapeStore.scrapeWorkbenchPaths = null
    directoryStore.mobileMenuOpen = false
    selectionStore.workbenchFocusPaths = null
    if (forceFoldersView) {
      selectionStore.setViewMode('folders')
      selectionStore.completenessFilter = 'all'
    }
    directoryStore.activeNav = 'items'
    directoryStore.previousWorkspaceNav = 'items'
    localStorage.setItem('xmedia_tool.active_nav', 'items')
    const targetRootId = directoryStore.fallbackRootId
    if (!targetRootId) return
    if (!directoryStore.activeRootId) directoryStore.activeRootId = targetRootId
    directoryStore.currentRelPath = relPath
    syncHashFromState()
    await directoryStore.loadDirectory(targetRootId, relPath)
  }

  async function openSettingsTab(tab: ToolSettingsTab): Promise<void> {
    if (
      directoryStore.activeNav !== 'settings' &&
      !(await inspectorStore.confirmActiveEditorClean())
    ) {
      return
    }
    const scrapeStore = useToolScrapeStore()
    scrapeStore.scrapeWorkbenchActive = false
    scrapeStore.scrapeWorkbenchPaths = null
    directoryStore.settingsInitialTab = tab
    if (directoryStore.activeNav !== 'settings') {
      directoryStore.previousWorkspaceNav = directoryStore.activeNav
    }
    directoryStore.activeNav = 'settings'
    directoryStore.mobileMenuOpen = false
    syncHashFromState()
  }

  function exitSettings(): void {
    directoryStore.activeNav = directoryStore.previousWorkspaceNav
    syncHashFromState()
  }

  async function loadInitial(): Promise<void> {
    directoryStore.loadingInitial = true
    directoryStore.initialLoadError = null
    const bootHash = parseToolHash()
    const scrapeStore = useToolScrapeStore()
    try {
      const s = await toolApi.getSettings()
      directoryStore.settings = s
      if (bootHash.rootId) {
        const matched = s.roots.find((r) => r.id === bootHash.rootId)
        if (matched?.default_kind) {
          directoryStore.activeDomain = matched.default_kind
          directoryStore.syncDomainStateToMemory(
            matched.default_kind,
            matched.id,
            bootHash.relPath ?? '',
          )
        }
      }
      if (!s.roots.some((r) => r.default_kind === directoryStore.activeDomain)) {
        const firstWithRoot = VALID_DOMAINS.find((d) => s.roots.some((r) => r.default_kind === d))
        if (firstWithRoot) {
          directoryStore.activeDomain = firstWithRoot
          localStorage.setItem('xmedia_tool.active_domain', firstWithRoot)
        }
      }
      suppressHashSync = true
      try {
        await directoryStore.activateDomainWorkspace(directoryStore.activeDomain)
        if (bootHash.workbench) {
          directoryStore.activeNav = 'items'
          directoryStore.previousWorkspaceNav = 'items'
          scrapeStore.scrapeWorkbenchActive = true
        }
        const targetEntry = bootHash.item
          ? directoryStore.findEntryByHashItem(bootHash.item)
          : bootHash.workbench
            ? selectionStore.filteredFiles[0]
            : undefined
        if (targetEntry) {
          await inspectEntry(targetEntry, { skipDirtyCheck: true, replaceHash: true })
        }
      } finally {
        suppressHashSync = false
      }
      syncHashFromState(true)
      directoryStore.initialLoaded = true
    } catch (err) {
      directoryStore.initialLoadError = err instanceof Error ? err.message : String(err)
    } finally {
      directoryStore.loadingInitial = false
    }
  }

  async function handleHashChange(): Promise<void> {
    const parsed = parseToolHash()
    if (!parsed.domain && !parsed.nav) return
    if (!(await inspectorStore.confirmActiveEditorClean())) {
      syncHashFromState(true)
      return
    }
    const scrapeStore = useToolScrapeStore()
    suppressHashSync = true
    try {
      const domainChanged = Boolean(parsed.domain && parsed.domain !== directoryStore.activeDomain)
      if (parsed.domain) {
        directoryStore.activeDomain = parsed.domain
        localStorage.setItem('xmedia_tool.active_domain', parsed.domain)
      }
      if (parsed.nav) {
        directoryStore.activeNav = parsed.nav
        if (parsed.nav !== 'settings') {
          directoryStore.previousWorkspaceNav = parsed.nav
          localStorage.setItem('xmedia_tool.active_nav', parsed.nav)
        } else if (parsed.settingsTab) {
          directoryStore.settingsInitialTab = parsed.settingsTab
        }
      }
      if (domainChanged) {
        if (parsed.rootId) {
          directoryStore.syncDomainStateToMemory(
            directoryStore.activeDomain,
            parsed.rootId,
            parsed.relPath ?? '',
          )
        }
        await directoryStore.activateDomainWorkspace(directoryStore.activeDomain)
      } else if (
        (parsed.rootId ?? '') !== directoryStore.activeRootId ||
        (parsed.relPath ?? '') !== directoryStore.currentRelPath
      ) {
        directoryStore.activeRootId = parsed.rootId ?? ''
        directoryStore.currentRelPath = parsed.relPath ?? ''
        await directoryStore.reloadCurrentScope()
      }
      const nextWorkbench = Boolean(parsed.workbench && directoryStore.activeNav === 'items')
      scrapeStore.scrapeWorkbenchActive = nextWorkbench
      if (!nextWorkbench) scrapeStore.scrapeWorkbenchPaths = null
      if (parsed.item) {
        const hit = directoryStore.findEntryByHashItem(parsed.item)
        if (hit && directoryStore.entrySelectionKey(hit) !== inspectorStore.inspectingPath) {
          await inspectEntry(hit, { skipDirtyCheck: true, replaceHash: true })
        }
      } else if (nextWorkbench && !inspectorStore.activeDetail && selectionStore.filteredFiles[0]) {
        await inspectEntry(selectionStore.filteredFiles[0], {
          skipDirtyCheck: true,
          replaceHash: true,
        })
      } else if (!parsed.item && !nextWorkbench && inspectorStore.activeDetail) {
        inspectorStore.clearActiveDetail()
      }
    } finally {
      suppressHashSync = false
    }
  }

  return {
    syncHashFromState,
    inspectEntry,
    closeInspectorAndSync,
    toggleSidebar,
    navigatePrimary,
    switchDomain,
    selectRoot,
    enterFolder,
    openSettingsTab,
    exitSettings,
    loadInitial,
    handleHashChange,
  }
})

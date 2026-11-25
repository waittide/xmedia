import { defineStore } from 'pinia'
import type { RouteLocationNormalizedLoaded } from 'vue-router'
import type { MediaKind } from '@/types/api'
import { toolApi } from '../api'
import {
  VALID_DOMAINS,
  buildToolRoute,
  parseToolRoute,
  toolRouter,
  type ToolSettingsTab,
} from '../router'
import type { MediaFileEntry } from '../types'
import { useToolDirectoryStore } from './directory'
import { useToolInspectorStore } from './inspector'
import { useToolScrapeStore } from './scrape'
import { useToolSelectionStore, type BrowseViewMode } from './selection'

let routerHooksInstalled = false

export const useToolNavigationStore = defineStore('toolNavigation', () => {
  const directoryStore = useToolDirectoryStore()
  const selectionStore = useToolSelectionStore()
  const inspectorStore = useToolInspectorStore()

  let suppressRouteSync = false
  let programmaticNavDepth = 0

  function isProgrammaticNav(): boolean {
    return programmaticNavDepth > 0 || suppressRouteSync
  }

  function syncRouteFromState(replace = false): void {
    if (suppressRouteSync) return
    const scrapeStore = useToolScrapeStore()
    const target = buildToolRoute({
      domain: directoryStore.activeDomain,
      nav: directoryStore.activeNav,
      workbench: scrapeStore.scrapeWorkbenchActive,
      rootId: directoryStore.activeRootId,
      relPath: directoryStore.currentRelPath,
      filter: selectionStore.completenessFilter,
      q: selectionStore.searchQuery,
      item: inspectorStore.inspectingPath,
      settingsTab: directoryStore.settingsInitialTab,
    })
    const resolved = toolRouter.resolve(target)
    if (resolved.fullPath === toolRouter.currentRoute.value.fullPath) return
    programmaticNavDepth += 1
    const task = replace ? toolRouter.replace(target) : toolRouter.push(target)
    void task.finally(() => {
      programmaticNavDepth = Math.max(0, programmaticNavDepth - 1)
    })
  }

  directoryStore.registerRouteSync(syncRouteFromState)

  if (!routerHooksInstalled) {
    routerHooksInstalled = true
    toolRouter.beforeEach(async (to, from) => {
      if (isProgrammaticNav() || !from.matched.length || to.fullPath === from.fullPath) {
        return true
      }
      if (!inspectorStore.isInspectorDirty() && !inspectorStore.isWorkbenchDirty()) {
        return true
      }
      const affectsActiveEditor =
        to.name !== from.name ||
        to.params.domain !== from.params.domain ||
        (to.query.root ?? '') !== (from.query.root ?? '') ||
        (to.query.path ?? '') !== (from.query.path ?? '') ||
        (to.query.item ?? '') !== (from.query.item ?? '')
      if (!affectsActiveEditor) return true
      return inspectorStore.confirmActiveEditorClean()
    })

    toolRouter.afterEach((to, from) => {
      if (isProgrammaticNav() || !directoryStore.initialLoaded || to.fullPath === from.fullPath) {
        return
      }
      void applyRouteLocation(to)
    })
  }

  async function inspectEntry(
    entry: MediaFileEntry,
    options?: { skipDirtyCheck?: boolean; replaceRoute?: boolean },
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
    syncRouteFromState(options?.replaceRoute ?? false)
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
        syncRouteFromState(true)
      }
    } finally {
      if (seq === inspectorStore.currentInspectSeq()) inspectorStore.loadingInspector = false
    }
  }

  function closeInspectorAndSync(): void {
    inspectorStore.clearActiveDetail()
    syncRouteFromState(false)
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
        syncRouteFromState()
        await directoryStore.loadAllDomainRoots()
        return
      }
    }
    syncRouteFromState()
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
    syncRouteFromState()
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
      syncRouteFromState()
      return
    }
    inspectorStore.clearActiveDetail()
    syncRouteFromState()
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
    syncRouteFromState()
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
    syncRouteFromState()
  }

  function exitSettings(): void {
    directoryStore.activeNav = directoryStore.previousWorkspaceNav
    syncRouteFromState()
  }

  async function loadInitial(): Promise<void> {
    directoryStore.loadingInitial = true
    directoryStore.initialLoadError = null
    const scrapeStore = useToolScrapeStore()
    try {
      await toolRouter.isReady()
      const bootRoute = parseToolRoute(toolRouter.currentRoute.value)
      if (bootRoute.domain) {
        directoryStore.activeDomain = bootRoute.domain
        localStorage.setItem('xmedia_tool.active_domain', bootRoute.domain)
      }
      directoryStore.activeNav = bootRoute.nav
      if (bootRoute.nav === 'settings') {
        directoryStore.settingsInitialTab = bootRoute.settingsTab
      } else {
        directoryStore.previousWorkspaceNav = bootRoute.nav
        localStorage.setItem('xmedia_tool.active_nav', bootRoute.nav)
      }

      const s = await toolApi.getSettings()
      directoryStore.settings = s

      if (bootRoute.rootId) {
        const matched = s.roots.find((r) => r.id === bootRoute.rootId)
        if (matched?.default_kind) {
          directoryStore.activeDomain = matched.default_kind
          directoryStore.syncDomainStateToMemory(
            matched.default_kind,
            matched.id,
            bootRoute.relPath,
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

      suppressRouteSync = true
      try {
        selectionStore.completenessFilter = bootRoute.filter
        selectionStore.searchQuery = bootRoute.q
        if (bootRoute.filter !== 'all') selectionStore.setViewMode('flat')
        await directoryStore.activateDomainWorkspace(directoryStore.activeDomain, true)
        if (bootRoute.workbench) {
          directoryStore.activeNav = 'items'
          directoryStore.previousWorkspaceNav = 'items'
          scrapeStore.scrapeWorkbenchActive = true
        }
        const targetEntry = bootRoute.item
          ? directoryStore.findEntryByHashItem(bootRoute.item)
          : bootRoute.workbench
            ? selectionStore.filteredFiles[0]
            : undefined
        if (targetEntry) {
          await inspectEntry(targetEntry, { skipDirtyCheck: true, replaceRoute: true })
        }
      } finally {
        suppressRouteSync = false
      }
      syncRouteFromState(true)
      directoryStore.initialLoaded = true
    } catch (err) {
      directoryStore.initialLoadError = err instanceof Error ? err.message : String(err)
    } finally {
      directoryStore.loadingInitial = false
    }
  }

  async function applyRouteLocation(route: RouteLocationNormalizedLoaded): Promise<void> {
    const parsed = parseToolRoute(route)
    const scrapeStore = useToolScrapeStore()
    suppressRouteSync = true
    try {
      const domainChanged = Boolean(parsed.domain && parsed.domain !== directoryStore.activeDomain)
      if (parsed.domain) {
        directoryStore.activeDomain = parsed.domain
        localStorage.setItem('xmedia_tool.active_domain', parsed.domain)
      }
      directoryStore.activeNav = parsed.nav
      if (parsed.nav === 'settings') {
        directoryStore.settingsInitialTab = parsed.settingsTab
      } else {
        directoryStore.previousWorkspaceNav = parsed.nav
        localStorage.setItem('xmedia_tool.active_nav', parsed.nav)
      }

      if (parsed.nav === 'items' && !parsed.workbench) {
        selectionStore.completenessFilter = parsed.filter
        selectionStore.searchQuery = parsed.q
        if (parsed.filter !== 'all') selectionStore.setViewMode('flat')
      }

      if (domainChanged) {
        if (parsed.rootId) {
          directoryStore.syncDomainStateToMemory(
            directoryStore.activeDomain,
            parsed.rootId,
            parsed.relPath,
          )
        }
        await directoryStore.activateDomainWorkspace(directoryStore.activeDomain, true)
      } else if (
        parsed.nav !== 'settings' &&
        (parsed.rootId !== directoryStore.activeRootId ||
          parsed.relPath !== directoryStore.currentRelPath)
      ) {
        directoryStore.activeRootId = parsed.rootId
        directoryStore.currentRelPath = parsed.relPath
        await directoryStore.reloadCurrentScope()
      }

      scrapeStore.scrapeWorkbenchActive = parsed.workbench
      if (!parsed.workbench) scrapeStore.scrapeWorkbenchPaths = null

      if (parsed.item) {
        const hit = directoryStore.findEntryByHashItem(parsed.item)
        if (hit && directoryStore.entrySelectionKey(hit) !== inspectorStore.inspectingPath) {
          await inspectEntry(hit, { skipDirtyCheck: true, replaceRoute: true })
        }
      } else if (parsed.workbench && !inspectorStore.activeDetail && selectionStore.filteredFiles[0]) {
        await inspectEntry(selectionStore.filteredFiles[0], {
          skipDirtyCheck: true,
          replaceRoute: true,
        })
      } else if (!parsed.item && !parsed.workbench && inspectorStore.activeDetail) {
        inspectorStore.clearActiveDetail()
      }
    } finally {
      suppressRouteSync = false
    }
  }

  return {
    syncRouteFromState,
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
    applyRouteLocation,
  }
})

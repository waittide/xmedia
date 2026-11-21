import { ref, type ComputedRef, type Ref } from 'vue'
import type { MediaKind } from '@/types/api'
import { toolApi } from '../../api'
import { useToolTaskStore } from '../../tasks'
import type { DirectorySnapshot, ManagedRootDto, MediaFileDetail, MediaFileEntry, PublicSettingsDto } from '../../types'
import type { ToolSettingsTab } from '../ToolSettingsView.vue'
import {
  VALID_DOMAINS,
  parentRelOf,
  parseToolHash,
  type BrowseViewMode,
  type CompletenessFilter,
  type ParsedToolHash,
  type PrimaryNavKey,
} from './hashState'

export interface UseWorkspaceDirectoryOptions {
  initialHash: ParsedToolHash
  settings: Ref<PublicSettingsDto | null>
  activeDomain: Ref<MediaKind>
  activeNav: Ref<PrimaryNavKey>
  previousWorkspaceNav: Ref<'home' | 'items'>
  settingsInitialTab: Ref<ToolSettingsTab>
  domainRoots: ComputedRef<ManagedRootDto[]>
  activeRootId: Ref<string>
  currentRelPath: Ref<string>
  snapshot: Ref<DirectorySnapshot | null>
  rootFileCounts: Ref<Record<string, number>>
  loadingDir: Ref<boolean>
  dirError: Ref<string | null>
  selectedPaths: Ref<Set<string>>
  lastSelectedRelPath: Ref<string | null>
  inspectingPath: Ref<string | null>
  activeDetail: Ref<MediaFileDetail | null>
  loadingInspector: Ref<boolean>
  completenessFilter: Ref<CompletenessFilter>
  searchQuery: Ref<string>
  mobileMenuOpen: Ref<boolean>
  workbenchFocusPaths: Ref<Set<string> | null>
  scrapeWorkbenchActive: Ref<boolean>
  scrapeWorkbenchPaths: Ref<string[] | null>
  entrySelectionKey: (entry: Pick<MediaFileEntry, 'root_id' | 'rel_path'>) => string
  confirmActiveEditorClean: () => Promise<boolean>
  setViewMode: (next: BrowseViewMode) => void
}

export function useWorkspaceDirectory(opts: UseWorkspaceDirectoryOptions) {
  const taskStore = useToolTaskStore()

  const initialLoadError = ref<string | null>(null)
  const loadingInitial = ref(false)
  const domainNavMemory = ref<Record<MediaKind, { rootId: string; relPath: string }>>({
    book: { rootId: '', relPath: '' },
    comic: { rootId: '', relPath: '' },
    music: { rootId: '', relPath: '' },
    video: { rootId: '', relPath: '' },
  })

  let dirLoadSeq = 0
  let inspectSeq = 0
  let suppressHashSync = false
  let initialLoaded = false

  function isInitialLoaded(): boolean {
    return initialLoaded
  }

  function nextInspectSeq(): number {
    return ++inspectSeq
  }

  function currentInspectSeq(): number {
    return inspectSeq
  }

  function syncHashFromState(replace = false): void {
    if (suppressHashSync) return
    const params = new URLSearchParams()
    if (opts.activeNav.value === 'settings') {
      params.set('tab', opts.settingsInitialTab.value)
    } else {
      if (opts.activeRootId.value) params.set('root', opts.activeRootId.value)
      if (opts.currentRelPath.value) params.set('path', opts.currentRelPath.value)
    }
    const qs = params.toString()
    const nextHash = `#/${opts.activeDomain.value}/${opts.activeNav.value}${qs ? `?${qs}` : ''}`
    if (window.location.hash !== nextHash) {
      if (replace) {
        window.history.replaceState(null, '', nextHash)
      } else {
        window.history.pushState(null, '', nextHash)
      }
    }
  }

  function clearActiveDetail(): void {
    inspectSeq += 1
    opts.activeDetail.value = null
    opts.inspectingPath.value = null
    opts.loadingInspector.value = false
  }

  function syncDomainStateToMemory(dom: MediaKind, rootId: string, relPath: string): void {
    domainNavMemory.value = {
      ...domainNavMemory.value,
      [dom]: { rootId, relPath },
    }
  }

  async function loadAllDomainRoots(): Promise<void> {
    const reqDomain = opts.activeDomain.value
    const roots = (opts.settings.value?.roots ?? []).filter((r) => r.default_kind === reqDomain)
    if (roots.length === 0) {
      opts.snapshot.value = null
      opts.rootFileCounts.value = {}
      return
    }
    const seq = ++dirLoadSeq
    const preserveSelection = opts.activeRootId.value === '' && opts.currentRelPath.value === ''
    opts.loadingDir.value = true
    opts.dirError.value = null
    if (!preserveSelection) {
      opts.selectedPaths.value = new Set()
      opts.lastSelectedRelPath.value = null
    }

    const taskId = `browse:${reqDomain}`
    taskStore.startTask({
      id: taskId,
      kind: 'browse',
      domain: reqDomain,
      title: roots.length > 1 ? `汇总 ${roots.length} 个挂载目录` : `读取目录 · ${roots[0]!.name}`,
      detail: roots.map((r) => r.name).join(' / '),
      ephemeral: true,
    })

    try {
      if (roots.length === 1) {
        const only = roots[0]!
        const snap = await toolApi.browse(only.id, '', reqDomain, true, (prog) => {
          taskStore.updateTask(taskId, prog)
        })
        taskStore.completeTask(taskId, `已加载 ${snap.files.length.toLocaleString()} 个媒体文件`)
        if (seq !== dirLoadSeq || opts.activeDomain.value !== reqDomain) return
        opts.snapshot.value = snap
        opts.rootFileCounts.value = { [only.id]: snap.files.length }
        opts.currentRelPath.value = ''
        syncDomainStateToMemory(reqDomain, '', '')
        syncHashFromState(true)
        if (preserveSelection && opts.selectedPaths.value.size > 0) {
          const surviving = new Set<string>([
            ...snap.files.map((f) => f.rel_path),
            ...(snap.raw_files ?? []).map((f) => f.rel_path),
            ...(snap.folders ?? []).map((d) => d.rel_path),
          ])
          opts.selectedPaths.value = new Set(
            [...opts.selectedPaths.value].filter((p) => surviving.has(p)),
          )
        }
        if (
          opts.inspectingPath.value &&
          !snap.files.some((f) => f.rel_path === opts.inspectingPath.value)
        ) {
          opts.inspectingPath.value = null
          opts.activeDetail.value = null
        }
        return
      }

      const snaps = await Promise.all(
        roots.map((r) => toolApi.browse(r.id, '', reqDomain, true)),
      )
      const counts: Record<string, number> = {}
      const mergedFiles: MediaFileEntry[] = []
      for (let i = 0; i < roots.length; i += 1) {
        const r = roots[i]!
        const s = snaps[i]!
        counts[r.id] = s.files.length
        mergedFiles.push(...s.files)
      }
      taskStore.completeTask(taskId, `已汇总 ${mergedFiles.length.toLocaleString()} 个媒体文件`)
      if (seq !== dirLoadSeq || opts.activeDomain.value !== reqDomain) return

      opts.snapshot.value = {
        root_id: '',
        root_name: `全部挂载目录 (${roots.length})`,
        current_rel_path: '',
        parent_rel_path: null,
        folders: [],
        files: mergedFiles,
        raw_files: [],
      }
      opts.rootFileCounts.value = counts
      opts.currentRelPath.value = ''
      syncDomainStateToMemory(reqDomain, '', '')
      syncHashFromState(true)

      if (preserveSelection && opts.selectedPaths.value.size > 0) {
        const surviving = new Set<string>(mergedFiles.map((f) => `${f.root_id}::${f.rel_path}`))
        opts.selectedPaths.value = new Set(
          [...opts.selectedPaths.value].filter((p) => surviving.has(p)),
        )
      }
      if (
        opts.inspectingPath.value &&
        !mergedFiles.some((f) => `${f.root_id}::${f.rel_path}` === opts.inspectingPath.value)
      ) {
        opts.inspectingPath.value = null
        opts.activeDetail.value = null
      }
    } catch (err) {
      const msg = err instanceof Error ? err.message : String(err)
      taskStore.failTask(taskId, msg)
      if (seq === dirLoadSeq && opts.activeDomain.value === reqDomain) {
        opts.dirError.value = msg
      }
    } finally {
      if (seq === dirLoadSeq && opts.activeDomain.value === reqDomain) {
        opts.loadingDir.value = false
      }
    }
  }

  async function loadDirectory(rootId: string, relPath: string): Promise<void> {
    if (!rootId) return
    const seq = ++dirLoadSeq
    const reqDomain = opts.activeDomain.value
    const preserveSelection =
      rootId === opts.activeRootId.value && relPath === opts.currentRelPath.value
    opts.loadingDir.value = true
    opts.dirError.value = null
    if (!preserveSelection) {
      opts.selectedPaths.value = new Set()
      opts.lastSelectedRelPath.value = null
    }

    const rootLabel = opts.settings.value?.roots.find((r) => r.id === rootId)?.name || '媒体目录'
    const taskId = `browse:${reqDomain}`
    taskStore.startTask({
      id: taskId,
      kind: 'browse',
      domain: reqDomain,
      title: `读取目录 · ${relPath || rootLabel}`,
      detail: relPath || rootLabel,
      ephemeral: true,
    })

    let candidateRel = relPath
    let snap: DirectorySnapshot | null = null
    try {
      while (true) {
        try {
          snap = await toolApi.browse(rootId, candidateRel, reqDomain, true, (prog) => {
            taskStore.updateTask(taskId, prog)
          })
          break
        } catch (err) {
          const errStatus = (err as Error & { status?: number })?.status
          const isNotFound =
            errStatus === 404 ||
            (err instanceof Error && /not found|不存在|no such/i.test(err.message))
          if (isNotFound && candidateRel) {
            candidateRel = parentRelOf(candidateRel)
            continue
          }
          throw err
        }
      }
      if (!snap) {
        taskStore.completeTask(taskId, '已取消')
        return
      }
      taskStore.completeTask(taskId, `已加载 ${snap.files.length.toLocaleString()} 个媒体文件`)
      if (seq !== dirLoadSeq || opts.activeDomain.value !== reqDomain) return
      opts.snapshot.value = snap
      opts.currentRelPath.value = snap.current_rel_path
      syncDomainStateToMemory(reqDomain, rootId, snap.current_rel_path)
      syncHashFromState(true)

      if (preserveSelection && opts.selectedPaths.value.size > 0) {
        const surviving = new Set<string>([
          ...snap.files.map((f) => f.rel_path),
          ...(snap.raw_files ?? []).map((f) => f.rel_path),
          ...(snap.folders ?? []).map((d) => d.rel_path),
        ])
        const pruned = new Set<string>()
        for (const p of opts.selectedPaths.value) {
          if (surviving.has(p)) pruned.add(p)
        }
        opts.selectedPaths.value = pruned
      }

      if (
        opts.inspectingPath.value &&
        !snap.files.some((f) => f.rel_path === opts.inspectingPath.value)
      ) {
        opts.inspectingPath.value = null
        opts.activeDetail.value = null
      }
    } catch (err) {
      const msg = err instanceof Error ? err.message : String(err)
      taskStore.failTask(taskId, msg)
      if (seq === dirLoadSeq && opts.activeDomain.value === reqDomain) {
        opts.dirError.value = msg
      }
    } finally {
      if (seq === dirLoadSeq && opts.activeDomain.value === reqDomain) {
        opts.loadingDir.value = false
      }
    }
  }

  async function reloadCurrentScope(): Promise<void> {
    if (opts.activeRootId.value) {
      await loadDirectory(opts.activeRootId.value, opts.currentRelPath.value)
    } else {
      await loadAllDomainRoots()
    }
  }

  async function activateDomainWorkspace(dom: MediaKind): Promise<void> {
    const roots = (opts.settings.value?.roots ?? []).filter((r) => r.default_kind === dom)
    opts.selectedPaths.value = new Set()
    clearActiveDetail()
    opts.completenessFilter.value = 'all'
    opts.searchQuery.value = ''
    opts.dirError.value = null

    if (roots.length === 0) {
      opts.activeRootId.value = ''
      opts.currentRelPath.value = ''
      opts.snapshot.value = null
      opts.rootFileCounts.value = {}
      return
    }

    const mem = domainNavMemory.value[dom]
    if (mem.rootId && roots.some((r) => r.id === mem.rootId)) {
      opts.activeRootId.value = mem.rootId
      opts.currentRelPath.value = mem.relPath
      await loadDirectory(mem.rootId, mem.relPath)
      return
    }

    opts.activeRootId.value = ''
    opts.currentRelPath.value = ''
    await loadAllDomainRoots()
  }

  async function navigatePrimary(nav: 'home' | 'items', keepFilter = false): Promise<void> {
    if (
      (opts.activeNav.value !== nav || opts.scrapeWorkbenchActive.value) &&
      (nav === 'home' || opts.activeNav.value === 'settings') &&
      !(await opts.confirmActiveEditorClean())
    ) {
      return
    }
    if (nav !== 'items') {
      opts.scrapeWorkbenchActive.value = false
      opts.scrapeWorkbenchPaths.value = null
    }
    const wasScopedToSingleRoot =
      Boolean(opts.activeRootId.value) || Boolean(opts.currentRelPath.value)
    opts.activeNav.value = nav
    opts.previousWorkspaceNav.value = nav
    localStorage.setItem('xmedia_tool.active_nav', nav)
    opts.mobileMenuOpen.value = false
    if (!keepFilter && nav === 'items') {
      opts.completenessFilter.value = 'all'
      opts.workbenchFocusPaths.value = null
    }
    if (opts.activeRootId.value || opts.currentRelPath.value || !opts.snapshot.value) {
      opts.activeRootId.value = ''
      opts.currentRelPath.value = ''
      opts.selectedPaths.value = new Set()
      opts.lastSelectedRelPath.value = null
      if (wasScopedToSingleRoot || !opts.snapshot.value) {
        clearActiveDetail()
        syncHashFromState()
        await loadAllDomainRoots()
        return
      }
    }
    syncHashFromState()
  }

  async function switchDomain(next: MediaKind): Promise<void> {
    if (opts.activeDomain.value === next) {
      opts.mobileMenuOpen.value = false
      return
    }
    if (!(await opts.confirmActiveEditorClean())) {
      return
    }
    opts.scrapeWorkbenchActive.value = false
    opts.scrapeWorkbenchPaths.value = null
    opts.activeDomain.value = next
    opts.mobileMenuOpen.value = false
    localStorage.setItem('xmedia_tool.active_domain', next)
    await activateDomainWorkspace(next)
    syncHashFromState()
  }

  async function loadInitial(): Promise<void> {
    loadingInitial.value = true
    initialLoadError.value = null
    try {
      const s = await toolApi.getSettings()
      opts.settings.value = s

      if (opts.initialHash.rootId) {
        const matched = s.roots.find((r) => r.id === opts.initialHash.rootId)
        if (matched && matched.default_kind) {
          opts.activeDomain.value = matched.default_kind
          domainNavMemory.value = {
            ...domainNavMemory.value,
            [matched.default_kind]: {
              rootId: matched.id,
              relPath: opts.initialHash.relPath ?? '',
            },
          }
        }
      }

      const currentHasRoots = s.roots.some((r) => r.default_kind === opts.activeDomain.value)
      if (!currentHasRoots) {
        const firstWithRoot = VALID_DOMAINS.find((d) =>
          s.roots.some((r) => r.default_kind === d),
        )
        if (firstWithRoot) {
          opts.activeDomain.value = firstWithRoot
          localStorage.setItem('xmedia_tool.active_domain', firstWithRoot)
        }
      }
      await activateDomainWorkspace(opts.activeDomain.value)
      syncHashFromState(true)
      initialLoaded = true
    } catch (err) {
      initialLoadError.value = err instanceof Error ? err.message : String(err)
    } finally {
      loadingInitial.value = false
    }
  }

  async function selectRoot(
    rootId: string,
    preferredViewMode?: BrowseViewMode,
  ): Promise<void> {
    if (!(await opts.confirmActiveEditorClean())) {
      return
    }
    opts.scrapeWorkbenchActive.value = false
    opts.scrapeWorkbenchPaths.value = null
    opts.mobileMenuOpen.value = false
    opts.completenessFilter.value = 'all'
    opts.workbenchFocusPaths.value = null
    if (preferredViewMode) {
      opts.setViewMode(preferredViewMode)
    }
    opts.activeNav.value = 'items'
    opts.previousWorkspaceNav.value = 'items'
    localStorage.setItem('xmedia_tool.active_nav', 'items')

    const alreadyLoaded =
      opts.activeRootId.value === rootId &&
      opts.currentRelPath.value === '' &&
      opts.snapshot.value?.root_id === rootId
    opts.activeRootId.value = rootId
    opts.currentRelPath.value = ''
    opts.selectedPaths.value = new Set()
    opts.lastSelectedRelPath.value = null
    if (alreadyLoaded) {
      syncHashFromState()
      return
    }
    clearActiveDetail()
    syncHashFromState()
    await loadDirectory(rootId, '')
  }

  async function enterFolder(relPath: string, forceFoldersView = true): Promise<void> {
    if (!(await opts.confirmActiveEditorClean())) {
      return
    }
    opts.scrapeWorkbenchActive.value = false
    opts.scrapeWorkbenchPaths.value = null
    opts.mobileMenuOpen.value = false
    opts.workbenchFocusPaths.value = null
    if (forceFoldersView) {
      opts.setViewMode('folders')
      opts.completenessFilter.value = 'all'
    }
    opts.activeNav.value = 'items'
    opts.previousWorkspaceNav.value = 'items'
    localStorage.setItem('xmedia_tool.active_nav', 'items')

    const targetRootId =
      opts.activeRootId.value || opts.snapshot.value?.root_id || opts.domainRoots.value[0]?.id || ''
    if (!targetRootId) return
    if (!opts.activeRootId.value) {
      opts.activeRootId.value = targetRootId
    }
    opts.currentRelPath.value = relPath
    syncHashFromState()
    await loadDirectory(targetRootId, relPath)
  }

  async function inspectEntry(entry: MediaFileEntry): Promise<void> {
    const key = opts.entrySelectionKey(entry)
    if (
      opts.activeDetail.value?.entry.root_id === entry.root_id &&
      opts.activeDetail.value?.entry.rel_path === entry.rel_path
    ) {
      return
    }
    if (!(await opts.confirmActiveEditorClean())) {
      return
    }
    const seq = ++inspectSeq
    opts.inspectingPath.value = key
    opts.loadingInspector.value = true
    try {
      const detail = await toolApi.inspectFile(
        entry.root_id,
        entry.rel_path,
        opts.activeDomain.value,
      )
      if (seq !== inspectSeq) return
      opts.activeDetail.value = detail
    } catch (err) {
      if (seq === inspectSeq) {
        opts.dirError.value = err instanceof Error ? err.message : String(err)
      }
    } finally {
      if (seq === inspectSeq) {
        opts.loadingInspector.value = false
      }
    }
  }

  async function handleHashChange(): Promise<void> {
    const parsed = parseToolHash()
    if (!parsed.domain && !parsed.nav) return
    if (!(await opts.confirmActiveEditorClean())) {
      syncHashFromState(true)
      return
    }
    suppressHashSync = true
    try {
      opts.scrapeWorkbenchActive.value = false
      opts.scrapeWorkbenchPaths.value = null
      const domainChanged = Boolean(parsed.domain && parsed.domain !== opts.activeDomain.value)
      if (parsed.domain) {
        opts.activeDomain.value = parsed.domain
        localStorage.setItem('xmedia_tool.active_domain', parsed.domain)
      }
      if (parsed.nav) {
        opts.activeNav.value = parsed.nav
        if (parsed.nav !== 'settings') {
          opts.previousWorkspaceNav.value = parsed.nav
          localStorage.setItem('xmedia_tool.active_nav', parsed.nav)
        } else if (parsed.settingsTab) {
          opts.settingsInitialTab.value = parsed.settingsTab
        }
      }
      if (domainChanged) {
        if (parsed.rootId) {
          domainNavMemory.value = {
            ...domainNavMemory.value,
            [opts.activeDomain.value]: {
              rootId: parsed.rootId,
              relPath: parsed.relPath ?? '',
            },
          }
        }
        await activateDomainWorkspace(opts.activeDomain.value)
      } else if (
        (parsed.rootId ?? '') !== opts.activeRootId.value ||
        (parsed.relPath ?? '') !== opts.currentRelPath.value
      ) {
        opts.activeRootId.value = parsed.rootId ?? ''
        opts.currentRelPath.value = parsed.relPath ?? ''
        await reloadCurrentScope()
      }
    } finally {
      suppressHashSync = false
    }
  }

  return {
    initialLoadError,
    loadingInitial,
    isInitialLoaded,
    nextInspectSeq,
    currentInspectSeq,
    syncHashFromState,
    clearActiveDetail,
    loadInitial,
    loadAllDomainRoots,
    loadDirectory,
    reloadCurrentScope,
    activateDomainWorkspace,
    navigatePrimary,
    switchDomain,
    selectRoot,
    enterFolder,
    inspectEntry,
    handleHashChange,
  }
}

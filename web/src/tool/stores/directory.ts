import { defineStore } from 'pinia'
import { computed, ref, watch } from 'vue'
import { i18n } from '@/i18n'
import { useSnackbarStore } from '@/stores/snackbar'
import type { MediaKind } from '@/types/api'
import { toolApi } from '../api'
import { entryCompositeKey } from '../completeness'
import type { ToolSettingsTab } from '../components/ToolSettingsView.vue'
import {
  loadSavedDomain,
  loadSavedNav,
  parentRelOf,
  parseToolHash,
  type PrimaryNavKey,
} from '../components/workspace/hashState'
import { useToolTaskStore } from '../tasks'
import type {
  DirectorySnapshot,
  ManagedRootDto,
  MediaFileEntry,
  PublicSettingsDto,
  ToolStatusDto,
} from '../types'
import { useToolInspectorStore } from './inspector'
import { useToolSelectionStore } from './selection'

export const useToolDirectoryStore = defineStore('toolDirectory', () => {
  const inspectorStore = useToolInspectorStore()
  const taskStore = useToolTaskStore()
  const snackbar = useSnackbarStore()
  const t = (key: string, params?: Record<string, unknown>): string =>
    params ? String(i18n.global.t(key, params)) : String(i18n.global.t(key))

  const initialHash = parseToolHash()

  const activeDomain = ref<MediaKind>(loadSavedDomain(initialHash))
  const activeNav = ref<PrimaryNavKey>(loadSavedNav(initialHash))
  const previousWorkspaceNav = ref<'home' | 'items'>(
    activeNav.value === 'settings' ? 'home' : activeNav.value,
  )
  const settingsInitialTab = ref<ToolSettingsTab>(initialHash.settingsTab ?? 'appearance')
  const collapsed = ref(localStorage.getItem('xmedia_tool.sidebar_collapsed') === '1')
  const mobileMenuOpen = ref(false)
  const notifyingRescan = ref(false)
  const initialLoaded = ref(false)

  const settings = ref<PublicSettingsDto | null>(null)
  const toolStatus = ref<ToolStatusDto | null>(null)
  const initialLoadError = ref<string | null>(null)
  const loadingInitial = ref(false)
  const activeRootId = ref<string>('')
  const currentRelPath = ref<string>('')
  const snapshot = ref<DirectorySnapshot | null>(null)
  const rootFileCounts = ref<Record<string, number>>({})
  const loadingDir = ref(false)
  const dirError = ref<string | null>(null)

  const domainNavMemory = ref<Record<MediaKind, { rootId: string; relPath: string }>>({
    book: { rootId: '', relPath: '' },
    comic: { rootId: '', relPath: '' },
    music: { rootId: '', relPath: '' },
    video: { rootId: '', relPath: '' },
  })

  let dirLoadSeq = 0
  let dirAbortController: AbortController | null = null
  let routeSyncCallback: ((replace?: boolean) => void) | null = null

  function registerRouteSync(cb: ((replace?: boolean) => void) | null): void {
    routeSyncCallback = cb
  }

  function notifyRouteSync(replace = false): void {
    routeSyncCallback?.(replace)
  }

  function nextDirAbortSignal(): AbortController {
    dirAbortController?.abort()
    const controller = new AbortController()
    dirAbortController = controller
    return controller
  }

  function isAbortError(err: unknown, controller: AbortController): boolean {
    if (controller.signal.aborted) return true
    return err instanceof Error && err.name === 'AbortError'
  }

  const isCompact = computed(() => collapsed.value && !mobileMenuOpen.value)
  const domainRoots = computed<ManagedRootDto[]>(() =>
    (settings.value?.roots ?? []).filter((r) => r.default_kind === activeDomain.value),
  )
  const isMultiRootSummary = computed<boolean>(
    () => !activeRootId.value && domainRoots.value.length > 1,
  )
  const fallbackRootId = computed<string>(
    () => activeRootId.value || snapshot.value?.root_id || domainRoots.value[0]?.id || '',
  )

  const domainLabels = computed(() => {
    const dom = activeDomain.value
    return {
      missingCover: t(`tool.domainLabels.${dom}.missingCover`),
      missingSummary: t(`tool.domainLabels.${dom}.missingSummary`),
      missingTagBlock: t(`tool.domainLabels.${dom}.missingTagBlock`),
      emptyRootsTitle: t(`tool.domainLabels.${dom}.emptyRootsTitle`),
      emptyRootsDesc: t(`tool.domainLabels.${dom}.emptyRootsDesc`),
      searchPlaceholder: t(`tool.domainLabels.${dom}.searchPlaceholder`),
    }
  })

  const breadcrumbs = computed<Array<{ label: string; relPath: string }>>(() => {
    if (!snapshot.value) return []
    const items: Array<{ label: string; relPath: string }> = [
      { label: snapshot.value.root_name, relPath: '' },
    ]
    if (!snapshot.value.current_rel_path) return items
    const segments = snapshot.value.current_rel_path.split('/').filter(Boolean)
    let acc = ''
    for (const seg of segments) {
      acc = acc ? `${acc}/${seg}` : seg
      items.push({ label: seg, relPath: acc })
    }
    return items
  })

  const currentFolderMap = computed(() => {
    const m = new Map<string, { name: string; media_file_count: number; child_dir_count: number }>()
    for (const f of snapshot.value?.folders ?? []) {
      m.set(f.rel_path, {
        name: f.name,
        media_file_count: f.media_file_count,
        child_dir_count: f.child_dir_count,
      })
    }
    return m
  })

  function entrySelectionKey(entry: Pick<MediaFileEntry, 'root_id' | 'rel_path'>): string {
    return isMultiRootSummary.value ? `${entry.root_id}::${entry.rel_path}` : entry.rel_path
  }

  function parseSelectionKey(key: string): { rootId: string; relPath: string } {
    const sep = key.indexOf('::')
    if (sep !== -1) return { rootId: key.slice(0, sep), relPath: key.slice(sep + 2) }
    return { rootId: fallbackRootId.value, relPath: key }
  }

  function resolveModalRootId(entries: MediaFileEntry[]): string | null {
    const uniqueRoots = new Set(entries.map((e) => e.root_id).filter(Boolean))
    if (uniqueRoots.size > 1) return null
    if (uniqueRoots.size === 1) return Array.from(uniqueRoots)[0]!
    return fallbackRootId.value
  }

  function syncDomainStateToMemory(dom: MediaKind, rootId: string, relPath: string): void {
    domainNavMemory.value = { ...domainNavMemory.value, [dom]: { rootId, relPath } }
  }

  function findEntryByHashItem(rawItem: string): MediaFileEntry | undefined {
    return (snapshot.value?.files ?? []).find(
      (f) =>
        entrySelectionKey(f) === rawItem ||
        entryCompositeKey(f) === rawItem ||
        f.rel_path === rawItem,
    )
  }

  function reconcileAfterSnapshot(
    survivingSelectionKeys: Set<string>,
    survivingInspectKeys: Set<string>,
    preserveSelection: boolean,
  ): void {
    const selectionStore = useToolSelectionStore()
    if (preserveSelection) selectionStore.pruneSelection(survivingSelectionKeys)
    if (inspectorStore.inspectingPath && !survivingInspectKeys.has(inspectorStore.inspectingPath)) {
      inspectorStore.clearActiveDetail()
      notifyRouteSync(true)
    }
  }

  watch(
    () => [activeDomain.value, domainRoots.value.map((r) => r.id).join(',')] as const,
    ([dom, rootsKey], [prevDom, prevRootsKey]) => {
      if (!initialLoaded.value || dom !== prevDom || rootsKey === prevRootsKey) return
      const roots = domainRoots.value
      if (roots.length === 0) {
        activeRootId.value = ''
        snapshot.value = null
        rootFileCounts.value = {}
        inspectorStore.clearActiveDetail()
      } else if (activeRootId.value && !roots.some((r) => r.id === activeRootId.value)) {
        activeRootId.value = ''
        currentRelPath.value = ''
        void loadAllDomainRoots()
      } else if (!activeRootId.value) {
        void loadAllDomainRoots()
      }
    },
  )

  async function loadAllDomainRoots(): Promise<void> {
    const selectionStore = useToolSelectionStore()
    const reqDomain = activeDomain.value
    const roots = (settings.value?.roots ?? []).filter((r) => r.default_kind === reqDomain)
    if (roots.length === 0) {
      dirAbortController?.abort()
      dirAbortController = null
      snapshot.value = null
      rootFileCounts.value = {}
      return
    }
    const seq = ++dirLoadSeq
    const controller = nextDirAbortSignal()
    const preserveSelection = activeRootId.value === '' && currentRelPath.value === ''
    loadingDir.value = true
    dirError.value = null
    if (!preserveSelection) selectionStore.clearSelection()

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
        const snap = await toolApi.browse(
          only.id,
          '',
          reqDomain,
          true,
          (prog) => {
            if (seq === dirLoadSeq && !controller.signal.aborted) taskStore.updateTask(taskId, prog)
          },
          controller.signal,
        )
        if (seq !== dirLoadSeq || activeDomain.value !== reqDomain || controller.signal.aborted) return
        taskStore.completeTask(taskId, `已加载 ${snap.files.length.toLocaleString()} 个媒体文件`)
        snapshot.value = snap
        rootFileCounts.value = { [only.id]: snap.files.length }
        currentRelPath.value = ''
        syncDomainStateToMemory(reqDomain, '', '')
        notifyRouteSync(true)
        const fileRels = new Set(snap.files.map((f) => f.rel_path))
        const allSurviving = new Set([
          ...fileRels,
          ...(snap.raw_files ?? []).map((f) => f.rel_path),
          ...(snap.folders ?? []).map((d) => d.rel_path),
        ])
        reconcileAfterSnapshot(allSurviving, fileRels, preserveSelection)
        return
      }

      const snaps = await Promise.all(
        roots.map((r) => toolApi.browse(r.id, '', reqDomain, true, undefined, controller.signal)),
      )
      if (seq !== dirLoadSeq || activeDomain.value !== reqDomain || controller.signal.aborted) return
      const counts: Record<string, number> = {}
      const mergedFiles: MediaFileEntry[] = []
      for (let i = 0; i < roots.length; i += 1) {
        counts[roots[i]!.id] = snaps[i]!.files.length
        mergedFiles.push(...snaps[i]!.files)
      }
      taskStore.completeTask(taskId, `已汇总 ${mergedFiles.length.toLocaleString()} 个媒体文件`)
      snapshot.value = {
        root_id: '',
        root_name: `全部挂载目录 (${roots.length})`,
        current_rel_path: '',
        parent_rel_path: null,
        folders: [],
        files: mergedFiles,
        raw_files: [],
      }
      rootFileCounts.value = counts
      currentRelPath.value = ''
      syncDomainStateToMemory(reqDomain, '', '')
      notifyRouteSync(true)
      const compositeKeys = new Set(mergedFiles.map((f) => `${f.root_id}::${f.rel_path}`))
      reconcileAfterSnapshot(compositeKeys, compositeKeys, preserveSelection)
    } catch (err) {
      if (isAbortError(err, controller)) return
      const msg = err instanceof Error ? err.message : String(err)
      if (seq === dirLoadSeq && activeDomain.value === reqDomain) {
        taskStore.failTask(taskId, msg)
        dirError.value = msg
      }
    } finally {
      if (seq === dirLoadSeq && activeDomain.value === reqDomain) loadingDir.value = false
    }
  }

  async function loadDirectory(rootId: string, relPath: string): Promise<void> {
    if (!rootId) return
    const selectionStore = useToolSelectionStore()
    const seq = ++dirLoadSeq
    const controller = nextDirAbortSignal()
    const reqDomain = activeDomain.value
    const preserveSelection = rootId === activeRootId.value && relPath === currentRelPath.value
    loadingDir.value = true
    dirError.value = null
    if (!preserveSelection) selectionStore.clearSelection()

    const rootLabel = settings.value?.roots.find((r) => r.id === rootId)?.name || '媒体目录'
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
          snap = await toolApi.browse(
            rootId,
            candidateRel,
            reqDomain,
            true,
            (prog) => {
              if (seq === dirLoadSeq && !controller.signal.aborted) taskStore.updateTask(taskId, prog)
            },
            controller.signal,
          )
          break
        } catch (err) {
          if (isAbortError(err, controller)) throw err
          const errStatus = (err as Error & { status?: number })?.status
          const isNotFound =
            errStatus === 404 || (err instanceof Error && /not found|不存在|no such/i.test(err.message))
          if (isNotFound && candidateRel) {
            candidateRel = parentRelOf(candidateRel)
            continue
          }
          throw err
        }
      }
      if (seq !== dirLoadSeq || activeDomain.value !== reqDomain || controller.signal.aborted) return
      if (!snap) {
        taskStore.completeTask(taskId, '已取消')
        return
      }
      taskStore.completeTask(taskId, `已加载 ${snap.files.length.toLocaleString()} 个媒体文件`)
      snapshot.value = snap
      currentRelPath.value = snap.current_rel_path
      syncDomainStateToMemory(reqDomain, rootId, snap.current_rel_path)
      notifyRouteSync(true)
      const fileRels = new Set(snap.files.map((f) => f.rel_path))
      const allSurviving = new Set([
        ...fileRels,
        ...(snap.raw_files ?? []).map((f) => f.rel_path),
        ...(snap.folders ?? []).map((d) => d.rel_path),
      ])
      reconcileAfterSnapshot(allSurviving, fileRels, preserveSelection)
    } catch (err) {
      if (isAbortError(err, controller)) return
      const msg = err instanceof Error ? err.message : String(err)
      if (seq === dirLoadSeq && activeDomain.value === reqDomain) {
        taskStore.failTask(taskId, msg)
        dirError.value = msg
      }
    } finally {
      if (seq === dirLoadSeq && activeDomain.value === reqDomain) loadingDir.value = false
    }
  }

  async function reloadCurrentScope(): Promise<void> {
    if (activeRootId.value) await loadDirectory(activeRootId.value, currentRelPath.value)
    else await loadAllDomainRoots()
  }

  async function activateDomainWorkspace(dom: MediaKind, preserveQuery = false): Promise<void> {
    const selectionStore = useToolSelectionStore()
    const roots = (settings.value?.roots ?? []).filter((r) => r.default_kind === dom)
    selectionStore.clearSelection()
    inspectorStore.clearActiveDetail()
    if (!preserveQuery) {
      selectionStore.completenessFilter = 'all'
      selectionStore.searchQuery = ''
    }
    dirError.value = null
    if (roots.length === 0) {
      activeRootId.value = ''
      currentRelPath.value = ''
      snapshot.value = null
      rootFileCounts.value = {}
      return
    }
    const mem = domainNavMemory.value[dom]
    if (mem.rootId && roots.some((r) => r.id === mem.rootId)) {
      activeRootId.value = mem.rootId
      currentRelPath.value = mem.relPath
      await loadDirectory(mem.rootId, mem.relPath)
      return
    }
    activeRootId.value = ''
    currentRelPath.value = ''
    await loadAllDomainRoots()
  }

  async function triggerManualRescan(): Promise<void> {
    if (notifyingRescan.value) return
    notifyingRescan.value = true
    try {
      const rep = await toolApi.triggerWebhookRescan(activeRootId.value || undefined)
      snackbar.show(rep.message)
    } catch (err) {
      dirError.value = err instanceof Error ? err.message : String(err)
    } finally {
      notifyingRescan.value = false
    }
  }

  function handleSettingsUpdated(next: PublicSettingsDto): void {
    const templatesChanged =
      JSON.stringify(settings.value?.rename_templates) !== JSON.stringify(next.rename_templates)
    settings.value = next
    if (templatesChanged) void reloadCurrentScope()
  }

  return {
    activeDomain,
    activeNav,
    previousWorkspaceNav,
    settingsInitialTab,
    collapsed,
    mobileMenuOpen,
    notifyingRescan,
    initialLoaded,
    settings,
    toolStatus,
    initialLoadError,
    loadingInitial,
    activeRootId,
    currentRelPath,
    snapshot,
    rootFileCounts,
    loadingDir,
    dirError,
    domainNavMemory,
    isCompact,
    domainRoots,
    domainLabels,
    isMultiRootSummary,
    fallbackRootId,
    breadcrumbs,
    currentFolderMap,
    registerRouteSync,
    notifyRouteSync,
    entrySelectionKey,
    parseSelectionKey,
    resolveModalRootId,
    findEntryByHashItem,
    syncDomainStateToMemory,
    loadAllDomainRoots,
    loadDirectory,
    reloadCurrentScope,
    activateDomainWorkspace,
    triggerManualRescan,
    handleSettingsUpdated,
  }
})

import { computed, onBeforeUnmount, onMounted, ref, type ComputedRef, type Ref } from 'vue'
import { useSnackbarStore } from '@/stores/snackbar'
import type { MediaKind } from '@/types/api'
import { toolApi } from '../../api'
import { formatAutoScrapeSummary, loadLastAutoScrapeOptions } from '../../autoScrapePrefs'
import { entryCompositeKey } from '../../completeness'
import { useToolTaskStore } from '../../tasks'
import type {
  AutoScrapeOptionsDto,
  DirectorySnapshot,
  ManagedRootDto,
  MediaFileDetail,
  MediaFileEntry,
} from '../../types'
import type { InspectTaskItemPayload, ReviewItemsPayload } from '../ToolTasksDrawer.vue'
import type { BrowseViewMode, CompletenessFilter, PrimaryNavKey } from './hashState'

export interface UseWorkspaceWorkbenchOptions {
  activeDomain: Ref<MediaKind>
  activeNav: Ref<PrimaryNavKey>
  activeRootId: Ref<string>
  currentRelPath: Ref<string>
  domainRoots: ComputedRef<ManagedRootDto[]>
  snapshot: Ref<DirectorySnapshot | null>
  filteredFiles: ComputedRef<MediaFileEntry[]>
  selectedPaths: Ref<Set<string>>
  inspectingPath: Ref<string | null>
  activeDetail: Ref<MediaFileDetail | null>
  completenessFilter: Ref<CompletenessFilter>
  workbenchFocusPaths: Ref<Set<string> | null>
  mobileMenuOpen: Ref<boolean>
  scrapeWorkbenchActive: Ref<boolean>
  scrapeWorkbenchPaths: Ref<string[] | null>
  entrySelectionKey: (entry: Pick<MediaFileEntry, 'root_id' | 'rel_path'>) => string
  bumpFileArtworkVersion: (rootId: string, relPath: string) => void
  confirmActiveEditorClean: () => Promise<boolean>
  clearActiveDetail: () => void
  setViewMode: (next: BrowseViewMode) => void
  switchDomain: (domain: MediaKind) => Promise<void>
  loadDirectory: (rootId: string, relPath: string) => Promise<void>
  loadAllDomainRoots: () => Promise<void>
  reloadCurrentScope: () => Promise<void>
  inspectEntry: (entry: MediaFileEntry) => Promise<void>
  nextInspectSeq: () => number
  currentInspectSeq: () => number
  onOpenBookContentEditor: (entry: MediaFileEntry) => void
}

export function useWorkspaceWorkbench(opts: UseWorkspaceWorkbenchOptions) {
  const taskStore = useToolTaskStore()
  const snackbar = useSnackbarStore()

  const scrapeWorkbenchSavedPaths = ref<Set<string>>(new Set())
  const scrapeStrategyRev = ref(0)

  const lastScrapeSummary = computed<string>(() => {
    void scrapeStrategyRev.value
    return formatAutoScrapeSummary(loadLastAutoScrapeOptions(opts.activeDomain.value))
  })

  function bumpScrapeStrategyRev(): void {
    scrapeStrategyRev.value += 1
  }

  function enqueueScrapeFiles(
    files: MediaFileEntry[],
    labelHint?: string,
    options?: AutoScrapeOptionsDto,
  ): void {
    if (files.length === 0) return
    const effectiveOpts = options ?? loadLastAutoScrapeOptions(files[0]!.media_kind)
    taskStore.enqueueScrapeTask(files, labelHint, effectiveOpts)
  }

  const workbenchQueueEntries = computed<MediaFileEntry[]>(() => {
    const scoped = opts.scrapeWorkbenchPaths.value
    if (!scoped) return opts.filteredFiles.value
    const byKey = new Map((opts.snapshot.value?.files ?? []).map((f) => [entryCompositeKey(f), f]))
    const resolved: MediaFileEntry[] = []
    for (const k of scoped) {
      const hit = byKey.get(k)
      if (hit) resolved.push(hit)
    }
    return resolved.length > 0 ? resolved : opts.filteredFiles.value
  })

  const activeQueueList = computed<MediaFileEntry[]>(() =>
    opts.scrapeWorkbenchActive.value ? workbenchQueueEntries.value : opts.filteredFiles.value,
  )

  const inspectingIndex = computed(() => {
    if (!opts.inspectingPath.value) return -1
    return activeQueueList.value.findIndex(
      (f) => opts.entrySelectionKey(f) === opts.inspectingPath.value,
    )
  })

  const hasPrevInspect = computed(() => inspectingIndex.value > 0)
  const hasNextInspect = computed(
    () => inspectingIndex.value >= 0 && inspectingIndex.value < activeQueueList.value.length - 1,
  )
  const inspectPositionLabel = computed(() => {
    if (inspectingIndex.value < 0) return ''
    return `${inspectingIndex.value + 1} / ${activeQueueList.value.length}`
  })

  async function stepInspectingFile(delta: -1 | 1): Promise<void> {
    const nextIdx = inspectingIndex.value + delta
    const target = activeQueueList.value[nextIdx]
    if (!target) return
    await opts.inspectEntry(target)
  }

  async function handleDetailUpdated(updated: MediaFileDetail, bumpArt = false): Promise<void> {
    const prevRel = opts.activeDetail.value?.entry.rel_path
    if (bumpArt) {
      opts.bumpFileArtworkVersion(updated.entry.root_id, updated.entry.rel_path)
      if (prevRel && prevRel !== updated.entry.rel_path) {
        opts.bumpFileArtworkVersion(updated.entry.root_id, prevRel)
      }
    }
    opts.activeDetail.value = updated
    opts.inspectingPath.value = opts.entrySelectionKey(updated.entry)

    if (opts.scrapeWorkbenchActive.value) {
      const nextKey = entryCompositeKey(updated.entry)
      const nextSaved = new Set(scrapeWorkbenchSavedPaths.value)
      nextSaved.add(nextKey)
      scrapeWorkbenchSavedPaths.value = nextSaved
      if (prevRel && prevRel !== updated.entry.rel_path && opts.scrapeWorkbenchPaths.value) {
        const prevComposite = `${updated.entry.root_id}::${prevRel}`
        opts.scrapeWorkbenchPaths.value = opts.scrapeWorkbenchPaths.value.map((k) =>
          k === prevComposite ? nextKey : k,
        )
      }
    }

    if (prevRel && prevRel !== updated.entry.rel_path) {
      await opts.reloadCurrentScope()
    } else if (opts.snapshot.value) {
      opts.snapshot.value = {
        ...opts.snapshot.value,
        files: opts.snapshot.value.files.map((f) =>
          f.root_id === updated.entry.root_id && f.rel_path === updated.entry.rel_path
            ? updated.entry
            : f,
        ),
      }
    }
  }

  async function openScrapeWorkbench(
    targetEntry?: MediaFileEntry,
    scopedEntries?: MediaFileEntry[],
  ): Promise<void> {
    if (!(await opts.confirmActiveEditorClean())) return
    const scopedList = scopedEntries && scopedEntries.length > 0 ? scopedEntries : null
    opts.scrapeWorkbenchPaths.value = scopedList
      ? scopedList.map((e) => entryCompositeKey(e))
      : null
    scrapeWorkbenchSavedPaths.value = new Set()
    opts.scrapeWorkbenchActive.value = true
    opts.activeNav.value = 'items'
    opts.mobileMenuOpen.value = false

    const target =
      targetEntry ??
      (scopedList ? scopedList[0] : opts.activeDetail.value?.entry ?? opts.filteredFiles.value[0])
    if (target) {
      if (
        !opts.activeDetail.value ||
        entryCompositeKey(opts.activeDetail.value.entry) !== entryCompositeKey(target)
      ) {
        await opts.inspectEntry(target)
      }
    }
  }

  async function exitScrapeWorkbench(): Promise<void> {
    if (!(await opts.confirmActiveEditorClean())) return
    opts.scrapeWorkbenchActive.value = false
    opts.scrapeWorkbenchPaths.value = null
    opts.mobileMenuOpen.value = false
  }

  async function ensureTaskItemDirectoryLoaded(
    domain: MediaKind,
    rootId: string,
  ): Promise<boolean> {
    if (opts.activeDomain.value !== domain) {
      if (!(await opts.confirmActiveEditorClean())) return false
      await opts.switchDomain(domain)
    }
    if (opts.activeRootId.value !== rootId || opts.currentRelPath.value !== '') {
      if (!(await opts.confirmActiveEditorClean())) return false
      opts.clearActiveDetail()
      opts.activeRootId.value = rootId
      opts.currentRelPath.value = ''
      await opts.loadDirectory(rootId, '')
    }
    return true
  }

  async function handleReviewUnmatchedItems(payload: ReviewItemsPayload): Promise<void> {
    if (payload.items.length === 0) return
    const uniqueRoots = new Set(payload.items.map((it) => it.rootId).filter(Boolean))
    if (uniqueRoots.size <= 1) {
      const onlyRootId = Array.from(uniqueRoots)[0] ?? ''
      if (onlyRootId) {
        const ok = await ensureTaskItemDirectoryLoaded(payload.domain, onlyRootId)
        if (!ok) return
      }
    } else {
      if (opts.activeDomain.value !== payload.domain) {
        if (!(await opts.confirmActiveEditorClean())) return
        await opts.switchDomain(payload.domain)
      }
      if (opts.activeRootId.value !== '' || opts.currentRelPath.value !== '' || !opts.snapshot.value) {
        if (!(await opts.confirmActiveEditorClean())) return
        opts.clearActiveDetail()
        opts.activeRootId.value = ''
        opts.currentRelPath.value = ''
        await opts.loadAllDomainRoots()
      }
    }

    const orderedKeys = payload.items.map((it) => `${it.rootId}::${it.relPath}`)
    const targetKeySet = new Set(orderedKeys)
    opts.completenessFilter.value = 'all'
    opts.workbenchFocusPaths.value = targetKeySet
    opts.setViewMode('flat')
    opts.activeNav.value = 'items'

    const byCompositeKey = new Map(
      (opts.snapshot.value?.files ?? []).map((f) => [entryCompositeKey(f), f]),
    )
    const matchedEntries: MediaFileEntry[] = []
    for (const key of orderedKeys) {
      const hit = byCompositeKey.get(key)
      if (hit) matchedEntries.push(hit)
    }

    if (payload.openCandidates ?? true) {
      if (matchedEntries.length > 0) {
        await openScrapeWorkbench(matchedEntries[0], matchedEntries)
      }
      return
    }
    opts.scrapeWorkbenchActive.value = false
    opts.scrapeWorkbenchPaths.value = null
    const firstEntry = matchedEntries[0]
    if (firstEntry) {
      void opts.inspectEntry(firstEntry)
    }
  }

  async function handleInspectTaskItem(payload: InspectTaskItemPayload): Promise<void> {
    const ok = await ensureTaskItemDirectoryLoaded(payload.domain, payload.rootId)
    if (!ok) return
    opts.completenessFilter.value = 'all'
    opts.workbenchFocusPaths.value = new Set([`${payload.rootId}::${payload.relPath}`])
    opts.setViewMode('flat')
    opts.activeNav.value = 'items'
    if (!payload.openCandidates) {
      opts.scrapeWorkbenchActive.value = false
      opts.scrapeWorkbenchPaths.value = null
    }
    const target = (opts.snapshot.value?.files ?? []).find(
      (f) => f.root_id === payload.rootId && f.rel_path === payload.relPath,
    )
    if (target) {
      if (payload.openCandidates) {
        await openScrapeWorkbench(target, [target])
      } else {
        await opts.inspectEntry(target)
      }
    } else {
      const seq = opts.nextInspectSeq()
      try {
        const detail = await toolApi.inspectFile(payload.rootId, payload.relPath, payload.domain)
        if (seq !== opts.currentInspectSeq()) return
        opts.activeDetail.value = detail
        opts.inspectingPath.value = opts.entrySelectionKey(detail.entry)
      } catch (err) {
        snackbar.show({
          message: err instanceof Error ? err.message : String(err),
          tone: 'error',
        })
      }
    }
  }

  async function handleOpenTaskBookEditor(payload: InspectTaskItemPayload): Promise<void> {
    const ok = await ensureTaskItemDirectoryLoaded(payload.domain, payload.rootId)
    if (!ok) return
    const target = (opts.snapshot.value?.files ?? []).find(
      (f) => f.root_id === payload.rootId && f.rel_path === payload.relPath,
    )
    if (target) {
      opts.onOpenBookContentEditor(target)
      return
    }
    try {
      const detail = await toolApi.inspectFile(payload.rootId, payload.relPath, 'book')
      opts.onOpenBookContentEditor(detail.entry)
    } catch (err) {
      snackbar.show({
        message: err instanceof Error ? err.message : String(err),
        tone: 'error',
      })
    }
  }

  let offFileScraped: (() => void) | null = null
  let offBatchSettled: (() => void) | null = null
  let scrapeRefreshTimer: ReturnType<typeof setTimeout> | null = null

  function scheduleScrapeDirectoryRefresh(delayMs = 350): void {
    if (opts.domainRoots.value.length === 0) return
    if (scrapeRefreshTimer) clearTimeout(scrapeRefreshTimer)
    scrapeRefreshTimer = setTimeout(() => {
      scrapeRefreshTimer = null
      if (opts.domainRoots.value.length > 0) {
        void opts.reloadCurrentScope()
      }
    }, delayMs)
  }

  onMounted(() => {
    offFileScraped = taskStore.onFileScraped((updated, prevRelPath) => {
      opts.bumpFileArtworkVersion(updated.entry.root_id, updated.entry.rel_path)
      if (prevRelPath && prevRelPath !== updated.entry.rel_path) {
        opts.bumpFileArtworkVersion(updated.entry.root_id, prevRelPath)
      }
      const moved = Boolean(prevRelPath && prevRelPath !== updated.entry.rel_path)
      const prevKey = opts.entrySelectionKey({
        root_id: updated.entry.root_id,
        rel_path: prevRelPath ?? updated.entry.rel_path,
      })
      const nextKey = opts.entrySelectionKey(updated.entry)

      if (moved) {
        if (opts.selectedPaths.value.has(prevKey)) {
          const nextSel = new Set(opts.selectedPaths.value)
          nextSel.delete(prevKey)
          nextSel.add(nextKey)
          opts.selectedPaths.value = nextSel
        }
        const prevComposite = `${updated.entry.root_id}::${prevRelPath!}`
        const nextComposite = entryCompositeKey(updated.entry)
        if (opts.scrapeWorkbenchPaths.value?.includes(prevComposite)) {
          opts.scrapeWorkbenchPaths.value = opts.scrapeWorkbenchPaths.value.map((k) =>
            k === prevComposite ? nextComposite : k,
          )
        }
      }

      const matchesCurrentScope =
        Boolean(opts.snapshot.value) &&
        (opts.snapshot.value!.root_id === updated.entry.root_id ||
          (!opts.activeRootId.value &&
            opts.domainRoots.value.some((r) => r.id === updated.entry.root_id)))
      if (matchesCurrentScope && opts.snapshot.value) {
        opts.snapshot.value = {
          ...opts.snapshot.value,
          files: opts.snapshot.value.files.map((f) =>
            f.root_id === updated.entry.root_id &&
            (f.rel_path === prevRelPath || f.rel_path === updated.entry.rel_path)
              ? updated.entry
              : f,
          ),
        }
        if (moved) {
          scheduleScrapeDirectoryRefresh(350)
        }
      }

      if (
        opts.activeDetail.value?.entry.root_id === updated.entry.root_id &&
        (opts.activeDetail.value.entry.rel_path === prevRelPath ||
          opts.activeDetail.value.entry.rel_path === updated.entry.rel_path)
      ) {
        opts.activeDetail.value = updated
        opts.inspectingPath.value = nextKey
      }
    })

    offBatchSettled = taskStore.onBatchSettled((task) => {
      if (opts.domainRoots.value.length === 0) return
      const scopeRootIds = opts.activeRootId.value
        ? new Set([opts.activeRootId.value])
        : new Set(opts.domainRoots.value.map((r) => r.id))
      const touchesCurrentScope = Boolean(
        task.subItems?.some((it) => scopeRootIds.has(it.rootId)),
      )
      if (!touchesCurrentScope) return
      if ((task.matchedCount ?? 0) > 0 || task.scrapeOptions?.organize_by_template) {
        if (scrapeRefreshTimer) {
          clearTimeout(scrapeRefreshTimer)
          scrapeRefreshTimer = null
        }
        void opts.reloadCurrentScope()
      }
    })
  })

  onBeforeUnmount(() => {
    if (offFileScraped) offFileScraped()
    if (offBatchSettled) offBatchSettled()
    if (scrapeRefreshTimer) clearTimeout(scrapeRefreshTimer)
  })

  return {
    scrapeWorkbenchSavedPaths,
    lastScrapeSummary,
    bumpScrapeStrategyRev,
    enqueueScrapeFiles,
    workbenchQueueEntries,
    activeQueueList,
    inspectingIndex,
    hasPrevInspect,
    hasNextInspect,
    inspectPositionLabel,
    stepInspectingFile,
    handleDetailUpdated,
    openScrapeWorkbench,
    exitScrapeWorkbench,
    handleReviewUnmatchedItems,
    handleInspectTaskItem,
    handleOpenTaskBookEditor,
  }
}

import { defineStore } from 'pinia'
import { computed, ref } from 'vue'
import { useSnackbarStore } from '@/stores/snackbar'
import type { MediaKind } from '@/types/api'
import { toolApi } from '../api'
import {
  formatAutoScrapeSummary,
  loadLastAutoScrapeOptions,
  saveLastAutoScrapeOptions,
} from '../autoScrapePrefs'
import { entryCompositeKey } from '../completeness'
import type { InspectTaskItemPayload, ReviewItemsPayload } from '../components/ToolTasksDrawer.vue'
import { useToolTaskStore } from '../tasks'
import type { AutoScrapeOptionsDto, MediaFileDetail, MediaFileEntry } from '../types'
import { useToolInspectorStore } from './inspector'
import { useToolWorkspaceStore } from './workspace'

export const useToolScrapeStore = defineStore('toolScrape', () => {
  const workspaceStore = useToolWorkspaceStore()
  const inspectorStore = useToolInspectorStore()
  const taskStore = useToolTaskStore()
  const snackbar = useSnackbarStore()

  const scrapeWorkbenchActive = ref(false)
  const scrapeWorkbenchPaths = ref<string[] | null>(null)
  const scrapeWorkbenchSavedPaths = ref<Set<string>>(new Set())
  const scrapeStrategyRev = ref(0)

  const lastScrapeSummary = computed<string>(() => {
    void scrapeStrategyRev.value
    return formatAutoScrapeSummary(loadLastAutoScrapeOptions(workspaceStore.activeDomain))
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

  function quickScrapeSelection(): void {
    const targets =
      workspaceStore.checkedFiles.length > 0
        ? [...workspaceStore.checkedFiles]
        : [...workspaceStore.filteredFiles]
    if (targets.length === 0) return
    const autoOpts = loadLastAutoScrapeOptions(workspaceStore.activeDomain)
    enqueueScrapeFiles(targets, undefined, autoOpts)
    snackbar.show({
      message: `已按「${formatAutoScrapeSummary(autoOpts)}」策略提交 ${targets.length} 项快刮任务`,
      actionLabel: '调整策略',
      onAction: () => {
        workspaceStore.autoScrapeTargetEntries = targets
        workspaceStore.autoScrapeModalOpen = true
      },
    })
  }

  function configureScrapeSelection(): void {
    const targets =
      workspaceStore.checkedFiles.length > 0
        ? [...workspaceStore.checkedFiles]
        : [...workspaceStore.filteredFiles]
    if (targets.length === 0) return
    workspaceStore.autoScrapeTargetEntries = targets
    workspaceStore.autoScrapeModalOpen = true
  }

  function handleAutoScrapeConfirmed(
    entries: MediaFileEntry[],
    options: AutoScrapeOptionsDto,
  ): void {
    workspaceStore.autoScrapeModalOpen = false
    saveLastAutoScrapeOptions(workspaceStore.activeDomain, options)
    bumpScrapeStrategyRev()
    enqueueScrapeFiles(entries, undefined, options)
  }

  function handleAutoScrapeToDryRun(
    entries: MediaFileEntry[],
    options: AutoScrapeOptionsDto,
  ): void {
    workspaceStore.autoScrapeModalOpen = false
    saveLastAutoScrapeOptions(workspaceStore.activeDomain, options)
    bumpScrapeStrategyRev()
    if (entries.length > 0 && !workspaceStore.resolveModalRootId(entries)) {
      snackbar.show({
        message: '所选条目跨越多个挂载目录，请先从左侧选择单个挂载目录再进行置信度预演',
        tone: 'error',
      })
      return
    }
    workspaceStore.selectedPaths = new Set(
      entries.map((e) => workspaceStore.entrySelectionKey(e)),
    )
    workspaceStore.dryRunTargetEntries = entries
    workspaceStore.dryRunScrapeOptions = options
    workspaceStore.dryRunModalOpen = true
  }

  async function handleDryRunManualScrape(entries: MediaFileEntry[]): Promise<void> {
    workspaceStore.dryRunModalOpen = false
    if (entries.length === 0) return
    await openScrapeWorkbench(entries[0], entries)
  }

  const workbenchQueueEntries = computed<MediaFileEntry[]>(() => {
    const scoped = scrapeWorkbenchPaths.value
    if (!scoped) return workspaceStore.filteredFiles
    const byKey = new Map(
      (workspaceStore.snapshot?.files ?? []).map((f) => [entryCompositeKey(f), f]),
    )
    const resolved: MediaFileEntry[] = []
    for (const k of scoped) {
      const hit = byKey.get(k)
      if (hit) resolved.push(hit)
    }
    return resolved.length > 0 ? resolved : workspaceStore.filteredFiles
  })

  const activeQueueList = computed<MediaFileEntry[]>(() =>
    scrapeWorkbenchActive.value ? workbenchQueueEntries.value : workspaceStore.filteredFiles,
  )

  const inspectingIndex = computed(() => {
    if (!inspectorStore.inspectingPath) return -1
    return activeQueueList.value.findIndex(
      (f) => workspaceStore.entrySelectionKey(f) === inspectorStore.inspectingPath,
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
    await workspaceStore.inspectEntry(target)
  }

  async function handleDetailUpdated(updated: MediaFileDetail, bumpArt = false): Promise<void> {
    const prevRel = inspectorStore.activeDetail?.entry.rel_path
    if (bumpArt) {
      inspectorStore.bumpFileArtworkVersion(updated.entry.root_id, updated.entry.rel_path)
      if (prevRel && prevRel !== updated.entry.rel_path) {
        inspectorStore.bumpFileArtworkVersion(updated.entry.root_id, prevRel)
      }
    }
    inspectorStore.activeDetail = updated
    inspectorStore.inspectingPath = workspaceStore.entrySelectionKey(updated.entry)

    if (scrapeWorkbenchActive.value) {
      const nextKey = entryCompositeKey(updated.entry)
      const nextSaved = new Set(scrapeWorkbenchSavedPaths.value)
      nextSaved.add(nextKey)
      scrapeWorkbenchSavedPaths.value = nextSaved
      if (prevRel && prevRel !== updated.entry.rel_path && scrapeWorkbenchPaths.value) {
        const prevComposite = `${updated.entry.root_id}::${prevRel}`
        scrapeWorkbenchPaths.value = scrapeWorkbenchPaths.value.map((k) =>
          k === prevComposite ? nextKey : k,
        )
      }
    }

    if (prevRel && prevRel !== updated.entry.rel_path) {
      await workspaceStore.reloadCurrentScope()
    } else if (workspaceStore.snapshot) {
      workspaceStore.snapshot = {
        ...workspaceStore.snapshot,
        files: workspaceStore.snapshot.files.map((f) =>
          f.root_id === updated.entry.root_id && f.rel_path === updated.entry.rel_path
            ? updated.entry
            : f,
        ),
      }
    }
    workspaceStore.syncHashFromState(true)
  }

  async function handleBookContentSaved(updated: MediaFileDetail): Promise<void> {
    await handleDetailUpdated(updated, true)
    snackbar.show(`已保存章节目录与正文：${updated.entry.file_name}`)
  }

  async function openScrapeWorkbench(
    targetEntry?: MediaFileEntry,
    scopedEntries?: MediaFileEntry[],
  ): Promise<void> {
    if (!(await inspectorStore.confirmActiveEditorClean())) return
    const scopedList = scopedEntries && scopedEntries.length > 0 ? scopedEntries : null
    scrapeWorkbenchPaths.value = scopedList
      ? scopedList.map((e) => entryCompositeKey(e))
      : null
    scrapeWorkbenchSavedPaths.value = new Set()
    scrapeWorkbenchActive.value = true
    workspaceStore.activeNav = 'items'
    workspaceStore.previousWorkspaceNav = 'items'
    workspaceStore.mobileMenuOpen = false

    const target =
      targetEntry ??
      (scopedList
        ? scopedList[0]
        : inspectorStore.activeDetail?.entry ?? workspaceStore.filteredFiles[0])
    if (target) {
      if (
        !inspectorStore.activeDetail ||
        entryCompositeKey(inspectorStore.activeDetail.entry) !== entryCompositeKey(target)
      ) {
        await workspaceStore.inspectEntry(target, { skipDirtyCheck: true, replaceHash: false })
        return
      }
    }
    workspaceStore.syncHashFromState(false)
  }

  async function exitScrapeWorkbench(): Promise<void> {
    if (!(await inspectorStore.confirmActiveEditorClean())) return
    scrapeWorkbenchActive.value = false
    scrapeWorkbenchPaths.value = null
    workspaceStore.mobileMenuOpen = false
    workspaceStore.syncHashFromState(false)
  }

  async function requestQueuePickEntry(entry: MediaFileEntry): Promise<void> {
    workspaceStore.mobileMenuOpen = false
    const guard = inspectorStore.getWorkbenchGuard()
    if (guard) {
      await guard.requestPickEntry(entry)
    } else {
      await workspaceStore.inspectEntry(entry)
    }
  }

  async function requestQueueStep(delta: -1 | 1): Promise<void> {
    const guard = inspectorStore.getWorkbenchGuard()
    if (guard) {
      await guard.requestStep(delta)
    } else {
      await stepInspectingFile(delta)
    }
  }

  async function requestQueueExit(): Promise<void> {
    const guard = inspectorStore.getWorkbenchGuard()
    if (guard) {
      await guard.requestExit()
    } else {
      await exitScrapeWorkbench()
    }
  }

  async function ensureTaskItemDirectoryLoaded(
    domain: MediaKind,
    rootId: string,
  ): Promise<boolean> {
    if (workspaceStore.activeDomain !== domain) {
      if (!(await inspectorStore.confirmActiveEditorClean())) return false
      await workspaceStore.switchDomain(domain)
    }
    if (workspaceStore.activeRootId !== rootId || workspaceStore.currentRelPath !== '') {
      if (!(await inspectorStore.confirmActiveEditorClean())) return false
      inspectorStore.clearActiveDetail()
      workspaceStore.activeRootId = rootId
      workspaceStore.currentRelPath = ''
      await workspaceStore.loadDirectory(rootId, '')
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
      if (workspaceStore.activeDomain !== payload.domain) {
        if (!(await inspectorStore.confirmActiveEditorClean())) return
        await workspaceStore.switchDomain(payload.domain)
      }
      if (
        workspaceStore.activeRootId !== '' ||
        workspaceStore.currentRelPath !== '' ||
        !workspaceStore.snapshot
      ) {
        if (!(await inspectorStore.confirmActiveEditorClean())) return
        inspectorStore.clearActiveDetail()
        workspaceStore.activeRootId = ''
        workspaceStore.currentRelPath = ''
        await workspaceStore.loadAllDomainRoots()
      }
    }

    const orderedKeys = payload.items.map((it) => `${it.rootId}::${it.relPath}`)
    const targetKeySet = new Set(orderedKeys)
    workspaceStore.completenessFilter = 'all'
    workspaceStore.workbenchFocusPaths = targetKeySet
    workspaceStore.setViewMode('flat')
    workspaceStore.activeNav = 'items'

    const byCompositeKey = new Map(
      (workspaceStore.snapshot?.files ?? []).map((f) => [entryCompositeKey(f), f]),
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
    scrapeWorkbenchActive.value = false
    scrapeWorkbenchPaths.value = null
    const firstEntry = matchedEntries[0]
    if (firstEntry) {
      void workspaceStore.inspectEntry(firstEntry)
    }
  }

  async function handleInspectTaskItem(payload: InspectTaskItemPayload): Promise<void> {
    const ok = await ensureTaskItemDirectoryLoaded(payload.domain, payload.rootId)
    if (!ok) return
    workspaceStore.completenessFilter = 'all'
    workspaceStore.workbenchFocusPaths = new Set([`${payload.rootId}::${payload.relPath}`])
    workspaceStore.setViewMode('flat')
    workspaceStore.activeNav = 'items'
    if (!payload.openCandidates) {
      scrapeWorkbenchActive.value = false
      scrapeWorkbenchPaths.value = null
    }
    const target = (workspaceStore.snapshot?.files ?? []).find(
      (f) => f.root_id === payload.rootId && f.rel_path === payload.relPath,
    )
    if (target) {
      if (payload.openCandidates) {
        await openScrapeWorkbench(target, [target])
      } else {
        await workspaceStore.inspectEntry(target)
      }
    } else {
      const seq = inspectorStore.nextInspectSeq()
      try {
        const detail = await toolApi.inspectFile(payload.rootId, payload.relPath, payload.domain)
        if (seq !== inspectorStore.currentInspectSeq()) return
        inspectorStore.activeDetail = detail
        inspectorStore.inspectingPath = workspaceStore.entrySelectionKey(detail.entry)
        workspaceStore.syncHashFromState(true)
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
    const target = (workspaceStore.snapshot?.files ?? []).find(
      (f) => f.root_id === payload.rootId && f.rel_path === payload.relPath,
    )
    if (target) {
      workspaceStore.openBookContentEditor(target)
      return
    }
    try {
      const detail = await toolApi.inspectFile(payload.rootId, payload.relPath, 'book')
      workspaceStore.openBookContentEditor(detail.entry)
    } catch (err) {
      snackbar.show({
        message: err instanceof Error ? err.message : String(err),
        tone: 'error',
      })
    }
  }

  function bindTaskListeners(): () => void {
    let scrapeRefreshTimer: ReturnType<typeof setTimeout> | null = null

    function scheduleScrapeDirectoryRefresh(delayMs = 350): void {
      if (workspaceStore.domainRoots.length === 0) return
      if (scrapeRefreshTimer) clearTimeout(scrapeRefreshTimer)
      scrapeRefreshTimer = setTimeout(() => {
        scrapeRefreshTimer = null
        if (workspaceStore.domainRoots.length > 0) {
          void workspaceStore.reloadCurrentScope()
        }
      }, delayMs)
    }

    const offFileScraped = taskStore.onFileScraped((updated, prevRelPath) => {
      inspectorStore.bumpFileArtworkVersion(updated.entry.root_id, updated.entry.rel_path)
      if (prevRelPath && prevRelPath !== updated.entry.rel_path) {
        inspectorStore.bumpFileArtworkVersion(updated.entry.root_id, prevRelPath)
      }
      const moved = Boolean(prevRelPath && prevRelPath !== updated.entry.rel_path)
      const prevKey = workspaceStore.entrySelectionKey({
        root_id: updated.entry.root_id,
        rel_path: prevRelPath ?? updated.entry.rel_path,
      })
      const nextKey = workspaceStore.entrySelectionKey(updated.entry)

      if (moved) {
        if (workspaceStore.selectedPaths.has(prevKey)) {
          const nextSel = new Set(workspaceStore.selectedPaths)
          nextSel.delete(prevKey)
          nextSel.add(nextKey)
          workspaceStore.selectedPaths = nextSel
        }
        const prevComposite = `${updated.entry.root_id}::${prevRelPath!}`
        const nextComposite = entryCompositeKey(updated.entry)
        if (scrapeWorkbenchPaths.value?.includes(prevComposite)) {
          scrapeWorkbenchPaths.value = scrapeWorkbenchPaths.value.map((k) =>
            k === prevComposite ? nextComposite : k,
          )
        }
      }

      const matchesCurrentScope =
        Boolean(workspaceStore.snapshot) &&
        (workspaceStore.snapshot!.root_id === updated.entry.root_id ||
          (!workspaceStore.activeRootId &&
            workspaceStore.domainRoots.some((r) => r.id === updated.entry.root_id)))
      if (matchesCurrentScope && workspaceStore.snapshot) {
        workspaceStore.snapshot = {
          ...workspaceStore.snapshot,
          files: workspaceStore.snapshot.files.map((f) =>
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
        inspectorStore.activeDetail?.entry.root_id === updated.entry.root_id &&
        (inspectorStore.activeDetail.entry.rel_path === prevRelPath ||
          inspectorStore.activeDetail.entry.rel_path === updated.entry.rel_path)
      ) {
        inspectorStore.activeDetail = updated
        inspectorStore.inspectingPath = nextKey
        workspaceStore.syncHashFromState(true)
      }
    })

    const offBatchSettled = taskStore.onBatchSettled((task) => {
      if (workspaceStore.domainRoots.length === 0) return
      const scopeRootIds = workspaceStore.activeRootId
        ? new Set([workspaceStore.activeRootId])
        : new Set(workspaceStore.domainRoots.map((r) => r.id))
      const touchesCurrentScope = Boolean(
        task.subItems?.some((it) => scopeRootIds.has(it.rootId)),
      )
      if (!touchesCurrentScope) return
      if ((task.matchedCount ?? 0) > 0 || task.scrapeOptions?.organize_by_template) {
        if (scrapeRefreshTimer) {
          clearTimeout(scrapeRefreshTimer)
          scrapeRefreshTimer = null
        }
        void workspaceStore.reloadCurrentScope()
      }
    })

    return () => {
      offFileScraped()
      offBatchSettled()
      if (scrapeRefreshTimer) clearTimeout(scrapeRefreshTimer)
    }
  }

  return {
    scrapeWorkbenchActive,
    scrapeWorkbenchPaths,
    scrapeWorkbenchSavedPaths,
    scrapeStrategyRev,
    lastScrapeSummary,
    bumpScrapeStrategyRev,
    enqueueScrapeFiles,
    quickScrapeSelection,
    configureScrapeSelection,
    handleAutoScrapeConfirmed,
    handleAutoScrapeToDryRun,
    handleDryRunManualScrape,
    workbenchQueueEntries,
    activeQueueList,
    inspectingIndex,
    hasPrevInspect,
    hasNextInspect,
    inspectPositionLabel,
    stepInspectingFile,
    handleDetailUpdated,
    handleBookContentSaved,
    openScrapeWorkbench,
    exitScrapeWorkbench,
    requestQueuePickEntry,
    requestQueueStep,
    requestQueueExit,
    handleReviewUnmatchedItems,
    handleInspectTaskItem,
    handleOpenTaskBookEditor,
    bindTaskListeners,
  }
})

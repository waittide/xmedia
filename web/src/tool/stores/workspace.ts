import { defineStore } from 'pinia'
import { computed, ref, watch } from 'vue'
import {
  LuAlertCircle,
  LuCheckCircle2,
  LuFileText,
  LuImage,
  LuLayers,
  LuSparkles,
} from 'vue-icons-plus/lu'
import { i18n } from '@/i18n'
import { useDialogStore } from '@/stores/dialog'
import { useSnackbarStore } from '@/stores/snackbar'
import type { MediaKind } from '@/types/api'
import { toolApi } from '../api'
import { completenessScore, entryCompositeKey, isEntryComplete } from '../completeness'
import type { ToolSettingsTab } from '../components/ToolSettingsView.vue'
import {
  PAGE_CHUNK_SIZE,
  VALID_DOMAINS,
  buildToolHash,
  loadSavedDomain,
  loadSavedNav,
  loadSavedSortField,
  loadSavedViewMode,
  parentRelOf,
  parseToolHash,
  type BrowseViewMode,
  type CompletenessFilter,
  type PrimaryNavKey,
  type SortDir,
  type SortField,
} from '../components/workspace/hashState'
import { useToolTaskStore } from '../tasks'
import type {
  AutoScrapeOptionsDto,
  BatchMetadataEditReportDto,
  DirectorySnapshot,
  ManagedRootDto,
  MediaFileEntry,
  NormalizeBooksBatchReportDto,
  PublicSettingsDto,
} from '../types'
import { useToolInspectorStore } from './inspector'
import { useToolScrapeStore } from './scrape'

export const useToolWorkspaceStore = defineStore('toolWorkspace', () => {
  const inspectorStore = useToolInspectorStore()
  const taskStore = useToolTaskStore()
  const dialogStore = useDialogStore()
  const snackbar = useSnackbarStore()
  const t = (key: string, params?: Record<string, unknown>): string =>
    params ? String(i18n.global.t(key, params)) : String(i18n.global.t(key))

  const initialHash = parseToolHash()

  // Navigation & UI state
  const activeDomain = ref<MediaKind>(loadSavedDomain(initialHash))
  const activeNav = ref<PrimaryNavKey>(loadSavedNav(initialHash))
  const previousWorkspaceNav = ref<'home' | 'items'>(
    activeNav.value === 'settings' ? 'home' : activeNav.value,
  )
  const settingsInitialTab = ref<ToolSettingsTab>(initialHash.settingsTab ?? 'appearance')
  const collapsed = ref(localStorage.getItem('xmedia_tool.sidebar_collapsed') === '1')
  const mobileMenuOpen = ref(false)
  const tasksDrawerOpen = ref(false)
  const viewMode = ref<BrowseViewMode>(loadSavedViewMode())
  const sortField = ref<SortField>(loadSavedSortField())
  const sortDir = ref<SortDir>(
    localStorage.getItem('xmedia_tool.sort_dir') === 'desc' ? 'desc' : 'asc',
  )
  const visibleLimit = ref(PAGE_CHUNK_SIZE)
  const completenessFilter = ref<CompletenessFilter>('all')
  const searchQuery = ref('')
  const workbenchFocusPaths = ref<Set<string> | null>(null)
  const notifyingRescan = ref(false)

  // Settings & directory state
  const settings = ref<PublicSettingsDto | null>(null)
  const initialLoadError = ref<string | null>(null)
  const loadingInitial = ref(false)
  const activeRootId = ref<string>('')
  const currentRelPath = ref<string>('')
  const snapshot = ref<DirectorySnapshot | null>(null)
  const rootFileCounts = ref<Record<string, number>>({})
  const loadingDir = ref(false)
  const dirError = ref<string | null>(null)
  const selectedPaths = ref<Set<string>>(new Set())
  const lastSelectedRelPath = ref<string | null>(null)

  const domainNavMemory = ref<Record<MediaKind, { rootId: string; relPath: string }>>({
    book: { rootId: '', relPath: '' },
    comic: { rootId: '', relPath: '' },
    music: { rootId: '', relPath: '' },
    video: { rootId: '', relPath: '' },
  })

  // Modals state
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
  const musicToolsOpen = ref(false)
  const musicToolsEntries = ref<MediaFileEntry[]>([])
  const videoToolsOpen = ref(false)
  const videoToolsEntries = ref<MediaFileEntry[]>([])
  const dryRunModalOpen = ref(false)
  const dryRunTargetEntries = ref<MediaFileEntry[] | null>(null)
  const dryRunScrapeOptions = ref<AutoScrapeOptionsDto | undefined>(undefined)
  const autoScrapeModalOpen = ref(false)
  const autoScrapeTargetEntries = ref<MediaFileEntry[]>([])

  let dirLoadSeq = 0
  let dirAbortController: AbortController | null = null
  let suppressHashSync = false
  let initialLoaded = false

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

  const stats = computed(() => {
    const files = snapshot.value?.files ?? []
    let complete = 0
    let missingCover = 0
    let missingSummary = 0
    let missingTag = 0
    for (const f of files) {
      if (isEntryComplete(f)) complete += 1
      if (!f.completeness.has_cover) missingCover += 1
      if (!f.completeness.has_summary_or_lyrics) missingSummary += 1
      if (!f.completeness.has_nfo_or_tag_block) missingTag += 1
    }
    return {
      total: files.length,
      complete,
      incomplete: files.length - complete,
      missingCover,
      missingSummary,
      missingTag,
    }
  })

  const filterItems = computed(() => [
    { id: 'all' as CompletenessFilter, label: t('tool.filters.all'), count: stats.value.total, icon: LuLayers },
    { id: 'complete' as CompletenessFilter, label: t('tool.filters.complete'), count: stats.value.complete, icon: LuCheckCircle2 },
    { id: 'incomplete' as CompletenessFilter, label: t('tool.filters.incomplete'), count: stats.value.incomplete, icon: LuAlertCircle },
    { id: 'missing_cover' as CompletenessFilter, label: domainLabels.value.missingCover, count: stats.value.missingCover, icon: LuImage },
    { id: 'missing_summary' as CompletenessFilter, label: domainLabels.value.missingSummary, count: stats.value.missingSummary, icon: LuFileText },
    { id: 'missing_tag' as CompletenessFilter, label: domainLabels.value.missingTagBlock, count: stats.value.missingTag, icon: LuSparkles },
  ])

  const hasActiveSearchOrFilter = computed<boolean>(
    () => Boolean(searchQuery.value.trim()) || completenessFilter.value !== 'all',
  )

  const sortOptions = computed(() => [
    { value: 'name', label: t('tool.sort.name') },
    { value: 'title', label: t('tool.sort.title') },
    { value: 'incomplete', label: t('tool.sort.incomplete') },
    { value: 'size', label: t('tool.sort.size') },
    { value: 'modified', label: t('tool.sort.modified') },
  ])

  const filteredFiles = computed<MediaFileEntry[]>(() => {
    const base = snapshot.value?.files ?? []
    const q = searchQuery.value.trim().toLowerCase()
    const focus = workbenchFocusPaths.value
    const showRecursive =
      viewMode.value === 'flat' ||
      hasActiveSearchOrFilter.value ||
      Boolean(focus && focus.size > 0)

    const list = base.filter((f) => {
      if (focus && focus.size > 0 && !focus.has(entryCompositeKey(f))) {
        return false
      }
      if (!showRecursive && parentRelOf(f.rel_path) !== currentRelPath.value) {
        return false
      }
      if (completenessFilter.value === 'complete' && !isEntryComplete(f)) return false
      if (completenessFilter.value === 'incomplete' && isEntryComplete(f)) return false
      if (completenessFilter.value === 'missing_cover' && f.completeness.has_cover) return false
      if (completenessFilter.value === 'missing_summary' && f.completeness.has_summary_or_lyrics)
        return false
      if (completenessFilter.value === 'missing_tag' && f.completeness.has_nfo_or_tag_block)
        return false
      if (q) {
        const hay =
          `${f.rel_path} ${f.file_name} ${f.display_title} ${f.display_subtitle}`.toLowerCase()
        if (!hay.includes(q)) return false
      }
      return true
    })

    const dir = sortDir.value === 'asc' ? 1 : -1
    return [...list].sort((a, b) => {
      let cmp = 0
      if (sortField.value === 'title') {
        cmp = a.display_title.localeCompare(b.display_title, undefined, { numeric: true })
      } else if (sortField.value === 'incomplete') {
        cmp = completenessScore(a) - completenessScore(b)
      } else if (sortField.value === 'size') {
        cmp = a.file_size - b.file_size
      } else if (sortField.value === 'modified') {
        cmp = a.modified_unix_secs - b.modified_unix_secs
      } else {
        cmp = a.file_name.localeCompare(b.file_name, undefined, { numeric: true })
      }
      if (cmp === 0) {
        cmp = a.rel_path.localeCompare(b.rel_path, undefined, { numeric: true })
      }
      return cmp * dir
    })
  })

  const visibleFiles = computed<MediaFileEntry[]>(() =>
    filteredFiles.value.slice(0, visibleLimit.value),
  )

  watch(
    [searchQuery, completenessFilter, viewMode, sortField, sortDir, activeRootId, currentRelPath],
    () => {
      visibleLimit.value = PAGE_CHUNK_SIZE
    },
  )

  watch(searchQuery, (q) => {
    if (q.trim() && activeNav.value !== 'items') {
      void navigatePrimary('items')
    }
  })

  watch(
    () => [activeDomain.value, domainRoots.value.map((r) => r.id).join(',')] as const,
    ([dom, rootsKey], [prevDom, prevRootsKey]) => {
      if (!initialLoaded) return
      if (dom !== prevDom || rootsKey === prevRootsKey) return
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

  const isMultiRootSummary = computed<boolean>(
    () => !activeRootId.value && domainRoots.value.length > 1,
  )

  function entrySelectionKey(entry: Pick<MediaFileEntry, 'root_id' | 'rel_path'>): string {
    return isMultiRootSummary.value ? `${entry.root_id}::${entry.rel_path}` : entry.rel_path
  }

  function parseSelectionKey(key: string): { rootId: string; relPath: string } {
    const sep = key.indexOf('::')
    if (sep !== -1) {
      return { rootId: key.slice(0, sep), relPath: key.slice(sep + 2) }
    }
    const fallback = activeRootId.value || snapshot.value?.root_id || domainRoots.value[0]?.id || ''
    return { rootId: fallback, relPath: key }
  }

  function resolveModalRootId(entries: MediaFileEntry[]): string | null {
    const uniqueRoots = new Set(entries.map((e) => e.root_id).filter(Boolean))
    if (uniqueRoots.size > 1) return null
    if (uniqueRoots.size === 1) return Array.from(uniqueRoots)[0]!
    return activeRootId.value || snapshot.value?.root_id || domainRoots.value[0]?.id || ''
  }

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

  const isFolderManagerMode = computed<boolean>(
    () =>
      viewMode.value === 'folders' &&
      !hasActiveSearchOrFilter.value &&
      !(workbenchFocusPaths.value && workbenchFocusPaths.value.size > 0),
  )

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

  const checkedFiles = computed<MediaFileEntry[]>(() => {
    if (selectedPaths.value.size === 0) return []
    if (isFolderManagerMode.value && !isMultiRootSummary.value) {
      const folderPrefixes: string[] = []
      for (const p of selectedPaths.value) {
        if (currentFolderMap.value.has(p)) {
          folderPrefixes.push(`${p}/`)
        }
      }
      const allFiles = snapshot.value?.files ?? []
      return allFiles.filter(
        (f) =>
          selectedPaths.value.has(f.rel_path) ||
          folderPrefixes.some((prefix) => f.rel_path.startsWith(prefix)),
      )
    }
    return filteredFiles.value.filter((f) => selectedPaths.value.has(entrySelectionKey(f)))
  })

  const selectedEntriesForRename = computed<MediaFileEntry[]>(() => {
    if (checkedFiles.value.length > 0) {
      return checkedFiles.value
    }
    if (inspectorStore.activeDetail) {
      const match = filteredFiles.value.find(
        (f) =>
          f.root_id === inspectorStore.activeDetail?.entry.root_id &&
          f.rel_path === inspectorStore.activeDetail?.entry.rel_path,
      )
      if (match) return [match]
    }
    return []
  })

  const selectedBookEntries = computed(() =>
    checkedFiles.value.filter(
      (f) =>
        f.media_kind === 'book' &&
        (f.extension === 'txt' || f.extension === 'epub' || f.extension === 'pdf'),
    ),
  )
  const selectedComicEntries = computed(() =>
    selectedEntriesForRename.value.filter((e) => e.media_kind === 'comic'),
  )
  const selectedMusicEntries = computed(() =>
    selectedEntriesForRename.value.filter((e) => e.media_kind === 'music'),
  )
  const selectedVideoEntries = computed(() =>
    selectedEntriesForRename.value.filter((e) => e.media_kind === 'video'),
  )

  const allFilteredSelected = computed(
    () =>
      filteredFiles.value.length > 0 &&
      filteredFiles.value.every((f) => selectedPaths.value.has(entrySelectionKey(f))),
  )

  const fallbackRootId = computed<string>(
    () => activeRootId.value || snapshot.value?.root_id || domainRoots.value[0]?.id || '',
  )
  const effectiveModalRootId = computed<string>(
    () =>
      resolveModalRootId(renameTargetEntries.value ?? selectedEntriesForRename.value) ||
      fallbackRootId.value,
  )
  const effectiveBookNormalizeRootId = computed<string>(
    () => resolveModalRootId(bookNormalizeEntries.value) || fallbackRootId.value,
  )
  const effectiveMusicRootId = computed<string>(
    () => resolveModalRootId(musicToolsEntries.value) || fallbackRootId.value,
  )
  const effectiveVideoRootId = computed<string>(
    () => resolveModalRootId(videoToolsEntries.value) || fallbackRootId.value,
  )
  const effectiveComicRootId = computed<string>(
    () => resolveModalRootId(selectedComicEntries.value) || fallbackRootId.value,
  )
  const effectiveDryRunRootId = computed<string>(
    () =>
      resolveModalRootId(dryRunTargetEntries.value ?? selectedEntriesForRename.value) ||
      fallbackRootId.value,
  )
  const effectiveHistoryRootId = computed<string>(
    () =>
      activeRootId.value ||
      inspectorStore.activeDetail?.entry.root_id ||
      snapshot.value?.root_id ||
      domainRoots.value[0]?.id ||
      '',
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

  function syncHashFromState(replace = false): void {
    if (suppressHashSync || typeof window === 'undefined') return
    const scrapeStore = useToolScrapeStore()
    const nextHash = buildToolHash({
      domain: activeDomain.value,
      nav: activeNav.value,
      rootId: activeRootId.value,
      relPath: currentRelPath.value,
      settingsTab: settingsInitialTab.value,
      workbench: scrapeStore.scrapeWorkbenchActive,
      item: inspectorStore.inspectingPath,
    })
    if (window.location.hash !== nextHash) {
      if (replace) {
        window.history.replaceState(null, '', nextHash)
      } else {
        window.history.pushState(null, '', nextHash)
      }
    }
  }

  function syncDomainStateToMemory(dom: MediaKind, rootId: string, relPath: string): void {
    domainNavMemory.value = {
      ...domainNavMemory.value,
      [dom]: { rootId, relPath },
    }
  }

  function findEntryByHashItem(rawItem: string): MediaFileEntry | undefined {
    const files = snapshot.value?.files ?? []
    return files.find(
      (f) =>
        entrySelectionKey(f) === rawItem ||
        entryCompositeKey(f) === rawItem ||
        f.rel_path === rawItem,
    )
  }

  async function loadAllDomainRoots(): Promise<void> {
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
    if (!preserveSelection) {
      selectedPaths.value = new Set()
      lastSelectedRelPath.value = null
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
        const snap = await toolApi.browse(
          only.id,
          '',
          reqDomain,
          true,
          (prog) => {
            if (seq === dirLoadSeq && !controller.signal.aborted) {
              taskStore.updateTask(taskId, prog)
            }
          },
          controller.signal,
        )
        if (seq !== dirLoadSeq || activeDomain.value !== reqDomain || controller.signal.aborted) {
          return
        }
        taskStore.completeTask(taskId, `已加载 ${snap.files.length.toLocaleString()} 个媒体文件`)
        snapshot.value = snap
        rootFileCounts.value = { [only.id]: snap.files.length }
        currentRelPath.value = ''
        syncDomainStateToMemory(reqDomain, '', '')
        syncHashFromState(true)
        if (preserveSelection && selectedPaths.value.size > 0) {
          const surviving = new Set<string>([
            ...snap.files.map((f) => f.rel_path),
            ...(snap.raw_files ?? []).map((f) => f.rel_path),
            ...(snap.folders ?? []).map((d) => d.rel_path),
          ])
          selectedPaths.value = new Set(
            [...selectedPaths.value].filter((p) => surviving.has(p)),
          )
        }
        if (
          inspectorStore.inspectingPath &&
          !snap.files.some((f) => f.rel_path === inspectorStore.inspectingPath)
        ) {
          inspectorStore.clearActiveDetail()
          syncHashFromState(true)
        }
        return
      }

      const snaps = await Promise.all(
        roots.map((r) => toolApi.browse(r.id, '', reqDomain, true, undefined, controller.signal)),
      )
      if (seq !== dirLoadSeq || activeDomain.value !== reqDomain || controller.signal.aborted) {
        return
      }
      const counts: Record<string, number> = {}
      const mergedFiles: MediaFileEntry[] = []
      for (let i = 0; i < roots.length; i += 1) {
        const r = roots[i]!
        const s = snaps[i]!
        counts[r.id] = s.files.length
        mergedFiles.push(...s.files)
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
      syncHashFromState(true)

      if (preserveSelection && selectedPaths.value.size > 0) {
        const surviving = new Set<string>(mergedFiles.map((f) => `${f.root_id}::${f.rel_path}`))
        selectedPaths.value = new Set(
          [...selectedPaths.value].filter((p) => surviving.has(p)),
        )
      }
      if (
        inspectorStore.inspectingPath &&
        !mergedFiles.some((f) => `${f.root_id}::${f.rel_path}` === inspectorStore.inspectingPath)
      ) {
        inspectorStore.clearActiveDetail()
        syncHashFromState(true)
      }
    } catch (err) {
      if (isAbortError(err, controller)) return
      const msg = err instanceof Error ? err.message : String(err)
      if (seq === dirLoadSeq && activeDomain.value === reqDomain) {
        taskStore.failTask(taskId, msg)
        dirError.value = msg
      }
    } finally {
      if (seq === dirLoadSeq && activeDomain.value === reqDomain) {
        loadingDir.value = false
      }
    }
  }

  async function loadDirectory(rootId: string, relPath: string): Promise<void> {
    if (!rootId) return
    const seq = ++dirLoadSeq
    const controller = nextDirAbortSignal()
    const reqDomain = activeDomain.value
    const preserveSelection = rootId === activeRootId.value && relPath === currentRelPath.value
    loadingDir.value = true
    dirError.value = null
    if (!preserveSelection) {
      selectedPaths.value = new Set()
      lastSelectedRelPath.value = null
    }

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
              if (seq === dirLoadSeq && !controller.signal.aborted) {
                taskStore.updateTask(taskId, prog)
              }
            },
            controller.signal,
          )
          break
        } catch (err) {
          if (isAbortError(err, controller)) throw err
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
      if (seq !== dirLoadSeq || activeDomain.value !== reqDomain || controller.signal.aborted) {
        return
      }
      if (!snap) {
        taskStore.completeTask(taskId, '已取消')
        return
      }
      taskStore.completeTask(taskId, `已加载 ${snap.files.length.toLocaleString()} 个媒体文件`)
      snapshot.value = snap
      currentRelPath.value = snap.current_rel_path
      syncDomainStateToMemory(reqDomain, rootId, snap.current_rel_path)
      syncHashFromState(true)

      if (preserveSelection && selectedPaths.value.size > 0) {
        const surviving = new Set<string>([
          ...snap.files.map((f) => f.rel_path),
          ...(snap.raw_files ?? []).map((f) => f.rel_path),
          ...(snap.folders ?? []).map((d) => d.rel_path),
        ])
        const pruned = new Set<string>()
        for (const p of selectedPaths.value) {
          if (surviving.has(p)) pruned.add(p)
        }
        selectedPaths.value = pruned
      }

      if (
        inspectorStore.inspectingPath &&
        !snap.files.some((f) => f.rel_path === inspectorStore.inspectingPath)
      ) {
        inspectorStore.clearActiveDetail()
        syncHashFromState(true)
      }
    } catch (err) {
      if (isAbortError(err, controller)) return
      const msg = err instanceof Error ? err.message : String(err)
      if (seq === dirLoadSeq && activeDomain.value === reqDomain) {
        taskStore.failTask(taskId, msg)
        dirError.value = msg
      }
    } finally {
      if (seq === dirLoadSeq && activeDomain.value === reqDomain) {
        loadingDir.value = false
      }
    }
  }

  async function reloadCurrentScope(): Promise<void> {
    if (activeRootId.value) {
      await loadDirectory(activeRootId.value, currentRelPath.value)
    } else {
      await loadAllDomainRoots()
    }
  }

  async function activateDomainWorkspace(dom: MediaKind): Promise<void> {
    const roots = (settings.value?.roots ?? []).filter((r) => r.default_kind === dom)
    selectedPaths.value = new Set()
    inspectorStore.clearActiveDetail()
    completenessFilter.value = 'all'
    searchQuery.value = ''
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

  async function inspectEntry(
    entry: MediaFileEntry,
    options?: { skipDirtyCheck?: boolean; replaceHash?: boolean },
  ): Promise<void> {
    const key = entrySelectionKey(entry)
    if (
      inspectorStore.activeDetail?.entry.root_id === entry.root_id &&
      inspectorStore.activeDetail?.entry.rel_path === entry.rel_path
    ) {
      return
    }
    if (!options?.skipDirtyCheck && !(await inspectorStore.confirmActiveEditorClean())) {
      return
    }
    const seq = inspectorStore.nextInspectSeq()
    inspectorStore.activeDetail = null
    inspectorStore.inspectingPath = key
    inspectorStore.loadingInspector = true
    syncHashFromState(options?.replaceHash ?? true)
    try {
      const detail = await toolApi.inspectFile(entry.root_id, entry.rel_path, activeDomain.value)
      if (seq !== inspectorStore.currentInspectSeq()) return
      inspectorStore.activeDetail = detail
    } catch (err) {
      if (seq === inspectorStore.currentInspectSeq()) {
        inspectorStore.inspectingPath = null
        dirError.value = err instanceof Error ? err.message : String(err)
        syncHashFromState(true)
      }
    } finally {
      if (seq === inspectorStore.currentInspectSeq()) {
        inspectorStore.loadingInspector = false
      }
    }
  }

  function closeInspectorAndSync(): void {
    inspectorStore.clearActiveDetail()
    syncHashFromState(true)
  }

  async function navigatePrimary(nav: 'home' | 'items', keepFilter = false): Promise<void> {
    const scrapeStore = useToolScrapeStore()
    if (
      (activeNav.value !== nav || scrapeStore.scrapeWorkbenchActive) &&
      (nav === 'home' || activeNav.value === 'settings') &&
      !(await inspectorStore.confirmActiveEditorClean())
    ) {
      return
    }
    if (nav !== 'items') {
      scrapeStore.scrapeWorkbenchActive = false
      scrapeStore.scrapeWorkbenchPaths = null
    }
    const wasScopedToSingleRoot = Boolean(activeRootId.value) || Boolean(currentRelPath.value)
    activeNav.value = nav
    previousWorkspaceNav.value = nav
    localStorage.setItem('xmedia_tool.active_nav', nav)
    mobileMenuOpen.value = false
    if (!keepFilter && nav === 'items') {
      completenessFilter.value = 'all'
      workbenchFocusPaths.value = null
    }
    if (activeRootId.value || currentRelPath.value || !snapshot.value) {
      activeRootId.value = ''
      currentRelPath.value = ''
      selectedPaths.value = new Set()
      lastSelectedRelPath.value = null
      if (wasScopedToSingleRoot || !snapshot.value) {
        inspectorStore.clearActiveDetail()
        syncHashFromState()
        await loadAllDomainRoots()
        return
      }
    }
    syncHashFromState()
  }

  async function switchDomain(next: MediaKind): Promise<void> {
    if (activeDomain.value === next) {
      mobileMenuOpen.value = false
      return
    }
    if (!(await inspectorStore.confirmActiveEditorClean())) {
      return
    }
    const scrapeStore = useToolScrapeStore()
    scrapeStore.scrapeWorkbenchActive = false
    scrapeStore.scrapeWorkbenchPaths = null
    activeDomain.value = next
    mobileMenuOpen.value = false
    localStorage.setItem('xmedia_tool.active_domain', next)
    await activateDomainWorkspace(next)
    syncHashFromState()
  }

  async function loadInitial(): Promise<void> {
    loadingInitial.value = true
    initialLoadError.value = null
    const bootHash = parseToolHash()
    const scrapeStore = useToolScrapeStore()
    try {
      const s = await toolApi.getSettings()
      settings.value = s

      if (bootHash.rootId) {
        const matched = s.roots.find((r) => r.id === bootHash.rootId)
        if (matched && matched.default_kind) {
          activeDomain.value = matched.default_kind
          domainNavMemory.value = {
            ...domainNavMemory.value,
            [matched.default_kind]: {
              rootId: matched.id,
              relPath: bootHash.relPath ?? '',
            },
          }
        }
      }

      const currentHasRoots = s.roots.some((r) => r.default_kind === activeDomain.value)
      if (!currentHasRoots) {
        const firstWithRoot = VALID_DOMAINS.find((d) =>
          s.roots.some((r) => r.default_kind === d),
        )
        if (firstWithRoot) {
          activeDomain.value = firstWithRoot
          localStorage.setItem('xmedia_tool.active_domain', firstWithRoot)
        }
      }

      suppressHashSync = true
      try {
        await activateDomainWorkspace(activeDomain.value)
        if (bootHash.workbench) {
          activeNav.value = 'items'
          previousWorkspaceNav.value = 'items'
          scrapeStore.scrapeWorkbenchActive = true
        }
        const targetEntry = bootHash.item
          ? findEntryByHashItem(bootHash.item)
          : bootHash.workbench
            ? filteredFiles.value[0]
            : undefined
        if (targetEntry) {
          await inspectEntry(targetEntry, { skipDirtyCheck: true, replaceHash: true })
        }
      } finally {
        suppressHashSync = false
      }
      syncHashFromState(true)
      initialLoaded = true
    } catch (err) {
      initialLoadError.value = err instanceof Error ? err.message : String(err)
    } finally {
      loadingInitial.value = false
    }
  }

  async function selectRoot(rootId: string, preferredViewMode?: BrowseViewMode): Promise<void> {
    if (!(await inspectorStore.confirmActiveEditorClean())) {
      return
    }
    const scrapeStore = useToolScrapeStore()
    scrapeStore.scrapeWorkbenchActive = false
    scrapeStore.scrapeWorkbenchPaths = null
    mobileMenuOpen.value = false
    completenessFilter.value = 'all'
    workbenchFocusPaths.value = null
    if (preferredViewMode) {
      setViewMode(preferredViewMode)
    }
    activeNav.value = 'items'
    previousWorkspaceNav.value = 'items'
    localStorage.setItem('xmedia_tool.active_nav', 'items')

    const alreadyLoaded =
      activeRootId.value === rootId &&
      currentRelPath.value === '' &&
      snapshot.value?.root_id === rootId
    activeRootId.value = rootId
    currentRelPath.value = ''
    selectedPaths.value = new Set()
    lastSelectedRelPath.value = null
    if (alreadyLoaded) {
      syncHashFromState()
      return
    }
    inspectorStore.clearActiveDetail()
    syncHashFromState()
    await loadDirectory(rootId, '')
  }

  async function enterFolder(relPath: string, forceFoldersView = true): Promise<void> {
    if (!(await inspectorStore.confirmActiveEditorClean())) {
      return
    }
    const scrapeStore = useToolScrapeStore()
    scrapeStore.scrapeWorkbenchActive = false
    scrapeStore.scrapeWorkbenchPaths = null
    mobileMenuOpen.value = false
    workbenchFocusPaths.value = null
    if (forceFoldersView) {
      setViewMode('folders')
      completenessFilter.value = 'all'
    }
    activeNav.value = 'items'
    previousWorkspaceNav.value = 'items'
    localStorage.setItem('xmedia_tool.active_nav', 'items')

    const targetRootId =
      activeRootId.value || snapshot.value?.root_id || domainRoots.value[0]?.id || ''
    if (!targetRootId) return
    if (!activeRootId.value) {
      activeRootId.value = targetRootId
    }
    currentRelPath.value = relPath
    syncHashFromState()
    await loadDirectory(targetRootId, relPath)
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
      const domainChanged = Boolean(parsed.domain && parsed.domain !== activeDomain.value)
      if (parsed.domain) {
        activeDomain.value = parsed.domain
        localStorage.setItem('xmedia_tool.active_domain', parsed.domain)
      }
      if (parsed.nav) {
        activeNav.value = parsed.nav
        if (parsed.nav !== 'settings') {
          previousWorkspaceNav.value = parsed.nav
          localStorage.setItem('xmedia_tool.active_nav', parsed.nav)
        } else if (parsed.settingsTab) {
          settingsInitialTab.value = parsed.settingsTab
        }
      }
      if (domainChanged) {
        if (parsed.rootId) {
          domainNavMemory.value = {
            ...domainNavMemory.value,
            [activeDomain.value]: {
              rootId: parsed.rootId,
              relPath: parsed.relPath ?? '',
            },
          }
        }
        await activateDomainWorkspace(activeDomain.value)
      } else if (
        (parsed.rootId ?? '') !== activeRootId.value ||
        (parsed.relPath ?? '') !== currentRelPath.value
      ) {
        activeRootId.value = parsed.rootId ?? ''
        currentRelPath.value = parsed.relPath ?? ''
        await reloadCurrentScope()
      }

      const nextWorkbench = Boolean(parsed.workbench && activeNav.value === 'items')
      if (!nextWorkbench) {
        scrapeStore.scrapeWorkbenchActive = false
        scrapeStore.scrapeWorkbenchPaths = null
      } else {
        scrapeStore.scrapeWorkbenchActive = true
      }

      if (parsed.item) {
        const hit = findEntryByHashItem(parsed.item)
        if (hit && entrySelectionKey(hit) !== inspectorStore.inspectingPath) {
          await inspectEntry(hit, { skipDirtyCheck: true, replaceHash: true })
        }
      } else if (nextWorkbench && !inspectorStore.activeDetail && filteredFiles.value[0]) {
        await inspectEntry(filteredFiles.value[0], { skipDirtyCheck: true, replaceHash: true })
      } else if (!parsed.item && !nextWorkbench && inspectorStore.activeDetail) {
        inspectorStore.clearActiveDetail()
      }
    } finally {
      suppressHashSync = false
    }
  }

  function setViewMode(next: BrowseViewMode): void {
    if (viewMode.value !== next) {
      selectedPaths.value = new Set()
      lastSelectedRelPath.value = null
    }
    viewMode.value = next
    if (next === 'folders') {
      completenessFilter.value = 'all'
      workbenchFocusPaths.value = null
    }
    localStorage.setItem('xmedia_tool.view_mode', next)
  }

  function setSortField(field: SortField): void {
    sortField.value = field
    localStorage.setItem('xmedia_tool.sort_field', field)
  }

  function toggleSortDir(): void {
    sortDir.value = sortDir.value === 'asc' ? 'desc' : 'asc'
    localStorage.setItem('xmedia_tool.sort_dir', sortDir.value)
  }

  function selectCompletenessFilter(filter: CompletenessFilter): void {
    completenessFilter.value = filter
    if (filter !== 'all') {
      setViewMode('flat')
    }
    void navigatePrimary('items', true)
  }

  function toggleSidebar(isDesktop: boolean): void {
    if (!isDesktop) {
      mobileMenuOpen.value = !mobileMenuOpen.value
      return
    }
    collapsed.value = !collapsed.value
    localStorage.setItem('xmedia_tool.sidebar_collapsed', collapsed.value ? '1' : '0')
  }

  function toggleSelectFile(relPath: string, shiftKey = false, orderedPaths?: string[]): void {
    const next = new Set(selectedPaths.value)
    if (shiftKey && lastSelectedRelPath.value) {
      const listPaths: string[] =
        orderedPaths && orderedPaths.length > 0
          ? orderedPaths
          : isFolderManagerMode.value
            ? [
                ...(snapshot.value?.folders ?? []).map((d) => d.rel_path),
                ...(snapshot.value?.raw_files ?? []).map((f) => f.rel_path),
              ]
            : filteredFiles.value.map((f) => entrySelectionKey(f))
      const fromIdx = listPaths.indexOf(lastSelectedRelPath.value)
      const toIdx = listPaths.indexOf(relPath)
      if (fromIdx !== -1 && toIdx !== -1) {
        const start = Math.min(fromIdx, toIdx)
        const end = Math.max(fromIdx, toIdx)
        const shouldSelect = !next.has(relPath)
        for (let i = start; i <= end; i += 1) {
          const p = listPaths[i]!
          if (shouldSelect) {
            next.add(p)
          } else {
            next.delete(p)
          }
        }
        selectedPaths.value = next
        lastSelectedRelPath.value = relPath
        return
      }
    }
    if (next.has(relPath)) {
      next.delete(relPath)
    } else {
      next.add(relPath)
    }
    selectedPaths.value = next
    lastSelectedRelPath.value = relPath
  }

  function toggleSelectAllFiltered(): void {
    if (allFilteredSelected.value) {
      selectedPaths.value = new Set()
    } else {
      selectedPaths.value = new Set(filteredFiles.value.map((f) => entrySelectionKey(f)))
    }
  }

  function toggleSelectAllRaw(paths: string[]): void {
    const allSelected = paths.length > 0 && paths.every((p) => selectedPaths.value.has(p))
    if (allSelected) {
      selectedPaths.value = new Set()
    } else {
      selectedPaths.value = new Set(paths)
    }
  }

  function clearSelection(): void {
    selectedPaths.value = new Set()
    lastSelectedRelPath.value = null
  }

  async function requestDeleteFiles(paths: string[], deleteCompanions: boolean): Promise<void> {
    const unique = Array.from(new Set(paths.map((p) => p.trim()).filter((p) => p.length > 0)))
    if (unique.length === 0) return

    const folderCount = unique.filter((p) => currentFolderMap.value.has(p)).length
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
      .map((p) => `• ${parseSelectionKey(p).relPath}`)
      .join('\n')
    const moreHint = unique.length > 6 ? `\n……等共 ${unique.length} 项` : ''

    const confirmed = await dialogStore.confirm({
      title,
      message: `${modeHint}\n此操作不可撤销：\n${previewList}${moreHint}`,
      confirmText: `永久删除 (${unique.length})`,
      cancelText: '取消',
      danger: true,
    })
    if (!confirmed) return

    try {
      const grouped = new Map<string, string[]>()
      for (const rawKey of unique) {
        const { rootId, relPath } = parseSelectionKey(rawKey)
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
          if (currentFolderMap.value.has(p)) {
            removedDirKeys.push({ rootId: rid, prefix: `${p}/` })
          }
        }
      }

      const isKeyRemoved = (k: string): boolean => {
        if (removedKeys.has(k)) return true
        const { rootId, relPath } = parseSelectionKey(k)
        return removedDirKeys.some((d) => d.rootId === rootId && relPath.startsWith(d.prefix))
      }

      if (selectedPaths.value.size > 0) {
        const nextSel = new Set<string>()
        for (const p of selectedPaths.value) {
          if (!isKeyRemoved(p)) nextSel.add(p)
        }
        selectedPaths.value = nextSel
      }
      if (
        inspectorStore.activeDetail &&
        isKeyRemoved(entrySelectionKey(inspectorStore.activeDetail.entry))
      ) {
        inspectorStore.clearActiveDetail()
        syncHashFromState(true)
      }
      if (allFailed.length > 0 && totalDeleted === 0) {
        snackbar.show({
          message: allFailed.map((f) => `${f.rel_path}: ${f.error}`).join('；'),
          tone: 'error',
        })
        return
      }
      await reloadCurrentScope()
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
    if (templatesChanged) {
      void reloadCurrentScope()
    }
  }

  async function openSettingsTab(tab: ToolSettingsTab): Promise<void> {
    if (activeNav.value !== 'settings' && !(await inspectorStore.confirmActiveEditorClean())) {
      return
    }
    const scrapeStore = useToolScrapeStore()
    scrapeStore.scrapeWorkbenchActive = false
    scrapeStore.scrapeWorkbenchPaths = null
    settingsInitialTab.value = tab
    if (activeNav.value !== 'settings') {
      previousWorkspaceNav.value = activeNav.value
    }
    activeNav.value = 'settings'
    mobileMenuOpen.value = false
    syncHashFromState()
  }

  function exitSettings(): void {
    activeNav.value = previousWorkspaceNav.value
    syncHashFromState()
  }

  function openHistoryModal(tab: 'metadata' | 'rename' = 'metadata'): void {
    historyInitialTab.value = tab
    historyModalOpen.value = true
  }

  function openRenameModal(entries?: MediaFileEntry[]): void {
    const targets = entries && entries.length > 0 ? entries : selectedEntriesForRename.value
    const pending = targets.filter((e) => !e.completeness.is_organized)
    const skippedCount = targets.length - pending.length
    if (targets.length > 0 && pending.length === 0) {
      snackbar.show({
        message: '所选条目均已按默认模板整理归档，无需重复整理',
      })
      return
    }
    if (pending.length > 0 && !resolveModalRootId(pending)) {
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
      selectedEntriesForRename.value.length > 0 &&
      !resolveModalRootId(selectedEntriesForRename.value)
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
      selectedEntriesForRename.value.length > 0 &&
      !resolveModalRootId(selectedEntriesForRename.value)
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

  function openComicSeriesToolsModal(): void {
    if (selectedComicEntries.value.length > 0 && !resolveModalRootId(selectedComicEntries.value)) {
      snackbar.show({
        message: '所选漫画跨越多个挂载目录，请先从左侧选择单个挂载目录再使用漫画工具',
        tone: 'error',
      })
      return
    }
    comicSeriesToolsOpen.value = true
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
    if (entries.length > 0 && !resolveModalRootId(entries)) {
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
    if (entries.length > 0 && !resolveModalRootId(entries)) {
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
    if (!resolveModalRootId(valid)) {
      snackbar.show({
        message: '所选图书跨越多个挂载目录，请先从左侧选择单个挂载目录再转为规范化 EPUB',
        tone: 'error',
      })
      return
    }
    bookNormalizeEntries.value = valid
    bookNormalizeOpen.value = true
  }

  async function handleRenamed(
    movedCount: number,
    movedMap: Record<string, string>,
  ): Promise<void> {
    const prevActive = inspectorStore.activeDetail
    const mappedRel = prevActive ? movedMap[prevActive.entry.rel_path] : undefined
    const activeEntryRootId = prevActive?.entry.root_id || effectiveModalRootId.value
    if (mappedRel && prevActive) {
      inspectorStore.inspectingPath = entrySelectionKey({
        root_id: activeEntryRootId,
        rel_path: mappedRel,
      })
    }
    if (movedCount > 0 && selectedPaths.value.size > 0) {
      const nextSel = new Set<string>()
      for (const p of selectedPaths.value) {
        const { relPath } = parseSelectionKey(p)
        if (!(relPath in movedMap)) {
          nextSel.add(p)
        }
      }
      selectedPaths.value = nextSel
    }
    renameTargetEntries.value = null
    renameSkippedOrganizedCount.value = 0
    if (movedCount > 0) {
      await reloadCurrentScope()
      snackbar.show({
        message: `已完成 ${movedCount} 个文件的整理与归档`,
        actionLabel: '撤销历史',
        onAction: () => openHistoryModal('rename'),
      })
    }
    if (mappedRel && activeEntryRootId) {
      try {
        const refreshed = await toolApi.inspectFile(
          activeEntryRootId,
          mappedRel,
          activeDomain.value,
        )
        inspectorStore.activeDetail = refreshed
        inspectorStore.inspectingPath = entrySelectionKey(refreshed.entry)
        syncHashFromState(true)
      } catch {
        // Fall back safely if renamed file lives outside filtered branch
      }
    }
  }

  async function handleHistoryRestored(): Promise<void> {
    inspectorStore.artworkVersion += 1
    await reloadCurrentScope()
    if (inspectorStore.activeDetail) {
      const stillHere = (snapshot.value?.files ?? []).find(
        (f) =>
          f.root_id === inspectorStore.activeDetail?.entry.root_id &&
          f.rel_path === inspectorStore.activeDetail?.entry.rel_path,
      )
      if (stillHere) {
        inspectorStore.activeDetail = null
        await inspectEntry(stillHere, { skipDirtyCheck: true, replaceHash: true })
      } else {
        closeInspectorAndSync()
      }
    }
  }

  async function handleBatchMetadataCompleted(report: BatchMetadataEditReportDto): Promise<void> {
    const targeted = selectedEntriesForRename.value
    if (targeted.length > 0) {
      for (const entry of targeted) {
        inspectorStore.bumpFileArtworkVersion(entry.root_id, entry.rel_path)
      }
    } else {
      inspectorStore.artworkVersion += 1
    }
    await reloadCurrentScope()
    if (inspectorStore.activeDetail) {
      const stillHere = (snapshot.value?.files ?? []).find(
        (f) =>
          f.root_id === inspectorStore.activeDetail?.entry.root_id &&
          f.rel_path === inspectorStore.activeDetail?.entry.rel_path,
      )
      if (stillHere) {
        inspectorStore.activeDetail = null
        void inspectEntry(stillHere, { skipDirtyCheck: true, replaceHash: true })
      }
    }
    const batchId = report.batch_id
    snackbar.show({
      message: `批量编辑完成：更新 ${report.updated_count} 项${
        report.skipped_count > 0 ? `，跳过 ${report.skipped_count} 项` : ''
      }${report.failed_count > 0 ? `，失败 ${report.failed_count} 项` : ''}`,
      actionLabel: batchId ? '撤销' : '历史',
      onAction: async () => {
        if (batchId) {
          const res = await toolApi.undoMetadataBatch(batchId)
          await handleHistoryRestored()
          snackbar.show(`已撤销本次批量修改，恢复 ${res.restored_count} 个文件`)
        } else {
          openHistoryModal('metadata')
        }
      },
    })
  }

  async function handleTxtConvertedToEpub(
    epubDetail: import('../types').MediaFileDetail,
    deletedSourceTxt: boolean,
  ): Promise<void> {
    inspectorStore.bumpFileArtworkVersion(epubDetail.entry.root_id, epubDetail.entry.rel_path)
    await reloadCurrentScope()
    inspectorStore.activeDetail = epubDetail
    inspectorStore.inspectingPath = entrySelectionKey(epubDetail.entry)
    bookEditorEntry.value = epubDetail.entry
    syncHashFromState(true)
    snackbar.show(
      deletedSourceTxt
        ? `已转换为 EPUB 并移除原 TXT：${epubDetail.entry.file_name}`
        : `已生成标准 EPUB：${epubDetail.entry.file_name}`,
    )
  }

  async function handleBookNormalizeCompleted(
    report: NormalizeBooksBatchReportDto,
  ): Promise<void> {
    const rootId = effectiveBookNormalizeRootId.value
    for (const it of report.items) {
      if (!it.error) {
        inspectorStore.bumpFileArtworkVersion(rootId, it.output_rel_path || it.source_rel_path)
      }
    }
    await reloadCurrentScope()
    if (inspectorStore.activeDetail) {
      const matchedItem = report.items.find(
        (it) => !it.error && it.source_rel_path === inspectorStore.activeDetail?.entry.rel_path,
      )
      const targetRel = matchedItem?.output_rel_path ?? inspectorStore.activeDetail.entry.rel_path
      const refreshed = (snapshot.value?.files ?? []).find(
        (f) => f.root_id === inspectorStore.activeDetail?.entry.root_id && f.rel_path === targetRel,
      )
      if (refreshed) {
        inspectorStore.activeDetail = null
        await inspectEntry(refreshed, { skipDirtyCheck: true, replaceHash: true })
      }
    }
    snackbar.show(
      report.failed > 0
        ? `规范化 EPUB 完成：成功 ${report.succeeded} 本，失败 ${report.failed} 本`
        : `规范化 EPUB 完成：共处理 ${report.succeeded} 本`,
    )
  }

  return {
    activeDomain,
    activeNav,
    previousWorkspaceNav,
    settingsInitialTab,
    collapsed,
    mobileMenuOpen,
    tasksDrawerOpen,
    viewMode,
    sortField,
    sortDir,
    visibleLimit,
    completenessFilter,
    searchQuery,
    workbenchFocusPaths,
    notifyingRescan,
    settings,
    initialLoadError,
    loadingInitial,
    activeRootId,
    currentRelPath,
    snapshot,
    rootFileCounts,
    loadingDir,
    dirError,
    selectedPaths,
    lastSelectedRelPath,
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
    musicToolsOpen,
    musicToolsEntries,
    videoToolsOpen,
    videoToolsEntries,
    dryRunModalOpen,
    dryRunTargetEntries,
    dryRunScrapeOptions,
    autoScrapeModalOpen,
    autoScrapeTargetEntries,
    isCompact,
    domainRoots,
    domainLabels,
    stats,
    filterItems,
    hasActiveSearchOrFilter,
    sortOptions,
    filteredFiles,
    visibleFiles,
    isMultiRootSummary,
    entrySelectionKey,
    parseSelectionKey,
    resolveModalRootId,
    breadcrumbs,
    isFolderManagerMode,
    currentFolderMap,
    checkedFiles,
    selectedEntriesForRename,
    selectedBookEntries,
    selectedComicEntries,
    selectedMusicEntries,
    selectedVideoEntries,
    allFilteredSelected,
    effectiveModalRootId,
    effectiveBookNormalizeRootId,
    effectiveMusicRootId,
    effectiveVideoRootId,
    effectiveComicRootId,
    effectiveDryRunRootId,
    effectiveHistoryRootId,
    anyModalOpen,
    syncHashFromState,
    loadInitial,
    loadAllDomainRoots,
    loadDirectory,
    reloadCurrentScope,
    activateDomainWorkspace,
    inspectEntry,
    closeInspectorAndSync,
    navigatePrimary,
    switchDomain,
    selectRoot,
    enterFolder,
    handleHashChange,
    setViewMode,
    setSortField,
    toggleSortDir,
    selectCompletenessFilter,
    toggleSidebar,
    toggleSelectFile,
    toggleSelectAllFiltered,
    toggleSelectAllRaw,
    clearSelection,
    requestDeleteFiles,
    triggerManualRescan,
    handleSettingsUpdated,
    openSettingsTab,
    exitSettings,
    openHistoryModal,
    openRenameModal,
    closeRenameModal,
    openBatchMetadataModal,
    openDryRunModal,
    closeDryRunModal,
    openComicSeriesToolsModal,
    openBookContentEditor,
    openComicPageManager,
    openMusicToolbox,
    openVideoToolbox,
    openBookNormalizeModal,
    handleRenamed,
    handleHistoryRestored,
    handleBatchMetadataCompleted,
    handleTxtConvertedToEpub,
    handleBookNormalizeCompleted,
  }
})

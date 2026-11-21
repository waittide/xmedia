<script setup lang="ts">
import { computed, defineAsyncComponent, onBeforeUnmount, onMounted, reactive, ref, watch, type Component } from 'vue'
import { LuBookOpen, LuClapperboard, LuDisc3, LuLayers } from 'vue-icons-plus/lu'
import { useSnackbarStore } from '@/stores/snackbar'
import type { MediaKind } from '@/types/api'
import { toolApi } from '../api'
import {
  formatAutoScrapeSummary,
  loadLastAutoScrapeOptions,
  saveLastAutoScrapeOptions,
} from '../autoScrapePrefs'
import { entryCompositeKey } from '../completeness'
import { useToolText } from '../i18n'
import { useToolTaskStore } from '../tasks'
import type {
  AutoScrapeOptionsDto,
  BatchMetadataEditReportDto,
  DirectorySnapshot,
  ManagedRootDto,
  MediaFileDetail,
  MediaFileEntry,
  NormalizeBooksBatchReportDto,
  PublicSettingsDto,
  ToolStatusDto,
} from '../types'
import type MetadataInspectorDrawerType from './MetadataInspectorDrawer.vue'
import type ScrapeWorkbenchViewType from './scrape/ScrapeWorkbenchView.vue'
import ToolHomeView from './ToolHomeView.vue'
import ToolItemsBrowser from './ToolItemsBrowser.vue'
import type { ToolSettingsTab } from './ToolSettingsView.vue'
import ToolSidebar from './ToolSidebar.vue'
import type {
  InspectTaskItemPayload,
  ReviewItemsPayload,
} from './ToolTasksDrawer.vue'
import ToolTopHeader from './ToolTopHeader.vue'
import {
  PAGE_CHUNK_SIZE,
  isTypingTarget,
  loadSavedDomain,
  loadSavedNav,
  loadSavedSortField,
  loadSavedViewMode,
  parseToolHash,
  type BrowseViewMode,
  type CompletenessFilter,
  type PrimaryNavKey,
  type SortDir,
  type SortField,
} from './workspace/hashState'
import ToolDeleteConfirmDialog from './workspace/ToolDeleteConfirmDialog.vue'
import ToolMobileBottomNav from './workspace/ToolMobileBottomNav.vue'
import { useWorkspaceDelete } from './workspace/useWorkspaceDelete'
import { useWorkspaceDirectory } from './workspace/useWorkspaceDirectory'
import { useWorkspaceFiltering } from './workspace/useWorkspaceFiltering'

const AutoScrapeOptionsModal = defineAsyncComponent(() => import('./AutoScrapeOptionsModal.vue'))
const BatchMetadataModal = defineAsyncComponent(() => import('./BatchMetadataModal.vue'))
const BatchRenameModal = defineAsyncComponent(() => import('./BatchRenameModal.vue'))
const MetadataHistoryModal = defineAsyncComponent(() => import('./MetadataHistoryModal.vue'))
const BookContentEditorModal = defineAsyncComponent(() => import('./BookContentEditorModal.vue'))
const BookNormalizeEpubModal = defineAsyncComponent(() => import('./BookNormalizeEpubModal.vue'))
const ComicPageManagerModal = defineAsyncComponent(() => import('./ComicPageManagerModal.vue'))
const ComicSeriesToolsModal = defineAsyncComponent(() => import('./ComicSeriesToolsModal.vue'))
const MusicToolboxModal = defineAsyncComponent(() => import('./MusicToolboxModal.vue'))
const DryRunScrapeModal = defineAsyncComponent(() => import('./DryRunScrapeModal.vue'))
const VideoToolboxModal = defineAsyncComponent(() => import('./VideoToolboxModal.vue'))
const MetadataInspectorDrawer = defineAsyncComponent(() => import('./MetadataInspectorDrawer.vue'))
const ScrapeQueueSidebar = defineAsyncComponent(() => import('./scrape/ScrapeQueueSidebar.vue'))
const ScrapeWorkbenchView = defineAsyncComponent(() => import('./scrape/ScrapeWorkbenchView.vue'))
const ToolSettingsView = defineAsyncComponent(() => import('./ToolSettingsView.vue'))
const ToolTasksDrawer = defineAsyncComponent(() => import('./ToolTasksDrawer.vue'))

defineProps<{
  status: ToolStatusDto | null
}>()

const emit = defineEmits<{
  lock: []
}>()

const taskStore = useToolTaskStore()
const snackbar = useSnackbarStore()
const txt = useToolText()

const initialHash = parseToolHash()

const activeNav = ref<PrimaryNavKey>(loadSavedNav(initialHash))
const previousWorkspaceNav = ref<'home' | 'items'>(
  activeNav.value === 'settings' ? 'home' : activeNav.value,
)
const tasksDrawerOpen = ref(false)
const tasksDrawerMounted = ref(false)
watch(tasksDrawerOpen, (isOpen) => {
  if (isOpen) tasksDrawerMounted.value = true
})
const collapsed = ref(localStorage.getItem('xmedia_tool.sidebar_collapsed') === '1')
const desktopMediaQuery =
  typeof window !== 'undefined' ? window.matchMedia('(min-width: 840px)') : null
const isDesktop = ref(desktopMediaQuery ? desktopMediaQuery.matches : true)
function handleDesktopBreakpointChange(ev: MediaQueryListEvent): void {
  isDesktop.value = ev.matches
  if (ev.matches) {
    mobileMenuOpen.value = false
  }
}

const viewMode = ref<BrowseViewMode>(loadSavedViewMode())
const sortField = ref<SortField>(loadSavedSortField())
const sortDir = ref<SortDir>(
  localStorage.getItem('xmedia_tool.sort_dir') === 'desc' ? 'desc' : 'asc',
)
const visibleLimit = ref(PAGE_CHUNK_SIZE)
const lastSelectedRelPath = ref<string | null>(null)
const mobileMenuOpen = ref(false)
const activeDomain = ref<MediaKind>(loadSavedDomain(initialHash))
const completenessFilter = ref<CompletenessFilter>('all')
const searchQuery = ref('')
const topHeaderRef = ref<InstanceType<typeof ToolTopHeader> | null>(null)
const notifyingRescan = ref(false)

const settings = ref<PublicSettingsDto | null>(null)
const mainContentScrolled = ref(false)
function onToolMainScroll(ev: Event): void {
  const el = ev.target as HTMLElement | null
  mainContentScrolled.value = Boolean(el && el.scrollTop > 4)
}

const activeRootId = ref<string>('')
const currentRelPath = ref<string>('')
const snapshot = ref<DirectorySnapshot | null>(null)
const rootFileCounts = ref<Record<string, number>>({})
const loadingDir = ref(false)
const dirError = ref<string | null>(null)

const selectedPaths = ref<Set<string>>(new Set())
const inspectingPath = ref<string | null>(null)
const activeDetail = ref<MediaFileDetail | null>(null)
const inspectorRef = ref<InstanceType<typeof MetadataInspectorDrawerType> | null>(null)
const scrapeWorkbenchRef = ref<InstanceType<typeof ScrapeWorkbenchViewType> | null>(null)
const scrapeWorkbenchActive = ref(false)
const scrapeWorkbenchPaths = ref<string[] | null>(null)
const scrapeWorkbenchSavedPaths = ref<Set<string>>(new Set())
const loadingInspector = ref(false)
const artworkVersion = ref(1)
const fileArtworkVersions = reactive<Record<string, number>>({})

function artworkVersionKey(rootId: string, relPath: string): string {
  return `${rootId}::${relPath}`
}

function bumpFileArtworkVersion(rootId: string, relPath: string): void {
  if (!rootId || !relPath) return
  const key = artworkVersionKey(rootId, relPath)
  fileArtworkVersions[key] = (fileArtworkVersions[key] ?? 0) + 1
}

function getFileArtworkVersion(rootId: string, relPath: string): number {
  return artworkVersion.value + (fileArtworkVersions[artworkVersionKey(rootId, relPath)] ?? 0)
}

async function confirmActiveEditorClean(): Promise<boolean> {
  if (scrapeWorkbenchRef.value) {
    return scrapeWorkbenchRef.value.confirmDiscardIfDirty()
  }
  if (inspectorRef.value) {
    return inspectorRef.value.confirmDiscardIfDirty()
  }
  return true
}

const settingsInitialTab = ref<ToolSettingsTab>(initialHash.settingsTab ?? 'appearance')
const renameModalOpen = ref(false)
const renameTargetEntries = ref<MediaFileEntry[] | null>(null)
const renameSkippedOrganizedCount = ref(0)
const batchMetadataModalOpen = ref(false)
const historyModalOpen = ref(false)
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
const workbenchFocusPaths = ref<Set<string> | null>(null)

const isCompact = computed(() => collapsed.value && !mobileMenuOpen.value)

const domainTabs = computed<Array<{ value: MediaKind; label: string; icon: Component }>>(() => [
  { value: 'book', label: txt.value.domains.book, icon: LuBookOpen },
  { value: 'comic', label: txt.value.domains.comic, icon: LuLayers },
  { value: 'music', label: txt.value.domains.music, icon: LuDisc3 },
  { value: 'video', label: txt.value.domains.video, icon: LuClapperboard },
])

const domainLabels = computed(() => txt.value.domainLabels[activeDomain.value])

const domainRoots = computed<ManagedRootDto[]>(() =>
  (settings.value?.roots ?? []).filter((r) => r.default_kind === activeDomain.value),
)

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

const {
  stats,
  filterItems,
  hasActiveSearchOrFilter,
  sortOptions,
  setSortField,
  toggleSortDir,
  filteredFiles,
  visibleFiles,
  entrySelectionKey,
  parseSelectionKey,
  resolveModalRootId,
  breadcrumbs,
  currentFolderMap,
  checkedFiles,
  selectedEntriesForRename,
  allFilteredSelected,
  toggleSelectFile,
  toggleSelectAllFiltered,
  toggleSelectAllRaw,
} = useWorkspaceFiltering({
  snapshot,
  domainRoots,
  activeRootId,
  currentRelPath,
  viewMode,
  sortField,
  sortDir,
  completenessFilter,
  searchQuery,
  workbenchFocusPaths,
  selectedPaths,
  lastSelectedRelPath,
  visibleLimit,
  activeDetail,
  domainLabels,
})

const {
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
  navigatePrimary,
  switchDomain,
  selectRoot,
  enterFolder,
  inspectEntry,
  handleHashChange,
} = useWorkspaceDirectory({
  initialHash,
  settings,
  activeDomain,
  activeNav,
  previousWorkspaceNav,
  settingsInitialTab,
  domainRoots,
  activeRootId,
  currentRelPath,
  snapshot,
  rootFileCounts,
  loadingDir,
  dirError,
  selectedPaths,
  lastSelectedRelPath,
  inspectingPath,
  activeDetail,
  loadingInspector,
  completenessFilter,
  searchQuery,
  mobileMenuOpen,
  workbenchFocusPaths,
  scrapeWorkbenchActive,
  scrapeWorkbenchPaths,
  entrySelectionKey,
  confirmActiveEditorClean,
  setViewMode,
})

const {
  deleteDialogOpen,
  deleteDialogPaths,
  deletingFiles,
  deleteDialogError,
  deleteDialogFolderCount,
  deleteDialogTitle,
  deleteDialogSubtitle,
  requestDeleteFiles,
  confirmDeleteFiles,
} = useWorkspaceDelete({
  currentFolderMap,
  selectedPaths,
  activeDetail,
  parseSelectionKey,
  entrySelectionKey,
  clearActiveDetail,
  reloadCurrentScope,
})

function selectCompletenessFilter(filter: CompletenessFilter): void {
  completenessFilter.value = filter
  if (filter !== 'all') {
    setViewMode('flat')
  }
  void navigatePrimary('items', true)
}

watch(searchQuery, (q) => {
  if (q.trim() && activeNav.value !== 'items') {
    void navigatePrimary('items')
  }
})

const effectiveModalRootId = computed<string>(
  () =>
    resolveModalRootId(renameTargetEntries.value ?? selectedEntriesForRename.value) ||
    activeRootId.value ||
    snapshot.value?.root_id ||
    domainRoots.value[0]?.id ||
    '',
)

const effectiveBookNormalizeRootId = computed<string>(
  () =>
    resolveModalRootId(bookNormalizeEntries.value) ||
    activeRootId.value ||
    snapshot.value?.root_id ||
    domainRoots.value[0]?.id ||
    '',
)

const effectiveMusicRootId = computed<string>(
  () =>
    resolveModalRootId(musicToolsEntries.value) ||
    activeRootId.value ||
    snapshot.value?.root_id ||
    domainRoots.value[0]?.id ||
    '',
)

const effectiveVideoRootId = computed<string>(
  () =>
    resolveModalRootId(videoToolsEntries.value) ||
    activeRootId.value ||
    snapshot.value?.root_id ||
    domainRoots.value[0]?.id ||
    '',
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

const effectiveComicRootId = computed<string>(
  () =>
    resolveModalRootId(selectedComicEntries.value) ||
    activeRootId.value ||
    snapshot.value?.root_id ||
    domainRoots.value[0]?.id ||
    '',
)

const effectiveDryRunRootId = computed<string>(
  () =>
    resolveModalRootId(dryRunTargetEntries.value ?? selectedEntriesForRename.value) ||
    activeRootId.value ||
    snapshot.value?.root_id ||
    domainRoots.value[0]?.id ||
    '',
)

const effectiveHistoryRootId = computed<string>(
  () =>
    activeRootId.value ||
    activeDetail.value?.entry.root_id ||
    snapshot.value?.root_id ||
    domainRoots.value[0]?.id ||
    '',
)

function toggleSidebar(): void {
  if (!isDesktop.value) {
    mobileMenuOpen.value = !mobileMenuOpen.value
    return
  }
  collapsed.value = !collapsed.value
  localStorage.setItem('xmedia_tool.sidebar_collapsed', collapsed.value ? '1' : '0')
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

async function handleDetailUpdated(updated: MediaFileDetail, bumpArt = false): Promise<void> {
  const prevRel = activeDetail.value?.entry.rel_path
  if (bumpArt) {
    bumpFileArtworkVersion(updated.entry.root_id, updated.entry.rel_path)
    if (prevRel && prevRel !== updated.entry.rel_path) {
      bumpFileArtworkVersion(updated.entry.root_id, prevRel)
    }
  }
  activeDetail.value = updated
  inspectingPath.value = entrySelectionKey(updated.entry)

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
    await reloadCurrentScope()
  } else if (snapshot.value) {
    snapshot.value = {
      ...snapshot.value,
      files: snapshot.value.files.map((f) =>
        f.root_id === updated.entry.root_id && f.rel_path === updated.entry.rel_path
          ? updated.entry
          : f,
      ),
    }
  }
}

async function handleRenamed(
  movedCount: number,
  movedMap: Record<string, string>,
): Promise<void> {
  const prevActive = activeDetail.value
  const mappedRel = prevActive ? movedMap[prevActive.entry.rel_path] : undefined
  const activeEntryRootId = prevActive?.entry.root_id || effectiveModalRootId.value
  if (mappedRel && prevActive) {
    inspectingPath.value = entrySelectionKey({ root_id: activeEntryRootId, rel_path: mappedRel })
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
      onAction: () => {
        historyModalOpen.value = true
      },
    })
  }
  if (mappedRel && activeEntryRootId) {
    try {
      const refreshed = await toolApi.inspectFile(
        activeEntryRootId,
        mappedRel,
        activeDomain.value,
      )
      activeDetail.value = refreshed
      inspectingPath.value = entrySelectionKey(refreshed.entry)
    } catch {
      // Fall back safely if renamed file lives outside filtered branch
    }
  }
}

const scrapeStrategyRev = ref(0)
const lastScrapeSummary = computed<string>(() => {
  void scrapeStrategyRev.value
  return formatAutoScrapeSummary(loadLastAutoScrapeOptions(activeDomain.value))
})

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
    checkedFiles.value.length > 0 ? [...checkedFiles.value] : [...filteredFiles.value]
  if (targets.length === 0) return
  const opts = loadLastAutoScrapeOptions(activeDomain.value)
  enqueueScrapeFiles(targets, undefined, opts)
  snackbar.show({
    message: `已按「${formatAutoScrapeSummary(opts)}」策略提交 ${targets.length} 项快刮任务`,
    actionLabel: '调整策略',
    onAction: () => {
      autoScrapeTargetEntries.value = targets
      autoScrapeModalOpen.value = true
    },
  })
}

function configureScrapeSelection(): void {
  const targets =
    checkedFiles.value.length > 0 ? [...checkedFiles.value] : [...filteredFiles.value]
  if (targets.length === 0) return
  autoScrapeTargetEntries.value = targets
  autoScrapeModalOpen.value = true
}

function handleAutoScrapeConfirmed(
  entries: MediaFileEntry[],
  options: AutoScrapeOptionsDto,
): void {
  autoScrapeModalOpen.value = false
  saveLastAutoScrapeOptions(activeDomain.value, options)
  scrapeStrategyRev.value += 1
  enqueueScrapeFiles(entries, undefined, options)
}

function handleAutoScrapeToDryRun(
  entries: MediaFileEntry[],
  options: AutoScrapeOptionsDto,
): void {
  autoScrapeModalOpen.value = false
  saveLastAutoScrapeOptions(activeDomain.value, options)
  scrapeStrategyRev.value += 1
  if (entries.length > 0 && !resolveModalRootId(entries)) {
    snackbar.show({
      message: '所选条目跨越多个挂载目录，请先从左侧选择单个挂载目录再进行置信度预演',
      tone: 'error',
    })
    return
  }
  selectedPaths.value = new Set(entries.map((e) => entrySelectionKey(e)))
  dryRunTargetEntries.value = entries
  dryRunScrapeOptions.value = options
  dryRunModalOpen.value = true
}

async function handleDryRunManualScrape(entries: MediaFileEntry[]): Promise<void> {
  dryRunModalOpen.value = false
  if (entries.length === 0) return
  await openScrapeWorkbench(entries[0], entries)
}

async function handleBatchMetadataCompleted(
  report: BatchMetadataEditReportDto,
): Promise<void> {
  const targeted = selectedEntriesForRename.value
  if (targeted.length > 0) {
    for (const entry of targeted) {
      bumpFileArtworkVersion(entry.root_id, entry.rel_path)
    }
  } else {
    artworkVersion.value += 1
  }
  await reloadCurrentScope()
  if (activeDetail.value) {
    const stillHere = (snapshot.value?.files ?? []).find(
      (f) =>
        f.root_id === activeDetail.value?.entry.root_id &&
        f.rel_path === activeDetail.value?.entry.rel_path,
    )
    if (stillHere) {
      void inspectEntry(stillHere)
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
        historyModalOpen.value = true
      }
    },
  })
}

async function handleHistoryRestored(): Promise<void> {
  artworkVersion.value += 1
  await reloadCurrentScope()
  if (activeDetail.value) {
    const stillHere = (snapshot.value?.files ?? []).find(
      (f) =>
        f.root_id === activeDetail.value?.entry.root_id &&
        f.rel_path === activeDetail.value?.entry.rel_path,
    )
    if (stillHere) {
      void inspectEntry(stillHere)
    } else {
      clearActiveDetail()
    }
  }
}

async function ensureTaskItemDirectoryLoaded(
  domain: MediaKind,
  rootId: string,
): Promise<boolean> {
  if (activeDomain.value !== domain) {
    if (!(await confirmActiveEditorClean())) return false
    await switchDomain(domain)
  }
  if (activeRootId.value !== rootId || currentRelPath.value !== '') {
    if (!(await confirmActiveEditorClean())) return false
    clearActiveDetail()
    activeRootId.value = rootId
    currentRelPath.value = ''
    await loadDirectory(rootId, '')
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
    if (activeDomain.value !== payload.domain) {
      if (!(await confirmActiveEditorClean())) return
      await switchDomain(payload.domain)
    }
    if (activeRootId.value !== '' || currentRelPath.value !== '' || !snapshot.value) {
      if (!(await confirmActiveEditorClean())) return
      clearActiveDetail()
      activeRootId.value = ''
      currentRelPath.value = ''
      await loadAllDomainRoots()
    }
  }

  const orderedKeys = payload.items.map((it) => `${it.rootId}::${it.relPath}`)
  const targetKeySet = new Set(orderedKeys)
  completenessFilter.value = 'all'
  workbenchFocusPaths.value = targetKeySet
  setViewMode('flat')
  activeNav.value = 'items'

  const byCompositeKey = new Map(
    (snapshot.value?.files ?? []).map((f) => [entryCompositeKey(f), f]),
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
    void inspectEntry(firstEntry)
  }
}

async function handleInspectTaskItem(payload: InspectTaskItemPayload): Promise<void> {
  const ok = await ensureTaskItemDirectoryLoaded(payload.domain, payload.rootId)
  if (!ok) return
  completenessFilter.value = 'all'
  workbenchFocusPaths.value = new Set([`${payload.rootId}::${payload.relPath}`])
  setViewMode('flat')
  activeNav.value = 'items'
  if (!payload.openCandidates) {
    scrapeWorkbenchActive.value = false
    scrapeWorkbenchPaths.value = null
  }
  const target = (snapshot.value?.files ?? []).find(
    (f) => f.root_id === payload.rootId && f.rel_path === payload.relPath,
  )
  if (target) {
    if (payload.openCandidates) {
      await openScrapeWorkbench(target, [target])
    } else {
      await inspectEntry(target)
    }
  } else {
    const seq = nextInspectSeq()
    try {
      const detail = await toolApi.inspectFile(payload.rootId, payload.relPath, payload.domain)
      if (seq !== currentInspectSeq()) return
      activeDetail.value = detail
      inspectingPath.value = entrySelectionKey(detail.entry)
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
  const target = (snapshot.value?.files ?? []).find(
    (f) => f.root_id === payload.rootId && f.rel_path === payload.relPath,
  )
  if (target) {
    openBookContentEditor(target)
    return
  }
  try {
    const detail = await toolApi.inspectFile(payload.rootId, payload.relPath, 'book')
    openBookContentEditor(detail.entry)
  } catch (err) {
    snackbar.show({
      message: err instanceof Error ? err.message : String(err),
      tone: 'error',
    })
  }
}

const workbenchQueueEntries = computed<MediaFileEntry[]>(() => {
  const scoped = scrapeWorkbenchPaths.value
  if (!scoped) return filteredFiles.value
  const byKey = new Map((snapshot.value?.files ?? []).map((f) => [entryCompositeKey(f), f]))
  const resolved: MediaFileEntry[] = []
  for (const k of scoped) {
    const hit = byKey.get(k)
    if (hit) resolved.push(hit)
  }
  return resolved.length > 0 ? resolved : filteredFiles.value
})

const activeQueueList = computed<MediaFileEntry[]>(() =>
  scrapeWorkbenchActive.value ? workbenchQueueEntries.value : filteredFiles.value,
)

async function openScrapeWorkbench(
  targetEntry?: MediaFileEntry,
  scopedEntries?: MediaFileEntry[],
): Promise<void> {
  if (!(await confirmActiveEditorClean())) return
  const scopedList = scopedEntries && scopedEntries.length > 0 ? scopedEntries : null
  scrapeWorkbenchPaths.value = scopedList ? scopedList.map((e) => entryCompositeKey(e)) : null
  scrapeWorkbenchSavedPaths.value = new Set()
  scrapeWorkbenchActive.value = true
  activeNav.value = 'items'
  mobileMenuOpen.value = false

  const target =
    targetEntry ??
    (scopedList ? scopedList[0] : activeDetail.value?.entry ?? filteredFiles.value[0])
  if (target) {
    if (
      !activeDetail.value ||
      entryCompositeKey(activeDetail.value.entry) !== entryCompositeKey(target)
    ) {
      await inspectEntry(target)
    }
  }
}

async function exitScrapeWorkbench(): Promise<void> {
  if (!(await confirmActiveEditorClean())) return
  scrapeWorkbenchActive.value = false
  scrapeWorkbenchPaths.value = null
  mobileMenuOpen.value = false
}

const inspectingIndex = computed(() => {
  if (!inspectingPath.value) return -1
  return activeQueueList.value.findIndex((f) => entrySelectionKey(f) === inspectingPath.value)
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
  await inspectEntry(target)
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

async function handleBookContentSaved(updated: MediaFileDetail): Promise<void> {
  await handleDetailUpdated(updated, true)
  snackbar.show(`已保存章节目录与正文：${updated.entry.file_name}`)
}

async function handleTxtConvertedToEpub(
  epubDetail: MediaFileDetail,
  deletedSourceTxt: boolean,
): Promise<void> {
  bumpFileArtworkVersion(epubDetail.entry.root_id, epubDetail.entry.rel_path)
  await reloadCurrentScope()
  activeDetail.value = epubDetail
  inspectingPath.value = entrySelectionKey(epubDetail.entry)
  bookEditorEntry.value = epubDetail.entry
  snackbar.show(
    deletedSourceTxt
      ? `已转换为 EPUB 并移除原 TXT：${epubDetail.entry.file_name}`
      : `已生成标准 EPUB：${epubDetail.entry.file_name}`,
  )
}

watch(
  () => [activeDomain.value, domainRoots.value.map((r) => r.id).join(',')] as const,
  ([dom, rootsKey], [prevDom, prevRootsKey]) => {
    if (!isInitialLoaded()) return
    if (dom !== prevDom || rootsKey === prevRootsKey) return
    const roots = domainRoots.value
    if (roots.length === 0) {
      activeRootId.value = ''
      snapshot.value = null
      rootFileCounts.value = {}
      activeDetail.value = null
      inspectingPath.value = null
    } else if (activeRootId.value && !roots.some((r) => r.id === activeRootId.value)) {
      activeRootId.value = ''
      currentRelPath.value = ''
      void loadAllDomainRoots()
    } else if (!activeRootId.value) {
      void loadAllDomainRoots()
    }
  },
)

function handleSettingsUpdated(next: PublicSettingsDto): void {
  const templatesChanged =
    JSON.stringify(settings.value?.rename_templates) !== JSON.stringify(next.rename_templates)
  settings.value = next
  if (templatesChanged) {
    void reloadCurrentScope()
  }
}

async function openSettingsTab(tab: ToolSettingsTab): Promise<void> {
  if (activeNav.value !== 'settings' && !(await confirmActiveEditorClean())) {
    return
  }
  scrapeWorkbenchActive.value = false
  scrapeWorkbenchPaths.value = null
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

async function handleBookNormalizeCompleted(report: NormalizeBooksBatchReportDto): Promise<void> {
  const rootId = effectiveBookNormalizeRootId.value
  for (const it of report.items) {
    if (!it.error) {
      bumpFileArtworkVersion(rootId, it.output_rel_path || it.source_rel_path)
    }
  }
  await reloadCurrentScope()
  if (activeDetail.value) {
    const matchedItem = report.items.find(
      (it) => !it.error && it.source_rel_path === activeDetail.value?.entry.rel_path,
    )
    const targetRel = matchedItem?.output_rel_path ?? activeDetail.value.entry.rel_path
    const refreshed = (snapshot.value?.files ?? []).find(
      (f) => f.root_id === activeDetail.value?.entry.root_id && f.rel_path === targetRel,
    )
    if (refreshed) {
      activeDetail.value = null
      await inspectEntry(refreshed)
    }
  }
  snackbar.show(
    report.failed > 0
      ? `规范化 EPUB 完成：成功 ${report.succeeded} 本，失败 ${report.failed} 本`
      : `规范化 EPUB 完成：共处理 ${report.succeeded} 本`,
  )
}

const anyModalOpen = computed(
  () =>
    tasksDrawerOpen.value ||
    renameModalOpen.value ||
    deleteDialogOpen.value ||
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

function handleKeydown(ev: KeyboardEvent): void {
  if ((ev.metaKey || ev.ctrlKey) && ev.key.toLowerCase() === 'k') {
    ev.preventDefault()
    topHeaderRef.value?.focusSearch()
    return
  }
  if ((ev.metaKey || ev.ctrlKey) && ev.key.toLowerCase() === 's') {
    if (activeDetail.value && inspectorRef.value && !anyModalOpen.value) {
      ev.preventDefault()
      void inspectorRef.value.save()
    }
    return
  }
  if (anyModalOpen.value) return
  if (ev.key === 'Escape') {
    if (mobileMenuOpen.value) {
      ev.preventDefault()
      mobileMenuOpen.value = false
    } else if (activeDetail.value && inspectorRef.value && !scrapeWorkbenchActive.value) {
      ev.preventDefault()
      void inspectorRef.value.requestClose()
    } else if (selectedPaths.value.size > 0) {
      ev.preventDefault()
      selectedPaths.value = new Set()
    }
    return
  }
  if (isTypingTarget(ev.target)) return
  if (
    activeNav.value === 'items' &&
    !scrapeWorkbenchActive.value &&
    (ev.key === 'ArrowDown' || ev.key === 'ArrowUp')
  ) {
    const list = filteredFiles.value
    if (list.length === 0) return
    ev.preventDefault()
    const curIdx = inspectingPath.value
      ? list.findIndex((f) => entrySelectionKey(f) === inspectingPath.value)
      : -1
    const nextIdx =
      ev.key === 'ArrowDown'
        ? Math.min(list.length - 1, curIdx + 1)
        : Math.max(0, curIdx <= 0 ? 0 : curIdx - 1)
    if (nextIdx >= visibleLimit.value) {
      visibleLimit.value = nextIdx + PAGE_CHUNK_SIZE
    }
    const target = list[nextIdx]
    if (target) {
      void inspectEntry(target)
    }
  }
}

function handleBeforeUnload(ev: BeforeUnloadEvent): void {
  if (
    taskStore.activeCount > 0 ||
    inspectorRef.value?.isDirty ||
    scrapeWorkbenchRef.value?.isDirty
  ) {
    ev.preventDefault()
    ev.returnValue = ''
  }
}

let offFileScraped: (() => void) | null = null
let offBatchSettled: (() => void) | null = null
let scrapeRefreshTimer: ReturnType<typeof setTimeout> | null = null

function scheduleScrapeDirectoryRefresh(delayMs = 350): void {
  if (domainRoots.value.length === 0) return
  if (scrapeRefreshTimer) clearTimeout(scrapeRefreshTimer)
  scrapeRefreshTimer = setTimeout(() => {
    scrapeRefreshTimer = null
    if (domainRoots.value.length > 0) {
      void reloadCurrentScope()
    }
  }, delayMs)
}

onMounted(() => {
  window.addEventListener('keydown', handleKeydown)
  window.addEventListener('beforeunload', handleBeforeUnload)
  window.addEventListener('hashchange', handleHashChange)
  desktopMediaQuery?.addEventListener('change', handleDesktopBreakpointChange)
  offFileScraped = taskStore.onFileScraped((updated, prevRelPath) => {
    bumpFileArtworkVersion(updated.entry.root_id, updated.entry.rel_path)
    if (prevRelPath && prevRelPath !== updated.entry.rel_path) {
      bumpFileArtworkVersion(updated.entry.root_id, prevRelPath)
    }
    const moved = Boolean(prevRelPath && prevRelPath !== updated.entry.rel_path)
    const prevKey = entrySelectionKey({
      root_id: updated.entry.root_id,
      rel_path: prevRelPath ?? updated.entry.rel_path,
    })
    const nextKey = entrySelectionKey(updated.entry)

    if (moved) {
      if (selectedPaths.value.has(prevKey)) {
        const nextSel = new Set(selectedPaths.value)
        nextSel.delete(prevKey)
        nextSel.add(nextKey)
        selectedPaths.value = nextSel
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
      Boolean(snapshot.value) &&
      (snapshot.value!.root_id === updated.entry.root_id ||
        (!activeRootId.value && domainRoots.value.some((r) => r.id === updated.entry.root_id)))
    if (matchesCurrentScope && snapshot.value) {
      snapshot.value = {
        ...snapshot.value,
        files: snapshot.value.files.map((f) =>
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
      activeDetail.value?.entry.root_id === updated.entry.root_id &&
      (activeDetail.value.entry.rel_path === prevRelPath ||
        activeDetail.value.entry.rel_path === updated.entry.rel_path)
    ) {
      activeDetail.value = updated
      inspectingPath.value = nextKey
    }
  })

  offBatchSettled = taskStore.onBatchSettled((task) => {
    if (domainRoots.value.length === 0) return
    const scopeRootIds = activeRootId.value
      ? new Set([activeRootId.value])
      : new Set(domainRoots.value.map((r) => r.id))
    const touchesCurrentScope = Boolean(
      task.subItems?.some((it) => scopeRootIds.has(it.rootId)),
    )
    if (!touchesCurrentScope) return
    if ((task.matchedCount ?? 0) > 0 || task.scrapeOptions?.organize_by_template) {
      if (scrapeRefreshTimer) {
        clearTimeout(scrapeRefreshTimer)
        scrapeRefreshTimer = null
      }
      void reloadCurrentScope()
    }
  })
  void loadInitial()
})

onBeforeUnmount(() => {
  window.removeEventListener('keydown', handleKeydown)
  window.removeEventListener('beforeunload', handleBeforeUnload)
  window.removeEventListener('hashchange', handleHashChange)
  desktopMediaQuery?.removeEventListener('change', handleDesktopBreakpointChange)
  if (offFileScraped) offFileScraped()
  if (offBatchSettled) offBatchSettled()
  if (scrapeRefreshTimer) clearTimeout(scrapeRefreshTimer)
})
</script>

<template>
  <div
    class="relative flex h-screen w-screen flex-col overflow-hidden bg-[var(--md-sys-color-surface-container)] text-[var(--md-sys-color-on-surface)] supports-[height:100dvh]:h-dvh"
  >
    <!-- Full-Width Top Bar -->
    <ToolTopHeader
      ref="topHeaderRef"
      :is-compact="isCompact"
      :active-domain="activeDomain"
      :active-nav="activeNav"
      :tasks-open="tasksDrawerOpen"
      :domain-tabs="domainTabs"
      :search-query="searchQuery"
      :search-placeholder="domainLabels.searchPlaceholder"
      :status="status"
      :scrolled="mainContentScrolled"
      @toggle-sidebar="toggleSidebar"
      @navigate-home="navigatePrimary('home')"
      @switch-domain="switchDomain"
      @update:search-query="searchQuery = $event"
      @lock="emit('lock')"
      @toggle-tasks="tasksDrawerOpen = !tasksDrawerOpen"
      @toggle-settings="activeNav === 'settings' ? exitSettings() : openSettingsTab('appearance')"
    />

    <!-- Middle Body: Collapsible Sidebar (Normal Nav OR Scrape Queue) + Flush Rounded Main Canvas -->
    <div class="relative flex min-h-0 flex-1 overflow-hidden">
      <ScrapeQueueSidebar
        v-if="scrapeWorkbenchActive && activeNav === 'items'"
        :mobile-menu-open="mobileMenuOpen"
        :collapsed="collapsed"
        :is-compact="isCompact"
        :entries="workbenchQueueEntries"
        :active-key="activeDetail ? entryCompositeKey(activeDetail.entry) : null"
        :saved-keys="scrapeWorkbenchSavedPaths"
        :artwork-version="artworkVersion"
        :artwork-versions="fileArtworkVersions"
        :has-prev="hasPrevInspect"
        :has-next="hasNextInspect"
        @close-mobile="mobileMenuOpen = false"
        @exit="exitScrapeWorkbench"
        @toggle-collapse="toggleSidebar"
        @select-entry="(entry) => { mobileMenuOpen = false; void inspectEntry(entry) }"
        @step="stepInspectingFile"
      />

      <ToolSidebar
        v-else
        :mobile-menu-open="mobileMenuOpen"
        :collapsed="collapsed"
        :is-compact="isCompact"
        :active-domain="activeDomain"
        :active-nav="activeNav"
        :domain-tabs="domainTabs"
        :total-items="stats.total"
        :domain-roots="domainRoots"
        :active-root-id="activeRootId"
        @close-mobile="mobileMenuOpen = false"
        @switch-domain="switchDomain"
        @navigate-primary="(nav) => navigatePrimary(nav)"
        @open-settings-roots="openSettingsTab('roots')"
        @select-root="(rid) => selectRoot(rid)"
      />

      <!-- Main Rounded Canvas + Supporting Pane -->
      <main
        class="flex min-w-0 flex-1 overflow-hidden bg-[var(--md-sys-color-surface)] md:mr-3 md:mb-3 md:rounded-[24px]"
      >
        <ScrapeWorkbenchView
          v-if="scrapeWorkbenchActive && activeNav === 'items'"
          ref="scrapeWorkbenchRef"
          :detail="activeDetail"
          :loading-detail="loadingInspector"
          :entries="workbenchQueueEntries"
          :saved-keys="scrapeWorkbenchSavedPaths"
          :artwork-version="artworkVersion"
          :artwork-versions="fileArtworkVersions"
          :sidebar-collapsed="collapsed"
          :has-prev="hasPrevInspect"
          :has-next="hasNextInspect"
          :position-label="inspectPositionLabel"
          :settings="settings"
          @exit="exitScrapeWorkbench"
          @toggle-sidebar="toggleSidebar"
          @select-entry="inspectEntry"
          @step="stepInspectingFile"
          @updated="handleDetailUpdated"
          @open-settings-scrapers="openSettingsTab('scrapers')"
          @settings-updated="handleSettingsUpdated"
          @open-history="historyModalOpen = true"
        />

        <template v-else>
          <div class="min-w-0 flex-1 overflow-y-auto" @scroll.passive="onToolMainScroll">
            <div
              v-if="initialLoadError && !settings"
              role="alert"
              class="flex h-full flex-col items-center justify-center gap-3 px-6 text-center"
            >
              <p class="max-w-md text-sm font-medium text-[var(--md-sys-color-error)]">
                {{ initialLoadError }}
              </p>
              <button
                type="button"
                :disabled="loadingInitial"
                class="md-btn md-btn-filled md-state"
                @click="loadInitial()"
              >
                重试加载配置
              </button>
            </div>

            <ToolSettingsView
              v-else-if="activeNav === 'settings'"
              :settings="settings"
              :active-domain="activeDomain"
              :initial-tab="settingsInitialTab"
              :ffmpeg-status="status?.ffmpeg ?? null"
              @back="exitSettings"
              @updated="handleSettingsUpdated"
              @select-domain="switchDomain"
            />

            <ToolHomeView
              v-else-if="activeNav === 'home'"
              :active-domain="activeDomain"
              :domain-roots="domainRoots"
              :active-root-id="activeRootId"
              :snapshot="snapshot"
              :loading-dir="loadingDir"
              :artwork-version="artworkVersion"
              :artwork-versions="fileArtworkVersions"
              :stats="stats"
              @open-items="(f) => selectCompletenessFilter(f ?? 'all')"
              @inspect-file="inspectEntry"
              @scrape-files="enqueueScrapeFiles"
              @open-rename-all="openRenameModal(snapshot?.files ?? [])"
              @open-settings="openSettingsTab"
              @refresh-dir="reloadCurrentScope"
            />

            <ToolItemsBrowser
              v-else
              :active-domain="activeDomain"
              :active-root-id="activeRootId"
              :current-rel-path="currentRelPath"
              :domain-roots="domainRoots"
              :root-file-counts="rootFileCounts"
              :domain-labels="domainLabels"
              :breadcrumbs="breadcrumbs"
              :view-mode="viewMode"
              :has-active-search-or-filter="hasActiveSearchOrFilter"
              :completeness-filter="completenessFilter"
              :filter-items="filterItems"
              :focus-paths="workbenchFocusPaths"
              :filtered-files="filteredFiles"
              :visible-files="visibleFiles"
              :all-filtered-selected="allFilteredSelected"
              :selected-paths="selectedPaths"
              :sort-field="sortField"
              :sort-dir="sortDir"
              :sort-options="sortOptions"
              :checked-files="checkedFiles"
              :selected-comic-entries="selectedComicEntries"
              :selected-music-entries="selectedMusicEntries"
              :selected-video-entries="selectedVideoEntries"
              :selected-entries-for-rename="selectedEntriesForRename"
              :webhook-enabled="Boolean(settings?.xmedia_webhook_enabled)"
              :notifying-rescan="notifyingRescan"
              :loading-dir="loadingDir"
              :dir-error="dirError"
              :snapshot="snapshot"
              :inspecting-path="inspectingPath"
              :artwork-version="artworkVersion"
              :artwork-versions="fileArtworkVersions"
              :last-scrape-summary="lastScrapeSummary"
              @open-settings-roots="openSettingsTab('roots')"
              @select-root="selectRoot"
              @enter-folder="enterFolder"
              @set-view-mode="setViewMode"
              @select-completeness-filter="selectCompletenessFilter"
              @clear-focus-paths="workbenchFocusPaths = null"
              @toggle-select-all="toggleSelectAllFiltered"
              @toggle-select-all-raw="toggleSelectAllRaw"
              @clear-selection="selectedPaths = new Set(); lastSelectedRelPath = null"
              @set-sort-field="setSortField"
              @toggle-sort-dir="toggleSortDir"
              @quick-scrape-selection="quickScrapeSelection"
              @configure-scrape-selection="configureScrapeSelection"
              @open-scrape-workbench="(scoped) => openScrapeWorkbench(undefined, scoped)"
              @open-comic-series-tools="openComicSeriesToolsModal"
              @open-music-toolbox="openMusicToolbox"
              @open-video-toolbox="openVideoToolbox"
              @open-dry-run="openDryRunModal"
              @open-batch-metadata="openBatchMetadataModal"
              @open-history="historyModalOpen = true"
              @open-rename="openRenameModal"
              @request-delete-files="requestDeleteFiles"
              @trigger-rescan="triggerManualRescan"
              @refresh-dir="reloadCurrentScope"
              @inspect-file="inspectEntry"
              @toggle-select-file="toggleSelectFile"
              @open-book-content-editor="openBookContentEditor"
              @open-book-normalize-epub="openBookNormalizeModal"
              @open-comic-page-manager="openComicPageManager"
              @enqueue-scrape-files="enqueueScrapeFiles"
              @load-more="visibleLimit += PAGE_CHUNK_SIZE"
              @show-all="visibleLimit = filteredFiles.length"
            />
          </div>

          <Transition name="md-sheet-right">
            <div
              v-if="
                (activeNav === 'home' || activeNav === 'items') &&
                (activeDetail || loadingInspector)
              "
              class="fixed inset-y-0 right-0 z-40 flex w-full max-w-full bg-[var(--md-sys-color-surface-container-low)] shadow-[var(--md-sys-elevation-3)] lg:static lg:z-auto lg:w-auto lg:shadow-none"
            >
              <div
                v-if="loadingInspector && !activeDetail"
                class="flex h-full w-full items-center justify-center px-12 text-xs text-[var(--md-sys-color-on-surface-variant)] lg:w-[440px]"
              >
                正在解析原文件流与元数据…
              </div>
              <MetadataInspectorDrawer
                v-else-if="activeDetail"
                ref="inspectorRef"
                :detail="activeDetail"
                :artwork-version="getFileArtworkVersion(activeDetail.entry.root_id, activeDetail.entry.rel_path)"
                :has-prev="hasPrevInspect"
                :has-next="hasNextInspect"
                :position-label="inspectPositionLabel"
                :shortcuts-disabled="anyModalOpen"
                @step="stepInspectingFile"
                @close="clearActiveDetail()"
                @updated="handleDetailUpdated"
                @open-workbench="(d) => openScrapeWorkbench(d.entry)"
                @open-content-editor="openBookContentEditor"
                @open-normalize-epub="(e) => openBookNormalizeModal([e])"
                @open-comic-pages="openComicPageManager"
                @open-music-tools="(e) => openMusicToolbox([e])"
                @open-video-tools="(e) => openVideoToolbox([e])"
                @open-rename="(e) => openRenameModal([e])"
                @request-delete="(e) => requestDeleteFiles([entrySelectionKey(e)], true)"
                @open-history="historyModalOpen = true"
              />
            </div>
          </Transition>
        </template>
      </main>
    </div>

    <ToolMobileBottomNav
      :active-nav="activeNav"
      :tasks-drawer-open="tasksDrawerOpen"
      :has-active-task="Boolean(taskStore.activeTask)"
      :unresolved-count="taskStore.unresolvedCount"
      @navigate-primary="(nav) => { tasksDrawerOpen = false; void navigatePrimary(nav) }"
      @toggle-tasks="tasksDrawerOpen = !tasksDrawerOpen"
      @toggle-settings="tasksDrawerOpen = false; activeNav === 'settings' ? exitSettings() : openSettingsTab('appearance')"
    />

    <BatchMetadataModal
      v-if="batchMetadataModalOpen && effectiveModalRootId"
      :open="batchMetadataModalOpen"
      :root-id="effectiveModalRootId"
      :domain="activeDomain"
      :entries="selectedEntriesForRename"
      @close="batchMetadataModalOpen = false"
      @completed="handleBatchMetadataCompleted"
      @open-history="batchMetadataModalOpen = false; historyModalOpen = true"
    />

    <MetadataHistoryModal
      v-if="historyModalOpen"
      :open="historyModalOpen"
      :root-id="effectiveHistoryRootId"
      @close="historyModalOpen = false"
      @restored="handleHistoryRestored"
    />

    <BatchRenameModal
      v-if="renameModalOpen && settings && effectiveModalRootId"
      :open="renameModalOpen"
      :root-id="effectiveModalRootId"
      :entries="renameTargetEntries ?? selectedEntriesForRename"
      :skipped-organized-count="renameSkippedOrganizedCount"
      :templates="settings.rename_templates"
      @close="renameModalOpen = false; renameTargetEntries = null; renameSkippedOrganizedCount = 0"
      @renamed="handleRenamed"
    />

    <BookContentEditorModal
      v-if="bookEditorOpen"
      :open="bookEditorOpen"
      :entry="bookEditorEntry"
      @close="bookEditorOpen = false"
      @saved="handleBookContentSaved"
      @converted="handleTxtConvertedToEpub"
    />

    <BookNormalizeEpubModal
      v-if="bookNormalizeOpen && effectiveBookNormalizeRootId"
      :open="bookNormalizeOpen"
      :root-id="effectiveBookNormalizeRootId"
      :entries="bookNormalizeEntries"
      @close="bookNormalizeOpen = false"
      @completed="handleBookNormalizeCompleted"
    />

    <ComicPageManagerModal
      v-if="comicPageManagerOpen"
      :open="comicPageManagerOpen"
      :entry="comicPageManagerEntry"
      @close="comicPageManagerOpen = false"
      @saved="(d) => handleDetailUpdated(d, true)"
    />

    <ComicSeriesToolsModal
      v-if="comicSeriesToolsOpen"
      :open="comicSeriesToolsOpen"
      :root-id="effectiveComicRootId"
      :entries="selectedComicEntries"
      @close="comicSeriesToolsOpen = false"
      @completed="reloadCurrentScope"
    />

    <MusicToolboxModal
      v-if="musicToolsOpen"
      :open="musicToolsOpen"
      :root-id="effectiveMusicRootId"
      :entries="musicToolsEntries"
      @close="musicToolsOpen = false"
      @updated="(d) => handleDetailUpdated(d, true)"
      @completed="reloadCurrentScope"
    />

    <VideoToolboxModal
      v-if="videoToolsOpen"
      :open="videoToolsOpen"
      :root-id="effectiveVideoRootId"
      :entries="videoToolsEntries"
      @close="videoToolsOpen = false"
      @updated="(d) => handleDetailUpdated(d, true)"
      @completed="reloadCurrentScope"
    />

    <AutoScrapeOptionsModal
      v-if="autoScrapeModalOpen"
      :open="autoScrapeModalOpen"
      :domain="activeDomain"
      :entries="autoScrapeTargetEntries"
      @close="autoScrapeModalOpen = false"
      @start="handleAutoScrapeConfirmed"
      @open-dry-run="handleAutoScrapeToDryRun"
      @open-settings="autoScrapeModalOpen = false; openSettingsTab('scrapers')"
    />

    <DryRunScrapeModal
      v-if="dryRunModalOpen"
      :open="dryRunModalOpen"
      :root-id="effectiveDryRunRootId"
      :entries="dryRunTargetEntries ?? selectedEntriesForRename"
      :options="dryRunScrapeOptions"
      @close="dryRunModalOpen = false; dryRunTargetEntries = null; dryRunScrapeOptions = undefined"
      @confirm="(entries, options) => enqueueScrapeFiles(entries, undefined, options)"
      @manual-scrape="handleDryRunManualScrape"
    />

    <ToolDeleteConfirmDialog
      :open="deleteDialogOpen"
      :title="deleteDialogTitle"
      :subtitle="deleteDialogSubtitle"
      :paths="deleteDialogPaths"
      :folder-count="deleteDialogFolderCount"
      :deleting="deletingFiles"
      :error="deleteDialogError"
      :current-folder-map="currentFolderMap"
      :parse-selection-key="parseSelectionKey"
      @close="deleteDialogOpen = false"
      @confirm="confirmDeleteFiles"
    />

    <ToolTasksDrawer
      v-if="tasksDrawerMounted"
      :open="tasksDrawerOpen"
      @close="tasksDrawerOpen = false"
      @review-items="handleReviewUnmatchedItems"
      @inspect-item="handleInspectTaskItem"
      @open-book-editor="handleOpenTaskBookEditor"
    />
  </div>
</template>

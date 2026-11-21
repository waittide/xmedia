<script setup lang="ts">
import { computed, defineAsyncComponent, onBeforeUnmount, onMounted, reactive, ref, watch, type Component } from 'vue'
import {
  LuAlertCircle,
  LuBookOpen,
  LuCheckCircle2,
  LuClapperboard,
  LuDisc3,
  LuFileText,
  LuFolder,
  LuImage,
  LuLayers,
  LuLoader2,
  LuSparkles,
  LuTrash2,
} from 'vue-icons-plus/lu'
import MdDialog from '@/components/ui/MdDialog.vue'
import { useSnackbarStore } from '@/stores/snackbar'
import type { MediaKind } from '@/types/api'
import { toolApi } from '../api'
import {
  formatAutoScrapeSummary,
  loadLastAutoScrapeOptions,
  saveLastAutoScrapeOptions,
} from '../autoScrapePrefs'
import {
  completenessScore,
  entryCompositeKey,
  isEntryComplete,
  type ToolCompletenessFilter,
} from '../completeness'
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

type PrimaryNavKey = 'home' | 'items' | 'settings'
type BrowseViewMode = 'flat' | 'folders'
type SortField = 'name' | 'title' | 'incomplete' | 'size' | 'modified'
type SortDir = 'asc' | 'desc'
type CompletenessFilter = ToolCompletenessFilter

const VALID_DOMAINS: MediaKind[] = ['book', 'comic', 'music', 'video']
const PAGE_CHUNK_SIZE = 100

interface ParsedToolHash {
  domain?: MediaKind
  nav?: PrimaryNavKey
  rootId?: string
  relPath?: string
  settingsTab?: ToolSettingsTab
}

function parseToolHash(): ParsedToolHash {
  const raw = window.location.hash.replace(/^#\/?/, '')
  if (!raw) return {}
  const [pathPart, queryPart = ''] = raw.split('?')
  const segs = (pathPart ?? '').split('/').filter(Boolean)
  const out: ParsedToolHash = {}
  if (segs[0] && VALID_DOMAINS.includes(segs[0] as MediaKind)) {
    out.domain = segs[0] as MediaKind
  }
  if (segs[1] === 'home' || segs[1] === 'items' || segs[1] === 'settings') {
    out.nav = segs[1]
  } else if (segs[1] === 'tasks') {
    out.nav = 'items'
  }
  const params = new URLSearchParams(queryPart)
  const root = params.get('root')
  if (root) out.rootId = root
  const path = params.get('path')
  if (path !== null) out.relPath = path
  const tab = params.get('tab')
  if (tab === 'appearance' || tab === 'scrapers' || tab === 'roots' || tab === 'templates') {
    out.settingsTab = tab
  }
  return out
}

const initialHash = parseToolHash()

function loadSavedDomain(): MediaKind {
  if (initialHash.domain) return initialHash.domain
  const saved = localStorage.getItem('xmedia_tool.active_domain') as MediaKind | null
  if (saved && VALID_DOMAINS.includes(saved)) {
    return saved
  }
  return 'book'
}

function loadSavedNav(): PrimaryNavKey {
  if (initialHash.nav) return initialHash.nav
  const saved = localStorage.getItem('xmedia_tool.active_nav')
  if (saved === 'home' || saved === 'items') return saved
  return 'home'
}

function loadSavedSortField(): SortField {
  const saved = localStorage.getItem('xmedia_tool.sort_field')
  if (
    saved === 'name' ||
    saved === 'title' ||
    saved === 'incomplete' ||
    saved === 'size' ||
    saved === 'modified'
  ) {
    return saved
  }
  return 'name'
}

const activeNav = ref<PrimaryNavKey>(loadSavedNav())
const previousWorkspaceNav = ref<'home' | 'items'>(
  activeNav.value === 'settings' ? 'home' : activeNav.value,
)
const tasksDrawerOpen = ref(false)
const tasksDrawerMounted = ref(false)
watch(tasksDrawerOpen, (isOpen) => {
  if (isOpen) tasksDrawerMounted.value = true
})
const collapsed = ref(localStorage.getItem('xmedia_tool.sidebar_collapsed') === '1')
function loadSavedViewMode(): BrowseViewMode {
  const saved = localStorage.getItem('xmedia_tool.view_mode')
  if (saved === 'folders') return 'folders'
  return 'flat'
}
const viewMode = ref<BrowseViewMode>(loadSavedViewMode())
const sortField = ref<SortField>(loadSavedSortField())
const sortDir = ref<SortDir>(
  localStorage.getItem('xmedia_tool.sort_dir') === 'desc' ? 'desc' : 'asc',
)
const visibleLimit = ref(PAGE_CHUNK_SIZE)
const lastSelectedRelPath = ref<string | null>(null)
const mobileMenuOpen = ref(false)
const activeDomain = ref<MediaKind>(loadSavedDomain())
const completenessFilter = ref<CompletenessFilter>('all')
const searchQuery = ref('')
const topHeaderRef = ref<InstanceType<typeof ToolTopHeader> | null>(null)
const notifyingRescan = ref(false)
let dirLoadSeq = 0
let inspectSeq = 0
let suppressHashSync = false
let initialLoaded = false

const settings = ref<PublicSettingsDto | null>(null)
const initialLoadError = ref<string | null>(null)
const loadingInitial = ref(false)
const domainNavMemory = ref<Record<MediaKind, { rootId: string; relPath: string }>>({
  book: { rootId: '', relPath: '' },
  comic: { rootId: '', relPath: '' },
  music: { rootId: '', relPath: '' },
  video: { rootId: '', relPath: '' },
})

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
const deleteDialogOpen = ref(false)
const deleteDialogPaths = ref<string[]>([])
const deleteDialogCompanions = ref(false)
const deletingFiles = ref(false)
const deleteDialogError = ref<string | null>(null)
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

function syncHashFromState(replace = false): void {
  if (suppressHashSync) return
  const params = new URLSearchParams()
  if (activeNav.value === 'settings') {
    params.set('tab', settingsInitialTab.value)
  } else {
    if (activeRootId.value) params.set('root', activeRootId.value)
    if (currentRelPath.value) params.set('path', currentRelPath.value)
  }
  const qs = params.toString()
  const nextHash = `#/${activeDomain.value}/${activeNav.value}${qs ? `?${qs}` : ''}`
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
  activeDetail.value = null
  inspectingPath.value = null
  loadingInspector.value = false
}

async function navigatePrimary(nav: 'home' | 'items', keepFilter = false): Promise<void> {
  if (
    (activeNav.value !== nav || scrapeWorkbenchActive.value) &&
    (nav === 'home' || activeNav.value === 'settings') &&
    !(await confirmActiveEditorClean())
  ) {
    return
  }
  if (nav !== 'items') {
    scrapeWorkbenchActive.value = false
    scrapeWorkbenchPaths.value = null
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
      clearActiveDetail()
      syncHashFromState()
      await loadAllDomainRoots()
      return
    }
  }
  syncHashFromState()
}

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
  { id: 'all' as CompletenessFilter, label: txt.value.filters.all, count: stats.value.total, icon: LuLayers },
  { id: 'complete' as CompletenessFilter, label: txt.value.filters.complete, count: stats.value.complete, icon: LuCheckCircle2 },
  { id: 'incomplete' as CompletenessFilter, label: txt.value.filters.incomplete, count: stats.value.incomplete, icon: LuAlertCircle },
  { id: 'missing_cover' as CompletenessFilter, label: domainLabels.value.missingCover, count: stats.value.missingCover, icon: LuImage },
  { id: 'missing_summary' as CompletenessFilter, label: domainLabels.value.missingSummary, count: stats.value.missingSummary, icon: LuFileText },
  { id: 'missing_tag' as CompletenessFilter, label: domainLabels.value.missingTagBlock, count: stats.value.missingTag, icon: LuSparkles },
])

function selectCompletenessFilter(filter: CompletenessFilter): void {
  completenessFilter.value = filter
  if (filter !== 'all') {
    setViewMode('flat')
  }
  void navigatePrimary('items', true)
}

function parentRelOf(relPath: string): string {
  const idx = relPath.lastIndexOf('/')
  return idx === -1 ? '' : relPath.slice(0, idx)
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

watch(searchQuery, (q) => {
  if (q.trim() && activeNav.value !== 'items') {
    navigatePrimary('items')
  }
})

const hasActiveSearchOrFilter = computed<boolean>(
  () => Boolean(searchQuery.value.trim()) || completenessFilter.value !== 'all',
)

const sortOptions = computed(() => [
  { value: 'name', label: txt.value.sort.name },
  { value: 'title', label: txt.value.sort.title },
  { value: 'incomplete', label: txt.value.sort.incomplete },
  { value: 'size', label: txt.value.sort.size },
  { value: 'modified', label: txt.value.sort.modified },
])

function setSortField(field: SortField): void {
  sortField.value = field
  localStorage.setItem('xmedia_tool.sort_field', field)
}

function toggleSortDir(): void {
  sortDir.value = sortDir.value === 'asc' ? 'desc' : 'asc'
  localStorage.setItem('xmedia_tool.sort_dir', sortDir.value)
}

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
      const hay = `${f.rel_path} ${f.file_name} ${f.display_title} ${f.display_subtitle}`.toLowerCase()
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

watch([searchQuery, completenessFilter, viewMode, sortField, sortDir, activeRootId, currentRelPath], () => {
  visibleLimit.value = PAGE_CHUNK_SIZE
})

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
  const fallbackRootId =
    activeRootId.value || snapshot.value?.root_id || domainRoots.value[0]?.id || ''
  return { rootId: fallbackRootId, relPath: key }
}

function resolveModalRootId(entries: MediaFileEntry[]): string | null {
  const uniqueRoots = new Set(entries.map((e) => e.root_id).filter(Boolean))
  if (uniqueRoots.size > 1) return null
  if (uniqueRoots.size === 1) return Array.from(uniqueRoots)[0]!
  return activeRootId.value || snapshot.value?.root_id || domainRoots.value[0]?.id || ''
}

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
  if (activeDetail.value) {
    const match = filteredFiles.value.find(
      (f) =>
        f.root_id === activeDetail.value?.entry.root_id &&
        f.rel_path === activeDetail.value?.entry.rel_path,
    )
    if (match) return [match]
  }
  return []
})

const allFilteredSelected = computed(
  () =>
    filteredFiles.value.length > 0 &&
    filteredFiles.value.every((f) => selectedPaths.value.has(entrySelectionKey(f))),
)

function toggleSidebar(): void {
  if (window.innerWidth < 768) {
    mobileMenuOpen.value = !mobileMenuOpen.value
    return
  }
  collapsed.value = !collapsed.value
  localStorage.setItem('xmedia_tool.sidebar_collapsed', collapsed.value ? '1' : '0')
}

function syncDomainStateToMemory(dom: MediaKind, rootId: string, relPath: string): void {
  domainNavMemory.value = {
    ...domainNavMemory.value,
    [dom]: { rootId, relPath },
  }
}

async function activateDomainWorkspace(dom: MediaKind): Promise<void> {
  const roots = (settings.value?.roots ?? []).filter((r) => r.default_kind === dom)
  selectedPaths.value = new Set()
  clearActiveDetail()
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

async function switchDomain(next: MediaKind): Promise<void> {
  if (activeDomain.value === next) {
    mobileMenuOpen.value = false
    return
  }
  if (!(await confirmActiveEditorClean())) {
    return
  }
  scrapeWorkbenchActive.value = false
  scrapeWorkbenchPaths.value = null
  activeDomain.value = next
  mobileMenuOpen.value = false
  localStorage.setItem('xmedia_tool.active_domain', next)
  await activateDomainWorkspace(next)
  syncHashFromState()
}

async function loadInitial(): Promise<void> {
  loadingInitial.value = true
  initialLoadError.value = null
  try {
    const s = await toolApi.getSettings()
    settings.value = s

    if (initialHash.rootId) {
      const matched = s.roots.find((r) => r.id === initialHash.rootId)
      if (matched && matched.default_kind) {
        activeDomain.value = matched.default_kind
        domainNavMemory.value = {
          ...domainNavMemory.value,
          [matched.default_kind]: {
            rootId: matched.id,
            relPath: initialHash.relPath ?? '',
          },
        }
      }
    }

    const currentHasRoots = s.roots.some((r) => r.default_kind === activeDomain.value)
    if (!currentHasRoots) {
      const firstWithRoot = VALID_DOMAINS.find((d) => s.roots.some((r) => r.default_kind === d))
      if (firstWithRoot) {
        activeDomain.value = firstWithRoot
        localStorage.setItem('xmedia_tool.active_domain', firstWithRoot)
      }
    }
    await activateDomainWorkspace(activeDomain.value)
    syncHashFromState(true)
    initialLoaded = true
  } catch (err) {
    initialLoadError.value = err instanceof Error ? err.message : String(err)
  } finally {
    loadingInitial.value = false
  }
}

async function loadAllDomainRoots(): Promise<void> {
  const reqDomain = activeDomain.value
  const roots = (settings.value?.roots ?? []).filter((r) => r.default_kind === reqDomain)
  if (roots.length === 0) {
    snapshot.value = null
    rootFileCounts.value = {}
    return
  }
  const seq = ++dirLoadSeq
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
      const snap = await toolApi.browse(only.id, '', reqDomain, true, (prog) => {
        taskStore.updateTask(taskId, prog)
      })
      taskStore.completeTask(taskId, `已加载 ${snap.files.length.toLocaleString()} 个媒体文件`)
      if (seq !== dirLoadSeq || activeDomain.value !== reqDomain) return
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
        selectedPaths.value = new Set([...selectedPaths.value].filter((p) => surviving.has(p)))
      }
      if (
        inspectingPath.value &&
        !snap.files.some((f) => f.rel_path === inspectingPath.value)
      ) {
        inspectingPath.value = null
        activeDetail.value = null
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
    if (seq !== dirLoadSeq || activeDomain.value !== reqDomain) return

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
      selectedPaths.value = new Set([...selectedPaths.value].filter((p) => surviving.has(p)))
    }
    if (
      inspectingPath.value &&
      !mergedFiles.some((f) => `${f.root_id}::${f.rel_path}` === inspectingPath.value)
    ) {
      inspectingPath.value = null
      activeDetail.value = null
    }
  } catch (err) {
    const msg = err instanceof Error ? err.message : String(err)
    taskStore.failTask(taskId, msg)
    if (seq === dirLoadSeq && activeDomain.value === reqDomain) {
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

async function loadDirectory(rootId: string, relPath: string): Promise<void> {
  if (!rootId) return
  const seq = ++dirLoadSeq
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
    if (seq !== dirLoadSeq || activeDomain.value !== reqDomain) return
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
      inspectingPath.value &&
      !snap.files.some((f) => f.rel_path === inspectingPath.value)
    ) {
      inspectingPath.value = null
      activeDetail.value = null
    }
  } catch (err) {
    const msg = err instanceof Error ? err.message : String(err)
    taskStore.failTask(taskId, msg)
    if (seq === dirLoadSeq && activeDomain.value === reqDomain) {
      dirError.value = msg
    }
  } finally {
    if (seq === dirLoadSeq && activeDomain.value === reqDomain) {
      loadingDir.value = false
    }
  }
}

async function selectRoot(
  rootId: string,
  preferredViewMode?: BrowseViewMode,
): Promise<void> {
  if (!(await confirmActiveEditorClean())) {
    return
  }
  scrapeWorkbenchActive.value = false
  scrapeWorkbenchPaths.value = null
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
  clearActiveDetail()
  syncHashFromState()
  await loadDirectory(rootId, '')
}

async function enterFolder(relPath: string, forceFoldersView = true): Promise<void> {
  if (!(await confirmActiveEditorClean())) {
    return
  }
  scrapeWorkbenchActive.value = false
  scrapeWorkbenchPaths.value = null
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

async function inspectEntry(entry: MediaFileEntry): Promise<void> {
  const key = entrySelectionKey(entry)
  if (
    activeDetail.value?.entry.root_id === entry.root_id &&
    activeDetail.value?.entry.rel_path === entry.rel_path
  ) {
    return
  }
  if (!(await confirmActiveEditorClean())) {
    return
  }
  const seq = ++inspectSeq
  inspectingPath.value = key
  loadingInspector.value = true
  try {
    const detail = await toolApi.inspectFile(
      entry.root_id,
      entry.rel_path,
      activeDomain.value,
    )
    if (seq !== inspectSeq) return
    activeDetail.value = detail
  } catch (err) {
    if (seq === inspectSeq) {
      dirError.value = err instanceof Error ? err.message : String(err)
    }
  } finally {
    if (seq === inspectSeq) {
      loadingInspector.value = false
    }
  }
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
  if (selectedEntriesForRename.value.length > 0 && !resolveModalRootId(selectedEntriesForRename.value)) {
    snackbar.show({
      message: '所选条目跨越多个挂载目录，请先从左侧选择单个挂载目录再批量编辑',
      tone: 'error',
    })
    return
  }
  batchMetadataModalOpen.value = true
}

function openDryRunModal(): void {
  if (selectedEntriesForRename.value.length > 0 && !resolveModalRootId(selectedEntriesForRename.value)) {
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

function requestDeleteFiles(paths: string[], deleteCompanions: boolean): void {
  const unique = Array.from(new Set(paths.map((p) => p.trim()).filter((p) => p.length > 0)))
  if (unique.length === 0) return
  deleteDialogPaths.value = unique
  deleteDialogCompanions.value = deleteCompanions
  deleteDialogError.value = null
  deleteDialogOpen.value = true
}

const deleteDialogFolderCount = computed<number>(
  () => deleteDialogPaths.value.filter((p) => currentFolderMap.value.has(p)).length,
)

const deleteDialogTitle = computed<string>(() => {
  if (deleteDialogCompanions.value) return '确认删除媒体条目与同名外挂？'
  if (deleteDialogFolderCount.value > 0) {
    return deleteDialogFolderCount.value === deleteDialogPaths.value.length
      ? '确认递归删除所选文件夹？'
      : '确认删除所选文件夹与文件？'
  }
  return '确认删除原始文件？'
})

const deleteDialogSubtitle = computed<string>(() => {
  if (deleteDialogCompanions.value) {
    return '主视图删除模式：同步删除主文件及其专属同名外挂（.opf / .nfo / .lrc / .srt / .ass / 同名封面）'
  }
  if (deleteDialogFolderCount.value > 0) {
    return `文件夹视图删除模式：包含 ${deleteDialogFolderCount.value} 个文件夹，将递归删除目录及其内部全部文件；单个文件仅删除本身`
  }
  return '文件视图删除模式：仅删除所选原始文件本身，不会牵连删除其他外挂文件'
})

async function confirmDeleteFiles(): Promise<void> {
  if (deleteDialogPaths.value.length === 0 || deletingFiles.value) return
  deletingFiles.value = true
  deleteDialogError.value = null
  try {
    const grouped = new Map<string, string[]>()
    for (const rawKey of deleteDialogPaths.value) {
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

    for (const [rid, paths] of grouped.entries()) {
      const report = await toolApi.deleteFiles(rid, paths, deleteDialogCompanions.value)
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
      activeDetail.value &&
      isKeyRemoved(entrySelectionKey(activeDetail.value.entry))
    ) {
      clearActiveDetail()
    }
    if (allFailed.length > 0 && totalDeleted === 0) {
      deleteDialogError.value = allFailed.map((f) => `${f.rel_path}: ${f.error}`).join('；')
      return
    }
    deleteDialogOpen.value = false
    await reloadCurrentScope()
    const compHint =
      totalCompanions > 0 ? `（同步清理 ${totalCompanions} 个同名外挂）` : ''
    const failHint = allFailed.length > 0 ? `，${allFailed.length} 项失败` : ''
    const unit = removedDirKeys.length > 0 ? '项' : '个文件'
    snackbar.show(`已删除 ${totalDeleted} ${unit}${compHint}${failHint}`)
  } catch (err) {
    deleteDialogError.value = err instanceof Error ? err.message : String(err)
  } finally {
    deletingFiles.value = false
  }
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

/**
 * Adds background scrape tasks following:
 * Local non-empty metadata -> High-priority provider -> Low-priority provider
 */
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
  const targets = checkedFiles.value.length > 0 ? [...checkedFiles.value] : [...filteredFiles.value]
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
  const targets = checkedFiles.value.length > 0 ? [...checkedFiles.value] : [...filteredFiles.value]
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
    const seq = ++inspectSeq
    try {
      const detail = await toolApi.inspectFile(payload.rootId, payload.relPath, payload.domain)
      if (seq !== inspectSeq) return
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

const selectedComicEntries = computed(() =>
  selectedEntriesForRename.value.filter((e) => e.media_kind === 'comic'),
)

const selectedMusicEntries = computed(() =>
  selectedEntriesForRename.value.filter((e) => e.media_kind === 'music'),
)

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

const selectedVideoEntries = computed(() =>
  selectedEntriesForRename.value.filter((e) => e.media_kind === 'video'),
)

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
    if (!initialLoaded) return
    // Domain transitions are handled explicitly by switchDomain() / handleHashChange() -> activateDomainWorkspace()
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
  if (
    activeNav.value !== 'settings' &&
    !(await confirmActiveEditorClean())
  ) {
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

function isTypingTarget(target: EventTarget | null): boolean {
  if (!(target instanceof HTMLElement)) return false
  const tag = target.tagName
  return (
    tag === 'INPUT' ||
    tag === 'TEXTAREA' ||
    tag === 'SELECT' ||
    target.isContentEditable
  )
}

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

async function handleHashChange(): Promise<void> {
  const parsed = parseToolHash()
  if (!parsed.domain && !parsed.nav) return
  if (!(await confirmActiveEditorClean())) {
    syncHashFromState(true)
    return
  }
  suppressHashSync = true
  try {
    scrapeWorkbenchActive.value = false
    scrapeWorkbenchPaths.value = null
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
  } finally {
    suppressHashSync = false
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
  if (offFileScraped) offFileScraped()
  if (offBatchSettled) offBatchSettled()
  if (scrapeRefreshTimer) clearTimeout(scrapeRefreshTimer)
})
</script>

<template>
  <div
    class="relative flex h-screen w-screen flex-col overflow-hidden bg-[var(--md-sys-color-surface-container)] text-[var(--md-sys-color-on-surface)]"
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

      <!-- Main Rounded Canvas + Supporting Pane (MD3 Tonal Surface without harsh borders) -->
      <main
        class="flex min-w-0 flex-1 overflow-hidden bg-[var(--md-sys-color-surface)] md:mr-3 md:mb-3 md:rounded-[24px]"
      >
        <!-- FULL-CANVAS 3-COLUMN SCRAPE WORKBENCH MODE -->
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
          <div class="min-w-0 flex-1 overflow-y-auto">
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
          <!-- VIEW 1: IN-PAGE SETTINGS (Server SettingsView Left-Right Layout + Domain Tabs) -->
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

          <!-- VIEW 2: DOMAIN HOME OVERVIEW -->
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

          <!-- VIEW 4: UNIFIED ITEMS BROWSER -->
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

        <!-- Unified MD3 Supporting Pane: Edit + Seamless Expandable Online Candidate Comparison -->
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

    <!-- Safe File Deletion Confirmation Dialog -->
    <MdDialog
      :open="deleteDialogOpen"
      :title="deleteDialogTitle"
      :subtitle="deleteDialogSubtitle"
      max-width="max-w-lg"
      @close="!deletingFiles && (deleteDialogOpen = false)"
    >
      <template #icon>
        <LuTrash2 class="h-5 w-5 text-[var(--md-sys-color-error)]" />
      </template>

      <div class="space-y-3 py-1">
        <div
          v-if="deleteDialogError"
          class="rounded-2xl bg-[var(--md-sys-color-error-container)] px-3.5 py-2.5 text-xs text-[var(--md-sys-color-on-error-container)]"
        >
          {{ deleteDialogError }}
        </div>

        <p class="text-xs leading-relaxed text-[var(--md-sys-color-on-surface-variant)]">
          即将从磁盘<strong class="text-[var(--md-sys-color-error)]">永久删除</strong>以下
          <strong class="font-mono text-[var(--md-sys-color-on-surface)]">{{ deleteDialogPaths.length }}</strong>
          {{ deleteDialogFolderCount > 0 ? '项（含文件夹及其内部全部文件）' : '个文件' }}，此操作不可撤销：
        </p>

        <div
          class="max-h-48 space-y-1.5 overflow-y-auto rounded-2xl border border-[var(--md-sys-color-outline-variant)]/50 bg-[var(--md-sys-color-surface-container)]/60 p-2.5 font-mono text-[11px] text-[var(--md-sys-color-on-surface)]"
        >
          <div
            v-for="p in deleteDialogPaths.slice(0, 12)"
            :key="p"
            class="flex items-center gap-2"
            :title="parseSelectionKey(p).relPath"
          >
            <LuFolder
              v-if="currentFolderMap.has(p)"
              class="h-3.5 w-3.5 shrink-0 text-[var(--md-sys-color-warning)]"
            />
            <span class="min-w-0 flex-1 truncate">{{ parseSelectionKey(p).relPath }}</span>
            <span
              v-if="currentFolderMap.has(p)"
              class="shrink-0 rounded-full bg-[var(--md-sys-color-error-container)] px-1.5 py-0.5 font-sans text-[10px] font-medium text-[var(--md-sys-color-on-error-container)]"
            >
              文件夹 · {{ currentFolderMap.get(p)!.media_file_count }} 项
            </span>
          </div>
          <div
            v-if="deleteDialogPaths.length > 12"
            class="pt-1 text-[10px] text-[var(--md-sys-color-on-surface-variant)]"
          >
            ……以及其他 {{ deleteDialogPaths.length - 12 }} 项
          </div>
        </div>
      </div>

      <template #footer>
        <div class="flex items-center justify-end gap-2">
          <button
            type="button"
            :disabled="deletingFiles"
            class="md-btn md-btn-sm md-btn-text md-state"
            @click="deleteDialogOpen = false"
          >
            取消
          </button>
          <button
            type="button"
            :disabled="deletingFiles"
            class="md-btn md-btn-sm md-state bg-[var(--md-sys-color-error)] text-[var(--md-sys-color-on-error)] hover:opacity-95"
            @click="confirmDeleteFiles"
          >
            <LuLoader2 v-if="deletingFiles" class="h-3.5 w-3.5 animate-spin" />
            <LuTrash2 v-else class="h-3.5 w-3.5" />
            <span>{{ deletingFiles ? '正在删除…' : `确认永久删除 (${deleteDialogPaths.length})` }}</span>
          </button>
        </div>
      </template>
    </MdDialog>

    <!-- Slide-Over Tasks Drawer (Opened from Top Bar Progress Pill or List Inline Banner) -->
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

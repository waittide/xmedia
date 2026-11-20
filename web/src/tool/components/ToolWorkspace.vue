<script setup lang="ts">
import { computed, onBeforeUnmount, onMounted, ref, watch, type Component } from 'vue'
import {
  LuAlertCircle,
  LuBookOpen,
  LuCheckCircle2,
  LuClapperboard,
  LuDisc3,
  LuFileText,
  LuImage,
  LuLayers,
  LuSparkles,
} from 'vue-icons-plus/lu'
import FloatingTaskShelf from '@/components/ui/FloatingTaskShelf.vue'
import { useSnackbarStore } from '@/stores/snackbar'
import type { MediaKind } from '@/types/api'
import { toolApi } from '../api'
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
import AutoScrapeOptionsModal from './AutoScrapeOptionsModal.vue'
import BatchMetadataModal from './BatchMetadataModal.vue'
import BatchRenameModal from './BatchRenameModal.vue'
import MetadataHistoryModal from './MetadataHistoryModal.vue'
import BookContentEditorModal from './BookContentEditorModal.vue'
import BookNormalizeEpubModal from './BookNormalizeEpubModal.vue'
import ComicPageManagerModal from './ComicPageManagerModal.vue'
import ComicSeriesToolsModal from './ComicSeriesToolsModal.vue'
import MusicToolboxModal from './MusicToolboxModal.vue'
import DryRunScrapeModal from './DryRunScrapeModal.vue'
import VideoToolboxModal from './VideoToolboxModal.vue'
import MetadataInspectorDrawer from './MetadataInspectorDrawer.vue'
import OnlineScrapeModal from './OnlineScrapeModal.vue'
import ScrapeQueueSidebar from './scrape/ScrapeQueueSidebar.vue'
import ScrapeWorkbenchView from './scrape/ScrapeWorkbenchView.vue'
import ToolHomeView from './ToolHomeView.vue'
import ToolItemsBrowser from './ToolItemsBrowser.vue'
import ToolSettingsView, { type ToolSettingsTab } from './ToolSettingsView.vue'
import ToolSidebar from './ToolSidebar.vue'
import ToolTasksView, {
  type InspectTaskItemPayload,
  type ReviewItemsPayload,
} from './ToolTasksView.vue'
import ToolTopHeader from './ToolTopHeader.vue'

defineProps<{
  status: ToolStatusDto | null
}>()

const emit = defineEmits<{
  lock: []
}>()

const taskStore = useToolTaskStore()
const snackbar = useSnackbarStore()
const txt = useToolText()

type PrimaryNavKey = 'home' | 'items' | 'tasks' | 'settings'
type BrowseViewMode = 'flat' | 'folders'
type SortField = 'name' | 'title' | 'incomplete' | 'size' | 'modified'
type SortDir = 'asc' | 'desc'
type CompletenessFilter =
  | 'all'
  | 'complete'
  | 'incomplete'
  | 'missing_cover'
  | 'missing_summary'
  | 'missing_tag'

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
  if (segs[1] === 'home' || segs[1] === 'items' || segs[1] === 'tasks' || segs[1] === 'settings') {
    out.nav = segs[1]
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
  if (saved === 'home' || saved === 'items' || saved === 'tasks') return saved
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
const previousWorkspaceNav = ref<'home' | 'items' | 'tasks'>(
  activeNav.value === 'settings' ? 'home' : activeNav.value,
)
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

const settings = ref<PublicSettingsDto | null>(null)
const domainNavMemory = ref<Record<MediaKind, { rootId: string; relPath: string }>>({
  book: { rootId: '', relPath: '' },
  comic: { rootId: '', relPath: '' },
  music: { rootId: '', relPath: '' },
  video: { rootId: '', relPath: '' },
})

const activeRootId = ref<string>('')
const currentRelPath = ref<string>('')
const snapshot = ref<DirectorySnapshot | null>(null)
const loadingDir = ref(false)
const dirError = ref<string | null>(null)

const selectedPaths = ref<Set<string>>(new Set())
const inspectingPath = ref<string | null>(null)
const activeDetail = ref<MediaFileDetail | null>(null)
const inspectorRef = ref<InstanceType<typeof MetadataInspectorDrawer> | null>(null)
const scrapeWorkbenchRef = ref<InstanceType<typeof ScrapeWorkbenchView> | null>(null)
const scrapeWorkbenchActive = ref(false)
const scrapeWorkbenchPaths = ref<string[] | null>(null)
const scrapeWorkbenchSavedPaths = ref<Set<string>>(new Set())
const loadingInspector = ref(false)
const artworkVersion = ref(1)

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
const scrapeModalOpen = ref(false)
const scrapeQueue = ref<MediaFileEntry[]>([])
const scrapeQueueIndex = ref(0)
const renameModalOpen = ref(false)
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

async function navigatePrimary(nav: 'home' | 'items' | 'tasks'): Promise<void> {
  if (
    (activeNav.value !== nav || scrapeWorkbenchActive.value) &&
    (nav === 'tasks' || nav === 'home' || activeNav.value === 'settings') &&
    !(await confirmActiveEditorClean())
  ) {
    return
  }
  if (nav !== 'items') {
    scrapeWorkbenchActive.value = false
    scrapeWorkbenchPaths.value = null
  }
  activeNav.value = nav
  previousWorkspaceNav.value = nav
  localStorage.setItem('xmedia_tool.active_nav', nav)
  mobileMenuOpen.value = false
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

function isEntryComplete(e: MediaFileEntry): boolean {
  const c = e.completeness
  return (
    c.has_title &&
    c.has_creator &&
    c.has_cover &&
    c.has_summary_or_lyrics &&
    c.has_nfo_or_tag_block
  )
}

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
  navigatePrimary('items')
}

function parentRelOf(relPath: string): string {
  const idx = relPath.lastIndexOf('/')
  return idx === -1 ? '' : relPath.slice(0, idx)
}

function setViewMode(next: BrowseViewMode): void {
  viewMode.value = next
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

function completenessScore(e: MediaFileEntry): number {
  const c = e.completeness
  return (
    (c.has_title ? 1 : 0) +
    (c.has_creator ? 1 : 0) +
    (c.has_cover ? 1 : 0) +
    (c.has_summary_or_lyrics ? 1 : 0) +
    (c.has_nfo_or_tag_block ? 1 : 0)
  )
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
    if (focus && focus.size > 0 && !focus.has(f.rel_path)) {
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

const checkedFiles = computed<MediaFileEntry[]>(() => {
  if (selectedPaths.value.size === 0) return []
  return filteredFiles.value.filter((f) => selectedPaths.value.has(f.rel_path))
})

const selectedEntriesForRename = computed<MediaFileEntry[]>(() => {
  if (checkedFiles.value.length > 0) {
    return checkedFiles.value
  }
  if (activeDetail.value) {
    const match = filteredFiles.value.find(
      (f) => f.rel_path === activeDetail.value?.entry.rel_path,
    )
    if (match) return [match]
  }
  return []
})

const allFilteredSelected = computed(
  () =>
    filteredFiles.value.length > 0 &&
    filteredFiles.value.every((f) => selectedPaths.value.has(f.rel_path)),
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
  inspectingPath.value = null
  activeDetail.value = null
  completenessFilter.value = 'all'
  searchQuery.value = ''
  dirError.value = null

  if (roots.length === 0) {
    activeRootId.value = ''
    currentRelPath.value = ''
    snapshot.value = null
    return
  }

  const mem = domainNavMemory.value[dom]
  const matchedRoot = roots.find((r) => r.id === mem.rootId) ?? roots[0]!
  const targetRel = matchedRoot.id === mem.rootId ? mem.relPath : ''
  activeRootId.value = matchedRoot.id
  currentRelPath.value = targetRel
  await loadDirectory(matchedRoot.id, targetRel)
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
      const surviving = new Set(snap.files.map((f) => f.rel_path))
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

async function selectRoot(rootId: string): Promise<void> {
  if (!(await confirmActiveEditorClean())) {
    return
  }
  scrapeWorkbenchActive.value = false
  scrapeWorkbenchPaths.value = null
  mobileMenuOpen.value = false
  if (activeNav.value === 'settings' || activeNav.value === 'tasks') {
    await navigatePrimary('items')
  }
  if (activeRootId.value === rootId && currentRelPath.value === '') return
  activeRootId.value = rootId
  currentRelPath.value = ''
  activeDetail.value = null
  inspectingPath.value = null
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
  if (activeNav.value !== 'items') {
    await navigatePrimary('items')
  }
  currentRelPath.value = relPath
  syncHashFromState()
  await loadDirectory(activeRootId.value, relPath)
}

async function inspectEntry(entry: MediaFileEntry): Promise<void> {
  if (activeDetail.value?.entry.rel_path === entry.rel_path) return
  if (!(await confirmActiveEditorClean())) {
    return
  }
  const seq = ++inspectSeq
  inspectingPath.value = entry.rel_path
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

function toggleSelectFile(relPath: string, shiftKey = false): void {
  const next = new Set(selectedPaths.value)
  if (shiftKey && lastSelectedRelPath.value) {
    const list = filteredFiles.value
    const fromIdx = list.findIndex((f) => f.rel_path === lastSelectedRelPath.value)
    const toIdx = list.findIndex((f) => f.rel_path === relPath)
    if (fromIdx !== -1 && toIdx !== -1) {
      const start = Math.min(fromIdx, toIdx)
      const end = Math.max(fromIdx, toIdx)
      const shouldSelect = !next.has(relPath)
      for (let i = start; i <= end; i += 1) {
        const p = list[i]!.rel_path
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
    selectedPaths.value = new Set(filteredFiles.value.map((f) => f.rel_path))
  }
}

async function handleDetailUpdated(updated: MediaFileDetail, bumpArt = false): Promise<void> {
  const prevRel = activeDetail.value?.entry.rel_path
  if (bumpArt) {
    artworkVersion.value += 1
  }
  activeDetail.value = updated
  inspectingPath.value = updated.entry.rel_path

  if (scrapeWorkbenchActive.value) {
    const nextSaved = new Set(scrapeWorkbenchSavedPaths.value)
    nextSaved.add(updated.entry.rel_path)
    scrapeWorkbenchSavedPaths.value = nextSaved
    if (prevRel && prevRel !== updated.entry.rel_path && scrapeWorkbenchPaths.value) {
      scrapeWorkbenchPaths.value = scrapeWorkbenchPaths.value.map((p) =>
        p === prevRel ? updated.entry.rel_path : p,
      )
    }
  }

  if (prevRel && prevRel !== updated.entry.rel_path && activeRootId.value) {
    await loadDirectory(activeRootId.value, currentRelPath.value)
  } else if (snapshot.value) {
    snapshot.value = {
      ...snapshot.value,
      files: snapshot.value.files.map((f) =>
        f.rel_path === updated.entry.rel_path ? updated.entry : f,
      ),
    }
  }
}

async function handleScrapeApplied(updated: MediaFileDetail): Promise<void> {
  await handleDetailUpdated(updated, true)
}

async function handleBatchScrapeApplied(
  lastDetail: MediaFileDetail | null,
  summary?: { success: number; failed: number },
): Promise<void> {
  artworkVersion.value += 1
  if (lastDetail) {
    activeDetail.value = lastDetail
    inspectingPath.value = lastDetail.entry.rel_path
  }
  if (activeRootId.value) {
    await loadDirectory(activeRootId.value, currentRelPath.value)
  }
  if (summary) {
    snackbar.show({
      message:
        summary.failed > 0
          ? `批量刮削完成：成功 ${summary.success} 项，失败 ${summary.failed} 项`
          : `批量刮削完成：全部 ${summary.success} 项写入成功`,
      actionLabel: '变更历史',
      onAction: () => {
        historyModalOpen.value = true
      },
    })
  }
}

async function handleRenamed(
  movedCount: number,
  movedMap: Record<string, string>,
): Promise<void> {
  const prevActive = activeDetail.value
  const mappedRel = prevActive ? movedMap[prevActive.entry.rel_path] : undefined
  if (mappedRel && activeRootId.value) {
    inspectingPath.value = mappedRel
  }
  if (activeRootId.value && movedCount > 0) {
    await loadDirectory(activeRootId.value, currentRelPath.value)
    snackbar.show({
      message: `已完成 ${movedCount} 个文件的重命名与归档`,
      actionLabel: '撤销历史',
      onAction: () => {
        historyModalOpen.value = true
      },
    })
  }
  if (mappedRel && activeRootId.value) {
    try {
      activeDetail.value = await toolApi.inspectFile(
        activeRootId.value,
        mappedRel,
        activeDomain.value,
      )
      inspectingPath.value = mappedRel
    } catch {
      // Fall back safely if renamed file lives outside filtered branch
    }
  }
}

/**
 * Adds background scrape tasks following:
 * Local non-empty metadata -> High-priority provider -> Low-priority provider
 */
function enqueueScrapeFiles(
  files: MediaFileEntry[],
  labelHint?: string,
  options?: AutoScrapeOptionsDto,
): void {
  if (files.length === 0) return
  taskStore.enqueueScrapeTask(files, labelHint, options)
}

function enqueueScrapeForSelection(): void {
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
  enqueueScrapeFiles(entries, undefined, options)
}

function handleAutoScrapeToDryRun(entries: MediaFileEntry[]): void {
  autoScrapeModalOpen.value = false
  selectedPaths.value = new Set(entries.map((e) => e.rel_path))
  dryRunModalOpen.value = true
}

async function handleBatchMetadataCompleted(
  report: BatchMetadataEditReportDto,
): Promise<void> {
  artworkVersion.value += 1
  if (activeRootId.value) {
    await loadDirectory(activeRootId.value, currentRelPath.value)
  }
  if (activeDetail.value) {
    const stillHere = (snapshot.value?.files ?? []).find(
      (f) => f.rel_path === activeDetail.value?.entry.rel_path,
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
  if (activeRootId.value) {
    await loadDirectory(activeRootId.value, currentRelPath.value)
  }
  if (activeDetail.value) {
    const stillHere = (snapshot.value?.files ?? []).find(
      (f) => f.rel_path === activeDetail.value?.entry.rel_path,
    )
    if (stillHere) {
      void inspectEntry(stillHere)
    } else {
      activeDetail.value = null
      inspectingPath.value = null
    }
  }
}

async function ensureTaskItemDirectoryLoaded(
  domain: MediaKind,
  rootId: string,
): Promise<void> {
  if (activeDomain.value !== domain) {
    await switchDomain(domain)
  }
  if (activeRootId.value !== rootId || currentRelPath.value !== '') {
    activeRootId.value = rootId
    currentRelPath.value = ''
    await loadDirectory(rootId, '')
  }
}

async function handleReviewUnmatchedItems(payload: ReviewItemsPayload): Promise<void> {
  if (payload.relPaths.length === 0) return
  await ensureTaskItemDirectoryLoaded(payload.domain, payload.rootId)
  completenessFilter.value = 'all'
  workbenchFocusPaths.value = new Set(payload.relPaths)
  setViewMode('flat')
  activeNav.value = 'items'
  const matchedEntries = (snapshot.value?.files ?? []).filter((f) =>
    payload.relPaths.includes(f.rel_path),
  )
  if (payload.openCandidates ?? true) {
    if (matchedEntries.length > 0) {
      await openScrapeWorkbench(matchedEntries[0], matchedEntries)
    }
    return
  }
  const firstEntry = matchedEntries[0]
  if (firstEntry) {
    void inspectEntry(firstEntry)
  }
}

async function handleInspectTaskItem(payload: InspectTaskItemPayload): Promise<void> {
  await ensureTaskItemDirectoryLoaded(payload.domain, payload.rootId)
  completenessFilter.value = 'all'
  workbenchFocusPaths.value = new Set([payload.relPath])
  setViewMode('flat')
  activeNav.value = 'items'
  const target = (snapshot.value?.files ?? []).find((f) => f.rel_path === payload.relPath)
  if (target) {
    if (payload.openCandidates) {
      await openScrapeWorkbench(target, [target])
    } else {
      await inspectEntry(target)
    }
  } else {
    try {
      const detail = await toolApi.inspectFile(payload.rootId, payload.relPath, payload.domain)
      activeDetail.value = detail
      inspectingPath.value = detail.entry.rel_path
    } catch (err) {
      snackbar.show({
        message: err instanceof Error ? err.message : String(err),
        tone: 'error',
      })
    }
  }
}

async function handleOpenTaskBookEditor(payload: InspectTaskItemPayload): Promise<void> {
  await ensureTaskItemDirectoryLoaded(payload.domain, payload.rootId)
  const target = (snapshot.value?.files ?? []).find((f) => f.rel_path === payload.relPath)
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
  const byPath = new Map((snapshot.value?.files ?? []).map((f) => [f.rel_path, f]))
  const resolved: MediaFileEntry[] = []
  for (const p of scoped) {
    const hit = byPath.get(p)
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
  scrapeWorkbenchPaths.value = scopedList ? scopedList.map((e) => e.rel_path) : null
  scrapeWorkbenchSavedPaths.value = new Set()
  scrapeWorkbenchActive.value = true
  activeNav.value = 'items'
  mobileMenuOpen.value = false

  const target =
    targetEntry ??
    (scopedList ? scopedList[0] : activeDetail.value?.entry ?? filteredFiles.value[0])
  if (target) {
    if (activeDetail.value?.entry.rel_path !== target.rel_path) {
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
  return activeQueueList.value.findIndex((f) => f.rel_path === inspectingPath.value)
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

async function handleSelectScrapeQueueIndex(nextIdx: number): Promise<void> {
  const target = scrapeQueue.value[nextIdx]
  if (!target) return
  scrapeQueueIndex.value = nextIdx
  await inspectEntry(target)
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
  musicToolsEntries.value = entries
  musicToolsOpen.value = true
}

const selectedVideoEntries = computed(() =>
  selectedEntriesForRename.value.filter((e) => e.media_kind === 'video'),
)

function openVideoToolbox(entries: MediaFileEntry[]): void {
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
  artworkVersion.value += 1
  if (activeRootId.value) {
    await loadDirectory(activeRootId.value, currentRelPath.value)
  }
  activeDetail.value = epubDetail
  inspectingPath.value = epubDetail.entry.rel_path
  bookEditorEntry.value = epubDetail.entry
  snackbar.show(
    deletedSourceTxt
      ? `已转换为 EPUB 并移除原 TXT：${epubDetail.entry.file_name}`
      : `已生成标准 EPUB：${epubDetail.entry.file_name}`,
  )
}

async function openManualScrapeModal(entry: MediaFileEntry): Promise<void> {
  if (activeDetail.value?.entry.rel_path !== entry.rel_path) {
    await inspectEntry(entry)
  }
  scrapeQueue.value = [entry]
  scrapeQueueIndex.value = 0
  if (activeDetail.value) {
    scrapeModalOpen.value = true
  }
}

watch(
  () => domainRoots.value,
  (roots) => {
    if (roots.length === 0) {
      activeRootId.value = ''
      snapshot.value = null
      activeDetail.value = null
      inspectingPath.value = null
    } else if (!activeRootId.value || !roots.some((r) => r.id === activeRootId.value)) {
      activeRootId.value = roots[0]!.id
      void loadDirectory(activeRootId.value, '')
    }
  },
)

function handleSettingsUpdated(next: PublicSettingsDto): void {
  settings.value = next
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
  bookNormalizeEntries.value = valid
  bookNormalizeOpen.value = true
}

async function handleBookNormalizeCompleted(report: NormalizeBooksBatchReportDto): Promise<void> {
  artworkVersion.value += 1
  if (activeRootId.value) {
    await loadDirectory(activeRootId.value, currentRelPath.value)
  }
  if (activeDetail.value) {
    const matchedItem = report.items.find(
      (it) => !it.error && it.source_rel_path === activeDetail.value?.entry.rel_path,
    )
    const targetRel = matchedItem?.output_rel_path ?? activeDetail.value.entry.rel_path
    const refreshed = (snapshot.value?.files ?? []).find((f) => f.rel_path === targetRel)
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
    scrapeModalOpen.value ||
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
    if (activeDetail.value && inspectorRef.value) {
      ev.preventDefault()
      void inspectorRef.value.requestClose()
    } else if (selectedPaths.value.size > 0) {
      ev.preventDefault()
      selectedPaths.value = new Set()
    }
    return
  }
  if (isTypingTarget(ev.target)) return
  if (activeNav.value === 'items' && (ev.key === 'ArrowDown' || ev.key === 'ArrowUp')) {
    const list = filteredFiles.value
    if (list.length === 0) return
    ev.preventDefault()
    const curIdx = inspectingPath.value
      ? list.findIndex((f) => f.rel_path === inspectingPath.value)
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
      parsed.rootId &&
      (parsed.rootId !== activeRootId.value || (parsed.relPath ?? '') !== currentRelPath.value)
    ) {
      activeRootId.value = parsed.rootId
      await loadDirectory(parsed.rootId, parsed.relPath ?? '')
    }
  } finally {
    suppressHashSync = false
  }
}

let offFileScraped: (() => void) | null = null

onMounted(() => {
  window.addEventListener('keydown', handleKeydown)
  window.addEventListener('beforeunload', handleBeforeUnload)
  window.addEventListener('hashchange', handleHashChange)
  offFileScraped = taskStore.onFileScraped((updated) => {
    artworkVersion.value += 1
    if (snapshot.value && snapshot.value.root_id === updated.entry.root_id) {
      snapshot.value = {
        ...snapshot.value,
        files: snapshot.value.files.map((f) =>
          f.rel_path === updated.entry.rel_path ? updated.entry : f,
        ),
      }
    }
    if (activeDetail.value?.entry.rel_path === updated.entry.rel_path) {
      activeDetail.value = updated
    }
  })
  void loadInitial()
})

onBeforeUnmount(() => {
  window.removeEventListener('keydown', handleKeydown)
  window.removeEventListener('beforeunload', handleBeforeUnload)
  window.removeEventListener('hashchange', handleHashChange)
  if (offFileScraped) offFileScraped()
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
      :domain-tabs="domainTabs"
      :search-query="searchQuery"
      :search-placeholder="domainLabels.searchPlaceholder"
      :status="status"
      @toggle-sidebar="toggleSidebar"
      @navigate-home="navigatePrimary('home')"
      @switch-domain="switchDomain"
      @update:search-query="searchQuery = $event"
      @lock="emit('lock')"
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
        :active-path="inspectingPath"
        :saved-paths="scrapeWorkbenchSavedPaths"
        :artwork-version="artworkVersion"
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
        :view-mode="viewMode"
        :completeness-filter="completenessFilter"
        :filter-items="filterItems"
        @close-mobile="mobileMenuOpen = false"
        @switch-domain="switchDomain"
        @navigate-primary="navigatePrimary"
        @open-settings-roots="openSettingsTab('roots')"
        @open-folders-view="setViewMode('folders'); navigatePrimary('items')"
        @select-root="selectRoot"
        @select-completeness-filter="(f) => { if (f === 'all') setViewMode('flat'); selectCompletenessFilter(f) }"
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
          :saved-paths="scrapeWorkbenchSavedPaths"
          :artwork-version="artworkVersion"
          :sidebar-collapsed="collapsed"
          :has-prev="hasPrevInspect"
          :has-next="hasNextInspect"
          :position-label="inspectPositionLabel"
          @exit="exitScrapeWorkbench"
          @toggle-sidebar="toggleSidebar"
          @select-entry="inspectEntry"
          @step="stepInspectingFile"
          @updated="handleDetailUpdated"
          @open-scrape="(d) => openManualScrapeModal(d.entry)"
          @open-settings-scrapers="openSettingsTab('scrapers')"
          @open-content-editor="openBookContentEditor"
          @open-normalize-epub="(e) => openBookNormalizeModal([e])"
          @open-comic-pages="openComicPageManager"
          @open-music-tools="(e) => openMusicToolbox([e])"
          @open-video-tools="(e) => openVideoToolbox([e])"
          @open-history="historyModalOpen = true"
        />

        <template v-else>
          <div class="min-w-0 flex-1 overflow-y-auto">
          <!-- VIEW 1: IN-PAGE SETTINGS (Server SettingsView Left-Right Layout + Domain Tabs) -->
          <ToolSettingsView
            v-if="activeNav === 'settings'"
            :settings="settings"
            :active-domain="activeDomain"
            :initial-tab="settingsInitialTab"
            :ffmpeg-status="status?.ffmpeg ?? null"
            @back="exitSettings"
            @updated="handleSettingsUpdated"
            @select-domain="switchDomain"
          />

          <!-- VIEW 2: TASKS CENTER -->
          <ToolTasksView
            v-else-if="activeNav === 'tasks'"
            @review-items="handleReviewUnmatchedItems"
            @inspect-item="handleInspectTaskItem"
            @open-book-editor="handleOpenTaskBookEditor"
          />

          <!-- VIEW 3: DOMAIN HOME OVERVIEW -->
          <ToolHomeView
            v-else-if="activeNav === 'home'"
            :active-domain="activeDomain"
            :domain-roots="domainRoots"
            :active-root-id="activeRootId"
            :snapshot="snapshot"
            :loading-dir="loadingDir"
            :artwork-version="artworkVersion"
            :stats="stats"
            @open-items="(f) => selectCompletenessFilter(f ?? 'all')"
            @open-folders-view="setViewMode('folders'); completenessFilter = 'all'; navigatePrimary('items')"
            @enter-folder="(rel) => enterFolder(rel, true)"
            @inspect-file="inspectEntry"
            @scrape-files="enqueueScrapeFiles"
            @open-rename-all="renameModalOpen = true"
            @open-settings="openSettingsTab"
            @refresh-dir="loadDirectory(activeRootId, currentRelPath)"
          />

          <!-- VIEW 4: UNIFIED ITEMS BROWSER -->
          <ToolItemsBrowser
            v-else
            :active-domain="activeDomain"
            :current-rel-path="currentRelPath"
            :domain-roots="domainRoots"
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
            @open-settings-roots="openSettingsTab('roots')"
            @enter-folder="enterFolder"
            @set-view-mode="setViewMode"
            @select-completeness-filter="selectCompletenessFilter"
            @clear-focus-paths="workbenchFocusPaths = null"
            @toggle-select-all="toggleSelectAllFiltered"
            @clear-selection="selectedPaths = new Set(); lastSelectedRelPath = null"
            @set-sort-field="setSortField"
            @toggle-sort-dir="toggleSortDir"
            @enqueue-scrape-selection="enqueueScrapeForSelection"
            @open-scrape-workbench="(scoped) => openScrapeWorkbench(undefined, scoped)"
            @open-comic-series-tools="comicSeriesToolsOpen = true"
            @open-music-toolbox="openMusicToolbox"
            @open-video-toolbox="openVideoToolbox"
            @open-dry-run="dryRunModalOpen = true"
            @open-batch-metadata="batchMetadataModalOpen = true"
            @open-history="historyModalOpen = true"
            @open-rename="renameModalOpen = true"
            @trigger-rescan="triggerManualRescan"
            @refresh-dir="loadDirectory(activeRootId, currentRelPath)"
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
              :artwork-version="artworkVersion"
              :show-candidates="false"
              :has-prev="hasPrevInspect"
              :has-next="hasNextInspect"
              :position-label="inspectPositionLabel"
              @step="stepInspectingFile"
              @close="activeDetail = null; inspectingPath = null"
              @updated="handleDetailUpdated"
              @open-scrape="(d) => openManualScrapeModal(d.entry)"
              @open-workbench="(d) => openScrapeWorkbench(d.entry)"
              @open-settings-scrapers="openSettingsTab('scrapers')"
              @open-content-editor="openBookContentEditor"
              @open-normalize-epub="(e) => openBookNormalizeModal([e])"
              @open-comic-pages="openComicPageManager"
              @open-music-tools="(e) => openMusicToolbox([e])"
              @open-video-tools="(e) => openVideoToolbox([e])"
              @open-history="historyModalOpen = true"
            />
          </div>
        </Transition>
        </template>
      </main>
    </div>

    <!-- Manual Match Scrape Modal (Accessible from Inspector Drawer) -->
    <OnlineScrapeModal
      :open="scrapeModalOpen"
      :detail="activeDetail"
      :settings="settings"
      :queue="scrapeQueue"
      :queue-index="scrapeQueueIndex"
      @close="scrapeModalOpen = false"
      @applied="handleScrapeApplied"
      @batch-applied="handleBatchScrapeApplied"
      @select-queue-index="handleSelectScrapeQueueIndex"
      @open-settings="scrapeModalOpen = false; openSettingsTab('scrapers')"
      @settings-updated="handleSettingsUpdated"
    />

    <BatchMetadataModal
      v-if="activeRootId"
      :open="batchMetadataModalOpen"
      :root-id="activeRootId"
      :domain="activeDomain"
      :entries="selectedEntriesForRename"
      @close="batchMetadataModalOpen = false"
      @completed="handleBatchMetadataCompleted"
      @open-history="batchMetadataModalOpen = false; historyModalOpen = true"
    />

    <MetadataHistoryModal
      :open="historyModalOpen"
      :root-id="activeRootId"
      @close="historyModalOpen = false"
      @restored="handleHistoryRestored"
    />

    <BatchRenameModal
      v-if="settings && activeRootId"
      :open="renameModalOpen"
      :root-id="activeRootId"
      :entries="selectedEntriesForRename"
      :templates="settings.rename_templates"
      @close="renameModalOpen = false"
      @renamed="handleRenamed"
    />

    <BookContentEditorModal
      :open="bookEditorOpen"
      :entry="bookEditorEntry"
      @close="bookEditorOpen = false"
      @saved="handleBookContentSaved"
      @converted="handleTxtConvertedToEpub"
    />

    <BookNormalizeEpubModal
      v-if="activeRootId"
      :open="bookNormalizeOpen"
      :root-id="activeRootId"
      :entries="bookNormalizeEntries"
      @close="bookNormalizeOpen = false"
      @completed="handleBookNormalizeCompleted"
    />

    <ComicPageManagerModal
      :open="comicPageManagerOpen"
      :entry="comicPageManagerEntry"
      @close="comicPageManagerOpen = false"
      @saved="(d) => handleDetailUpdated(d, true)"
    />

    <ComicSeriesToolsModal
      :open="comicSeriesToolsOpen"
      :root-id="activeRootId"
      :entries="selectedComicEntries"
      @close="comicSeriesToolsOpen = false"
      @completed="() => activeRootId && loadDirectory(activeRootId, currentRelPath)"
    />

    <MusicToolboxModal
      :open="musicToolsOpen"
      :root-id="activeRootId"
      :entries="musicToolsEntries"
      @close="musicToolsOpen = false"
      @updated="(d) => handleDetailUpdated(d, true)"
      @completed="() => activeRootId && loadDirectory(activeRootId, currentRelPath)"
    />

    <VideoToolboxModal
      :open="videoToolsOpen"
      :root-id="activeRootId"
      :entries="videoToolsEntries"
      @close="videoToolsOpen = false"
      @updated="(d) => handleDetailUpdated(d, true)"
      @completed="() => activeRootId && loadDirectory(activeRootId, currentRelPath)"
    />

    <AutoScrapeOptionsModal
      :open="autoScrapeModalOpen"
      :domain="activeDomain"
      :entries="autoScrapeTargetEntries"
      @close="autoScrapeModalOpen = false"
      @start="handleAutoScrapeConfirmed"
      @open-dry-run="handleAutoScrapeToDryRun"
      @open-settings="autoScrapeModalOpen = false; openSettingsTab('scrapers')"
    />

    <DryRunScrapeModal
      :open="dryRunModalOpen"
      :root-id="activeRootId"
      :entries="selectedEntriesForRename"
      @close="dryRunModalOpen = false"
      @confirm="(entries) => enqueueScrapeFiles(entries)"
    />

    <!-- Bottom-Right Floating Task Progress Panel (Click task jumps to Tasks View) -->
    <FloatingTaskShelf
      v-if="activeNav !== 'tasks'"
      high-z-index
      :tasks="taskStore.shelfItems"
      :collapsed="taskStore.collapsed"
      @update:collapsed="taskStore.collapsed = $event"
      @clear-completed="taskStore.clearCompleted"
      @dismiss-all="taskStore.dismissAll"
      @select-task="navigatePrimary('tasks')"
    />
  </div>
</template>

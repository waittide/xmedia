<script setup lang="ts">
import { computed, onBeforeUnmount, onMounted, ref, watch, type Component } from 'vue'
import { useI18n } from 'vue-i18n'
import {
  LuAlertCircle,
  LuAlertTriangle,
  LuArrowUp,
  LuBookOpen,
  LuCheckCircle2,
  LuChevronDown,
  LuChevronRight,
  LuClapperboard,
  LuClock,
  LuDisc3,
  LuFileSignature,
  LuFileText,
  LuFolder,
  LuFolderOpen,
  LuFolderPlus,
  LuHardDrive,
  LuHome,
  LuImage,
  LuLayers,
  LuLibrary,
  LuList,
  LuLock,
  LuMonitor,
  LuMoon,
  LuPanelLeftClose,
  LuPanelLeftOpen,
  LuSearch,
  LuSettings,
  LuSparkles,
  LuSun,
  LuWrench,
  LuX,
} from 'vue-icons-plus/lu'
import AppCheckbox from '@/components/ui/AppCheckbox.vue'
import FloatingTaskShelf from '@/components/ui/FloatingTaskShelf.vue'
import RefreshButton from '@/components/ui/RefreshButton.vue'
import { useThemeStore } from '@/stores/theme'
import type { MediaKind } from '@/types/api'
import { buildArtworkUrl, toolApi } from '../api'
import { formatBytes, useToolText } from '../i18n'
import { useToolTaskStore } from '../tasks'
import type {
  DirectorySnapshot,
  ManagedRootDto,
  MediaFileDetail,
  MediaFileEntry,
  PublicSettingsDto,
  ToolStatusDto,
} from '../types'
import BatchRenameModal from './BatchRenameModal.vue'
import BookContentEditorModal from './BookContentEditorModal.vue'
import MetadataInspectorDrawer from './MetadataInspectorDrawer.vue'
import OnlineScrapeModal from './OnlineScrapeModal.vue'
import ToolHomeView from './ToolHomeView.vue'
import ToolSettingsView, { type ToolSettingsTab } from './ToolSettingsView.vue'
import ToolTasksView from './ToolTasksView.vue'

defineProps<{
  status: ToolStatusDto | null
}>()

const emit = defineEmits<{
  lock: []
}>()

const { t } = useI18n()
const themeStore = useThemeStore()
const taskStore = useToolTaskStore()
const txt = useToolText()

type PrimaryNavKey = 'home' | 'items' | 'tasks' | 'settings'
type BrowseViewMode = 'flat' | 'folders'
type CompletenessFilter =
  | 'all'
  | 'complete'
  | 'incomplete'
  | 'missing_cover'
  | 'missing_summary'
  | 'missing_tag'

const VALID_DOMAINS: MediaKind[] = ['book', 'comic', 'music', 'video']

function loadSavedDomain(): MediaKind {
  const saved = localStorage.getItem('xmedia_tool.active_domain') as MediaKind | null
  if (saved && VALID_DOMAINS.includes(saved)) {
    return saved
  }
  return 'book'
}

function loadSavedNav(): PrimaryNavKey {
  const saved = localStorage.getItem('xmedia_tool.active_nav')
  if (saved === 'home' || saved === 'items' || saved === 'tasks') return saved
  return 'home'
}

const activeNav = ref<PrimaryNavKey>(loadSavedNav())
const previousWorkspaceNav = ref<'home' | 'items' | 'tasks'>('home')
const collapsed = ref(localStorage.getItem('xmedia_tool.sidebar_collapsed') === '1')
const viewMode = ref<BrowseViewMode>(
  localStorage.getItem('xmedia_tool.view_mode') === 'folders' ? 'folders' : 'flat',
)
const mobileMenuOpen = ref(false)
const activeDomain = ref<MediaKind>(loadSavedDomain())
const completenessFilter = ref<CompletenessFilter>('all')
const searchQuery = ref('')
const searchInputRef = ref<HTMLInputElement | null>(null)

// Collapsible sidebar groups (Server WorkspaceLayout style)
const collapsedGroups = ref<Record<string, boolean>>({
  roots: false,
  folders: false,
  filters: false,
})

function toggleGroupFold(groupId: string): void {
  collapsedGroups.value[groupId] = !collapsedGroups.value[groupId]
}

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
const loadingInspector = ref(false)
const artworkVersion = ref(1)
const batchSummaryBanner = ref<string | null>(null)

const settingsInitialTab = ref<ToolSettingsTab>('appearance')
const scrapeModalOpen = ref(false)
const scrapeQueue = ref<MediaFileEntry[]>([])
const scrapeQueueIndex = ref(0)
const renameModalOpen = ref(false)
const bookEditorOpen = ref(false)
const bookEditorEntry = ref<MediaFileEntry | null>(null)

const isCompact = computed(() => collapsed.value && !mobileMenuOpen.value)

function navigatePrimary(nav: 'home' | 'items' | 'tasks'): void {
  activeNav.value = nav
  previousWorkspaceNav.value = nav
  localStorage.setItem('xmedia_tool.active_nav', nav)
  mobileMenuOpen.value = false
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

const themeModeTooltip = computed(() => {
  if (themeStore.themeMode === 'light') return `${t('theme.modeTitle')}: ${t('theme.light')}`
  if (themeStore.themeMode === 'dark') return `${t('theme.modeTitle')}: ${t('theme.dark')}`
  return `${t('theme.modeTitle')}: ${t('theme.system')}`
})

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

function relativeDirBadge(relPath: string): string | null {
  const parent = parentRelOf(relPath)
  const cur = currentRelPath.value
  if (!parent || parent === cur) return null
  if (cur && parent.startsWith(`${cur}/`)) {
    return parent.slice(cur.length + 1)
  }
  return parent
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

const filteredFiles = computed<MediaFileEntry[]>(() => {
  const base = snapshot.value?.files ?? []
  const q = searchQuery.value.trim().toLowerCase()
  const showRecursive = viewMode.value === 'flat' || hasActiveSearchOrFilter.value

  return base.filter((f) => {
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

const selectedEntriesForRename = computed<MediaFileEntry[]>(() => {
  const files = filteredFiles.value
  if (selectedPaths.value.size > 0) {
    return files.filter((f) => selectedPaths.value.has(f.rel_path))
  }
  if (activeDetail.value) {
    const match = files.find((f) => f.rel_path === activeDetail.value?.entry.rel_path)
    if (match) return [match]
  }
  return files
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

function cycleThemeMode(): void {
  const order = ['system', 'light', 'dark'] as const
  const idx = order.indexOf(themeStore.themeMode)
  themeStore.setThemeMode(order[(idx + 1) % order.length]!)
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
  if (inspectorRef.value && !(await inspectorRef.value.confirmDiscardIfDirty())) {
    return
  }
  activeDomain.value = next
  mobileMenuOpen.value = false
  localStorage.setItem('xmedia_tool.active_domain', next)
  await activateDomainWorkspace(next)
}

async function loadInitial(): Promise<void> {
  const s = await toolApi.getSettings()
  settings.value = s

  const currentHasRoots = s.roots.some((r) => r.default_kind === activeDomain.value)
  if (!currentHasRoots) {
    const firstWithRoot = VALID_DOMAINS.find((d) => s.roots.some((r) => r.default_kind === d))
    if (firstWithRoot) {
      activeDomain.value = firstWithRoot
      localStorage.setItem('xmedia_tool.active_domain', firstWithRoot)
    }
  }
  await activateDomainWorkspace(activeDomain.value)
}

async function loadDirectory(rootId: string, relPath: string): Promise<void> {
  if (!rootId) return
  const reqDomain = activeDomain.value
  const preserveSelection = rootId === activeRootId.value && relPath === currentRelPath.value
  loadingDir.value = true
  dirError.value = null
  if (!preserveSelection) {
    selectedPaths.value = new Set()
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
    if (activeDomain.value !== reqDomain) return
    snapshot.value = snap
    currentRelPath.value = snap.current_rel_path
    syncDomainStateToMemory(reqDomain, rootId, snap.current_rel_path)

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
    if (activeDomain.value === reqDomain) {
      dirError.value = msg
    }
  } finally {
    if (activeDomain.value === reqDomain) {
      loadingDir.value = false
    }
  }
}

async function selectRoot(rootId: string): Promise<void> {
  if (inspectorRef.value && !(await inspectorRef.value.confirmDiscardIfDirty())) {
    return
  }
  mobileMenuOpen.value = false
  if (activeNav.value === 'settings' || activeNav.value === 'tasks') {
    navigatePrimary('items')
  }
  if (activeRootId.value === rootId && currentRelPath.value === '') return
  activeRootId.value = rootId
  currentRelPath.value = ''
  activeDetail.value = null
  inspectingPath.value = null
  await loadDirectory(rootId, '')
}

async function enterFolder(relPath: string, forceFoldersView = false): Promise<void> {
  if (inspectorRef.value && !(await inspectorRef.value.confirmDiscardIfDirty())) {
    return
  }
  mobileMenuOpen.value = false
  if (forceFoldersView) {
    setViewMode('folders')
  }
  if (activeNav.value !== 'items') {
    navigatePrimary('items')
  }
  await loadDirectory(activeRootId.value, relPath)
}

async function inspectEntry(entry: MediaFileEntry): Promise<void> {
  if (activeDetail.value?.entry.rel_path === entry.rel_path) return
  if (inspectorRef.value && !(await inspectorRef.value.confirmDiscardIfDirty())) {
    return
  }
  inspectingPath.value = entry.rel_path
  loadingInspector.value = true
  try {
    activeDetail.value = await toolApi.inspectFile(
      entry.root_id,
      entry.rel_path,
      activeDomain.value,
    )
  } catch (err) {
    dirError.value = err instanceof Error ? err.message : String(err)
  } finally {
    loadingInspector.value = false
  }
}

function toggleSelectFile(relPath: string): void {
  const next = new Set(selectedPaths.value)
  if (next.has(relPath)) {
    next.delete(relPath)
  } else {
    next.add(relPath)
  }
  selectedPaths.value = next
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
    batchSummaryBanner.value =
      summary.failed > 0
        ? `批量刮削完成：成功 ${summary.success} 项，失败 ${summary.failed} 项`
        : `批量刮削完成：全部 ${summary.success} 项写入成功`
    setTimeout(() => {
      batchSummaryBanner.value = null
    }, 5500)
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
function enqueueScrapeFiles(files: MediaFileEntry[], labelHint?: string): void {
  if (files.length === 0) return
  taskStore.enqueueScrapeTask(files, labelHint)
  batchSummaryBanner.value = `已添加 ${files.length} 个刮削任务（按 本地信息 → 高优先级源 → 低优先级源 自动补齐）`
  setTimeout(() => {
    batchSummaryBanner.value = null
  }, 4000)
}

function enqueueScrapeForSelection(): void {
  const targets =
    selectedPaths.value.size > 0
      ? filteredFiles.value.filter((f) => selectedPaths.value.has(f.rel_path))
      : [...filteredFiles.value]
  enqueueScrapeFiles(targets)
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

async function handleBookContentSaved(updated: MediaFileDetail): Promise<void> {
  await handleDetailUpdated(updated, true)
  batchSummaryBanner.value = `已保存章节目录与正文：${updated.entry.file_name}`
  setTimeout(() => {
    batchSummaryBanner.value = null
  }, 4000)
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
  batchSummaryBanner.value = deletedSourceTxt
    ? `已转换为 EPUB 并移除原 TXT：${epubDetail.entry.file_name}`
    : `已生成标准 EPUB：${epubDetail.entry.file_name}`
  setTimeout(() => {
    batchSummaryBanner.value = null
  }, 5000)
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

function openSettingsTab(tab: ToolSettingsTab): void {
  settingsInitialTab.value = tab
  if (activeNav.value !== 'settings') {
    previousWorkspaceNav.value = activeNav.value
  }
  activeNav.value = 'settings'
  mobileMenuOpen.value = false
}

function exitSettings(): void {
  activeNav.value = previousWorkspaceNav.value
}

function handleKeydown(ev: KeyboardEvent): void {
  if ((ev.metaKey || ev.ctrlKey) && ev.key.toLowerCase() === 'k') {
    ev.preventDefault()
    searchInputRef.value?.focus()
  }
}

let offFileScraped: (() => void) | null = null

onMounted(() => {
  window.addEventListener('keydown', handleKeydown)
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
  if (offFileScraped) offFileScraped()
})
</script>

<template>
  <div
    class="relative flex h-screen w-screen flex-col overflow-hidden bg-[var(--ctp-shell)] text-[var(--ctp-text)]"
  >
    <!-- Full-Width Top Bar (Aligned with Server WorkspaceLayout) -->
    <header
      class="flex h-16 shrink-0 items-center justify-between gap-3 bg-[var(--ctp-shell)] px-4 select-none sm:px-5"
    >
      <!-- Left: Sidebar Toggle + Accent Brand + 4-Domain Capsule Switcher -->
      <div class="flex shrink-0 items-center gap-2.5">
        <button
          type="button"
          class="flex h-9 w-9 items-center justify-center rounded-xl text-[var(--ctp-subtext1)] transition hover:bg-[var(--ctp-surface0)]/60 hover:text-[var(--ctp-text)]"
          @click="toggleSidebar"
        >
          <LuPanelLeftOpen v-if="isCompact" class="h-[18px] w-[18px]" />
          <LuPanelLeftClose v-else class="h-[18px] w-[18px]" />
        </button>

        <button
          type="button"
          class="flex items-center gap-2.5 text-left outline-none transition hover:opacity-90"
          @click="navigatePrimary('home')"
        >
          <div
            class="flex h-9 w-9 items-center justify-center rounded-xl bg-[var(--ctp-accent)] text-[var(--ctp-on-accent)] shadow-sm"
          >
            <LuWrench class="h-[18px] w-[18px]" />
          </div>
          <div class="hidden lg:block">
            <div class="text-sm leading-tight font-bold tracking-tight">{{ txt.brand }}</div>
            <div class="text-[10px] text-[var(--ctp-subtext0)]">{{ txt.brandSub }}</div>
          </div>
        </button>

        <!-- Unified 4-Domain Capsule Switcher -->
        <div
          class="ml-1 hidden items-center gap-0.5 rounded-xl border border-[var(--ctp-surface0)]/75 bg-[var(--ctp-mantle)]/70 p-1 md:flex"
        >
          <button
            v-for="tab in domainTabs"
            :key="tab.value"
            type="button"
            class="flex items-center gap-1.5 rounded-lg px-2.5 py-1.5 text-xs font-semibold transition"
            :class="
              activeDomain === tab.value
                ? 'bg-[var(--ctp-accent)] text-[var(--ctp-on-accent)] shadow-xs'
                : 'text-[var(--ctp-subtext1)] hover:bg-[var(--ctp-surface0)]/60 hover:text-[var(--ctp-text)]'
            "
            @click="switchDomain(tab.value)"
          >
            <component :is="tab.icon" class="h-3.5 w-3.5 shrink-0" />
            <span>{{ tab.label }}</span>
          </button>
        </div>
      </div>

      <!-- Center: Pill Search Bar -->
      <div class="relative mx-auto w-full max-w-xl">
        <LuSearch
          class="pointer-events-none absolute top-1/2 left-3.5 h-4 w-4 -translate-y-1/2 text-[var(--ctp-subtext0)]"
        />
        <input
          ref="searchInputRef"
          v-model="searchQuery"
          type="text"
          :placeholder="domainLabels.searchPlaceholder"
          class="h-10 w-full rounded-full border border-[var(--ctp-surface0)]/90 bg-[var(--ctp-mantle)]/70 pr-14 pl-10 text-xs text-[var(--ctp-text)] placeholder-[var(--ctp-subtext0)] outline-none transition focus:border-[var(--ctp-accent)] focus:bg-[var(--ctp-canvas)]"
        />
        <kbd
          v-if="!searchQuery"
          class="pointer-events-none absolute top-1/2 right-3 hidden -translate-y-1/2 rounded-md border border-[var(--ctp-surface0)] bg-[var(--ctp-shell)]/80 px-1.5 py-0.5 font-mono text-[10px] text-[var(--ctp-subtext0)] sm:inline"
        >
          ⌘K
        </kbd>
        <button
          v-else
          type="button"
          class="absolute top-1/2 right-3 -translate-y-1/2 rounded-full p-0.5 text-[var(--ctp-subtext0)] hover:text-[var(--ctp-text)]"
          @click="searchQuery = ''"
        >
          <LuX class="h-3.5 w-3.5" />
        </button>
      </div>

      <!-- Right: ffprobe hint + Theme Mode + Settings + Lock -->
      <div class="flex shrink-0 items-center gap-1.5">
        <span
          v-if="status && !status.ffmpeg.ffprobe_available"
          class="hidden items-center gap-1 rounded-xl border border-amber-500/35 bg-amber-500/10 px-2.5 py-1 text-[11px] font-medium text-amber-500 sm:inline-flex"
          title="系统未检测到 ffprobe，视频编码/分辨率/音轨与字幕流详情探测将受限"
        >
          <LuAlertTriangle class="h-3.5 w-3.5" />
          <span>无 ffprobe</span>
        </span>

        <button
          type="button"
          class="flex h-9 w-9 items-center justify-center rounded-xl transition hover:bg-[var(--ctp-surface0)]/60 hover:text-[var(--ctp-text)]"
          :class="
            themeStore.themeMode === 'system'
              ? 'bg-[var(--ctp-surface0)]/55 text-[var(--ctp-text)]'
              : 'text-[var(--ctp-subtext1)]'
          "
          :title="themeModeTooltip"
          @click="cycleThemeMode"
        >
          <LuMonitor v-if="themeStore.themeMode === 'system'" class="h-[18px] w-[18px]" />
          <LuSun v-else-if="themeStore.themeMode === 'light'" class="h-[18px] w-[18px]" />
          <LuMoon v-else class="h-[18px] w-[18px]" />
        </button>

        <button
          v-if="status?.requires_password"
          type="button"
          class="flex h-9 items-center gap-1 rounded-xl border border-[var(--ctp-surface0)] bg-[var(--ctp-mantle)]/80 px-2.5 text-xs font-semibold text-[var(--ctp-subtext1)] transition hover:border-amber-500/50 hover:text-amber-500"
          :title="txt.actions.lock"
          @click="emit('lock')"
        >
          <LuLock class="h-3.5 w-3.5" />
          <span class="hidden md:inline">{{ txt.actions.lock }}</span>
        </button>

        <button
          type="button"
          class="flex h-9 w-9 items-center justify-center rounded-xl transition"
          :class="
            activeNav === 'settings'
              ? 'bg-[var(--ctp-accent-soft)] text-[var(--ctp-accent)]'
              : 'text-[var(--ctp-subtext1)] hover:bg-[var(--ctp-surface0)]/60 hover:text-[var(--ctp-text)]'
          "
          :title="txt.actions.settings"
          :aria-label="txt.actions.settings"
          @click="activeNav === 'settings' ? exitSettings() : openSettingsTab('appearance')"
        >
          <LuSettings class="h-[18px] w-[18px]" />
        </button>
      </div>
    </header>

    <!-- Middle Body: Server-Aligned Collapsible Sidebar + Flush Rounded Main Canvas -->
    <div class="relative flex min-h-0 flex-1 overflow-hidden">
      <div
        v-if="mobileMenuOpen"
        class="fixed inset-0 z-30 bg-black/45 md:hidden"
        @click="mobileMenuOpen = false"
      />

      <!-- Left Sidebar (Aligned with Server WorkspaceLayout: Home / Items / Tasks -> Groups) -->
      <aside
        class="fixed top-16 bottom-0 left-0 z-40 flex shrink-0 flex-col border-r border-[var(--ctp-surface0)]/65 bg-[var(--ctp-shell)] transition-all duration-200 select-none md:static md:border-r-0"
        :class="[
          mobileMenuOpen ? 'w-64 translate-x-0' : '-translate-x-full md:translate-x-0',
          collapsed ? 'md:w-16' : 'md:w-60',
        ]"
      >
        <!-- Mobile 4-Domain Switcher Grid -->
        <div
          class="grid grid-cols-2 gap-1.5 border-b border-[var(--ctp-surface0)]/60 px-3 py-2.5 md:hidden"
        >
          <button
            v-for="tab in domainTabs"
            :key="`mob-${tab.value}`"
            type="button"
            class="flex items-center justify-center gap-1.5 rounded-xl px-2.5 py-2 text-xs font-semibold transition"
            :class="
              activeDomain === tab.value
                ? 'bg-[var(--ctp-accent)] text-[var(--ctp-on-accent)]'
                : 'bg-[var(--ctp-mantle)] text-[var(--ctp-subtext1)]'
            "
            @click="switchDomain(tab.value)"
          >
            <component :is="tab.icon" class="h-3.5 w-3.5 shrink-0" />
            <span class="truncate">{{ tab.label }}</span>
          </button>
        </div>

        <nav class="flex-1 space-y-0.5 overflow-y-auto px-3 pt-1 pb-4">
          <!-- Fixed Top Primary Navigation: 首页 / 条目 / 任务 -->
          <button
            type="button"
            class="flex w-full items-center gap-3 rounded-xl px-3 py-2 text-sm font-medium transition"
            :class="[
              isCompact ? 'md:justify-center md:px-0' : '',
              activeNav === 'home'
                ? 'bg-[var(--ctp-accent-soft)] font-semibold text-[var(--ctp-accent)]'
                : 'text-[var(--ctp-subtext1)] hover:bg-[var(--ctp-surface0)]/60 hover:text-[var(--ctp-text)]',
            ]"
            :title="isCompact ? txt.nav.home : undefined"
            @click="navigatePrimary('home')"
          >
            <LuHome class="h-4 w-4 shrink-0" />
            <span v-if="!isCompact" class="truncate">{{ txt.nav.home }}</span>
          </button>

          <button
            type="button"
            class="flex w-full items-center gap-3 rounded-xl px-3 py-2 text-sm font-medium transition"
            :class="[
              isCompact ? 'md:justify-center md:px-0' : '',
              activeNav === 'items'
                ? 'bg-[var(--ctp-accent-soft)] font-semibold text-[var(--ctp-accent)]'
                : 'text-[var(--ctp-subtext1)] hover:bg-[var(--ctp-surface0)]/60 hover:text-[var(--ctp-text)]',
            ]"
            :title="isCompact ? txt.nav.items : undefined"
            @click="navigatePrimary('items')"
          >
            <LuLibrary class="h-4 w-4 shrink-0" />
            <template v-if="!isCompact">
              <span class="flex-1 truncate text-left">{{ txt.nav.items }}</span>
              <span class="font-mono text-[11px] opacity-75">{{ stats.total }}</span>
            </template>
          </button>

          <button
            type="button"
            class="relative flex w-full items-center gap-3 rounded-xl px-3 py-2 text-sm font-medium transition"
            :class="[
              isCompact ? 'md:justify-center md:px-0' : '',
              activeNav === 'tasks'
                ? 'bg-[var(--ctp-accent-soft)] font-semibold text-[var(--ctp-accent)]'
                : 'text-[var(--ctp-subtext1)] hover:bg-[var(--ctp-surface0)]/60 hover:text-[var(--ctp-text)]',
            ]"
            :title="isCompact ? txt.nav.tasks : undefined"
            @click="navigatePrimary('tasks')"
          >
            <LuClock class="h-4 w-4 shrink-0" />
            <template v-if="!isCompact">
              <span class="flex-1 truncate text-left">{{ txt.nav.tasks }}</span>
              <span
                v-if="taskStore.activeCount > 0"
                class="rounded-full bg-[var(--ctp-accent)] px-1.5 py-0.5 font-mono text-[10px] font-bold text-[var(--ctp-on-accent)]"
              >
                {{ taskStore.activeCount }}
              </span>
              <span
                v-else-if="taskStore.persistentTasks.length > 0"
                class="font-mono text-[11px] opacity-70"
              >
                {{ taskStore.persistentTasks.length }}
              </span>
            </template>
            <span
              v-else-if="taskStore.activeCount > 0"
              class="absolute top-1.5 right-2 h-2 w-2 rounded-full bg-[var(--ctp-accent)]"
            />
          </button>

          <!-- Compact Rail Icons for Sidebar Groups -->
          <div
            v-if="isCompact"
            class="mt-3 hidden space-y-0.5 border-t border-[var(--ctp-surface0)]/55 pt-3 md:block"
          >
            <button
              type="button"
              class="flex w-full items-center justify-center rounded-xl py-2 text-[var(--ctp-subtext1)] transition hover:bg-[var(--ctp-surface0)]/60 hover:text-[var(--ctp-text)]"
              :title="txt.roots.title"
              @click="openSettingsTab('roots')"
            >
              <LuHardDrive class="h-4 w-4 shrink-0" />
            </button>
            <button
              type="button"
              class="flex w-full items-center justify-center rounded-xl py-2 text-[var(--ctp-subtext1)] transition hover:bg-[var(--ctp-surface0)]/60 hover:text-[var(--ctp-text)]"
              :title="txt.roots.foldersTitle"
              @click="setViewMode('folders'); navigatePrimary('items')"
            >
              <LuFolder class="h-4 w-4 shrink-0" />
            </button>
            <button
              type="button"
              class="flex w-full items-center justify-center rounded-xl py-2 text-[var(--ctp-subtext1)] transition hover:bg-[var(--ctp-surface0)]/60 hover:text-[var(--ctp-text)]"
              :title="txt.filters.title"
              @click="selectCompletenessFilter('incomplete')"
            >
              <LuCheckCircle2 class="h-4 w-4 shrink-0" />
            </button>
          </div>

          <!-- Expanded Collapsible Sidebar Groups (Server WorkspaceLayout Style) -->
          <template v-if="!isCompact">
            <div class="mt-4 flex items-center justify-between border-t border-[var(--ctp-surface0)]/50 px-2 pt-3">
              <span class="text-[10px] font-semibold tracking-wider text-[var(--ctp-subtext0)]/80 uppercase">
                {{ txt.domains[activeDomain] }}
              </span>
            </div>

            <!-- Group 1: Media Roots -->
            <div class="pt-3">
              <div class="mb-1 flex items-center justify-between px-2">
                <button
                  type="button"
                  class="flex min-w-0 flex-1 items-center gap-1 rounded-md py-0.5 text-left text-[11px] font-semibold tracking-wider text-[var(--ctp-subtext0)] uppercase transition hover:text-[var(--ctp-text)]"
                  @click="toggleGroupFold('roots')"
                >
                  <LuChevronRight v-if="collapsedGroups.roots" class="h-3 w-3 shrink-0" />
                  <LuChevronDown v-else class="h-3 w-3 shrink-0" />
                  <span class="truncate">{{ txt.roots.title }}</span>
                </button>
                <button
                  type="button"
                  class="rounded-md p-1 text-[var(--ctp-subtext0)] transition hover:bg-[var(--ctp-surface0)]/60 hover:text-[var(--ctp-accent)]"
                  :title="txt.roots.add"
                  @click="openSettingsTab('roots')"
                >
                  <LuFolderPlus class="h-3.5 w-3.5" />
                </button>
              </div>

              <div v-if="!collapsedGroups.roots" class="space-y-0.5">
                <div
                  v-if="domainRoots.length === 0"
                  class="px-3 py-1.5 text-[11px] text-[var(--ctp-subtext0)]"
                >
                  暂无目录，点击右上角 + 添加
                </div>
                <button
                  v-for="r in domainRoots"
                  :key="r.id"
                  type="button"
                  class="flex w-full items-center gap-2.5 rounded-xl px-3 py-1.5 text-xs font-medium transition"
                  :class="
                    activeRootId === r.id
                      ? 'bg-[var(--ctp-accent-soft)] font-semibold text-[var(--ctp-accent)]'
                      : 'text-[var(--ctp-subtext1)] hover:bg-[var(--ctp-surface0)]/60 hover:text-[var(--ctp-text)]'
                  "
                  :title="r.path"
                  @click="selectRoot(r.id)"
                >
                  <LuHardDrive class="h-3.5 w-3.5 shrink-0" />
                  <span class="flex-1 truncate text-left">{{ r.name }}</span>
                </button>
              </div>
            </div>

            <!-- Group 2: Subfolders -->
            <div
              v-if="snapshot && (snapshot.parent_rel_path !== null || snapshot.folders.length > 0)"
              class="pt-3"
            >
              <div class="mb-1 flex items-center justify-between px-2">
                <button
                  type="button"
                  class="flex min-w-0 flex-1 items-center gap-1 rounded-md py-0.5 text-left text-[11px] font-semibold tracking-wider text-[var(--ctp-subtext0)] uppercase transition hover:text-[var(--ctp-text)]"
                  @click="toggleGroupFold('folders')"
                >
                  <LuChevronRight v-if="collapsedGroups.folders" class="h-3 w-3 shrink-0" />
                  <LuChevronDown v-else class="h-3 w-3 shrink-0" />
                  <span class="truncate">{{ txt.roots.foldersTitle }}</span>
                </button>
                <span class="font-mono text-[10px] text-[var(--ctp-subtext0)]">
                  {{ snapshot.folders.length }}
                </span>
              </div>

              <div v-if="!collapsedGroups.folders" class="space-y-0.5">
                <button
                  v-if="snapshot.parent_rel_path !== null"
                  type="button"
                  class="flex w-full items-center gap-2.5 rounded-xl px-3 py-1.5 text-xs text-[var(--ctp-subtext1)] transition hover:bg-[var(--ctp-surface0)]/60 hover:text-[var(--ctp-text)]"
                  @click="enterFolder(snapshot.parent_rel_path)"
                >
                  <LuArrowUp class="h-3.5 w-3.5 shrink-0" />
                  <span class="truncate">{{ txt.roots.parentDir }}</span>
                </button>

                <button
                  v-for="folder in snapshot.folders"
                  :key="folder.rel_path"
                  type="button"
                  class="flex w-full items-center gap-2 rounded-xl px-3 py-1.5 text-xs text-[var(--ctp-subtext1)] transition hover:bg-[var(--ctp-surface0)]/60 hover:text-[var(--ctp-text)]"
                  @click="enterFolder(folder.rel_path)"
                >
                  <LuFolder class="h-3.5 w-3.5 shrink-0 text-[var(--ctp-accent)]" />
                  <span class="flex-1 truncate text-left">{{ folder.name }}</span>
                  <span class="font-mono text-[10px] opacity-70">{{ folder.media_file_count }}</span>
                </button>
              </div>
            </div>

            <!-- Group 3: Completeness Filters -->
            <div class="pt-3">
              <div class="mb-1 flex items-center justify-between px-2">
                <button
                  type="button"
                  class="flex min-w-0 flex-1 items-center gap-1 rounded-md py-0.5 text-left text-[11px] font-semibold tracking-wider text-[var(--ctp-subtext0)] uppercase transition hover:text-[var(--ctp-text)]"
                  @click="toggleGroupFold('filters')"
                >
                  <LuChevronRight v-if="collapsedGroups.filters" class="h-3 w-3 shrink-0" />
                  <LuChevronDown v-else class="h-3 w-3 shrink-0" />
                  <span class="truncate">{{ txt.filters.title }}</span>
                </button>
              </div>

              <div v-if="!collapsedGroups.filters" class="space-y-0.5">
                <button
                  v-for="item in filterItems"
                  :key="item.id"
                  type="button"
                  class="flex w-full items-center gap-2.5 rounded-xl px-3 py-1.5 text-xs font-medium transition"
                  :class="
                    activeNav === 'items' && completenessFilter === item.id
                      ? 'bg-[var(--ctp-accent-soft)] font-semibold text-[var(--ctp-accent)]'
                      : 'text-[var(--ctp-subtext1)] hover:bg-[var(--ctp-surface0)]/60 hover:text-[var(--ctp-text)]'
                  "
                  @click="selectCompletenessFilter(item.id)"
                >
                  <component :is="item.icon" class="h-3.5 w-3.5 shrink-0" />
                  <span class="flex-1 truncate text-left">{{ item.label }}</span>
                  <span class="font-mono text-[10px] opacity-75">{{ item.count }}</span>
                </button>
              </div>
            </div>
          </template>
        </nav>
      </aside>

      <!-- Main Rounded Canvas + Optional Right Metadata Inspector Drawer -->
      <main
        class="flex min-w-0 flex-1 overflow-hidden border-t border-[var(--ctp-surface0)]/65 bg-[var(--ctp-canvas)] md:rounded-tl-3xl md:border-l"
      >
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
          <ToolTasksView v-else-if="activeNav === 'tasks'" />

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
            @enter-folder="(rel) => enterFolder(rel, true)"
            @inspect-file="inspectEntry"
            @scrape-files="enqueueScrapeFiles"
            @open-rename-all="renameModalOpen = true"
            @open-settings="openSettingsTab"
            @refresh-dir="loadDirectory(activeRootId, currentRelPath)"
          />

          <!-- VIEW 4: ITEMS BROWSER -->
          <div v-else class="p-4 sm:p-6">
            <!-- Domain Empty State -->
            <div
              v-if="domainRoots.length === 0"
              class="mx-auto mt-12 max-w-lg rounded-3xl border border-dashed border-[var(--ctp-surface1)] bg-[var(--ctp-mantle)]/45 p-8 text-center"
            >
              <div
                class="mx-auto mb-4 flex h-12 w-12 items-center justify-center rounded-2xl bg-[var(--ctp-accent-soft)] text-[var(--ctp-accent)]"
              >
                <LuFolderPlus class="h-6 w-6" />
              </div>
              <h2 class="text-base font-bold text-[var(--ctp-text)]">
                {{ domainLabels.emptyRootsTitle }}
              </h2>
              <p class="mt-2 text-xs leading-relaxed text-[var(--ctp-subtext0)]">
                {{ domainLabels.emptyRootsDesc }}
              </p>
              <button
                type="button"
                class="mt-5 inline-flex items-center gap-2 rounded-xl bg-[var(--ctp-accent)] px-5 py-2.5 text-xs font-semibold text-[var(--ctp-on-accent)] shadow-sm transition hover:opacity-95"
                @click="openSettingsTab('roots')"
              >
                <LuFolderPlus class="h-4 w-4" />
                <span>{{ txt.roots.add }}</span>
              </button>
            </div>

            <template v-else>
              <!-- Breadcrumb + Batch Action Toolbar -->
              <div
                class="mb-4 flex flex-wrap items-center justify-between gap-3 rounded-2xl border border-[var(--ctp-surface0)]/75 bg-[var(--ctp-mantle)]/45 px-4 py-2.5"
              >
                <div class="flex min-w-0 flex-wrap items-center gap-1 text-xs">
                  <template v-for="(bc, idx) in breadcrumbs" :key="bc.relPath || '__root__'">
                    <LuChevronRight
                      v-if="idx > 0"
                      class="h-3.5 w-3.5 shrink-0 text-[var(--ctp-subtext0)]"
                    />
                    <button
                      type="button"
                      class="rounded-lg px-2 py-1 font-medium transition hover:bg-[var(--ctp-surface0)]/65"
                      :class="
                        idx === breadcrumbs.length - 1
                          ? 'font-bold text-[var(--ctp-text)]'
                          : 'text-[var(--ctp-subtext1)]'
                      "
                      @click="enterFolder(bc.relPath)"
                    >
                      {{ bc.label }}
                    </button>
                  </template>
                </div>

                <div class="flex flex-wrap items-center gap-2">
                  <!-- View Mode Switcher: Flat All-Media vs Folder Hierarchy -->
                  <div
                    class="flex items-center gap-0.5 rounded-xl border border-[var(--ctp-surface0)]/80 bg-[var(--ctp-canvas)] p-0.5"
                  >
                    <button
                      type="button"
                      class="inline-flex items-center gap-1 rounded-lg px-2.5 py-1 text-[11px] font-semibold transition"
                      :class="
                        viewMode === 'flat'
                          ? 'bg-[var(--ctp-accent)] text-[var(--ctp-on-accent)]'
                          : 'text-[var(--ctp-subtext1)] hover:text-[var(--ctp-text)]'
                      "
                      title="平铺展示当前目录及所有子目录内的媒体文件"
                      @click="setViewMode('flat')"
                    >
                      <LuList class="h-3.5 w-3.5" />
                      <span>全部媒体</span>
                    </button>
                    <button
                      type="button"
                      class="inline-flex items-center gap-1 rounded-lg px-2.5 py-1 text-[11px] font-semibold transition"
                      :class="
                        viewMode === 'folders'
                          ? 'bg-[var(--ctp-accent)] text-[var(--ctp-on-accent)]'
                          : 'text-[var(--ctp-subtext1)] hover:text-[var(--ctp-text)]'
                      "
                      title="按文件夹目录层级逐级浏览"
                      @click="setViewMode('folders')"
                    >
                      <LuFolder class="h-3.5 w-3.5" />
                      <span>文件夹</span>
                    </button>
                  </div>

                  <AppCheckbox
                    v-if="filteredFiles.length > 0"
                    :model-value="allFilteredSelected"
                    :label="
                      selectedPaths.size > 0
                        ? `已选 ${selectedPaths.size} 项`
                        : txt.actions.selectAll
                    "
                    @change="toggleSelectAllFiltered"
                  />

                  <button
                    v-if="selectedPaths.size > 0"
                    type="button"
                    class="rounded-xl border border-[var(--ctp-surface0)] bg-[var(--ctp-canvas)] px-2.5 py-1 text-[11px] font-medium text-[var(--ctp-subtext1)] transition hover:border-[var(--ctp-accent)] hover:text-[var(--ctp-text)]"
                    @click="selectedPaths = new Set()"
                  >
                    {{ txt.actions.clearSelect }}
                  </button>

                  <!-- Scrape Button: Adds Background Scrape Task (Local -> High Priority -> Low Priority) -->
                  <button
                    type="button"
                    :disabled="filteredFiles.length === 0"
                    class="inline-flex items-center gap-1.5 rounded-xl bg-[var(--ctp-accent-soft)] px-3 py-1.5 text-xs font-semibold text-[var(--ctp-accent)] transition hover:opacity-90 disabled:opacity-45"
                    title="添加后台刮削任务（按 本地已有非空信息 → 高优先级源 → 低优先级源 自动补齐）"
                    @click="enqueueScrapeForSelection"
                  >
                    <LuSparkles class="h-3.5 w-3.5" />
                    <span>
                      {{
                        selectedPaths.size > 0
                          ? `${txt.actions.scrape} (${selectedPaths.size})`
                          : txt.actions.scrape
                      }}
                    </span>
                  </button>

                  <button
                    type="button"
                    :disabled="selectedEntriesForRename.length === 0"
                    class="inline-flex items-center gap-1.5 rounded-xl border border-[var(--ctp-surface0)] bg-[var(--ctp-canvas)] px-3 py-1.5 text-xs font-semibold text-[var(--ctp-text)] transition hover:border-[var(--ctp-accent)] disabled:opacity-45"
                    @click="renameModalOpen = true"
                  >
                    <LuFileSignature class="h-3.5 w-3.5 text-[var(--ctp-accent)]" />
                    <span>{{ txt.actions.batchRename }}</span>
                  </button>

                  <RefreshButton
                    :action="() => loadDirectory(activeRootId, currentRelPath)"
                    :busy="loadingDir"
                    :title="txt.actions.refresh"
                  />
                </div>
              </div>

              <div
                v-if="batchSummaryBanner"
                class="mb-4 flex items-center justify-between gap-2 rounded-2xl border border-emerald-500/35 bg-emerald-500/10 px-4 py-2.5 text-xs font-medium text-emerald-500"
              >
                <span>{{ batchSummaryBanner }}</span>
                <button
                  type="button"
                  class="rounded-lg p-1 hover:bg-emerald-500/15"
                  @click="batchSummaryBanner = null"
                >
                  <LuX class="h-3.5 w-3.5" />
                </button>
              </div>

              <div
                v-if="dirError"
                class="mb-4 rounded-2xl border border-red-500/35 bg-red-500/10 px-4 py-3 text-xs text-red-400"
              >
                {{ dirError }}
              </div>

              <!-- Subfolder Cards (Folder View) -->
              <div
                v-if="viewMode === 'folders' && !hasActiveSearchOrFilter && snapshot && snapshot.folders.length > 0"
                class="mb-4 grid grid-cols-2 gap-2.5 sm:grid-cols-3 lg:grid-cols-4"
              >
                <button
                  v-for="folder in snapshot.folders"
                  :key="`grid-${folder.rel_path}`"
                  type="button"
                  class="card-soft flex items-center gap-3 rounded-2xl bg-[var(--ctp-mantle)]/45 p-3 text-left transition hover:border-[var(--ctp-accent)]"
                  @click="enterFolder(folder.rel_path, true)"
                >
                  <div
                    class="flex h-9 w-9 shrink-0 items-center justify-center rounded-xl bg-[var(--ctp-accent-soft)] text-[var(--ctp-accent)]"
                  >
                    <LuFolderOpen class="h-4 w-4" />
                  </div>
                  <div class="min-w-0 flex-1">
                    <div class="truncate text-xs font-semibold text-[var(--ctp-text)]">
                      {{ folder.name }}
                    </div>
                    <div class="mt-0.5 text-[11px] text-[var(--ctp-subtext0)]">
                      {{ folder.media_file_count }} 个媒体文件
                    </div>
                  </div>
                </button>
              </div>

              <!-- Media Files List -->
              <div
                v-if="loadingDir && !snapshot"
                class="py-16 text-center text-xs text-[var(--ctp-subtext0)]"
              >
                正在读取当前领域本地目录结构…
              </div>

              <div
                v-else-if="filteredFiles.length === 0"
                class="rounded-3xl border border-dashed border-[var(--ctp-surface1)] py-16 text-center text-xs text-[var(--ctp-subtext0)]"
              >
                当前目录与筛选条件下暂无匹配的{{ txt.domains[activeDomain] }}文件
              </div>

              <div v-else class="space-y-2">
                <div
                  v-for="file in filteredFiles"
                  :key="file.rel_path"
                  class="group flex cursor-pointer items-center gap-3 rounded-2xl border px-3.5 py-2.5 transition"
                  :class="
                    inspectingPath === file.rel_path
                      ? 'border-[var(--ctp-accent)] bg-[var(--ctp-accent-soft)]/55'
                      : 'border-[var(--ctp-surface0)]/75 bg-[var(--ctp-mantle)]/35 hover:border-[var(--ctp-surface1)] hover:bg-[var(--ctp-mantle)]/65'
                  "
                  @click="inspectEntry(file)"
                >
                  <div @click.stop>
                    <AppCheckbox
                      :model-value="selectedPaths.has(file.rel_path)"
                      @change="toggleSelectFile(file.rel_path)"
                    />
                  </div>

                  <!-- Artwork Thumbnail -->
                  <div
                    class="relative flex h-12 w-12 shrink-0 items-center justify-center overflow-hidden rounded-xl border border-[var(--ctp-surface0)] bg-[var(--ctp-canvas)]"
                  >
                    <img
                      v-if="file.completeness.has_cover"
                      :src="buildArtworkUrl(file.root_id, file.rel_path, file.media_kind, artworkVersion)"
                      :alt="file.display_title"
                      loading="lazy"
                      class="h-full w-full object-cover"
                    />
                    <LuBookOpen
                      v-else-if="file.media_kind === 'book'"
                      class="h-5 w-5 text-[var(--ctp-subtext0)]"
                    />
                    <LuLayers
                      v-else-if="file.media_kind === 'comic'"
                      class="h-5 w-5 text-[var(--ctp-subtext0)]"
                    />
                    <LuDisc3
                      v-else-if="file.media_kind === 'music'"
                      class="h-5 w-5 text-[var(--ctp-subtext0)]"
                    />
                    <LuClapperboard v-else class="h-5 w-5 text-[var(--ctp-subtext0)]" />
                  </div>

                  <!-- Title + Subtitle + Folder Badge -->
                  <div class="min-w-0 flex-1">
                    <div class="flex items-center gap-2">
                      <span class="truncate text-xs font-bold text-[var(--ctp-text)]">
                        {{ file.display_title }}
                      </span>
                      <span
                        v-if="file.year"
                        class="shrink-0 font-mono text-[11px] text-[var(--ctp-subtext0)]"
                      >
                        ({{ file.year }})
                      </span>
                      <span
                        class="shrink-0 rounded-md bg-[var(--ctp-surface0)]/80 px-1.5 py-0.5 font-mono text-[10px] font-semibold uppercase text-[var(--ctp-subtext1)]"
                      >
                        {{ file.extension }}
                      </span>
                    </div>
                    <div class="mt-0.5 flex items-center gap-2 text-[11px] text-[var(--ctp-subtext0)]">
                      <span class="truncate">{{ file.display_subtitle }}</span>
                      <span>·</span>
                      <span
                        v-if="relativeDirBadge(file.rel_path)"
                        class="inline-flex max-w-[14rem] shrink-0 items-center gap-1 truncate rounded-md bg-[var(--ctp-surface0)]/65 px-1.5 py-0.5 font-mono text-[10px] text-[var(--ctp-subtext1)]"
                      >
                        <LuFolder class="h-2.5 w-2.5 shrink-0 text-[var(--ctp-accent)]" />
                        <span class="truncate">{{ relativeDirBadge(file.rel_path) }}</span>
                      </span>
                      <span class="truncate font-mono text-[10px] opacity-80">
                        {{ file.file_name }}
                      </span>
                    </div>
                  </div>

                  <!-- Completeness Badges -->
                  <div class="hidden shrink-0 items-center gap-1.5 sm:flex">
                    <span
                      class="rounded-full px-2 py-0.5 text-[10px] font-medium"
                      :class="
                        file.completeness.has_cover
                          ? 'bg-emerald-500/15 text-emerald-500'
                          : 'bg-amber-500/15 text-amber-500'
                      "
                    >
                      {{ file.completeness.has_cover ? '封面✓' : '缺封面' }}
                    </span>
                    <span
                      class="rounded-full px-2 py-0.5 text-[10px] font-medium"
                      :class="
                        file.completeness.has_summary_or_lyrics
                          ? 'bg-emerald-500/15 text-emerald-500'
                          : 'bg-[var(--ctp-surface0)]/80 text-[var(--ctp-subtext0)]'
                      "
                    >
                      {{
                        file.media_kind === 'music'
                          ? file.completeness.has_summary_or_lyrics
                            ? '歌词✓'
                            : '无歌词'
                          : file.completeness.has_summary_or_lyrics
                            ? '简介✓'
                            : '无简介'
                      }}
                    </span>
                    <span
                      class="rounded-full px-2 py-0.5 text-[10px] font-medium"
                      :class="
                        file.completeness.has_nfo_or_tag_block
                          ? 'bg-emerald-500/15 text-emerald-500'
                          : 'bg-[var(--ctp-surface0)]/80 text-[var(--ctp-subtext0)]'
                      "
                    >
                      {{
                        file.media_kind === 'video'
                          ? file.completeness.has_nfo_or_tag_block
                            ? 'NFO✓'
                            : '无NFO'
                          : file.media_kind === 'comic'
                            ? file.completeness.has_nfo_or_tag_block
                              ? 'ComicInfo✓'
                              : '无XML'
                            : file.completeness.has_nfo_or_tag_block
                              ? '元数据✓'
                              : '待完善'
                      }}
                    </span>
                  </div>

                  <div class="hidden w-16 shrink-0 text-right font-mono text-[11px] text-[var(--ctp-subtext0)] lg:block">
                    {{ formatBytes(file.file_size) }}
                  </div>

                  <button
                    v-if="
                      file.media_kind === 'book' &&
                      (file.extension === 'txt' || file.extension === 'epub')
                    "
                    type="button"
                    class="shrink-0 rounded-xl p-2 text-[var(--ctp-subtext0)] opacity-80 transition hover:bg-[var(--ctp-accent-soft)] hover:text-[var(--ctp-accent)]"
                    :title="
                      file.extension === 'txt'
                        ? '编辑目录与正文 / 转为 EPUB'
                        : '编辑目录与正文'
                    "
                    @click.stop="openBookContentEditor(file)"
                  >
                    <LuFileText class="h-4 w-4" />
                  </button>

                  <button
                    type="button"
                    class="shrink-0 rounded-xl p-2 text-[var(--ctp-subtext0)] opacity-80 transition hover:bg-[var(--ctp-accent-soft)] hover:text-[var(--ctp-accent)]"
                    title="添加刮削任务（本地信息 → 高优先级源 → 低优先级源）"
                    @click.stop="enqueueScrapeFiles([file])"
                  >
                    <LuSparkles class="h-4 w-4" />
                  </button>
                </div>
              </div>
            </template>
          </div>
        </div>

        <!-- Right Drawer: Metadata & Stream Inspector (Available on Home & Items) -->
        <div
          v-if="(activeNav === 'home' || activeNav === 'items') && (activeDetail || loadingInspector)"
          class="fixed inset-y-0 right-0 z-40 flex w-full max-w-full bg-[var(--ctp-canvas)] shadow-2xl lg:static lg:z-auto lg:w-auto lg:shadow-none"
        >
          <div
            v-if="loadingInspector && !activeDetail"
            class="flex h-full w-full items-center justify-center px-12 text-xs text-[var(--ctp-subtext0)] lg:w-[440px]"
          >
            正在解析原文件流与元数据…
          </div>
          <MetadataInspectorDrawer
            v-else-if="activeDetail"
            ref="inspectorRef"
            :detail="activeDetail"
            :artwork-version="artworkVersion"
            @close="activeDetail = null; inspectingPath = null"
            @updated="handleDetailUpdated"
            @open-scrape="(d) => openManualScrapeModal(d.entry)"
            @open-content-editor="openBookContentEditor"
          />
        </div>
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

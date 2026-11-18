<script setup lang="ts">
import { computed, onBeforeUnmount, onMounted, ref, watch, type Component } from 'vue'
import { useI18n } from 'vue-i18n'
import {
  LuAlertCircle,
  LuAlertTriangle,
  LuArrowUp,
  LuBookOpen,
  LuCheckCircle2,
  LuChevronRight,
  LuClapperboard,
  LuDisc3,
  LuFileSignature,
  LuFileText,
  LuFolder,
  LuFolderOpen,
  LuFolderPlus,
  LuHardDrive,
  LuImage,
  LuLayers,
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
import { useToolTaskStore } from '../tasks'
import { formatBytes, useToolText } from '../i18n'
import type {
  DirectorySnapshot,
  ManagedRootDto,
  MediaFileDetail,
  MediaFileEntry,
  PublicSettingsDto,
  ToolStatusDto,
} from '../types'
import BatchRenameModal from './BatchRenameModal.vue'
import MetadataInspectorDrawer from './MetadataInspectorDrawer.vue'
import OnlineScrapeModal from './OnlineScrapeModal.vue'
import ToolSettingsModal, { type ToolSettingsTab } from './ToolSettingsModal.vue'

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

type BrowseViewMode = 'flat' | 'folders'

const collapsed = ref(localStorage.getItem('xmedia_tool.sidebar_collapsed') === '1')
const viewMode = ref<BrowseViewMode>(
  localStorage.getItem('xmedia_tool.view_mode') === 'folders' ? 'folders' : 'flat',
)
const mobileMenuOpen = ref(false)
const activeDomain = ref<MediaKind>(loadSavedDomain())
const completenessFilter = ref<CompletenessFilter>('all')
const searchQuery = ref('')
const searchInputRef = ref<HTMLInputElement | null>(null)

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

const settingsModalOpen = ref(false)
const settingsInitialTab = ref<ToolSettingsTab>('appearance')
const scrapeModalOpen = ref(false)
const scrapeQueue = ref<MediaFileEntry[]>([])
const scrapeQueueIndex = ref(0)
const renameModalOpen = ref(false)

const isCompact = computed(() => collapsed.value && !mobileMenuOpen.value)

const domainTabs = computed<Array<{ value: MediaKind; label: string; icon: Component }>>(() => [
  { value: 'book', label: txt.value.domains.book, icon: LuBookOpen },
  { value: 'comic', label: txt.value.domains.comic, icon: LuLayers },
  { value: 'music', label: txt.value.domains.music, icon: LuDisc3 },
  { value: 'video', label: txt.value.domains.video, icon: LuClapperboard },
])

const domainLabels = computed(() => txt.value.domainLabels[activeDomain.value])

// Strictly isolated roots per domain
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
  return c.has_title && c.has_creator && c.has_cover && c.has_summary_or_lyrics && c.has_nfo_or_tag_block
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

  // If current domain has no roots yet, auto-pick the first domain that has configured roots
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

  const rootLabel =
    settings.value?.roots.find((r) => r.id === rootId)?.name || '媒体目录'
  const taskId = `browse:${reqDomain}`
  taskStore.startTask({
    id: taskId,
    kind: 'browse',
    title: `读取目录 · ${relPath || rootLabel}`,
    detail: relPath || rootLabel,
    ephemeral: true,
  })

  let candidateRel = relPath
  let snap: DirectorySnapshot | null = null
  try {
    // Walk up parent segments when empty source folders were removed by archive/rename
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
  if (activeRootId.value === rootId && currentRelPath.value === '') return
  if (inspectorRef.value && !(await inspectorRef.value.confirmDiscardIfDirty())) {
    return
  }
  activeRootId.value = rootId
  currentRelPath.value = ''
  activeDetail.value = null
  inspectingPath.value = null
  mobileMenuOpen.value = false
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
  // Update active refs BEFORE `loadDirectory` so moving/archiving keeps the drawer anchored
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

async function handleSelectScrapeQueueIndex(nextIdx: number): Promise<void> {
  const target = scrapeQueue.value[nextIdx]
  if (!target) return
  scrapeQueueIndex.value = nextIdx
  await inspectEntry(target)
}

async function openScrapeForEntry(entry?: MediaFileEntry): Promise<void> {
  if (entry) {
    if (inspectorRef.value && activeDetail.value?.entry.rel_path !== entry.rel_path) {
      if (!(await inspectorRef.value.confirmDiscardIfDirty())) return
    }
    scrapeQueue.value = [entry]
    scrapeQueueIndex.value = 0
    if (activeDetail.value?.entry.rel_path !== entry.rel_path) {
      await inspectEntry(entry)
    }
    if (activeDetail.value) {
      scrapeModalOpen.value = true
    }
    return
  }

  const queue =
    selectedPaths.value.size > 0
      ? filteredFiles.value.filter((f) => selectedPaths.value.has(f.rel_path))
      : activeDetail.value
        ? [activeDetail.value.entry]
        : [...filteredFiles.value]

  if (queue.length === 0) return
  scrapeQueue.value = queue
  scrapeQueueIndex.value = 0
  if (activeDetail.value?.entry.rel_path !== queue[0]!.rel_path) {
    await inspectEntry(queue[0]!)
  }
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
  settingsModalOpen.value = true
}

function handleKeydown(ev: KeyboardEvent): void {
  if ((ev.metaKey || ev.ctrlKey) && ev.key.toLowerCase() === 'k') {
    ev.preventDefault()
    searchInputRef.value?.focus()
  }
}

onMounted(() => {
  window.addEventListener('keydown', handleKeydown)
  void loadInitial()
})

onBeforeUnmount(() => {
  window.removeEventListener('keydown', handleKeydown)
})
</script>

<template>
  <div
    class="relative flex h-screen w-screen flex-col overflow-hidden bg-[var(--ctp-shell)] text-[var(--ctp-text)]"
  >
    <!-- Full-Width Top Bar (Aligned with xmedia TopBar) -->
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

        <div class="flex items-center gap-2.5">
          <div
            class="flex h-9 w-9 items-center justify-center rounded-xl bg-[var(--ctp-accent)] text-[var(--ctp-on-accent)] shadow-sm"
          >
            <LuWrench class="h-[18px] w-[18px]" />
          </div>
          <div class="hidden lg:block">
            <div class="text-sm leading-tight font-bold tracking-tight">{{ txt.brand }}</div>
            <div class="text-[10px] text-[var(--ctp-subtext0)]">{{ txt.brandSub }}</div>
          </div>
        </div>

        <!-- Strictly Separated 4-Domain Capsule Switcher -->
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

      <!-- Center: Pill-Shaped Search Filter with ⌘K -->
      <div class="relative mx-auto max-w-md flex-1 lg:max-w-lg">
        <LuSearch
          class="pointer-events-none absolute top-1/2 left-4 h-4 w-4 -translate-y-1/2 text-[var(--ctp-subtext0)]"
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
          class="flex h-9 items-center gap-1.5 rounded-xl border border-[var(--ctp-surface0)] bg-[var(--ctp-mantle)]/80 px-3 text-xs font-semibold text-[var(--ctp-text)] transition hover:border-[var(--ctp-accent)]"
          @click="openSettingsTab('appearance')"
        >
          <LuSettings class="h-4 w-4 text-[var(--ctp-accent)]" />
          <span class="hidden sm:inline">{{ txt.actions.settings }}</span>
        </button>
      </div>
    </header>

    <!-- Middle Body: Collapsible Domain Sidebar + Flush Rounded Main Canvas -->
    <div class="relative flex min-h-0 flex-1 overflow-hidden">
      <div
        v-if="mobileMenuOpen"
        class="fixed inset-0 z-30 bg-black/45 md:hidden"
        @click="mobileMenuOpen = false"
      />

      <!-- Domain-Specific Sidebar -->
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

        <nav class="flex-1 space-y-4 overflow-y-auto px-3 pt-1 pb-4">
          <!-- Domain Completeness Filters -->
          <div class="space-y-0.5">
            <div
              v-if="!isCompact"
              class="px-3 pb-1 text-[10px] font-semibold tracking-wider uppercase text-[var(--ctp-subtext0)]"
            >
              {{ txt.filters.title }}
            </div>
            <button
              v-for="item in [
                { id: 'all' as CompletenessFilter, label: txt.filters.all, count: stats.total, icon: LuLayers },
                { id: 'complete' as CompletenessFilter, label: txt.filters.complete, count: stats.complete, icon: LuCheckCircle2 },
                { id: 'incomplete' as CompletenessFilter, label: txt.filters.incomplete, count: stats.incomplete, icon: LuAlertCircle },
                { id: 'missing_cover' as CompletenessFilter, label: domainLabels.missingCover, count: stats.missingCover, icon: LuImage },
                { id: 'missing_summary' as CompletenessFilter, label: domainLabels.missingSummary, count: stats.missingSummary, icon: LuFileText },
                { id: 'missing_tag' as CompletenessFilter, label: domainLabels.missingTagBlock, count: stats.missingTag, icon: LuSparkles },
              ]"
              :key="item.id"
              type="button"
              class="flex w-full items-center gap-3 rounded-xl px-3 py-2 text-xs font-medium transition"
              :class="[
                isCompact ? 'md:justify-center md:px-0' : '',
                completenessFilter === item.id
                  ? 'bg-[var(--ctp-accent-soft)] font-semibold text-[var(--ctp-accent)]'
                  : 'text-[var(--ctp-subtext1)] hover:bg-[var(--ctp-surface0)]/60 hover:text-[var(--ctp-text)]',
              ]"
              :title="isCompact ? item.label : undefined"
              @click="completenessFilter = item.id"
            >
              <component :is="item.icon" class="h-4 w-4 shrink-0" />
              <template v-if="!isCompact">
                <span class="flex-1 truncate text-left">{{ item.label }}</span>
                <span class="font-mono text-[11px] opacity-75">{{ item.count }}</span>
              </template>
            </button>
          </div>

          <!-- Isolated Domain Roots -->
          <div class="space-y-1">
            <div
              v-if="!isCompact"
              class="flex items-center justify-between px-3 text-[10px] font-semibold tracking-wider uppercase text-[var(--ctp-subtext0)]"
            >
              <span>{{ txt.roots.title }}</span>
              <button
                type="button"
                class="rounded-md p-0.5 text-[var(--ctp-subtext0)] hover:text-[var(--ctp-accent)]"
                :title="txt.roots.add"
                @click="openSettingsTab('roots')"
              >
                <LuFolderPlus class="h-3.5 w-3.5" />
              </button>
            </div>

            <button
              v-for="r in domainRoots"
              :key="r.id"
              type="button"
              class="flex w-full items-center gap-2.5 rounded-xl px-3 py-2 text-xs font-medium transition"
              :class="[
                isCompact ? 'md:justify-center md:px-0' : '',
                activeRootId === r.id
                  ? 'bg-[var(--ctp-accent-soft)] font-semibold text-[var(--ctp-accent)]'
                  : 'text-[var(--ctp-subtext1)] hover:bg-[var(--ctp-surface0)]/60 hover:text-[var(--ctp-text)]',
              ]"
              :title="r.path"
              @click="selectRoot(r.id)"
            >
              <LuHardDrive class="h-4 w-4 shrink-0" />
              <span v-if="!isCompact" class="flex-1 truncate text-left">{{ r.name }}</span>
            </button>
          </div>

          <!-- Subfolders in Current Domain Root -->
          <div
            v-if="!isCompact && snapshot && (snapshot.parent_rel_path !== null || snapshot.folders.length > 0)"
            class="space-y-1"
          >
            <div
              class="px-3 text-[10px] font-semibold tracking-wider uppercase text-[var(--ctp-subtext0)]"
            >
              {{ txt.roots.foldersTitle }}
            </div>

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
              class="flex w-full items-center gap-2.5 rounded-xl px-3 py-1.5 text-xs text-[var(--ctp-subtext1)] transition hover:bg-[var(--ctp-surface0)]/60 hover:text-[var(--ctp-text)]"
              @click="enterFolder(folder.rel_path)"
            >
              <LuFolder class="h-3.5 w-3.5 shrink-0 text-[var(--ctp-accent)]" />
              <span class="flex-1 truncate text-left">{{ folder.name }}</span>
              <span class="font-mono text-[10px] text-[var(--ctp-subtext0)]">
                {{ folder.media_file_count }}
              </span>
            </button>
          </div>
        </nav>
      </aside>

      <!-- Main Flush Rounded Top-Left Canvas + Optional Right Inspector -->
      <main
        class="flex min-w-0 flex-1 overflow-hidden border-t border-[var(--ctp-surface0)]/65 bg-[var(--ctp-canvas)] md:rounded-tl-3xl md:border-l"
      >
        <div class="flex min-w-0 flex-1 flex-col overflow-y-auto p-4 sm:p-6">
          <!-- Domain-Specific Empty Roots Onboarding -->
          <div
            v-if="settings && domainRoots.length === 0"
            class="m-auto flex max-w-md flex-col items-center rounded-3xl border border-dashed border-[var(--ctp-surface0)] bg-[var(--ctp-mantle)]/40 p-8 text-center"
          >
            <div
              class="mb-4 flex h-14 w-14 items-center justify-center rounded-2xl bg-[var(--ctp-accent-soft)] text-[var(--ctp-accent)]"
            >
              <LuFolderOpen class="h-7 w-7" />
            </div>
            <h2 class="text-base font-bold text-[var(--ctp-text)]">
              {{ domainLabels.emptyRootsTitle }}
            </h2>
            <p class="mt-1.5 text-xs leading-relaxed text-[var(--ctp-subtext0)]">
              {{ domainLabels.emptyRootsDesc }}
            </p>
            <button
              type="button"
              class="mt-5 inline-flex items-center gap-2 rounded-xl bg-[var(--ctp-accent)] px-4 py-2.5 text-xs font-semibold text-[var(--ctp-on-accent)] shadow-sm transition hover:opacity-95"
              @click="openSettingsTab('roots')"
            >
              <LuFolderPlus class="h-4 w-4" />
              <span>{{ txt.roots.add }}</span>
            </button>
          </div>

          <template v-else>
            <!-- Top 4 Domain Stat Cards -->
            <div class="mb-5 grid grid-cols-2 gap-3 xl:grid-cols-4">
              <button
                type="button"
                class="card-soft flex items-center justify-between rounded-2xl bg-[var(--ctp-mantle)]/55 p-4 text-left transition hover:border-[var(--ctp-accent)]/50"
                @click="completenessFilter = 'all'"
              >
                <div>
                  <div class="text-xs text-[var(--ctp-subtext0)]">{{ txt.cards.totalFiles }}</div>
                  <div class="mt-1 text-2xl font-bold tracking-tight text-[var(--ctp-text)]">
                    {{ stats.total }}
                  </div>
                </div>
                <div
                  class="flex h-10 w-10 items-center justify-center rounded-xl bg-[var(--ctp-accent-soft)] text-[var(--ctp-accent)]"
                >
                  <LuBookOpen v-if="activeDomain === 'book'" class="h-5 w-5" />
                  <LuLayers v-else-if="activeDomain === 'comic'" class="h-5 w-5" />
                  <LuDisc3 v-else-if="activeDomain === 'music'" class="h-5 w-5" />
                  <LuClapperboard v-else class="h-5 w-5" />
                </div>
              </button>

              <button
                type="button"
                class="card-soft flex items-center justify-between rounded-2xl bg-[var(--ctp-mantle)]/55 p-4 text-left transition hover:border-emerald-500/50"
                @click="completenessFilter = 'complete'"
              >
                <div>
                  <div class="text-xs text-[var(--ctp-subtext0)]">{{ txt.cards.completeFiles }}</div>
                  <div class="mt-1 text-2xl font-bold tracking-tight text-emerald-500">
                    {{ stats.complete }}
                  </div>
                </div>
                <div
                  class="flex h-10 w-10 items-center justify-center rounded-xl bg-emerald-500/15 text-emerald-500"
                >
                  <LuCheckCircle2 class="h-5 w-5" />
                </div>
              </button>

              <button
                type="button"
                class="card-soft flex items-center justify-between rounded-2xl bg-[var(--ctp-mantle)]/55 p-4 text-left transition hover:border-amber-500/50"
                @click="completenessFilter = 'missing_cover'"
              >
                <div>
                  <div class="text-xs text-[var(--ctp-subtext0)]">
                    {{ domainLabels.missingCover }}
                  </div>
                  <div class="mt-1 text-2xl font-bold tracking-tight text-amber-500">
                    {{ stats.missingCover }}
                  </div>
                </div>
                <div
                  class="flex h-10 w-10 items-center justify-center rounded-xl bg-amber-500/15 text-amber-500"
                >
                  <LuImage class="h-5 w-5" />
                </div>
              </button>

              <button
                type="button"
                class="card-soft flex items-center justify-between rounded-2xl bg-[var(--ctp-mantle)]/55 p-4 text-left transition hover:border-[var(--ctp-accent)]/50"
                @click="completenessFilter = 'missing_summary'"
              >
                <div>
                  <div class="text-xs text-[var(--ctp-subtext0)]">
                    {{ domainLabels.missingSummary }}
                  </div>
                  <div class="mt-1 text-2xl font-bold tracking-tight text-[var(--ctp-accent)]">
                    {{ stats.missingSummary }}
                  </div>
                </div>
                <div
                  class="flex h-10 w-10 items-center justify-center rounded-xl bg-[var(--ctp-accent-soft)] text-[var(--ctp-accent)]"
                >
                  <LuFileText class="h-5 w-5" />
                </div>
              </button>
            </div>

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

                <button
                  type="button"
                  :disabled="filteredFiles.length === 0"
                  class="inline-flex items-center gap-1.5 rounded-xl bg-[var(--ctp-accent-soft)] px-3 py-1.5 text-xs font-semibold text-[var(--ctp-accent)] transition hover:opacity-90 disabled:opacity-45"
                  @click="openScrapeForEntry()"
                >
                  <LuSparkles class="h-3.5 w-3.5" />
                  <span>
                    {{
                      selectedPaths.size > 1
                        ? `队列刮削 (${selectedPaths.size})`
                        : txt.actions.onlineScrape
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

            <!-- Subfolder Cards (shown in Folder View when not actively filtering) -->
            <div
              v-if="viewMode === 'folders' && !hasActiveSearchOrFilter && snapshot && snapshot.folders.length > 0"
              class="mb-4 grid grid-cols-2 gap-2.5 sm:grid-cols-3 lg:grid-cols-4"
            >
              <button
                v-for="folder in snapshot.folders"
                :key="`grid-${folder.rel_path}`"
                type="button"
                class="flex items-center gap-3 rounded-2xl border border-[var(--ctp-surface0)]/75 bg-[var(--ctp-mantle)]/40 px-3.5 py-2.5 text-left transition hover:border-[var(--ctp-accent)]/60 hover:bg-[var(--ctp-surface0)]/45"
                @click="enterFolder(folder.rel_path)"
              >
                <LuFolder class="h-5 w-5 shrink-0 text-[var(--ctp-accent)]" />
                <div class="min-w-0 flex-1">
                  <div class="truncate text-xs font-semibold text-[var(--ctp-text)]">
                    {{ folder.name }}
                  </div>
                  <div class="text-[11px] text-[var(--ctp-subtext0)]">
                    {{ folder.media_file_count }} 个文件
                  </div>
                </div>
              </button>
            </div>

            <!-- Domain Media Files List -->
            <div
              v-if="loadingDir && !snapshot"
              class="py-16 text-center text-xs text-[var(--ctp-subtext0)]"
            >
              正在扫描目录与解析原文件元信息…
            </div>
            <div
              v-else-if="filteredFiles.length === 0"
              class="rounded-3xl border border-dashed border-[var(--ctp-surface0)] py-16 text-center text-xs text-[var(--ctp-subtext0)]"
            >
              当前目录与筛选条件下暂无匹配的{{ txt.domains[activeDomain] }}文件
            </div>
            <div v-else class="space-y-1.5">
              <div
                v-for="file in filteredFiles"
                :key="file.rel_path"
                class="group flex cursor-pointer items-center gap-3 rounded-2xl border px-3 py-2.5 transition"
                :class="
                  inspectingPath === file.rel_path
                    ? 'border-[var(--ctp-accent)] bg-[var(--ctp-accent-soft)]/55'
                    : 'border-[var(--ctp-surface0)]/65 bg-[var(--ctp-mantle)]/30 hover:border-[var(--ctp-surface1)] hover:bg-[var(--ctp-surface0)]/40'
                "
                @click="inspectEntry(file)"
              >
                <div @click.stop>
                  <AppCheckbox
                    :model-value="selectedPaths.has(file.rel_path)"
                    @change="toggleSelectFile(file.rel_path)"
                  />
                </div>

                <!-- Thumbnail -->
                <div
                  class="flex h-12 w-12 shrink-0 items-center justify-center overflow-hidden rounded-xl bg-[var(--ctp-mantle)]"
                >
                  <img
                    v-if="file.completeness.has_cover"
                    :src="
                      buildArtworkUrl(
                        file.root_id,
                        file.rel_path,
                        file.media_kind,
                        artworkVersion,
                      )
                    "
                    alt=""
                    class="h-full w-full object-cover"
                    loading="lazy"
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

                <!-- Title & Filename -->
                <div class="min-w-0 flex-1">
                  <div class="flex items-center gap-2">
                    <span class="truncate text-xs font-bold text-[var(--ctp-text)]">
                      {{ file.display_title }}
                    </span>
                    <span
                      class="shrink-0 rounded-md bg-[var(--ctp-canvas)] px-1.5 py-0.5 font-mono text-[10px] uppercase text-[var(--ctp-subtext0)]"
                    >
                      {{ file.extension }}
                    </span>
                    <span v-if="file.year" class="shrink-0 text-[11px] text-[var(--ctp-subtext0)]">
                      {{ file.year }}
                    </span>
                  </div>
                  <div class="mt-0.5 flex flex-wrap items-center gap-1.5 truncate text-[11px] text-[var(--ctp-subtext0)]">
                    <button
                      v-if="relativeDirBadge(file.rel_path)"
                      type="button"
                      class="inline-flex max-w-64 shrink-0 items-center gap-1 truncate rounded-md bg-[var(--ctp-canvas)] px-1.5 py-0.5 text-[10px] font-medium text-[var(--ctp-subtext1)] transition hover:text-[var(--ctp-accent)]"
                      :title="`点击进入子目录：${parentRelOf(file.rel_path)}`"
                      @click.stop="enterFolder(parentRelOf(file.rel_path), true)"
                    >
                      <LuFolder class="h-3 w-3 shrink-0 text-[var(--ctp-accent)]" />
                      <span class="truncate">{{ relativeDirBadge(file.rel_path) }}</span>
                    </button>
                    <span v-if="file.display_subtitle" class="truncate">{{ file.display_subtitle }} ·</span>
                    <span class="truncate font-mono">{{ file.file_name }}</span>
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
                  type="button"
                  class="shrink-0 rounded-xl p-2 text-[var(--ctp-subtext0)] opacity-80 transition hover:bg-[var(--ctp-accent-soft)] hover:text-[var(--ctp-accent)]"
                  title="在线刮削此文件"
                  @click.stop="openScrapeForEntry(file)"
                >
                  <LuSparkles class="h-4 w-4" />
                </button>
              </div>
            </div>
          </template>
        </div>

        <!-- Right Drawer: Metadata & Stream Inspector -->
        <div
          v-if="activeDetail || loadingInspector"
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
            @open-scrape="(d) => openScrapeForEntry(d.entry)"
          />
        </div>
      </main>
    </div>

    <!-- Modals -->
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

    <ToolSettingsModal
      :open="settingsModalOpen"
      :settings="settings"
      :active-domain="activeDomain"
      :initial-tab="settingsInitialTab"
      @close="settingsModalOpen = false"
      @updated="handleSettingsUpdated"
    />

    <!-- Bottom-Right Floating Task Progress Panel -->
    <FloatingTaskShelf
      high-z-index
      :tasks="taskStore.shelfItems"
      :collapsed="taskStore.collapsed"
      @update:collapsed="taskStore.collapsed = $event"
      @clear-completed="taskStore.clearCompleted"
      @dismiss-all="taskStore.dismissAll"
    />
  </div>
</template>

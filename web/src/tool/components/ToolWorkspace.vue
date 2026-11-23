<script setup lang="ts">
import { computed, defineAsyncComponent, onBeforeUnmount, onMounted, reactive, ref, watch, type Component } from 'vue'
import { useI18n } from 'vue-i18n'
import { LuBookOpen, LuClapperboard, LuDisc3, LuLayers } from 'vue-icons-plus/lu'
import { useEscapeStack } from '@/composables/useEscapeStack'
import { useFocusTrap } from '@/composables/useFocusTrap'
import { useSnackbarStore } from '@/stores/snackbar'
import type { MediaKind } from '@/types/api'
import { stripAriaModalOnLeave } from '@/utils/dom'
import { toolApi } from '../api'
import { entryCompositeKey } from '../completeness'
import { useToolTaskStore } from '../tasks'
import type {
  DirectorySnapshot,
  ManagedRootDto,
  MediaFileDetail,
  PublicSettingsDto,
  ToolStatusDto,
} from '../types'
import type MetadataInspectorDrawerType from './MetadataInspectorDrawer.vue'
import type ScrapeWorkbenchViewType from './scrape/ScrapeWorkbenchView.vue'
import ToolHomeView from './ToolHomeView.vue'
import ToolItemsBrowser from './ToolItemsBrowser.vue'
import type { ToolSettingsTab } from './ToolSettingsView.vue'
import ToolSidebar from './ToolSidebar.vue'
import ToolTopHeader from './ToolTopHeader.vue'
import {
  PAGE_CHUNK_SIZE,
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
import ToolWorkspaceModals from './workspace/ToolWorkspaceModals.vue'
import { useWorkspaceDelete } from './workspace/useWorkspaceDelete'
import { useWorkspaceDirectory } from './workspace/useWorkspaceDirectory'
import { useWorkspaceFiltering } from './workspace/useWorkspaceFiltering'
import { useWorkspaceModals } from './workspace/useWorkspaceModals'
import { useWorkspaceShortcuts } from './workspace/useWorkspaceShortcuts'
import { useWorkspaceWorkbench } from './workspace/useWorkspaceWorkbench'

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
const { t } = useI18n()

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
  typeof window !== 'undefined' ? window.matchMedia('(min-width: 600px)') : null
const isDesktop = ref(desktopMediaQuery ? desktopMediaQuery.matches : true)
function handleDesktopBreakpointChange(ev: MediaQueryListEvent): void {
  isDesktop.value = ev.matches
  if (ev.matches) {
    mobileMenuOpen.value = false
  }
}

const largeMediaQuery =
  typeof window !== 'undefined' ? window.matchMedia('(min-width: 1200px)') : null
const isLargeDesktop = ref(largeMediaQuery ? largeMediaQuery.matches : true)
function handleLargeBreakpointChange(ev: MediaQueryListEvent): void {
  isLargeDesktop.value = ev.matches
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
const inspectorOverlayRef = ref<HTMLElement | null>(null)
const scrapeWorkbenchRef = ref<InstanceType<typeof ScrapeWorkbenchViewType> | null>(null)
const scrapeWorkbenchActive = ref(false)
const scrapeWorkbenchPaths = ref<string[] | null>(null)
const loadingInspector = ref(false)
const artworkVersion = ref(1)
const fileArtworkVersions = reactive<Record<string, number>>({})
const settingsInitialTab = ref<ToolSettingsTab>(initialHash.settingsTab ?? 'appearance')
const workbenchFocusPaths = ref<Set<string> | null>(null)

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

const isCompact = computed(() => collapsed.value && !mobileMenuOpen.value)

const domainTabs = computed<Array<{ value: MediaKind; label: string; icon: Component }>>(() => [
  { value: 'book', label: t('tool.domains.book'), icon: LuBookOpen },
  { value: 'comic', label: t('tool.domains.comic'), icon: LuLayers },
  { value: 'music', label: t('tool.domains.music'), icon: LuDisc3 },
  { value: 'video', label: t('tool.domains.video'), icon: LuClapperboard },
])

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

const workbench = useWorkspaceWorkbench({
  activeDomain,
  activeNav,
  activeRootId,
  currentRelPath,
  domainRoots,
  snapshot,
  filteredFiles,
  selectedPaths,
  inspectingPath,
  activeDetail,
  completenessFilter,
  workbenchFocusPaths,
  mobileMenuOpen,
  scrapeWorkbenchActive,
  scrapeWorkbenchPaths,
  entrySelectionKey,
  bumpFileArtworkVersion,
  confirmActiveEditorClean,
  clearActiveDetail,
  setViewMode,
  switchDomain,
  loadDirectory,
  loadAllDomainRoots,
  reloadCurrentScope,
  inspectEntry,
  nextInspectSeq,
  currentInspectSeq,
  onOpenBookContentEditor: (entry) => modals.openBookContentEditor(entry),
})

const modals = useWorkspaceModals({
  activeDomain,
  activeRootId,
  domainRoots,
  snapshot,
  filteredFiles,
  checkedFiles,
  selectedEntriesForRename,
  selectedPaths,
  inspectingPath,
  activeDetail,
  artworkVersion,
  deleteDialogOpen,
  tasksDrawerOpen,
  resolveModalRootId,
  entrySelectionKey,
  parseSelectionKey,
  bumpFileArtworkVersion,
  clearActiveDetail,
  reloadCurrentScope,
  inspectEntry,
  handleDetailUpdated: workbench.handleDetailUpdated,
  openScrapeWorkbench: workbench.openScrapeWorkbench,
  enqueueScrapeFiles: workbench.enqueueScrapeFiles,
  bumpScrapeStrategyRev: workbench.bumpScrapeStrategyRev,
})

useWorkspaceShortcuts({
  activeNav,
  scrapeWorkbenchActive,
  mobileMenuOpen,
  activeDetail,
  selectedPaths,
  inspectingPath,
  visibleLimit,
  filteredFiles,
  anyModalOpen: modals.anyModalOpen,
  focusSearch: () => topHeaderRef.value?.focusSearch(),
  saveInspector: () => {
    if (inspectorRef.value) void inspectorRef.value.save()
  },
  requestCloseInspector: () => {
    if (inspectorRef.value) void inspectorRef.value.requestClose()
  },
  isInspectorDirty: () => Boolean(inspectorRef.value?.isDirty),
  isWorkbenchDirty: () => Boolean(scrapeWorkbenchRef.value?.isDirty),
  entrySelectionKey,
  inspectEntry,
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

function toggleSidebar(): void {
  if (!isDesktop.value) {
    mobileMenuOpen.value = !mobileMenuOpen.value
    return
  }
  collapsed.value = !collapsed.value
  localStorage.setItem('xmedia_tool.sidebar_collapsed', collapsed.value ? '1' : '0')
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

const escapeStack = useEscapeStack()
const inspectorVisible = computed(
  () =>
    (activeNav.value === 'home' || activeNav.value === 'items') &&
    Boolean(activeDetail.value || loadingInspector.value),
)
const inspectorModalActive = computed(() => !isLargeDesktop.value && inspectorVisible.value)

async function closeInspectorOverlay(): Promise<void> {
  if (!(await confirmActiveEditorClean())) return
  clearActiveDetail()
}

useFocusTrap(inspectorOverlayRef, () => inspectorModalActive.value)
escapeStack.register(
  () => inspectorModalActive.value,
  () => {
    void closeInspectorOverlay()
  },
)

onMounted(() => {
  window.addEventListener('hashchange', handleHashChange)
  desktopMediaQuery?.addEventListener('change', handleDesktopBreakpointChange)
  largeMediaQuery?.addEventListener('change', handleLargeBreakpointChange)
  void loadInitial()
})

onBeforeUnmount(() => {
  window.removeEventListener('hashchange', handleHashChange)
  desktopMediaQuery?.removeEventListener('change', handleDesktopBreakpointChange)
  largeMediaQuery?.removeEventListener('change', handleLargeBreakpointChange)
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
        :entries="workbench.workbenchQueueEntries.value"
        :active-key="activeDetail ? entryCompositeKey(activeDetail.entry) : null"
        :saved-keys="workbench.scrapeWorkbenchSavedPaths.value"
        :artwork-version="artworkVersion"
        :artwork-versions="fileArtworkVersions"
        :has-prev="workbench.hasPrevInspect.value"
        :has-next="workbench.hasNextInspect.value"
        @close-mobile="mobileMenuOpen = false"
        @exit="workbench.exitScrapeWorkbench"
        @toggle-collapse="toggleSidebar"
        @select-entry="(entry) => { mobileMenuOpen = false; void inspectEntry(entry) }"
        @step="workbench.stepInspectingFile"
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
          :entries="workbench.workbenchQueueEntries.value"
          :saved-keys="workbench.scrapeWorkbenchSavedPaths.value"
          :artwork-version="artworkVersion"
          :artwork-versions="fileArtworkVersions"
          :sidebar-collapsed="collapsed"
          :has-prev="workbench.hasPrevInspect.value"
          :has-next="workbench.hasNextInspect.value"
          :position-label="workbench.inspectPositionLabel.value"
          :settings="settings"
          @exit="workbench.exitScrapeWorkbench"
          @toggle-sidebar="toggleSidebar"
          @select-entry="inspectEntry"
          @step="workbench.stepInspectingFile"
          @updated="workbench.handleDetailUpdated"
          @open-settings-scrapers="openSettingsTab('scrapers')"
          @settings-updated="handleSettingsUpdated"
          @open-history="modals.historyModalOpen.value = true"
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
                {{ t('tool.workspace.retryLoadConfig') }}
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
              @scrape-files="workbench.enqueueScrapeFiles"
              @open-rename-all="modals.openRenameModal(snapshot?.files ?? [])"
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
              :selected-comic-entries="modals.selectedComicEntries.value"
              :selected-music-entries="modals.selectedMusicEntries.value"
              :selected-video-entries="modals.selectedVideoEntries.value"
              :selected-entries-for-rename="selectedEntriesForRename"
              :webhook-enabled="Boolean(settings?.xmedia_webhook_enabled)"
              :notifying-rescan="notifyingRescan"
              :loading-dir="loadingDir"
              :dir-error="dirError"
              :snapshot="snapshot"
              :inspecting-path="inspectingPath"
              :artwork-version="artworkVersion"
              :artwork-versions="fileArtworkVersions"
              :last-scrape-summary="workbench.lastScrapeSummary.value"
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
              @quick-scrape-selection="modals.quickScrapeSelection"
              @configure-scrape-selection="modals.configureScrapeSelection"
              @open-scrape-workbench="(scoped) => workbench.openScrapeWorkbench(undefined, scoped)"
              @open-comic-series-tools="modals.openComicSeriesToolsModal"
              @open-music-toolbox="modals.openMusicToolbox"
              @open-video-toolbox="modals.openVideoToolbox"
              @open-dry-run="modals.openDryRunModal"
              @open-batch-metadata="modals.openBatchMetadataModal"
              @open-history="modals.historyModalOpen.value = true"
              @open-rename="modals.openRenameModal"
              @request-delete-files="requestDeleteFiles"
              @trigger-rescan="triggerManualRescan"
              @refresh-dir="reloadCurrentScope"
              @inspect-file="inspectEntry"
              @toggle-select-file="toggleSelectFile"
              @open-book-content-editor="modals.openBookContentEditor"
              @open-book-normalize-epub="modals.openBookNormalizeModal"
              @open-comic-page-manager="modals.openComicPageManager"
              @enqueue-scrape-files="workbench.enqueueScrapeFiles"
              @load-more="visibleLimit += PAGE_CHUNK_SIZE"
              @show-all="visibleLimit = filteredFiles.length"
            />
          </div>

          <Transition name="md-fade">
            <div
              v-if="inspectorModalActive"
              class="fixed inset-0 z-[var(--z-sheet)] bg-scrim/32 lg:hidden"
              aria-hidden="true"
              @mousedown="closeInspectorOverlay"
            />
          </Transition>

          <Transition name="md-sheet-right" @before-leave="stripAriaModalOnLeave">
            <div
              v-if="inspectorVisible"
              ref="inspectorOverlayRef"
              :role="inspectorModalActive ? 'dialog' : undefined"
              :aria-modal="inspectorModalActive ? 'true' : undefined"
              :tabindex="inspectorModalActive ? -1 : undefined"
              class="fixed inset-y-0 right-0 z-[var(--z-sheet)] flex w-full max-w-full bg-surface-container-low shadow-elevation-3 outline-none sm:w-[440px] lg:static lg:z-auto lg:w-auto lg:shadow-none"
            >
              <div
                v-if="loadingInspector && !activeDetail"
                class="flex h-full w-full items-center justify-center px-12 text-xs text-on-surface-variant lg:w-[440px]"
              >
                {{ t('tool.workspace.parsingFile') }}
              </div>
              <MetadataInspectorDrawer
                v-else-if="activeDetail"
                ref="inspectorRef"
                :detail="activeDetail"
                :artwork-version="getFileArtworkVersion(activeDetail.entry.root_id, activeDetail.entry.rel_path)"
                :has-prev="workbench.hasPrevInspect.value"
                :has-next="workbench.hasNextInspect.value"
                :position-label="workbench.inspectPositionLabel.value"
                :shortcuts-disabled="modals.anyModalOpen.value"
                @step="workbench.stepInspectingFile"
                @close="clearActiveDetail()"
                @updated="workbench.handleDetailUpdated"
                @open-workbench="(d) => workbench.openScrapeWorkbench(d.entry)"
                @open-content-editor="modals.openBookContentEditor"
                @open-normalize-epub="(e) => modals.openBookNormalizeModal([e])"
                @open-comic-pages="modals.openComicPageManager"
                @open-music-tools="(e) => modals.openMusicToolbox([e])"
                @open-video-tools="(e) => modals.openVideoToolbox([e])"
                @open-rename="(e) => modals.openRenameModal([e])"
                @request-delete="(e) => requestDeleteFiles([entrySelectionKey(e)], true)"
                @open-history="modals.historyModalOpen.value = true"
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

    <ToolWorkspaceModals
      :active-domain="activeDomain"
      :settings="settings"
      :selected-entries-for-rename="selectedEntriesForRename"
      :selected-comic-entries="modals.selectedComicEntries.value"
      :effective-modal-root-id="modals.effectiveModalRootId.value"
      :effective-history-root-id="modals.effectiveHistoryRootId.value"
      :effective-book-normalize-root-id="modals.effectiveBookNormalizeRootId.value"
      :effective-comic-root-id="modals.effectiveComicRootId.value"
      :effective-music-root-id="modals.effectiveMusicRootId.value"
      :effective-video-root-id="modals.effectiveVideoRootId.value"
      :effective-dry-run-root-id="modals.effectiveDryRunRootId.value"
      :batch-metadata-modal-open="modals.batchMetadataModalOpen.value"
      :history-modal-open="modals.historyModalOpen.value"
      :rename-modal-open="modals.renameModalOpen.value"
      :rename-target-entries="modals.renameTargetEntries.value"
      :rename-skipped-organized-count="modals.renameSkippedOrganizedCount.value"
      :book-editor-open="modals.bookEditorOpen.value"
      :book-editor-entry="modals.bookEditorEntry.value"
      :book-normalize-open="modals.bookNormalizeOpen.value"
      :book-normalize-entries="modals.bookNormalizeEntries.value"
      :comic-page-manager-open="modals.comicPageManagerOpen.value"
      :comic-page-manager-entry="modals.comicPageManagerEntry.value"
      :comic-series-tools-open="modals.comicSeriesToolsOpen.value"
      :music-tools-open="modals.musicToolsOpen.value"
      :music-tools-entries="modals.musicToolsEntries.value"
      :video-tools-open="modals.videoToolsOpen.value"
      :video-tools-entries="modals.videoToolsEntries.value"
      :auto-scrape-modal-open="modals.autoScrapeModalOpen.value"
      :auto-scrape-target-entries="modals.autoScrapeTargetEntries.value"
      :dry-run-modal-open="modals.dryRunModalOpen.value"
      :dry-run-target-entries="modals.dryRunTargetEntries.value"
      :dry-run-scrape-options="modals.dryRunScrapeOptions.value"
      @close-batch-metadata="modals.batchMetadataModalOpen.value = false"
      @batch-metadata-completed="modals.handleBatchMetadataCompleted"
      @open-history-from-batch="modals.batchMetadataModalOpen.value = false; modals.historyModalOpen.value = true"
      @close-history="modals.historyModalOpen.value = false"
      @history-restored="modals.handleHistoryRestored"
      @close-rename="modals.closeRenameModal()"
      @renamed="modals.handleRenamed"
      @close-book-editor="modals.bookEditorOpen.value = false"
      @book-content-saved="modals.handleBookContentSaved"
      @txt-converted="modals.handleTxtConvertedToEpub"
      @close-book-normalize="modals.bookNormalizeOpen.value = false"
      @book-normalize-completed="modals.handleBookNormalizeCompleted"
      @close-comic-page-manager="modals.comicPageManagerOpen.value = false"
      @detail-updated-with-art="(d) => workbench.handleDetailUpdated(d, true)"
      @close-comic-series-tools="modals.comicSeriesToolsOpen.value = false"
      @reload-scope="reloadCurrentScope"
      @close-music-tools="modals.musicToolsOpen.value = false"
      @close-video-tools="modals.videoToolsOpen.value = false"
      @close-auto-scrape="modals.autoScrapeModalOpen.value = false"
      @auto-scrape-confirmed="modals.handleAutoScrapeConfirmed"
      @auto-scrape-to-dry-run="modals.handleAutoScrapeToDryRun"
      @open-scraper-settings="modals.autoScrapeModalOpen.value = false; openSettingsTab('scrapers')"
      @close-dry-run="modals.closeDryRunModal()"
      @dry-run-confirmed="(entries, options) => workbench.enqueueScrapeFiles(entries, undefined, options)"
      @dry-run-manual-scrape="modals.handleDryRunManualScrape"
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
      @review-items="workbench.handleReviewUnmatchedItems"
      @inspect-item="workbench.handleInspectTaskItem"
      @open-book-editor="workbench.handleOpenTaskBookEditor"
    />
  </div>
</template>

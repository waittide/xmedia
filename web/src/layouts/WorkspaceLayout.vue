<script setup lang="ts">
import { computed, onBeforeUnmount, onMounted, ref, watch, type Component } from 'vue'
import { useRoute, useRouter } from 'vue-router'
import { useI18n } from 'vue-i18n'
import {
  LuBookOpen,
  LuClapperboard,
  LuDisc3,
  LuFileUp,
  LuFolderUp,
  LuLayers,
  LuListChecks,
  LuLogOut,
  LuMonitor,
  LuMoon,
  LuPanelLeftClose,
  LuPanelLeftOpen,
  LuSearch,
  LuSettings,
  LuSun,
  LuUpload,
  LuX,
} from 'vue-icons-plus/lu'
import { useEscapeStack } from '@/composables/useEscapeStack'
import { ensureDomainLocale } from '@/router'
import { useAppClientStore } from '@/stores/appServers'
import { useAuthStore } from '@/stores/auth'
import { useDomainStore } from '@/stores/domain'
import { useEventsStore } from '@/stores/events'
import { useOnlineDownloadsStore } from '@/stores/onlineDownloads'
import { useThemeStore } from '@/stores/theme'
import { getAcceptedUploadExtensions, useUploadStore } from '@/stores/uploads'
import type { MediaKind } from '@/types/api'
import { extractFilesFromDataTransfer, normalizePickedFiles } from '@/utils/fileDrop'

import { useCatalogStore } from '@/domains/book/store'
import { useSidebarStore as useComicSidebarStore } from '@/domains/comic/store'
import { usePlayerStore } from '@/domains/music/player'
import { useMusicStore } from '@/domains/music/store'
import { useVideoStore } from '@/domains/video/store'

import BackgroundTasksDrawer from '@/components/BackgroundTasksDrawer.vue'
import GlobalSearchModal from '@/components/GlobalSearchModal.vue'
import LayoutCustomizerModal from '@/components/LayoutCustomizerModal.vue'
import MdBadge from '@/components/ui/MdBadge.vue'
import MdCircularProgress from '@/components/ui/MdCircularProgress.vue'
import MdSegmentedButton from '@/components/ui/MdSegmentedButton.vue'
import WorkspaceSidebar from '@/layouts/WorkspaceSidebar.vue'
import { usePrimaryNavItems } from '@/layouts/useWorkspaceSidebar'

const { t } = useI18n()

const route = useRoute()
const router = useRouter()
const appClientStore = useAppClientStore()
const authStore = useAuthStore()
const domainStore = useDomainStore()
const eventsStore = useEventsStore()
const onlineDownloadsStore = useOnlineDownloadsStore()
const themeStore = useThemeStore()
const uploadStore = useUploadStore()
const escapeStack = useEscapeStack()

const bookStore = useCatalogStore()
const comicSidebarStore = useComicSidebarStore()
const musicStore = useMusicStore()
const playerStore = usePlayerStore()
const videoStore = useVideoStore()

const collapsed = ref(localStorage.getItem('xmedia.sidebar_collapsed') === '1')
const desktopMediaQuery =
  typeof window !== 'undefined' ? window.matchMedia('(min-width: 840px)') : null
const isDesktop = ref(desktopMediaQuery ? desktopMediaQuery.matches : true)
const mobileMenuOpen = ref(false)
const accountMenuOpen = ref(false)
const uploadMenuOpen = ref(false)
const tasksDrawerOpen = ref(false)
const layoutCustomizerOpen = ref(false)
const commandPaletteOpen = ref(false)
const dragDepth = ref(0)

const filePickerRef = ref<HTMLInputElement | null>(null)
const folderPickerRef = ref<HTMLInputElement | null>(null)
const mainScrollRef = ref<HTMLElement | null>(null)
const isContentScrolled = ref(false)
function onMainScroll(ev: Event): void {
  const el = ev.target as HTMLElement | null
  isContentScrolled.value = Boolean(el && el.scrollTop > 4)
}
const searchInputRef = ref<HTMLInputElement | null>(null)
const accountMenuRef = ref<HTMLElement | null>(null)
const uploadMenuRef = ref<HTMLElement | null>(null)

const folderInputAttrs: Record<string, string> = {
  webkitdirectory: '',
  directory: '',
}

const comicSearchQuery = ref('')
const videoSearchQuery = ref('')

// LIFO Escape stack: pressing Esc closes only the topmost active layer
escapeStack.register(
  () => mobileMenuOpen.value,
  () => {
    mobileMenuOpen.value = false
  },
)
escapeStack.register(
  () => uploadMenuOpen.value,
  () => {
    uploadMenuOpen.value = false
  },
)
escapeStack.register(
  () => accountMenuOpen.value,
  () => {
    accountMenuOpen.value = false
  },
)
escapeStack.register(
  () => playerStore.lyricsOpen,
  () => {
    playerStore.lyricsOpen = false
  },
)

const loadedDomains = new Set<MediaKind>()

watch(
  () => authStore.user?.id,
  () => {
    loadedDomains.clear()
    if (authStore.isAuthenticated) {
      void ensureDomainSidebarLoaded(domainStore.activeDomain)
    }
  },
)

watch(
  () => [authStore.isAuthenticated, authStore.user?.id] as const,
  ([isAuth]) => {
    if (isAuth && authStore.hasModule('book')) {
      void onlineDownloadsStore.refresh()
    }
  },
  { immediate: true },
)

async function ensureDomainSidebarLoaded(kind: MediaKind): Promise<void> {
  if (!authStore.isAuthenticated || !authStore.hasModule(kind) || loadedDomains.has(kind)) return
  loadedDomains.add(kind)
  try {
    if (kind === 'book') await bookStore.refreshSidebarData()
    else if (kind === 'comic') await comicSidebarStore.refresh()
    else if (kind === 'music') await musicStore.refreshSidebar()
    else if (kind === 'video') await videoStore.refreshSidebar()
  } catch {
    loadedDomains.delete(kind)
  }
}

let sidebarLoadTimer: ReturnType<typeof setTimeout> | null = null

watch(
  () => route.path,
  () => {
    if (mainScrollRef.value) {
      mainScrollRef.value.scrollTop = 0
    }
  },
)

watch(
  () => domainStore.activeDomain,
  (next) => {
    if (mainScrollRef.value) {
      mainScrollRef.value.scrollTop = 0
    }
    if (sidebarLoadTimer) clearTimeout(sidebarLoadTimer)
    if (loadedDomains.has(next)) {
      void ensureDomainSidebarLoaded(next)
    } else {
      sidebarLoadTimer = setTimeout(() => {
        void ensureDomainSidebarLoaded(next)
      }, 60)
    }
  },
  { immediate: true },
)

watch(
  () => [route.name, route.query.q] as const,
  ([name, q]) => {
    const str = typeof q === 'string' ? q : ''
    if (name === 'comic-series-list') {
      comicSearchQuery.value = str
    } else if (name === 'video-catalog') {
      videoSearchQuery.value = str
    } else if (name === 'music-search') {
      musicStore.globalSearchQuery = str
    }
  },
  { immediate: true },
)

const domainTabs = computed<Array<{ kind: MediaKind; label: string; icon: Component }>>(() => {
  const all: Array<{ kind: MediaKind; label: string; icon: Component }> = [
    { kind: 'book', label: t('app.domains.book'), icon: LuBookOpen },
    { kind: 'comic', label: t('app.domains.comic'), icon: LuLayers },
    { kind: 'music', label: t('app.domains.music'), icon: LuDisc3 },
    { kind: 'video', label: t('app.domains.video'), icon: LuClapperboard },
  ]
  return all.filter((item) => authStore.hasModule(item.kind))
})

const acceptedExtensions = computed(() =>
  getAcceptedUploadExtensions(authStore.enabledModules),
)

const showMusicBar = computed(
  () =>
    authStore.hasModule('music') &&
    (domainStore.activeDomain === 'music' || playerStore.currentTrack !== null),
)

const { primaryNavItems } = usePrimaryNavItems(() => {
  mobileMenuOpen.value = false
})

const showMobileBottomNav = computed(() => primaryNavItems.value.length > 0)

const isCompact = computed(() => isDesktop.value && collapsed.value)
const sidebarToggleExpanded = computed(() =>
  isDesktop.value ? !collapsed.value : mobileMenuOpen.value,
)
const sidebarToggleLabel = computed(() =>
  sidebarToggleExpanded.value ? t('nav.collapseSidebar') : t('nav.expandSidebar'),
)

async function switchDomain(kind: MediaKind): Promise<void> {
  const alreadyInDomain = domainStore.activeDomain === kind && route.meta.domain === kind
  const target = alreadyInDomain ? `/${kind}` : domainStore.getLastRoute(kind)
  await ensureDomainLocale(kind)
  domainStore.setActiveDomain(kind)
  mobileMenuOpen.value = false
  await router.push(target)
}

function handleSidebarButtonClick(): void {
  if (!isDesktop.value) {
    uploadMenuOpen.value = false
    accountMenuOpen.value = false
    mobileMenuOpen.value = !mobileMenuOpen.value
    return
  }
  collapsed.value = !collapsed.value
  localStorage.setItem('xmedia.sidebar_collapsed', collapsed.value ? '1' : '0')
}

// Unified search state across 4 domains
const activeSearchValue = computed({
  get(): string {
    const d = domainStore.activeDomain
    if (d === 'book') return bookStore.keyword
    if (d === 'comic') return comicSearchQuery.value
    if (d === 'music') return musicStore.globalSearchQuery
    return videoSearchQuery.value
  },
  set(val: string): void {
    const d = domainStore.activeDomain
    if (d === 'book') bookStore.keyword = val
    else if (d === 'comic') comicSearchQuery.value = val
    else if (d === 'music') musicStore.globalSearchQuery = val
    else videoSearchQuery.value = val
  },
})

const searchPlaceholder = computed(() => {
  const d = domainStore.activeDomain
  if (d === 'book') return t('common.searchBook')
  if (d === 'comic') return t('common.searchComic')
  if (d === 'music') return t('common.searchMusic')
  return t('common.searchVideo')
})

async function submitGlobalSearch(): Promise<void> {
  const d = domainStore.activeDomain
  if (d === 'book') {
    const q = bookStore.keyword.trim()
    bookStore.page = 1
    if (route.name !== 'book-catalog') {
      bookStore.activeSection = 'all'
    }
    const nextQuery: Record<string, string> = {}
    if (route.name === 'book-catalog') {
      for (const [k, v] of Object.entries(route.query)) {
        if (typeof v === 'string' && k !== 'q' && k !== 'page') {
          nextQuery[k] = v
        }
      }
    }
    if (q) nextQuery.q = q
    await router.push({ path: '/book/catalog', query: nextQuery })
  } else if (d === 'comic') {
    const q = comicSearchQuery.value.trim()
    searchInputRef.value?.blur()
    await router.push(q ? { path: '/comic/series', query: { q } } : '/comic/series')
  } else if (d === 'music') {
    const q = musicStore.globalSearchQuery.trim()
    if (!q) return
    await router.push({ path: '/music/search', query: { q } })
  } else {
    const q = videoSearchQuery.value.trim()
    await router.push(q ? { path: '/video/catalog', query: { q } } : '/video/catalog')
  }
}

function clearGlobalSearch(): void {
  const d = domainStore.activeDomain
  if (d === 'book') {
    bookStore.keyword = ''
    bookStore.page = 1
    if (route.name === 'book-catalog') {
      const nextQuery = { ...route.query }
      delete nextQuery.q
      delete nextQuery.page
      void router.replace({ path: '/book/catalog', query: nextQuery })
    }
  } else if (d === 'comic') {
    comicSearchQuery.value = ''
    if (route.name === 'comic-series-list' && route.query.q) {
      const nextQuery = { ...route.query }
      delete nextQuery.q
      void router.replace({ path: '/comic/series', query: nextQuery })
    }
  } else if (d === 'music') {
    musicStore.globalSearchQuery = ''
    if (route.name === 'music-search') {
      void router.push('/music/songs')
    }
  } else {
    videoSearchQuery.value = ''
    if (route.name === 'video-catalog' && route.query.q) {
      const nextQuery = { ...route.query }
      delete nextQuery.q
      void router.replace({ path: '/video/catalog', query: nextQuery })
    }
  }
}

// Upload & Drag-and-Drop
function toggleUploadMenu(): void {
  mobileMenuOpen.value = false
  accountMenuOpen.value = false
  uploadMenuOpen.value = !uploadMenuOpen.value
}

function triggerFilePicker(): void {
  uploadMenuOpen.value = false
  filePickerRef.value?.click()
}

function triggerFolderPicker(): void {
  uploadMenuOpen.value = false
  folderPickerRef.value?.click()
}

function handlePickedFiles(ev: Event): void {
  const input = ev.target as HTMLInputElement
  if (input.files && input.files.length > 0) {
    const { files, fromDirectory } = normalizePickedFiles(input.files, false)
    uploadStore.enqueueFiles(files, { fromDirectory })
    input.value = ''
  }
}

function handlePickedFolder(ev: Event): void {
  const input = ev.target as HTMLInputElement
  if (input.files && input.files.length > 0) {
    const { files } = normalizePickedFiles(input.files, true)
    uploadStore.enqueueFiles(files, { fromDirectory: true })
    input.value = ''
  }
}

function onDragEnter(ev: DragEvent): void {
  if (!authStore.isAdmin) return
  if (!ev.dataTransfer?.types.includes('Files')) return
  dragDepth.value += 1
}

function onDragLeave(): void {
  dragDepth.value = Math.max(0, dragDepth.value - 1)
}

async function onDropFiles(ev: DragEvent): Promise<void> {
  dragDepth.value = 0
  if (!authStore.isAdmin || !ev.dataTransfer) return
  const { files, fromDirectory } = await extractFilesFromDataTransfer(ev.dataTransfer)
  if (files.length > 0 || fromDirectory) {
    uploadStore.enqueueFiles(files, { fromDirectory })
  }
}

// Unified Global Background Task Metrics (Scans + Uploads + Online Downloads)
const globalActiveSummary = computed(() => {
  const progressValues: number[] = []
  let activeCount = 0

  for (const sc of eventsStore.scanTaskList) {
    if (sc.status !== 'running') continue
    activeCount += 1
    if (sc.total !== null && sc.total > 0) {
      progressValues.push(Math.min(100, Math.round((sc.scanned / sc.total) * 100)))
    }
  }

  for (const task of uploadStore.tasks) {
    if (task.status !== 'queued' && task.status !== 'uploading') continue
    activeCount += 1
    progressValues.push(Math.max(0, Math.min(100, task.progress)))
  }

  for (const job of onlineDownloadsStore.activeJobs) {
    activeCount += 1
    if (job.total_chapters > 0) {
      progressValues.push(
        Math.min(100, Math.round((job.processed_chapters / job.total_chapters) * 100)),
      )
    }
  }

  const progress =
    activeCount > 0 && progressValues.length > 0
      ? Math.round(progressValues.reduce((acc, v) => acc + v, 0) / progressValues.length)
      : null

  return { activeCount, progress }
})

const globalActiveTaskCount = computed(() => globalActiveSummary.value.activeCount)
const globalActiveProgress = computed(() => globalActiveSummary.value.progress)
const globalFailedTaskCount = computed(() => {
  const failedScans = eventsStore.scanTaskList.filter((sc) => sc.status === 'failed').length
  return failedScans + uploadStore.failedCount + onlineDownloadsStore.failedCount
})

const tasksButtonTooltip = computed(() => {
  if (globalActiveTaskCount.value > 0) {
    return t('transfers.openTasksActive', { count: globalActiveTaskCount.value })
  }
  return t('transfers.openTasks')
})

function openTasksDrawer(): void {
  uploadMenuOpen.value = false
  accountMenuOpen.value = false
  tasksDrawerOpen.value = true
  if (authStore.isAuthenticated && authStore.hasModule('book')) {
    void onlineDownloadsStore.refresh()
  }
}

// Theme & Account
function cycleThemeMode(): void {
  const order = ['system', 'light', 'dark'] as const
  const idx = order.indexOf(themeStore.themeMode)
  const next = order[(idx + 1) % order.length]!
  themeStore.setThemeMode(next)
}

const themeModeTooltip = computed(() => {
  if (themeStore.themeMode === 'light') return `${t('theme.modeTitle')}: ${t('theme.light')}`
  if (themeStore.themeMode === 'dark') return `${t('theme.modeTitle')}: ${t('theme.dark')}`
  return `${t('theme.modeTitle')}: ${t('theme.system')}`
})

const userInitials = computed(() => {
  const label = authStore.user?.display_name || authStore.user?.username || 'U'
  return label.slice(0, 1).toUpperCase()
})

async function handleSignOut(): Promise<void> {
  accountMenuOpen.value = false
  uploadMenuOpen.value = false
  tasksDrawerOpen.value = false
  playerStore.pause()
  eventsStore.disconnect()
  await authStore.logout()
  eventsStore.disconnect()
  await router.replace('/login')
}

function handleOutsideClick(ev: MouseEvent): void {
  const target = ev.target as Node
  if (accountMenuOpen.value && accountMenuRef.value && !accountMenuRef.value.contains(target)) {
    accountMenuOpen.value = false
  }
  if (uploadMenuOpen.value && uploadMenuRef.value && !uploadMenuRef.value.contains(target)) {
    uploadMenuOpen.value = false
  }
}

function handleGlobalKeydown(ev: KeyboardEvent): void {
  if (playerStore.lyricsOpen) return
  if ((ev.metaKey || ev.ctrlKey) && ev.key.toLowerCase() === 'k') {
    ev.preventDefault()
    commandPaletteOpen.value = !commandPaletteOpen.value
  } else if ((ev.metaKey || ev.ctrlKey) && ev.key.toLowerCase() === 'b') {
    ev.preventDefault()
    handleSidebarButtonClick()
  }
}

function handleDesktopBreakpointChange(ev: MediaQueryListEvent): void {
  isDesktop.value = ev.matches
  if (ev.matches) {
    mobileMenuOpen.value = false
  }
}

onMounted(() => {
  document.addEventListener('mousedown', handleOutsideClick)
  window.addEventListener('keydown', handleGlobalKeydown)
  desktopMediaQuery?.addEventListener('change', handleDesktopBreakpointChange)
})

onBeforeUnmount(() => {
  document.removeEventListener('mousedown', handleOutsideClick)
  window.removeEventListener('keydown', handleGlobalKeydown)
  desktopMediaQuery?.removeEventListener('change', handleDesktopBreakpointChange)
  if (sidebarLoadTimer) clearTimeout(sidebarLoadTimer)
})
</script>

<template>
  <div
    data-workspace-shell
    :data-has-player="showMusicBar ? '1' : '0'"
    class="relative flex h-screen w-screen flex-col overflow-hidden bg-[var(--md-sys-color-surface-container)] text-[var(--md-sys-color-on-surface)] supports-[height:100dvh]:h-dvh"
    @dragenter.prevent="onDragEnter"
    @dragover.prevent
    @dragleave.prevent="onDragLeave"
    @drop.prevent="onDropFiles"
  >
    <!-- Hidden Global Multi-Domain File & Folder Pickers -->
    <input
      ref="filePickerRef"
      type="file"
      multiple
      :accept="acceptedExtensions"
      class="hidden"
      @change="handlePickedFiles"
    />
    <input
      ref="folderPickerRef"
      type="file"
      multiple
      class="hidden"
      v-bind="folderInputAttrs"
      @change="handlePickedFolder"
    />

    <!-- Full-Window Drag & Drop Overlay -->
    <div
      v-if="dragDepth > 0"
      class="pointer-events-none fixed inset-3 z-[var(--z-modal)] flex flex-col items-center justify-center rounded-[28px] border-2 border-dashed border-[var(--md-sys-color-primary)] bg-[var(--md-sys-color-surface-container-high)]/92 backdrop-blur-md"
    >
      <div
        class="mb-3 flex h-14 w-14 items-center justify-center rounded-2xl bg-[var(--md-sys-color-primary-container)] text-[var(--md-sys-color-on-primary-container)]"
      >
        <LuUpload class="h-7 w-7" />
      </div>
      <p class="text-base font-bold text-[var(--md-sys-color-on-surface)]">
        {{ t('transfers.dropOverlayTitle') }}
      </p>
      <p class="mt-1 px-4 text-center text-xs text-[var(--md-sys-color-on-surface-variant)]">
        {{ t('transfers.dropOverlayHint') }}
      </p>
    </div>

    <!-- Full-Width MD3 Top App Bar (Scroll-triggered tonal transition) -->
    <header
      class="safe-pt safe-px relative z-30 shrink-0 transition-colors duration-[var(--md-sys-motion-duration-short)] select-none"
      :class="
        isContentScrolled
          ? 'bg-[var(--md-sys-color-surface-container)] shadow-[var(--md-sys-elevation-1)]'
          : 'bg-[var(--md-sys-color-surface)] md:bg-[var(--md-sys-color-surface-container)]'
      "
    >
      <div class="flex h-16 items-center justify-between gap-2.5 px-3 sm:gap-3 sm:px-4">
      <!-- Left: Rail/Drawer Toggle + Soft Tonal Brand + MD3 Domain Segmented Switcher -->
      <div class="flex shrink-0 items-center gap-2">
        <button
          type="button"
          class="md-icon-btn md-state"
          :title="sidebarToggleLabel"
          :aria-label="sidebarToggleLabel"
          :aria-expanded="sidebarToggleExpanded"
          @click="handleSidebarButtonClick"
        >
          <LuPanelLeftClose v-if="sidebarToggleExpanded" class="h-5 w-5" />
          <LuPanelLeftOpen v-else class="h-5 w-5" />
        </button>

        <RouterLink
          :to="'/' + domainStore.activeDomain"
          class="md-state flex items-center gap-2.5 rounded-full py-1 pr-2.5 pl-1"
          @click="mobileMenuOpen = false"
        >
          <div
            class="flex h-9 w-9 items-center justify-center rounded-2xl bg-[var(--md-sys-color-primary-container)] text-[var(--md-sys-color-on-primary-container)]"
          >
            <LuLayers class="h-[18px] w-[18px]" />
          </div>
          <span class="hidden text-base font-semibold tracking-tight text-[var(--md-sys-color-on-surface)] lg:inline">
            {{ t('app.name') }}
          </span>
        </RouterLink>

        <!-- MD3 Segmented Domain Switcher -->
        <div v-if="domainTabs.length > 1" class="ml-1 hidden md:block">
          <MdSegmentedButton
            :model-value="domainStore.activeDomain"
            :options="domainTabs.map((tab) => ({ value: tab.kind, label: tab.label, icon: tab.icon }))"
            size="sm"
            @update:model-value="switchDomain($event)"
          />
        </div>
      </div>

      <!-- Center: Unified MD3 Search Bar (sm+) -->
      <form
        class="relative mx-auto hidden max-w-md flex-1 sm:block lg:max-w-lg"
        @submit.prevent="submitGlobalSearch"
      >
        <LuSearch
          class="pointer-events-none absolute top-1/2 left-4 h-4 w-4 -translate-y-1/2 text-[var(--md-sys-color-on-surface-variant)]"
        />
        <input
          ref="searchInputRef"
          v-model="activeSearchValue"
          type="text"
          :placeholder="searchPlaceholder"
          class="h-11 w-full rounded-full bg-[var(--md-sys-color-surface-container-high)] pr-14 pl-11 text-[13px] text-[var(--md-sys-color-on-surface)] placeholder-[var(--md-sys-color-on-surface-variant)] outline-none transition duration-200 focus:bg-[var(--md-sys-color-surface-container-lowest)] focus:shadow-[var(--md-sys-elevation-2)]"
        />
        <button
          v-if="!activeSearchValue"
          type="button"
          class="md-state absolute top-1/2 right-3 inline-flex -translate-y-1/2 items-center rounded-full bg-[var(--md-sys-color-surface-container)] px-2.5 py-0.5 font-mono text-[10px] text-[var(--md-sys-color-on-surface-variant)] hover:text-[var(--md-sys-color-on-surface)]"
          :title="t('common.search') + ' (⌘K)'"
          :aria-label="t('common.search')"
          @click="commandPaletteOpen = true"
        >
          ⌘K
        </button>
        <button
          v-else
          type="button"
          class="md-icon-btn md-icon-btn-sm md-state absolute top-1/2 right-1.5 -translate-y-1/2"
          :aria-label="t('common.cancel')"
          @click="clearGlobalSearch"
        >
          <LuX class="h-4 w-4" />
        </button>
      </form>

      <!-- Right: Mobile Search Icon + Upload Menu + Background Tasks Ring Button + Theme + Avatar Menu -->
      <div class="flex shrink-0 items-center gap-1">
        <button
          type="button"
          class="md-icon-btn md-state sm:hidden"
          :title="t('common.search')"
          :aria-label="t('common.search')"
          @click="commandPaletteOpen = true"
        >
          <LuSearch class="h-[18px] w-[18px]" />
        </button>

        <!-- Upload Menu (Files / Folder) -->
        <div v-if="authStore.isAdmin" ref="uploadMenuRef" class="relative">
          <button
            type="button"
            class="md-icon-btn md-state"
            :class="
              uploadMenuOpen
                ? 'bg-[var(--md-sys-color-secondary-container)] text-[var(--md-sys-color-on-secondary-container)]'
                : ''
            "
            :title="t('common.uploadMedia')"
            :aria-label="t('common.uploadMedia')"
            :aria-expanded="uploadMenuOpen"
            aria-haspopup="menu"
            @click="toggleUploadMenu"
          >
            <LuUpload class="h-[18px] w-[18px]" />
          </button>

          <Transition name="md-dialog">
            <div
              v-if="uploadMenuOpen"
              role="menu"
              class="absolute top-12 right-0 z-[var(--z-popover)] w-44 rounded-2xl bg-[var(--md-sys-color-surface-container-high)] p-1.5 shadow-[var(--md-sys-elevation-3)]"
            >
              <button
                type="button"
                role="menuitem"
                class="md-state flex w-full items-center gap-2.5 rounded-full px-3 py-2 text-left text-xs font-medium text-[var(--md-sys-color-on-surface)]"
                @click="triggerFilePicker"
              >
                <LuFileUp class="h-4 w-4 shrink-0 text-[var(--md-sys-color-on-surface-variant)]" />
                <span>{{ t('common.uploadFiles') }}</span>
              </button>

              <button
                type="button"
                role="menuitem"
                class="md-state flex w-full items-center gap-2.5 rounded-full px-3 py-2 text-left text-xs font-medium text-[var(--md-sys-color-on-surface)]"
                @click="triggerFolderPicker"
              >
                <LuFolderUp class="h-4 w-4 shrink-0 text-[var(--md-sys-color-on-surface-variant)]" />
                <span>{{ t('common.uploadFolder') }}</span>
              </button>
            </div>
          </Transition>
        </div>

        <!-- Global Background Tasks Button with SVG Progress Ring -->
        <div class="relative inline-flex items-center justify-center">
          <button
            type="button"
            class="md-icon-btn md-state"
            :class="
              tasksDrawerOpen
                ? 'bg-[var(--md-sys-color-secondary-container)] text-[var(--md-sys-color-on-secondary-container)]'
                : globalActiveTaskCount > 0
                  ? 'text-[var(--md-sys-color-primary)]'
                  : ''
            "
            :title="tasksButtonTooltip"
            :aria-label="tasksButtonTooltip"
            @click="openTasksDrawer"
          >
            <LuListChecks class="h-[18px] w-[18px]" />
          </button>

          <MdCircularProgress
            v-if="globalActiveTaskCount > 0"
            :value="globalActiveProgress"
            :indeterminate="globalActiveProgress === null"
            :size="40"
            :stroke-width="2.5"
            class="pointer-events-none absolute inset-0"
          />

          <MdBadge
            v-if="globalActiveTaskCount > 0"
            :value="globalActiveTaskCount"
            :max="99"
            tone="error"
            class="pointer-events-none absolute -top-0.5 -right-0.5 ring-2 ring-[var(--md-sys-color-surface-container)]"
          />
          <MdBadge
            v-else-if="globalFailedTaskCount > 0"
            dot
            tone="error"
            class="pointer-events-none absolute top-1.5 right-1.5 ring-2 ring-[var(--md-sys-color-surface-container)]"
          />
        </div>

        <button
          type="button"
          class="md-icon-btn md-state"
          :class="
            themeStore.themeMode === 'system'
              ? 'bg-[var(--md-sys-color-surface-container-high)] text-[var(--md-sys-color-on-surface)]'
              : ''
          "
          :title="themeModeTooltip"
          @click="cycleThemeMode"
        >
          <LuMonitor v-if="themeStore.themeMode === 'system'" class="h-[18px] w-[18px]" />
          <LuSun v-else-if="themeStore.themeMode === 'light'" class="h-[18px] w-[18px]" />
          <LuMoon v-else class="h-[18px] w-[18px]" />
        </button>

        <div ref="accountMenuRef" class="relative ml-1">
          <button
            type="button"
            class="md-state flex h-9 w-9 items-center justify-center rounded-full bg-[var(--md-sys-color-primary-container)] text-xs font-bold text-[var(--md-sys-color-on-primary-container)]"
            @click="mobileMenuOpen = false; uploadMenuOpen = false; accountMenuOpen = !accountMenuOpen"
          >
            {{ userInitials }}
          </button>

          <Transition name="md-dialog">
            <div
              v-if="accountMenuOpen"
              class="absolute top-12 right-0 z-[var(--z-popover)] w-56 rounded-2xl bg-[var(--md-sys-color-surface-container-high)] p-2 shadow-[var(--md-sys-elevation-3)]"
            >
              <div class="px-3 py-2">
                <p class="truncate text-xs font-semibold text-[var(--md-sys-color-on-surface)]">
                  {{ authStore.user?.display_name || authStore.user?.username }}
                </p>
                <p class="truncate text-[11px] text-[var(--md-sys-color-on-surface-variant)]">
                  @{{ authStore.user?.username }} ·
                  {{ authStore.isAdmin ? t('auth.roleAdmin') : t('auth.roleMember') }}
                </p>
              </div>

              <div class="my-1 h-px bg-[var(--md-sys-color-outline-variant)]/50" />

              <RouterLink
                to="/settings"
                class="md-state flex items-center gap-2.5 rounded-full px-3 py-2 text-xs font-medium text-[var(--md-sys-color-on-surface)]"
                @click="accountMenuOpen = false; mobileMenuOpen = false"
              >
                <LuSettings class="h-4 w-4 text-[var(--md-sys-color-on-surface-variant)]" />
                <span>{{ t('nav.settings') }}</span>
              </RouterLink>

              <button
                v-if="!appClientStore.isStandaloneApp"
                type="button"
                class="md-state flex w-full items-center gap-2.5 rounded-full px-3 py-2 text-left text-xs font-medium text-[var(--md-sys-color-error)]"
                @click="handleSignOut"
              >
                <LuLogOut class="h-4 w-4" />
                <span>{{ t('auth.signOut') }}</span>
              </button>
            </div>
          </Transition>
        </div>
      </div>
      </div>
    </header>

    <!-- Middle Body: Collapsible Domain Sidebar + Flush Rounded Main Canvas -->
    <div class="relative flex min-h-0 flex-1 overflow-hidden">
      <WorkspaceSidebar
        :mobile-menu-open="mobileMenuOpen"
        :collapsed="collapsed"
        :is-compact="isCompact"
        :show-music-bar="showMusicBar"
        :domain-tabs="domainTabs"
        @close-mobile="mobileMenuOpen = false"
        @switch-domain="switchDomain"
        @open-customizer="layoutCustomizerOpen = true"
      />

      <!-- Main Route Viewport (Outer rounded clip avoids scroll-layer corner artifacts) -->
      <div
        class="min-w-0 flex-1 overflow-hidden bg-[var(--md-sys-color-surface)] md:mr-3 md:mb-3 md:rounded-[24px]"
      >
        <main
          ref="mainScrollRef"
          class="h-full overflow-y-auto"
          :class="!showMobileBottomNav && !showMusicBar ? 'safe-pb md:pb-0' : ''"
          @scroll.passive="onMainScroll"
        >
          <RouterView v-slot="{ Component }">
            <KeepAlive :include="['BookHomeView', 'ComicHomeView', 'MusicHomeView', 'VideoHomeView']">
              <component :is="Component" />
            </KeepAlive>
          </RouterView>
        </main>
      </div>
    </div>

    <!-- Reserve Space for Persistent Global PlayerBar Rendered in App.vue -->
    <div
      v-if="showMusicBar"
      class="shrink-0"
      :class="
        showMobileBottomNav
          ? 'h-[var(--player-bar-h)] md:h-[calc(var(--player-bar-h)+var(--safe-area-bottom))]'
          : 'h-[calc(var(--player-bar-h)+var(--safe-area-bottom))]'
      "
      aria-hidden="true"
    />

    <!-- Mobile MD3 Navigation Bar (< md): Active Domain Primary Navigation -->
    <nav
      v-if="showMobileBottomNav"
      data-mobile-bottom-nav
      :aria-label="t(`app.domains.${domainStore.activeDomain}`)"
      class="safe-pb safe-px relative z-30 shrink-0 border-t border-[var(--md-sys-color-outline-variant)]/50 bg-[var(--md-sys-color-surface-container)] select-none md:hidden"
    >
      <div
        class="grid h-16 items-center"
        :style="{ gridTemplateColumns: `repeat(${primaryNavItems.length}, minmax(0, 1fr))` }"
      >
        <button
          v-for="item in primaryNavItems"
          :key="`bottom-${domainStore.activeDomain}-${item.key}`"
          type="button"
          :aria-current="item.active ? 'page' : undefined"
          class="group flex h-full flex-col items-center justify-center gap-1 outline-none"
          @click="item.onClick()"
        >
          <span
            class="md-state flex h-8 w-16 items-center justify-center rounded-full transition-colors duration-200 group-focus-visible:outline-2 group-focus-visible:outline-offset-2 group-focus-visible:outline-[var(--md-sys-color-primary)]"
            :class="
              item.active
                ? 'bg-[var(--md-sys-color-secondary-container)] text-[var(--md-sys-color-on-secondary-container)]'
                : 'text-[var(--md-sys-color-on-surface-variant)] group-hover:text-[var(--md-sys-color-on-surface)]'
            "
          >
            <component :is="item.icon" class="h-5 w-5 shrink-0" />
          </span>
          <span
            class="md-label-medium max-w-full truncate px-1"
            :class="
              item.active
                ? 'font-semibold text-[var(--md-sys-color-on-surface)]'
                : 'text-[var(--md-sys-color-on-surface-variant)]'
            "
          >
            {{ item.label }}
          </span>
        </button>
      </div>
    </nav>

    <!-- Unified Global Background Tasks Drawer -->
    <BackgroundTasksDrawer
      :open="tasksDrawerOpen"
      @close="tasksDrawerOpen = false"
    />

    <!-- Sidebar Layout Customizer Modal -->
    <LayoutCustomizerModal
      :open="layoutCustomizerOpen"
      :domain="domainStore.activeDomain"
      initial-tab="sidebar"
      @close="layoutCustomizerOpen = false"
    />

    <!-- Cross-Domain Quick Search / Command Palette (⌘K) -->
    <GlobalSearchModal
      :open="commandPaletteOpen"
      @close="commandPaletteOpen = false"
    />
  </div>
</template>

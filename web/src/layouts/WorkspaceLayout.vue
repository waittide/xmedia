<script setup lang="ts">
import { computed, onBeforeUnmount, onMounted, ref, watch, type Component } from 'vue'
import { useRoute, useRouter } from 'vue-router'
import { useI18n } from 'vue-i18n'
import {
  LuBookOpen,
  LuClapperboard,
  LuDisc3,
  LuLayers,
  LuLogOut,
  LuMonitor,
  LuMoon,
  LuPanelLeftClose,
  LuPanelLeftOpen,
  LuRefreshCw,
  LuSearch,
  LuSettings,
  LuSun,
  LuUpload,
  LuX,
} from 'vue-icons-plus/lu'
import { useEscapeStack } from '@/composables/useEscapeStack'
import { useAuthStore } from '@/stores/auth'
import { useDomainStore } from '@/stores/domain'
import { useEventsStore } from '@/stores/events'
import { useThemeStore } from '@/stores/theme'
import { getAcceptedUploadExtensions, useUploadStore } from '@/stores/uploads'
import type { MediaKind } from '@/types/api'

import { useCatalogStore } from '@/domains/book/store'
import { useSidebarStore as useComicSidebarStore } from '@/domains/comic/store'
import { usePlayerStore } from '@/domains/music/player'
import { useMusicStore } from '@/domains/music/store'
import { useVideoStore } from '@/domains/video/store'

import GlobalSearchModal from '@/components/GlobalSearchModal.vue'
import LayoutCustomizerModal from '@/components/LayoutCustomizerModal.vue'
import MdSegmentedButton from '@/components/ui/MdSegmentedButton.vue'
import UploadShelf from '@/components/UploadShelf.vue'
import WorkspaceSidebar from '@/layouts/WorkspaceSidebar.vue'

const { t } = useI18n()

const route = useRoute()
const router = useRouter()
const authStore = useAuthStore()
const domainStore = useDomainStore()
const eventsStore = useEventsStore()
const themeStore = useThemeStore()
const uploadStore = useUploadStore()
const escapeStack = useEscapeStack()

const bookStore = useCatalogStore()
const comicSidebarStore = useComicSidebarStore()
const musicStore = useMusicStore()
const playerStore = usePlayerStore()
const videoStore = useVideoStore()

const collapsed = ref(localStorage.getItem('xmedia.sidebar_collapsed') === '1')
const mobileMenuOpen = ref(false)
const accountMenuOpen = ref(false)
const layoutCustomizerOpen = ref(false)
const commandPaletteOpen = ref(false)
const dragDepth = ref(0)

const filePickerRef = ref<HTMLInputElement | null>(null)
const mainScrollRef = ref<HTMLElement | null>(null)
const searchInputRef = ref<HTMLInputElement | null>(null)
const accountMenuRef = ref<HTMLElement | null>(null)

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
escapeStack.register(
  () => playerStore.queueOpen,
  () => {
    playerStore.queueOpen = false
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

const isCompact = computed(() => collapsed.value && !mobileMenuOpen.value)

async function switchDomain(kind: MediaKind): Promise<void> {
  const alreadyInDomain = domainStore.activeDomain === kind && route.meta.domain === kind
  const target = alreadyInDomain ? `/${kind}` : domainStore.getLastRoute(kind)
  domainStore.setActiveDomain(kind)
  mobileMenuOpen.value = false
  await router.push(target)
}

function handleSidebarButtonClick(): void {
  if (window.innerWidth < 768) {
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
    await bookStore.fetchBooks()
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
      void bookStore.fetchBooks()
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
function triggerUploadPicker(): void {
  filePickerRef.value?.click()
}

function handlePickedFiles(ev: Event): void {
  const input = ev.target as HTMLInputElement
  if (input.files && input.files.length > 0) {
    uploadStore.enqueueFiles(input.files)
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

function onDropFiles(ev: DragEvent): void {
  dragDepth.value = 0
  if (!authStore.isAdmin) return
  if (ev.dataTransfer?.files && ev.dataTransfer.files.length > 0) {
    uploadStore.enqueueFiles(ev.dataTransfer.files)
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

const topScanLabel = computed(() => {
  const active = eventsStore.firstActiveScan
  if (!active) return ''
  if (active.total && active.total > 0) {
    const pct = Math.min(100, Math.round((active.scanned / active.total) * 100))
    return t('admin.scanningWithTotal', {
      scanned: active.scanned,
      total: active.total,
      pct,
    })
  }
  return t('admin.scanning', { scanned: active.scanned })
})

const userInitials = computed(() => {
  const label = authStore.user?.display_name || authStore.user?.username || 'U'
  return label.slice(0, 1).toUpperCase()
})

async function handleSignOut(): Promise<void> {
  accountMenuOpen.value = false
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
}

function handleGlobalKeydown(ev: KeyboardEvent): void {
  if ((ev.metaKey || ev.ctrlKey) && ev.key.toLowerCase() === 'k') {
    ev.preventDefault()
    commandPaletteOpen.value = !commandPaletteOpen.value
  } else if ((ev.metaKey || ev.ctrlKey) && ev.key.toLowerCase() === 'b') {
    ev.preventDefault()
    handleSidebarButtonClick()
  }
}

onMounted(() => {
  document.addEventListener('mousedown', handleOutsideClick)
  window.addEventListener('keydown', handleGlobalKeydown)
})

onBeforeUnmount(() => {
  document.removeEventListener('mousedown', handleOutsideClick)
  window.removeEventListener('keydown', handleGlobalKeydown)
  if (sidebarLoadTimer) clearTimeout(sidebarLoadTimer)
})
</script>

<template>
  <div
    class="relative flex h-screen w-screen flex-col overflow-hidden bg-[var(--md-sys-color-surface-container)] text-[var(--md-sys-color-on-surface)]"
    @dragenter.prevent="onDragEnter"
    @dragover.prevent
    @dragleave.prevent="onDragLeave"
    @drop.prevent="onDropFiles"
  >
    <!-- Hidden Global Multi-Domain File Picker -->
    <input
      ref="filePickerRef"
      type="file"
      multiple
      :accept="acceptedExtensions"
      class="hidden"
      @change="handlePickedFiles"
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

    <!-- Full-Width MD3 Top App Bar -->
    <header
      class="flex h-16 shrink-0 items-center justify-between gap-2.5 bg-[var(--md-sys-color-surface-container)] px-3 select-none sm:gap-3 sm:px-4"
    >
      <!-- Left: Rail/Drawer Toggle + Soft Tonal Brand + MD3 Domain Segmented Switcher -->
      <div class="flex shrink-0 items-center gap-2">
        <button
          type="button"
          class="md-icon-btn md-state"
          :title="collapsed ? t('nav.expandSidebar') : t('nav.collapseSidebar')"
          @click="handleSidebarButtonClick"
        >
          <LuPanelLeftOpen v-if="isCompact" class="h-5 w-5" />
          <LuPanelLeftClose v-else class="h-5 w-5" />
        </button>

        <RouterLink
          :to="'/' + domainStore.activeDomain"
          class="md-state flex items-center gap-2.5 rounded-full py-1 pr-2.5 pl-1 outline-none"
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

      <!-- Center: Mobile Global Search Trigger Pill (< sm) -->
      <button
        type="button"
        class="md-state flex h-10 min-w-0 flex-1 items-center gap-2 rounded-full bg-[var(--md-sys-color-surface-container-high)] px-3.5 text-left text-xs text-[var(--md-sys-color-on-surface-variant)] sm:hidden"
        :aria-label="t('common.search')"
        @click="commandPaletteOpen = true"
      >
        <LuSearch class="h-4 w-4 shrink-0" />
        <span class="truncate">{{ activeSearchValue || t('common.search') + '…' }}</span>
      </button>

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

      <!-- Right: Scan Chip + Circular MD3 Icon Buttons + Avatar Menu -->
      <div class="flex shrink-0 items-center gap-1">
        <button
          v-if="eventsStore.anyScanning"
          type="button"
          class="md-btn md-btn-sm md-btn-tonal md-state mr-1 hidden xl:inline-flex"
          @click="uploadStore.collapsed = false"
        >
          <LuRefreshCw class="h-3.5 w-3.5 animate-spin" />
          <span>{{ topScanLabel }}</span>
        </button>

        <button
          v-if="authStore.isAdmin"
          type="button"
          class="md-icon-btn md-state relative"
          :title="t('common.uploadMedia')"
          @click="triggerUploadPicker"
        >
          <LuUpload class="h-[18px] w-[18px]" />
          <span
            v-if="uploadStore.activeCount > 0"
            class="absolute top-2 right-2 h-2 w-2 rounded-full bg-[var(--md-sys-color-primary)]"
          />
        </button>

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
            @click="accountMenuOpen = !accountMenuOpen"
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

      <!-- Main Route Viewport (MD3 Tonal Surface Container without harsh borders) -->
      <main
        ref="mainScrollRef"
        class="min-w-0 flex-1 overflow-y-auto bg-[var(--md-sys-color-surface)] md:mr-3 md:mb-3 md:rounded-[24px]"
      >
        <RouterView v-slot="{ Component }">
          <KeepAlive :include="['BookHomeView', 'ComicHomeView', 'MusicHomeView', 'VideoHomeView']">
            <component :is="Component" />
          </KeepAlive>
        </RouterView>
      </main>
    </div>

    <!-- Reserve Space for Persistent Global PlayerBar Rendered in App.vue -->
    <div v-if="showMusicBar" class="h-20 shrink-0" aria-hidden="true" />

    <!-- Non-Blocking Multi-Domain Scan & Upload Task Shelf -->
    <UploadShelf :elevated-bottom="showMusicBar" />

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

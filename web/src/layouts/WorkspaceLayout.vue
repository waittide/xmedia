<script setup lang="ts">
import { computed, onBeforeUnmount, onMounted, ref, watch, type Component } from 'vue'
import { useRoute, useRouter } from 'vue-router'
import { useI18n } from 'vue-i18n'
import {
  LuCheck,
  LuHistory,
  LuHome,
  LuLibrary,
  LuListChecks,
  LuLogOut,
  LuMonitor,
  LuMoon,
  LuSearch,
  LuSettings,
  LuSun,
  LuUpload,
} from 'vue-icons-plus/lu'
import BackgroundTasksDrawer from '@/components/BackgroundTasksDrawer.vue'
import GlobalSearchModal from '@/components/GlobalSearchModal.vue'
import { MdBadge, MdDomainIcons, MdIconButton, MdMenu, MdMenuItem } from '@/components/ui'
import { useMediaFocus } from '@/composables/useMediaFocus'
import { useShowMusicBar } from '@/composables/useShowMusicBar'
import LyricsOverlay from '@/domains/music/components/LyricsOverlay.vue'
import PlayerBar from '@/domains/music/components/PlayerBar.vue'
import QueueDrawer from '@/domains/music/components/QueueDrawer.vue'
import { usePlayerStore } from '@/domains/music/player'
import { ensureDomainLocale } from '@/router'
import { useAppClientStore } from '@/stores/appServers'
import { useAuthStore } from '@/stores/auth'
import { useBackgroundDownloadTasks } from '@/stores/backgroundTasks'
import { useDomainStore } from '@/stores/domain'
import { useEventsStore } from '@/stores/events'
import { useThemeStore } from '@/stores/theme'
import { useUploadStore } from '@/stores/uploads'
import type { ThemeMode } from '@/theme/md3'
import type { MediaKind } from '@/types/api'
import { extractFilesFromDataTransfer } from '@/utils/fileDrop'
import NavigationBar, { type MobileDomainSubTab } from './NavigationBar.vue'
import NavigationRail, { type DomainNavTab } from './NavigationRail.vue'

const { t } = useI18n()
const route = useRoute()
const router = useRouter()
const appClientStore = useAppClientStore()
const authStore = useAuthStore()
const domainStore = useDomainStore()
const eventsStore = useEventsStore()
const backgroundDownloads = useBackgroundDownloadTasks()
const playerStore = usePlayerStore()
const mediaFocus = useMediaFocus()
const themeStore = useThemeStore()
const uploadStore = useUploadStore()

const showMusicBar = useShowMusicBar()
const tasksDrawerOpen = ref(false)
const searchOpen = ref(false)
const dragDepth = ref(0)
const mainScrollRef = ref<HTMLElement | null>(null)
const isContentScrolled = ref(false)

const compactViewportMedia =
  typeof window !== 'undefined' ? window.matchMedia('(max-width: 599.98px)') : null
const isCompactViewport = ref(compactViewportMedia ? compactViewportMedia.matches : false)
function onCompactViewportChange(ev: MediaQueryListEvent): void {
  isCompactViewport.value = ev.matches
}

watch(
  () => [authStore.isAuthenticated, authStore.user?.id] as const,
  ([isAuth]) => {
    if (isAuth) {
      void backgroundDownloads.refreshAll()
    }
  },
  { immediate: true },
)

watch(
  () => [route.path, domainStore.activeDomain] as const,
  () => {
    if (mainScrollRef.value) {
      mainScrollRef.value.scrollTop = 0
      isContentScrolled.value = false
    }
  },
)

function onMainScroll(ev: Event): void {
  const el = ev.target as HTMLElement | null
  if (!el) return
  isContentScrolled.value = Math.max(0, el.scrollTop) > 4
}

const isSettingsRoute = computed(() => route.path.startsWith('/settings'))

const domainTabs = computed<DomainNavTab[]>(() => {
  const all: DomainNavTab[] = [
    { kind: 'book', label: t('app.domains.book') },
    { kind: 'comic', label: t('app.domains.comic') },
    { kind: 'music', label: t('app.domains.music') },
    { kind: 'video', label: t('app.domains.video') },
  ]
  return all.filter((item) => authStore.hasModule(item.kind))
})

const domainSubTabs = computed<MobileDomainSubTab[]>(() => {
  const d = domainStore.activeDomain
  const inDomain = !isSettingsRoute.value

  const isHome = inDomain && route.path === `/${d}`
  const isHistory = inDomain && route.path === `/${d}/history`
  const isDomainSettings = inDomain && route.path.startsWith(`/${d}/settings`)
  const isLibrary =
    inDomain &&
    !isHome &&
    !isHistory &&
    !isDomainSettings &&
    (route.path.startsWith(`/${d}/library`) || route.path.startsWith(`/${d}/item/`))

  return [
    {
      key: 'home',
      label: t('nav.home'),
      icon: LuHome,
      active: isHome,
      to: `/${d}`,
    },
    {
      key: 'library',
      label: t('nav.library'),
      icon: LuLibrary,
      active: isLibrary,
      to: `/${d}/library`,
    },
    {
      key: 'history',
      label: t('nav.history'),
      icon: LuHistory,
      active: isHistory,
      to: `/${d}/history`,
    },
    {
      key: 'settings',
      label: t('nav.domainSettings'),
      icon: LuSettings,
      active: isDomainSettings,
      to: `/${d}/settings`,
    },
  ]
})

const topBarTitle = computed(() =>
  isSettingsRoute.value ? t('settings.title') : t(`app.domains.${domainStore.activeDomain}`),
)

const searchPlaceholder = computed(() => {
  const d = domainStore.activeDomain
  if (d === 'book') return t('common.searchBook')
  if (d === 'comic') return t('common.searchComic')
  if (d === 'music') return t('common.searchMusic')
  return t('common.searchVideo')
})

async function switchDomain(kind: MediaKind): Promise<void> {
  const alreadyInDomain =
    !isSettingsRoute.value && domainStore.activeDomain === kind && route.meta.domain === kind
  const target = alreadyInDomain ? `/${kind}` : domainStore.getLastRoute(kind)
  await ensureDomainLocale(kind)
  domainStore.setActiveDomain(kind)
  themeStore.rememberActiveDomain(kind)
  await router.push(target)
}

function selectDomainFromUserMenu(kind: MediaKind, closeMenu: () => void): void {
  closeMenu()
  void switchDomain(kind)
}

function navigateTab(tab: MobileDomainSubTab): void {
  void router.push(tab.to)
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

const globalActiveTaskCount = computed(() => {
  const activeScans = eventsStore.scanTaskList.filter((sc) => sc.status === 'running').length
  const activeUploads = uploadStore.tasks.filter(
    (task) => task.status === 'queued' || task.status === 'uploading',
  ).length
  return activeScans + activeUploads + backgroundDownloads.activeCount.value
})

const globalFailedTaskCount = computed(() => {
  const failedScans = eventsStore.scanTaskList.filter((sc) => sc.status === 'failed').length
  return failedScans + uploadStore.failedCount + backgroundDownloads.failedCount.value
})

function openTasksDrawer(): void {
  tasksDrawerOpen.value = true
  if (authStore.isAuthenticated) {
    void backgroundDownloads.refreshAll()
  }
}

const themeModeOptions = computed<Array<{ value: ThemeMode; label: string; icon: Component }>>(
  () => [
    { value: 'system', label: t('theme.system'), icon: LuMonitor },
    { value: 'light', label: t('theme.light'), icon: LuSun },
    { value: 'dark', label: t('theme.dark'), icon: LuMoon },
  ],
)

const userInitials = computed(() => {
  const label = authStore.user?.display_name || authStore.user?.username || 'U'
  return label.slice(0, 1).toUpperCase()
})

const accountButtonLabel = computed(
  () => authStore.user?.display_name || authStore.user?.username || t('settings.navAccount'),
)

async function handleSignOut(): Promise<void> {
  tasksDrawerOpen.value = false
  mediaFocus.pauseAll()
  playerStore.pause()
  eventsStore.disconnect()
  await authStore.logout()
  await router.replace('/login')
}

function handleGlobalKeydown(ev: KeyboardEvent): void {
  if (ev.isComposing || playerStore.lyricsOpen) return
  if ((ev.metaKey || ev.ctrlKey) && ev.key.toLowerCase() === 'k') {
    if (document.querySelector('[aria-modal="true"]') && !searchOpen.value) return
    ev.preventDefault()
    searchOpen.value = !searchOpen.value
  }
}

onMounted(() => {
  window.addEventListener('keydown', handleGlobalKeydown)
  compactViewportMedia?.addEventListener('change', onCompactViewportChange)
})

onBeforeUnmount(() => {
  window.removeEventListener('keydown', handleGlobalKeydown)
  compactViewportMedia?.removeEventListener('change', onCompactViewportChange)
})
</script>

<template>
  <div
    data-workspace-shell
    :data-has-player="showMusicBar ? '1' : '0'"
    class="relative grid h-screen w-screen grid-cols-1 grid-rows-[auto_minmax(0,1fr)_auto_auto] overflow-hidden bg-surface text-on-surface supports-[height:100dvh]:h-dvh sm:grid-cols-[auto_minmax(0,1fr)] sm:grid-rows-[auto_minmax(0,1fr)_auto]"
    @dragenter.prevent="onDragEnter"
    @dragover.prevent
    @dragleave.prevent="onDragLeave"
    @drop.prevent="onDropFiles"
  >
    <!-- Full-Window Drag & Drop Upload Overlay -->
    <div
      v-if="dragDepth > 0"
      class="pointer-events-none fixed inset-3 z-[var(--z-modal)] flex flex-col items-center justify-center rounded-corner-xl border-2 border-dashed border-primary bg-surface-container-high/95"
    >
      <div
        class="mb-3 flex h-14 w-14 items-center justify-center rounded-corner-lg bg-primary-container text-on-primary-container"
      >
        <LuUpload class="h-7 w-7" aria-hidden="true" />
      </div>
      <p class="text-title-medium text-on-surface">
        {{ t('transfers.dropOverlayTitle') }}
      </p>
      <p class="text-body-small mt-1 px-4 text-center text-on-surface-variant">
        {{ t('transfers.dropOverlayHint') }}
      </p>
    </div>

    <!-- Medium / Expanded (>=600px): MD3 Navigation Rail (Col 1, spans all rows) -->
    <NavigationRail
      class="sm:col-start-1 sm:row-span-3"
      :domain-tabs="domainTabs"
      :active-domain="domainStore.activeDomain"
      :is-settings-route="isSettingsRoute"
      @switch-domain="switchDomain"
    />

    <!-- Top Region: Minimal MD3 Top App Bar + Desktop Domain Sub-Navigation -->
    <div
      class="safe-pt safe-px col-start-1 row-start-1 z-20 shrink-0 transition-colors duration-150 ease-standard select-none sm:col-start-2 sm:pl-0"
      :class="isContentScrolled ? 'bg-surface-container' : 'bg-surface'"
    >
      <header class="flex h-14 items-center justify-between gap-3 px-4 sm:h-16 md:px-6">
        <div class="flex min-w-0 items-center gap-2.5">
          <span
            v-if="!isSettingsRoute"
            class="flex h-8 w-8 shrink-0 items-center justify-center rounded-corner-sm bg-secondary-container text-on-secondary-container sm:hidden"
            aria-hidden="true"
          >
            <MdDomainIcons :kind="domainStore.activeDomain" active class="h-4 w-4" />
          </span>
          <h1 class="text-title-large shrink-0 truncate text-on-surface">
            {{ topBarTitle }}
          </h1>
        </div>

        <!-- Desktop Single Search Entry (opens unified SearchView) -->
        <button
          type="button"
          class="md-state hidden h-11 max-w-md flex-1 cursor-pointer items-center gap-3 rounded-corner-full bg-surface-container-high px-4 text-left text-on-surface-variant hover:text-on-surface sm:flex"
          :aria-label="t('common.search')"
          @click="searchOpen = true"
        >
          <LuSearch class="h-4 w-4 shrink-0" aria-hidden="true" />
          <span class="text-body-medium min-w-0 flex-1 truncate">
            {{ searchPlaceholder }}
          </span>
          <span
            class="text-label-small shrink-0 rounded-corner-full bg-surface-container px-2 py-0.5 font-mono text-on-surface-variant"
          >
            ⌘K
          </span>
        </button>

        <!-- Right Actions (<=2): Mobile Search Icon + Account Panel -->
        <div class="flex shrink-0 items-center gap-1">
          <MdIconButton
            class="sm:hidden"
            :icon="LuSearch"
            :aria-label="t('common.search')"
            @click="searchOpen = true"
          />

          <MdMenu
            placement="bottom-end"
            :min-width="284"
            :max-width="320"
            :aria-label="accountButtonLabel"
          >
            <template #trigger="{ toggle, triggerAttrs }">
              <button
                type="button"
                v-bind="triggerAttrs"
                data-account-menu
                :aria-label="accountButtonLabel"
                class="md-state md-touch-target text-label-large relative inline-flex h-10 w-10 cursor-pointer items-center justify-center rounded-corner-full bg-primary-container text-on-primary-container"
                @click="toggle"
              >
                {{ userInitials }}
                <MdBadge
                  v-if="globalActiveTaskCount > 0"
                  :value="globalActiveTaskCount"
                  :max="99"
                  tone="primary"
                  class="pointer-events-none absolute -top-0.5 -right-0.5"
                />
                <MdBadge
                  v-else-if="globalFailedTaskCount > 0"
                  dot
                  tone="error"
                  class="pointer-events-none absolute top-0.5 right-0.5"
                />
              </button>
            </template>

            <template #default="{ close }">
              <div class="px-2.5 pt-1.5 pb-2">
                <p class="text-title-small truncate text-on-surface">
                  {{ authStore.user?.display_name || authStore.user?.username }}
                </p>
                <p class="text-body-small truncate text-on-surface-variant">
                  @{{ authStore.user?.username }} ·
                  {{ authStore.isAdmin ? t('auth.roleAdmin') : t('auth.roleMember') }}
                </p>
              </div>

              <!-- Active Media Domain Switcher (persisted across sessions) -->
              <div v-if="domainTabs.length > 1" class="px-2 pb-2">
                <p class="text-label-small mb-1.5 px-1 text-on-surface-variant">
                  {{ t('nav.currentDomain') }}
                </p>
                <div
                  role="radiogroup"
                  :aria-label="t('nav.currentDomain')"
                  class="grid grid-cols-2 gap-1.5"
                >
                  <button
                    v-for="tab in domainTabs"
                    :key="`user-domain-${tab.kind}`"
                    type="button"
                    role="radio"
                    :data-user-domain-switch="tab.kind"
                    :aria-checked="domainStore.activeDomain === tab.kind"
                    class="md-state text-label-large flex min-h-9 cursor-pointer items-center justify-between gap-2 rounded-corner-md px-2.5 py-1.5 transition-colors duration-150 ease-standard"
                    :class="
                      domainStore.activeDomain === tab.kind
                        ? 'bg-secondary-container text-on-secondary-container'
                        : 'bg-surface-container-high text-on-surface-variant hover:text-on-surface'
                    "
                    @click="selectDomainFromUserMenu(tab.kind, close)"
                  >
                    <span class="flex min-w-0 items-center gap-2">
                      <MdDomainIcons
                        :kind="tab.kind"
                        :active="domainStore.activeDomain === tab.kind"
                        class="h-4 w-4 shrink-0"
                      />
                      <span class="truncate">{{ tab.label }}</span>
                    </span>
                    <LuCheck
                      v-if="domainStore.activeDomain === tab.kind"
                      class="h-3.5 w-3.5 shrink-0"
                      aria-hidden="true"
                    />
                  </button>
                </div>
              </div>

              <!-- Compact 3-state Theme Mode Switcher -->
              <div class="px-2 pb-2">
                <div
                  role="radiogroup"
                  :aria-label="t('theme.modeTitle')"
                  class="grid grid-cols-[minmax(0,1.25fr)_minmax(0,1fr)_minmax(0,1fr)] gap-1 rounded-corner-full bg-surface-container-high p-1"
                >
                  <button
                    v-for="opt in themeModeOptions"
                    :key="opt.value"
                    type="button"
                    role="radio"
                    :aria-checked="themeStore.themeMode === opt.value"
                    :title="opt.label"
                    class="md-state text-label-small flex cursor-pointer items-center justify-center gap-1 rounded-corner-full px-1.5 py-1.5 whitespace-nowrap"
                    :class="
                      themeStore.themeMode === opt.value
                        ? 'bg-secondary-container text-on-secondary-container'
                        : 'text-on-surface-variant hover:text-on-surface'
                    "
                    @click="themeStore.setThemeMode(opt.value)"
                  >
                    <component :is="opt.icon" class="h-3.5 w-3.5 shrink-0" aria-hidden="true" />
                    <span>{{ opt.label }}</span>
                  </button>
                </div>
              </div>

              <hr class="mx-1.5 my-1.5 border-outline-variant" />

              <MdMenuItem
                :icon="LuListChecks"
                :label="t('transfers.openTasks')"
                :trailing-text="globalActiveTaskCount > 0 ? String(globalActiveTaskCount) : ''"
                @select="openTasksDrawer"
              />

              <MdMenuItem
                :icon="LuSettings"
                :label="t('nav.settings')"
                to="/settings"
              />

              <MdMenuItem
                v-if="!appClientStore.isStandaloneApp"
                :icon="LuLogOut"
                :label="t('auth.signOut')"
                destructive
                @select="handleSignOut"
              />
            </template>
          </MdMenu>
        </div>
      </header>

      <!-- Desktop (>=600px) Domain Sub-Navigation (mobile uses bottom NavigationBar) -->
      <div
        v-if="!isSettingsRoute && domainSubTabs.length > 0"
        class="hidden sm:block"
      >
        <nav
          :aria-label="t('nav.navigation')"
          class="no-scrollbar flex items-end gap-1 overflow-x-auto border-b border-outline-variant px-4 md:px-6"
        >
          <button
            v-for="tab in domainSubTabs"
            :key="tab.key"
            type="button"
            :data-nav="!isCompactViewport ? tab.key : undefined"
            :aria-label="tab.label"
            :title="!themeStore.showNavLabels ? tab.label : undefined"
            :aria-current="tab.active ? 'page' : undefined"
            class="md-state text-title-small relative inline-flex shrink-0 cursor-pointer items-center justify-center whitespace-nowrap transition-colors duration-150 ease-standard"
            :class="[
              themeStore.showNavLabels ? 'h-16 px-5' : 'h-12 px-4',
              tab.active
                ? 'text-primary'
                : 'text-on-surface-variant hover:text-on-surface',
            ]"
            @click="navigateTab(tab)"
          >
            <span class="relative inline-flex h-full flex-col items-center justify-center gap-1">
              <component :is="tab.icon" class="h-5 w-5 shrink-0 sm:h-6 sm:w-6" aria-hidden="true" />
              <span v-if="themeStore.showNavLabels">{{ tab.label }}</span>
              <span
                v-if="tab.active"
                aria-hidden="true"
                class="pointer-events-none absolute right-0 bottom-0 left-0 mx-auto h-[3px] min-w-6 rounded-t-[3px] bg-primary"
              />
            </span>
          </button>
        </nav>
      </div>
    </div>

    <!-- Main Scrollable Route Viewport -->
    <main
      ref="mainScrollRef"
      class="col-start-1 row-start-2 min-h-0 min-w-0 overflow-y-auto bg-surface sm:col-start-2"
      @scroll.passive="onMainScroll"
    >
      <RouterView v-slot="{ Component }">
        <KeepAlive :include="['BookHomeView', 'ComicHomeView', 'MusicHomeView', 'VideoHomeView']">
          <component :is="Component" />
        </KeepAlive>
      </RouterView>
    </main>

    <!-- Grid-Flow Global Player Bar (never fixed, never overlaps Nav Bar or main) -->
    <div
      v-if="showMusicBar"
      class="col-start-1 row-start-3 min-w-0 sm:col-start-2 sm:pb-[var(--safe-area-bottom)]"
    >
      <PlayerBar />
    </div>

    <!-- Compact (<600px): MD3 Bottom Navigation Bar (Domain Sub-Tabs) -->
    <NavigationBar
      class="col-start-1 row-start-4"
      :tabs="domainSubTabs"
      :bind-data-nav="isCompactViewport"
      @navigate="navigateTab"
    />

    <!-- Player Side Sheets & Fullscreen Lyrics Overlay -->
    <template v-if="showMusicBar">
      <QueueDrawer />
      <LyricsOverlay />
    </template>

    <!-- Global Background Tasks Sheet -->
    <BackgroundTasksDrawer
      :open="tasksDrawerOpen"
      @close="tasksDrawerOpen = false"
    />

    <!-- Unified MD3 Search View (Top Bar / Mobile Search / ⌘K) -->
    <GlobalSearchModal
      :open="searchOpen"
      @close="searchOpen = false"
    />
  </div>
</template>

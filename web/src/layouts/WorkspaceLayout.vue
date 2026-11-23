<script setup lang="ts">
import { computed, onBeforeUnmount, onMounted, ref, watch, type Component } from 'vue'
import { useRoute, useRouter, type RouteLocationRaw } from 'vue-router'
import { useI18n } from 'vue-i18n'
import {
  LuBookOpen,
  LuClapperboard,
  LuCompass,
  LuDisc3,
  LuHistory,
  LuHome,
  LuLayers,
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
import { MdBadge, MdIconButton, MdMenu, MdMenuItem } from '@/components/ui'
import { useEscapeStack } from '@/composables/useEscapeStack'
import { useShowMusicBar } from '@/composables/useShowMusicBar'
import LyricsOverlay from '@/domains/music/components/LyricsOverlay.vue'
import PlayerBar from '@/domains/music/components/PlayerBar.vue'
import QueueDrawer from '@/domains/music/components/QueueDrawer.vue'
import { usePlayerStore } from '@/domains/music/player'
import { ensureDomainLocale } from '@/router'
import { useAppClientStore } from '@/stores/appServers'
import { useAuthStore } from '@/stores/auth'
import { useDomainStore } from '@/stores/domain'
import { useEventsStore } from '@/stores/events'
import { useOnlineDownloadsStore } from '@/stores/onlineDownloads'
import { useThemeStore } from '@/stores/theme'
import { useUploadStore } from '@/stores/uploads'
import type { ThemeMode } from '@/theme/md3'
import type { MediaKind } from '@/types/api'
import { extractFilesFromDataTransfer } from '@/utils/fileDrop'
import NavigationBar from './NavigationBar.vue'
import NavigationRail, { type DomainNavTab } from './NavigationRail.vue'

interface SecondaryTab {
  key: 'home' | 'library' | 'discover' | 'history'
  label: string
  icon: Component
  active: boolean
  to: RouteLocationRaw
}

const { t, te, locale } = useI18n()
const route = useRoute()
const router = useRouter()
const appClientStore = useAppClientStore()
const authStore = useAuthStore()
const domainStore = useDomainStore()
const eventsStore = useEventsStore()
const onlineDownloadsStore = useOnlineDownloadsStore()
const playerStore = usePlayerStore()
const themeStore = useThemeStore()
const uploadStore = useUploadStore()
const escapeStack = useEscapeStack()

const showMusicBar = useShowMusicBar()
const tasksDrawerOpen = ref(false)
const searchOpen = ref(false)
const dragDepth = ref(0)
const mainScrollRef = ref<HTMLElement | null>(null)
const isContentScrolled = ref(false)

escapeStack.register(
  () => playerStore.lyricsOpen,
  () => {
    playerStore.lyricsOpen = false
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
  isContentScrolled.value = Boolean(el && el.scrollTop > 4)
}

const isZh = computed(() => String(locale.value).startsWith('zh'))

function tr(key: string, zh: string, en: string): string {
  return te(key) ? t(key) : isZh.value ? zh : en
}

const isSettingsRoute = computed(() => route.path.startsWith('/settings'))

const domainTabs = computed<DomainNavTab[]>(() => {
  const all: DomainNavTab[] = [
    { kind: 'book', label: t('app.domains.book'), icon: LuBookOpen },
    { kind: 'comic', label: t('app.domains.comic'), icon: LuLayers },
    { kind: 'music', label: t('app.domains.music'), icon: LuDisc3 },
    { kind: 'video', label: t('app.domains.video'), icon: LuClapperboard },
  ]
  return all.filter((item) => authStore.hasModule(item.kind))
})

function hasCanonicalRoute(path: string): boolean {
  const resolved = router.resolve(path)
  if (resolved.matched.length === 0) return false
  return resolved.matched[resolved.matched.length - 1]?.path !== '/:pathMatch(.*)*'
}

function legacyCatalogPath(domain: MediaKind): string {
  if (domain === 'comic') return '/comic/series'
  if (domain === 'music') return '/music/songs'
  return `/${domain}/catalog`
}

const secondaryTabs = computed<SecondaryTab[]>(() => {
  if (isSettingsRoute.value) return []
  const d = domainStore.activeDomain
  const useCanonical = hasCanonicalRoute(`/${d}/library`)
  const scope = typeof route.query.scope === 'string' ? route.query.scope : ''
  const name = String(route.name ?? '')

  const isHome = route.path === `/${d}`
  const isHistory = route.path === `/${d}/history`
  const isDiscover =
    route.path === `/${d}/discover` || (scope === 'online' && !name.includes('detail'))
  const isLibrary =
    !isHome &&
    !isHistory &&
    !isDiscover &&
    (route.path.startsWith(`/${d}/library`) ||
      name === 'book-catalog' ||
      name === 'book-detail' ||
      name === 'comic-series-list' ||
      name === 'comic-series-detail' ||
      name === 'music-songs' ||
      name === 'music-album-detail' ||
      name === 'music-artist-detail' ||
      name === 'video-catalog' ||
      name === 'video-show-detail')

  return [
    {
      key: 'home',
      label: tr('nav.home', '首页', 'Home'),
      icon: LuHome,
      active: isHome,
      to: `/${d}`,
    },
    {
      key: 'library',
      label: tr('nav.library', '资源', 'Resources'),
      icon: LuLibrary,
      active: isLibrary,
      to: useCanonical ? `/${d}/library` : legacyCatalogPath(d),
    },
    {
      key: 'discover',
      label: t('nav.discover'),
      icon: LuCompass,
      active: isDiscover,
      to: useCanonical
        ? `/${d}/discover`
        : { path: legacyCatalogPath(d), query: { scope: 'online' } },
    },
    {
      key: 'history',
      label: tr('nav.history', '历史', 'History'),
      icon: LuHistory,
      active: isHistory,
      to: `/${d}/history`,
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
  await router.push(target)
}

function navigateTab(tab: SecondaryTab): void {
  void router.push(tab.to)
}

// Drag-and-drop file/folder upload anywhere in the workspace (admins only)
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

// Background task counts for subtle avatar badge & account menu entry
const globalActiveTaskCount = computed(() => {
  const activeScans = eventsStore.scanTaskList.filter((sc) => sc.status === 'running').length
  const activeUploads = uploadStore.tasks.filter(
    (task) => task.status === 'queued' || task.status === 'uploading',
  ).length
  return activeScans + activeUploads + onlineDownloadsStore.activeJobs.length
})

const globalFailedTaskCount = computed(() => {
  const failedScans = eventsStore.scanTaskList.filter((sc) => sc.status === 'failed').length
  return failedScans + uploadStore.failedCount + onlineDownloadsStore.failedCount
})

function openTasksDrawer(): void {
  tasksDrawerOpen.value = true
  if (authStore.isAuthenticated && authStore.hasModule('book')) {
    void onlineDownloadsStore.refresh()
  }
}

const themeModeOptions = computed<Array<{ value: ThemeMode; label: string; icon: Component }>>(() => [
  { value: 'system', label: t('theme.system'), icon: LuMonitor },
  { value: 'light', label: t('theme.light'), icon: LuSun },
  { value: 'dark', label: t('theme.dark'), icon: LuMoon },
])

const userInitials = computed(() => {
  const label = authStore.user?.display_name || authStore.user?.username || 'U'
  return label.slice(0, 1).toUpperCase()
})

const accountButtonLabel = computed(
  () => authStore.user?.display_name || authStore.user?.username || t('settings.navAccount'),
)

async function handleSignOut(): Promise<void> {
  tasksDrawerOpen.value = false
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
})

onBeforeUnmount(() => {
  window.removeEventListener('keydown', handleGlobalKeydown)
})
</script>

<template>
  <div
    data-workspace-shell
    :data-has-player="showMusicBar ? '1' : '0'"
    class="relative grid h-screen w-screen grid-cols-1 grid-rows-[auto_minmax(0,1fr)_auto_auto] overflow-hidden bg-surface text-on-surface supports-[height:100dvh]:h-dvh md:grid-cols-[80px_minmax(0,1fr)] md:grid-rows-[auto_minmax(0,1fr)_auto]"
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

    <!-- Expanded (>=840px): MD3 Navigation Rail (Col 1, spans all rows) -->
    <NavigationRail
      class="md:col-start-1 md:row-span-3"
      :domain-tabs="domainTabs"
      :active-domain="domainStore.activeDomain"
      :is-settings-route="isSettingsRoute"
      @switch-domain="switchDomain"
    />

    <!-- Top Region: Minimal MD3 Top App Bar + Domain Primary Tabs -->
    <div
      class="safe-pt safe-px col-start-1 row-start-1 z-20 shrink-0 transition-shadow duration-150 ease-standard select-none md:col-start-2"
      :class="
        isContentScrolled ? 'bg-surface-container shadow-elevation-1' : 'bg-surface'
      "
    >
      <header class="flex h-16 items-center justify-between gap-3 px-4 md:px-6">
        <h1 class="text-title-large shrink-0 truncate text-on-surface">
          {{ topBarTitle }}
        </h1>

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

        <!-- Right Actions (<=2): Mobile Search Icon + Account Menu -->
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

            <div class="px-2.5 pt-1.5 pb-2">
              <p class="text-title-small truncate text-on-surface">
                {{ authStore.user?.display_name || authStore.user?.username }}
              </p>
              <p class="text-body-small truncate text-on-surface-variant">
                @{{ authStore.user?.username }} ·
                {{ authStore.isAdmin ? t('auth.roleAdmin') : t('auth.roleMember') }}
              </p>
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

            <div class="mx-1.5 my-1.5 border-t border-outline-variant/60" />

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
          </MdMenu>
        </div>
      </header>

      <!-- Domain Secondary Navigation: MD3 Primary Tabs (hidden on /settings*) -->
      <nav
        v-if="secondaryTabs.length > 0"
        :aria-label="t('nav.navigation')"
        class="no-scrollbar flex items-end gap-1 overflow-x-auto border-b border-outline-variant px-4 md:px-6"
      >
        <button
          v-for="tab in secondaryTabs"
          :key="tab.key"
          type="button"
          :data-nav="tab.key"
          :aria-current="tab.active ? 'page' : undefined"
          class="md-state text-title-small relative inline-flex h-12 shrink-0 cursor-pointer items-center gap-2 px-4 whitespace-nowrap transition-colors duration-150 ease-standard"
          :class="
            tab.active ? 'text-primary' : 'text-on-surface-variant hover:text-on-surface'
          "
          @click="navigateTab(tab)"
        >
          <component :is="tab.icon" class="h-[18px] w-[18px] shrink-0" aria-hidden="true" />
          <span>{{ tab.label }}</span>
          <span
            v-if="tab.active"
            aria-hidden="true"
            class="pointer-events-none absolute right-3 bottom-0 left-3 h-[3px] rounded-t-corner-xs bg-primary"
          />
        </button>
      </nav>
    </div>

    <!-- Main Scrollable Route Viewport -->
    <main
      ref="mainScrollRef"
      class="col-start-1 row-start-2 min-h-0 min-w-0 overflow-y-auto bg-surface md:col-start-2"
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
      class="col-start-1 row-start-3 min-w-0 md:col-start-2 md:pb-[var(--safe-area-bottom)]"
    >
      <PlayerBar />
    </div>

    <!-- Compact / Medium (<840px): MD3 Bottom Navigation Bar -->
    <NavigationBar
      class="col-start-1 row-start-4"
      :domain-tabs="domainTabs"
      :active-domain="domainStore.activeDomain"
      :is-settings-route="isSettingsRoute"
      @switch-domain="switchDomain"
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

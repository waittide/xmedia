<script setup lang="ts">
import { computed, defineAsyncComponent, onBeforeUnmount, onMounted, ref, watch, type Component } from 'vue'
import { useI18n } from 'vue-i18n'
import { LuBookOpen, LuClapperboard, LuDisc3, LuLayers } from 'vue-icons-plus/lu'
import { useEscapeStack } from '@/composables/useEscapeStack'
import { useFocusTrap } from '@/composables/useFocusTrap'
import type { MediaKind } from '@/types/api'
import { stripAriaModalOnLeave } from '@/utils/dom'
import { entryCompositeKey } from '../completeness'
import { useToolInspectorStore } from '../stores/inspector'
import { useToolScrapeStore } from '../stores/scrape'
import { useToolWorkspaceStore } from '../stores/workspace'
import { useToolTaskStore } from '../tasks'
import type { ToolStatusDto } from '../types'
import ToolHomeView from './ToolHomeView.vue'
import ToolItemsBrowser from './ToolItemsBrowser.vue'
import ToolSidebar from './ToolSidebar.vue'
import ToolTopHeader from './ToolTopHeader.vue'
import ToolMobileBottomNav from './workspace/ToolMobileBottomNav.vue'
import ToolWorkspaceModals from './workspace/ToolWorkspaceModals.vue'
import { useWorkspaceShortcuts } from './workspace/useWorkspaceShortcuts'

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

const { t } = useI18n()
const workspaceStore = useToolWorkspaceStore()
const inspectorStore = useToolInspectorStore()
const scrapeStore = useToolScrapeStore()
const taskStore = useToolTaskStore()

const topHeaderRef = ref<InstanceType<typeof ToolTopHeader> | null>(null)
const inspectorOverlayRef = ref<HTMLElement | null>(null)
const tasksDrawerMounted = ref(false)
watch(
  () => workspaceStore.tasksDrawerOpen,
  (isOpen) => {
    if (isOpen) tasksDrawerMounted.value = true
  },
)

const desktopMediaQuery =
  typeof window !== 'undefined' ? window.matchMedia('(min-width: 600px)') : null
const isDesktop = ref(desktopMediaQuery ? desktopMediaQuery.matches : true)
function handleDesktopBreakpointChange(ev: MediaQueryListEvent): void {
  isDesktop.value = ev.matches
  if (ev.matches) {
    workspaceStore.mobileMenuOpen = false
  }
}

const largeMediaQuery =
  typeof window !== 'undefined' ? window.matchMedia('(min-width: 1200px)') : null
const isLargeDesktop = ref(largeMediaQuery ? largeMediaQuery.matches : true)
function handleLargeBreakpointChange(ev: MediaQueryListEvent): void {
  isLargeDesktop.value = ev.matches
}

const mainContentScrolled = ref(false)
function onToolMainScroll(ev: Event): void {
  const el = ev.target as HTMLElement | null
  mainContentScrolled.value = Boolean(el && el.scrollTop > 4)
}

const domainTabs = computed<Array<{ value: MediaKind; label: string; icon: Component }>>(() => [
  { value: 'book', label: t('tool.domains.book'), icon: LuBookOpen },
  { value: 'comic', label: t('tool.domains.comic'), icon: LuLayers },
  { value: 'music', label: t('tool.domains.music'), icon: LuDisc3 },
  { value: 'video', label: t('tool.domains.video'), icon: LuClapperboard },
])

useWorkspaceShortcuts({
  focusSearch: () => topHeaderRef.value?.focusSearch(),
})

const escapeStack = useEscapeStack()
const inspectorVisible = computed(
  () =>
    (workspaceStore.activeNav === 'home' || workspaceStore.activeNav === 'items') &&
    Boolean(inspectorStore.activeDetail || inspectorStore.loadingInspector),
)
const inspectorModalActive = computed(() => !isLargeDesktop.value && inspectorVisible.value)

async function closeInspectorOverlay(): Promise<void> {
  if (!(await inspectorStore.confirmActiveEditorClean())) return
  workspaceStore.closeInspectorAndSync()
}

useFocusTrap(inspectorOverlayRef, () => inspectorModalActive.value)
escapeStack.register(
  () => inspectorModalActive.value,
  () => {
    void closeInspectorOverlay()
  },
)

let unbindTaskListeners: (() => void) | null = null

onMounted(() => {
  unbindTaskListeners = scrapeStore.bindTaskListeners()
  window.addEventListener('hashchange', workspaceStore.handleHashChange)
  desktopMediaQuery?.addEventListener('change', handleDesktopBreakpointChange)
  largeMediaQuery?.addEventListener('change', handleLargeBreakpointChange)
  void workspaceStore.loadInitial()
})

onBeforeUnmount(() => {
  unbindTaskListeners?.()
  window.removeEventListener('hashchange', workspaceStore.handleHashChange)
  desktopMediaQuery?.removeEventListener('change', handleDesktopBreakpointChange)
  largeMediaQuery?.removeEventListener('change', handleLargeBreakpointChange)
})
</script>

<template>
  <div
    class="relative flex h-screen w-screen flex-col overflow-hidden bg-surface-container text-on-surface supports-[height:100dvh]:h-dvh"
  >
    <!-- Full-Width Top Bar -->
    <ToolTopHeader
      ref="topHeaderRef"
      :is-compact="workspaceStore.isCompact"
      :active-domain="workspaceStore.activeDomain"
      :active-nav="workspaceStore.activeNav"
      :tasks-open="workspaceStore.tasksDrawerOpen"
      :domain-tabs="domainTabs"
      :search-query="workspaceStore.searchQuery"
      :search-placeholder="workspaceStore.domainLabels.searchPlaceholder"
      :status="status"
      :scrolled="mainContentScrolled"
      @toggle-sidebar="workspaceStore.toggleSidebar(isDesktop)"
      @navigate-home="workspaceStore.navigatePrimary('home')"
      @switch-domain="workspaceStore.switchDomain"
      @update:search-query="workspaceStore.searchQuery = $event"
      @lock="emit('lock')"
      @toggle-tasks="workspaceStore.tasksDrawerOpen = !workspaceStore.tasksDrawerOpen"
      @toggle-settings="
        workspaceStore.activeNav === 'settings'
          ? workspaceStore.exitSettings()
          : workspaceStore.openSettingsTab('appearance')
      "
    />

    <!-- Middle Body: Collapsible Sidebar (Normal Nav OR Scrape Queue) + Main Canvas -->
    <div class="relative flex min-h-0 flex-1 overflow-hidden">
      <ScrapeQueueSidebar
        v-if="scrapeStore.scrapeWorkbenchActive && workspaceStore.activeNav === 'items'"
        :mobile-menu-open="workspaceStore.mobileMenuOpen"
        :collapsed="workspaceStore.collapsed"
        :is-compact="workspaceStore.isCompact"
        :entries="scrapeStore.workbenchQueueEntries"
        :active-key="
          inspectorStore.activeDetail ? entryCompositeKey(inspectorStore.activeDetail.entry) : null
        "
        :saved-keys="scrapeStore.scrapeWorkbenchSavedPaths"
        :artwork-version="inspectorStore.artworkVersion"
        :artwork-versions="inspectorStore.fileArtworkVersions"
        :has-prev="scrapeStore.hasPrevInspect"
        :has-next="scrapeStore.hasNextInspect"
        @close-mobile="workspaceStore.mobileMenuOpen = false"
        @exit="scrapeStore.requestQueueExit()"
        @toggle-collapse="workspaceStore.toggleSidebar(isDesktop)"
        @select-entry="scrapeStore.requestQueuePickEntry"
        @step="scrapeStore.requestQueueStep"
      />

      <ToolSidebar
        v-else
        :mobile-menu-open="workspaceStore.mobileMenuOpen"
        :collapsed="workspaceStore.collapsed"
        :is-compact="workspaceStore.isCompact"
        :active-domain="workspaceStore.activeDomain"
        :active-nav="workspaceStore.activeNav"
        :domain-tabs="domainTabs"
        :total-items="workspaceStore.stats.total"
        :domain-roots="workspaceStore.domainRoots"
        :active-root-id="workspaceStore.activeRootId"
        @close-mobile="workspaceStore.mobileMenuOpen = false"
        @switch-domain="workspaceStore.switchDomain"
        @navigate-primary="(nav) => workspaceStore.navigatePrimary(nav)"
        @open-settings-roots="workspaceStore.openSettingsTab('roots')"
        @select-root="(rid) => workspaceStore.selectRoot(rid)"
      />

      <!-- Main Rounded Canvas + Supporting Pane -->
      <main
        class="flex min-w-0 flex-1 overflow-hidden bg-surface md:mr-3 md:mb-3 md:rounded-corner-xl"
      >
        <ScrapeWorkbenchView
          v-if="scrapeStore.scrapeWorkbenchActive && workspaceStore.activeNav === 'items'"
          :detail="inspectorStore.activeDetail"
          :loading-detail="inspectorStore.loadingInspector"
          :entries="scrapeStore.workbenchQueueEntries"
          :saved-keys="scrapeStore.scrapeWorkbenchSavedPaths"
          :artwork-version="inspectorStore.artworkVersion"
          :artwork-versions="inspectorStore.fileArtworkVersions"
          :sidebar-collapsed="workspaceStore.collapsed"
          :has-prev="scrapeStore.hasPrevInspect"
          :has-next="scrapeStore.hasNextInspect"
          :position-label="scrapeStore.inspectPositionLabel"
          :settings="workspaceStore.settings"
          @exit="scrapeStore.exitScrapeWorkbench()"
          @toggle-sidebar="workspaceStore.toggleSidebar(isDesktop)"
          @select-entry="workspaceStore.inspectEntry"
          @step="scrapeStore.stepInspectingFile"
          @updated="scrapeStore.handleDetailUpdated"
          @open-settings-scrapers="workspaceStore.openSettingsTab('scrapers')"
          @settings-updated="workspaceStore.handleSettingsUpdated"
          @open-history="workspaceStore.openHistoryModal('metadata')"
        />

        <template v-else>
          <div class="min-w-0 flex-1 overflow-y-auto" @scroll.passive="onToolMainScroll">
            <div
              v-if="workspaceStore.initialLoadError && !workspaceStore.settings"
              role="alert"
              class="flex h-full flex-col items-center justify-center gap-3 px-6 text-center"
            >
              <p class="max-w-md text-body-medium font-medium text-error">
                {{ workspaceStore.initialLoadError }}
              </p>
              <button
                type="button"
                :disabled="workspaceStore.loadingInitial"
                class="md-btn md-btn-filled md-state"
                @click="workspaceStore.loadInitial()"
              >
                {{ t('tool.workspace.retryLoadConfig') }}
              </button>
            </div>

            <ToolSettingsView
              v-else-if="workspaceStore.activeNav === 'settings'"
              :settings="workspaceStore.settings"
              :active-domain="workspaceStore.activeDomain"
              :initial-tab="workspaceStore.settingsInitialTab"
              :ffmpeg-status="status?.ffmpeg ?? null"
              @back="workspaceStore.exitSettings()"
              @updated="workspaceStore.handleSettingsUpdated"
              @select-domain="workspaceStore.switchDomain"
            />

            <ToolHomeView
              v-else-if="workspaceStore.activeNav === 'home'"
              :active-domain="workspaceStore.activeDomain"
              :domain-roots="workspaceStore.domainRoots"
              :active-root-id="workspaceStore.activeRootId"
              :snapshot="workspaceStore.snapshot"
              :loading-dir="workspaceStore.loadingDir"
              :artwork-version="inspectorStore.artworkVersion"
              :artwork-versions="inspectorStore.fileArtworkVersions"
              :stats="workspaceStore.stats"
              @open-items="(f) => workspaceStore.selectCompletenessFilter(f ?? 'all')"
              @inspect-file="workspaceStore.inspectEntry"
              @scrape-files="scrapeStore.enqueueScrapeFiles"
              @open-rename-all="workspaceStore.openRenameModal(workspaceStore.snapshot?.files ?? [])"
              @open-settings="workspaceStore.openSettingsTab"
              @refresh-dir="workspaceStore.reloadCurrentScope()"
            />

            <ToolItemsBrowser v-else />
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
                v-if="inspectorStore.loadingInspector && !inspectorStore.activeDetail"
                class="flex h-full w-full items-center justify-center px-12 text-body-small text-on-surface-variant lg:w-[440px]"
              >
                {{ t('tool.workspace.parsingFile') }}
              </div>
              <MetadataInspectorDrawer
                v-else-if="inspectorStore.activeDetail"
                :detail="inspectorStore.activeDetail"
                :artwork-version="
                  inspectorStore.getFileArtworkVersion(
                    inspectorStore.activeDetail.entry.root_id,
                    inspectorStore.activeDetail.entry.rel_path,
                  )
                "
                :has-prev="scrapeStore.hasPrevInspect"
                :has-next="scrapeStore.hasNextInspect"
                :position-label="scrapeStore.inspectPositionLabel"
                :shortcuts-disabled="workspaceStore.anyModalOpen"
                @step="scrapeStore.stepInspectingFile"
                @close="workspaceStore.closeInspectorAndSync()"
                @updated="scrapeStore.handleDetailUpdated"
                @open-workbench="(d) => scrapeStore.openScrapeWorkbench(d.entry)"
                @open-content-editor="workspaceStore.openBookContentEditor"
                @open-normalize-epub="(e) => workspaceStore.openBookNormalizeModal([e])"
                @open-comic-pages="workspaceStore.openComicPageManager"
                @open-music-tools="(e) => workspaceStore.openMusicToolbox([e])"
                @open-video-tools="(e) => workspaceStore.openVideoToolbox([e])"
                @open-rename="(e) => workspaceStore.openRenameModal([e])"
                @request-delete="
                  (e) => workspaceStore.requestDeleteFiles([workspaceStore.entrySelectionKey(e)], true)
                "
                @open-history="workspaceStore.openHistoryModal('metadata')"
              />
            </div>
          </Transition>
        </template>
      </main>
    </div>

    <ToolMobileBottomNav
      :active-nav="workspaceStore.activeNav"
      :tasks-drawer-open="workspaceStore.tasksDrawerOpen"
      :has-active-task="Boolean(taskStore.activeTask)"
      :unresolved-count="taskStore.unresolvedCount"
      @navigate-primary="
        (nav) => {
          workspaceStore.tasksDrawerOpen = false;
          void workspaceStore.navigatePrimary(nav)
        }
      "
      @toggle-tasks="workspaceStore.tasksDrawerOpen = !workspaceStore.tasksDrawerOpen"
      @toggle-settings="
        workspaceStore.tasksDrawerOpen = false;
        workspaceStore.activeNav === 'settings'
          ? workspaceStore.exitSettings()
          : workspaceStore.openSettingsTab('appearance')
      "
    />

    <ToolWorkspaceModals />

    <ToolTasksDrawer
      v-if="tasksDrawerMounted"
      :open="workspaceStore.tasksDrawerOpen"
      @close="workspaceStore.tasksDrawerOpen = false"
      @review-items="scrapeStore.handleReviewUnmatchedItems"
      @inspect-item="scrapeStore.handleInspectTaskItem"
      @open-book-editor="scrapeStore.handleOpenTaskBookEditor"
    />
  </div>
</template>

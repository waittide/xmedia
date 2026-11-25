<script setup lang="ts">
import { computed, ref, type Component } from 'vue'
import { useI18n } from 'vue-i18n'
import {
  LuAlertTriangle,
  LuClock,
  LuLoader2,
  LuLock,
  LuMonitor,
  LuMoon,
  LuPanelLeftClose,
  LuPanelLeftOpen,
  LuSearch,
  LuSettings,
  LuSun,
  LuWrench,
} from 'vue-icons-plus/lu'
import MdBadge from '@/components/ui/MdBadge.vue'
import MdButton from '@/components/ui/MdButton.vue'
import MdEmptyState from '@/components/ui/MdEmptyState.vue'
import MdIconButton from '@/components/ui/MdIconButton.vue'
import MdListItem from '@/components/ui/MdListItem.vue'
import MdSearchView from '@/components/ui/MdSearchView.vue'
import MdSegmentedButton from '@/components/ui/MdSegmentedButton.vue'
import MdTextField from '@/components/ui/MdTextField.vue'
import { useThemeStore } from '@/stores/theme'
import type { MediaKind } from '@/types/api'
import { useToolWorkspaceStore } from '../stores/workspace'
import { useToolTaskStore } from '../tasks'
import type { MediaFileEntry, ToolStatusDto } from '../types'

defineProps<{
  isCompact: boolean
  activeDomain: MediaKind
  activeNav: 'home' | 'items' | 'settings'
  tasksOpen: boolean
  domainTabs: Array<{ value: MediaKind; label: string; icon: Component }>
  searchQuery: string
  searchPlaceholder: string
  status: ToolStatusDto | null
  scrolled?: boolean
}>()

const emit = defineEmits<{
  toggleSidebar: []
  navigateHome: []
  switchDomain: [domain: MediaKind]
  'update:searchQuery': [value: string]
  lock: []
  toggleTasks: []
  toggleSettings: []
}>()

const { t } = useI18n()
const themeStore = useThemeStore()
const taskStore = useToolTaskStore()
const workspaceStore = useToolWorkspaceStore()
const mobileSearchOpen = ref(false)

const searchPreviewItems = computed<MediaFileEntry[]>(() =>
  workspaceStore.filteredFiles.slice(0, 12),
)

function pickMobileSearchItem(file: MediaFileEntry): void {
  mobileSearchOpen.value = false
  void workspaceStore.inspectEntry(file)
}

const activeTaskPillLabel = computed<string>(() => {
  const task = taskStore.activeTask
  if (!task) return ''
  const verb =
    task.kind === 'scrape'
      ? t('tool.header.verbScrape')
      : task.kind === 'rename'
        ? t('tool.header.verbRename')
        : t('tool.header.verbScan')
  if (task.total !== null && task.total > 0) {
    return `${verb} ${task.processed}/${task.total}`
  }
  if (task.processed > 0) {
    return `${verb} ${task.processed}`
  }
  return verb
})

const taskButtonTooltip = computed<string>(() => {
  if (taskStore.activeTask) {
    return t('tool.header.taskActiveTooltip', { title: taskStore.activeTask.title })
  }
  if (taskStore.unresolvedCount > 0) {
    return t('tool.header.taskUnresolvedTooltip', { count: taskStore.unresolvedCount })
  }
  return t('tool.tasks.title')
})

const searchFieldRef = ref<InstanceType<typeof MdTextField> | null>(null)

const themeModeTooltip = computed(() => {
  if (themeStore.themeMode === 'light') return `${t('theme.modeTitle')}: ${t('theme.light')}`
  if (themeStore.themeMode === 'dark') return `${t('theme.modeTitle')}: ${t('theme.dark')}`
  return `${t('theme.modeTitle')}: ${t('theme.system')}`
})

function cycleThemeMode(): void {
  if (themeStore.themeMode === 'system') themeStore.setThemeMode('light')
  else if (themeStore.themeMode === 'light') themeStore.setThemeMode('dark')
  else themeStore.setThemeMode('system')
}

function focusSearch(): void {
  if (typeof window !== 'undefined' && window.innerWidth < 600) {
    mobileSearchOpen.value = true
    return
  }
  searchFieldRef.value?.focus()
  searchFieldRef.value?.select()
}

defineExpose({ focusSearch })
</script>

<template>
  <header
    class="flex h-16 shrink-0 items-center justify-between gap-3 bg-surface px-3 transition-colors duration-150 select-none sm:px-4 md:bg-surface-container"
  >
    <!-- Left: Rail/Drawer Toggle + Soft Tonal Brand + MD3 Domain Segmented Button -->
    <div class="flex shrink-0 items-center gap-2">
      <MdIconButton
        :icon="isCompact ? LuPanelLeftOpen : LuPanelLeftClose"
        :tooltip="isCompact ? t('tool.nav.expandSidebar') : t('tool.nav.collapseSidebar')"
        :aria-label="isCompact ? t('tool.nav.expandSidebar') : t('tool.nav.collapseSidebar')"
        @click="emit('toggleSidebar')"
      />

      <button
        type="button"
        class="md-state flex cursor-pointer items-center gap-2.5 rounded-corner-full py-1 pr-2.5 pl-1 text-left"
        @click="emit('navigateHome')"
      >
        <div
          class="flex h-9 w-9 items-center justify-center rounded-corner-md bg-primary-container text-on-primary-container"
        >
          <LuWrench class="h-[18px] w-[18px]" aria-hidden="true" />
        </div>
        <div class="hidden lg:block">
          <div class="text-title-small leading-tight text-on-surface">
            {{ t('tool.brand') }}
          </div>
          <div class="text-label-small text-on-surface-variant">
            {{ t('tool.brandSub') }}
          </div>
        </div>
      </button>

      <!-- MD3 Segmented Domain Switcher -->
      <div class="ml-1 hidden md:block">
        <MdSegmentedButton
          :model-value="activeDomain"
          :options="domainTabs"
          size="sm"
          @update:model-value="emit('switchDomain', $event)"
        />
      </div>
    </div>

    <!-- Center: Desktop MD3 Search Field -->
    <div class="mx-auto hidden w-full max-w-xl sm:block">
      <MdTextField
        ref="searchFieldRef"
        :model-value="searchQuery"
        type="search"
        clearable
        :placeholder="searchPlaceholder"
        :aria-label="searchPlaceholder"
        @update:model-value="emit('update:searchQuery', $event)"
      >
        <template v-if="!searchQuery" #trailing>
          <kbd
            class="text-label-small hidden rounded-corner-full bg-surface-container px-2 py-0.5 font-mono text-on-surface-variant sm:inline"
          >
            ⌘K
          </kbd>
        </template>
      </MdTextField>
    </div>

    <!-- Right: Status Chip + Circular MD3 Icon Buttons -->
    <div class="flex shrink-0 items-center gap-1">
      <MdIconButton
        class="sm:hidden"
        :variant="searchQuery ? 'tonal' : 'standard'"
        :selected="Boolean(searchQuery)"
        :icon="LuSearch"
        :tooltip="searchPlaceholder"
        :aria-label="searchPlaceholder"
        @click="mobileSearchOpen = true"
      />
      <span
        v-if="status && !status.ffmpeg.ffprobe_available"
        class="text-label-medium hidden items-center gap-1.5 rounded-corner-full bg-warning-container px-3 py-1 text-on-warning-container sm:inline-flex"
        :title="t('tool.header.noFfprobeTooltip')"
      >
        <LuAlertTriangle class="h-3.5 w-3.5" aria-hidden="true" />
        <span>{{ t('tool.header.noFfprobe') }}</span>
      </span>

      <MdIconButton
        :icon="
          themeStore.themeMode === 'system'
            ? LuMonitor
            : themeStore.themeMode === 'light'
              ? LuSun
              : LuMoon
        "
        :tooltip="themeModeTooltip"
        :aria-label="themeModeTooltip"
        @click="cycleThemeMode"
      />

      <MdButton
        v-if="status?.requires_password"
        variant="tonal"
        :icon="LuLock"
        :title="t('tool.actions.lock')"
        @click="emit('lock')"
      >
        <span class="hidden sm:inline">{{ t('tool.actions.lock') }}</span>
      </MdButton>

      <!-- Persistent Task Progress Pill / Drawer Toggle -->
      <button
        v-if="taskStore.activeTask"
        type="button"
        class="md-state text-label-medium inline-flex h-9 cursor-pointer items-center gap-1.5 rounded-corner-full bg-primary-container px-3 text-on-primary-container transition hover:opacity-95"
        :class="tasksOpen ? 'ring-2 ring-primary/50' : ''"
        :title="taskButtonTooltip"
        :aria-label="taskButtonTooltip"
        @click="emit('toggleTasks')"
      >
        <LuLoader2 class="h-3.5 w-3.5 shrink-0 animate-spin" aria-hidden="true" />
        <span class="font-mono whitespace-nowrap">{{ activeTaskPillLabel }}</span>
      </button>

      <div v-else class="relative hidden sm:inline-flex">
        <MdIconButton
          :variant="tasksOpen ? 'tonal' : 'standard'"
          :selected="tasksOpen"
          :icon="LuClock"
          :tooltip="taskButtonTooltip"
          :aria-label="taskButtonTooltip"
          @click="emit('toggleTasks')"
        />
        <MdBadge
          v-if="taskStore.unresolvedCount > 0"
          :value="taskStore.unresolvedCount"
          :max="99"
          tone="error"
          class="pointer-events-none absolute -top-0.5 -right-0.5 ring-2 ring-surface-container"
        />
      </div>

      <MdIconButton
        class="hidden sm:inline-flex"
        :variant="activeNav === 'settings' ? 'tonal' : 'standard'"
        :selected="activeNav === 'settings'"
        :icon="LuSettings"
        :tooltip="t('nav.settings')"
        :aria-label="t('nav.settings')"
        @click="emit('toggleSettings')"
      />
    </div>

    <MdSearchView
      :open="mobileSearchOpen"
      :model-value="searchQuery"
      :placeholder="searchPlaceholder"
      :aria-label="searchPlaceholder"
      @update:model-value="emit('update:searchQuery', $event)"
      @submit="mobileSearchOpen = false"
      @close="mobileSearchOpen = false"
    >
      <div
        v-if="searchPreviewItems.length > 0"
        class="divide-y divide-outline-variant/35 overflow-hidden rounded-corner-lg bg-surface-container-low"
      >
        <MdListItem
          v-for="item in searchPreviewItems"
          :key="`${item.root_id}::${item.rel_path}`"
          clickable
          :headline="item.display_title"
          :supporting-text="item.rel_path"
          :trailing-supporting-text="item.extension.toUpperCase()"
          @click="pickMobileSearchItem(item)"
        />
      </div>
      <MdEmptyState
        v-else
        compact
        :icon="LuSearch"
        :title="t('tool.browser.emptyFilteredTitle')"
        :description="t('tool.browser.emptyFilteredDesc')"
      />
    </MdSearchView>
  </header>
</template>

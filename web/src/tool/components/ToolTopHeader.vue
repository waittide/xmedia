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
  LuX,
} from 'vue-icons-plus/lu'
import MdSegmentedButton from '@/components/ui/MdSegmentedButton.vue'
import { useThemeStore } from '@/stores/theme'
import type { MediaKind } from '@/types/api'
import { useToolText } from '../i18n'
import { useToolTaskStore } from '../tasks'
import type { ToolStatusDto } from '../types'

const props = defineProps<{
  isCompact: boolean
  activeDomain: MediaKind
  activeNav: 'home' | 'items' | 'settings'
  tasksOpen: boolean
  domainTabs: Array<{ value: MediaKind; label: string; icon: Component }>
  searchQuery: string
  searchPlaceholder: string
  status: ToolStatusDto | null
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
const txt = useToolText()
const themeStore = useThemeStore()
const taskStore = useToolTaskStore()

const activeTaskPillLabel = computed<string>(() => {
  const task = taskStore.activeTask
  if (!task) return ''
  const verb = task.kind === 'scrape' ? '刮削中' : task.kind === 'rename' ? '重命名中' : '扫描中'
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
    return `${taskStore.activeTask.title} · 点击展开任务抽屉`
  }
  if (taskStore.unresolvedCount > 0) {
    return `后台任务 · ${taskStore.unresolvedCount} 项待手动刮削或重试`
  }
  return txt.value.tasks.title
})

const searchInputRef = ref<HTMLInputElement | null>(null)

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
  searchInputRef.value?.focus()
  searchInputRef.value?.select()
}

defineExpose({ focusSearch })
</script>

<template>
  <header
    class="flex h-16 shrink-0 items-center justify-between gap-3 bg-[var(--md-sys-color-surface-container)] px-3 select-none sm:px-4"
  >
    <!-- Left: Rail/Drawer Toggle + Soft Tonal Brand + MD3 Domain Segmented Button -->
    <div class="flex shrink-0 items-center gap-2">
      <button
        type="button"
        class="md-icon-btn md-state"
        :title="isCompact ? '展开导航栏' : '收起为图标导航栏'"
        @click="emit('toggleSidebar')"
      >
        <LuPanelLeftOpen v-if="isCompact" class="h-5 w-5" />
        <LuPanelLeftClose v-else class="h-5 w-5" />
      </button>

      <button
        type="button"
        class="md-state flex items-center gap-2.5 rounded-full py-1 pr-2.5 pl-1 text-left outline-none"
        @click="emit('navigateHome')"
      >
        <div
          class="flex h-9 w-9 items-center justify-center rounded-2xl bg-[var(--md-sys-color-primary-container)] text-[var(--md-sys-color-on-primary-container)]"
        >
          <LuWrench class="h-[18px] w-[18px]" />
        </div>
        <div class="hidden lg:block">
          <div class="text-sm leading-tight font-semibold tracking-tight text-[var(--md-sys-color-on-surface)]">
            {{ txt.brand }}
          </div>
          <div class="text-[10px] text-[var(--md-sys-color-on-surface-variant)]">
            {{ txt.brandSub }}
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

    <!-- Center: Authentic MD3 Search Bar -->
    <div class="relative mx-auto w-full max-w-xl">
      <LuSearch
        class="pointer-events-none absolute top-1/2 left-4 h-4 w-4 -translate-y-1/2 text-[var(--md-sys-color-on-surface-variant)]"
      />
      <input
        ref="searchInputRef"
        :value="searchQuery"
        type="text"
        :placeholder="searchPlaceholder"
        class="h-11 w-full rounded-full bg-[var(--md-sys-color-surface-container-high)] pr-14 pl-11 text-[13px] text-[var(--md-sys-color-on-surface)] placeholder-[var(--md-sys-color-on-surface-variant)] outline-none transition duration-200 focus:bg-[var(--md-sys-color-surface-container-lowest)] focus:shadow-[var(--md-sys-elevation-2)]"
        @input="emit('update:searchQuery', ($event.target as HTMLInputElement).value)"
      />
      <kbd
        v-if="!searchQuery"
        class="pointer-events-none absolute top-1/2 right-3.5 hidden -translate-y-1/2 rounded-full bg-[var(--md-sys-color-surface-container)] px-2 py-0.5 font-mono text-[10px] text-[var(--md-sys-color-on-surface-variant)] sm:inline"
      >
        ⌘K
      </kbd>
      <button
        v-else
        type="button"
        class="md-icon-btn md-icon-btn-sm md-state absolute top-1/2 right-1.5 -translate-y-1/2"
        @click="emit('update:searchQuery', '')"
      >
        <LuX class="h-4 w-4" />
      </button>
    </div>

    <!-- Right: Status Chip + Circular MD3 Icon Buttons -->
    <div class="flex shrink-0 items-center gap-1">
      <span
        v-if="status && !status.ffmpeg.ffprobe_available"
        class="hidden items-center gap-1.5 rounded-full bg-[var(--md-sys-color-warning-container)] px-3 py-1 text-[11px] font-medium text-[var(--md-sys-color-on-warning-container)] sm:inline-flex"
        title="系统未检测到 ffprobe，视频编码/分辨率/音轨与字幕流详情探测将受限"
      >
        <LuAlertTriangle class="h-3.5 w-3.5" />
        <span>无 ffprobe</span>
      </span>

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

      <button
        v-if="status?.requires_password"
        type="button"
        class="md-btn md-btn-sm md-btn-tonal md-state"
        :title="txt.actions.lock"
        @click="emit('lock')"
      >
        <LuLock class="h-3.5 w-3.5" />
        <span class="hidden sm:inline">{{ txt.actions.lock }}</span>
      </button>

      <!-- Persistent Task Progress Pill / Drawer Toggle -->
      <button
        v-if="taskStore.activeTask"
        type="button"
        class="md-state inline-flex h-9 items-center gap-1.5 rounded-full bg-[var(--md-sys-color-primary-container)] px-3 text-xs font-semibold text-[var(--md-sys-color-on-primary-container)] transition hover:opacity-95"
        :class="tasksOpen ? 'ring-2 ring-[var(--md-sys-color-primary)]/50' : ''"
        :title="taskButtonTooltip"
        @click="emit('toggleTasks')"
      >
        <LuLoader2 class="h-3.5 w-3.5 shrink-0 animate-spin" />
        <span class="font-mono whitespace-nowrap">{{ activeTaskPillLabel }}</span>
      </button>

      <button
        v-else
        type="button"
        class="md-icon-btn md-state relative"
        :class="
          tasksOpen
            ? 'bg-[var(--md-sys-color-secondary-container)] text-[var(--md-sys-color-on-secondary-container)]'
            : ''
        "
        :title="taskButtonTooltip"
        @click="emit('toggleTasks')"
      >
        <LuClock class="h-[18px] w-[18px]" />
        <span
          v-if="taskStore.unresolvedCount > 0"
          class="absolute top-1.5 right-1.5 h-2 w-2 rounded-full bg-[var(--md-sys-color-warning)] ring-2 ring-[var(--md-sys-color-surface-container)]"
        />
      </button>

      <button
        type="button"
        class="md-icon-btn md-state"
        :class="
          activeNav === 'settings'
            ? 'bg-[var(--md-sys-color-secondary-container)] text-[var(--md-sys-color-on-secondary-container)]'
            : ''
        "
        :title="t('nav.settings')"
        @click="emit('toggleSettings')"
      >
        <LuSettings class="h-[18px] w-[18px]" />
      </button>
    </div>
  </header>
</template>

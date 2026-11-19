<script setup lang="ts">
import { computed, ref, type Component } from 'vue'
import { useI18n } from 'vue-i18n'
import {
  LuAlertTriangle,
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
import { useThemeStore } from '@/stores/theme'
import type { MediaKind } from '@/types/api'
import { useToolText } from '../i18n'
import type { ToolStatusDto } from '../types'

const props = defineProps<{
  isCompact: boolean
  activeDomain: MediaKind
  activeNav: 'home' | 'items' | 'tasks' | 'settings'
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
  toggleSettings: []
}>()

const { t } = useI18n()
const txt = useToolText()
const themeStore = useThemeStore()

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
    class="flex h-16 shrink-0 items-center justify-between gap-3 bg-[var(--ctp-shell)] px-4 select-none sm:px-5"
  >
    <!-- Left: Sidebar Toggle + Accent Brand + 4-Domain Capsule Switcher -->
    <div class="flex shrink-0 items-center gap-2.5">
      <button
        type="button"
        class="flex h-9 w-9 items-center justify-center rounded-xl text-[var(--ctp-subtext1)] transition hover:bg-[var(--ctp-surface0)]/60 hover:text-[var(--ctp-text)]"
        @click="emit('toggleSidebar')"
      >
        <LuPanelLeftOpen v-if="isCompact" class="h-[18px] w-[18px]" />
        <LuPanelLeftClose v-else class="h-[18px] w-[18px]" />
      </button>

      <button
        type="button"
        class="flex items-center gap-2.5 text-left outline-none transition hover:opacity-90"
        @click="emit('navigateHome')"
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
          @click="emit('switchDomain', tab.value)"
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
        :value="searchQuery"
        type="text"
        :placeholder="searchPlaceholder"
        class="h-10 w-full rounded-full border border-[var(--ctp-surface0)]/90 bg-[var(--ctp-mantle)]/70 pr-14 pl-10 text-xs text-[var(--ctp-text)] placeholder-[var(--ctp-subtext0)] outline-none transition focus:border-[var(--ctp-accent)] focus:bg-[var(--ctp-canvas)]"
        @input="emit('update:searchQuery', ($event.target as HTMLInputElement).value)"
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
        @click="emit('update:searchQuery', '')"
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
        @click="emit('toggleSettings')"
      >
        <LuSettings class="h-[18px] w-[18px]" />
      </button>
    </div>
  </header>
</template>

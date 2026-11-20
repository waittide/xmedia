<script setup lang="ts">
import { ref, type Component } from 'vue'
import {
  LuCheckCircle2,
  LuChevronDown,
  LuChevronRight,
  LuFolder,
  LuFolderPlus,
  LuHardDrive,
  LuHome,
  LuLibrary,
} from 'vue-icons-plus/lu'
import type { MediaKind } from '@/types/api'
import { useToolText } from '../i18n'
import type { ManagedRootDto } from '../types'

export type ToolCompletenessFilter =
  | 'all'
  | 'incomplete'
  | 'missing_cover'
  | 'missing_summary'
  | 'missing_tag'
  | 'complete'

defineProps<{
  mobileMenuOpen: boolean
  collapsed: boolean
  isCompact: boolean
  activeDomain: MediaKind
  activeNav: 'home' | 'items' | 'settings'
  domainTabs: Array<{ value: MediaKind; label: string; icon: Component }>
  totalItems: number
  domainRoots: ManagedRootDto[]
  activeRootId: string
  viewMode: 'flat' | 'folders'
  completenessFilter: ToolCompletenessFilter
  filterItems: Array<{
    id: ToolCompletenessFilter
    label: string
    count: number
    icon: Component
  }>
}>()

const emit = defineEmits<{
  closeMobile: []
  switchDomain: [domain: MediaKind]
  navigatePrimary: [nav: 'home' | 'items']
  openSettingsRoots: []
  openFoldersView: []
  selectRoot: [rootId: string]
  selectCompletenessFilter: [filter: ToolCompletenessFilter]
}>()

const txt = useToolText()

const collapsedGroups = ref<Record<string, boolean>>({
  roots: false,
  filters: false,
})

function toggleGroupFold(key: string): void {
  collapsedGroups.value = {
    ...collapsedGroups.value,
    [key]: !collapsedGroups.value[key],
  }
}
</script>

<template>
  <Transition name="md-fade">
    <div
      v-if="mobileMenuOpen"
      class="fixed inset-0 z-30 bg-black/40 backdrop-blur-[1px] md:hidden"
      @click="emit('closeMobile')"
    />
  </Transition>

  <aside
    class="fixed top-16 bottom-0 left-0 z-40 flex shrink-0 flex-col bg-[var(--md-sys-color-surface-container)] transition-[width,transform] duration-300 ease-[var(--md-sys-motion-easing-standard)] select-none md:static"
    :class="[
      mobileMenuOpen
        ? 'w-72 translate-x-0 rounded-r-[28px] bg-[var(--md-sys-color-surface-container-low)] shadow-[var(--md-sys-elevation-3)] md:rounded-none md:bg-[var(--md-sys-color-surface-container)] md:shadow-none'
        : '-translate-x-full md:translate-x-0',
      collapsed ? 'md:w-20' : 'md:w-64',
    ]"
  >
    <!-- Mobile 4-Domain Switcher Pills -->
    <div class="grid grid-cols-2 gap-1.5 px-3 pt-3 pb-2 md:hidden">
      <button
        v-for="tab in domainTabs"
        :key="`mob-${tab.value}`"
        type="button"
        class="md-state flex items-center justify-center gap-1.5 rounded-full px-3 py-2 text-xs font-semibold"
        :class="
          activeDomain === tab.value
            ? 'bg-[var(--md-sys-color-secondary-container)] text-[var(--md-sys-color-on-secondary-container)]'
            : 'bg-[var(--md-sys-color-surface-container-high)] text-[var(--md-sys-color-on-surface-variant)]'
        "
        @click="emit('switchDomain', tab.value)"
      >
        <component :is="tab.icon" class="h-3.5 w-3.5 shrink-0" />
        <span class="truncate">{{ tab.label }}</span>
      </button>
    </div>

    <!-- COMPACT MODE: Authentic MD3 Navigation Rail (80px, 56x32 Active Pill + Label) -->
    <nav
      v-if="isCompact"
      class="hidden flex-1 flex-col items-center gap-2 overflow-y-auto px-2 pt-2 pb-4 md:flex"
    >
      <button
        type="button"
        class="group flex w-full flex-col items-center gap-1 py-1 text-[11px] font-medium outline-none"
        @click="emit('navigatePrimary', 'home')"
      >
        <span
          class="md-state flex h-8 w-14 items-center justify-center rounded-full transition"
          :class="
            activeNav === 'home'
              ? 'bg-[var(--md-sys-color-secondary-container)] text-[var(--md-sys-color-on-secondary-container)]'
              : 'text-[var(--md-sys-color-on-surface-variant)] group-hover:text-[var(--md-sys-color-on-surface)]'
          "
        >
          <LuHome class="h-[18px] w-[18px]" />
        </span>
        <span
          class="truncate"
          :class="
            activeNav === 'home'
              ? 'font-semibold text-[var(--md-sys-color-on-surface)]'
              : 'text-[var(--md-sys-color-on-surface-variant)]'
          "
        >
          {{ txt.nav.home }}
        </span>
      </button>

      <button
        type="button"
        class="group flex w-full flex-col items-center gap-1 py-1 text-[11px] font-medium outline-none"
        @click="emit('navigatePrimary', 'items')"
      >
        <span
          class="md-state flex h-8 w-14 items-center justify-center rounded-full transition"
          :class="
            activeNav === 'items'
              ? 'bg-[var(--md-sys-color-secondary-container)] text-[var(--md-sys-color-on-secondary-container)]'
              : 'text-[var(--md-sys-color-on-surface-variant)] group-hover:text-[var(--md-sys-color-on-surface)]'
          "
        >
          <LuLibrary class="h-[18px] w-[18px]" />
        </span>
        <span
          class="truncate"
          :class="
            activeNav === 'items'
              ? 'font-semibold text-[var(--md-sys-color-on-surface)]'
              : 'text-[var(--md-sys-color-on-surface-variant)]'
          "
        >
          {{ txt.nav.items }}
        </span>
      </button>

      <div class="my-1 h-px w-8 bg-[var(--md-sys-color-outline-variant)]/60" />

      <button
        type="button"
        class="md-icon-btn md-state"
        :title="txt.roots.title"
        @click="emit('openSettingsRoots')"
      >
        <LuHardDrive class="h-[18px] w-[18px]" />
      </button>
      <button
        type="button"
        class="md-icon-btn md-state"
        title="文件夹视图"
        @click="emit('openFoldersView')"
      >
        <LuFolder class="h-[18px] w-[18px]" />
      </button>
      <button
        type="button"
        class="md-icon-btn md-state"
        :title="txt.filters.incomplete"
        @click="emit('selectCompletenessFilter', 'incomplete')"
      >
        <LuCheckCircle2 class="h-[18px] w-[18px]" />
      </button>
    </nav>

    <!-- EXPANDED MODE: Authentic MD3 Navigation Drawer (Full-Pill Active Indicator) -->
    <nav
      v-else
      class="flex-1 space-y-1 overflow-y-auto px-3 pt-2 pb-5"
    >
      <button
        type="button"
        class="md-state flex h-11 w-full items-center gap-3 rounded-full px-4 text-[13px] font-medium"
        :class="
          activeNav === 'home'
            ? 'bg-[var(--md-sys-color-secondary-container)] font-semibold text-[var(--md-sys-color-on-secondary-container)]'
            : 'text-[var(--md-sys-color-on-surface-variant)] hover:text-[var(--md-sys-color-on-surface)]'
        "
        @click="emit('navigatePrimary', 'home')"
      >
        <LuHome class="h-[18px] w-[18px] shrink-0" />
        <span class="truncate">{{ txt.nav.home }}</span>
      </button>

      <button
        type="button"
        class="md-state flex h-11 w-full items-center gap-3 rounded-full px-4 text-[13px] font-medium"
        :class="
          activeNav === 'items'
            ? 'bg-[var(--md-sys-color-secondary-container)] font-semibold text-[var(--md-sys-color-on-secondary-container)]'
            : 'text-[var(--md-sys-color-on-surface-variant)] hover:text-[var(--md-sys-color-on-surface)]'
        "
        @click="emit('navigatePrimary', 'items')"
      >
        <LuLibrary class="h-[18px] w-[18px] shrink-0" />
        <span class="flex-1 truncate text-left">{{ txt.nav.items }}</span>
        <span class="font-mono text-[11px] opacity-75">{{ totalItems }}</span>
      </button>

      <!-- Group 1: Media Roots -->
      <div class="pt-4">
        <div class="mb-1 flex items-center justify-between px-3">
          <button
            type="button"
            class="flex min-w-0 flex-1 items-center gap-1 py-1 text-left text-[11px] font-semibold tracking-wider text-[var(--md-sys-color-on-surface-variant)] uppercase transition hover:text-[var(--md-sys-color-on-surface)]"
            @click="toggleGroupFold('roots')"
          >
            <LuChevronRight v-if="collapsedGroups.roots" class="h-3.5 w-3.5 shrink-0" />
            <LuChevronDown v-else class="h-3.5 w-3.5 shrink-0" />
            <span class="truncate">{{ txt.roots.title }}</span>
          </button>
          <button
            type="button"
            class="md-icon-btn md-icon-btn-sm md-state"
            :title="txt.roots.add"
            @click="emit('openSettingsRoots')"
          >
            <LuFolderPlus class="h-3.5 w-3.5" />
          </button>
        </div>

        <div v-if="!collapsedGroups.roots" class="space-y-0.5">
          <button
            v-if="domainRoots.length === 0"
            type="button"
            class="md-state flex w-full items-center gap-2 rounded-2xl border border-dashed border-[var(--md-sys-color-outline-variant)] px-3.5 py-2.5 text-left text-xs text-[var(--md-sys-color-on-surface-variant)]"
            @click="emit('openSettingsRoots')"
          >
            <LuFolderPlus class="h-3.5 w-3.5 shrink-0 text-[var(--md-sys-color-primary)]" />
            <span>{{ txt.roots.add }}</span>
          </button>

          <button
            v-for="r in domainRoots"
            :key="r.id"
            type="button"
            class="md-state flex h-9 w-full items-center gap-2.5 rounded-full px-3.5 text-xs font-medium"
            :class="
              activeRootId === r.id
                ? 'bg-[var(--md-sys-color-secondary-container)] font-semibold text-[var(--md-sys-color-on-secondary-container)]'
                : 'text-[var(--md-sys-color-on-surface-variant)] hover:text-[var(--md-sys-color-on-surface)]'
            "
            :title="r.path"
            @click="emit('selectRoot', r.id)"
          >
            <LuHardDrive class="h-3.5 w-3.5 shrink-0" />
            <span class="flex-1 truncate text-left">{{ r.name }}</span>
          </button>
        </div>
      </div>

      <!-- Group 2: Completeness Filters -->
      <div class="pt-3">
        <div class="mb-1 flex items-center justify-between px-3">
          <button
            type="button"
            class="flex min-w-0 flex-1 items-center gap-1 py-1 text-left text-[11px] font-semibold tracking-wider text-[var(--md-sys-color-on-surface-variant)] uppercase transition hover:text-[var(--md-sys-color-on-surface)]"
            @click="toggleGroupFold('filters')"
          >
            <LuChevronRight v-if="collapsedGroups.filters" class="h-3.5 w-3.5 shrink-0" />
            <LuChevronDown v-else class="h-3.5 w-3.5 shrink-0" />
            <span class="truncate">{{ txt.filters.title }}</span>
          </button>
        </div>

        <div v-if="!collapsedGroups.filters" class="space-y-0.5">
          <button
            v-for="f in filterItems"
            :key="f.id"
            type="button"
            class="md-state flex h-9 w-full items-center gap-2.5 rounded-full px-3.5 text-xs font-medium"
            :class="
              activeNav === 'items' &&
              completenessFilter === f.id &&
              (f.id !== 'all' || viewMode === 'flat')
                ? 'bg-[var(--md-sys-color-secondary-container)] font-semibold text-[var(--md-sys-color-on-secondary-container)]'
                : 'text-[var(--md-sys-color-on-surface-variant)] hover:text-[var(--md-sys-color-on-surface)]'
            "
            @click="emit('selectCompletenessFilter', f.id)"
          >
            <component :is="f.icon" class="h-3.5 w-3.5 shrink-0" />
            <span class="flex-1 truncate text-left">{{ f.label }}</span>
            <span class="font-mono text-[10px] opacity-75">{{ f.count }}</span>
          </button>
        </div>
      </div>
    </nav>
  </aside>
</template>

<script setup lang="ts">
import { ref, type Component } from 'vue'
import {
  LuArrowUp,
  LuCheckCircle2,
  LuChevronDown,
  LuChevronRight,
  LuClock,
  LuFolder,
  LuFolderPlus,
  LuHardDrive,
  LuHome,
  LuLibrary,
} from 'vue-icons-plus/lu'
import type { MediaKind } from '@/types/api'
import { useToolText } from '../i18n'
import { useToolTaskStore } from '../tasks'
import type { DirectorySnapshot, ManagedRootDto } from '../types'

export type ToolCompletenessFilter =
  | 'all'
  | 'incomplete'
  | 'missing_cover'
  | 'missing_summary'
  | 'missing_tag'
  | 'complete'

const props = defineProps<{
  mobileMenuOpen: boolean
  collapsed: boolean
  isCompact: boolean
  activeDomain: MediaKind
  activeNav: 'home' | 'items' | 'tasks' | 'settings'
  domainTabs: Array<{ value: MediaKind; label: string; icon: Component }>
  totalItems: number
  domainRoots: ManagedRootDto[]
  activeRootId: string
  snapshot: DirectorySnapshot | null
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
  navigatePrimary: [nav: 'home' | 'items' | 'tasks']
  openSettingsRoots: []
  openFoldersView: []
  selectRoot: [rootId: string]
  enterFolder: [relPath: string]
  selectCompletenessFilter: [filter: ToolCompletenessFilter]
}>()

const txt = useToolText()
const taskStore = useToolTaskStore()

const collapsedGroups = ref<Record<string, boolean>>({
  roots: false,
  folders: false,
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
  <div
    v-if="mobileMenuOpen"
    class="fixed inset-0 z-30 bg-black/45 md:hidden"
    @click="emit('closeMobile')"
  />

  <aside
    class="fixed top-16 bottom-0 left-0 z-40 flex shrink-0 flex-col border-r border-[var(--ctp-surface0)]/65 bg-[var(--ctp-shell)] transition-all duration-200 select-none md:static md:border-r-0"
    :class="[
      mobileMenuOpen ? 'w-64 translate-x-0' : '-translate-x-full md:translate-x-0',
      collapsed ? 'md:w-16' : 'md:w-60',
    ]"
  >
    <!-- Mobile 4-Domain Switcher Grid -->
    <div
      class="grid grid-cols-2 gap-1.5 border-b border-[var(--ctp-surface0)]/60 px-3 py-2.5 md:hidden"
    >
      <button
        v-for="tab in domainTabs"
        :key="`mob-${tab.value}`"
        type="button"
        class="flex items-center justify-center gap-1.5 rounded-xl px-2.5 py-2 text-xs font-semibold transition"
        :class="
          activeDomain === tab.value
            ? 'bg-[var(--ctp-accent)] text-[var(--ctp-on-accent)]'
            : 'bg-[var(--ctp-mantle)] text-[var(--ctp-subtext1)]'
        "
        @click="emit('switchDomain', tab.value)"
      >
        <component :is="tab.icon" class="h-3.5 w-3.5 shrink-0" />
        <span class="truncate">{{ tab.label }}</span>
      </button>
    </div>

    <nav class="flex-1 space-y-0.5 overflow-y-auto px-3 pt-1 pb-4">
      <!-- Fixed Top Primary Navigation: 首页 / 条目 / 任务 -->
      <button
        type="button"
        class="flex w-full items-center gap-3 rounded-xl px-3 py-2 text-sm font-medium transition"
        :class="[
          isCompact ? 'md:justify-center md:px-0' : '',
          activeNav === 'home'
            ? 'bg-[var(--ctp-accent-soft)] font-semibold text-[var(--ctp-accent)]'
            : 'text-[var(--ctp-subtext1)] hover:bg-[var(--ctp-surface0)]/60 hover:text-[var(--ctp-text)]',
        ]"
        :title="isCompact ? txt.nav.home : undefined"
        @click="emit('navigatePrimary', 'home')"
      >
        <LuHome class="h-4 w-4 shrink-0" />
        <span v-if="!isCompact" class="truncate">{{ txt.nav.home }}</span>
      </button>

      <button
        type="button"
        class="flex w-full items-center gap-3 rounded-xl px-3 py-2 text-sm font-medium transition"
        :class="[
          isCompact ? 'md:justify-center md:px-0' : '',
          activeNav === 'items'
            ? 'bg-[var(--ctp-accent-soft)] font-semibold text-[var(--ctp-accent)]'
            : 'text-[var(--ctp-subtext1)] hover:bg-[var(--ctp-surface0)]/60 hover:text-[var(--ctp-text)]',
        ]"
        :title="isCompact ? txt.nav.items : undefined"
        @click="emit('navigatePrimary', 'items')"
      >
        <LuLibrary class="h-4 w-4 shrink-0" />
        <template v-if="!isCompact">
          <span class="flex-1 truncate text-left">{{ txt.nav.items }}</span>
          <span class="font-mono text-[11px] opacity-75">{{ totalItems }}</span>
        </template>
      </button>

      <button
        type="button"
        class="relative flex w-full items-center gap-3 rounded-xl px-3 py-2 text-sm font-medium transition"
        :class="[
          isCompact ? 'md:justify-center md:px-0' : '',
          activeNav === 'tasks'
            ? 'bg-[var(--ctp-accent-soft)] font-semibold text-[var(--ctp-accent)]'
            : 'text-[var(--ctp-subtext1)] hover:bg-[var(--ctp-surface0)]/60 hover:text-[var(--ctp-text)]',
        ]"
        :title="isCompact ? txt.nav.tasks : undefined"
        @click="emit('navigatePrimary', 'tasks')"
      >
        <LuClock class="h-4 w-4 shrink-0" />
        <template v-if="!isCompact">
          <span class="flex-1 truncate text-left">{{ txt.nav.tasks }}</span>
          <span
            v-if="taskStore.activeCount > 0"
            class="rounded-full bg-[var(--ctp-accent)] px-1.5 py-0.5 font-mono text-[10px] font-bold text-[var(--ctp-on-accent)]"
          >
            {{ taskStore.activeCount }}
          </span>
          <span
            v-else-if="taskStore.persistentTasks.length > 0"
            class="font-mono text-[11px] opacity-70"
          >
            {{ taskStore.persistentTasks.length }}
          </span>
        </template>
        <span
          v-else-if="taskStore.activeCount > 0"
          class="absolute top-1.5 right-2 h-2 w-2 rounded-full bg-[var(--ctp-accent)]"
        />
      </button>

      <!-- Compact Rail Icons for Sidebar Groups -->
      <div
        v-if="isCompact"
        class="mt-3 hidden space-y-0.5 border-t border-[var(--ctp-surface0)]/55 pt-3 md:block"
      >
        <button
          type="button"
          class="flex w-full items-center justify-center rounded-xl py-2 text-[var(--ctp-subtext1)] transition hover:bg-[var(--ctp-surface0)]/60 hover:text-[var(--ctp-text)]"
          :title="txt.roots.title"
          @click="emit('openSettingsRoots')"
        >
          <LuHardDrive class="h-4 w-4 shrink-0" />
        </button>
        <button
          type="button"
          class="flex w-full items-center justify-center rounded-xl py-2 text-[var(--ctp-subtext1)] transition hover:bg-[var(--ctp-surface0)]/60 hover:text-[var(--ctp-text)]"
          :title="txt.roots.foldersTitle"
          @click="emit('openFoldersView')"
        >
          <LuFolder class="h-4 w-4 shrink-0" />
        </button>
        <button
          type="button"
          class="flex w-full items-center justify-center rounded-xl py-2 text-[var(--ctp-subtext1)] transition hover:bg-[var(--ctp-surface0)]/60 hover:text-[var(--ctp-text)]"
          :title="txt.filters.title"
          @click="emit('selectCompletenessFilter', 'incomplete')"
        >
          <LuCheckCircle2 class="h-4 w-4 shrink-0" />
        </button>
      </div>

      <!-- Expanded Collapsible Sidebar Groups -->
      <template v-if="!isCompact">
        <div class="mt-4 flex items-center justify-between border-t border-[var(--ctp-surface0)]/50 px-2 pt-3">
          <span class="text-[10px] font-semibold tracking-wider text-[var(--ctp-subtext0)]/80 uppercase">
            {{ txt.domains[activeDomain] }}
          </span>
        </div>

        <!-- Group 1: Media Roots -->
        <div class="pt-3">
          <div class="mb-1 flex items-center justify-between px-2">
            <button
              type="button"
              class="flex min-w-0 flex-1 items-center gap-1 rounded-md py-0.5 text-left text-[11px] font-semibold tracking-wider text-[var(--ctp-subtext0)] uppercase transition hover:text-[var(--ctp-text)]"
              @click="toggleGroupFold('roots')"
            >
              <LuChevronRight v-if="collapsedGroups.roots" class="h-3 w-3 shrink-0" />
              <LuChevronDown v-else class="h-3 w-3 shrink-0" />
              <span class="truncate">{{ txt.roots.title }}</span>
            </button>
            <button
              type="button"
              class="rounded-md p-1 text-[var(--ctp-subtext0)] transition hover:bg-[var(--ctp-surface0)]/60 hover:text-[var(--ctp-accent)]"
              :title="txt.roots.add"
              @click="emit('openSettingsRoots')"
            >
              <LuFolderPlus class="h-3.5 w-3.5" />
            </button>
          </div>

          <div v-if="!collapsedGroups.roots" class="space-y-0.5">
            <div
              v-if="domainRoots.length === 0"
              class="rounded-xl border border-dashed border-[var(--ctp-surface1)] px-3 py-2.5 text-[11px] text-[var(--ctp-subtext0)]"
            >
              {{ txt.roots.add }}
            </div>

            <button
              v-for="r in domainRoots"
              :key="r.id"
              type="button"
              class="flex w-full items-center gap-2.5 rounded-xl px-3 py-1.5 text-xs font-medium transition"
              :class="
                activeRootId === r.id
                  ? 'bg-[var(--ctp-accent-soft)] font-semibold text-[var(--ctp-accent)]'
                  : 'text-[var(--ctp-subtext1)] hover:bg-[var(--ctp-surface0)]/60 hover:text-[var(--ctp-text)]'
              "
              :title="r.path"
              @click="emit('selectRoot', r.id)"
            >
              <LuHardDrive class="h-3.5 w-3.5 shrink-0" />
              <span class="flex-1 truncate text-left">{{ r.name }}</span>
            </button>
          </div>
        </div>

        <!-- Group 2: Subfolders -->
        <div
          v-if="snapshot && (snapshot.parent_rel_path !== null || snapshot.folders.length > 0)"
          class="pt-3"
        >
          <div class="mb-1 flex items-center justify-between px-2">
            <button
              type="button"
              class="flex min-w-0 flex-1 items-center gap-1 rounded-md py-0.5 text-left text-[11px] font-semibold tracking-wider text-[var(--ctp-subtext0)] uppercase transition hover:text-[var(--ctp-text)]"
              @click="toggleGroupFold('folders')"
            >
              <LuChevronRight v-if="collapsedGroups.folders" class="h-3 w-3 shrink-0" />
              <LuChevronDown v-else class="h-3 w-3 shrink-0" />
              <span class="truncate">{{ txt.roots.foldersTitle }}</span>
            </button>
            <span class="font-mono text-[10px] text-[var(--ctp-subtext0)]">
              {{ snapshot.folders.length }}
            </span>
          </div>

          <div v-if="!collapsedGroups.folders" class="space-y-0.5">
            <button
              v-if="snapshot.parent_rel_path !== null"
              type="button"
              class="flex w-full items-center gap-2.5 rounded-xl px-3 py-1.5 text-xs text-[var(--ctp-subtext1)] transition hover:bg-[var(--ctp-surface0)]/60 hover:text-[var(--ctp-text)]"
              @click="emit('enterFolder', snapshot.parent_rel_path)"
            >
              <LuArrowUp class="h-3.5 w-3.5 shrink-0" />
              <span class="truncate">{{ txt.roots.parentDir }}</span>
            </button>

            <button
              v-for="folder in snapshot.folders"
              :key="folder.rel_path"
              type="button"
              class="flex w-full items-center gap-2 rounded-xl px-3 py-1.5 text-xs text-[var(--ctp-subtext1)] transition hover:bg-[var(--ctp-surface0)]/60 hover:text-[var(--ctp-text)]"
              @click="emit('enterFolder', folder.rel_path)"
            >
              <LuFolder class="h-3.5 w-3.5 shrink-0 text-[var(--ctp-accent)]" />
              <span class="flex-1 truncate text-left">{{ folder.name }}</span>
              <span class="font-mono text-[10px] opacity-70">{{ folder.media_file_count }}</span>
            </button>
          </div>
        </div>

        <!-- Group 3: Completeness Filters -->
        <div class="pt-3">
          <div class="mb-1 flex items-center justify-between px-2">
            <button
              type="button"
              class="flex min-w-0 flex-1 items-center gap-1 rounded-md py-0.5 text-left text-[11px] font-semibold tracking-wider text-[var(--ctp-subtext0)] uppercase transition hover:text-[var(--ctp-text)]"
              @click="toggleGroupFold('filters')"
            >
              <LuChevronRight v-if="collapsedGroups.filters" class="h-3 w-3 shrink-0" />
              <LuChevronDown v-else class="h-3 w-3 shrink-0" />
              <span class="truncate">{{ txt.filters.title }}</span>
            </button>
          </div>

          <div v-if="!collapsedGroups.filters" class="space-y-0.5">
            <button
              v-for="item in filterItems"
              :key="item.id"
              type="button"
              class="flex w-full items-center gap-2.5 rounded-xl px-3 py-1.5 text-xs font-medium transition"
              :class="
                activeNav === 'items' && completenessFilter === item.id
                  ? 'bg-[var(--ctp-accent-soft)] font-semibold text-[var(--ctp-accent)]'
                  : 'text-[var(--ctp-subtext1)] hover:bg-[var(--ctp-surface0)]/60 hover:text-[var(--ctp-text)]'
              "
              @click="emit('selectCompletenessFilter', item.id)"
            >
              <component :is="item.icon" class="h-3.5 w-3.5 shrink-0" />
              <span class="flex-1 truncate text-left">{{ item.label }}</span>
              <span class="font-mono text-[10px] opacity-75">{{ item.count }}</span>
            </button>
          </div>
        </div>
      </template>
    </nav>
  </aside>
</template>

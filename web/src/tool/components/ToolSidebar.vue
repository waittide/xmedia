<script setup lang="ts">
import { ref, type Component } from 'vue'
import { useI18n } from 'vue-i18n'
import {
  LuChevronDown,
  LuChevronRight,
  LuFolderPlus,
  LuHardDrive,
  LuHome,
  LuLibrary,
} from 'vue-icons-plus/lu'
import type { MediaKind } from '@/types/api'
import { useEscapeStack } from '@/composables/useEscapeStack'
import { useFocusTrap } from '@/composables/useFocusTrap'
import type { ManagedRootDto } from '../types'

const props = defineProps<{
  mobileMenuOpen: boolean
  collapsed: boolean
  isCompact: boolean
  activeDomain: MediaKind
  activeNav: 'home' | 'items' | 'settings'
  domainTabs: Array<{ value: MediaKind; label: string; icon: Component }>
  totalItems: number
  domainRoots: ManagedRootDto[]
  activeRootId: string
}>()

const emit = defineEmits<{
  closeMobile: []
  switchDomain: [domain: MediaKind]
  navigatePrimary: [nav: 'home' | 'items']
  openSettingsRoots: []
  selectRoot: [rootId: string]
}>()

const { t } = useI18n()
const rootsFolded = ref(false)
const asideRef = ref<HTMLElement | null>(null)
const escapeStack = useEscapeStack()
escapeStack.register(
  () => props.mobileMenuOpen,
  () => emit('closeMobile'),
)
useFocusTrap(asideRef, () => props.mobileMenuOpen)
</script>

<template>
  <Transition name="md-fade">
    <div
      v-if="mobileMenuOpen"
      class="fixed inset-0 z-30 bg-scrim/32 sm:hidden"
      @click="emit('closeMobile')"
    />
  </Transition>

  <aside
    ref="asideRef"
    :role="mobileMenuOpen ? 'dialog' : undefined"
    :aria-modal="mobileMenuOpen ? 'true' : undefined"
    :aria-label="t('tool.brand')"
    class="fixed top-16 bottom-0 left-0 z-40 flex shrink-0 flex-col bg-surface-container transition-[width,transform] duration-200 ease-standard select-none sm:static"
    :class="[
      mobileMenuOpen
        ? 'w-72 translate-x-0 rounded-r-corner-lg bg-surface-container-low shadow-elevation-1 sm:rounded-none sm:bg-surface-container sm:shadow-none'
        : '-translate-x-full sm:translate-x-0',
      collapsed ? 'sm:w-20' : 'sm:w-64',
    ]"
  >
    <!-- Mobile 4-Domain Switcher Pills -->
    <div class="grid grid-cols-2 gap-1.5 px-3 pt-3 pb-2 sm:hidden">
      <button
        v-for="tab in domainTabs"
        :key="`mob-${tab.value}`"
        type="button"
        class="md-state text-label-large flex min-h-10 cursor-pointer items-center justify-center gap-1.5 rounded-corner-full px-3 py-2"
        :class="
          activeDomain === tab.value
            ? 'bg-secondary-container text-on-secondary-container'
            : 'bg-surface-container-high text-on-surface-variant'
        "
        @click="emit('switchDomain', tab.value)"
      >
        <component :is="tab.icon" class="h-4 w-4 shrink-0" aria-hidden="true" />
        <span class="truncate">{{ tab.label }}</span>
      </button>
    </div>

    <!-- COMPACT MODE: MD3 Navigation Rail -->
    <nav
      v-if="isCompact"
      class="hidden flex-1 flex-col items-center gap-2 overflow-y-auto px-2 pt-2 pb-4 sm:flex"
    >
      <button
        type="button"
        class="group flex w-full cursor-pointer flex-col items-center gap-1 py-1 outline-none"
        @click="emit('navigatePrimary', 'home')"
      >
        <span
          class="md-state md-state-target flex h-8 w-14 items-center justify-center rounded-corner-full transition-colors duration-150 ease-standard group-focus-visible:outline-3 group-focus-visible:outline-offset-2 group-focus-visible:outline-secondary"
          :class="
            activeNav === 'home'
              ? 'bg-secondary-container text-on-secondary-container'
              : 'text-on-surface-variant group-hover:text-on-surface'
          "
        >
          <LuHome class="h-5 w-5" aria-hidden="true" />
        </span>
        <span
          class="text-label-medium truncate"
          :class="activeNav === 'home' ? 'text-on-surface' : 'text-on-surface-variant'"
        >
          {{ t('tool.nav.home') }}
        </span>
      </button>

      <button
        type="button"
        class="group flex w-full cursor-pointer flex-col items-center gap-1 py-1 outline-none"
        @click="emit('navigatePrimary', 'items')"
      >
        <span
          class="md-state md-state-target flex h-8 w-14 items-center justify-center rounded-corner-full transition-colors duration-150 ease-standard group-focus-visible:outline-3 group-focus-visible:outline-offset-2 group-focus-visible:outline-secondary"
          :class="
            activeNav === 'items' && !activeRootId
              ? 'bg-secondary-container text-on-secondary-container'
              : 'text-on-surface-variant group-hover:text-on-surface'
          "
        >
          <LuLibrary class="h-5 w-5" aria-hidden="true" />
        </span>
        <span
          class="text-label-medium truncate"
          :class="
            activeNav === 'items' && !activeRootId ? 'text-on-surface' : 'text-on-surface-variant'
          "
        >
          {{ t('tool.nav.items') }}
        </span>
      </button>

      <div class="my-1 h-px w-8 bg-outline-variant/60" />

      <button
        type="button"
        class="md-icon-btn md-state"
        :title="t('tool.roots.title')"
        :aria-label="t('tool.roots.title')"
        @click="emit('openSettingsRoots')"
      >
        <LuHardDrive class="h-5 w-5" aria-hidden="true" />
      </button>
    </nav>

    <!-- EXPANDED MODE: MD3 Navigation Drawer -->
    <nav
      v-else
      class="flex-1 space-y-1 overflow-y-auto px-3 pt-2 pb-5"
    >
      <button
        type="button"
        class="md-state text-label-large flex h-14 w-full cursor-pointer items-center gap-3 rounded-corner-full px-4"
        :class="
          activeNav === 'home'
            ? 'bg-secondary-container text-on-secondary-container'
            : 'text-on-surface-variant hover:text-on-surface'
        "
        @click="emit('navigatePrimary', 'home')"
      >
        <LuHome class="h-5 w-5 shrink-0" aria-hidden="true" />
        <span class="truncate">{{ t('tool.nav.home') }}</span>
      </button>

      <button
        type="button"
        class="md-state text-label-large flex h-14 w-full cursor-pointer items-center gap-3 rounded-corner-full px-4"
        :class="
          activeNav === 'items' && !activeRootId
            ? 'bg-secondary-container text-on-secondary-container'
            : 'text-on-surface-variant hover:text-on-surface'
        "
        @click="emit('navigatePrimary', 'items')"
      >
        <LuLibrary class="h-5 w-5 shrink-0" aria-hidden="true" />
        <span class="flex-1 truncate text-left">{{ t('tool.nav.items') }}</span>
        <span class="text-label-medium font-mono tabular-nums opacity-80">{{ totalItems }}</span>
      </button>

      <!-- Mounted Directories -->
      <div class="pt-4">
        <div class="mb-1 flex items-center justify-between px-3">
          <button
            type="button"
            class="text-label-medium flex min-w-0 flex-1 cursor-pointer items-center gap-1 py-1.5 text-left tracking-wider text-on-surface-variant uppercase transition-colors hover:text-on-surface"
            @click="rootsFolded = !rootsFolded"
          >
            <LuChevronRight v-if="rootsFolded" class="h-4 w-4 shrink-0" aria-hidden="true" />
            <LuChevronDown v-else class="h-4 w-4 shrink-0" aria-hidden="true" />
            <span class="truncate">{{ t('tool.roots.title') }}</span>
          </button>
          <button
            type="button"
            class="md-icon-btn md-icon-btn-sm md-state"
            :title="t('tool.roots.add')"
            :aria-label="t('tool.roots.add')"
            @click="emit('openSettingsRoots')"
          >
            <LuFolderPlus class="h-4 w-4" aria-hidden="true" />
          </button>
        </div>

        <div v-if="!rootsFolded" class="space-y-0.5">
          <button
            v-if="domainRoots.length === 0"
            type="button"
            class="md-state text-body-medium flex w-full cursor-pointer items-center gap-2 rounded-corner-md border border-dashed border-outline-variant px-3.5 py-2.5 text-left text-on-surface-variant"
            @click="emit('openSettingsRoots')"
          >
            <LuFolderPlus class="h-4 w-4 shrink-0 text-primary" aria-hidden="true" />
            <span>{{ t('tool.roots.add') }}</span>
          </button>

          <button
            v-for="r in domainRoots"
            :key="r.id"
            type="button"
            class="md-state text-label-large flex h-12 w-full cursor-pointer items-center gap-2.5 rounded-corner-full px-4"
            :class="
              activeNav === 'items' && activeRootId === r.id
                ? 'bg-secondary-container text-on-secondary-container'
                : 'text-on-surface-variant hover:text-on-surface'
            "
            :title="r.path"
            @click="emit('selectRoot', r.id)"
          >
            <LuHardDrive class="h-4 w-4 shrink-0" aria-hidden="true" />
            <span class="flex-1 truncate text-left">{{ r.name }}</span>
          </button>
        </div>
      </div>
    </nav>
  </aside>
</template>

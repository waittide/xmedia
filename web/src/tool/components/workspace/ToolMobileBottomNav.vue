<script setup lang="ts">
import { LuClock, LuHome, LuLibrary, LuLoader2, LuSettings } from 'vue-icons-plus/lu'
import MdBadge from '@/components/ui/MdBadge.vue'
import { useToolText } from '../../i18n'
import type { PrimaryNavKey } from './hashState'

defineProps<{
  activeNav: PrimaryNavKey
  tasksDrawerOpen: boolean
  hasActiveTask: boolean
  unresolvedCount: number
}>()

const emit = defineEmits<{
  navigatePrimary: [nav: 'home' | 'items']
  toggleTasks: []
  toggleSettings: []
}>()

const txt = useToolText()
</script>

<template>
  <nav
    aria-label="主导航"
    class="safe-pb safe-px relative z-30 shrink-0 border-t border-[var(--md-sys-color-outline-variant)]/50 bg-[var(--md-sys-color-surface-container)] select-none md:hidden"
  >
    <div class="grid h-16 grid-cols-4 items-center">
      <button
        type="button"
        :aria-current="activeNav === 'home' && !tasksDrawerOpen ? 'page' : undefined"
        class="group flex h-full flex-col items-center justify-center gap-1 outline-none"
        @click="emit('navigatePrimary', 'home')"
      >
        <span
          class="md-state flex h-8 w-16 items-center justify-center rounded-full transition-colors duration-[var(--md-sys-motion-duration-short)] group-focus-visible:outline-3 group-focus-visible:outline-offset-2 group-focus-visible:outline-[var(--md-sys-color-secondary)]"
          :class="
            activeNav === 'home' && !tasksDrawerOpen
              ? 'bg-[var(--md-sys-color-secondary-container)] text-[var(--md-sys-color-on-secondary-container)]'
              : 'text-[var(--md-sys-color-on-surface-variant)] group-hover:text-[var(--md-sys-color-on-surface)]'
          "
        >
          <LuHome class="h-5 w-5 shrink-0" />
        </span>
        <span
          class="md-label-medium truncate"
          :class="
            activeNav === 'home' && !tasksDrawerOpen
              ? 'font-semibold text-[var(--md-sys-color-on-surface)]'
              : 'text-[var(--md-sys-color-on-surface-variant)]'
          "
        >
          {{ txt.nav.home }}
        </span>
      </button>

      <button
        type="button"
        :aria-current="activeNav === 'items' && !tasksDrawerOpen ? 'page' : undefined"
        class="group flex h-full flex-col items-center justify-center gap-1 outline-none"
        @click="emit('navigatePrimary', 'items')"
      >
        <span
          class="md-state flex h-8 w-16 items-center justify-center rounded-full transition-colors duration-[var(--md-sys-motion-duration-short)] group-focus-visible:outline-3 group-focus-visible:outline-offset-2 group-focus-visible:outline-[var(--md-sys-color-secondary)]"
          :class="
            activeNav === 'items' && !tasksDrawerOpen
              ? 'bg-[var(--md-sys-color-secondary-container)] text-[var(--md-sys-color-on-secondary-container)]'
              : 'text-[var(--md-sys-color-on-surface-variant)] group-hover:text-[var(--md-sys-color-on-surface)]'
          "
        >
          <LuLibrary class="h-5 w-5 shrink-0" />
        </span>
        <span
          class="md-label-medium truncate"
          :class="
            activeNav === 'items' && !tasksDrawerOpen
              ? 'font-semibold text-[var(--md-sys-color-on-surface)]'
              : 'text-[var(--md-sys-color-on-surface-variant)]'
          "
        >
          {{ txt.nav.items }}
        </span>
      </button>

      <button
        type="button"
        :aria-pressed="tasksDrawerOpen"
        class="group flex h-full flex-col items-center justify-center gap-1 outline-none"
        @click="emit('toggleTasks')"
      >
        <span
          class="md-state relative flex h-8 w-16 items-center justify-center rounded-full transition-colors duration-[var(--md-sys-motion-duration-short)] group-focus-visible:outline-3 group-focus-visible:outline-offset-2 group-focus-visible:outline-[var(--md-sys-color-secondary)]"
          :class="
            tasksDrawerOpen
              ? 'bg-[var(--md-sys-color-secondary-container)] text-[var(--md-sys-color-on-secondary-container)]'
              : 'text-[var(--md-sys-color-on-surface-variant)] group-hover:text-[var(--md-sys-color-on-surface)]'
          "
        >
          <LuLoader2 v-if="hasActiveTask" class="h-5 w-5 shrink-0 animate-spin" />
          <LuClock v-else class="h-5 w-5 shrink-0" />
          <MdBadge
            v-if="unresolvedCount > 0"
            :value="unresolvedCount"
            :max="99"
            tone="error"
            class="pointer-events-none absolute top-0.5 right-3"
          />
        </span>
        <span
          class="md-label-medium truncate"
          :class="
            tasksDrawerOpen
              ? 'font-semibold text-[var(--md-sys-color-on-surface)]'
              : 'text-[var(--md-sys-color-on-surface-variant)]'
          "
        >
          {{ txt.tasks.title }}
        </span>
      </button>

      <button
        type="button"
        :aria-current="activeNav === 'settings' && !tasksDrawerOpen ? 'page' : undefined"
        class="group flex h-full flex-col items-center justify-center gap-1 outline-none"
        @click="emit('toggleSettings')"
      >
        <span
          class="md-state flex h-8 w-16 items-center justify-center rounded-full transition-colors duration-[var(--md-sys-motion-duration-short)] group-focus-visible:outline-3 group-focus-visible:outline-offset-2 group-focus-visible:outline-[var(--md-sys-color-secondary)]"
          :class="
            activeNav === 'settings' && !tasksDrawerOpen
              ? 'bg-[var(--md-sys-color-secondary-container)] text-[var(--md-sys-color-on-secondary-container)]'
              : 'text-[var(--md-sys-color-on-surface-variant)] group-hover:text-[var(--md-sys-color-on-surface)]'
          "
        >
          <LuSettings class="h-5 w-5 shrink-0" />
        </span>
        <span
          class="md-label-medium truncate"
          :class="
            activeNav === 'settings' && !tasksDrawerOpen
              ? 'font-semibold text-[var(--md-sys-color-on-surface)]'
              : 'text-[var(--md-sys-color-on-surface-variant)]'
          "
        >
          设置
        </span>
      </button>
    </div>
  </nav>
</template>

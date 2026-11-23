<script setup lang="ts">
import { useI18n } from 'vue-i18n'
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

const { t } = useI18n()
const txt = useToolText()
</script>

<template>
  <nav
    data-mobile-bottom-nav
    :aria-label="t('nav.navigation')"
    class="safe-pb safe-px relative z-30 shrink-0 bg-surface-container select-none sm:hidden"
  >
    <div class="grid h-20 grid-cols-4 items-center">
      <button
        type="button"
        :aria-current="activeNav === 'home' && !tasksDrawerOpen ? 'page' : undefined"
        class="group flex h-full cursor-pointer flex-col items-center justify-center gap-1 outline-none"
        @click="emit('navigatePrimary', 'home')"
      >
        <span
          class="md-state md-state-target flex h-8 w-16 items-center justify-center rounded-corner-full transition-colors duration-150 ease-standard group-focus-visible:outline-3 group-focus-visible:outline-offset-2 group-focus-visible:outline-secondary"
          :class="
            activeNav === 'home' && !tasksDrawerOpen
              ? 'bg-secondary-container text-on-secondary-container'
              : 'text-on-surface-variant group-hover:text-on-surface'
          "
        >
          <LuHome class="h-5 w-5 shrink-0" aria-hidden="true" />
        </span>
        <span
          class="text-label-medium truncate"
          :class="
            activeNav === 'home' && !tasksDrawerOpen
              ? 'text-on-surface'
              : 'text-on-surface-variant'
          "
        >
          {{ txt.nav.home }}
        </span>
      </button>

      <button
        type="button"
        :aria-current="activeNav === 'items' && !tasksDrawerOpen ? 'page' : undefined"
        class="group flex h-full cursor-pointer flex-col items-center justify-center gap-1 outline-none"
        @click="emit('navigatePrimary', 'items')"
      >
        <span
          class="md-state md-state-target flex h-8 w-16 items-center justify-center rounded-corner-full transition-colors duration-150 ease-standard group-focus-visible:outline-3 group-focus-visible:outline-offset-2 group-focus-visible:outline-secondary"
          :class="
            activeNav === 'items' && !tasksDrawerOpen
              ? 'bg-secondary-container text-on-secondary-container'
              : 'text-on-surface-variant group-hover:text-on-surface'
          "
        >
          <LuLibrary class="h-5 w-5 shrink-0" aria-hidden="true" />
        </span>
        <span
          class="text-label-medium truncate"
          :class="
            activeNav === 'items' && !tasksDrawerOpen
              ? 'text-on-surface'
              : 'text-on-surface-variant'
          "
        >
          {{ txt.nav.items }}
        </span>
      </button>

      <button
        type="button"
        :aria-pressed="tasksDrawerOpen"
        class="group flex h-full cursor-pointer flex-col items-center justify-center gap-1 outline-none"
        @click="emit('toggleTasks')"
      >
        <span
          class="md-state md-state-target relative flex h-8 w-16 items-center justify-center rounded-corner-full transition-colors duration-150 ease-standard group-focus-visible:outline-3 group-focus-visible:outline-offset-2 group-focus-visible:outline-secondary"
          :class="
            tasksDrawerOpen
              ? 'bg-secondary-container text-on-secondary-container'
              : 'text-on-surface-variant group-hover:text-on-surface'
          "
        >
          <LuLoader2 v-if="hasActiveTask" class="h-5 w-5 shrink-0 animate-spin" aria-hidden="true" />
          <LuClock v-else class="h-5 w-5 shrink-0" aria-hidden="true" />
          <MdBadge
            v-if="unresolvedCount > 0"
            :value="unresolvedCount"
            :max="99"
            tone="error"
            class="pointer-events-none absolute top-0.5 right-3"
          />
        </span>
        <span
          class="text-label-medium truncate"
          :class="tasksDrawerOpen ? 'text-on-surface' : 'text-on-surface-variant'"
        >
          {{ txt.tasks.title }}
        </span>
      </button>

      <button
        type="button"
        :aria-current="activeNav === 'settings' && !tasksDrawerOpen ? 'page' : undefined"
        class="group flex h-full cursor-pointer flex-col items-center justify-center gap-1 outline-none"
        @click="emit('toggleSettings')"
      >
        <span
          class="md-state md-state-target flex h-8 w-16 items-center justify-center rounded-corner-full transition-colors duration-150 ease-standard group-focus-visible:outline-3 group-focus-visible:outline-offset-2 group-focus-visible:outline-secondary"
          :class="
            activeNav === 'settings' && !tasksDrawerOpen
              ? 'bg-secondary-container text-on-secondary-container'
              : 'text-on-surface-variant group-hover:text-on-surface'
          "
        >
          <LuSettings class="h-5 w-5 shrink-0" aria-hidden="true" />
        </span>
        <span
          class="text-label-medium truncate"
          :class="
            activeNav === 'settings' && !tasksDrawerOpen
              ? 'text-on-surface'
              : 'text-on-surface-variant'
          "
        >
          {{ t('nav.settings') }}
        </span>
      </button>
    </div>
  </nav>
</template>

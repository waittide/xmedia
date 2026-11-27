<script setup lang="ts">
import type { Component } from 'vue'
import type { RouteLocationRaw } from 'vue-router'
import { useI18n } from 'vue-i18n'
import { useThemeStore } from '@/stores/theme'

export interface MobileDomainSubTab {
  key: 'home' | 'library' | 'history' | 'settings'
  label: string
  icon: Component
  active: boolean
  to: RouteLocationRaw
}

withDefaults(
  defineProps<{
    tabs: readonly MobileDomainSubTab[]
    bindDataNav?: boolean
  }>(),
  {
    bindDataNav: false,
  },
)

const emit = defineEmits<{
  navigate: [tab: MobileDomainSubTab]
}>()

const { t } = useI18n()
const themeStore = useThemeStore()
</script>

<template>
  <nav
    v-if="tabs.length > 0"
    data-mobile-bottom-nav
    :data-compact="themeStore.showNavLabels ? '0' : '1'"
    :aria-label="t('nav.navigation')"
    class="safe-pb safe-px relative z-20 shrink-0 bg-surface-container select-none sm:hidden"
  >
    <div
      class="grid items-center"
      :class="themeStore.showNavLabels ? 'h-16' : 'h-13'"
      :style="{ gridTemplateColumns: `repeat(${tabs.length}, minmax(0, 1fr))` }"
    >
      <button
        v-for="tab in tabs"
        :key="`bar-${tab.key}`"
        type="button"
        :data-nav="bindDataNav ? tab.key : undefined"
        :aria-current="tab.active ? 'page' : undefined"
        :aria-label="tab.label"
        :title="tab.label"
        class="group flex h-full cursor-pointer flex-col items-center justify-center gap-0.5 outline-none"
        @click="emit('navigate', tab)"
      >
        <span
          class="md-state md-state-target flex items-center justify-center rounded-corner-full transition-colors duration-150 ease-standard group-focus-visible:outline-3 group-focus-visible:outline-offset-2 group-focus-visible:outline-secondary"
          :class="[
            themeStore.showNavLabels ? 'h-8 w-14' : 'h-9 w-14',
            tab.active
              ? 'bg-secondary-container text-on-secondary-container'
              : 'text-on-surface-variant group-hover:text-on-surface',
          ]"
        >
          <component
            :is="tab.icon"
            class="h-5 w-5 shrink-0"
            aria-hidden="true"
          />
        </span>
        <span
          v-if="themeStore.showNavLabels"
          class="text-label-medium max-w-full truncate px-1"
          :class="
            tab.active
              ? 'font-semibold text-on-surface'
              : 'text-on-surface-variant group-hover:text-on-surface'
          "
        >
          {{ tab.label }}
        </span>
      </button>
    </div>
  </nav>
</template>

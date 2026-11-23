<script setup lang="ts">
import type { Component } from 'vue'
import { RouterLink } from 'vue-router'
import { useI18n } from 'vue-i18n'
import { LuLayers } from 'vue-icons-plus/lu'
import type { MediaKind } from '@/types/api'

export interface DomainNavTab {
  kind: MediaKind
  label: string
  icon: Component
}

defineProps<{
  domainTabs: DomainNavTab[]
  activeDomain: MediaKind
  isSettingsRoute: boolean
}>()

const emit = defineEmits<{
  switchDomain: [kind: MediaKind]
}>()

const { t } = useI18n()
</script>

<template>
  <nav
    :aria-label="t('nav.navigation')"
    class="safe-py safe-pl hidden w-20 shrink-0 flex-col items-center bg-surface-container py-3 select-none sm:flex"
  >
    <div class="flex w-full flex-col items-center gap-3">
      <!-- Subtle brand mark -->
      <RouterLink
        :to="'/' + activeDomain"
        :aria-label="t('app.name')"
        class="md-state md-touch-target mb-1 flex h-10 w-10 items-center justify-center rounded-corner-md bg-primary-container text-on-primary-container"
      >
        <LuLayers class="h-5 w-5" aria-hidden="true" />
      </RouterLink>

      <!-- Top-level 4-domain destinations -->
      <button
        v-for="tab in domainTabs"
        :key="tab.kind"
        type="button"
        :data-domain-switch="tab.kind"
        :aria-current="!isSettingsRoute && activeDomain === tab.kind ? 'page' : undefined"
        class="group flex w-full cursor-pointer flex-col items-center gap-1 py-1 outline-none"
        @click="emit('switchDomain', tab.kind)"
      >
        <span
          class="md-state md-state-target flex h-8 w-14 items-center justify-center rounded-corner-full transition-colors duration-150 ease-standard group-focus-visible:outline-3 group-focus-visible:outline-offset-2 group-focus-visible:outline-secondary"
          :class="
            !isSettingsRoute && activeDomain === tab.kind
              ? 'bg-secondary-container text-on-secondary-container'
              : 'text-on-surface-variant group-hover:text-on-surface'
          "
        >
          <component :is="tab.icon" class="h-5 w-5 shrink-0" aria-hidden="true" />
        </span>
        <span
          class="text-label-medium max-w-[72px] truncate"
          :class="
            !isSettingsRoute && activeDomain === tab.kind
              ? 'text-on-surface'
              : 'text-on-surface-variant'
          "
        >
          {{ tab.label }}
        </span>
      </button>
    </div>
  </nav>
</template>

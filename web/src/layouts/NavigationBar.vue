<script setup lang="ts">
import { useI18n } from 'vue-i18n'
import type { MediaKind } from '@/types/api'
import type { DomainNavTab } from './NavigationRail.vue'

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
    v-if="domainTabs.length > 1"
    data-mobile-bottom-nav
    :aria-label="t('nav.navigation')"
    class="safe-pb safe-px relative z-20 shrink-0 bg-surface-container select-none sm:hidden"
  >
    <div
      class="grid h-20 items-center"
      :style="{ gridTemplateColumns: `repeat(${domainTabs.length}, minmax(0, 1fr))` }"
    >
      <button
        v-for="tab in domainTabs"
        :key="`bar-${tab.kind}`"
        type="button"
        :data-domain-switch="tab.kind"
        :aria-current="!isSettingsRoute && activeDomain === tab.kind ? 'page' : undefined"
        class="group flex h-full cursor-pointer flex-col items-center justify-center gap-1 outline-none"
        @click="emit('switchDomain', tab.kind)"
      >
        <span
          class="md-state md-state-target flex h-8 w-16 items-center justify-center rounded-corner-full transition-colors duration-150 ease-standard group-focus-visible:outline-3 group-focus-visible:outline-offset-2 group-focus-visible:outline-secondary"
          :class="
            !isSettingsRoute && activeDomain === tab.kind
              ? 'bg-secondary-container text-on-secondary-container'
              : 'text-on-surface-variant group-hover:text-on-surface'
          "
        >
          <component :is="tab.icon" class="h-5 w-5 shrink-0" aria-hidden="true" />
        </span>
        <span
          class="text-label-medium max-w-full truncate px-1"
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

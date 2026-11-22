<script setup lang="ts">
import { computed, type Component } from 'vue'
import { useI18n } from 'vue-i18n'
import RefreshButton from '@/components/ui/RefreshButton.vue'
import type { MediaKind } from '@/types/api'

export interface StatCardItem {
  key: string
  label?: string
  value: number
  icon: Component
  tone?: 'accent' | 'rose'
  onClick: () => void
}

const props = defineProps<{
  domain: MediaKind
  cards: StatCardItem[]
  onRefresh: () => Promise<void>
}>()

const { t } = useI18n()

const hasNonZeroStat = computed(() => props.cards.some((card) => card.value > 0))

const cardsGridClass = computed(() => {
  const n = props.cards.length
  if (n <= 2) return 'grid-cols-1 sm:grid-cols-2'
  if (n === 3) return 'grid-cols-1 sm:grid-cols-3'
  return 'grid-cols-2 lg:grid-cols-4'
})
</script>

<template>
  <section v-if="cards.length > 0 && hasNonZeroStat" class="space-y-2.5">
    <div class="flex items-center justify-end">
      <RefreshButton variant="ghost" :action="onRefresh" />
    </div>

    <div class="grid gap-3.5" :class="cardsGridClass">
      <button
        v-for="card in cards"
        :key="card.key"
        type="button"
        class="md-card-filled md-state flex items-center gap-3.5 p-4 text-left transition hover:-translate-y-0.5 hover:border-[var(--md-sys-color-outline)]/40 hover:shadow-[var(--md-sys-elevation-1)]"
        @click="card.onClick()"
      >
        <div
          class="flex h-10 w-10 shrink-0 items-center justify-center rounded-xl"
          :class="
            card.tone === 'rose'
              ? 'bg-[var(--md-sys-color-error-container)]/85 text-[var(--md-sys-color-on-error-container)]'
              : 'bg-[var(--md-sys-color-secondary-container)] text-[var(--md-sys-color-on-secondary-container)]'
          "
        >
          <component :is="card.icon" class="h-[18px] w-[18px]" />
        </div>
        <div class="min-w-0">
          <p class="text-xl leading-none font-semibold tabular-nums text-[var(--md-sys-color-on-surface)]">
            {{ card.value }}
          </p>
          <p class="mt-1.5 truncate text-xs text-[var(--md-sys-color-on-surface-variant)]">
            {{ card.label ?? t(`layoutCustomizer.cards.${domain}.${card.key}`) }}
          </p>
        </div>
      </button>
    </div>
  </section>
</template>

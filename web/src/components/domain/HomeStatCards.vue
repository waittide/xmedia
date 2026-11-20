<script setup lang="ts">
import { computed, ref, type Component } from 'vue'
import { useI18n } from 'vue-i18n'
import { LuSlidersHorizontal } from 'vue-icons-plus/lu'
import LayoutCustomizerModal from '@/components/LayoutCustomizerModal.vue'
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
const customizerOpen = ref(false)

const cardsGridClass = computed(() => {
  const n = props.cards.length
  if (n <= 2) return 'grid-cols-1 sm:grid-cols-2'
  if (n === 3) return 'grid-cols-1 sm:grid-cols-3'
  return 'grid-cols-2 lg:grid-cols-4'
})
</script>

<template>
  <section class="space-y-2.5">
    <div class="flex items-center justify-end gap-1.5">
      <RefreshButton variant="ghost" :action="onRefresh" />
      <button
        type="button"
        class="md-btn md-btn-sm md-btn-text md-state !h-7 !px-2.5 !text-[11px] !text-[var(--md-sys-color-on-surface-variant)] hover:!text-[var(--md-sys-color-on-surface)]"
        @click="customizerOpen = true"
      >
        <LuSlidersHorizontal class="h-3.5 w-3.5" />
        <span>{{ t('layoutCustomizer.customizeCards') }}</span>
      </button>
    </div>

    <div class="grid gap-4" :class="cardsGridClass">
      <button
        v-for="card in cards"
        :key="card.key"
        type="button"
        class="md-card-filled md-state flex items-center gap-3.5 border border-transparent p-4 text-left transition hover:border-[var(--md-sys-color-primary)]/45 hover:shadow-[var(--md-sys-elevation-1)]"
        @click="card.onClick()"
      >
        <div
          class="flex h-10 w-10 shrink-0 items-center justify-center rounded-xl"
          :class="
            card.tone === 'rose'
              ? 'bg-[var(--md-sys-color-error-container)] text-[var(--md-sys-color-on-error-container)]'
              : 'bg-[var(--md-sys-color-primary-container)] text-[var(--md-sys-color-on-primary-container)]'
          "
        >
          <component :is="card.icon" class="h-5 w-5" />
        </div>
        <div class="min-w-0">
          <p class="text-xl leading-none font-bold text-[var(--md-sys-color-on-surface)]">
            {{ card.value }}
          </p>
          <p class="mt-1 truncate text-xs text-[var(--md-sys-color-on-surface-variant)]">
            {{ card.label ?? t(`layoutCustomizer.cards.${domain}.${card.key}`) }}
          </p>
        </div>
      </button>
    </div>

    <LayoutCustomizerModal
      :open="customizerOpen"
      :domain="domain"
      initial-tab="cards"
      @close="customizerOpen = false"
    />
  </section>
</template>

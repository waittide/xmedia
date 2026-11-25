<script setup lang="ts" generic="T extends string | number">
import { computed, ref, useId, type Component } from 'vue'

export interface MdTabItem<V extends string | number = string> {
  value: V
  label: string
  icon?: Component
  badge?: string | number
  disabled?: boolean
}

const props = withDefaults(
  defineProps<{
    modelValue: T
    tabs: MdTabItem<T>[]
    variant?: 'primary' | 'secondary'
    ariaLabel?: string
    scrollable?: boolean
  }>(),
  {
    variant: 'primary',
    ariaLabel: undefined,
    scrollable: true,
  },
)

const emit = defineEmits<{
  'update:modelValue': [value: T]
  change: [value: T]
}>()

const tablistId = useId()
const tabRefs = ref<(HTMLButtonElement | null)[]>([])

const isStackedPrimary = computed(
  () => props.variant === 'primary' && props.tabs.some((tab) => Boolean(tab.icon)),
)

function select(tab: MdTabItem<T>): void {
  if (tab.disabled || tab.value === props.modelValue) return
  emit('update:modelValue', tab.value)
  emit('change', tab.value)
}

function setTabRef(el: unknown, index: number): void {
  tabRefs.value[index] = el instanceof HTMLButtonElement ? el : null
}

function onKeydown(ev: KeyboardEvent, currentIndex: number): void {
  const enabledIndices = props.tabs
    .map((tab, idx) => (tab.disabled ? -1 : idx))
    .filter((idx) => idx >= 0)
  if (enabledIndices.length === 0) return

  const pos = enabledIndices.indexOf(currentIndex)
  let nextIdx: number | null = null
  if (ev.key === 'ArrowRight') {
    nextIdx = enabledIndices[(pos + 1) % enabledIndices.length]!
  } else if (ev.key === 'ArrowLeft') {
    nextIdx = enabledIndices[(pos - 1 + enabledIndices.length) % enabledIndices.length]!
  } else if (ev.key === 'Home') {
    nextIdx = enabledIndices[0]!
  } else if (ev.key === 'End') {
    nextIdx = enabledIndices[enabledIndices.length - 1]!
  }

  if (nextIdx !== null) {
    ev.preventDefault()
    const targetTab = props.tabs[nextIdx]
    if (targetTab) {
      select(targetTab)
      tabRefs.value[nextIdx]?.focus()
    }
  }
}
</script>

<template>
  <div
    role="tablist"
    :aria-label="ariaLabel"
    class="relative flex items-end border-b border-outline-variant"
    :class="scrollable ? 'no-scrollbar overflow-x-auto' : ''"
  >
    <button
      v-for="(tab, idx) in tabs"
      :id="`${tablistId}-tab-${idx}`"
      :key="String(tab.value)"
      :ref="(el) => setTabRef(el, idx)"
      type="button"
      role="tab"
      :aria-selected="modelValue === tab.value"
      :tabindex="modelValue === tab.value ? 0 : -1"
      :disabled="tab.disabled"
      class="md-state text-title-small relative inline-flex cursor-pointer items-center justify-center px-4 transition-colors duration-150 ease-standard outline-none select-none focus-visible:outline-3 focus-visible:-outline-offset-3 focus-visible:outline-secondary disabled:cursor-not-allowed disabled:opacity-38"
      :class="[
        isStackedPrimary ? 'h-16' : 'h-12',
        scrollable ? 'shrink-0' : 'flex-1',
        modelValue === tab.value
          ? variant === 'primary'
            ? 'text-primary'
            : 'text-on-surface'
          : 'text-on-surface-variant hover:text-on-surface',
      ]"
      @click="select(tab)"
      @keydown="onKeydown($event, idx)"
    >
      <!-- MD3 Primary Tabs with Icons: 64dp vertically stacked 24dp icon + label + content-width 3dp rounded-top indicator -->
      <span
        v-if="isStackedPrimary"
        class="relative inline-flex h-full flex-col items-center justify-center gap-1"
      >
        <span v-if="tab.icon" class="relative inline-flex items-center justify-center">
          <component :is="tab.icon" class="h-6 w-6 shrink-0" aria-hidden="true" />
          <span
            v-if="tab.badge !== undefined"
            class="text-label-small absolute -top-1 -right-3.5 rounded-corner-full bg-error px-1 text-on-error tabular-nums"
          >
            {{ tab.badge }}
          </span>
        </span>
        <span class="inline-flex items-center gap-1.5 whitespace-nowrap">
          <span>{{ tab.label }}</span>
          <span
            v-if="!tab.icon && tab.badge !== undefined"
            class="text-label-small rounded-corner-full bg-surface-container-highest px-1.5 py-0.5 tabular-nums text-on-surface-variant"
          >
            {{ tab.badge }}
          </span>
        </span>
        <span
          v-if="modelValue === tab.value"
          aria-hidden="true"
          class="pointer-events-none absolute right-0 bottom-0 left-0 mx-auto h-[3px] min-w-6 rounded-t-[3px] bg-primary"
        />
      </span>

      <!-- MD3 Label-only Primary Tabs OR Secondary Tabs: 48dp horizontal inline -->
      <span
        v-else
        class="relative inline-flex h-full items-center justify-center gap-2"
      >
        <component
          :is="tab.icon"
          v-if="tab.icon"
          class="h-[18px] w-[18px] shrink-0"
          aria-hidden="true"
        />
        <span class="whitespace-nowrap">{{ tab.label }}</span>
        <span
          v-if="tab.badge !== undefined"
          class="text-label-small rounded-corner-full bg-surface-container-highest px-1.5 py-0.5 tabular-nums text-on-surface-variant"
        >
          {{ tab.badge }}
        </span>
        <span
          v-if="modelValue === tab.value && variant === 'primary'"
          aria-hidden="true"
          class="pointer-events-none absolute right-0 bottom-0 left-0 mx-auto h-[3px] min-w-6 rounded-t-[3px] bg-primary"
        />
      </span>

      <!-- MD3 Secondary Tabs full-tab-width 2dp bottom indicator -->
      <span
        v-if="modelValue === tab.value && variant === 'secondary'"
        aria-hidden="true"
        class="pointer-events-none absolute right-0 bottom-0 left-0 h-[2px] bg-primary"
      />
    </button>
  </div>
</template>

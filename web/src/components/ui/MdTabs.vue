<script setup lang="ts" generic="T extends string | number">
import { ref, useId, type Component } from 'vue'

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
    class="relative flex items-end border-b border-[var(--md-sys-color-outline-variant)]"
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
      class="md-state relative inline-flex h-12 shrink-0 cursor-pointer items-center justify-center gap-2 px-4 md-title-small transition-colors duration-[var(--md-sys-motion-duration-short)] outline-none select-none focus-visible:outline-3 focus-visible:outline-offset-[-3px] focus-visible:outline-[var(--md-sys-color-secondary)] disabled:cursor-not-allowed disabled:opacity-38"
      :class="[
        modelValue === tab.value
          ? variant === 'primary'
            ? 'text-[var(--md-sys-color-primary)]'
            : 'text-[var(--md-sys-color-on-surface)]'
          : 'text-[var(--md-sys-color-on-surface-variant)] hover:text-[var(--md-sys-color-on-surface)]',
      ]"
      @click="select(tab)"
      @keydown="onKeydown($event, idx)"
    >
      <component :is="tab.icon" v-if="tab.icon" class="h-[18px] w-[18px] shrink-0" />
      <span class="whitespace-nowrap">{{ tab.label }}</span>
      <span
        v-if="tab.badge !== undefined"
        class="md-label-small rounded-full bg-[var(--md-sys-color-surface-container-highest)] px-1.5 py-0.5 tabular-nums text-[var(--md-sys-color-on-surface-variant)]"
      >
        {{ tab.badge }}
      </span>
      <span
        v-if="modelValue === tab.value"
        aria-hidden="true"
        class="pointer-events-none absolute right-3 bottom-0 left-3 bg-[var(--md-sys-color-primary)]"
        :class="variant === 'primary' ? 'h-[3px] rounded-t-[3px]' : 'right-0 left-0 h-[2px]'"
      />
    </button>
  </div>
</template>

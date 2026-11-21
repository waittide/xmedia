<script setup lang="ts">
import { computed } from 'vue'

const props = withDefaults(
  defineProps<{
    value?: number | string | null
    max?: number
    dot?: boolean
    tone?: 'error' | 'primary' | 'warning'
    ariaLabel?: string
  }>(),
  {
    value: null,
    max: 999,
    dot: false,
    tone: 'error',
    ariaLabel: undefined,
  },
)

const isDot = computed(() => props.dot || props.value === null || props.value === undefined || props.value === '')

const displayText = computed(() => {
  if (isDot.value) return ''
  if (typeof props.value === 'number' && props.value > props.max) {
    return `${props.max}+`
  }
  return String(props.value)
})

const toneClass = computed(() => {
  if (props.tone === 'primary') {
    return 'bg-[var(--md-sys-color-primary)] text-[var(--md-sys-color-on-primary)]'
  }
  if (props.tone === 'warning') {
    return 'bg-[var(--md-sys-color-warning)] text-[var(--md-sys-color-on-primary)]'
  }
  return 'bg-[var(--md-sys-color-error)] text-[var(--md-sys-color-on-error)]'
})
</script>

<template>
  <span
    v-if="isDot"
    :aria-label="ariaLabel"
    class="inline-block h-1.5 w-1.5 shrink-0 rounded-full"
    :class="toneClass"
  />
  <span
    v-else
    :aria-label="ariaLabel"
    class="md-label-small inline-flex h-4 min-w-4 shrink-0 items-center justify-center rounded-full px-1 leading-none tabular-nums"
    :class="toneClass"
  >
    {{ displayText }}
  </span>
</template>

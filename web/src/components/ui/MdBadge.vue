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
    return 'bg-primary text-on-primary'
  }
  if (props.tone === 'warning') {
    return 'bg-warning text-on-warning'
  }
  return 'bg-error text-on-error'
})
</script>

<template>
  <span
    v-if="isDot"
    :role="ariaLabel ? 'status' : undefined"
    :aria-label="ariaLabel"
    :aria-hidden="ariaLabel ? undefined : 'true'"
    class="inline-block h-1.5 w-1.5 shrink-0 rounded-corner-full"
    :class="toneClass"
  />
  <span
    v-else
    :aria-label="ariaLabel"
    class="text-label-small inline-flex h-4 min-w-4 shrink-0 items-center justify-center rounded-corner-full px-1 leading-none tabular-nums"
    :class="toneClass"
  >
    {{ displayText }}
  </span>
</template>

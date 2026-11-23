<script setup lang="ts">
import { computed } from 'vue'

const props = withDefaults(
  defineProps<{
    value?: number | null
    max?: number
    size?: number
    strokeWidth?: number
    indeterminate?: boolean
    ariaLabel?: string
  }>(),
  {
    value: null,
    max: 100,
    size: 40,
    strokeWidth: 4,
    indeterminate: false,
    ariaLabel: undefined,
  },
)

const isIndeterminate = computed(
  () => props.indeterminate || props.value === null || props.value === undefined,
)

const radius = computed(() => Math.max(1, (props.size - props.strokeWidth) / 2))
const circumference = computed(() => 2 * Math.PI * radius.value)
const clampedRatio = computed(() => {
  if (isIndeterminate.value || props.max <= 0) return 0
  return Math.max(0, Math.min(1, Number(props.value) / props.max))
})
const dashOffset = computed(() => circumference.value * (1 - clampedRatio.value))
</script>

<template>
  <svg
    role="progressbar"
    :aria-label="ariaLabel"
    :aria-valuemin="isIndeterminate ? undefined : 0"
    :aria-valuemax="isIndeterminate ? undefined : max"
    :aria-valuenow="isIndeterminate ? undefined : Math.round(Number(value ?? 0))"
    :width="size"
    :height="size"
    :viewBox="`0 0 ${size} ${size}`"
    class="shrink-0"
    :class="isIndeterminate ? 'animate-spin motion-reduce:animate-pulse' : ''"
  >
    <circle
      :cx="size / 2"
      :cy="size / 2"
      :r="radius"
      fill="none"
      stroke="var(--md-sys-color-secondary-container)"
      :stroke-width="strokeWidth"
    />
    <circle
      v-if="!isIndeterminate"
      :cx="size / 2"
      :cy="size / 2"
      :r="radius"
      fill="none"
      stroke="var(--md-sys-color-primary)"
      :stroke-width="strokeWidth"
      stroke-linecap="round"
      :transform="`rotate(-90 ${size / 2} ${size / 2})`"
      :stroke-dasharray="circumference"
      :stroke-dashoffset="dashOffset"
      class="transition-[stroke-dashoffset] duration-300 ease-standard"
    />
    <circle
      v-else
      :cx="size / 2"
      :cy="size / 2"
      :r="radius"
      fill="none"
      stroke="var(--md-sys-color-primary)"
      :stroke-width="strokeWidth"
      stroke-linecap="round"
      :stroke-dasharray="`${circumference * 0.28} ${circumference * 0.72}`"
    />
  </svg>
</template>

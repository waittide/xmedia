<script setup lang="ts">
import { computed } from 'vue'

const props = withDefaults(
  defineProps<{
    value?: number | null
    max?: number
    indeterminate?: boolean
    tone?: 'primary' | 'success' | 'error' | 'neutral'
    ariaLabel?: string
  }>(),
  {
    value: null,
    max: 100,
    indeterminate: false,
    tone: 'primary',
    ariaLabel: undefined,
  },
)

const isIndeterminate = computed(
  () => props.indeterminate || props.value === null || props.value === undefined,
)

const clampedPercent = computed(() => {
  if (isIndeterminate.value || props.max <= 0) return 0
  return Math.max(0, Math.min(100, (Number(props.value) / props.max) * 100))
})

const indicatorClass = computed(() => {
  if (props.tone === 'success') return 'bg-[var(--md-sys-color-success)]'
  if (props.tone === 'error') return 'bg-[var(--md-sys-color-error)]'
  if (props.tone === 'neutral') return 'bg-[var(--md-sys-color-outline)]'
  return 'bg-[var(--md-sys-color-primary)]'
})
</script>

<template>
  <div
    role="progressbar"
    :aria-label="ariaLabel"
    :aria-valuemin="isIndeterminate ? undefined : 0"
    :aria-valuemax="isIndeterminate ? undefined : max"
    :aria-valuenow="isIndeterminate ? undefined : Math.round(Number(value ?? 0))"
    class="relative h-1 w-full overflow-hidden rounded-full bg-[var(--md-sys-color-secondary-container)]"
  >
    <div
      v-if="!isIndeterminate"
      class="h-full rounded-full transition-[width] duration-[var(--md-sys-motion-duration-medium)] ease-[var(--md-sys-motion-easing-standard)]"
      :class="indicatorClass"
      :style="{ width: `${clampedPercent}%` }"
    />
    <div
      v-else
      class="md-linear-indeterminate h-full w-2/5 rounded-full"
      :class="indicatorClass"
    />
    <span
      v-if="!isIndeterminate && clampedPercent < 98"
      aria-hidden="true"
      class="pointer-events-none absolute top-0 right-0 h-1 w-1 rounded-full"
      :class="indicatorClass"
    />
  </div>
</template>

<style scoped>
@keyframes md-linear-indeterminate-slide {
  0% {
    transform: translateX(-110%);
  }
  100% {
    transform: translateX(260%);
  }
}

.md-linear-indeterminate {
  animation: md-linear-indeterminate-slide 1.25s var(--md-sys-motion-easing-standard) infinite;
}

@media (prefers-reduced-motion: reduce) {
  .md-linear-indeterminate {
    animation: none;
    width: 100%;
    opacity: 0.6;
  }
}
</style>

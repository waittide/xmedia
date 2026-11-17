<script setup lang="ts">
import { computed, ref, watch } from 'vue'

const props = withDefaults(
  defineProps<{
    modelValue: number
    min?: number
    max?: number
    step?: number
    buffered?: number
    commitOnRelease?: boolean
    disabled?: boolean
    ariaLabel?: string
  }>(),
  {
    min: 0,
    max: 100,
    step: 1,
    buffered: 0,
    commitOnRelease: false,
    disabled: false,
    ariaLabel: 'slider',
  },
)

const emit = defineEmits<{
  'update:modelValue': [value: number]
  change: [value: number]
}>()

const trackRef = ref<HTMLDivElement | null>(null)
const dragging = ref(false)
const draftValue = ref(props.modelValue)

watch(
  () => props.modelValue,
  (val) => {
    if (!dragging.value) {
      draftValue.value = val
    }
  },
)

const effectiveValue = computed(() => (dragging.value ? draftValue.value : props.modelValue))

const fillPercent = computed(() => {
  const span = props.max - props.min
  if (span <= 0) return 0
  const clamped = Math.max(props.min, Math.min(props.max, effectiveValue.value))
  return ((clamped - props.min) / span) * 100
})

const bufferedPercent = computed(() => Math.max(0, Math.min(100, props.buffered)))

function computeValueFromClientX(clientX: number): number {
  const el = trackRef.value
  if (!el) return props.modelValue
  const rect = el.getBoundingClientRect()
  if (rect.width <= 0) return props.modelValue
  const ratio = Math.max(0, Math.min(1, (clientX - rect.left) / rect.width))
  const raw = props.min + ratio * (props.max - props.min)
  if (props.step > 0) {
    const stepped = Math.round((raw - props.min) / props.step) * props.step + props.min
    return Math.max(props.min, Math.min(props.max, Number(stepped.toFixed(4))))
  }
  return Math.max(props.min, Math.min(props.max, raw))
}

function onPointerDown(ev: PointerEvent): void {
  if (props.disabled) return
  ev.preventDefault()
  const el = trackRef.value
  if (!el) return
  el.setPointerCapture(ev.pointerId)
  dragging.value = true
  const next = computeValueFromClientX(ev.clientX)
  draftValue.value = next
  if (!props.commitOnRelease) {
    emit('update:modelValue', next)
  }
}

function onPointerMove(ev: PointerEvent): void {
  if (!dragging.value || props.disabled) return
  const next = computeValueFromClientX(ev.clientX)
  draftValue.value = next
  if (!props.commitOnRelease) {
    emit('update:modelValue', next)
  }
}

function onPointerUp(ev: PointerEvent): void {
  if (!dragging.value) return
  const el = trackRef.value
  if (el && el.hasPointerCapture(ev.pointerId)) {
    el.releasePointerCapture(ev.pointerId)
  }
  dragging.value = false
  emit('update:modelValue', draftValue.value)
  emit('change', draftValue.value)
}

function onKeydown(ev: KeyboardEvent): void {
  if (props.disabled) return
  const delta = props.step > 0 ? props.step : (props.max - props.min) / 50
  let next: number | null = null
  if (ev.key === 'ArrowRight' || ev.key === 'ArrowUp') {
    next = Math.min(props.max, props.modelValue + delta)
  } else if (ev.key === 'ArrowLeft' || ev.key === 'ArrowDown') {
    next = Math.max(props.min, props.modelValue - delta)
  } else if (ev.key === 'Home') {
    next = props.min
  } else if (ev.key === 'End') {
    next = props.max
  }
  if (next !== null) {
    ev.preventDefault()
    emit('update:modelValue', next)
    emit('change', next)
  }
}
</script>

<template>
  <div
    ref="trackRef"
    role="slider"
    tabindex="0"
    :aria-label="ariaLabel"
    :aria-valuemin="min"
    :aria-valuemax="max"
    :aria-valuenow="Math.round(effectiveValue * 100) / 100"
    class="group relative flex h-5 w-full cursor-pointer items-center select-none outline-none touch-none"
    :class="disabled ? 'pointer-events-none opacity-45' : ''"
    @pointerdown="onPointerDown"
    @pointermove="onPointerMove"
    @pointerup="onPointerUp"
    @pointercancel="onPointerUp"
    @keydown="onKeydown"
  >
    <div class="relative h-1.5 w-full overflow-hidden rounded-full bg-[var(--ctp-surface0)] transition-all group-hover:h-2">
      <div
        v-if="bufferedPercent > 0"
        class="absolute inset-y-0 left-0 rounded-full bg-[var(--ctp-surface2)]/65"
        :style="{ width: `${bufferedPercent}%` }"
      />
      <div
        class="absolute inset-y-0 left-0 rounded-full bg-[var(--ctp-accent)]"
        :style="{ width: `${fillPercent}%` }"
      />
    </div>

    <span
      class="pointer-events-none absolute h-3.5 w-3.5 -translate-x-1/2 rounded-full bg-[var(--ctp-accent)] ring-2 ring-[var(--ctp-canvas)] shadow-sm transition-transform"
      :class="dragging ? 'scale-125' : 'scale-0 group-hover:scale-100 group-focus-visible:scale-100'"
      :style="{ left: `${fillPercent}%` }"
    />
  </div>
</template>

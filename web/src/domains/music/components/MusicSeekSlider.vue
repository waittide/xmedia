<script setup lang="ts">
import { computed, ref } from 'vue'

const props = withDefaults(
  defineProps<{
    modelValue: number
    min?: number
    max?: number
    step?: number
    buffered?: number
    disabled?: boolean
    commitOnRelease?: boolean
    alwaysShowThumb?: boolean
    ariaLabel?: string
    ariaValueText?: string
  }>(),
  {
    min: 0,
    max: 100,
    step: 0.2,
    buffered: 0,
    disabled: false,
    commitOnRelease: true,
    alwaysShowThumb: false,
    ariaLabel: undefined,
    ariaValueText: undefined,
  },
)

const emit = defineEmits<{
  'update:modelValue': [value: number]
  preview: [value: number | null]
  commit: [value: number]
}>()

const trackRef = ref<HTMLDivElement | null>(null)
const dragging = ref(false)
const draftValue = ref<number | null>(null)
const activePointerId = ref<number | null>(null)

const safeMax = computed(() => (props.max > props.min ? props.max : props.min + 1))

const displayValue = computed(() => {
  const raw = dragging.value && draftValue.value !== null ? draftValue.value : props.modelValue
  return Math.max(props.min, Math.min(safeMax.value, raw))
})

const ratioPercent = computed(() => {
  const span = safeMax.value - props.min
  if (span <= 0) return 0
  return Math.max(0, Math.min(100, ((displayValue.value - props.min) / span) * 100))
})

const bufferedPercent = computed(() => Math.max(0, Math.min(100, props.buffered)))

function snapValue(raw: number): number {
  const clamped = Math.max(props.min, Math.min(safeMax.value, raw))
  if (props.step <= 0) return clamped
  const steps = Math.round((clamped - props.min) / props.step)
  const snapped = props.min + steps * props.step
  return Math.max(props.min, Math.min(safeMax.value, Number(snapped.toFixed(4))))
}

function valueFromClientX(clientX: number): number {
  const el = trackRef.value
  if (!el) return displayValue.value
  const rect = el.getBoundingClientRect()
  if (rect.width <= 0) return displayValue.value
  const ratio = Math.max(0, Math.min(1, (clientX - rect.left) / rect.width))
  return snapValue(props.min + ratio * (safeMax.value - props.min))
}

function onPointerDown(ev: PointerEvent): void {
  if (props.disabled) return
  if (ev.button !== 0 && ev.pointerType === 'mouse') return
  const el = trackRef.value
  if (!el) return
  ev.preventDefault()
  el.focus()
  activePointerId.value = ev.pointerId
  try {
    el.setPointerCapture(ev.pointerId)
  } catch {
    // ignore pointer capture failures
  }
  dragging.value = true
  const val = valueFromClientX(ev.clientX)
  draftValue.value = val
  emit('preview', val)
  if (!props.commitOnRelease) {
    emit('update:modelValue', val)
  }
}

function onPointerMove(ev: PointerEvent): void {
  if (!dragging.value || activePointerId.value !== ev.pointerId) return
  const val = valueFromClientX(ev.clientX)
  draftValue.value = val
  emit('preview', val)
  if (!props.commitOnRelease) {
    emit('update:modelValue', val)
  }
}

function finishDrag(ev: PointerEvent, commit: boolean): void {
  if (!dragging.value || activePointerId.value !== ev.pointerId) return
  const finalVal = commit ? valueFromClientX(ev.clientX) : draftValue.value
  dragging.value = false
  activePointerId.value = null
  draftValue.value = null
  const el = trackRef.value
  if (el && el.hasPointerCapture(ev.pointerId)) {
    try {
      el.releasePointerCapture(ev.pointerId)
    } catch {
      // ignore
    }
  }
  emit('preview', null)
  if (commit && finalVal !== null) {
    emit('update:modelValue', finalVal)
    emit('commit', finalVal)
  }
}

function onKeydown(ev: KeyboardEvent): void {
  if (props.disabled) return
  const span = safeMax.value - props.min
  const coarse = Math.max(props.step * 10, span * 0.05)
  const fine = Math.max(props.step, span * 0.01)
  let next: number | null = null

  if (ev.key === 'ArrowRight' || ev.key === 'ArrowUp') {
    next = snapValue(displayValue.value + fine)
  } else if (ev.key === 'ArrowLeft' || ev.key === 'ArrowDown') {
    next = snapValue(displayValue.value - fine)
  } else if (ev.key === 'PageUp') {
    next = snapValue(displayValue.value + coarse)
  } else if (ev.key === 'PageDown') {
    next = snapValue(displayValue.value - coarse)
  } else if (ev.key === 'Home') {
    next = props.min
  } else if (ev.key === 'End') {
    next = safeMax.value
  }

  if (next !== null) {
    ev.preventDefault()
    emit('update:modelValue', next)
    emit('commit', next)
  }
}
</script>

<template>
  <div
    ref="trackRef"
    role="slider"
    :tabindex="disabled ? -1 : 0"
    :aria-label="ariaLabel"
    :aria-valuemin="min"
    :aria-valuemax="safeMax"
    :aria-valuenow="Number(displayValue.toFixed(2))"
    :aria-valuetext="ariaValueText"
    :aria-disabled="disabled ? 'true' : undefined"
    class="group/slider relative flex h-6 w-full touch-none items-center select-none focus-visible:outline-none"
    :class="disabled ? 'cursor-not-allowed opacity-45' : 'cursor-pointer'"
    @pointerdown="onPointerDown"
    @pointermove="onPointerMove"
    @pointerup="finishDrag($event, true)"
    @pointercancel="finishDrag($event, false)"
    @keydown="onKeydown"
  >
    <!-- Inactive Track -->
    <div
      class="relative h-1 w-full overflow-hidden rounded-corner-full bg-surface-container-highest transition-[height] duration-150 ease-standard group-hover/slider:h-1.5"
      :class="dragging ? 'h-1.5' : ''"
    >
      <!-- Buffered Track -->
      <div
        v-if="bufferedPercent > 0"
        class="absolute inset-y-0 left-0 rounded-corner-full bg-on-surface-variant/25"
        :style="{ width: `${bufferedPercent}%` }"
      />
      <!-- Active Track -->
      <div
        class="absolute inset-y-0 left-0 rounded-corner-full bg-primary"
        :style="{ width: `${ratioPercent}%` }"
      />
    </div>

    <!-- MD3 Handle / Thumb -->
    <span
      aria-hidden="true"
      class="pointer-events-none absolute top-1/2 -translate-x-1/2 -translate-y-1/2 rounded-corner-full bg-primary shadow-elevation-1 transition-[transform,opacity] duration-150 ease-standard group-focus-visible/slider:ring-4 group-focus-visible/slider:ring-secondary/35"
      :class="[
        dragging
          ? 'h-4 w-4 scale-110 opacity-100'
          : alwaysShowThumb
            ? 'h-3.5 w-3.5 opacity-100 group-hover/slider:scale-110'
            : 'h-3.5 w-3.5 opacity-0 group-focus-visible/slider:opacity-100 group-hover/slider:opacity-100',
      ]"
      :style="{ left: `${ratioPercent}%` }"
    />
  </div>
</template>

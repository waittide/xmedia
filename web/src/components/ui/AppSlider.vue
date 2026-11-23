<script setup lang="ts">
import { computed, ref, watch } from 'vue'
import { useI18n } from 'vue-i18n'

const props = withDefaults(
  defineProps<{
    modelValue: number
    min?: number
    max?: number
    step?: number
    buffered?: number
    commitOnRelease?: boolean
    alwaysShowThumb?: boolean
    disabled?: boolean
    rtl?: boolean
    ariaLabel?: string
    ariaValueText?: string
  }>(),
  {
    min: 0,
    max: 100,
    step: 1,
    buffered: 0,
    commitOnRelease: false,
    alwaysShowThumb: true,
    disabled: false,
    rtl: false,
    ariaLabel: undefined,
    ariaValueText: undefined,
  },
)

const emit = defineEmits<{
  'update:modelValue': [value: number]
  preview: [value: number | null]
  commit: [value: number]
  change: [value: number]
}>()

const { t, te } = useI18n()

const trackRef = ref<HTMLDivElement | null>(null)
const dragging = ref(false)
const draftValue = ref(props.modelValue)
const activePointerId = ref<number | null>(null)

const safeMax = computed(() => (props.max > props.min ? props.max : props.min + 1))

const resolvedAriaLabel = computed(() => {
  if (props.ariaLabel) return props.ariaLabel
  return te('common.slider') ? t('common.slider') : 'Slider'
})

watch(
  () => props.modelValue,
  (val) => {
    if (!dragging.value) {
      draftValue.value = val
    }
  },
)

const effectiveValue = computed(() => {
  const raw = dragging.value ? draftValue.value : props.modelValue
  return Math.max(props.min, Math.min(safeMax.value, raw))
})

const fillPercent = computed(() => {
  const span = safeMax.value - props.min
  if (span <= 0) return 0
  return Math.max(0, Math.min(100, ((effectiveValue.value - props.min) / span) * 100))
})

const bufferedPercent = computed(() => Math.max(0, Math.min(100, props.buffered)))

function snapValue(raw: number): number {
  const clamped = Math.max(props.min, Math.min(safeMax.value, raw))
  if (props.step > 0) {
    const stepped = Math.round((clamped - props.min) / props.step) * props.step + props.min
    return Math.max(props.min, Math.min(safeMax.value, Number(stepped.toFixed(4))))
  }
  return clamped
}

function computeValueFromClientX(clientX: number): number {
  const el = trackRef.value
  if (!el) return effectiveValue.value
  const rect = el.getBoundingClientRect()
  if (rect.width <= 0) return effectiveValue.value
  const rawRatio = Math.max(0, Math.min(1, (clientX - rect.left) / rect.width))
  const ratio = props.rtl ? 1 - rawRatio : rawRatio
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
  const next = computeValueFromClientX(ev.clientX)
  draftValue.value = next
  emit('preview', next)
  if (!props.commitOnRelease) {
    emit('update:modelValue', next)
  }
}

function onPointerMove(ev: PointerEvent): void {
  if (!dragging.value || props.disabled || activePointerId.value !== ev.pointerId) return
  const next = computeValueFromClientX(ev.clientX)
  draftValue.value = next
  emit('preview', next)
  if (!props.commitOnRelease) {
    emit('update:modelValue', next)
  }
}

function finishDrag(ev: PointerEvent, shouldCommit: boolean): void {
  if (!dragging.value || activePointerId.value !== ev.pointerId) return
  const finalVal = shouldCommit ? computeValueFromClientX(ev.clientX) : draftValue.value
  const el = trackRef.value
  if (el && el.hasPointerCapture(ev.pointerId)) {
    try {
      el.releasePointerCapture(ev.pointerId)
    } catch {
      // ignore
    }
  }
  dragging.value = false
  activePointerId.value = null
  emit('preview', null)
  if (shouldCommit) {
    draftValue.value = finalVal
    emit('update:modelValue', finalVal)
    emit('commit', finalVal)
    emit('change', finalVal)
  }
}

function onKeydown(ev: KeyboardEvent): void {
  if (props.disabled) return
  const span = safeMax.value - props.min
  const fine = props.step > 0 ? props.step : span / 50
  const coarse = Math.max(fine * 10, span * 0.05)
  const dir = props.rtl ? -1 : 1
  let next: number | null = null
  if (ev.key === 'ArrowRight' || ev.key === 'ArrowUp') {
    next = snapValue(effectiveValue.value + fine * dir)
  } else if (ev.key === 'ArrowLeft' || ev.key === 'ArrowDown') {
    next = snapValue(effectiveValue.value - fine * dir)
  } else if (ev.key === 'PageUp') {
    next = snapValue(effectiveValue.value + coarse * dir)
  } else if (ev.key === 'PageDown') {
    next = snapValue(effectiveValue.value - coarse * dir)
  } else if (ev.key === 'Home') {
    next = props.min
  } else if (ev.key === 'End') {
    next = safeMax.value
  }
  if (next !== null) {
    ev.preventDefault()
    emit('update:modelValue', next)
    emit('commit', next)
    emit('change', next)
  }
}
</script>

<template>
  <div
    ref="trackRef"
    role="slider"
    :tabindex="disabled ? -1 : 0"
    :aria-label="resolvedAriaLabel"
    :aria-valuemin="min"
    :aria-valuemax="safeMax"
    :aria-valuenow="Math.round(effectiveValue * 100) / 100"
    :aria-valuetext="ariaValueText"
    :aria-disabled="disabled ? 'true' : undefined"
    class="group md-touch-target relative flex h-8 w-full touch-none items-center select-none focus-visible:outline-none"
    :class="disabled ? 'pointer-events-none opacity-38' : 'cursor-pointer'"
    @pointerdown="onPointerDown"
    @pointermove="onPointerMove"
    @pointerup="finishDrag($event, true)"
    @pointercancel="finishDrag($event, false)"
    @keydown="onKeydown"
  >
    <div
      class="relative h-1 w-full overflow-hidden rounded-corner-full bg-secondary-container"
    >
      <div
        v-if="bufferedPercent > 0"
        class="absolute inset-y-0 rounded-corner-full bg-outline-variant"
        :class="rtl ? 'right-0' : 'left-0'"
        :style="{ width: `${bufferedPercent}%` }"
      />
      <div
        class="absolute inset-y-0 rounded-corner-full bg-primary"
        :class="rtl ? 'right-0' : 'left-0'"
        :style="{ width: `${fillPercent}%` }"
      />
      <span
        aria-hidden="true"
        class="pointer-events-none absolute top-1/2 h-1 w-1 -translate-y-1/2 rounded-corner-full bg-primary opacity-60"
        :class="rtl ? 'left-0.5' : 'right-0.5'"
      />
    </div>

    <span
      aria-hidden="true"
      class="pointer-events-none absolute flex h-5 w-5 -translate-x-1/2 items-center justify-center rounded-corner-full bg-primary shadow-elevation-1 transition-transform duration-150 ease-standard group-focus-visible:outline-3 group-focus-visible:outline-offset-2 group-focus-visible:outline-secondary"
      :class="
        dragging
          ? 'scale-115'
          : alwaysShowThumb
            ? 'scale-100 group-hover:scale-105'
            : 'scale-0 group-hover:scale-100 group-focus-visible:scale-100'
      "
      :style="{ left: `${rtl ? 100 - fillPercent : fillPercent}%` }"
    >
      <span
        v-if="!disabled"
        aria-hidden="true"
        class="pointer-events-none absolute -inset-2.5 rounded-corner-full bg-primary opacity-0 transition-opacity group-hover:opacity-[0.08] group-focus-visible:opacity-[0.10]"
        :class="dragging ? 'opacity-[0.10]' : ''"
      />
    </span>
  </div>
</template>

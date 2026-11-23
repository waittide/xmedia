<script setup lang="ts">
import { nextTick, onBeforeUnmount, ref, useId, watch } from 'vue'

const props = withDefaults(
  defineProps<{
    text: string
    placement?: 'top' | 'bottom'
  }>(),
  {
    placement: 'top',
  },
)

const tooltipId = useId()
const visible = ref(false)
const triggerRef = ref<HTMLElement | null>(null)
const tooltipRef = ref<HTMLElement | null>(null)
const tooltipStyle = ref<Record<string, string>>({})
const teleportTarget = ref<string | HTMLElement>('body')

function syncTeleportTarget(): void {
  if (typeof document !== 'undefined' && document.fullscreenElement instanceof HTMLElement) {
    teleportTarget.value = document.fullscreenElement
  } else {
    teleportTarget.value = 'body'
  }
}

function updatePosition(): void {
  if (!visible.value || typeof window === 'undefined') return
  const trigger = triggerRef.value
  if (!trigger) return

  const rect = trigger.getBoundingClientRect()
  const vw = window.innerWidth
  const vh = window.innerHeight
  const margin = 8
  const gap = 6

  const tipWidth = tooltipRef.value?.offsetWidth || 0
  const tipHeight = tooltipRef.value?.offsetHeight || 28

  const spaceAbove = rect.top - margin
  const spaceBelow = vh - rect.bottom - margin

  let placeBottom = props.placement === 'bottom'
  if (!placeBottom && spaceAbove < tipHeight + gap && spaceBelow > spaceAbove) {
    placeBottom = true
  } else if (placeBottom && spaceBelow < tipHeight + gap && spaceAbove > spaceBelow) {
    placeBottom = false
  }

  const rawTop = placeBottom ? rect.bottom + gap : rect.top - gap - tipHeight
  const top = Math.max(margin, Math.min(rawTop, vh - tipHeight - margin))

  const centerX = rect.left + rect.width / 2
  const left =
    tipWidth > 0
      ? Math.max(margin, Math.min(centerX - tipWidth / 2, vw - tipWidth - margin))
      : Math.max(margin, Math.min(centerX, vw - margin))

  tooltipStyle.value = {
    position: 'fixed',
    top: `${Math.round(top)}px`,
    left: `${Math.round(left)}px`,
    ...(tipWidth === 0 ? { transform: 'translateX(-50%)' } : {}),
  }
}

function show(): void {
  if (!props.text) return
  syncTeleportTarget()
  visible.value = true
}

function hide(): void {
  visible.value = false
}

function handleViewportChange(): void {
  if (visible.value) updatePosition()
}

function bindListeners(): void {
  window.addEventListener('resize', handleViewportChange)
  window.addEventListener('scroll', hide, true)
}

function unbindListeners(): void {
  window.removeEventListener('resize', handleViewportChange)
  window.removeEventListener('scroll', hide, true)
}

watch(visible, async (openNow) => {
  if (openNow) {
    bindListeners()
    updatePosition()
    await nextTick()
    updatePosition()
  } else {
    unbindListeners()
  }
})

onBeforeUnmount(unbindListeners)
</script>

<template>
  <span
    ref="triggerRef"
    class="inline-flex"
    @mouseenter="show"
    @mouseleave="hide"
    @focusin="show"
    @focusout="hide"
    @pointerdown="hide"
    @keydown.escape="hide"
  >
    <slot :tooltip-id="tooltipId" />
    <Teleport :to="teleportTarget">
      <Transition name="md-fade">
        <span
          v-if="visible && text"
          :id="tooltipId"
          ref="tooltipRef"
          role="tooltip"
          :style="tooltipStyle"
          class="text-body-small pointer-events-none z-[var(--z-tooltip)] max-w-[calc(100vw-16px)] truncate rounded-corner-xs bg-inverse-surface px-2 py-1 text-inverse-on-surface shadow-elevation-2"
        >
          {{ text }}
        </span>
      </Transition>
    </Teleport>
  </span>
</template>

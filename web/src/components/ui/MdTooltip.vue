<script setup lang="ts">
import { ref, useId } from 'vue'

withDefaults(
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
</script>

<template>
  <span
    class="relative inline-flex"
    @mouseenter="visible = true"
    @mouseleave="visible = false"
    @focusin="visible = true"
    @focusout="visible = false"
    @keydown.escape="visible = false"
  >
    <slot :tooltip-id="tooltipId" />
    <Transition name="md-fade">
      <span
        v-if="visible && text"
        :id="tooltipId"
        role="tooltip"
        class="md-body-small pointer-events-none absolute left-1/2 z-[var(--z-tooltip)] -translate-x-1/2 whitespace-nowrap rounded-[var(--md-sys-shape-corner-xs)] bg-[var(--md-sys-color-inverse-surface)] px-2 py-1 text-[var(--md-sys-color-inverse-on-surface)] shadow-[var(--md-sys-elevation-2)]"
        :class="placement === 'top' ? 'bottom-full mb-1.5' : 'top-full mt-1.5'"
      >
        {{ text }}
      </span>
    </Transition>
  </span>
</template>

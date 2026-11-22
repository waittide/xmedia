<script setup lang="ts">
import { computed, useSlots, type Component } from 'vue'
import { RouterLink, type RouteLocationRaw } from 'vue-router'

export type MdFabSize = 'sm' | 'md' | 'lg' | 'extended'
export type MdFabTone = 'surface' | 'primary' | 'secondary' | 'tertiary'

const props = withDefaults(
  defineProps<{
    size?: MdFabSize
    tone?: MdFabTone
    icon?: Component
    label?: string
    lowered?: boolean
    to?: RouteLocationRaw
    disabled?: boolean
    ariaLabel?: string
  }>(),
  {
    size: 'md',
    tone: 'primary',
    icon: undefined,
    label: '',
    lowered: false,
    to: undefined,
    disabled: false,
    ariaLabel: undefined,
  },
)

const slots = useSlots()
const isExtended = computed(
  () => props.size === 'extended' || Boolean(props.label) || Boolean(slots.default),
)

const sizeClass = computed(() => {
  if (isExtended.value) {
    return 'h-14 gap-3 rounded-corner-lg px-4 text-label-large'
  }
  switch (props.size) {
    case 'sm':
      return 'md-touch-target h-10 w-10 rounded-corner-md'
    case 'lg':
      return 'h-24 w-24 rounded-corner-xl'
    case 'md':
    default:
      return 'h-14 w-14 rounded-corner-lg'
  }
})

const iconSizeClass = computed(() =>
  props.size === 'lg' && !isExtended.value ? 'h-9 w-9 shrink-0' : 'h-6 w-6 shrink-0',
)

const toneClass = computed(() => {
  if (props.disabled) {
    return 'bg-on-surface/12 text-on-surface/38 shadow-none'
  }
  const elevation = props.lowered
    ? 'shadow-elevation-1 hover:shadow-elevation-2'
    : 'shadow-elevation-3'
  switch (props.tone) {
    case 'surface':
      return `bg-surface-container-high text-primary ${elevation}`
    case 'secondary':
      return `bg-secondary-container text-on-secondary-container ${elevation}`
    case 'tertiary':
      return `bg-tertiary-container text-on-tertiary-container ${elevation}`
    case 'primary':
    default:
      return `bg-primary-container text-on-primary-container ${elevation}`
  }
})
</script>

<template>
  <RouterLink
    v-if="to && !disabled"
    :to="to"
    :aria-label="ariaLabel || label || undefined"
    class="md-state inline-flex cursor-pointer items-center justify-center whitespace-nowrap select-none"
    :class="[sizeClass, toneClass]"
  >
    <slot name="icon">
      <component :is="icon" v-if="icon" :class="iconSizeClass" aria-hidden="true" />
    </slot>
    <slot>
      <span v-if="label">{{ label }}</span>
    </slot>
  </RouterLink>

  <button
    v-else
    type="button"
    :disabled="disabled"
    :aria-label="ariaLabel || label || undefined"
    class="md-state inline-flex items-center justify-center whitespace-nowrap select-none"
    :class="[sizeClass, toneClass, disabled ? 'cursor-not-allowed' : 'cursor-pointer']"
  >
    <slot name="icon">
      <component :is="icon" v-if="icon" :class="iconSizeClass" aria-hidden="true" />
    </slot>
    <slot>
      <span v-if="label">{{ label }}</span>
    </slot>
  </button>
</template>

<script setup lang="ts">
import { computed, useSlots } from 'vue'
import { RouterLink, type RouteLocationRaw } from 'vue-router'

export type MdListItemLines = 1 | 2 | 3

const props = withDefaults(
  defineProps<{
    headline?: string
    supportingText?: string
    overline?: string
    trailingSupportingText?: string
    lines?: MdListItemLines
    clickable?: boolean
    active?: boolean
    disabled?: boolean
    to?: RouteLocationRaw
  }>(),
  {
    headline: '',
    supportingText: '',
    overline: '',
    trailingSupportingText: '',
    lines: undefined,
    clickable: false,
    active: false,
    disabled: false,
    to: undefined,
  },
)

const slots = useSlots()

const resolvedLines = computed<MdListItemLines>(() => {
  if (props.lines) return props.lines
  const hasSupporting = Boolean(props.supportingText || slots.supporting)
  const hasOverline = Boolean(props.overline)
  if (hasOverline && hasSupporting) return 3
  if (hasSupporting || hasOverline) return 2
  return 1
})

const heightClass = computed(() => {
  switch (resolvedLines.value) {
    case 3:
      return 'min-h-[88px] py-3 items-start'
    case 2:
      return 'min-h-[72px] py-2 items-center'
    case 1:
    default:
      return 'min-h-14 py-2 items-center'
  }
})

const stateClass = computed(() => {
  if (props.disabled) {
    return 'text-on-surface/38 cursor-not-allowed'
  }
  if (props.active) {
    return 'bg-secondary-container text-on-secondary-container'
  }
  return 'text-on-surface'
})

const rootTag = computed(() => {
  if (props.to && !props.disabled) return RouterLink
  if (props.clickable) return 'button'
  return 'div'
})

const rootAttrs = computed(() => {
  if (props.to && !props.disabled) {
    return {
      to: props.to,
      'aria-current': props.active ? 'page' : undefined,
    }
  }
  if (props.clickable) {
    return {
      type: 'button',
      disabled: props.disabled,
      'aria-current': props.active ? 'true' : undefined,
    }
  }
  return {}
})

const rootInteractionClass = computed(() => {
  if (props.to && !props.disabled) return 'md-state cursor-pointer select-none'
  if (props.clickable) {
    return props.disabled ? 'md-state cursor-not-allowed select-none' : 'md-state cursor-pointer select-none'
  }
  return ''
})
</script>

<template>
  <component
    :is="rootTag"
    v-bind="rootAttrs"
    class="flex w-full gap-4 px-4 text-left"
    :class="[heightClass, stateClass, rootInteractionClass]"
  >
    <div v-if="$slots.leading" class="flex shrink-0 items-center justify-center text-on-surface-variant">
      <slot name="leading" />
    </div>

    <div class="min-w-0 flex-1">
      <p v-if="overline" class="text-label-small truncate text-on-surface-variant">
        {{ overline }}
      </p>
      <p class="text-body-large truncate">
        <slot>{{ headline }}</slot>
      </p>
      <p
        v-if="supportingText || $slots.supporting"
        class="text-body-medium text-on-surface-variant"
        :class="resolvedLines === 3 ? 'line-clamp-2' : 'truncate'"
      >
        <slot name="supporting">{{ supportingText }}</slot>
      </p>
    </div>

    <div v-if="trailingSupportingText || $slots.trailing" class="flex shrink-0 items-center gap-2 text-on-surface-variant">
      <span v-if="trailingSupportingText" class="text-label-small tabular-nums">
        {{ trailingSupportingText }}
      </span>
      <slot name="trailing" />
    </div>
  </component>
</template>

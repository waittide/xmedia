<script setup lang="ts">
import { computed } from 'vue'
import { RouterLink, type RouteLocationRaw } from 'vue-router'

export type MdCardVariant = 'filled' | 'tonal' | 'elevated' | 'outlined'
export type MdCardPadding = 'none' | 'sm' | 'md' | 'lg'

const props = withDefaults(
  defineProps<{
    variant?: MdCardVariant
    padding?: MdCardPadding
    clickable?: boolean
    to?: RouteLocationRaw
    as?: 'div' | 'article' | 'section' | 'button'
    disabled?: boolean
  }>(),
  {
    variant: 'filled',
    padding: 'none',
    clickable: false,
    to: undefined,
    as: 'div',
    disabled: false,
  },
)

const isInteractive = computed(() => !props.disabled && (props.clickable || Boolean(props.to)))

const variantClass = computed(() => {
  switch (props.variant) {
    case 'tonal':
      return isInteractive.value
        ? 'bg-surface-container-low text-on-surface hover:bg-surface-container'
        : 'bg-surface-container-low text-on-surface'
    case 'elevated':
      return isInteractive.value
        ? 'bg-surface-container-low text-on-surface shadow-elevation-1 hover:shadow-elevation-2'
        : 'bg-surface-container-low text-on-surface shadow-elevation-1'
    case 'outlined':
      return 'border border-outline-variant bg-surface text-on-surface'
    case 'filled':
    default:
      return isInteractive.value
        ? 'bg-surface-container-highest text-on-surface hover:shadow-elevation-1'
        : 'bg-surface-container-highest text-on-surface'
  }
})

const paddingClass = computed(() => {
  switch (props.padding) {
    case 'sm':
      return 'p-3'
    case 'md':
      return 'p-4'
    case 'lg':
      return 'p-6'
    case 'none':
    default:
      return ''
  }
})

function onSemanticKeydown(ev: KeyboardEvent): void {
  if (!isInteractive.value || ev.target !== ev.currentTarget) return
  if (ev.key === 'Enter' || ev.key === ' ') {
    ev.preventDefault()
    ;(ev.currentTarget as HTMLElement | null)?.click()
  }
}
</script>

<template>
  <RouterLink
    v-if="to && !disabled"
    :to="to"
    class="md-state block rounded-corner-md text-left cursor-pointer"
    :class="[variantClass, paddingClass]"
  >
    <slot />
  </RouterLink>

  <button
    v-else-if="as === 'button' || (clickable && as === 'div')"
    type="button"
    :disabled="disabled"
    class="md-state block w-full rounded-corner-md text-left"
    :class="[variantClass, paddingClass, disabled ? 'cursor-not-allowed opacity-38' : 'cursor-pointer']"
  >
    <slot />
  </button>

  <component
    :is="as"
    v-else
    :role="isInteractive ? 'button' : undefined"
    :tabindex="isInteractive ? 0 : undefined"
    class="rounded-corner-md"
    :class="[
      variantClass,
      paddingClass,
      isInteractive ? 'md-state cursor-pointer' : '',
      disabled ? 'opacity-38' : '',
    ]"
    @keydown="onSemanticKeydown"
  >
    <slot />
  </component>
</template>

<script setup lang="ts">
import { computed, type Component } from 'vue'
import { RouterLink, type RouteLocationRaw } from 'vue-router'
import { LuLoader2 } from 'vue-icons-plus/lu'

export type MdButtonVariant = 'filled' | 'tonal' | 'outlined' | 'text' | 'elevated'
export type MdButtonSize = 'sm' | 'md'
export type MdButtonTone = 'primary' | 'error'

const props = withDefaults(
  defineProps<{
    variant?: MdButtonVariant
    size?: MdButtonSize
    tone?: MdButtonTone
    type?: 'button' | 'submit' | 'reset'
    to?: RouteLocationRaw
    href?: string
    icon?: Component
    loading?: boolean
    disabled?: boolean
    block?: boolean
  }>(),
  {
    variant: 'filled',
    size: 'md',
    tone: 'primary',
    type: 'button',
    to: undefined,
    href: undefined,
    icon: undefined,
    loading: false,
    disabled: false,
    block: false,
  },
)

const isDisabled = computed(() => props.disabled || props.loading)
const isLink = computed(() => Boolean(props.to || props.href))

const sizeClass = computed(() => {
  const isText = props.variant === 'text'
  if (props.size === 'sm') {
    return isText
      ? 'md-touch-target h-8 gap-1.5 px-2.5 text-label-medium'
      : 'md-touch-target h-8 gap-1.5 px-4 text-label-medium'
  }
  return isText ? 'h-10 gap-2 px-3 text-label-large' : 'h-10 gap-2 px-6 text-label-large'
})

const variantClass = computed(() => {
  if (isDisabled.value) {
    if (props.variant === 'outlined') {
      return 'border border-on-surface/12 bg-transparent text-on-surface/38 shadow-none'
    }
    if (props.variant === 'text') {
      return 'bg-transparent text-on-surface/38 shadow-none'
    }
    return 'bg-on-surface/12 text-on-surface/38 shadow-none'
  }

  const isError = props.tone === 'error'
  switch (props.variant) {
    case 'filled':
      return isError
        ? 'bg-error text-on-error hover:shadow-elevation-1'
        : 'bg-primary text-on-primary hover:shadow-elevation-1'
    case 'tonal':
      return isError
        ? 'bg-error-container text-on-error-container hover:shadow-elevation-1'
        : 'bg-secondary-container text-on-secondary-container hover:shadow-elevation-1'
    case 'elevated':
      return isError
        ? 'bg-surface-container-low text-error shadow-elevation-1 hover:shadow-elevation-2'
        : 'bg-surface-container-low text-primary shadow-elevation-1 hover:shadow-elevation-2'
    case 'outlined':
      return isError
        ? 'border border-error bg-transparent text-error'
        : 'border border-outline bg-transparent text-primary'
    case 'text':
      return isError ? 'bg-transparent text-error' : 'bg-transparent text-primary'
  }
})

const iconSizeClass = computed(() =>
  props.size === 'sm' ? 'h-4 w-4 shrink-0' : 'h-[18px] w-[18px] shrink-0',
)

function onLinkClick(ev: MouseEvent): void {
  if (isDisabled.value) {
    ev.preventDefault()
    ev.stopPropagation()
  }
}
</script>

<template>
  <RouterLink
    v-if="to && !isDisabled"
    :to="to"
    class="md-state inline-flex cursor-pointer items-center justify-center rounded-corner-full whitespace-nowrap select-none"
    :class="[sizeClass, variantClass, block ? 'w-full' : '']"
  >
    <LuLoader2 v-if="loading" :class="[iconSizeClass, 'animate-spin']" aria-hidden="true" />
    <slot v-else name="icon">
      <component :is="icon" v-if="icon" :class="iconSizeClass" aria-hidden="true" />
    </slot>
    <slot />
    <slot name="trailing" />
  </RouterLink>

  <a
    v-else-if="href"
    :href="isDisabled ? undefined : href"
    :aria-disabled="isDisabled ? 'true' : undefined"
    :aria-busy="loading ? 'true' : undefined"
    :tabindex="isDisabled ? -1 : undefined"
    class="md-state inline-flex items-center justify-center rounded-corner-full whitespace-nowrap select-none"
    :class="[sizeClass, variantClass, block ? 'w-full' : '', isDisabled ? 'cursor-not-allowed' : 'cursor-pointer']"
    @click="onLinkClick"
  >
    <LuLoader2 v-if="loading" :class="[iconSizeClass, 'animate-spin']" aria-hidden="true" />
    <slot v-else name="icon">
      <component :is="icon" v-if="icon" :class="iconSizeClass" aria-hidden="true" />
    </slot>
    <slot />
    <slot name="trailing" />
  </a>

  <button
    v-else
    :type="isLink ? 'button' : type"
    :disabled="isDisabled"
    :aria-busy="loading ? 'true' : undefined"
    class="md-state inline-flex items-center justify-center rounded-corner-full whitespace-nowrap select-none"
    :class="[sizeClass, variantClass, block ? 'w-full' : '', isDisabled ? 'cursor-not-allowed' : 'cursor-pointer']"
  >
    <LuLoader2 v-if="loading" :class="[iconSizeClass, 'animate-spin']" aria-hidden="true" />
    <slot v-else name="icon">
      <component :is="icon" v-if="icon" :class="iconSizeClass" aria-hidden="true" />
    </slot>
    <slot />
    <slot name="trailing" />
  </button>
</template>

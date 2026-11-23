<script setup lang="ts">
import { computed, inject, type Component } from 'vue'
import { RouterLink, type RouteLocationRaw } from 'vue-router'
import { LuCheck } from 'vue-icons-plus/lu'
import { MD_MENU_CONTEXT_KEY } from './menuContext'

const props = withDefaults(
  defineProps<{
    label?: string
    icon?: Component
    trailingText?: string
    selected?: boolean
    destructive?: boolean
    disabled?: boolean
    to?: RouteLocationRaw
    keepOpen?: boolean
  }>(),
  {
    label: '',
    icon: undefined,
    trailingText: '',
    selected: false,
    destructive: false,
    disabled: false,
    to: undefined,
    keepOpen: false,
  },
)

const emit = defineEmits<{
  select: [ev: MouseEvent]
}>()

const menuCtx = inject(MD_MENU_CONTEXT_KEY, null)

const toneClass = computed(() => {
  if (props.disabled) {
    return 'text-on-surface/38 cursor-not-allowed'
  }
  if (props.destructive) {
    return 'text-error cursor-pointer'
  }
  if (props.selected) {
    return 'bg-secondary-container text-on-secondary-container cursor-pointer'
  }
  return 'text-on-surface cursor-pointer'
})

const iconToneClass = computed(() => {
  if (props.disabled) return 'text-on-surface/38'
  if (props.destructive) return 'text-error'
  if (props.selected) return 'text-on-secondary-container'
  return 'text-on-surface-variant'
})

function handleClick(ev: MouseEvent): void {
  if (props.disabled) {
    ev.preventDefault()
    ev.stopPropagation()
    return
  }
  emit('select', ev)
  if (!props.keepOpen) {
    menuCtx?.close()
  }
}
const isRouterLink = computed(() => Boolean(props.to && !props.disabled))

const rootAttrs = computed(() =>
  isRouterLink.value
    ? { to: props.to }
    : {
        type: 'button',
        disabled: props.disabled,
        'aria-disabled': props.disabled ? 'true' : undefined,
      },
)
</script>

<template>
  <component
    :is="isRouterLink ? RouterLink : 'button'"
    v-bind="rootAttrs"
    role="menuitem"
    class="md-state text-label-large flex h-12 w-full items-center gap-3 rounded-corner-xs px-3 text-left select-none focus-visible:outline-2 focus-visible:-outline-offset-2 focus-visible:outline-secondary"
    :class="toneClass"
    @click="handleClick"
  >
    <slot name="leading">
      <component :is="icon" v-if="icon" class="h-5 w-5 shrink-0" :class="iconToneClass" aria-hidden="true" />
    </slot>
    <span class="min-w-0 flex-1 truncate">
      <slot>{{ label }}</slot>
    </span>
    <span v-if="trailingText" class="text-label-small shrink-0 text-on-surface-variant">
      {{ trailingText }}
    </span>
    <slot name="trailing">
      <LuCheck v-if="selected" class="h-4 w-4 shrink-0" aria-hidden="true" />
    </slot>
  </component>
</template>

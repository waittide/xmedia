<script setup lang="ts">
import { computed, useAttrs, type Component } from 'vue'
import { RouterLink, type RouteLocationRaw } from 'vue-router'
import { LuLoader2 } from 'vue-icons-plus/lu'
import MdTooltip from './MdTooltip.vue'

export type MdIconButtonVariant = 'standard' | 'filled' | 'tonal' | 'outlined'
export type MdIconButtonSize = 'sm' | 'md'

defineOptions({ inheritAttrs: false })

const props = withDefaults(
  defineProps<{
    variant?: MdIconButtonVariant
    size?: MdIconButtonSize
    selected?: boolean
    toggle?: boolean
    icon?: Component
    tooltip?: string
    tooltipPlacement?: 'top' | 'bottom'
    ariaLabel?: string
    type?: 'button' | 'submit' | 'reset'
    to?: RouteLocationRaw
    loading?: boolean
    disabled?: boolean
  }>(),
  {
    variant: 'standard',
    size: 'md',
    selected: undefined,
    toggle: false,
    icon: undefined,
    tooltip: '',
    tooltipPlacement: 'top',
    ariaLabel: undefined,
    type: 'button',
    to: undefined,
    loading: false,
    disabled: false,
  },
)

const emit = defineEmits<{
  'update:selected': [value: boolean]
}>()

const attrs = useAttrs()
const isDisabled = computed(() => props.disabled || props.loading)
const isToggle = computed(() => props.toggle || props.selected !== undefined)
const isSelected = computed(() => Boolean(props.selected))
const resolvedAriaLabel = computed(() => props.ariaLabel || props.tooltip || undefined)

const sizeClass = computed(() => (props.size === 'sm' ? 'h-8 w-8' : 'h-10 w-10'))
const iconSizeClass = computed(() => (props.size === 'sm' ? 'h-4 w-4 shrink-0' : 'h-5 w-5 shrink-0'))

const variantClass = computed(() => {
  if (isDisabled.value) {
    if (props.variant === 'outlined') {
      return 'border border-on-surface/12 bg-transparent text-on-surface/38'
    }
    if (props.variant === 'standard') {
      return 'bg-transparent text-on-surface/38'
    }
    return 'bg-on-surface/12 text-on-surface/38'
  }

  switch (props.variant) {
    case 'filled':
      if (isToggle.value && !isSelected.value) {
        return 'bg-surface-container-highest text-primary'
      }
      return 'bg-primary text-on-primary'
    case 'tonal':
      if (isToggle.value && !isSelected.value) {
        return 'bg-surface-container-highest text-on-surface-variant'
      }
      return 'bg-secondary-container text-on-secondary-container'
    case 'outlined':
      if (isSelected.value) {
        return 'border border-transparent bg-inverse-surface text-inverse-on-surface'
      }
      return 'border border-outline bg-transparent text-on-surface-variant hover:text-on-surface'
    case 'standard':
      if (isSelected.value) {
        return 'bg-transparent text-primary'
      }
      return 'bg-transparent text-on-surface-variant hover:text-on-surface'
  }
})

function handleClick(): void {
  if (isDisabled.value) return
  if (isToggle.value) {
    emit('update:selected', !isSelected.value)
  }
}
</script>

<template>
  <MdTooltip v-if="tooltip && !isDisabled" :text="tooltip" :placement="tooltipPlacement">
    <template #default="{ tooltipId }">
      <RouterLink
        v-if="to"
        v-bind="attrs"
        :to="to"
        :aria-label="resolvedAriaLabel"
        :aria-describedby="tooltipId"
        class="md-state md-touch-target inline-flex shrink-0 cursor-pointer items-center justify-center rounded-corner-full select-none"
        :class="[sizeClass, variantClass]"
      >
        <LuLoader2 v-if="loading" :class="[iconSizeClass, 'animate-spin']" aria-hidden="true" />
        <slot v-else>
          <component :is="icon" v-if="icon" :class="iconSizeClass" aria-hidden="true" />
        </slot>
      </RouterLink>
      <button
        v-else
        v-bind="attrs"
        :type="type"
        :disabled="isDisabled"
        :aria-label="resolvedAriaLabel"
        :aria-describedby="tooltipId"
        :aria-pressed="isToggle ? isSelected : undefined"
        :aria-busy="loading ? 'true' : undefined"
        class="md-state md-touch-target inline-flex shrink-0 cursor-pointer items-center justify-center rounded-corner-full select-none"
        :class="[sizeClass, variantClass]"
        @click="handleClick"
      >
        <LuLoader2 v-if="loading" :class="[iconSizeClass, 'animate-spin']" aria-hidden="true" />
        <slot v-else>
          <component :is="icon" v-if="icon" :class="iconSizeClass" aria-hidden="true" />
        </slot>
      </button>
    </template>
  </MdTooltip>

  <RouterLink
    v-else-if="to && !isDisabled"
    v-bind="attrs"
    :to="to"
    :aria-label="resolvedAriaLabel"
    class="md-state md-touch-target inline-flex shrink-0 cursor-pointer items-center justify-center rounded-corner-full select-none"
    :class="[sizeClass, variantClass]"
  >
    <LuLoader2 v-if="loading" :class="[iconSizeClass, 'animate-spin']" aria-hidden="true" />
    <slot v-else>
      <component :is="icon" v-if="icon" :class="iconSizeClass" aria-hidden="true" />
    </slot>
  </RouterLink>

  <button
    v-else
    v-bind="attrs"
    :type="type"
    :disabled="isDisabled"
    :aria-label="resolvedAriaLabel"
    :aria-pressed="isToggle ? isSelected : undefined"
    :aria-busy="loading ? 'true' : undefined"
    class="md-state md-touch-target inline-flex shrink-0 items-center justify-center rounded-corner-full select-none"
    :class="[sizeClass, variantClass, isDisabled ? 'cursor-not-allowed' : 'cursor-pointer']"
    @click="handleClick"
  >
    <LuLoader2 v-if="loading" :class="[iconSizeClass, 'animate-spin']" aria-hidden="true" />
    <slot v-else>
      <component :is="icon" v-if="icon" :class="iconSizeClass" aria-hidden="true" />
    </slot>
  </button>
</template>

<script setup lang="ts">
import { computed, type Component } from 'vue'
import { useI18n } from 'vue-i18n'
import { RouterLink, type RouteLocationRaw } from 'vue-router'
import { LuCheck, LuX } from 'vue-icons-plus/lu'

export type MdChipVariant = 'assist' | 'filter' | 'input' | 'suggestion'

const props = withDefaults(
  defineProps<{
    variant?: MdChipVariant
    label?: string
    icon?: Component
    to?: RouteLocationRaw
    selected?: boolean
    elevated?: boolean
    removable?: boolean
    hideCheck?: boolean
    disabled?: boolean
  }>(),
  {
    variant: 'assist',
    label: '',
    icon: undefined,
    to: undefined,
    selected: false,
    elevated: false,
    removable: false,
    hideCheck: false,
    disabled: false,
  },
)

const emit = defineEmits<{
  click: [ev: MouseEvent]
  remove: []
  'update:selected': [value: boolean]
}>()

const { t } = useI18n()

const isSelectable = computed(() => props.variant === 'filter' || props.variant === 'input')
const showCheck = computed(() => isSelectable.value && props.selected && !props.hideCheck)
const showRemove = computed(() => props.removable || props.variant === 'input')

const surfaceClass = computed(() => {
  if (props.disabled) {
    return props.selected || props.elevated
      ? 'border border-transparent bg-on-surface/12 text-on-surface/38 shadow-none'
      : 'border border-on-surface/12 bg-transparent text-on-surface/38'
  }
  if (props.selected) {
    return props.elevated
      ? 'border border-transparent bg-secondary-container text-on-secondary-container shadow-elevation-1'
      : 'border border-transparent bg-secondary-container text-on-secondary-container'
  }
  if (props.elevated) {
    return 'border border-transparent bg-surface-container-low text-on-surface shadow-elevation-1 hover:shadow-elevation-2'
  }
  return 'border border-outline bg-transparent text-on-surface-variant hover:text-on-surface'
})

function onChipClick(ev: MouseEvent): void {
  if (props.disabled) return
  if (props.variant === 'filter') {
    emit('update:selected', !props.selected)
  }
  emit('click', ev)
}

function onRemoveClick(): void {
  if (props.disabled) return
  emit('remove')
}
</script>

<template>
  <span
    class="inline-flex h-8 max-w-full items-center rounded-corner-sm select-none"
    :class="surfaceClass"
  >
    <RouterLink
      v-if="to && !disabled"
      :to="to"
      class="md-state md-touch-target text-label-large inline-flex h-full min-w-0 cursor-pointer items-center gap-2 rounded-corner-sm whitespace-nowrap"
      :class="[
        showCheck || icon || $slots.icon ? 'pl-2' : 'pl-4',
        showRemove ? 'pr-1.5' : 'pr-4',
      ]"
      @click="emit('click', $event)"
    >
      <LuCheck v-if="showCheck" class="h-[18px] w-[18px] shrink-0" aria-hidden="true" />
      <slot v-else name="icon">
        <component
          :is="icon"
          v-if="icon"
          class="h-[18px] w-[18px] shrink-0"
          :class="selected ? '' : 'text-primary'"
          aria-hidden="true"
        />
      </slot>
      <span class="truncate">
        <slot>{{ label }}</slot>
      </span>
    </RouterLink>

    <button
      v-else
      type="button"
      :disabled="disabled"
      :aria-pressed="variant === 'filter' ? selected : undefined"
      class="md-state md-touch-target text-label-large inline-flex h-full min-w-0 items-center gap-2 rounded-corner-sm whitespace-nowrap"
      :class="[
        showCheck || icon || $slots.icon ? 'pl-2' : 'pl-4',
        showRemove ? 'pr-1.5' : 'pr-4',
        disabled ? 'cursor-not-allowed' : 'cursor-pointer',
      ]"
      @click="onChipClick"
    >
      <LuCheck v-if="showCheck" class="h-[18px] w-[18px] shrink-0" aria-hidden="true" />
      <slot v-else name="icon">
        <component
          :is="icon"
          v-if="icon"
          class="h-[18px] w-[18px] shrink-0"
          :class="selected ? '' : 'text-primary'"
          aria-hidden="true"
        />
      </slot>
      <span class="truncate">
        <slot>{{ label }}</slot>
      </span>
    </button>

    <button
      v-if="showRemove"
      type="button"
      :disabled="disabled"
      :aria-label="t('common.delete')"
      class="md-state mr-0.5 inline-flex h-7 w-7 shrink-0 items-center justify-center rounded-corner-full"
      :class="disabled ? 'cursor-not-allowed' : 'cursor-pointer'"
      @click.stop="onRemoveClick"
    >
      <LuX class="h-4 w-4" aria-hidden="true" />
    </button>
  </span>
</template>

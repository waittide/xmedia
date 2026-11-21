<script setup lang="ts">
import { LuCheck, LuMinus } from 'vue-icons-plus/lu'

const props = withDefaults(
  defineProps<{
    modelValue: boolean
    indeterminate?: boolean
    error?: boolean
    label?: string
    ariaLabel?: string
    description?: string
    disabled?: boolean
  }>(),
  {
    indeterminate: false,
    error: false,
    label: '',
    ariaLabel: '',
    description: '',
    disabled: false,
  },
)

const emit = defineEmits<{
  'update:modelValue': [value: boolean]
  change: [value: boolean]
}>()

function toggle(): void {
  if (props.disabled) return
  const next = props.indeterminate ? true : !props.modelValue
  emit('update:modelValue', next)
  emit('change', next)
}
</script>

<template>
  <button
    type="button"
    role="checkbox"
    :aria-checked="indeterminate ? 'mixed' : modelValue"
    :aria-invalid="error || undefined"
    :aria-label="ariaLabel || (!label ? 'Select' : undefined)"
    :disabled="disabled"
    class="group/cb md-touch-target md-body-small inline-flex cursor-pointer gap-2.5 text-left text-[var(--md-sys-color-on-surface-variant)] select-none transition-colors hover:text-[var(--md-sys-color-on-surface)] focus-visible:outline-none disabled:cursor-not-allowed disabled:text-[color-mix(in_srgb,var(--md-sys-color-on-surface)_38%,transparent)]"
    :class="description ? 'items-start' : 'items-center'"
    @click="toggle"
  >
    <!-- MD3 18dp Checkbox Container + 40dp Circular Touch State Layer -->
    <span
      class="relative flex h-[18px] w-[18px] shrink-0 items-center justify-center rounded-[2px] border-2 transition-colors duration-150 group-focus-visible/cb:outline-3 group-focus-visible/cb:outline-offset-2 group-focus-visible/cb:outline-[var(--md-sys-color-secondary)]"
      :class="[
        description ? 'mt-0.5' : '',
        disabled
          ? modelValue || indeterminate
            ? 'border-transparent bg-[color-mix(in_srgb,var(--md-sys-color-on-surface)_12%,transparent)] text-[color-mix(in_srgb,var(--md-sys-color-on-surface)_38%,transparent)]'
            : 'border-[color-mix(in_srgb,var(--md-sys-color-on-surface)_12%,transparent)] bg-transparent'
          : error
            ? modelValue || indeterminate
              ? 'border-[var(--md-sys-color-error)] bg-[var(--md-sys-color-error)] text-[var(--md-sys-color-on-error)]'
              : 'border-[var(--md-sys-color-error)] bg-transparent'
            : modelValue || indeterminate
              ? 'border-[var(--md-sys-color-primary)] bg-[var(--md-sys-color-primary)] text-[var(--md-sys-color-on-primary)]'
              : 'border-[var(--md-sys-color-on-surface-variant)] bg-transparent group-hover/cb:border-[var(--md-sys-color-on-surface)]',
      ]"
    >
      <span
        v-if="!disabled"
        aria-hidden="true"
        class="pointer-events-none absolute -inset-[11px] h-10 w-10 rounded-full opacity-0 transition-opacity duration-150 group-hover/cb:opacity-[0.08] group-focus-visible/cb:opacity-[0.10] group-active/cb:opacity-[0.12]"
        :class="
          error
            ? 'bg-[var(--md-sys-color-error)]'
            : modelValue || indeterminate
              ? 'bg-[var(--md-sys-color-primary)]'
              : 'bg-[var(--md-sys-color-on-surface)]'
        "
      />
      <LuMinus v-if="indeterminate" class="h-3.5 w-3.5 stroke-[3]" />
      <LuCheck v-else-if="modelValue" class="h-3.5 w-3.5 stroke-[3]" />
    </span>
    <span v-if="description" class="flex min-w-0 flex-col gap-0.5">
      <span
        v-if="label"
        class="font-medium"
        :class="disabled ? '' : 'text-[var(--md-sys-color-on-surface)]'"
      >
        {{ label }}
      </span>
      <span
        class="text-xs leading-relaxed"
        :class="disabled ? '' : 'text-[var(--md-sys-color-on-surface-variant)]'"
      >
        {{ description }}
      </span>
    </span>
    <span v-else-if="label">{{ label }}</span>
    <slot />
  </button>
</template>

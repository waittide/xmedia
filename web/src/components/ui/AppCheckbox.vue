<script setup lang="ts">
import { useI18n } from 'vue-i18n'
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

const { t, te } = useI18n()

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
    :aria-label="ariaLabel || (!label ? (te('common.select') ? t('common.select') : 'Select') : undefined)"
    :disabled="disabled"
    class="group/cb md-touch-target text-body-medium inline-flex cursor-pointer gap-2.5 text-left text-on-surface-variant select-none transition-colors duration-150 ease-standard hover:text-on-surface focus-visible:outline-none disabled:cursor-not-allowed disabled:text-on-surface/38"
    :class="description ? 'items-start' : 'items-center'"
    @click="toggle"
  >
    <!-- MD3 18dp Checkbox Container + 40dp Circular Touch State Layer -->
    <span
      class="relative flex h-[18px] w-[18px] shrink-0 items-center justify-center rounded-[2px] border-2 transition-colors duration-150 ease-standard group-focus-visible/cb:outline-3 group-focus-visible/cb:outline-offset-2 group-focus-visible/cb:outline-secondary"
      :class="[
        description ? 'mt-0.5' : '',
        disabled
          ? modelValue || indeterminate
            ? 'border-transparent bg-on-surface/12 text-on-surface/38'
            : 'border-on-surface/12 bg-transparent'
          : error
            ? modelValue || indeterminate
              ? 'border-error bg-error text-on-error'
              : 'border-error bg-transparent'
            : modelValue || indeterminate
              ? 'border-primary bg-primary text-on-primary'
              : 'border-on-surface-variant bg-transparent group-hover/cb:border-on-surface',
      ]"
    >
      <span
        v-if="!disabled"
        aria-hidden="true"
        class="pointer-events-none absolute -inset-[11px] h-10 w-10 rounded-corner-full opacity-0 transition-opacity duration-150 group-hover/cb:opacity-[0.08] group-focus-visible/cb:opacity-[0.10] group-active/cb:opacity-[0.10]"
        :class="
          error
            ? 'bg-error'
            : modelValue || indeterminate
              ? 'bg-primary'
              : 'bg-on-surface'
        "
      />
      <LuMinus v-if="indeterminate" class="h-3.5 w-3.5 stroke-[3]" aria-hidden="true" />
      <LuCheck v-else-if="modelValue" class="h-3.5 w-3.5 stroke-[3]" aria-hidden="true" />
    </span>
    <span v-if="description" class="flex min-w-0 flex-col gap-0.5">
      <span
        v-if="label"
        class="text-label-large"
        :class="disabled ? '' : 'text-on-surface'"
      >
        {{ label }}
      </span>
      <span
        class="text-body-small"
        :class="disabled ? '' : 'text-on-surface-variant'"
      >
        {{ description }}
      </span>
    </span>
    <span v-else-if="label">{{ label }}</span>
    <slot />
  </button>
</template>

<script setup lang="ts">
import { LuCheck } from 'vue-icons-plus/lu'

const props = withDefaults(
  defineProps<{
    modelValue: boolean
    label?: string
    disabled?: boolean
  }>(),
  {
    label: '',
    disabled: false,
  },
)

const emit = defineEmits<{
  'update:modelValue': [value: boolean]
  change: [value: boolean]
}>()

function toggle(): void {
  if (props.disabled) return
  const next = !props.modelValue
  emit('update:modelValue', next)
  emit('change', next)
}
</script>

<template>
  <button
    type="button"
    role="checkbox"
    :aria-checked="modelValue"
    :disabled="disabled"
    class="group/cb md-body-small inline-flex cursor-pointer items-center gap-2.5 text-[var(--md-sys-color-on-surface-variant)] select-none transition-colors hover:text-[var(--md-sys-color-on-surface)] focus-visible:outline-none disabled:cursor-not-allowed disabled:text-[color-mix(in_srgb,var(--md-sys-color-on-surface)_38%,transparent)]"
    @click="toggle"
  >
    <!-- MD3 18px Checkbox Container + 40px Circular Touch State Layer -->
    <span
      class="relative flex h-[18px] w-[18px] shrink-0 items-center justify-center rounded-[2px] border-2 transition-colors duration-150 group-focus-visible/cb:outline-2 group-focus-visible/cb:outline-offset-2 group-focus-visible/cb:outline-[var(--md-sys-color-primary)]"
      :class="
        disabled
          ? modelValue
            ? 'border-transparent bg-[color-mix(in_srgb,var(--md-sys-color-on-surface)_38%,transparent)] text-[var(--md-sys-color-surface)]'
            : 'border-[color-mix(in_srgb,var(--md-sys-color-on-surface)_38%,transparent)] bg-transparent'
          : modelValue
            ? 'border-[var(--md-sys-color-primary)] bg-[var(--md-sys-color-primary)] text-[var(--md-sys-color-on-primary)]'
            : 'border-[var(--md-sys-color-on-surface-variant)] bg-transparent group-hover/cb:border-[var(--md-sys-color-on-surface)]'
      "
    >
      <span
        v-if="!disabled"
        aria-hidden="true"
        class="pointer-events-none absolute -inset-[11px] h-10 w-10 rounded-full opacity-0 transition-opacity duration-150 group-hover/cb:opacity-[0.08] group-focus-visible/cb:opacity-[0.12] group-active/cb:opacity-[0.12]"
        :class="
          modelValue
            ? 'bg-[var(--md-sys-color-primary)]'
            : 'bg-[var(--md-sys-color-on-surface)]'
        "
      />
      <LuCheck v-if="modelValue" class="h-3.5 w-3.5 stroke-[3]" />
    </span>
    <span v-if="label">{{ label }}</span>
    <slot />
  </button>
</template>

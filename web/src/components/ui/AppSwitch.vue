<script setup lang="ts">
import { LuCheck } from 'vue-icons-plus/lu'

const props = withDefaults(
  defineProps<{
    modelValue: boolean
    label?: string
    ariaLabel?: string
    disabled?: boolean
  }>(),
  {
    label: '',
    ariaLabel: '',
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
    role="switch"
    :aria-checked="modelValue"
    :aria-label="ariaLabel || (!label ? 'Toggle' : undefined)"
    :disabled="disabled"
    class="group/switch md-touch-target md-label-medium inline-flex min-h-10 cursor-pointer items-center gap-2.5 text-[var(--md-sys-color-on-surface)] select-none focus-visible:outline-none disabled:cursor-not-allowed disabled:text-[color-mix(in_srgb,var(--md-sys-color-on-surface)_38%,transparent)]"
    @click="toggle"
  >
    <!-- Refined MD3 Switch Track (44x26dp sleek visual inside 48dp accessible hit area) -->
    <span
      class="relative inline-flex h-[26px] w-11 shrink-0 items-center rounded-full border-[1.5px] transition-colors duration-200 group-focus-visible/switch:outline-3 group-focus-visible/switch:outline-offset-2 group-focus-visible/switch:outline-[var(--md-sys-color-secondary)]"
      :class="
        disabled
          ? modelValue
            ? 'border-transparent bg-[color-mix(in_srgb,var(--md-sys-color-on-surface)_12%,transparent)]'
            : 'border-[color-mix(in_srgb,var(--md-sys-color-on-surface)_12%,transparent)] bg-[color-mix(in_srgb,var(--md-sys-color-surface-container-highest)_12%,transparent)]'
          : modelValue
            ? 'border-[var(--md-sys-color-primary)] bg-[var(--md-sys-color-primary)]'
            : 'border-[var(--md-sys-color-outline)] bg-[var(--md-sys-color-surface-container-high)]'
      "
    >
      <!-- Dynamic Thumb (14dp off / 20dp on) + MD3 State Layer Halo -->
      <span
        class="relative flex items-center justify-center rounded-full transition-all duration-200 ease-[var(--md-sys-motion-easing-standard)]"
        :class="
          modelValue
            ? [
                'h-5 w-5 translate-x-[20px] group-active/switch:scale-105',
                disabled
                  ? 'bg-[var(--md-sys-color-surface)] text-[color-mix(in_srgb,var(--md-sys-color-on-surface)_38%,transparent)]'
                  : 'bg-[var(--md-sys-color-on-primary)] text-[var(--md-sys-color-primary)] shadow-xs',
              ]
            : [
                'h-3.5 w-3.5 translate-x-[4px] group-active/switch:h-4 group-active/switch:w-4',
                disabled
                  ? 'bg-[color-mix(in_srgb,var(--md-sys-color-on-surface)_38%,transparent)]'
                  : 'bg-[var(--md-sys-color-outline)] group-hover/switch:bg-[var(--md-sys-color-on-surface-variant)]',
              ]
        "
      >
        <span
          v-if="!disabled"
          aria-hidden="true"
          class="pointer-events-none absolute -inset-2 rounded-full opacity-0 transition-opacity duration-150 group-hover/switch:opacity-[0.08] group-focus-visible/switch:opacity-[0.10] group-active/switch:opacity-[0.12]"
          :class="
            modelValue
              ? 'bg-[var(--md-sys-color-primary)]'
              : 'bg-[var(--md-sys-color-on-surface)]'
          "
        />
        <LuCheck v-if="modelValue" class="h-3.5 w-3.5 stroke-[2.75]" />
      </span>
    </span>
    <span v-if="label">{{ label }}</span>
    <slot />
  </button>
</template>

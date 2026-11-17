<script setup lang="ts">
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
    role="switch"
    :aria-checked="modelValue"
    :disabled="disabled"
    class="inline-flex items-center gap-2.5 text-xs font-medium text-[var(--ctp-text)] select-none outline-none disabled:opacity-50"
    @click="toggle"
  >
    <span
      class="relative inline-flex h-5 w-9 shrink-0 items-center rounded-full border transition-colors duration-150"
      :class="
        modelValue
          ? 'border-[var(--ctp-accent)] bg-[var(--ctp-accent)]'
          : 'border-[var(--ctp-surface1)] bg-[var(--ctp-mantle)]'
      "
    >
      <span
        class="inline-block h-3.5 w-3.5 transform rounded-full shadow-xs transition-transform duration-150"
        :class="
          modelValue
            ? 'translate-x-4 bg-[var(--ctp-on-accent)]'
            : 'translate-x-0.5 bg-[var(--ctp-subtext0)]'
        "
      />
    </span>
    <span v-if="label">{{ label }}</span>
    <slot />
  </button>
</template>

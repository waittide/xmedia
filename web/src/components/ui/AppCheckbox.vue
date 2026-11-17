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
    class="inline-flex items-center gap-2 text-xs text-[var(--ctp-subtext1)] select-none outline-none transition hover:text-[var(--ctp-text)] disabled:opacity-50"
    @click="toggle"
  >
    <span
      class="flex h-4 w-4 shrink-0 items-center justify-center rounded-[5px] border transition"
      :class="
        modelValue
          ? 'border-[var(--ctp-accent)] bg-[var(--ctp-accent)] text-[var(--ctp-on-accent)] shadow-xs'
          : 'border-[var(--ctp-surface2)] bg-[var(--ctp-canvas)] hover:border-[var(--ctp-accent)]'
      "
    >
      <LuCheck v-if="modelValue" class="h-3 w-3 stroke-[3]" />
    </span>
    <span v-if="label">{{ label }}</span>
    <slot />
  </button>
</template>

<script setup lang="ts">
const props = withDefaults(
  defineProps<{
    checked: boolean
    label?: string
    disabled?: boolean
    ariaLabel?: string
  }>(),
  {
    label: '',
    disabled: false,
    ariaLabel: undefined,
  },
)

const emit = defineEmits<{
  select: []
}>()

function handleSelect(): void {
  if (props.disabled) return
  emit('select')
}
</script>

<template>
  <button
    type="button"
    role="radio"
    :aria-checked="checked"
    :aria-label="ariaLabel || label || undefined"
    :disabled="disabled"
    class="group md-touch-target inline-flex cursor-pointer items-center gap-2.5 text-left outline-none select-none disabled:cursor-not-allowed disabled:opacity-38"
    @click="handleSelect"
  >
    <span
      class="relative flex h-5 w-5 shrink-0 items-center justify-center rounded-full border-2 transition-colors duration-[var(--md-sys-motion-duration-short)] group-focus-visible:outline-3 group-focus-visible:outline-offset-2 group-focus-visible:outline-[var(--md-sys-color-secondary)]"
      :class="
        checked
          ? 'border-[var(--md-sys-color-primary)]'
          : 'border-[var(--md-sys-color-on-surface-variant)] group-hover:border-[var(--md-sys-color-on-surface)]'
      "
    >
      <span
        v-if="checked"
        class="h-2.5 w-2.5 rounded-full bg-[var(--md-sys-color-primary)] transition-transform duration-[var(--md-sys-motion-duration-short)]"
      />
      <span
        v-if="!disabled"
        aria-hidden="true"
        class="pointer-events-none absolute -inset-2.5 rounded-full opacity-0 transition-opacity group-hover:opacity-[0.08] group-focus-visible:opacity-[0.10] group-active:opacity-[0.10]"
        :class="checked ? 'bg-[var(--md-sys-color-primary)]' : 'bg-[var(--md-sys-color-on-surface)]'"
      />
    </span>
    <span v-if="label || $slots.default" class="md-body-medium text-[var(--md-sys-color-on-surface)]">
      <slot>{{ label }}</slot>
    </span>
  </button>
</template>

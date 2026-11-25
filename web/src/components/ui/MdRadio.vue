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
      class="relative flex h-5 w-5 shrink-0 items-center justify-center rounded-corner-full border-2 transition-colors duration-150 ease-standard group-focus-visible:outline-3 group-focus-visible:outline-offset-2 group-focus-visible:outline-secondary"
      :class="
        checked
          ? 'border-primary'
          : 'border-on-surface-variant group-hover:border-on-surface'
      "
    >
      <span
        v-if="checked"
        class="h-2.5 w-2.5 rounded-corner-full bg-primary transition-transform duration-150 ease-standard"
      />
      <span
        v-if="!disabled"
        aria-hidden="true"
        class="pointer-events-none absolute -inset-2.5 rounded-corner-full opacity-0 transition-opacity group-hover:opacity-[0.08] group-focus-visible:opacity-[0.10] group-active:opacity-[0.10]"
        :class="checked ? 'bg-primary' : 'bg-on-surface'"
      />
    </span>
    <span v-if="label || $slots.default" class="text-body-medium text-on-surface">
      <slot>{{ label }}</slot>
    </span>
  </button>
</template>

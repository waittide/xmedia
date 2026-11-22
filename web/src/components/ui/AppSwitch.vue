<script setup lang="ts">
import MdSwitch from './MdSwitch.vue'

withDefaults(
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
</script>

<template>
  <MdSwitch
    :model-value="modelValue"
    :label="label"
    :aria-label="ariaLabel"
    :disabled="disabled"
    @update:model-value="emit('update:modelValue', $event)"
    @change="emit('change', $event)"
  >
    <slot />
  </MdSwitch>
</template>

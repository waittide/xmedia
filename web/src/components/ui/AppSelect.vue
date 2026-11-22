<script setup lang="ts">
import { useAttrs } from 'vue'
import MdSelect, { type SelectOption } from './MdSelect.vue'

export type { SelectOption }

defineOptions({ inheritAttrs: false })

withDefaults(
  defineProps<{
    modelValue: string
    options: SelectOption[]
    placeholder?: string
    variant?: 'field' | 'inline' | 'compact'
    size?: 'sm' | 'md'
    searchable?: boolean
    searchPlaceholder?: string
    disabled?: boolean
    teleportTo?: string | HTMLElement | null
  }>(),
  {
    placeholder: '',
    variant: 'field',
    size: 'md',
    searchable: false,
    searchPlaceholder: '',
    disabled: false,
    teleportTo: null,
  },
)

const emit = defineEmits<{
  'update:modelValue': [value: string]
  change: [value: string]
}>()

const attrs = useAttrs()
</script>

<template>
  <MdSelect
    v-bind="attrs"
    :model-value="modelValue"
    :options="options"
    :placeholder="placeholder"
    :variant="variant"
    :size="size"
    :searchable="searchable"
    :search-placeholder="searchPlaceholder"
    :disabled="disabled"
    :teleport-to="teleportTo"
    @update:model-value="emit('update:modelValue', $event)"
    @change="emit('change', $event)"
  />
</template>

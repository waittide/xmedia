<script setup lang="ts">
import { MdTextField } from '@/components/ui'

export interface RuleFieldDef {
  key: string
  label: string
  hint?: string
  placeholder?: string
  multiline?: boolean
  rows?: number
  fullWidth?: boolean
}

const props = defineProps<{
  title: string
  fields: RuleFieldDef[]
  modelValue: Record<string, string>
}>()

const emit = defineEmits<{
  'update:modelValue': [value: Record<string, string>]
}>()

function onFieldInput(key: string, value: string): void {
  emit('update:modelValue', {
    ...props.modelValue,
    [key]: value,
  })
}
</script>

<template>
  <section class="flex flex-col gap-3 rounded-corner-lg bg-surface-container p-4">
    <h4 class="text-title-small text-on-surface">
      {{ title }}
    </h4>
    <div class="grid grid-cols-1 gap-3 sm:grid-cols-2">
      <div
        v-for="field in fields"
        :key="field.key"
        :class="field.fullWidth ? 'sm:col-span-2' : ''"
      >
        <MdTextField
          :model-value="modelValue[field.key] ?? ''"
          :label="field.hint ? `${field.label} (${field.hint})` : field.label"
          :placeholder="field.placeholder"
          :multiline="field.multiline"
          :rows="field.rows ?? 2"
          size="sm"
          class="font-mono"
          @update:model-value="onFieldInput(field.key, $event)"
        />
      </div>
    </div>
  </section>
</template>

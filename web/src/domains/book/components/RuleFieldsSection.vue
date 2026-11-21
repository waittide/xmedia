<script setup lang="ts">
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
  <section class="space-y-3 rounded-2xl bg-[var(--md-sys-color-surface-container)] p-4">
    <h4 class="text-xs font-semibold text-[var(--md-sys-color-on-surface)]">
      {{ title }}
    </h4>
    <div class="grid grid-cols-1 gap-3 sm:grid-cols-2">
      <label
        v-for="field in fields"
        :key="field.key"
        class="block space-y-1"
        :class="field.fullWidth ? 'sm:col-span-2' : ''"
      >
        <span class="text-xs text-[var(--md-sys-color-on-surface-variant)]">
          {{ field.label }}
          <span v-if="field.hint" class="font-mono opacity-65">{{ field.hint }}</span>
        </span>
        <textarea
          v-if="field.multiline"
          :value="modelValue[field.key] ?? ''"
          :rows="field.rows ?? 2"
          :placeholder="field.placeholder"
          spellcheck="false"
          class="md-input w-full py-2 font-mono text-xs leading-5"
          @input="onFieldInput(field.key, ($event.target as HTMLTextAreaElement).value)"
        />
        <input
          v-else
          :value="modelValue[field.key] ?? ''"
          type="text"
          :placeholder="field.placeholder"
          spellcheck="false"
          class="md-input w-full py-2 font-mono text-xs"
          @input="onFieldInput(field.key, ($event.target as HTMLInputElement).value)"
        />
      </label>
    </div>
  </section>
</template>

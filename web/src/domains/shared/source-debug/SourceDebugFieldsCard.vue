<script setup lang="ts">
import type { SourceDebugFieldRow } from './useSourceDebugHelpers'

defineProps<{
  fields: readonly SourceDebugFieldRow[]
  emptyLabel: string
}>()
</script>

<template>
  <div class="space-y-1.5 rounded-corner-lg bg-surface-container-low p-2">
    <div
      v-for="field in fields"
      :key="field.label"
      class="grid grid-cols-1 gap-1 rounded-corner-md bg-surface-container px-3.5 py-2.5 sm:grid-cols-[11rem_1fr] sm:gap-3"
    >
      <div class="font-mono text-label-medium text-on-surface-variant">
        {{ field.label }}
      </div>
      <div
        class="min-w-0 text-body-small"
        :class="[
          field.value ? 'text-on-surface' : 'italic text-on-surface-variant/65',
          field.mono ? 'break-all font-mono' : '',
          field.multiline ? 'leading-relaxed whitespace-pre-wrap' : 'break-words',
        ]"
      >
        {{ field.value || emptyLabel }}
      </div>
    </div>
  </div>
</template>

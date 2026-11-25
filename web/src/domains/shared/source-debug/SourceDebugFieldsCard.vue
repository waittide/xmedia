<script setup lang="ts">
import type { SourceDebugFieldRow } from './useSourceDebugHelpers'

defineProps<{
  fields: readonly SourceDebugFieldRow[]
  emptyLabel: string
}>()
</script>

<template>
  <div class="overflow-hidden rounded-corner-lg bg-surface-container">
    <div
      v-for="(field, fIdx) in fields"
      :key="field.label"
      class="grid grid-cols-1 gap-1 px-4 py-3 sm:grid-cols-[11rem_1fr] sm:gap-3"
      :class="fIdx > 0 ? 'border-t border-outline-variant/50' : ''"
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

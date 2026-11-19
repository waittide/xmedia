<script setup lang="ts" generic="T extends string">
import type { Component } from 'vue'
import { LuCheck } from 'vue-icons-plus/lu'

export interface SegmentedOption<V extends string = string> {
  value: V
  label: string
  icon?: Component
  title?: string
}

withDefaults(
  defineProps<{
    modelValue: T
    options: readonly SegmentedOption<T>[]
    size?: 'sm' | 'md'
    showCheck?: boolean
  }>(),
  {
    size: 'sm',
    showCheck: false,
  },
)

const emit = defineEmits<{
  'update:modelValue': [value: T]
}>()
</script>

<template>
  <div
    role="radiogroup"
    class="inline-flex items-center overflow-hidden rounded-full border border-[var(--md-sys-color-outline-variant)] bg-[var(--md-sys-color-surface-container-low)]"
  >
    <button
      v-for="(opt, idx) in options"
      :key="opt.value"
      type="button"
      role="radio"
      :aria-checked="modelValue === opt.value"
      :title="opt.title || opt.label"
      class="md-state inline-flex items-center justify-center gap-1.5 font-medium transition"
      :class="[
        size === 'sm' ? 'h-8 px-3 text-xs' : 'h-9 px-4 text-[13px]',
        idx > 0 ? 'border-l border-[var(--md-sys-color-outline-variant)]' : '',
        modelValue === opt.value
          ? 'bg-[var(--md-sys-color-secondary-container)] font-semibold text-[var(--md-sys-color-on-secondary-container)]'
          : 'text-[var(--md-sys-color-on-surface-variant)] hover:text-[var(--md-sys-color-on-surface)]',
      ]"
      @click="emit('update:modelValue', opt.value)"
    >
      <LuCheck
        v-if="showCheck && modelValue === opt.value"
        class="h-3.5 w-3.5 shrink-0"
      />
      <component :is="opt.icon" v-else-if="opt.icon" class="h-3.5 w-3.5 shrink-0" />
      <span>{{ opt.label }}</span>
    </button>
  </div>
</template>

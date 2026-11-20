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
    size: 'md',
    showCheck: true,
  },
)

const emit = defineEmits<{
  'update:modelValue': [value: T]
}>()
</script>

<template>
  <div
    role="radiogroup"
    class="inline-flex items-center overflow-hidden rounded-full border border-[var(--md-sys-color-outline)] bg-transparent"
  >
    <button
      v-for="(opt, idx) in options"
      :key="opt.value"
      type="button"
      role="radio"
      :aria-checked="modelValue === opt.value"
      :title="opt.title || opt.label"
      class="md-state inline-flex cursor-pointer items-center justify-center gap-1.5 font-medium transition focus-visible:z-10"
      :class="[
        size === 'sm' ? 'h-9 px-3.5 text-xs' : 'md-label-large h-10 px-4',
        idx > 0 ? 'border-l border-[var(--md-sys-color-outline)]' : '',
        modelValue === opt.value
          ? 'bg-[var(--md-sys-color-secondary-container)] text-[var(--md-sys-color-on-secondary-container)]'
          : 'text-[var(--md-sys-color-on-surface)]',
      ]"
      @click="emit('update:modelValue', opt.value)"
    >
      <LuCheck
        v-if="showCheck && modelValue === opt.value"
        class="h-4 w-4 shrink-0"
      />
      <component :is="opt.icon" v-else-if="opt.icon" class="h-4 w-4 shrink-0" />
      <span>{{ opt.label }}</span>
    </button>
  </div>
</template>

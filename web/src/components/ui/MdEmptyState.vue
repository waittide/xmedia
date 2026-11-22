<script setup lang="ts">
import type { Component } from 'vue'

withDefaults(
  defineProps<{
    icon?: Component
    title: string
    description?: string
    compact?: boolean
  }>(),
  {
    icon: undefined,
    description: '',
    compact: false,
  },
)
</script>

<template>
  <div
    class="flex flex-col items-center justify-center text-center"
    :class="compact ? 'px-4 py-8' : 'px-6 py-16'"
  >
    <div
      v-if="icon || $slots.icon"
      class="mb-4 flex h-14 w-14 items-center justify-center rounded-corner-xl bg-surface-container-high text-on-surface-variant"
      aria-hidden="true"
    >
      <slot name="icon">
        <component :is="icon" class="h-6 w-6" />
      </slot>
    </div>

    <h3 class="text-title-medium text-on-surface">
      {{ title }}
    </h3>

    <p
      v-if="description"
      class="text-body-medium mt-1.5 max-w-md text-on-surface-variant"
    >
      {{ description }}
    </p>

    <div v-if="$slots.action || $slots.default" class="mt-5 flex flex-wrap items-center justify-center gap-3">
      <slot name="action" />
      <slot />
    </div>
  </div>
</template>

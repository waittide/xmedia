<script setup lang="ts">
import { ref, watch, type Component } from 'vue'

const props = withDefaults(
  defineProps<{
    src?: string | null
    alt?: string
    aspectClass?: string
    rounded?: string
    bgClass?: string
    imgClass?: string
    loading?: 'lazy' | 'eager'
    fallbackIcon?: Component
    fallbackIconClass?: string
  }>(),
  {
    src: null,
    alt: '',
    aspectClass: 'aspect-square',
    rounded: 'rounded-2xl',
    bgClass:
      'bg-gradient-to-br from-[var(--md-sys-color-surface-container-high)] via-[var(--md-sys-color-surface-container)] to-[var(--md-sys-color-primary-container)]/45',
    imgClass: 'h-full w-full object-cover',
    loading: 'lazy',
    fallbackIcon: undefined,
    fallbackIconClass: 'h-2/5 w-2/5 text-[var(--md-sys-color-on-surface-variant)]/55',
  },
)

const failed = ref(false)

watch(
  () => props.src,
  () => {
    failed.value = false
  },
)
</script>

<template>
  <div
    class="relative flex items-center justify-center overflow-hidden select-none"
    :class="[aspectClass, rounded, bgClass]"
  >
    <img
      v-if="src && !failed"
      :src="src"
      :alt="alt"
      :loading="loading"
      decoding="async"
      :class="imgClass"
      @error="failed = true"
    />
    <slot v-else name="fallback">
      <component :is="fallbackIcon" v-if="fallbackIcon" :class="fallbackIconClass" />
    </slot>
    <slot />
  </div>
</template>

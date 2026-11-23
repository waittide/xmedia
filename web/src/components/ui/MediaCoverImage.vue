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
    rounded: 'rounded-corner-lg',
    bgClass:
      'bg-gradient-to-br from-surface-container-high via-surface-container to-primary-container/45',
    imgClass: 'h-full w-full object-cover',
    loading: 'lazy',
    fallbackIcon: undefined,
    fallbackIconClass: 'h-2/5 w-2/5 text-on-surface-variant/55',
  },
)

const loaded = ref(false)
const failed = ref(false)

watch(
  () => props.src,
  () => {
    loaded.value = false
    failed.value = false
  },
)
</script>

<template>
  <div
    class="relative flex items-center justify-center overflow-hidden select-none"
    :class="[aspectClass, rounded, bgClass]"
  >
    <div
      v-if="!src || failed || !loaded"
      class="pointer-events-none absolute inset-0 flex items-center justify-center"
    >
      <slot name="fallback">
        <component :is="fallbackIcon" v-if="fallbackIcon" :class="fallbackIconClass" />
      </slot>
    </div>
    <img
      v-if="src && !failed"
      :src="src"
      :alt="alt"
      :loading="loading"
      decoding="async"
      class="relative transition-opacity duration-200"
      :class="[imgClass, loaded ? 'opacity-100' : 'opacity-0']"
      @load="loaded = true"
      @error="failed = true"
    />
    <slot />
  </div>
</template>

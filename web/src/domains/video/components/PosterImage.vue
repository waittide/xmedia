<script setup lang="ts">
import { computed, ref, watch } from 'vue'
import { LuFilm } from 'vue-icons-plus/lu'
import { buildPosterUrl } from '../api'

const props = withDefaults(
  defineProps<{
    posterKey?: string | null
    size?: number
    alt?: string
    aspect?: 'video' | 'poster'
    rounded?: string
  }>(),
  {
    posterKey: null,
    size: 480,
    alt: '',
    aspect: 'video',
    rounded: 'rounded-2xl',
  },
)

const failed = ref(false)

watch(
  () => props.posterKey,
  () => {
    failed.value = false
  },
)

const src = computed(() => {
  if (!props.posterKey || failed.value) return null
  return buildPosterUrl(props.posterKey, props.size)
})
</script>

<template>
  <div
    class="relative flex items-center justify-center overflow-hidden bg-gradient-to-br from-[var(--md-sys-color-surface-container-highest)] via-[var(--md-sys-color-surface-container)] to-[var(--md-sys-color-primary-container)] select-none"
    :class="[aspect === 'poster' ? 'aspect-[2/3]' : 'aspect-video', rounded]"
  >
    <img
      v-if="src"
      :src="src"
      :alt="alt"
      loading="lazy"
      decoding="async"
      class="h-full w-full object-cover"
      @error="failed = true"
    />
    <LuFilm v-else class="h-1/3 w-1/3 text-[var(--md-sys-color-on-surface-variant)]/50" />
    <slot />
  </div>
</template>

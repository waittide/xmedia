<script setup lang="ts">
import { computed } from 'vue'
import { LuFilm } from 'vue-icons-plus/lu'
import MediaCoverImage from '@/components/ui/MediaCoverImage.vue'
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

const src = computed(() => (props.posterKey ? buildPosterUrl(props.posterKey, props.size) : null))
</script>

<template>
  <MediaCoverImage
    :src="src"
    :alt="alt"
    :aspect-class="aspect === 'poster' ? 'aspect-[2/3]' : 'aspect-video'"
    :rounded="rounded"
    bg-class="bg-gradient-to-br from-[var(--md-sys-color-surface-container-highest)] via-[var(--md-sys-color-surface-container)] to-[var(--md-sys-color-primary-container)]"
    :fallback-icon="LuFilm"
    fallback-icon-class="h-1/3 w-1/3 text-[var(--md-sys-color-on-surface-variant)]/50"
  >
    <slot />
  </MediaCoverImage>
</template>

<script setup lang="ts">
import { computed } from 'vue'
import { LuDisc3 } from 'vue-icons-plus/lu'
import MediaCoverImage from '@/components/ui/MediaCoverImage.vue'
import { buildCoverUrl } from '../api'

const props = withDefaults(
  defineProps<{
    coverId?: string | null
    hasCover?: boolean
    src?: string | null
    size?: number
    alt?: string
    rounded?: string
  }>(),
  {
    coverId: null,
    hasCover: true,
    src: undefined,
    size: 300,
    alt: '',
    rounded: 'rounded-2xl',
  },
)

const resolvedSrc = computed(() => {
  if (props.src !== undefined) {
    const trimmed = props.src?.trim()
    return trimmed ? trimmed : null
  }
  return props.hasCover && props.coverId ? buildCoverUrl(props.coverId, props.size) : null
})
</script>

<template>
  <MediaCoverImage :src="resolvedSrc" :alt="alt" :rounded="rounded" :fallback-icon="LuDisc3">
    <slot />
  </MediaCoverImage>
</template>

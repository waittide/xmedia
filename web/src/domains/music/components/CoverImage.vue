<script setup lang="ts">
import { computed, ref, watch } from 'vue'
import { LuDisc3 } from 'vue-icons-plus/lu'
import { buildCoverUrl } from '../api'

const props = withDefaults(
  defineProps<{
    coverId?: string | null
    hasCover?: boolean
    size?: number
    alt?: string
    rounded?: string
  }>(),
  {
    coverId: null,
    hasCover: true,
    size: 300,
    alt: '',
    rounded: 'rounded-2xl',
  },
)

const failed = ref(false)

watch(
  () => props.coverId,
  () => {
    failed.value = false
  },
)

const src = computed(() => {
  if (!props.hasCover || !props.coverId || failed.value) return null
  return buildCoverUrl(props.coverId, props.size)
})
</script>

<template>
  <div
    class="relative flex aspect-square items-center justify-center overflow-hidden bg-gradient-to-br from-[var(--md-sys-color-surface-container-high)] via-[var(--md-sys-color-surface-container)] to-[var(--md-sys-color-primary-container)]/45 select-none"
    :class="rounded"
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
    <LuDisc3 v-else class="h-2/5 w-2/5 text-[var(--md-sys-color-on-surface-variant)]/55" />
    <slot />
  </div>
</template>

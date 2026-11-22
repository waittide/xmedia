<script setup lang="ts">
import { computed } from 'vue'

export type MdSkeletonVariant = 'text' | 'rect' | 'circle' | 'card'
export type MdSkeletonAspect = 'square' | 'poster' | 'video'

const props = withDefaults(
  defineProps<{
    variant?: MdSkeletonVariant
    aspect?: MdSkeletonAspect
    width?: string
    height?: string
    lines?: number
  }>(),
  {
    variant: 'rect',
    aspect: undefined,
    width: undefined,
    height: undefined,
    lines: 1,
  },
)

const aspectClass = computed(() => {
  switch (props.aspect) {
    case 'square':
      return 'aspect-square'
    case 'poster':
      return 'aspect-[2/3]'
    case 'video':
      return 'aspect-video'
    default:
      return ''
  }
})

const shapeClass = computed(() => {
  switch (props.variant) {
    case 'circle':
      return 'rounded-corner-full'
    case 'text':
      return 'h-4 rounded-corner-xs'
    case 'card':
      return 'rounded-corner-md'
    case 'rect':
    default:
      return 'rounded-corner-sm'
  }
})

const boxStyle = computed(() => {
  const style: Record<string, string> = {}
  if (props.width) style.width = props.width
  if (props.height) style.height = props.height
  return style
})
</script>

<template>
  <div v-if="variant === 'text' && lines > 1" class="space-y-2" aria-hidden="true">
    <div
      v-for="idx in lines"
      :key="idx"
      class="animate-pulse rounded-corner-xs bg-surface-container-high h-4"
      :class="idx === lines ? 'w-2/3' : 'w-full'"
    />
  </div>

  <div v-else-if="variant === 'card'" class="space-y-2.5" aria-hidden="true">
    <div
      class="w-full animate-pulse rounded-corner-md bg-surface-container-high"
      :class="aspectClass || 'aspect-[2/3]'"
      :style="boxStyle"
    />
    <div class="h-4 w-4/5 animate-pulse rounded-corner-xs bg-surface-container-high" />
    <div class="h-3 w-1/2 animate-pulse rounded-corner-xs bg-surface-container-high" />
  </div>

  <div
    v-else
    aria-hidden="true"
    class="animate-pulse bg-surface-container-high"
    :class="[shapeClass, aspectClass]"
    :style="boxStyle"
  />
</template>

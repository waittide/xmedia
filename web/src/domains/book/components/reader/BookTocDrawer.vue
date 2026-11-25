<script setup lang="ts">
import TocTree from '@/domains/book/components/TocTree.vue'
import type { TocItem } from '@/domains/book/types'

withDefaults(
  defineProps<{
    items: TocItem[]
    activeHref?: string | null
    searchable?: boolean
  }>(),
  {
    activeHref: null,
    searchable: true,
  },
)

const emit = defineEmits<{
  select: [href: string]
}>()
</script>

<template>
  <TocTree
    :items="items"
    :active-href="activeHref"
    :searchable="searchable"
    @select="emit('select', $event)"
  />
</template>

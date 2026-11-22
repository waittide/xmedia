<script setup lang="ts">
import OnlineSourceManagerPanel from '@/components/online-sources/OnlineSourceManagerPanel.vue'
import { bookSourceAdapter, type BookSourceSummaryDto } from '../onlineApi'
import OnlineSourceDebugSheet from './OnlineSourceDebugSheet.vue'
import OnlineSourceEditSheet from './OnlineSourceEditSheet.vue'

defineProps<{
  sources: BookSourceSummaryDto[]
}>()

const emit = defineEmits<{
  refresh: []
}>()
</script>

<template>
  <OnlineSourceManagerPanel
    :sources="sources"
    :adapter="bookSourceAdapter"
    @refresh="emit('refresh')"
  >
    <template #edit-sheet="{ open, source, close, saved, savedAndDebug }">
      <OnlineSourceEditSheet
        :open="open"
        :source="source"
        @close="close"
        @saved="saved"
        @saved-and-debug="savedAndDebug"
      />
    </template>

    <template #debug-sheet="{ open, source, close, tested }">
      <OnlineSourceDebugSheet
        :open="open"
        :source="source"
        @close="close"
        @tested="tested"
      />
    </template>
  </OnlineSourceManagerPanel>
</template>

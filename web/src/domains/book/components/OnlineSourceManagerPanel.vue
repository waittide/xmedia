<script setup lang="ts">
import OnlineSourceManagerPanel from '@/components/online-sources/OnlineSourceManagerPanel.vue'
import type { OnlineSourceManagerAdapter } from '@/components/online-sources/types'
import { onlineBookApi, type BookSourceSummaryDto } from '../onlineApi'
import OnlineSourceDebugSheet from './OnlineSourceDebugSheet.vue'
import OnlineSourceEditSheet from './OnlineSourceEditSheet.vue'

defineProps<{
  sources: BookSourceSummaryDto[]
}>()

const emit = defineEmits<{
  refresh: []
}>()

const bookAdapter: OnlineSourceManagerAdapter<BookSourceSummaryDto> = {
  domain: 'book',
  exportFilePrefix: 'xmedia-book-sources',
  searchRoutePath: '/book/catalog',
  listSources: onlineBookApi.listSources,
  importSources: onlineBookApi.importSources,
  reorderSources: onlineBookApi.reorderSources,
  batchSources: onlineBookApi.batchSources,
  exportSourcesBatch: onlineBookApi.exportSourcesBatch,
  updateSource: onlineBookApi.updateSource,
  deleteSource: onlineBookApi.deleteSource,
  exportSourceUrl: onlineBookApi.exportSourceUrl,
  fetchSourceJson: onlineBookApi.fetchSourceJson,
  getSourceVariable: onlineBookApi.getSourceVariable,
  runLoginAction: onlineBookApi.runLoginAction,
}
</script>

<template>
  <OnlineSourceManagerPanel
    :sources="sources"
    :adapter="bookAdapter"
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

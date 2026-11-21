<script setup lang="ts">
import { computed, onMounted, reactive, watch } from 'vue'
import { useI18n } from 'vue-i18n'
import { LuLoader2, LuLock } from 'vue-icons-plus/lu'
import OnlineSourceManagerPanel from '@/components/online-sources/OnlineSourceManagerPanel.vue'
import {
  extractApiError,
  type OnlineSourceSummaryDto,
} from '@/components/online-sources/types'
import BookOnlineSourceManagerPanel from '@/domains/book/components/OnlineSourceManagerPanel.vue'
import { onlineBookApi, type BookSourceSummaryDto } from '@/domains/book/onlineApi'
import ComicSourceDebugSheet from '@/domains/comic/components/ComicSourceDebugSheet.vue'
import ComicSourceEditSheet from '@/domains/comic/components/ComicSourceEditSheet.vue'
import { comicSourceAdapter, type ComicSourceSummaryDto } from '@/domains/comic/onlineApi'
import MusicSourceDebugSheet from '@/domains/music/components/MusicSourceDebugSheet.vue'
import MusicSourceEditSheet from '@/domains/music/components/MusicSourceEditSheet.vue'
import { musicSourceAdapter, type MusicSourceSummaryDto } from '@/domains/music/onlineApi'
import VideoSourceDebugSheet from '@/domains/video/components/VideoSourceDebugSheet.vue'
import VideoSourceEditSheet from '@/domains/video/components/VideoSourceEditSheet.vue'
import { videoSourceAdapter, type VideoSourceSummaryDto } from '@/domains/video/onlineApi'
import { useAuthStore } from '@/stores/auth'
import type { MediaKind } from '@/types/api'

const props = defineProps<{
  domain: MediaKind
}>()

const emit = defineEmits<{
  error: [message: string]
}>()

const { t } = useI18n()
const authStore = useAuthStore()

const sourcesByDomain = reactive<Record<MediaKind, OnlineSourceSummaryDto[]>>({
  book: [],
  comic: [],
  music: [],
  video: [],
})

const loadingByDomain = reactive<Record<MediaKind, boolean>>({
  book: false,
  comic: false,
  music: false,
  video: false,
})

const loadedByDomain = reactive<Record<MediaKind, boolean>>({
  book: false,
  comic: false,
  music: false,
  video: false,
})

const isActiveLoading = computed(() => loadingByDomain[props.domain])
const isActiveLoaded = computed(() => loadedByDomain[props.domain])

async function fetchDomainSources(targetDomain: MediaKind): Promise<OnlineSourceSummaryDto[]> {
  if (targetDomain === 'book') return onlineBookApi.listSources()
  if (targetDomain === 'comic') return comicSourceAdapter.listSources()
  if (targetDomain === 'music') return musicSourceAdapter.listSources()
  return videoSourceAdapter.listSources()
}

async function refreshDomainSources(targetDomain: MediaKind = props.domain): Promise<void> {
  if (!authStore.isAdmin) return
  emit('error', '')
  loadingByDomain[targetDomain] = true
  try {
    sourcesByDomain[targetDomain] = await fetchDomainSources(targetDomain)
    loadedByDomain[targetDomain] = true
  } catch (err) {
    emit('error', extractApiError(err))
  } finally {
    loadingByDomain[targetDomain] = false
  }
}

onMounted(() => {
  if (authStore.isAdmin) {
    void refreshDomainSources(props.domain)
  }
})

watch(
  () => props.domain,
  (next) => {
    if (authStore.isAdmin && !loadedByDomain[next] && !loadingByDomain[next]) {
      void refreshDomainSources(next)
    }
  },
)
</script>

<template>
  <!-- Top-level Admin Gate across ALL 4 domains -->
  <section v-if="!authStore.isAdmin" class="md-card-outlined px-6 py-14 text-center">
    <div
      class="mx-auto mb-3 flex h-12 w-12 items-center justify-center rounded-2xl bg-[var(--md-sys-color-surface-container-high)] text-[var(--md-sys-color-on-surface-variant)]"
    >
      <LuLock class="h-5 w-5" />
    </div>
    <p class="md-title-small text-[var(--md-sys-color-on-surface)]">
      {{ t('settings.dataSourcesAdminOnly') }}
    </p>
  </section>

  <div
    v-else-if="isActiveLoading && !isActiveLoaded"
    class="flex items-center justify-center py-16 text-xs text-[var(--md-sys-color-on-surface-variant)]"
  >
    <LuLoader2 class="mr-2 h-4 w-4 animate-spin" />
    <span>{{ t('common.loading') }}</span>
  </div>

  <!-- Book Domain: Preserves Legado Book Source Edit & Debug Sheets -->
  <BookOnlineSourceManagerPanel
    v-else-if="domain === 'book'"
    :sources="sourcesByDomain.book as BookSourceSummaryDto[]"
    @refresh="refreshDomainSources('book')"
  />

  <!-- Comic Domain -->
  <OnlineSourceManagerPanel
    v-else-if="domain === 'comic'"
    :sources="sourcesByDomain.comic as ComicSourceSummaryDto[]"
    :adapter="comicSourceAdapter"
    @refresh="refreshDomainSources('comic')"
  >
    <template #edit-sheet="{ open, source, close, saved, savedAndDebug }">
      <ComicSourceEditSheet
        :open="open"
        :source="source"
        @close="close"
        @saved="saved"
        @saved-and-debug="savedAndDebug"
      />
    </template>
    <template #debug-sheet="{ open, source, close, tested }">
      <ComicSourceDebugSheet
        :open="open"
        :source="source"
        @close="close"
        @tested="tested"
      />
    </template>
  </OnlineSourceManagerPanel>

  <!-- Music Domain -->
  <OnlineSourceManagerPanel
    v-else-if="domain === 'music'"
    :sources="sourcesByDomain.music as MusicSourceSummaryDto[]"
    :adapter="musicSourceAdapter"
    @refresh="refreshDomainSources('music')"
  >
    <template #edit-sheet="{ open, source, close, saved, savedAndDebug }">
      <MusicSourceEditSheet
        :open="open"
        :source="source"
        @close="close"
        @saved="saved"
        @saved-and-debug="savedAndDebug"
      />
    </template>
    <template #debug-sheet="{ open, source, close, tested }">
      <MusicSourceDebugSheet
        :open="open"
        :source="source"
        @close="close"
        @tested="tested"
      />
    </template>
  </OnlineSourceManagerPanel>

  <!-- Video Domain -->
  <OnlineSourceManagerPanel
    v-else-if="domain === 'video'"
    :sources="sourcesByDomain.video as VideoSourceSummaryDto[]"
    :adapter="videoSourceAdapter"
    @refresh="refreshDomainSources('video')"
  >
    <template #edit-sheet="{ open, source, close, saved, savedAndDebug }">
      <VideoSourceEditSheet
        :open="open"
        :source="source"
        @close="close"
        @saved="saved"
        @saved-and-debug="savedAndDebug"
      />
    </template>
    <template #debug-sheet="{ open, source, close, tested }">
      <VideoSourceDebugSheet
        :open="open"
        :source="source"
        @close="close"
        @tested="tested"
      />
    </template>
  </OnlineSourceManagerPanel>
</template>

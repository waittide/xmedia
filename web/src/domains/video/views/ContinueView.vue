<script setup lang="ts">
import { onMounted, ref, watch } from 'vue'
import { useRouter } from 'vue-router'
import { LuClock, LuPlay } from 'vue-icons-plus/lu'
import RefreshButton from '@/components/ui/RefreshButton.vue'
import { api } from '../api'
import VideoCard from '../components/VideoCard.vue'
import VideoMetadataModal from '../components/VideoMetadataModal.vue'
import { useVideoI18n as useI18n } from '../i18n'
import { useVideoStore } from '../store'
import type { VideoSummaryDto } from '../types'

const { t } = useI18n()
const router = useRouter()
const videoStore = useVideoStore()

const loading = ref(true)
const items = ref<VideoSummaryDto[]>([])
const editingVideo = ref<VideoSummaryDto | null>(null)

async function loadContinue(): Promise<void> {
  loading.value = true
  try {
    const home = await api.homeOverview()
    items.value = home.continue_watching
  } finally {
    loading.value = false
  }
}

onMounted(loadContinue)
watch(() => videoStore.catalogVersion, loadContinue)
</script>

<template>
  <div class="mx-auto max-w-7xl space-y-8 px-6 py-7 sm:px-8">
    <div class="flex flex-wrap items-center justify-between gap-4">
      <div>
        <h1 class="text-2xl font-extrabold tracking-tight text-[var(--md-sys-color-on-surface)]">
          {{ t('continueView.title') }}
        </h1>
        <p class="mt-0.5 text-xs text-[var(--md-sys-color-on-surface-variant)]">
          {{ t('continueView.subtitle') }}
        </p>
      </div>

      <RefreshButton size="md" :action="loadContinue" :busy="loading" />
    </div>

    <div
      v-if="!loading && items.length === 0"
      class="md-card-filled flex flex-col items-center justify-center rounded-3xl px-6 py-16 text-center"
    >
      <div
        class="mb-4 flex h-14 w-14 items-center justify-center rounded-2xl bg-[var(--md-sys-color-primary-container)] text-[var(--md-sys-color-on-primary-container)]"
      >
        <LuClock class="h-7 w-7" />
      </div>
      <h2 class="text-base font-bold text-[var(--md-sys-color-on-surface)]">
        {{ t('continueView.emptyTitle') }}
      </h2>
      <p class="mt-1.5 max-w-md text-xs leading-relaxed text-[var(--md-sys-color-on-surface-variant)]">
        {{ t('continueView.emptyDesc') }}
      </p>
      <button
        type="button"
        class="md-btn md-btn-filled md-state mt-5"
        @click="router.push('/video/catalog')"
      >
        <LuPlay class="h-3.5 w-3.5 fill-current" />
        <span>{{ t('continueView.browseCatalog') }}</span>
      </button>
    </div>

    <div v-else class="grid grid-cols-1 gap-3.5 sm:grid-cols-2 md:grid-cols-3 lg:grid-cols-4">
      <VideoCard
        v-for="v in items"
        :key="v.id"
        :video="v"
        @edit="editingVideo = $event"
        @deleted="loadContinue"
      />
    </div>

    <VideoMetadataModal
      :open="editingVideo !== null"
      :video="editingVideo"
      @close="editingVideo = null"
      @saved="loadContinue"
    />
  </div>
</template>

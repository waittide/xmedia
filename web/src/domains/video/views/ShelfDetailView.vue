<script setup lang="ts">
import { onMounted, ref, watch } from 'vue'
import { useRoute, useRouter } from 'vue-router'
import { LuPlay, LuPencil, LuTrash2, LuFolderHeart } from 'vue-icons-plus/lu'
import { useDialogStore } from '@/stores/dialog'
import RefreshButton from '@/components/ui/RefreshButton.vue'
import { api } from '../api'
import PosterImage from '../components/PosterImage.vue'
import VideoCard from '../components/VideoCard.vue'
import VideoMetadataModal from '../components/VideoMetadataModal.vue'
import { useVideoI18n as useI18n } from '../i18n'
import { useVideoStore } from '../store'
import type { VideoShelfDetailDto, VideoSummaryDto } from '../types'

const { t } = useI18n()
const route = useRoute()
const router = useRouter()
const dialogStore = useDialogStore()
const videoStore = useVideoStore()

const detail = ref<VideoShelfDetailDto | null>(null)
const editingVideo = ref<VideoSummaryDto | null>(null)

async function loadShelf(): Promise<void> {
  const id = String(route.params.id || '')
  if (!id) return
  detail.value = await api.getShelf(id)
}

async function handleRename(): Promise<void> {
  if (!detail.value) return
  const next = await dialogStore.prompt({
    title: t('shelf.renameTitle'),
    message: t('shelf.renamePrompt'),
    initialValue: detail.value.shelf.name,
  })
  if (!next || !next.trim()) return
  detail.value = await api.updateShelf(detail.value.shelf.id, {
    name: next.trim(),
    description: detail.value.shelf.description,
    video_ids: detail.value.videos.map((v) => v.id),
  })
  await videoStore.refreshSidebar()
}

async function handleRemoveFromShelf(videoId: string): Promise<void> {
  if (!detail.value) return
  const remaining = detail.value.videos.filter((v) => v.id !== videoId).map((v) => v.id)
  detail.value = await api.updateShelf(detail.value.shelf.id, {
    name: detail.value.shelf.name,
    description: detail.value.shelf.description,
    video_ids: remaining,
  })
  await videoStore.refreshSidebar()
}

async function handleDeleteShelf(): Promise<void> {
  if (!detail.value) return
  const ok = await dialogStore.confirm({
    title: t('shelf.deleteTitle'),
    message: t('shelf.confirmDelete', { name: detail.value.shelf.name }),
    danger: true,
  })
  if (!ok) return
  await api.deleteShelf(detail.value.shelf.id)
  await videoStore.refreshSidebar()
  await router.replace('/video')
}

onMounted(loadShelf)
watch(() => route.params.id, loadShelf)
</script>

<template>
  <div v-if="detail" class="mx-auto max-w-6xl space-y-7 px-6 py-8 sm:px-8">
    <div class="flex flex-col items-start gap-6 sm:flex-row sm:items-end">
      <PosterImage
        v-if="detail.shelf.poster_key"
        :poster-key="detail.shelf.poster_key"
        :size="480"
        aspect="video"
        rounded="rounded-3xl"
        class="album-shadow w-64 shrink-0"
      />
      <div
        v-else
        class="flex h-36 w-64 shrink-0 items-center justify-center rounded-3xl bg-[var(--ctp-accent-soft)] text-[var(--ctp-accent)]"
      >
        <LuFolderHeart class="h-12 w-12" />
      </div>

      <div class="min-w-0 flex-1 space-y-2.5">
        <p class="text-xs font-medium text-[var(--ctp-subtext0)]">
          {{ t('common.videosCount', { count: detail.shelf.video_count }) }}
        </p>

        <h1 class="truncate text-2xl font-extrabold tracking-tight sm:text-3xl">
          {{ detail.shelf.name }}
        </h1>

        <div class="flex flex-wrap items-center gap-2.5 pt-2">
          <button
            v-if="detail.videos.length > 0"
            type="button"
            class="inline-flex items-center gap-2 rounded-full bg-[var(--ctp-accent)] px-5 py-2.5 text-xs font-bold text-[var(--ctp-on-accent)] shadow-md"
            @click="router.push(`/video/watch/${detail.videos[0]!.id}`)"
          >
            <LuPlay class="h-4 w-4 fill-current" />
            <span>{{ t('common.play') }}</span>
          </button>

          <button
            type="button"
            class="inline-flex items-center gap-1.5 rounded-full border border-[var(--ctp-surface0)] bg-[var(--ctp-shell)] px-3.5 py-2 text-xs font-medium text-[var(--ctp-subtext1)] hover:text-[var(--ctp-text)]"
            @click="handleRename"
          >
            <LuPencil class="h-3.5 w-3.5" />
            <span>{{ t('common.edit') }}</span>
          </button>

          <button
            type="button"
            class="inline-flex items-center gap-1.5 rounded-full border border-[var(--ctp-red)]/30 bg-[var(--ctp-red)]/10 px-3.5 py-2 text-xs font-medium text-[var(--ctp-red)]"
            @click="handleDeleteShelf"
          >
            <LuTrash2 class="h-3.5 w-3.5" />
            <span>{{ t('common.delete') }}</span>
          </button>

          <RefreshButton size="md" :action="loadShelf" />
        </div>
      </div>
    </div>

    <div
      v-if="detail.videos.length === 0"
      class="card-soft rounded-2xl bg-[var(--ctp-shell)] py-14 text-center text-xs text-[var(--ctp-subtext0)]"
    >
      {{ t('shelf.emptyVideos') }}
    </div>
    <div v-else class="grid grid-cols-1 gap-3.5 sm:grid-cols-2 md:grid-cols-3 lg:grid-cols-4">
      <VideoCard
        v-for="v in detail.videos"
        :key="v.id"
        :video="v"
        removable-from-shelf
        @remove-from-shelf="handleRemoveFromShelf"
        @edit="editingVideo = $event"
        @deleted="loadShelf"
      />
    </div>

    <VideoMetadataModal
      :open="editingVideo !== null"
      :video="editingVideo"
      @close="editingVideo = null"
      @saved="loadShelf"
    />
  </div>
</template>

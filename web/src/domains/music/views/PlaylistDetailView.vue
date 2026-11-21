<script setup lang="ts">
import { computed, onMounted, ref, watch } from 'vue'
import { useRoute, useRouter } from 'vue-router'
import { LuPlay, LuShuffle, LuPencil, LuTrash2 } from 'vue-icons-plus/lu'
import AppSwitch from '@/components/ui/AppSwitch.vue'
import { useAuthStore } from '@/stores/auth'
import { useDialogStore } from '@/stores/dialog'
import RefreshButton from '@/components/ui/RefreshButton.vue'
import { api } from '../api'
import CoverImage from '../components/CoverImage.vue'
import TrackTable from '../components/TrackTable.vue'
import { useMusicI18n as useI18n } from '../i18n'
import { usePlayerStore } from '../player'
import { useMusicStore } from '../store'
import type { PlaylistDetailDto } from '../types'

const { t } = useI18n()
const route = useRoute()
const router = useRouter()
const authStore = useAuthStore()
const dialogStore = useDialogStore()
const musicStore = useMusicStore()
const playerStore = usePlayerStore()

const detail = ref<PlaylistDetailDto | null>(null)

const canEdit = computed(
  () =>
    detail.value !== null &&
    (detail.value.playlist.owner_id === authStore.user?.id || authStore.isAdmin),
)

async function loadPlaylist(): Promise<void> {
  const id = String(route.params.id || '')
  if (!id) return
  detail.value = await api.getPlaylist(id)
}

async function handleRename(): Promise<void> {
  if (!detail.value) return
  const next = await dialogStore.prompt({
    title: t('playlist.renameTitle'),
    message: t('playlist.renamePrompt'),
    initialValue: detail.value.playlist.name,
  })
  if (!next || !next.trim()) return
  detail.value = await api.updatePlaylist(detail.value.playlist.id, { name: next.trim() })
  await musicStore.refreshSidebar()
}

async function handleVisibilityToggle(nextPublic: boolean): Promise<void> {
  if (!detail.value) return
  detail.value = await api.updatePlaylist(detail.value.playlist.id, { is_public: nextPublic })
  await musicStore.refreshSidebar()
}

async function handleRemoveTrack(idx: number): Promise<void> {
  if (!detail.value) return
  detail.value = await api.updatePlaylist(detail.value.playlist.id, { remove_indices: [idx] })
  await musicStore.refreshSidebar()
}

async function handleDelete(): Promise<void> {
  if (!detail.value) return
  const ok = await dialogStore.confirm({
    title: t('playlist.deleteTitle'),
    message: t('playlist.confirmDelete', { name: detail.value.playlist.name }),
    danger: true,
  })
  if (!ok) return
  await api.deletePlaylist(detail.value.playlist.id)
  await musicStore.refreshSidebar()
  await router.replace('/music')
}

onMounted(loadPlaylist)
watch(() => route.params.id, loadPlaylist)
</script>

<template>
  <div v-if="detail" class="mx-auto max-w-6xl space-y-7 px-6 py-8 sm:px-8">
    <div class="flex flex-col items-start gap-6 sm:flex-row sm:items-end">
      <CoverImage
        :cover-id="detail.playlist.id"
        :has-cover="Boolean(detail.playlist.cover_key)"
        :size="420"
        rounded="rounded-3xl"
        class="album-shadow h-44 w-44 shrink-0 sm:h-52 sm:w-52"
      />

      <div class="min-w-0 flex-1 space-y-2.5">
        <p class="text-xs font-medium text-[var(--md-sys-color-on-surface-variant)]">
          {{ t('playlist.ownerBy', { owner: detail.playlist.owner_username }) }} ·
          {{ t('common.tracksCount', { count: detail.playlist.track_count }) }}
        </p>

        <h1 class="truncate text-2xl font-extrabold tracking-tight text-[var(--md-sys-color-on-surface)] sm:text-3xl">
          {{ detail.playlist.name }}
        </h1>

        <div v-if="canEdit" class="pt-1">
          <AppSwitch
            :model-value="detail.playlist.is_public"
            :label="detail.playlist.is_public ? t('playlist.publicLabel') : t('playlist.privateLabel')"
            @change="handleVisibilityToggle"
          />
        </div>

        <div class="flex flex-wrap items-center gap-2.5 pt-2">
          <button
            v-if="detail.tracks.length > 0"
            type="button"
            class="md-btn md-btn-filled md-state"
            @click="playerStore.playQueue(detail.tracks, 0)"
          >
            <LuPlay class="h-4 w-4 fill-current" />
            <span>{{ t('common.playAll') }}</span>
          </button>

          <button
            v-if="detail.tracks.length > 0"
            type="button"
            class="md-btn md-btn-tonal md-state"
            @click="playerStore.shuffleAndPlay(detail.tracks)"
          >
            <LuShuffle class="h-4 w-4" />
            <span>{{ t('common.shufflePlay') }}</span>
          </button>

          <button
            v-if="canEdit"
            type="button"
            class="md-btn md-btn-outlined md-btn-sm md-state"
            @click="handleRename"
          >
            <LuPencil class="h-3.5 w-3.5" />
            <span>{{ t('common.edit') }}</span>
          </button>

          <button
            v-if="canEdit"
            type="button"
            class="md-btn md-btn-sm md-state bg-[var(--md-sys-color-error-container)] text-[var(--md-sys-color-on-error-container)]"
            @click="handleDelete"
          >
            <LuTrash2 class="h-3.5 w-3.5" />
            <span>{{ t('common.delete') }}</span>
          </button>

          <RefreshButton size="md" :action="loadPlaylist" />
        </div>
      </div>
    </div>

    <div
      v-if="detail.tracks.length === 0"
      class="md-card-filled py-14 text-center text-xs text-[var(--md-sys-color-on-surface-variant)]"
    >
      {{ t('playlist.emptyTracks') }}
    </div>
    <TrackTable
      v-else
      :tracks="detail.tracks"
      :removable="canEdit"
      @remove="handleRemoveTrack"
    />
  </div>
</template>

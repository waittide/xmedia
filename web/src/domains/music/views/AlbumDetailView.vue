<script setup lang="ts">
import { computed, onMounted, ref, watch } from 'vue'
import { useRoute, useRouter } from 'vue-router'
import { LuPlay, LuShuffle, LuHeart } from 'vue-icons-plus/lu'
import RefreshButton from '@/components/ui/RefreshButton.vue'
import { api } from '../api'
import CoverImage from '../components/CoverImage.vue'
import TrackTable from '../components/TrackTable.vue'
import { useMusicI18n as useI18n } from '../i18n'
import { usePlayerStore } from '../player'
import { useMusicStore } from '../store'
import type { AlbumDetailDto, TrackDto } from '../types'

const { t } = useI18n()
const route = useRoute()
const router = useRouter()
const musicStore = useMusicStore()
const playerStore = usePlayerStore()

const detail = ref<AlbumDetailDto | null>(null)
const loading = ref(true)

async function loadAlbum(): Promise<void> {
  const id = String(route.params.id || '')
  if (!id) return
  loading.value = true
  try {
    detail.value = await api.getAlbum(id)
  } finally {
    loading.value = false
  }
}

const discGroups = computed(() => {
  if (!detail.value) return []
  const map = new Map<number, TrackDto[]>()
  for (const tr of detail.value.tracks) {
    const d = tr.disc_number ?? 1
    const arr = map.get(d) ?? []
    arr.push(tr)
    map.set(d, arr)
  }
  return Array.from(map.entries()).sort((a, b) => a[0] - b[0])
})

function formatTotalMins(secs: number): string {
  const mins = Math.round(secs / 60)
  return `${mins} min`
}

onMounted(loadAlbum)
watch(() => route.params.id, loadAlbum)
</script>

<template>
  <div v-if="detail" class="mx-auto max-w-5xl space-y-7 px-6 py-8 sm:px-8">
    <!-- Album Header -->
    <div class="flex flex-col items-start gap-6 sm:flex-row sm:items-end">
      <CoverImage
        :cover-id="detail.album.id"
        :has-cover="Boolean(detail.album.cover_key)"
        :size="480"
        rounded="rounded-3xl"
        class="album-shadow h-48 w-48 shrink-0 sm:h-56 sm:w-56"
      />

      <div class="min-w-0 flex-1 space-y-2.5">
        <div class="flex flex-wrap items-center gap-2">
          <span
            v-if="detail.album.is_compilation"
            class="rounded-full bg-[var(--md-sys-color-primary-container)] px-2.5 py-0.5 text-[11px] font-bold text-[var(--md-sys-color-on-primary-container)]"
          >
            {{ t('catalog.compilationBadge') }}
          </span>
          <span
            v-if="detail.album.genre"
            class="text-xs font-medium text-[var(--md-sys-color-on-surface-variant)]"
          >
            {{ detail.album.genre }}
          </span>
        </div>

        <h1 class="text-2xl font-extrabold tracking-tight text-[var(--md-sys-color-on-surface)] sm:text-3xl">
          {{ detail.album.title }}
        </h1>

        <div class="flex flex-wrap items-center gap-2 text-xs text-[var(--md-sys-color-on-surface-variant)]">
          <button
            type="button"
            class="font-bold text-[var(--md-sys-color-on-surface)] hover:text-[var(--md-sys-color-primary)] hover:underline"
            @click="router.push(`/music/artists/${detail.album.album_artist_id}`)"
          >
            {{ detail.album.album_artist_name }}
          </button>
          <span v-if="detail.album.year">· {{ detail.album.year }}</span>
          <span>· {{ t('common.tracksCount', { count: detail.album.track_count }) }}</span>
          <span>· {{ formatTotalMins(detail.album.duration_secs) }}</span>
        </div>

        <div class="flex flex-wrap items-center gap-2.5 pt-2">
          <button
            type="button"
            class="md-btn md-btn-filled md-state"
            @click="playerStore.playQueue(detail.tracks, 0)"
          >
            <LuPlay class="h-4 w-4 fill-current" />
            <span>{{ t('common.playAll') }}</span>
          </button>

          <button
            type="button"
            class="md-btn md-btn-tonal md-state"
            @click="playerStore.shuffleAndPlay(detail.tracks)"
          >
            <LuShuffle class="h-4 w-4" />
            <span>{{ t('common.shufflePlay') }}</span>
          </button>

          <button
            type="button"
            class="md-icon-btn md-state border border-[var(--md-sys-color-outline-variant)] bg-[var(--md-sys-color-surface-container-low)]"
            @click="musicStore.toggleAlbumStar(detail.album)"
          >
            <LuHeart
              class="h-4 w-4"
              :class="
                detail.album.starred_at
                  ? 'fill-[var(--md-sys-color-error)] text-[var(--md-sys-color-error)]'
                  : 'text-[var(--md-sys-color-on-surface-variant)]'
              "
            />
          </button>

          <RefreshButton size="md" :action="loadAlbum" :busy="loading" />
        </div>
      </div>
    </div>

    <!-- Multi-Disc Track Tables -->
    <div v-for="[discNum, discTracks] in discGroups" :key="discNum" class="space-y-2">
      <h3
        v-if="discGroups.length > 1"
        class="px-1 text-xs font-bold uppercase tracking-wider text-[var(--md-sys-color-on-surface-variant)]"
      >
        {{ t('catalog.discHeader', { disc: discNum }) }}
      </h3>
      <TrackTable :tracks="discTracks" :show-album="false" :show-cover="false" />
    </div>
  </div>
</template>

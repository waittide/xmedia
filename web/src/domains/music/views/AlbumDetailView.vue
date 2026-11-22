<script setup lang="ts">
import { computed, onMounted, ref, watch } from 'vue'
import { useRoute, useRouter } from 'vue-router'
import {
  LuDisc3,
  LuListPlus,
  LuMic,
  LuPlay,
  LuRefreshCw,
  LuShuffle,
  LuTag,
} from 'vue-icons-plus/lu'
import {
  MediaDetailTemplate,
  type MediaDetailSpec,
  type MediaDetailTag,
} from '@/components/domain'
import { MdMenuItem } from '@/components/ui'
import { useSnackbarStore } from '@/stores/snackbar'
import { api, buildCoverUrl } from '../api'
import TrackTable from '../components/TrackTable.vue'
import { useMusicI18n as useI18n } from '../i18n'
import { usePlayerStore } from '../player'
import { useMusicStore } from '../store'
import type { AlbumDetailDto } from '../types'

const { t } = useI18n()
const route = useRoute()
const router = useRouter()
const musicStore = useMusicStore()
const playerStore = usePlayerStore()
const snackbarStore = useSnackbarStore()

const detail = ref<AlbumDetailDto | null>(null)
const loading = ref(true)
const error = ref<string | null>(null)
const favoriteLoading = ref(false)
let loadSeq = 0

async function loadAlbum(): Promise<void> {
  const id = String(route.params.id || '')
  if (!id) {
    error.value = t('common.noResults')
    loading.value = false
    return
  }
  const seq = ++loadSeq
  loading.value = true
  error.value = null
  try {
    const res = await api.getAlbum(id)
    if (seq !== loadSeq) return
    detail.value = res
  } catch (err) {
    if (seq !== loadSeq) return
    error.value = err instanceof Error ? err.message : String(err)
  } finally {
    if (seq === loadSeq) {
      loading.value = false
    }
  }
}

const coverSrc = computed(() =>
  detail.value?.album.cover_key ? buildCoverUrl(detail.value.album.id, 480) : null,
)

const totalMins = computed(() =>
  detail.value ? Math.max(1, Math.round(detail.value.album.duration_secs / 60)) : 0,
)

const hasMultipleDiscs = computed(() => {
  if (!detail.value) return false
  const discs = new Set(detail.value.tracks.map((tr) => tr.disc_number ?? 1))
  return discs.size > 1
})

const albumSubtitle = computed(() => {
  const al = detail.value?.album
  if (!al) return ''
  const parts: string[] = [al.album_artist_name]
  if (al.year) parts.push(String(al.year))
  parts.push(t('common.tracksCount', { count: al.track_count }))
  parts.push(t('common.minutesCount', { count: totalMins.value }))
  return parts.filter(Boolean).join(' · ')
})

const albumTags = computed<MediaDetailTag[]>(() => {
  const al = detail.value?.album
  if (!al) return []
  const list: MediaDetailTag[] = []
  if (al.is_compilation) {
    list.push({ label: t('catalog.compilationBadge') })
  }
  if (al.genre) {
    list.push({
      label: al.genre,
      to: { path: '/music/library', query: { view: 'items', genre: al.genre } },
    })
  }
  return list
})

const albumSpecs = computed<MediaDetailSpec[]>(() => {
  const al = detail.value?.album
  if (!al) return []
  const list: MediaDetailSpec[] = [
    { label: t('nav.artists'), value: al.album_artist_name },
    { label: t('catalog.colDuration'), value: t('common.minutesCount', { count: totalMins.value }) },
  ]
  if (al.disc_count > 1) {
    list.push({ label: t('catalog.discHeader', { disc: al.disc_count }), value: String(al.disc_count) })
  }
  if (al.genre) {
    list.push({ label: t('nav.genres'), value: al.genre })
  }
  return list
})

function handleBack(): void {
  if (window.history.length > 1) {
    router.back()
  } else {
    void router.push({ path: '/music/library', query: { view: 'items' } })
  }
}

async function handleToggleFavorite(): Promise<void> {
  if (!detail.value || favoriteLoading.value) return
  favoriteLoading.value = true
  try {
    await musicStore.toggleAlbumStar(detail.value.album)
  } catch (err) {
    snackbarStore.show({
      message: err instanceof Error ? err.message : String(err),
      tone: 'error',
    })
  } finally {
    favoriteLoading.value = false
  }
}

function handleAddAlbumToQueue(): void {
  if (!detail.value || detail.value.tracks.length === 0) return
  playerStore.enqueueTracks(detail.value.tracks)
  snackbarStore.show(t('common.addedToQueue'))
}

function handleTagsUpdated(payload: { trackId: string; tags: string[] }): void {
  if (!detail.value) return
  detail.value = {
    ...detail.value,
    tracks: detail.value.tracks.map((tr) =>
      tr.id === payload.trackId ? { ...tr, tags: payload.tags } : tr,
    ),
  }
}

onMounted(loadAlbum)
watch(() => route.params.id, loadAlbum)
</script>

<template>
  <div class="px-4 py-6 sm:px-6 lg:px-8">
    <MediaDetailTemplate
      :title="detail?.album.title ?? ''"
      :subtitle="albumSubtitle"
      :cover="coverSrc"
      aspect="square"
      cover-style="album"
      :fallback-icon="LuDisc3"
      :loading="loading && !detail"
      :error="error"
      :primary-action-label="t('common.playAll')"
      :primary-action-icon="LuPlay"
      :primary-action-disabled="!detail || detail.tracks.length === 0"
      :secondary-action-label="t('common.shufflePlay')"
      :secondary-action-icon="LuShuffle"
      :secondary-action-disabled="!detail || detail.tracks.length === 0"
      :show-favorite="Boolean(detail)"
      :favorite="Boolean(detail?.album.starred_at)"
      :favorite-loading="favoriteLoading"
      :tags="albumTags"
      :specs="albumSpecs"
      @back="handleBack"
      @retry="loadAlbum"
      @primary-action="detail && playerStore.playQueue(detail.tracks, 0)"
      @secondary-action="detail && playerStore.shuffleAndPlay(detail.tracks)"
      @toggle-favorite="handleToggleFavorite"
    >
      <template #overflow-menu>
        <MdMenuItem
          v-if="detail && detail.tracks.length > 0"
          :icon="LuListPlus"
          :label="t('common.addAlbumToQueue')"
          @select="handleAddAlbumToQueue"
        />
        <MdMenuItem
          v-if="detail?.album.album_artist_id"
          :icon="LuMic"
          :label="t('common.viewArtist')"
          :to="{ path: '/music/library', query: { artist: detail.album.album_artist_id } }"
        />
        <MdMenuItem
          v-if="detail?.album.genre"
          :icon="LuTag"
          :label="detail.album.genre"
          :to="{ path: '/music/library', query: { view: 'items', genre: detail.album.genre } }"
        />
        <MdMenuItem
          :icon="LuRefreshCw"
          :label="$t('common.refresh')"
          @select="loadAlbum"
        />
      </template>

      <TrackTable
        v-if="detail && detail.tracks.length > 0"
        :tracks="detail.tracks"
        :show-album="false"
        :show-cover="false"
        :group-by-disc="hasMultipleDiscs"
        @tags-updated="handleTagsUpdated"
      />
    </MediaDetailTemplate>
  </div>
</template>

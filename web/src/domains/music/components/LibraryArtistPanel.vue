<script setup lang="ts">
import { computed, onMounted, ref, watch } from 'vue'
import { LuArrowLeft, LuHeart, LuMic, LuPlay, LuRefreshCw, LuShuffle } from 'vue-icons-plus/lu'
import { MediaErrorState } from '@/components/domain'
import { MdButton, MdIconButton, MdSkeleton, MediaCoverImage } from '@/components/ui'
import { useSnackbarStore } from '@/stores/snackbar'
import { api, buildCoverUrl } from '../api'
import { useMusicI18n as useI18n } from '../i18n'
import { usePlayerStore } from '../player'
import { useMusicStore } from '../store'
import type { ArtistDetailDto, TrackDto } from '../types'
import AlbumCard from './AlbumCard.vue'
import TrackTable from './TrackTable.vue'

const props = defineProps<{
  artistId: string
}>()

const emit = defineEmits<{
  back: []
}>()

const { t } = useI18n()
const musicStore = useMusicStore()
const playerStore = usePlayerStore()
const snackbarStore = useSnackbarStore()

const detail = ref<ArtistDetailDto | null>(null)
const loading = ref(true)
const error = ref<string | null>(null)
const favoriteLoading = ref(false)
let loadSeq = 0

async function loadArtist(): Promise<void> {
  if (!props.artistId) return
  const seq = ++loadSeq
  loading.value = true
  error.value = null
  try {
    const res = await api.getArtist(props.artistId)
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
  detail.value?.artist.cover_key ? buildCoverUrl(detail.value.artist.id, 320) : null,
)

// Fallback to appears_on_tracks when artist has no top_tracks (fixes P1-7)
const playableTracks = computed<TrackDto[]>(() => {
  if (!detail.value) return []
  if (detail.value.top_tracks.length > 0) return detail.value.top_tracks
  return detail.value.appears_on_tracks
})

async function handleToggleFavorite(): Promise<void> {
  if (!detail.value || favoriteLoading.value) return
  favoriteLoading.value = true
  try {
    await musicStore.toggleArtistStar(detail.value.artist)
  } catch (err) {
    snackbarStore.show({
      message: err instanceof Error ? err.message : String(err),
      tone: 'error',
    })
  } finally {
    favoriteLoading.value = false
  }
}

onMounted(loadArtist)
watch(() => props.artistId, loadArtist)
</script>

<template>
  <div class="space-y-6">
    <div>
      <MdButton variant="text" size="sm" :icon="LuArrowLeft" @click="emit('back')">
        {{ t('nav.artists') }}
      </MdButton>
    </div>

    <MediaErrorState
      v-if="error"
      :message="error"
      :retrying="loading"
      @retry="loadArtist"
    />

    <div v-else-if="loading && !detail" class="space-y-6" aria-busy="true">
      <div class="flex items-center gap-5 rounded-corner-xl bg-surface-container-low p-5">
        <MdSkeleton variant="circle" width="6.5rem" height="6.5rem" class="shrink-0" />
        <div class="flex-1 space-y-2.5">
          <MdSkeleton variant="text" width="40%" height="1.75rem" />
          <MdSkeleton variant="text" width="28%" height="1rem" />
          <div class="flex gap-2 pt-1">
            <MdSkeleton variant="rect" width="7rem" height="2.25rem" class="rounded-corner-full" />
            <MdSkeleton variant="rect" width="7rem" height="2.25rem" class="rounded-corner-full" />
          </div>
        </div>
      </div>
    </div>

    <template v-else-if="detail">
      <!-- Artist Header Card (minimalist MD3, zero technical role badges) -->
      <section
        class="flex flex-col items-start gap-5 rounded-corner-xl bg-surface-container-low p-5 sm:flex-row sm:items-center"
      >
        <MediaCoverImage
          :src="coverSrc"
          :alt="detail.artist.name"
          aspect-class="h-28 w-28 sm:h-36 sm:w-36 shrink-0"
          rounded="rounded-corner-full"
          bg-class="bg-surface-container-high"
          :fallback-icon="LuMic"
          fallback-icon-class="h-10 w-10 text-on-surface-variant/50"
          loading="eager"
          class="album-shadow"
        />

        <div class="min-w-0 flex-1 space-y-2">
          <h2 class="text-headline-medium truncate text-on-surface">
            {{ detail.artist.name }}
          </h2>
          <p class="text-body-medium text-on-surface-variant">
            {{ t('common.albumsCount', { count: detail.artist.album_count }) }} ·
            {{ t('common.tracksCount', { count: detail.artist.track_count }) }}
          </p>

          <div class="flex flex-wrap items-center gap-2 pt-1">
            <MdButton
              v-if="playableTracks.length > 0"
              variant="filled"
              :icon="LuPlay"
              @click="playerStore.playQueue(playableTracks, 0)"
            >
              {{ t('common.playAll') }}
            </MdButton>

            <MdButton
              v-if="playableTracks.length > 0"
              variant="tonal"
              :icon="LuShuffle"
              @click="playerStore.shuffleAndPlay(playableTracks)"
            >
              {{ t('common.shufflePlay') }}
            </MdButton>

            <MdIconButton
              variant="tonal"
              :selected="Boolean(detail.artist.starred_at)"
              :loading="favoriteLoading"
              :aria-label="detail.artist.starred_at ? t('common.unfavorite') : t('common.favorite')"
              @click="handleToggleFavorite"
            >
              <LuHeart
                class="h-5 w-5"
                :class="detail.artist.starred_at ? 'fill-error text-error' : ''"
              />
            </MdIconButton>

            <MdIconButton
              variant="standard"
              :icon="LuRefreshCw"
              :loading="loading"
              :aria-label="$t('common.refresh')"
              @click="loadArtist"
            />
          </div>
        </div>
      </section>

      <section v-if="detail.albums.length > 0" class="space-y-3">
        <h3 class="text-title-large text-on-surface">
          {{ t('nav.albums') }}
        </h3>
        <div class="grid grid-cols-2 gap-5 sm:grid-cols-3 md:grid-cols-4 lg:grid-cols-5 xl:grid-cols-6">
          <AlbumCard v-for="al in detail.albums" :key="al.id" :album="al" />
        </div>
      </section>

      <section v-if="detail.top_tracks.length > 0" class="space-y-3">
        <h3 class="text-title-large text-on-surface">
          {{ t('catalog.topTracks') }}
        </h3>
        <TrackTable :tracks="detail.top_tracks" />
      </section>

      <section v-if="detail.appears_on_tracks.length > 0" class="space-y-3">
        <h3 class="text-title-large text-on-surface">
          {{ t('catalog.appearsOn') }}
        </h3>
        <TrackTable :tracks="detail.appears_on_tracks" />
      </section>
    </template>
  </div>
</template>

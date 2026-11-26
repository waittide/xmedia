<script setup lang="ts">
import { computed, onBeforeUnmount, onMounted, ref, watch } from 'vue'
import { LuHeart, LuPlay, LuShuffle } from 'vue-icons-plus/lu'
import { MediaErrorState } from '@/components/domain'
import { MdButton, MdChip, MdEmptyState, MdSkeleton } from '@/components/ui'
import { useDelayedLoading } from '@/composables/useDelayedLoading'
import { api } from '../api'
import { useMusicI18n as useI18n } from '../i18n'
import { usePlayerStore } from '../player'
import { useMusicStore } from '../store'
import type { StarredBundleDto } from '../types'
import AlbumCard from './AlbumCard.vue'
import ArtistCard from './ArtistCard.vue'
import TrackTable from './TrackTable.vue'

type FavSubTab = 'tracks' | 'albums' | 'artists'

const props = withDefaults(
  defineProps<{
    query?: string
  }>(),
  {
    query: '',
  },
)

const { t } = useI18n()
const playerStore = usePlayerStore()
const musicStore = useMusicStore()

const activeSubTab = ref<FavSubTab>('tracks')
const loadingLocal = ref(false)
const localError = ref<string | null>(null)
const starred = ref<StarredBundleDto>({ artists: [], albums: [], tracks: [] })

const needle = computed(() => props.query.trim().toLowerCase())

const visibleTracks = computed(() =>
  starred.value.tracks.filter(
    (tr) =>
      Boolean(tr.starred_at) &&
      (!needle.value ||
        tr.title.toLowerCase().includes(needle.value) ||
        tr.artist_name.toLowerCase().includes(needle.value) ||
        tr.album_title.toLowerCase().includes(needle.value)),
  ),
)

const visibleAlbums = computed(() =>
  starred.value.albums.filter(
    (al) =>
      Boolean(al.starred_at) &&
      (!needle.value ||
        al.title.toLowerCase().includes(needle.value) ||
        al.album_artist_name.toLowerCase().includes(needle.value)),
  ),
)

const visibleArtists = computed(() =>
  starred.value.artists.filter(
    (ar) => Boolean(ar.starred_at) && (!needle.value || ar.name.toLowerCase().includes(needle.value)),
  ),
)

let loadSeq = 0

async function loadStarred(): Promise<void> {
  const seq = ++loadSeq
  loadingLocal.value = true
  localError.value = null
  try {
    const res = await api.getStarred()
    if (seq !== loadSeq) return
    starred.value = res
  } catch (err) {
    if (seq !== loadSeq) return
    localError.value = err instanceof Error ? err.message : String(err)
  } finally {
    if (seq === loadSeq) {
      loadingLocal.value = false
    }
  }
}

onBeforeUnmount(() => {
  loadSeq += 1
})

async function refresh(): Promise<void> {
  await loadStarred()
}

const showLocalSkeleton = useDelayedLoading(
  () =>
    loadingLocal.value &&
    starred.value.tracks.length === 0 &&
    starred.value.albums.length === 0,
)

onMounted(loadStarred)
watch(() => [musicStore.catalogVersion, musicStore.favoritesVersion], loadStarred)

defineExpose({ refresh })
</script>

<template>
  <div class="space-y-4">
    <div class="flex flex-wrap items-center justify-between gap-2">
      <div class="flex flex-wrap items-center gap-1.5">
        <MdChip
          variant="filter"
          :label="`${t('favorites.tabTracks')} (${visibleTracks.length})`"
          :selected="activeSubTab === 'tracks'"
          @click="activeSubTab = 'tracks'"
        />
        <MdChip
          variant="filter"
          :label="`${t('favorites.tabAlbums')} (${visibleAlbums.length})`"
          :selected="activeSubTab === 'albums'"
          @click="activeSubTab = 'albums'"
        />
        <MdChip
          variant="filter"
          :label="`${t('favorites.tabArtists')} (${visibleArtists.length})`"
          :selected="activeSubTab === 'artists'"
          @click="activeSubTab = 'artists'"
        />
      </div>

      <!-- Fixes P1-4: Play All / Shuffle strictly scoped to tracks sub-tab -->
      <div
        v-if="activeSubTab === 'tracks' && visibleTracks.length > 0"
        class="flex items-center gap-1.5"
      >
        <MdButton
          variant="tonal"
          :icon="LuPlay"
          @click="playerStore.playQueue(visibleTracks, 0)"
        >
          {{ t('common.playAllBtn') }}
        </MdButton>
        <MdButton
          variant="text"
          :icon="LuShuffle"
          @click="playerStore.shuffleAndPlay(visibleTracks)"
        >
          {{ t('common.shufflePlayBtn') }}
        </MdButton>
      </div>
    </div>

    <MediaErrorState
      v-if="localError"
      :message="localError"
      :retrying="loadingLocal"
      @retry="loadStarred"
    />

    <div
      v-else-if="loadingLocal && starred.tracks.length === 0 && starred.albums.length === 0"
      class="space-y-2.5"
      aria-busy="true"
    >
      <template v-if="showLocalSkeleton">
        <MdSkeleton v-for="i in 6" :key="`fv-sk-${i}`" variant="rect" height="3.5rem" />
      </template>
    </div>

    <template v-else-if="activeSubTab === 'tracks'">
      <TrackTable v-if="visibleTracks.length > 0" :tracks="visibleTracks" />
      <MdEmptyState v-else compact :icon="LuHeart" :title="t('favorites.empty')" />
    </template>

    <template v-else-if="activeSubTab === 'albums'">
      <div
        v-if="visibleAlbums.length > 0"
        class="grid grid-cols-2 gap-5 sm:grid-cols-3 md:grid-cols-4 lg:grid-cols-5 xl:grid-cols-6"
      >
        <AlbumCard v-for="al in visibleAlbums" :key="al.id" :album="al" />
      </div>
      <MdEmptyState v-else compact :icon="LuHeart" :title="t('favorites.empty')" />
    </template>

    <template v-else>
      <div
        v-if="visibleArtists.length > 0"
        class="grid grid-cols-2 gap-4 sm:grid-cols-3 md:grid-cols-4 lg:grid-cols-5 xl:grid-cols-6"
      >
        <ArtistCard v-for="ar in visibleArtists" :key="ar.id" :artist="ar" />
      </div>
      <MdEmptyState v-else compact :icon="LuHeart" :title="t('favorites.empty')" />
    </template>
  </div>
</template>

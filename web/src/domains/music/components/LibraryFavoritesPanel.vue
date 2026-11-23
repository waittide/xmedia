<script setup lang="ts">
import { computed, onMounted, ref, watch } from 'vue'
import { LuDisc3, LuHeart, LuPlay, LuShuffle } from 'vue-icons-plus/lu'
import { MediaErrorState, MediaItemCard } from '@/components/domain'
import { MdButton, MdChip, MdEmptyState, MdSkeleton } from '@/components/ui'
import { useDelayedLoading } from '@/composables/useDelayedLoading'
import {
  isAvailabilityBlocked,
  onlineItemsApi,
  seedOnlineItemStateCache,
  useMergedShelf,
  useOnlineItemStates,
  type MediaItemVM,
  type SourceScope,
  type TrackedItemRef,
} from '@/sources'
import { useSnackbarStore } from '@/stores/snackbar'
import { api } from '../api'
import { useMusicI18n as useI18n } from '../i18n'
import { snapshotToOnlineMusicTrack, snapshotToRemoteSubsonicSong } from '../onlineTrackRef'
import { usePlayerStore } from '../player'
import { useMusicStore } from '../store'
import type { StarredBundleDto } from '../types'
import AlbumCard from './AlbumCard.vue'
import ArtistCard from './ArtistCard.vue'
import TrackTable from './TrackTable.vue'

type FavSubTab = 'tracks' | 'albums' | 'artists' | 'online'

const props = withDefaults(
  defineProps<{
    scope: SourceScope
    sourceId?: string | null
    query?: string
  }>(),
  {
    sourceId: null,
    query: '',
  },
)

const { t } = useI18n()
const playerStore = usePlayerStore()
const musicStore = useMusicStore()
const snackbarStore = useSnackbarStore()

const activeSubTab = ref<FavSubTab>('tracks')
const loadingLocal = ref(false)
const localError = ref<string | null>(null)
const starred = ref<StarredBundleDto>({ artists: [], albums: [], tracks: [] })

const onlineShelfScope = computed<SourceScope | 'all'>(() =>
  props.scope === 'local' ? 'online' : props.scope,
)

const onlineFavShelf = useMergedShelf('music', 'favorites', {
  scope: onlineShelfScope,
  sourceId: () => props.sourceId ?? null,
  query: () => props.query,
  limit: 100,
})

const trackedRefs = computed<TrackedItemRef[]>(() => {
  const out: TrackedItemRef[] = []
  for (const item of onlineFavShelf.items.value) {
    if (item.ref.scope === 'online' || item.ref.scope === 'remote') {
      out.push(item.ref)
    }
  }
  return out
})

const onlineStates = useOnlineItemStates(trackedRefs, { autoLoad: false })

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

async function loadStarred(): Promise<void> {
  if (props.scope !== 'local') return
  loadingLocal.value = true
  localError.value = null
  try {
    starred.value = await api.getStarred()
  } catch (err) {
    localError.value = err instanceof Error ? err.message : String(err)
  } finally {
    loadingLocal.value = false
  }
}

async function refresh(): Promise<void> {
  await Promise.allSettled([loadStarred(), onlineFavShelf.refresh()])
}

function handlePlayOnlineFav(item: MediaItemVM): void {
  if (item.badge?.blocked || isAvailabilityBlocked(item.badge?.availability)) {
    snackbarStore.show({ message: t('common.sourceUnavailable'), tone: 'error' })
    return
  }
  if (item.ref.scope === 'local') return
  const snap = onlineStates.getState(item.ref)
  if (!snap) return
  if (item.ref.scope === 'online') {
    playerStore.playOnlineTrackNow(snapshotToOnlineMusicTrack(snap))
  } else {
    playerStore.playRemoteSongAt(
      snap.sourceId,
      snap.sourceName || snap.sourceId,
      snapshotToRemoteSubsonicSong(snap),
      0,
    )
  }
}

async function handleUnfavoriteOnline(item: MediaItemVM): Promise<void> {
  if (item.ref.scope === 'local') return
  const snap = onlineStates.getState(item.ref)
  if (!snap?.id) return
  try {
    const updated = await onlineItemsApi.patch('music', snap.id, { favorite: false })
    seedOnlineItemStateCache([updated])
    await onlineFavShelf.refresh()
  } catch (err) {
    snackbarStore.show({
      message: err instanceof Error ? err.message : String(err),
      tone: 'error',
    })
  }
}

const showOnlineSkeleton = useDelayedLoading(
  () => onlineFavShelf.loading.value && onlineFavShelf.items.value.length === 0,
)
const showLocalSkeleton = useDelayedLoading(
  () =>
    loadingLocal.value &&
    activeSubTab.value !== 'online' &&
    starred.value.tracks.length === 0 &&
    starred.value.albums.length === 0,
)

onMounted(loadStarred)
watch(() => [props.scope, musicStore.catalogVersion, musicStore.favoritesVersion], loadStarred)

defineExpose({ refresh })
</script>

<template>
  <div class="space-y-4">
    <!-- Online/Remote Scope Favorites -->
    <template v-if="scope !== 'local'">
      <MediaErrorState
        v-if="onlineFavShelf.error.value"
        :message="onlineFavShelf.error.value"
        :retrying="onlineFavShelf.loading.value"
        @retry="onlineFavShelf.refresh()"
      />
      <div
        v-else-if="onlineFavShelf.loading.value && onlineFavShelf.items.value.length === 0"
        class="grid grid-cols-2 gap-5 sm:grid-cols-3 md:grid-cols-4 lg:grid-cols-5 xl:grid-cols-6"
        aria-busy="true"
      >
        <template v-if="showOnlineSkeleton">
          <MdSkeleton v-for="i in 8" :key="`of-sk-${i}`" variant="card" aspect="square" />
        </template>
      </div>
      <MdEmptyState
        v-else-if="onlineFavShelf.items.value.length === 0"
        :icon="LuHeart"
        :title="t('favorites.empty')"
      />
      <div
        v-else
        class="grid grid-cols-2 gap-5 sm:grid-cols-3 md:grid-cols-4 lg:grid-cols-5 xl:grid-cols-6"
      >
        <MediaItemCard
          v-for="(item, idx) in onlineFavShelf.items.value"
          :key="`of-${idx}-${item.ref.scope === 'local' ? item.ref.id : item.ref.ref}`"
          :item="item"
          aspect="square"
          cover-style="album"
          as-button
          show-favorite
          :favorite="true"
          :fallback-icon="LuDisc3"
          @select="handlePlayOnlineFav"
          @toggle-favorite="handleUnfavoriteOnline"
        />
      </div>
    </template>

    <!-- Local Scope Favorites (+ Online/Remote sub-filter) -->
    <template v-else>
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
          <MdChip
            v-if="onlineFavShelf.items.value.length > 0"
            variant="filter"
            :label="`${t('favorites.tabOnline')} (${onlineFavShelf.items.value.length})`"
            :selected="activeSubTab === 'online'"
            @click="activeSubTab = 'online'"
          />
        </div>

        <!-- Fixes P1-4: Play All / Shuffle strictly scoped to tracks sub-tab -->
        <div
          v-if="activeSubTab === 'tracks' && visibleTracks.length > 0"
          class="flex items-center gap-1.5"
        >
          <MdButton
            variant="tonal"
            size="sm"
            :icon="LuPlay"
            @click="playerStore.playQueue(visibleTracks, 0)"
          >
            {{ t('common.playAll') }}
          </MdButton>
          <MdButton
            variant="text"
            size="sm"
            :icon="LuShuffle"
            @click="playerStore.shuffleAndPlay(visibleTracks)"
          >
            {{ t('common.shufflePlay') }}
          </MdButton>
        </div>
      </div>

      <MediaErrorState
        v-if="localError && activeSubTab !== 'online'"
        :message="localError"
        :retrying="loadingLocal"
        @retry="loadStarred"
      />

      <div
        v-else-if="loadingLocal && activeSubTab !== 'online' && starred.tracks.length === 0 && starred.albums.length === 0"
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

      <template v-else-if="activeSubTab === 'artists'">
        <div
          v-if="visibleArtists.length > 0"
          class="grid grid-cols-2 gap-4 sm:grid-cols-3 md:grid-cols-4 lg:grid-cols-5 xl:grid-cols-6"
        >
          <ArtistCard v-for="ar in visibleArtists" :key="ar.id" :artist="ar" />
        </div>
        <MdEmptyState v-else compact :icon="LuHeart" :title="t('favorites.empty')" />
      </template>

      <template v-else>
        <div
          v-if="onlineFavShelf.items.value.length > 0"
          class="grid grid-cols-2 gap-5 sm:grid-cols-3 md:grid-cols-4 lg:grid-cols-5 xl:grid-cols-6"
        >
          <MediaItemCard
            v-for="(item, idx) in onlineFavShelf.items.value"
            :key="`of2-${idx}-${item.ref.scope === 'local' ? item.ref.id : item.ref.ref}`"
            :item="item"
            aspect="square"
            cover-style="album"
            as-button
            show-favorite
            :favorite="true"
            :fallback-icon="LuDisc3"
            @select="handlePlayOnlineFav"
            @toggle-favorite="handleUnfavoriteOnline"
          />
        </div>
        <MdEmptyState v-else compact :icon="LuHeart" :title="t('favorites.empty')" />
      </template>
    </template>
  </div>
</template>

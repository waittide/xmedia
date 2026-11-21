<script setup lang="ts">
import { computed, onMounted, ref, watch } from 'vue'
import { useRouter } from 'vue-router'
import { LuPlay, LuShuffle, LuHeart } from 'vue-icons-plus/lu'
import MdSegmentedButton from '@/components/ui/MdSegmentedButton.vue'
import RefreshButton from '@/components/ui/RefreshButton.vue'
import { api } from '../api'
import AlbumCard from '../components/AlbumCard.vue'
import CoverImage from '../components/CoverImage.vue'
import TrackTable from '../components/TrackTable.vue'
import { useMusicI18n as useI18n } from '../i18n'
import { usePlayerStore } from '../player'
import { useMusicStore } from '../store'
import type { StarredBundleDto } from '../types'

const { t } = useI18n()
const router = useRouter()
const playerStore = usePlayerStore()
const musicStore = useMusicStore()

const activeTab = ref<'tracks' | 'albums' | 'artists'>('tracks')
const starred = ref<StarredBundleDto>({ artists: [], albums: [], tracks: [] })

const visibleTracks = computed(() => starred.value.tracks.filter((tr) => Boolean(tr.starred_at)))
const visibleAlbums = computed(() => starred.value.albums.filter((al) => Boolean(al.starred_at)))
const visibleArtists = computed(() => starred.value.artists.filter((ar) => Boolean(ar.starred_at)))

const tabOptions = computed(
  () =>
    [
      { value: 'tracks', label: `${t('favorites.tabTracks')} (${visibleTracks.value.length})` },
      { value: 'albums', label: `${t('favorites.tabAlbums')} (${visibleAlbums.value.length})` },
      { value: 'artists', label: `${t('favorites.tabArtists')} (${visibleArtists.value.length})` },
    ] as const,
)

async function loadStarred(): Promise<void> {
  starred.value = await api.getStarred()
}

onMounted(() => {
  void loadStarred().catch(() => {})
})
watch(() => [musicStore.catalogVersion, musicStore.favoritesVersion], () => {
  void loadStarred().catch(() => {})
})
</script>

<template>
  <div class="mx-auto max-w-7xl space-y-6 px-6 py-7 sm:px-8">
    <div class="flex flex-wrap items-center justify-between gap-4">
      <div>
        <h1 class="text-2xl font-extrabold tracking-tight text-[var(--md-sys-color-on-surface)]">
          {{ t('favorites.title') }}
        </h1>
        <p class="mt-0.5 text-xs text-[var(--md-sys-color-on-surface-variant)]">
          {{ t('favorites.subtitle') }}
        </p>
      </div>

      <div class="flex items-center gap-2">
        <template v-if="visibleTracks.length > 0">
          <button
            type="button"
            class="md-btn md-btn-filled md-btn-sm md-state"
            @click="playerStore.playQueue(visibleTracks, 0)"
          >
            <LuPlay class="h-3.5 w-3.5 fill-current" />
            <span>{{ t('common.playAll') }}</span>
          </button>
          <button
            type="button"
            class="md-btn md-btn-tonal md-btn-sm md-state"
            @click="playerStore.shuffleAndPlay(visibleTracks)"
          >
            <LuShuffle class="h-3.5 w-3.5" />
            <span>{{ t('common.shufflePlay') }}</span>
          </button>
        </template>
        <RefreshButton size="md" :action="loadStarred" />
      </div>
    </div>

    <!-- Tabs -->
    <div>
      <MdSegmentedButton v-model="activeTab" :options="tabOptions" size="md" />
    </div>

    <div v-if="activeTab === 'tracks'">
      <TrackTable v-if="visibleTracks.length > 0" :tracks="visibleTracks" />
      <div v-else class="py-16 text-center text-xs text-[var(--md-sys-color-on-surface-variant)]">
        <LuHeart class="mx-auto mb-2 h-6 w-6 opacity-40" />
        {{ t('favorites.empty') }}
      </div>
    </div>

    <div v-else-if="activeTab === 'albums'">
      <div
        v-if="visibleAlbums.length > 0"
        class="grid grid-cols-2 gap-3 sm:grid-cols-3 md:grid-cols-4 lg:grid-cols-6"
      >
        <AlbumCard v-for="al in visibleAlbums" :key="al.id" :album="al" />
      </div>
      <div v-else class="py-16 text-center text-xs text-[var(--md-sys-color-on-surface-variant)]">
        {{ t('favorites.empty') }}
      </div>
    </div>

    <div v-else>
      <div
        v-if="visibleArtists.length > 0"
        class="grid grid-cols-2 gap-3 sm:grid-cols-3 md:grid-cols-4 lg:grid-cols-6"
      >
        <div
          v-for="ar in visibleArtists"
          :key="ar.id"
          class="flex cursor-pointer flex-col items-center rounded-2xl p-3 text-center transition hover:bg-[var(--md-sys-color-surface-container)]"
          @click="router.push(`/music/artists/${ar.id}`)"
        >
          <CoverImage
            :cover-id="ar.id"
            :has-cover="Boolean(ar.cover_key)"
            :size="240"
            rounded="rounded-full"
            class="mb-2.5 w-3/4"
          />
          <p class="w-full truncate text-xs font-bold text-[var(--md-sys-color-on-surface)]">
            {{ ar.name }}
          </p>
        </div>
      </div>
      <div v-else class="py-16 text-center text-xs text-[var(--md-sys-color-on-surface-variant)]">
        {{ t('favorites.empty') }}
      </div>
    </div>
  </div>
</template>

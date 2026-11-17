<script setup lang="ts">
import { onMounted, ref } from 'vue'
import { useRouter } from 'vue-router'
import { LuPlay, LuShuffle, LuHeart } from 'vue-icons-plus/lu'
import { api } from '../api'
import AlbumCard from '../components/AlbumCard.vue'
import CoverImage from '../components/CoverImage.vue'
import TrackTable from '../components/TrackTable.vue'
import { useMusicI18n as useI18n } from '../i18n'
import { usePlayerStore } from '../player'
import type { StarredBundleDto } from '../types'

const { t } = useI18n()
const router = useRouter()
const playerStore = usePlayerStore()

const activeTab = ref<'tracks' | 'albums' | 'artists'>('tracks')
const starred = ref<StarredBundleDto>({ artists: [], albums: [], tracks: [] })

async function loadStarred(): Promise<void> {
  starred.value = await api.getStarred()
}

onMounted(loadStarred)
</script>

<template>
  <div class="mx-auto max-w-6xl space-y-6 px-6 py-7 sm:px-8">
    <div class="flex flex-wrap items-center justify-between gap-4">
      <div>
        <h1 class="text-2xl font-extrabold tracking-tight">{{ t('favorites.title') }}</h1>
        <p class="mt-0.5 text-xs text-[var(--ctp-subtext0)]">{{ t('favorites.subtitle') }}</p>
      </div>

      <div v-if="starred.tracks.length > 0" class="flex items-center gap-2">
        <button
          type="button"
          class="inline-flex items-center gap-2 rounded-full bg-[var(--ctp-accent)] px-4 py-2 text-xs font-bold text-[var(--ctp-on-accent)] shadow-sm"
          @click="playerStore.playQueue(starred.tracks, 0)"
        >
          <LuPlay class="h-3.5 w-3.5 fill-current" />
          <span>{{ t('common.playAll') }}</span>
        </button>
        <button
          type="button"
          class="inline-flex items-center gap-2 rounded-full border border-[var(--ctp-surface0)] bg-[var(--ctp-shell)] px-4 py-2 text-xs font-semibold"
          @click="playerStore.shuffleAndPlay(starred.tracks)"
        >
          <LuShuffle class="h-3.5 w-3.5" />
          <span>{{ t('common.shufflePlay') }}</span>
        </button>
      </div>
    </div>

    <!-- Tabs -->
    <div class="inline-flex gap-1 rounded-2xl bg-[var(--ctp-shell)] p-1">
      <button
        type="button"
        class="rounded-xl px-4 py-2 text-xs font-semibold transition"
        :class="
          activeTab === 'tracks'
            ? 'bg-[var(--ctp-accent)] text-[var(--ctp-on-accent)]'
            : 'text-[var(--ctp-subtext1)] hover:text-[var(--ctp-text)]'
        "
        @click="activeTab = 'tracks'"
      >
        {{ t('favorites.tabTracks') }} ({{ starred.tracks.length }})
      </button>
      <button
        type="button"
        class="rounded-xl px-4 py-2 text-xs font-semibold transition"
        :class="
          activeTab === 'albums'
            ? 'bg-[var(--ctp-accent)] text-[var(--ctp-on-accent)]'
            : 'text-[var(--ctp-subtext1)] hover:text-[var(--ctp-text)]'
        "
        @click="activeTab = 'albums'"
      >
        {{ t('favorites.tabAlbums') }} ({{ starred.albums.length }})
      </button>
      <button
        type="button"
        class="rounded-xl px-4 py-2 text-xs font-semibold transition"
        :class="
          activeTab === 'artists'
            ? 'bg-[var(--ctp-accent)] text-[var(--ctp-on-accent)]'
            : 'text-[var(--ctp-subtext1)] hover:text-[var(--ctp-text)]'
        "
        @click="activeTab = 'artists'"
      >
        {{ t('favorites.tabArtists') }} ({{ starred.artists.length }})
      </button>
    </div>

    <div v-if="activeTab === 'tracks'">
      <TrackTable v-if="starred.tracks.length > 0" :tracks="starred.tracks" />
      <div v-else class="py-16 text-center text-xs text-[var(--ctp-subtext0)]">
        <LuHeart class="mx-auto mb-2 h-6 w-6 opacity-40" />
        {{ t('favorites.empty') }}
      </div>
    </div>

    <div v-else-if="activeTab === 'albums'">
      <div
        v-if="starred.albums.length > 0"
        class="grid grid-cols-2 gap-3 sm:grid-cols-3 md:grid-cols-4 lg:grid-cols-6"
      >
        <AlbumCard v-for="al in starred.albums" :key="al.id" :album="al" />
      </div>
      <div v-else class="py-16 text-center text-xs text-[var(--ctp-subtext0)]">
        {{ t('favorites.empty') }}
      </div>
    </div>

    <div v-else>
      <div
        v-if="starred.artists.length > 0"
        class="grid grid-cols-2 gap-3 sm:grid-cols-3 md:grid-cols-4 lg:grid-cols-6"
      >
        <div
          v-for="ar in starred.artists"
          :key="ar.id"
          class="flex cursor-pointer flex-col items-center rounded-2xl p-3 text-center transition hover:bg-[var(--ctp-mantle)]"
          @click="router.push(`/music/artists/${ar.id}`)"
        >
          <CoverImage
            :cover-id="ar.id"
            :has-cover="Boolean(ar.cover_key)"
            :size="240"
            rounded="rounded-full"
            class="mb-2.5 w-3/4"
          />
          <p class="w-full truncate text-xs font-bold">{{ ar.name }}</p>
        </div>
      </div>
      <div v-else class="py-16 text-center text-xs text-[var(--ctp-subtext0)]">
        {{ t('favorites.empty') }}
      </div>
    </div>
  </div>
</template>

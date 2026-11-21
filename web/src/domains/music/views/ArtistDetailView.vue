<script setup lang="ts">
import { onMounted, ref, watch } from 'vue'
import { useRoute } from 'vue-router'
import { LuPlay, LuShuffle, LuHeart } from 'vue-icons-plus/lu'
import RefreshButton from '@/components/ui/RefreshButton.vue'
import { api } from '../api'
import AlbumCard from '../components/AlbumCard.vue'
import CoverImage from '../components/CoverImage.vue'
import TrackTable from '../components/TrackTable.vue'
import { useMusicI18n as useI18n } from '../i18n'
import { usePlayerStore } from '../player'
import { useMusicStore } from '../store'
import type { ArtistDetailDto } from '../types'

const { t } = useI18n()
const route = useRoute()
const musicStore = useMusicStore()
const playerStore = usePlayerStore()

const detail = ref<ArtistDetailDto | null>(null)

async function loadArtist(): Promise<void> {
  const id = String(route.params.id || '')
  if (!id) return
  detail.value = await api.getArtist(id)
}

onMounted(loadArtist)
watch(() => route.params.id, loadArtist)
</script>

<template>
  <div v-if="detail" class="mx-auto max-w-6xl space-y-8 px-6 py-8 sm:px-8">
    <div class="flex flex-col items-start gap-6 sm:flex-row sm:items-center">
      <CoverImage
        :cover-id="detail.artist.id"
        :has-cover="Boolean(detail.artist.cover_key)"
        :size="400"
        rounded="rounded-full"
        class="album-shadow h-36 w-36 shrink-0 sm:h-44 sm:w-44"
      />

      <div class="space-y-2.5">
        <div class="flex flex-wrap gap-1.5">
          <span
            v-if="detail.artist.is_album_artist"
            class="rounded-full bg-[var(--md-sys-color-primary-container)] px-2.5 py-0.5 text-[11px] font-bold text-[var(--md-sys-color-on-primary-container)]"
          >
            {{ t('catalog.badgeAlbumArtist') }}
          </span>
          <span
            v-if="detail.artist.is_track_artist"
            class="rounded-full bg-[var(--md-sys-color-surface-container-high)] px-2.5 py-0.5 text-[11px] font-medium text-[var(--md-sys-color-on-surface-variant)]"
          >
            {{ t('catalog.badgeTrackArtist') }}
          </span>
        </div>

        <h1 class="text-3xl font-extrabold tracking-tight text-[var(--md-sys-color-on-surface)]">
          {{ detail.artist.name }}
        </h1>
        <p class="text-xs text-[var(--md-sys-color-on-surface-variant)]">
          {{ t('common.albumsCount', { count: detail.artist.album_count }) }} ·
          {{ t('common.tracksCount', { count: detail.artist.track_count }) }}
        </p>

        <div class="flex flex-wrap items-center gap-2.5 pt-1">
          <button
            v-if="detail.top_tracks.length > 0"
            type="button"
            class="md-btn md-btn-filled md-state"
            @click="playerStore.playQueue(detail.top_tracks, 0)"
          >
            <LuPlay class="h-4 w-4 fill-current" />
            <span>{{ t('common.playAll') }}</span>
          </button>

          <button
            v-if="detail.top_tracks.length > 0"
            type="button"
            class="md-btn md-btn-tonal md-state"
            @click="playerStore.shuffleAndPlay(detail.top_tracks)"
          >
            <LuShuffle class="h-4 w-4" />
            <span>{{ t('common.shufflePlay') }}</span>
          </button>

          <button
            type="button"
            class="md-icon-btn md-state border border-[var(--md-sys-color-outline-variant)] bg-[var(--md-sys-color-surface-container-low)]"
            @click="musicStore.toggleArtistStar(detail.artist)"
          >
            <LuHeart
              class="h-4 w-4"
              :class="
                detail.artist.starred_at
                  ? 'fill-[var(--md-sys-color-error)] text-[var(--md-sys-color-error)]'
                  : 'text-[var(--md-sys-color-on-surface-variant)]'
              "
            />
          </button>

          <RefreshButton size="md" :action="loadArtist" />
        </div>
      </div>
    </div>

    <section v-if="detail.albums.length > 0">
      <h2 class="mb-3 text-base font-bold text-[var(--md-sys-color-on-surface)]">
        {{ t('nav.albums') }}
      </h2>
      <div class="grid grid-cols-2 gap-3 sm:grid-cols-3 md:grid-cols-4 lg:grid-cols-6">
        <AlbumCard v-for="al in detail.albums" :key="al.id" :album="al" />
      </div>
    </section>

    <section v-if="detail.top_tracks.length > 0">
      <h2 class="mb-3 text-base font-bold text-[var(--md-sys-color-on-surface)]">
        {{ t('catalog.topTracks') }}
      </h2>
      <TrackTable :tracks="detail.top_tracks" />
    </section>

    <section v-if="detail.appears_on_tracks.length > 0">
      <h2 class="mb-3 text-base font-bold text-[var(--md-sys-color-on-surface)]">
        {{ t('catalog.appearsOn') }}
      </h2>
      <TrackTable :tracks="detail.appears_on_tracks" />
    </section>
  </div>
</template>

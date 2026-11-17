<script setup lang="ts">
import { onMounted, ref, watch } from 'vue'
import { useI18n } from 'vue-i18n'
import { LuDisc3, LuHeart, LuPlay, LuPlus, LuSearch, LuTrash2 } from 'vue-icons-plus/lu'
import { api } from '@/api/client'
import { useEventsStore } from '@/stores/events'
import { useMusicPlayerStore } from '@/stores/musicPlayer'
import type {
  AlbumDetailDto,
  AlbumDto,
  ArtistDto,
  PlaylistDetailDto,
  PlaylistSummaryDto,
  TrackDto,
} from '@/types/api'

const { t } = useI18n()
const events = useEventsStore()
const player = useMusicPlayerStore()

type MusicTab = 'albums' | 'tracks' | 'artists' | 'playlists' | 'starred'
const tab = ref<MusicTab>('albums')
const search = ref('')

const albums = ref<AlbumDto[]>([])
const tracks = ref<TrackDto[]>([])
const artists = ref<ArtistDto[]>([])
const playlists = ref<PlaylistSummaryDto[]>([])
const starredTracks = ref<TrackDto[]>([])

const activeAlbum = ref<AlbumDetailDto | null>(null)
const activePlaylist = ref<PlaylistDetailDto | null>(null)
const newPlaylistName = ref('')

function fmtDur(secs: number): string {
  const m = Math.floor(secs / 60)
  const s = Math.floor(secs % 60)
  return `${m}:${String(s).padStart(2, '0')}`
}

async function loadTab(): Promise<void> {
  activeAlbum.value = null
  activePlaylist.value = null
  const q = search.value.trim()
  if (tab.value === 'albums') {
    const res = await api.listAlbums(q ? { search: q, size: '100' } : { size: '100' })
    albums.value = res.items
  } else if (tab.value === 'tracks') {
    const res = await api.listTracks(q ? { search: q, page_size: '200' } : { page_size: '200' })
    tracks.value = res.items
  } else if (tab.value === 'artists') {
    const res = await api.listArtists(q ? { search: q, size: '200' } : { size: '200' })
    artists.value = res.items
  } else if (tab.value === 'playlists') {
    playlists.value = await api.listPlaylists()
  } else {
    const st = await api.getStarredMusic()
    starredTracks.value = st.tracks
  }
}

onMounted(loadTab)
watch(tab, loadTab)
watch(() => events.changeTick.music, loadTab)

async function openAlbum(id: string): Promise<void> {
  activeAlbum.value = await api.getAlbum(id)
}

async function openPlaylist(id: string): Promise<void> {
  activePlaylist.value = await api.getPlaylist(id)
}

async function createNewPlaylist(): Promise<void> {
  if (!newPlaylistName.value.trim()) return
  await api.createPlaylist(newPlaylistName.value.trim())
  newPlaylistName.value = ''
  playlists.value = await api.listPlaylists()
}

async function removePlaylist(id: string, ev: Event): Promise<void> {
  ev.stopPropagation()
  await api.deletePlaylist(id)
  playlists.value = await api.listPlaylists()
}

async function toggleStar(tr: TrackDto, ev: Event): Promise<void> {
  ev.stopPropagation()
  if (tr.starred_at) {
    await api.unstarMusic([tr.id])
    tr.starred_at = null
  } else {
    await api.starMusic([tr.id])
    tr.starred_at = new Date().toISOString()
  }
}
</script>

<template>
  <div class="mx-auto max-w-7xl space-y-6 p-6">
    <!-- Top Tabs & Search -->
    <div class="flex flex-wrap items-center justify-between gap-3">
      <div class="flex flex-wrap gap-1.5 rounded-xl border border-[var(--ctp-surface0)] bg-[var(--ctp-mantle)] p-1 text-xs">
        <button
          v-for="item in (['albums', 'tracks', 'artists', 'playlists', 'starred'] as MusicTab[])"
          :key="item"
          type="button"
          class="rounded-lg px-3 py-1.5 capitalize transition"
          :class="
            tab === item
              ? 'bg-[var(--ctp-accent)] text-[var(--ctp-on-accent)] font-semibold'
              : 'text-[var(--ctp-subtext1)] hover:text-[var(--ctp-text)]'
          "
          @click="tab = item"
        >
          {{ item }}
        </button>
      </div>

      <div class="relative min-w-60 max-w-sm flex-1">
        <LuSearch class="pointer-events-none absolute left-3 top-1/2 h-4 w-4 -translate-y-1/2 text-[var(--ctp-subtext0)]" />
        <input
          v-model="search"
          :placeholder="t('common.search')"
          class="w-full rounded-xl border border-[var(--ctp-surface0)] bg-[var(--ctp-mantle)] pl-9 pr-4 py-2 text-sm outline-none focus:border-[var(--ctp-accent)]"
          @keyup.enter="loadTab"
        />
      </div>
    </div>

    <!-- Album Drilldown -->
    <div v-if="activeAlbum" class="space-y-5">
      <button type="button" class="text-xs text-[var(--ctp-accent)] hover:underline" @click="activeAlbum = null">
        ← Back to Albums
      </button>
      <div class="flex flex-col gap-6 rounded-2xl border border-[var(--ctp-surface0)] bg-[var(--ctp-mantle)] p-6 sm:flex-row">
        <div class="h-40 w-40 shrink-0 overflow-hidden rounded-xl bg-[var(--ctp-surface0)] media-shadow flex items-center justify-center">
          <img
            v-if="activeAlbum.album.cover_key"
            :src="api.musicCoverUrl(activeAlbum.album.id, 360) ?? ''"
            alt=""
            class="h-full w-full object-cover"
          />
          <LuDisc3 v-else class="h-12 w-12 text-[var(--ctp-subtext0)]" />
        </div>
        <div class="space-y-2">
          <h2 class="text-2xl font-bold">{{ activeAlbum.album.title }}</h2>
          <p class="text-sm text-[var(--ctp-subtext1)]">
            {{ activeAlbum.album.album_artist_name }}
            <span v-if="activeAlbum.album.year"> · {{ activeAlbum.album.year }}</span>
          </p>
          <button
            type="button"
            class="mt-2 inline-flex items-center gap-2 rounded-xl bg-[var(--ctp-accent)] px-4 py-2 text-xs font-semibold text-[var(--ctp-on-accent)]"
            @click="player.playTracks(activeAlbum.tracks, 0)"
          >
            <LuPlay class="h-4 w-4" /> Play Album ({{ activeAlbum.tracks.length }})
          </button>
        </div>
      </div>

      <div class="divide-y divide-[var(--ctp-surface0)] rounded-2xl border border-[var(--ctp-surface0)] bg-[var(--ctp-mantle)]">
        <div
          v-for="(tr, idx) in activeAlbum.tracks"
          :key="tr.id"
          class="flex cursor-pointer items-center justify-between px-4 py-3 text-sm hover:bg-[var(--ctp-surface0)]/40"
          @click="player.playTracks(activeAlbum.tracks, idx)"
        >
          <div class="flex items-center gap-3 min-w-0">
            <span class="w-6 text-right text-xs text-[var(--ctp-subtext0)] tabular-nums">
              {{ tr.track_number ?? idx + 1 }}
            </span>
            <span class="truncate font-medium">{{ tr.title }}</span>
            <span class="rounded bg-[var(--ctp-surface0)] px-1.5 py-0.5 text-[10px] uppercase text-[var(--ctp-subtext0)]">
              {{ tr.format }}
            </span>
          </div>
          <div class="flex items-center gap-3 text-xs text-[var(--ctp-subtext0)]">
            <button type="button" @click="toggleStar(tr, $event)">
              <LuHeart class="h-4 w-4" :class="tr.starred_at ? 'fill-current text-[var(--ctp-red)]' : ''" />
            </button>
            <span class="tabular-nums">{{ fmtDur(tr.duration_secs) }}</span>
          </div>
        </div>
      </div>
    </div>

    <!-- Playlist Drilldown -->
    <div v-else-if="activePlaylist" class="space-y-4">
      <button type="button" class="text-xs text-[var(--ctp-accent)] hover:underline" @click="activePlaylist = null">
        ← Back to Playlists
      </button>
      <div class="flex items-center justify-between rounded-2xl border border-[var(--ctp-surface0)] bg-[var(--ctp-mantle)] p-5">
        <div>
          <h2 class="text-xl font-bold">{{ activePlaylist.playlist.name }}</h2>
          <p class="text-xs text-[var(--ctp-subtext0)]">{{ activePlaylist.tracks.length }} tracks</p>
        </div>
        <button
          v-if="activePlaylist.tracks.length > 0"
          type="button"
          class="inline-flex items-center gap-2 rounded-xl bg-[var(--ctp-accent)] px-4 py-2 text-xs font-semibold text-[var(--ctp-on-accent)]"
          @click="player.playTracks(activePlaylist.tracks, 0)"
        >
          <LuPlay class="h-4 w-4" /> Play All
        </button>
      </div>
      <div class="divide-y divide-[var(--ctp-surface0)] rounded-2xl border border-[var(--ctp-surface0)] bg-[var(--ctp-mantle)]">
        <div
          v-for="(tr, idx) in activePlaylist.tracks"
          :key="tr.id"
          class="flex cursor-pointer items-center justify-between px-4 py-3 text-sm hover:bg-[var(--ctp-surface0)]/40"
          @click="player.playTracks(activePlaylist.tracks, idx)"
        >
          <div class="truncate font-medium">{{ tr.title }} — {{ tr.artist_name }}</div>
          <span class="text-xs tabular-nums text-[var(--ctp-subtext0)]">{{ fmtDur(tr.duration_secs) }}</span>
        </div>
      </div>
    </div>

    <!-- Albums Grid -->
    <div v-else-if="tab === 'albums'" class="grid grid-cols-2 gap-5 sm:grid-cols-3 md:grid-cols-5 lg:grid-cols-6">
      <div
        v-for="al in albums"
        :key="al.id"
        class="group cursor-pointer space-y-2"
        @click="openAlbum(al.id)"
      >
        <div class="aspect-square overflow-hidden rounded-xl border border-[var(--ctp-surface0)] bg-[var(--ctp-mantle)] media-shadow flex items-center justify-center">
          <img
            v-if="al.cover_key"
            :src="api.musicCoverUrl(al.id, 320) ?? ''"
            alt=""
            class="h-full w-full object-cover transition group-hover:scale-105"
          />
          <LuDisc3 v-else class="h-10 w-10 text-[var(--ctp-subtext0)]" />
        </div>
        <div class="truncate text-xs font-semibold">{{ al.title }}</div>
        <div class="truncate text-[11px] text-[var(--ctp-subtext0)]">{{ al.album_artist_name }}</div>
      </div>
    </div>

    <!-- Tracks / Starred Table -->
    <div
      v-else-if="tab === 'tracks' || tab === 'starred'"
      class="divide-y divide-[var(--ctp-surface0)] rounded-2xl border border-[var(--ctp-surface0)] bg-[var(--ctp-mantle)]"
    >
      <div
        v-for="(tr, idx) in (tab === 'tracks' ? tracks : starredTracks)"
        :key="tr.id"
        class="flex cursor-pointer items-center justify-between px-4 py-3 text-sm hover:bg-[var(--ctp-surface0)]/40"
        @click="player.playTracks(tab === 'tracks' ? tracks : starredTracks, idx)"
      >
        <div class="min-w-0">
          <div class="truncate font-medium">{{ tr.title }}</div>
          <div class="truncate text-xs text-[var(--ctp-subtext0)]">
            {{ tr.artist_name }} · {{ tr.album_title }}
          </div>
        </div>
        <div class="flex items-center gap-3 text-xs text-[var(--ctp-subtext0)]">
          <span class="rounded bg-[var(--ctp-surface0)] px-1.5 py-0.5 text-[10px] uppercase">
            {{ tr.format }}
          </span>
          <button type="button" @click="toggleStar(tr, $event)">
            <LuHeart class="h-4 w-4" :class="tr.starred_at ? 'fill-current text-[var(--ctp-red)]' : ''" />
          </button>
          <span class="tabular-nums">{{ fmtDur(tr.duration_secs) }}</span>
        </div>
      </div>
    </div>

    <!-- Artists Grid -->
    <div v-else-if="tab === 'artists'" class="grid grid-cols-2 gap-4 sm:grid-cols-3 md:grid-cols-5">
      <div
        v-for="ar in artists"
        :key="ar.id"
        class="rounded-xl border border-[var(--ctp-surface0)] bg-[var(--ctp-mantle)] p-4"
      >
        <div class="truncate text-sm font-semibold">{{ ar.name }}</div>
        <div class="mt-1 text-xs text-[var(--ctp-subtext0)]">
          {{ ar.album_count }} albums · {{ ar.track_count }} tracks
        </div>
      </div>
    </div>

    <!-- Playlists -->
    <div v-else class="space-y-4">
      <form class="flex max-w-sm gap-2" @submit.prevent="createNewPlaylist">
        <input
          v-model="newPlaylistName"
          placeholder="New playlist name…"
          class="flex-1 rounded-xl border border-[var(--ctp-surface0)] bg-[var(--ctp-mantle)] px-3 py-2 text-sm"
        />
        <button
          type="submit"
          class="inline-flex items-center gap-1 rounded-xl bg-[var(--ctp-accent)] px-3 py-2 text-xs font-semibold text-[var(--ctp-on-accent)]"
        >
          <LuPlus class="h-4 w-4" /> Create
        </button>
      </form>

      <div class="grid grid-cols-1 gap-3 sm:grid-cols-2 md:grid-cols-3">
        <div
          v-for="pl in playlists"
          :key="pl.id"
          class="flex cursor-pointer items-center justify-between rounded-xl border border-[var(--ctp-surface0)] bg-[var(--ctp-mantle)] p-4 hover:border-[var(--ctp-accent)]"
          @click="openPlaylist(pl.id)"
        >
          <div>
            <div class="font-semibold text-sm">{{ pl.name }}</div>
            <div class="text-xs text-[var(--ctp-subtext0)]">{{ pl.track_count }} tracks</div>
          </div>
          <button
            type="button"
            class="p-1.5 text-[var(--ctp-subtext0)] hover:text-[var(--ctp-red)]"
            @click="removePlaylist(pl.id, $event)"
          >
            <LuTrash2 class="h-4 w-4" />
          </button>
        </div>
      </div>
    </div>
  </div>
</template>

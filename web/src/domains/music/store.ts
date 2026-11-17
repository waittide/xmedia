import { ref, watch } from 'vue'
import { defineStore } from 'pinia'
import { useEventsStore } from '@/stores/events'
import { api } from './api'
import type {
  AlbumDto,
  ArtistDto,
  GenreStatDto,
  LibraryDto,
  PlaylistSummaryDto,
  TrackDto,
} from './types'

export const useMusicStore = defineStore('music', () => {
  const eventsStore = useEventsStore()

  const libraries = ref<LibraryDto[]>([])
  const playlists = ref<PlaylistSummaryDto[]>([])
  const genres = ref<GenreStatDto[]>([])
  const globalSearchQuery = ref('')
  const catalogVersion = ref(0)

  async function refreshSidebar(): Promise<void> {
    const [libs, pls, gs] = await Promise.allSettled([
      api.listLibraries(),
      api.listPlaylists(),
      api.listGenres(),
    ])
    if (libs.status === 'fulfilled') libraries.value = libs.value
    if (pls.status === 'fulfilled') playlists.value = pls.value
    if (gs.status === 'fulfilled') genres.value = gs.value
  }

  function bumpCatalogVersion(): void {
    catalogVersion.value += 1
  }

  async function toggleTrackStar(track: TrackDto): Promise<boolean> {
    if (track.starred_at) {
      await api.unstar({ ids: [track.id] })
      track.starred_at = null
      return false
    } else {
      await api.star({ ids: [track.id] })
      track.starred_at = new Date().toISOString()
      return true
    }
  }

  async function toggleAlbumStar(album: AlbumDto): Promise<boolean> {
    if (album.starred_at) {
      await api.unstar({ album_ids: [album.id] })
      album.starred_at = null
      return false
    } else {
      await api.star({ album_ids: [album.id] })
      album.starred_at = new Date().toISOString()
      return true
    }
  }

  async function toggleArtistStar(artist: ArtistDto): Promise<boolean> {
    if (artist.starred_at) {
      await api.unstar({ artist_ids: [artist.id] })
      artist.starred_at = null
      return false
    } else {
      await api.star({ artist_ids: [artist.id] })
      artist.starred_at = new Date().toISOString()
      return true
    }
  }

  async function addTrackToPlaylist(playlistId: string, trackId: string): Promise<void> {
    await api.updatePlaylist(playlistId, { add_track_ids: [trackId] })
    await refreshSidebar()
  }

  watch(
    () => eventsStore.domainRefreshTick.music,
    () => {
      bumpCatalogVersion()
      void refreshSidebar().catch(() => {})
    },
  )

  return {
    libraries,
    playlists,
    genres,
    globalSearchQuery,
    catalogVersion,
    refreshSidebar,
    bumpCatalogVersion,
    toggleTrackStar,
    toggleAlbumStar,
    toggleArtistStar,
    addTrackToPlaylist,
  }
})

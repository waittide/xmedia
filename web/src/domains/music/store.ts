import { ref, watch } from 'vue'
import { defineStore } from 'pinia'
import { useEventsStore } from '@/stores/events'
import { api } from './api'
import type {
  AlbumDto,
  ArtistDto,
  GenreStatDto,
  LibraryDto,
  MusicTagStatDto,
  PlaylistSummaryDto,
  TrackDto,
} from './types'

export const useMusicStore = defineStore('music', () => {
  const eventsStore = useEventsStore()

  const libraries = ref<LibraryDto[]>([])
  const playlists = ref<PlaylistSummaryDto[]>([])
  const genres = ref<GenreStatDto[]>([])
  const tags = ref<MusicTagStatDto[]>([])
  const artists = ref<ArtistDto[]>([])
  const albums = ref<AlbumDto[]>([])
  const globalSearchQuery = ref('')
  const catalogVersion = ref(0)

  async function refreshSidebar(): Promise<void> {
    const [libs, pls, gs, tgs, ars, als] = await Promise.allSettled([
      api.listLibraries(),
      api.listPlaylists(),
      api.listGenres(),
      api.listTags(),
      api.listArtists({ size: 24 }),
      api.listAlbums({ type: 'newest', size: 24 }),
    ])
    if (libs.status === 'fulfilled') libraries.value = libs.value
    if (pls.status === 'fulfilled') playlists.value = pls.value
    if (gs.status === 'fulfilled') genres.value = gs.value
    if (tgs.status === 'fulfilled') tags.value = tgs.value
    if (ars.status === 'fulfilled') artists.value = ars.value.items
    if (als.status === 'fulfilled') albums.value = als.value.items
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
    tags,
    artists,
    albums,
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

import { api as coreApi, requestJson, withAuthToken } from '@/api/client'
import { sendKeepalivePutJson } from '@/domains/shared/keepalive'
import type {
  BindMediaItemsResultDto,
  CoreLibraryDto,
  UnbindMediaItemResultDto,
} from '@/types/api'
import type {
  AlbumDetailDto,
  AlbumDto,
  AlbumListType,
  ArtistDetailDto,
  ArtistDto,
  ArtistRoleFilter,
  AudioFormat,
  BindMusicItemsInput,
  BookmarkDto,
  GenreStatDto,
  HomeOverviewDto,
  LibraryDto,
  MusicStatusDto,
  MusicTagStatDto,
  PagedResult,
  PlayQueueDto,
  PlaylistDetailDto,
  PlaylistSummaryDto,
  PoolTrackDto,
  SearchResultDto,
  SortOrder,
  StarredBundleDto,
  StructuredLyricsDto,
  TrackDto,
  TrackSortField,
  TranscodeProfile,
} from './types'

function toQuery(params: Record<string, string | number | boolean | undefined | null>): string {
  const sp = new URLSearchParams()
  for (const [k, v] of Object.entries(params)) {
    if (v !== undefined && v !== null && v !== '') {
      sp.set(k, String(v))
    }
  }
  const s = sp.toString()
  return s ? `?${s}` : ''
}

export function mapMusicLibrary(raw: CoreLibraryDto): LibraryDto {
  return {
    id: raw.id,
    name: raw.name,
    kind: raw.kind,
    track_count: raw.item_count,
    scan_status: raw.scan_status,
    last_scanned_at: raw.last_scanned_at,
    created_at: raw.created_at,
    updated_at: raw.updated_at,
  }
}

export function buildCoverUrl(idOrKey: string | null | undefined, size = 300): string | null {
  if (!idOrKey) return null
  return withAuthToken(`/api/v1/music/covers/${encodeURIComponent(idOrKey)}?size=${size}`)
}

let cachedAlacSupport: boolean | null = null

export function canPlayFormatNatively(format: AudioFormat): boolean {
  if (format !== 'alac') return true
  if (cachedAlacSupport === null) {
    const probe = document.createElement('audio')
    cachedAlacSupport = probe.canPlayType('audio/mp4; codecs="alac"') !== ''
  }
  return cachedAlacSupport
}

export function buildStreamUrl(
  trackId: string,
  profile: TranscodeProfile,
  timeOffsetSecs = 0,
  sourceFormat?: AudioFormat,
): string {
  const sp = new URLSearchParams()

  let isTranscoded = profile !== 'raw'
  if (profile === 'raw') {
    if (sourceFormat && !canPlayFormatNatively(sourceFormat)) {
      sp.set('format', 'flac')
      isTranscoded = true
    } else {
      sp.set('format', 'raw')
    }
  } else if (profile === 'opus128') {
    sp.set('format', 'opus')
    sp.set('max_bit_rate', '128')
  } else if (profile === 'mp3192') {
    sp.set('format', 'mp3')
    sp.set('max_bit_rate', '192')
  } else if (profile === 'mp3320') {
    sp.set('format', 'mp3')
    sp.set('max_bit_rate', '320')
  } else if (profile === 'aac256') {
    sp.set('format', 'aac')
    sp.set('max_bit_rate', '256')
  }

  if (timeOffsetSecs > 0 && isTranscoded) {
    sp.set('time_offset', String(Math.floor(timeOffsetSecs)))
  }

  return withAuthToken(`/api/v1/music/tracks/${encodeURIComponent(trackId)}/stream?${sp.toString()}`)
}

export function buildDownloadUrl(trackId: string): string {
  return withAuthToken(`/api/v1/music/tracks/${encodeURIComponent(trackId)}/download`)
}

export const api = {
  musicStatus: () => requestJson<MusicStatusDto>('/music/status'),
  listLibraries: async (): Promise<LibraryDto[]> => {
    const libs = await coreApi.listLibraries('music')
    return libs.map(mapMusicLibrary)
  },
  scanLibrary: async (id: string): Promise<LibraryDto> =>
    mapMusicLibrary(await coreApi.scanLibrary(id)),

  homeOverview: () => requestJson<HomeOverviewDto>('/music/home'),
  queryTracks: (params: {
    library_id?: string
    album_id?: string
    artist_id?: string
    genre?: string
    tag?: string
    year_from?: number
    year_to?: number
    search?: string
    sort_by?: TrackSortField
    sort_order?: SortOrder
    page?: number
    page_size?: number
  }) => requestJson<PagedResult<TrackDto>>(`/music/tracks${toQuery(params)}`),
  listPool: (
    params: {
      library_id?: string
      album_id?: string
      artist_id?: string
      genre?: string
      search?: string
      sort_by?: TrackSortField
      sort_order?: SortOrder
      page?: number
      page_size?: number
    } = {},
  ) => requestJson<PagedResult<PoolTrackDto>>(`/music/pool${toQuery(params)}`),
  bindItems: (input: BindMusicItemsInput) =>
    requestJson<BindMediaItemsResultDto>('/music/bindings', {
      method: 'POST',
      body: JSON.stringify(input),
    }),
  unbindItem: (trackId: string, userId?: string | null) =>
    requestJson<UnbindMediaItemResultDto>(
      `/music/bindings/${encodeURIComponent(trackId)}${toQuery({ user_id: userId })}`,
      { method: 'DELETE' },
    ),
  randomTracks: (size = 20, genre?: string) =>
    requestJson<TrackDto[]>(`/music/tracks/random${toQuery({ size, genre })}`),
  recentTracks: (limit = 50) =>
    requestJson<TrackDto[]>(`/music/tracks/recent${toQuery({ limit })}`),
  getTrack: (id: string) => requestJson<TrackDto>(`/music/tracks/${encodeURIComponent(id)}`),
  getTrackLyrics: (id: string) =>
    requestJson<StructuredLyricsDto>(`/music/tracks/${encodeURIComponent(id)}/lyrics`),
  listAlbums: (params: {
    type?: AlbumListType
    library_id?: string
    genre?: string
    from_year?: number
    to_year?: number
    search?: string
    offset?: number
    size?: number
  }) => requestJson<PagedResult<AlbumDto>>(`/music/albums${toQuery(params)}`),
  getAlbum: (id: string) => requestJson<AlbumDetailDto>(`/music/albums/${encodeURIComponent(id)}`),
  listArtists: (params: {
    search?: string
    role?: ArtistRoleFilter
    offset?: number
    size?: number
  }) => requestJson<PagedResult<ArtistDto>>(`/music/artists${toQuery(params)}`),
  getArtist: (id: string) => requestJson<ArtistDetailDto>(`/music/artists/${encodeURIComponent(id)}`),
  listGenres: () => requestJson<GenreStatDto[]>('/music/genres'),
  listTags: () => requestJson<MusicTagStatDto[]>('/music/tags'),
  setTrackTags: (trackId: string, tags: string[]) =>
    requestJson<TrackDto>(`/music/tracks/${encodeURIComponent(trackId)}/tags`, {
      method: 'PUT',
      body: JSON.stringify({ tags }),
    }),
  deleteTrack: (trackId: string) =>
    requestJson<{ ok: boolean }>(`/music/tracks/${encodeURIComponent(trackId)}`, {
      method: 'DELETE',
    }),
  search: (q: string, limit = 24) =>
    requestJson<SearchResultDto>(`/music/search${toQuery({ q, limit })}`),

  listPlaylists: () => requestJson<PlaylistSummaryDto[]>('/music/playlists'),
  createPlaylist: (payload: {
    name: string
    comment?: string
    is_public?: boolean
    track_ids?: string[]
  }) =>
    requestJson<PlaylistDetailDto>('/music/playlists', {
      method: 'POST',
      body: JSON.stringify(payload),
    }),
  getPlaylist: (id: string) =>
    requestJson<PlaylistDetailDto>(`/music/playlists/${encodeURIComponent(id)}`),
  updatePlaylist: (
    id: string,
    payload: {
      name?: string
      comment?: string
      is_public?: boolean
      replace_track_ids?: string[]
      add_track_ids?: string[]
      remove_indices?: number[]
    },
  ) =>
    requestJson<PlaylistDetailDto>(`/music/playlists/${encodeURIComponent(id)}`, {
      method: 'PATCH',
      body: JSON.stringify(payload),
    }),
  deletePlaylist: (id: string) =>
    requestJson<{ ok: boolean }>(`/music/playlists/${encodeURIComponent(id)}`, { method: 'DELETE' }),

  getStarred: () => requestJson<StarredBundleDto>('/music/starred'),
  star: (payload: { ids?: string[]; album_ids?: string[]; artist_ids?: string[] }) =>
    requestJson<{ ok: boolean }>('/music/starred', {
      method: 'POST',
      body: JSON.stringify(payload),
    }),
  unstar: (payload: { ids?: string[]; album_ids?: string[]; artist_ids?: string[] }) =>
    requestJson<{ ok: boolean }>('/music/unstarred', {
      method: 'POST',
      body: JSON.stringify(payload),
    }),
  setRating: (id: string, rating: number) =>
    requestJson<{ ok: boolean }>('/music/ratings', {
      method: 'POST',
      body: JSON.stringify({ id, rating }),
    }),
  scrobble: (track_id: string, submission: boolean) =>
    requestJson<{ ok: boolean }>('/music/scrobble', {
      method: 'POST',
      body: JSON.stringify({ track_id, submission, played_at_ms: Date.now() }),
    }),
  getPlayQueue: () => requestJson<PlayQueueDto | null>('/music/play-queue'),
  savePlayQueue: (payload: {
    track_ids: string[]
    current_track_id: string | null
    position_ms: number
  }) =>
    requestJson<{ ok: boolean }>('/music/play-queue', {
      method: 'PUT',
      body: JSON.stringify(payload),
    }),
  savePlayQueueKeepalive: (payload: {
    track_ids: string[]
    current_track_id: string | null
    position_ms: number
  }) => {
    sendKeepalivePutJson('/api/v1/music/play-queue', payload)
  },
  listBookmarks: () => requestJson<BookmarkDto[]>('/music/bookmarks'),
  saveBookmark: (trackId: string, position_ms: number, comment?: string) =>
    requestJson<BookmarkDto>(`/music/bookmarks/${encodeURIComponent(trackId)}`, {
      method: 'PUT',
      body: JSON.stringify({ position_ms, comment }),
    }),
  deleteBookmark: (trackId: string) =>
    requestJson<{ ok: boolean }>(`/music/bookmarks/${encodeURIComponent(trackId)}`, {
      method: 'DELETE',
    }),
}

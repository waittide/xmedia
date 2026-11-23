import type { PlayMode, TranscodeProfile } from '@/types/api'

export type { PlayMode, TranscodeProfile }

export type AudioFormat = 'flac' | 'mp3' | 'm4a' | 'alac' | 'ogg' | 'opus' | 'wav'
export type SortOrder = 'asc' | 'desc'
export type TrackSortField =
  | 'title'
  | 'artist'
  | 'album'
  | 'created_at'
  | 'year'
  | 'duration'
  | 'track_order'

export type AlbumListType =
  | 'newest'
  | 'recent'
  | 'frequent'
  | 'random'
  | 'starred'
  | 'alphabeticalByName'
  | 'alphabeticalByArtist'
  | 'byYear'
  | 'byGenre'

export type ArtistRoleFilter = 'all' | 'album_artist' | 'track_artist'

export type LibraryKind =
  | { type: 'managed' }
  | { type: 'local_mount'; root_path: string }

export type ScanStatus =
  | { state: 'idle' }
  | { state: 'scanning'; scanned: number; total?: number | null }
  | { state: 'completed'; scanned: number; added: number; updated: number; missing: number }
  | { state: 'failed'; message: string }

export interface LibraryDto {
  id: string
  name: string
  kind: LibraryKind
  track_count: number
  scan_status: ScanStatus
  last_scanned_at: string | null
  created_at: string
  updated_at: string
}

export interface MusicStatusDto {
  ffmpeg: {
    available: boolean
    version: string | null
  }
  scan: {
    scanning: boolean
    count: number
  }
  subsonic_api_version: string
  open_subsonic: boolean
}

export interface ReplayGain {
  track_gain_db: number | null
  track_peak: number | null
  album_gain_db: number | null
  album_peak: number | null
}

export interface OnlineTrackedTrackDto {
  source_id: string
  item_id: string
  cached: boolean
}

export interface TrackDto {
  id: string
  library_id: string
  album_id: string
  artist_id: string
  album_artist_id: string
  title: string
  artist_name: string
  album_artist_name: string
  album_title: string
  track_number: number | null
  disc_number: number | null
  year: number | null
  genre: string | null
  duration_secs: number
  bit_rate_kbps: number | null
  sample_rate_hz: number | null
  bit_depth: number | null
  channels: number | null
  format: AudioFormat
  mime_type: string
  is_lossless: boolean
  file_size: number
  replay_gain: ReplayGain
  has_lyrics: boolean
  has_embedded_lyrics: boolean
  has_sidecar_lyrics: boolean
  is_compilation: boolean
  cover_key: string | null
  path: string
  tags: string[]
  starred_at: string | null
  user_rating: number | null
  play_count: number
  last_played_at: string | null
  created_at: string
  online_tracking?: OnlineTrackedTrackDto | null
}

export interface PoolTrackDto extends TrackDto {
  bound: boolean
}

export interface BindMusicItemsInput {
  item_ids?: string[]
  album_id?: string | null
  artist_id?: string | null
  library_id?: string | null
  user_id?: string | null
}

export interface AlbumDto {
  id: string
  library_id: string
  title: string
  album_artist_id: string
  album_artist_name: string
  year: number | null
  genre: string | null
  disc_count: number
  track_count: number
  duration_secs: number
  is_compilation: boolean
  cover_key: string | null
  starred_at: string | null
  user_rating: number | null
  play_count: number
  created_at: string
}

export interface ArtistDto {
  id: string
  name: string
  is_album_artist: boolean
  is_track_artist: boolean
  album_count: number
  track_count: number
  cover_key: string | null
  starred_at: string | null
  user_rating: number | null
  created_at: string
}

export interface AlbumDetailDto {
  album: AlbumDto
  tracks: TrackDto[]
}

export interface ArtistDetailDto {
  artist: ArtistDto
  albums: AlbumDto[]
  appears_on_tracks: TrackDto[]
  top_tracks: TrackDto[]
}

export interface GenreStatDto {
  name: string
  song_count: number
  album_count: number
}

export interface MusicTagStatDto {
  name: string
  song_count: number
}

export interface LyricLineDto {
  start_ms: number
  text: string
}

export interface StructuredLyricsDto {
  lang: string
  synced: boolean
  display_artist: string | null
  display_title: string | null
  offset_ms: number
  lines: LyricLineDto[]
  plain_text: string
}

export interface PagedResult<T> {
  items: T[]
  total: number
  page: number
  page_size: number
}

export interface SearchResultDto {
  artists: ArtistDto[]
  albums: AlbumDto[]
  tracks: TrackDto[]
}

export interface HomeOverviewDto {
  stats: {
    total_tracks: number
    total_albums: number
    total_artists: number
    starred_tracks: number
    playlists_count: number
    listening_count: number
    bookmarks_count: number
    genres_count: number
    tags_count: number
    libraries_count: number
  }
  recent_added_albums: AlbumDto[]
  frequent_albums: AlbumDto[]
  recent_played_tracks: TrackDto[]
  random_tracks: TrackDto[]
  bookmarks: BookmarkDto[]
  play_queue: PlayQueueDto | null
}

export interface PlaylistSummaryDto {
  id: string
  owner_id: string
  owner_username: string
  name: string
  comment: string | null
  is_public: boolean
  track_count: number
  duration_secs: number
  cover_key: string | null
  created_at: string
  updated_at: string
}

export interface PlaylistDetailDto {
  playlist: PlaylistSummaryDto
  tracks: TrackDto[]
}

export interface StarredBundleDto {
  artists: ArtistDto[]
  albums: AlbumDto[]
  tracks: TrackDto[]
}

export interface PlayQueueDto {
  tracks: TrackDto[]
  current_track_id: string | null
  position_ms: number
  changed_by: string
  updated_at: string
}

export interface BookmarkDto {
  track: TrackDto
  position_ms: number
  comment: string | null
  created_at: string
  updated_at: string
}

export interface UploadBatchResultDto {
  uploaded_tracks: TrackDto[]
  attached_lyrics_count: number
}

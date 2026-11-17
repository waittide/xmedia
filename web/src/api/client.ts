import type {
  AlbumDetailDto,
  AlbumDto,
  ArtistDetailDto,
  ArtistDto,
  AuthSessionDto,
  BookBookmark,
  BookShelfDto,
  BookSummaryDto,
  ComicHomeOverviewDto,
  ComicReaderContextDto,
  LibraryDto,
  MediaKind,
  MusicHomeOverviewDto,
  PagedResult,
  PlayQueueDto,
  PlaylistDetailDto,
  PlaylistSummaryDto,
  SeriesDetailDto,
  SeriesSummaryDto,
  ShowDetailDto,
  ShowSummaryDto,
  StructuredLyricsDto,
  SystemCapabilitiesDto,
  SystemOverviewDto,
  TocItem,
  TrackDto,
  TxtChapterContent,
  UserDto,
  UserPreferences,
  UserRole,
  VideoDetailDto,
  VideoHomeOverviewDto,
  VideoQualityProfile,
  VideoSummaryDto,
  WatchProgress,
} from '@/types/api'

const TOKEN_STORAGE_KEY = 'xmedia.auth_token'

export function getStoredToken(): string | null {
  return localStorage.getItem(TOKEN_STORAGE_KEY)
}

export function setStoredToken(token: string | null): void {
  if (token) {
    localStorage.setItem(TOKEN_STORAGE_KEY, token)
  } else {
    localStorage.removeItem(TOKEN_STORAGE_KEY)
  }
}

export function withAuthQuery(url: string): string {
  const token = getStoredToken()
  if (!token) return url
  const sep = url.includes('?') ? '&' : '?'
  return `${url}${sep}token=${encodeURIComponent(token)}`
}

async function request<T>(path: string, init: RequestInit = {}): Promise<T> {
  const headers = new Headers(init.headers)
  const token = getStoredToken()
  if (token && !headers.has('Authorization')) {
    headers.set('Authorization', `Bearer ${token}`)
  }
  if (init.body && !(init.body instanceof FormData) && !headers.has('Content-Type')) {
    headers.set('Content-Type', 'application/json')
  }

  const res = await fetch(path, {
    ...init,
    headers,
    credentials: 'include',
  })

  if (!res.ok) {
    let msg = `HTTP ${res.status}`
    try {
      const errJson = (await res.json()) as { error?: { message?: string } }
      if (errJson.error?.message) msg = errJson.error.message
    } catch {
      // ignore non-json error body
    }
    throw new Error(msg)
  }

  if (res.status === 204) {
    return undefined as T
  }
  return (await res.json()) as T
}

export const api = {
  // --- Core Auth, Users, Libraries, System ---
  setupStatus: () => request<{ needs_setup: boolean }>('/api/v1/auth/setup-status'),
  setupAdmin: (payload: { username: string; display_name?: string; password: string }) =>
    request<AuthSessionDto>('/api/v1/auth/setup', {
      method: 'POST',
      body: JSON.stringify(payload),
    }),
  login: (payload: { username: string; password: string }) =>
    request<AuthSessionDto>('/api/v1/auth/login', {
      method: 'POST',
      body: JSON.stringify(payload),
    }),
  logout: () => request<{ ok: boolean }>('/api/v1/auth/logout', { method: 'POST' }),
  getMe: () => request<UserDto>('/api/v1/auth/me'),
  updatePreferences: (preferences: UserPreferences) =>
    request<UserDto>('/api/v1/auth/preferences', {
      method: 'PUT',
      body: JSON.stringify(preferences),
    }),
  changePassword: (current_password: string, new_password: string) =>
    request<UserDto>('/api/v1/auth/password', {
      method: 'POST',
      body: JSON.stringify({ current_password, new_password }),
    }),
  rotateApiKey: () => request<UserDto>('/api/v1/auth/rotate-api-key', { method: 'POST' }),

  capabilities: () => request<SystemCapabilitiesDto>('/api/v1/system/capabilities'),
  systemOverview: () => request<SystemOverviewDto>('/api/v1/system/overview'),

  listUsers: () => request<UserDto[]>('/api/v1/users'),
  createUser: (payload: {
    username: string
    display_name?: string
    password: string
    role: UserRole
  }) => request<UserDto>('/api/v1/users', { method: 'POST', body: JSON.stringify(payload) }),
  updateUser: (
    id: string,
    payload: { display_name?: string; role?: UserRole; is_active?: boolean; password?: string },
  ) => request<UserDto>(`/api/v1/users/${id}`, { method: 'PATCH', body: JSON.stringify(payload) }),
  deleteUser: (id: string) => request<{ ok: boolean }>(`/api/v1/users/${id}`, { method: 'DELETE' }),

  listLibraries: (mediaKind?: MediaKind) =>
    request<LibraryDto[]>(
      mediaKind ? `/api/v1/libraries?media_kind=${mediaKind}` : '/api/v1/libraries',
    ),
  createLibrary: (payload: { name: string; media_kind: MediaKind; root_path: string }) =>
    request<LibraryDto>('/api/v1/libraries', {
      method: 'POST',
      body: JSON.stringify(payload),
    }),
  scanLibrary: (id: string) =>
    request<LibraryDto>(`/api/v1/libraries/${id}/scan`, { method: 'POST' }),
  deleteLibrary: (id: string) =>
    request<{ ok: boolean }>(`/api/v1/libraries/${id}`, { method: 'DELETE' }),

  // --- Books ---
  listBooks: (params: Record<string, string> = {}) => {
    const qs = new URLSearchParams(params).toString()
    return request<PagedResult<BookSummaryDto>>(`/api/v1/book/books${qs ? `?${qs}` : ''}`)
  },
  recentBooks: (limit = 8) =>
    request<BookSummaryDto[]>(`/api/v1/book/recent?limit=${limit}`),
  getBook: (id: string) => request<BookSummaryDto>(`/api/v1/book/books/${id}`),
  deleteBook: (id: string) => request<void>(`/api/v1/book/books/${id}`, { method: 'DELETE' }),
  setBookFavorite: (id: string, favorite: boolean) =>
    request<void>(`/api/v1/book/books/${id}/favorite`, {
      method: 'PUT',
      body: JSON.stringify({ favorite }),
    }),
  getBookToc: (id: string) => request<TocItem[]>(`/api/v1/book/books/${id}/toc`),
  getTxtChapter: (id: string, index: number) =>
    request<TxtChapterContent>(`/api/v1/book/books/${id}/chapters/${index}`),
  saveBookProgress: (
    id: string,
    payload: {
      progress_percent: number
      locator: string
      chapter_index?: number | null
      page_number?: number | null
      scroll_ratio?: number | null
    },
  ) =>
    request<unknown>(`/api/v1/book/books/${id}/progress`, {
      method: 'PUT',
      body: JSON.stringify(payload),
    }),
  listBookBookmarks: (id: string) =>
    request<BookBookmark[]>(`/api/v1/book/books/${id}/bookmarks`),
  createBookBookmark: (
    id: string,
    payload: {
      title: string
      note?: string | null
      progress_percent: number
      locator: string
      chapter_index?: number | null
      page_number?: number | null
      scroll_ratio?: number | null
    },
  ) =>
    request<BookBookmark>(`/api/v1/book/books/${id}/bookmarks`, {
      method: 'POST',
      body: JSON.stringify(payload),
    }),
  deleteBookBookmark: (bookmarkId: string) =>
    request<void>(`/api/v1/book/bookmarks/${bookmarkId}`, { method: 'DELETE' }),
  listBookShelves: () => request<BookShelfDto[]>('/api/v1/book/shelves'),
  uploadBooks: (files: File[]) => {
    const fd = new FormData()
    for (const f of files) fd.append('files', f, f.name)
    return request<BookSummaryDto[]>('/api/v1/book/books/upload', {
      method: 'POST',
      body: fd,
    })
  },
  bookCoverUrl: (id: string) => withAuthQuery(`/api/v1/book/books/${id}/cover`),
  bookFileUrl: (id: string) => withAuthQuery(`/api/v1/book/books/${id}/file`),

  // --- Comics ---
  comicHome: () => request<ComicHomeOverviewDto>('/api/v1/comic/home'),
  listComicSeries: (params: Record<string, string> = {}) => {
    const qs = new URLSearchParams(params).toString()
    return request<SeriesSummaryDto[]>(`/api/v1/comic/series${qs ? `?${qs}` : ''}`)
  },
  getComicSeries: (id: string) => request<SeriesDetailDto>(`/api/v1/comic/series/${id}`),
  deleteComicSeries: (id: string) =>
    request<{ ok: boolean }>(`/api/v1/comic/series/${id}`, { method: 'DELETE' }),
  setComicFavorite: (id: string, favorite: boolean) =>
    request<{ ok: boolean }>(`/api/v1/comic/series/${id}/favorite`, {
      method: 'PUT',
      body: JSON.stringify({ favorite }),
    }),
  getComicReaderContext: (chapterId: string) =>
    request<ComicReaderContextDto>(`/api/v1/comic/chapters/${chapterId}/reader`),
  updateComicProgress: (chapterId: string, current_page: number, is_completed?: boolean) =>
    request<unknown>(`/api/v1/comic/chapters/${chapterId}/progress`, {
      method: 'PUT',
      body: JSON.stringify({ current_page, is_completed }),
    }),
  uploadComicArchive: (file: File) => {
    const fd = new FormData()
    fd.append('file', file, file.name)
    return request<unknown>('/api/v1/comic/upload', { method: 'POST', body: fd })
  },
  comicCoverUrl: (coverUrl: string | null) => (coverUrl ? withAuthQuery(coverUrl) : null),
  comicPageUrl: (chapterId: string, pageNum: number) =>
    withAuthQuery(`/api/v1/comic/chapters/${chapterId}/pages/${pageNum}`),

  // --- Music ---
  musicHome: () => request<MusicHomeOverviewDto>('/api/v1/music/home'),
  listTracks: (params: Record<string, string> = {}) => {
    const qs = new URLSearchParams(params).toString()
    return request<PagedResult<TrackDto>>(`/api/v1/music/tracks${qs ? `?${qs}` : ''}`)
  },
  listAlbums: (params: Record<string, string> = {}) => {
    const qs = new URLSearchParams(params).toString()
    return request<PagedResult<AlbumDto>>(`/api/v1/music/albums${qs ? `?${qs}` : ''}`)
  },
  getAlbum: (id: string) => request<AlbumDetailDto>(`/api/v1/music/albums/${id}`),
  listArtists: (params: Record<string, string> = {}) => {
    const qs = new URLSearchParams(params).toString()
    return request<PagedResult<ArtistDto>>(`/api/v1/music/artists${qs ? `?${qs}` : ''}`)
  },
  getArtist: (id: string) => request<ArtistDetailDto>(`/api/v1/music/artists/${id}`),
  getTrackLyrics: (id: string) =>
    request<StructuredLyricsDto>(`/api/v1/music/tracks/${id}/lyrics`),
  listPlaylists: () => request<PlaylistSummaryDto[]>('/api/v1/music/playlists'),
  getPlaylist: (id: string) => request<PlaylistDetailDto>(`/api/v1/music/playlists/${id}`),
  createPlaylist: (name: string, track_ids: string[] = []) =>
    request<PlaylistDetailDto>('/api/v1/music/playlists', {
      method: 'POST',
      body: JSON.stringify({ name, is_public: false, track_ids }),
    }),
  deletePlaylist: (id: string) =>
    request<{ ok: boolean }>(`/api/v1/music/playlists/${id}`, { method: 'DELETE' }),
  starMusic: (ids: string[]) =>
    request<{ ok: boolean }>('/api/v1/music/starred', {
      method: 'POST',
      body: JSON.stringify({ ids }),
    }),
  unstarMusic: (ids: string[]) =>
    request<{ ok: boolean }>('/api/v1/music/unstarred', {
      method: 'POST',
      body: JSON.stringify({ ids }),
    }),
  getStarredMusic: () =>
    request<{ tracks: TrackDto[]; albums: AlbumDto[]; artists: ArtistDto[] }>(
      '/api/v1/music/starred',
    ),
  scrobbleMusic: (track_id: string, submission = true) =>
    request<{ ok: boolean }>('/api/v1/music/scrobble', {
      method: 'POST',
      body: JSON.stringify({ track_id, submission }),
    }),
  getPlayQueue: () => request<PlayQueueDto | null>('/api/v1/music/play-queue'),
  savePlayQueue: (track_ids: string[], current_track_id: string | null, position_ms: number) =>
    request<{ ok: boolean }>('/api/v1/music/play-queue', {
      method: 'PUT',
      body: JSON.stringify({ track_ids, current_track_id, position_ms }),
    }),
  uploadMusicFiles: (files: File[]) => {
    const fd = new FormData()
    for (const f of files) fd.append('files', f, f.name)
    return request<unknown>('/api/v1/music/upload', { method: 'POST', body: fd })
  },
  musicCoverUrl: (idOrKey: string | null, size = 320) =>
    idOrKey ? `/api/v1/music/covers/${encodeURIComponent(idOrKey)}?size=${size}` : null,
  musicStreamUrl: (
    trackId: string,
    opts?: { format?: string; maxBitRate?: number; timeOffset?: number },
  ) => {
    const q = new URLSearchParams()
    if (opts?.format) q.set('format', opts.format)
    if (opts?.maxBitRate) q.set('max_bit_rate', String(opts.maxBitRate))
    if (opts?.timeOffset && opts.timeOffset > 0) q.set('time_offset', String(opts.timeOffset))
    const qs = q.toString()
    return withAuthQuery(`/api/v1/music/tracks/${trackId}/stream${qs ? `?${qs}` : ''}`)
  },

  // --- Video ---
  videoHome: () => request<VideoHomeOverviewDto>('/api/v1/video/home'),
  listVideos: (params: Record<string, string> = {}) => {
    const qs = new URLSearchParams(params).toString()
    return request<VideoSummaryDto[]>(`/api/v1/video/videos${qs ? `?${qs}` : ''}`)
  },
  getVideo: (id: string) => request<VideoDetailDto>(`/api/v1/video/videos/${id}`),
  deleteVideo: (id: string) =>
    request<{ ok: boolean }>(`/api/v1/video/videos/${id}`, { method: 'DELETE' }),
  saveVideoProgress: (
    id: string,
    payload: {
      position_secs: number
      duration_secs?: number
      completed?: boolean
      selected_audio_stream?: number | null
      selected_subtitle?: string | null
    },
  ) =>
    request<WatchProgress>(`/api/v1/video/videos/${id}/progress`, {
      method: 'PUT',
      body: JSON.stringify(payload),
    }),
  setVideoWatched: (id: string, watched: boolean) =>
    request<WatchProgress>(`/api/v1/video/videos/${id}/watched`, {
      method: 'POST',
      body: JSON.stringify({ watched }),
    }),
  setVideoFavorite: (kind: 'video' | 'show', id: string, favorite: boolean) =>
    request<{ ok: boolean }>(`/api/v1/video/favorites/${kind}/${id}`, {
      method: 'PUT',
      body: JSON.stringify({ favorite }),
    }),
  listShows: (params: Record<string, string> = {}) => {
    const qs = new URLSearchParams(params).toString()
    return request<ShowSummaryDto[]>(`/api/v1/video/shows${qs ? `?${qs}` : ''}`)
  },
  getShow: (id: string) => request<ShowDetailDto>(`/api/v1/video/shows/${id}`),
  uploadVideoFiles: (files: File[]) => {
    const fd = new FormData()
    for (const f of files) fd.append('files', f, f.name)
    return request<unknown>('/api/v1/video/upload', { method: 'POST', body: fd })
  },
  videoPosterUrl: (key: string | null, size = 480) =>
    key ? `/api/v1/video/posters/${encodeURIComponent(key)}?size=${size}` : null,
  videoStreamUrl: (
    id: string,
    opts?: { quality?: VideoQualityProfile; audioStream?: number | null; startSecs?: number },
  ) => {
    const q = new URLSearchParams()
    if (opts?.quality) q.set('quality', opts.quality)
    if (opts?.audioStream !== undefined && opts.audioStream !== null) {
      q.set('audio_stream', String(opts.audioStream))
    }
    if (opts?.startSecs && opts.startSecs > 0) {
      q.set('start_secs', String(Math.floor(opts.startSecs)))
    }
    const qs = q.toString()
    return withAuthQuery(`/api/v1/video/videos/${id}/stream${qs ? `?${qs}` : ''}`)
  },
  videoSubtitleUrl: (videoId: string, subId: string) =>
    withAuthQuery(`/api/v1/video/videos/${videoId}/subtitles/${encodeURIComponent(subId)}.vtt`),
}

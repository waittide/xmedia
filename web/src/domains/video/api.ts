import { api as coreApi, requestJson, withAuthToken } from '@/api/client'
import type { CoreLibraryDto } from '@/types/api'
import type {
  ShowDetailDto,
  ShowMetadataPatch,
  ShowSummaryDto,
  SubtitleTrackDto,
  VideoDetailDto,
  VideoFavoritesDto,
  VideoHomeOverviewDto,
  VideoKindFilter,
  VideoLibraryDto,
  VideoMetadataPatch,
  VideoQualityProfile,
  VideoShelfDetailDto,
  VideoShelfDto,
  VideoSortBy,
  VideoSummaryDto,
  VideoSystemStatusDto,
  WatchProgressDto,
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

export function mapVideoLibrary(raw: CoreLibraryDto): VideoLibraryDto {
  return {
    id: raw.id,
    name: raw.name,
    kind: raw.kind,
    video_count: raw.item_count,
    scan_status: raw.scan_status,
    last_scanned_at: raw.last_scanned_at,
    created_at: raw.created_at,
    updated_at: raw.updated_at,
  }
}

export function buildPosterUrl(posterKey: string | null | undefined, size = 480): string | null {
  if (!posterKey) return null
  return withAuthToken(`/api/v1/video/posters/${encodeURIComponent(posterKey)}?size=${size}`)
}

export function buildVideoStreamUrl(
  videoId: string,
  quality: VideoQualityProfile = 'auto',
  audioStream?: number | null,
  startSecs?: number,
): string {
  const sp = new URLSearchParams()
  sp.set('quality', quality)
  if (audioStream !== undefined && audioStream !== null) {
    sp.set('audio_stream', String(audioStream))
  }
  if (startSecs && startSecs > 0) {
    sp.set('start_secs', String(Math.floor(startSecs)))
  }
  return `/api/v1/video/videos/${encodeURIComponent(videoId)}/stream?${sp.toString()}`
}

export function buildVideoDownloadUrl(videoId: string): string {
  return withAuthToken(`/api/v1/video/videos/${encodeURIComponent(videoId)}/download`)
}

export function buildSubtitleVttUrl(
  videoId: string,
  subId: string,
  offsetSecs = 0,
): string {
  const base = `/api/v1/video/videos/${encodeURIComponent(videoId)}/subtitles/${encodeURIComponent(subId)}`
  const query = offsetSecs > 0 ? `?offset_secs=${Math.floor(offsetSecs)}` : ''
  return withAuthToken(`${base}${query}`)
}

export const api = {
  systemStatus: () => requestJson<VideoSystemStatusDto>('/video/status'),
  listLibraries: async (): Promise<VideoLibraryDto[]> => {
    const libs = await coreApi.listLibraries('video')
    return libs.map(mapVideoLibrary)
  },
  scanLibrary: async (id: string): Promise<VideoLibraryDto> =>
    mapVideoLibrary(await coreApi.scanLibrary(id)),

  homeOverview: () => requestJson<VideoHomeOverviewDto>('/video/home'),
  listVideos: (params: {
    library_id?: string
    show_id?: string
    kind?: VideoKindFilter
    genre?: string
    tag?: string
    year?: number
    search?: string
    sort_by?: VideoSortBy
    sort_desc?: boolean
  } = {}) => requestJson<VideoSummaryDto[]>(`/video/videos${toQuery(params)}`),
  getVideo: (id: string) => requestJson<VideoDetailDto>(`/video/videos/${id}`),
  updateVideo: (id: string, patch: VideoMetadataPatch) =>
    requestJson<VideoSummaryDto>(`/video/videos/${id}`, {
      method: 'PATCH',
      body: JSON.stringify(patch),
    }),
  deleteVideo: (id: string) =>
    requestJson<{ ok: boolean }>(`/video/videos/${id}`, { method: 'DELETE' }),
  uploadVideoPoster: (id: string, file: File) => {
    const fd = new FormData()
    fd.append('poster', file)
    return requestJson<VideoSummaryDto>(`/video/videos/${id}/poster`, {
      method: 'POST',
      body: fd,
    })
  },
  listSubtitles: (videoId: string) =>
    requestJson<SubtitleTrackDto[]>(`/video/videos/${videoId}/subtitles`),
  saveProgress: (
    videoId: string,
    payload: {
      position_secs: number
      duration_secs?: number
      completed?: boolean
      selected_audio_stream?: number | null
      selected_subtitle?: string | null
    },
  ) =>
    requestJson<WatchProgressDto>(`/video/videos/${videoId}/progress`, {
      method: 'PUT',
      body: JSON.stringify(payload),
    }),
  setWatched: (videoId: string, watched: boolean) =>
    requestJson<WatchProgressDto>(`/video/videos/${videoId}/watched`, {
      method: 'POST',
      body: JSON.stringify({ watched }),
    }),

  listShows: (
    params: { library_id?: string; genre?: string; tag?: string; search?: string } = {},
  ) => requestJson<ShowSummaryDto[]>(`/video/shows${toQuery(params)}`),
  getShow: (id: string) => requestJson<ShowDetailDto>(`/video/shows/${id}`),
  updateShow: (id: string, patch: ShowMetadataPatch) =>
    requestJson<ShowSummaryDto>(`/video/shows/${id}`, {
      method: 'PATCH',
      body: JSON.stringify(patch),
    }),
  uploadShowPoster: (id: string, file: File) => {
    const fd = new FormData()
    fd.append('poster', file)
    return requestJson<ShowSummaryDto>(`/video/shows/${id}/poster`, {
      method: 'POST',
      body: fd,
    })
  },

  listFavorites: () => requestJson<VideoFavoritesDto>('/video/favorites'),
  setFavorite: (kind: 'video' | 'show', id: string, favorite: boolean) =>
    requestJson<{ ok: boolean }>(`/video/favorites/${kind}/${id}`, {
      method: 'PUT',
      body: JSON.stringify({ favorite }),
    }),

  listShelves: () => requestJson<VideoShelfDto[]>('/video/shelves'),
  getShelf: (id: string) => requestJson<VideoShelfDetailDto>(`/video/shelves/${id}`),
  createShelf: (payload: { name: string; description?: string; video_ids?: string[] }) =>
    requestJson<VideoShelfDetailDto>('/video/shelves', {
      method: 'POST',
      body: JSON.stringify({
        name: payload.name,
        description: payload.description ?? '',
        video_ids: payload.video_ids ?? [],
      }),
    }),
  updateShelf: (
    id: string,
    payload: { name: string; description?: string; video_ids: string[] },
  ) =>
    requestJson<VideoShelfDetailDto>(`/video/shelves/${id}`, {
      method: 'PUT',
      body: JSON.stringify({
        name: payload.name,
        description: payload.description ?? '',
        video_ids: payload.video_ids,
      }),
    }),
  deleteShelf: (id: string) =>
    requestJson<{ ok: boolean }>(`/video/shelves/${id}`, { method: 'DELETE' }),
}

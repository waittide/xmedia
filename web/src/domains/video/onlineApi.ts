import { requestJson, withAuthToken } from '@/api/client'
import { createDomainSourceApi } from '@/components/online-sources/createDomainSourceApi'
import {
  isSafeDataImageUrl,
  isSafeHttpUrl,
  type BaseSourceTestReportDto,
  type ExecutionDiagnostics,
  type OnlineSourceManagerAdapter,
  type OnlineSourceSummaryDto,
} from '@/components/online-sources/types'
import type { ShowDetailDto, VideoSummaryDto } from './types'

export type VideoSourceSummaryDto = OnlineSourceSummaryDto
export type VideoSourceTestMode = 'full' | 'categories' | 'search' | 'browse' | 'detail' | 'play'

export interface VideoSourceTestInput {
  keyword?: string | null
  mode?: VideoSourceTestMode
  category_id?: string | null
  video_id?: string | null
  detail_url?: string | null
  play_url?: string | null
  group_name?: string | null
  target_url?: string | null
}

export interface OnlineVideoCategory {
  id: string
  name: string
  parent_id: string | null
}

export interface OnlineVideoSummary {
  vod_id: string
  title: string
  cover_url: string | null
  proxy_cover_url: string | null
  remarks: string | null
  year: string | null
  area: string | null
  type_name: string | null
  detail_url: string
}

export interface OnlineVideoSearchItemDto extends OnlineVideoSummary {
  source_id: string
  source_name: string
}

export interface OnlineVideoEpisode {
  index: number
  name: string
  play_url: string
}

export interface OnlineVideoPlayGroup {
  name: string
  episodes: OnlineVideoEpisode[]
}

export interface OnlineVideoDetail {
  id: string
  title: string
  cover_url: string | null
  proxy_cover_url: string | null
  intro: string | null
  year: string | null
  area: string | null
  actors: string | null
  director: string | null
  remarks: string | null
  play_groups: OnlineVideoPlayGroup[]
}

export interface OnlineVideoSubtitle {
  label: string
  lang: string | null
  url: string
  proxy_url?: string | null
}

export interface OnlineVideoPlayResult {
  stream_url: string
  proxy_stream_url: string
  stream_type: 'hls' | 'mp4' | 'auto' | string
  headers_applied: boolean
  subtitles: OnlineVideoSubtitle[]
}

export interface VideoSourceTestReportDto extends BaseSourceTestReportDto<VideoSourceTestMode> {
  categories: OnlineVideoCategory[]
  search_items: OnlineVideoSummary[]
  video_detail: OnlineVideoDetail | null
  play_result: OnlineVideoPlayResult | null
}

export interface OnlineVideoCategoriesResponseDto {
  source_id: string
  source_name: string
  categories: OnlineVideoCategory[]
  has_explore?: boolean
  diagnostics: ExecutionDiagnostics
}

export interface OnlineVideoSearchErrorDto {
  source_id: string
  source_name: string
  error: string
}

export interface OnlineVideoSearchResponseDto {
  keyword: string
  page: number
  results: OnlineVideoSearchItemDto[]
  errors: OnlineVideoSearchErrorDto[]
  toasts: string[]
}

export interface OnlineVideoBrowseResponseDto {
  source_id: string
  source_name: string
  category_id: string | null
  results: OnlineVideoSummary[]
  total: number | null
  page: number
  page_count: number | null
  diagnostics: ExecutionDiagnostics
}

export interface OnlineVideoDetailResponseDto extends OnlineVideoDetail {
  source_id: string
  source_name: string
  diagnostics: ExecutionDiagnostics
}

export interface OnlineVideoPlayResponseDto extends OnlineVideoPlayResult {
  source_id: string
  source_name: string
  diagnostics: ExecutionDiagnostics
}

export interface TrackOnlineVideoInput {
  source_id: string
  video_id?: string | null
  detail_url?: string | null
  group_name?: string | null
}

export interface TrackOnlineVideoResultDto {
  kind: 'movie' | 'show' | string
  video?: VideoSummaryDto | null
  show?: ShowDetailDto | null
  added_episodes: number
}

export interface TrackedVideoPlayResponseDto {
  video_id: string
  cached: boolean
  source_id?: string | null
  stream_url?: string | null
  proxy_stream_url?: string | null
  stream_type?: 'hls' | 'mp4' | 'auto' | string | null
  subtitles: OnlineVideoSubtitle[]
}

export interface VideoCacheReportDto {
  cached_count: number
  skipped_count: number
  failed_count: number
  errors: string[]
}

export const ONLINE_VIDEO_CTX_KEY = 'xmedia_online_video_ctx'
export const ONLINE_VIDEO_PLAYER_CTX_KEY = 'xmedia_online_video_player_ctx'

export interface OnlineVideoContextStorage {
  sourceId: string
  sourceName?: string
  videoId?: string
  detailUrl?: string
  title?: string
  coverUrl?: string | null
  proxyCoverUrl?: string | null
  remarks?: string | null
  year?: string | null
  area?: string | null
  typeName?: string | null
  returnKeyword?: string
  returnSourceId?: string | null
}

export interface OnlineVideoPlayerContextStorage {
  sourceId: string
  sourceName?: string
  videoId?: string
  detailUrl?: string
  title?: string
  coverUrl?: string | null
  proxyCoverUrl?: string | null
  groupName?: string
  episodeIndex?: number
  episodeName?: string
  playUrl: string
  playGroups?: OnlineVideoPlayGroup[]
}

export function buildOnlineVideoPosterUrl(
  rawUrl: string | null | undefined,
  sourceId?: string | null,
): string {
  if (!rawUrl || !rawUrl.trim()) return ''
  const u = rawUrl.trim()
  if (u.toLowerCase().startsWith('data:')) {
    return isSafeDataImageUrl(u) ? u : ''
  }
  if (!isSafeHttpUrl(u)) return ''
  const qs = new URLSearchParams({ url: u })
  if (sourceId) qs.set('source_id', sourceId)
  return withAuthToken(`/api/v1/video/online/poster-proxy?${qs.toString()}`)
}

export function resolveOnlineVideoPosterUrl(
  proxyCoverUrl?: string | null,
  rawCoverUrl?: string | null,
  sourceId?: string | null,
): string {
  const proxy = proxyCoverUrl?.trim()
  if (proxy) {
    if (proxy.toLowerCase().startsWith('data:')) {
      return isSafeDataImageUrl(proxy) ? proxy : ''
    }
    if (proxy.startsWith('/') && !proxy.startsWith('//')) {
      try {
        const parsed = new URL(proxy, 'http://localhost')
        if (parsed.origin !== 'http://localhost') return ''
        if (parsed.pathname !== '/api/v1/video/online/poster-proxy') return ''
        const innerUrl = parsed.searchParams.get('url')?.trim()
        if (!innerUrl || !isSafeHttpUrl(innerUrl)) return ''
        return withAuthToken(proxy)
      } catch {
        return ''
      }
    }
    if (isSafeHttpUrl(proxy)) {
      return withAuthToken(proxy)
    }
    return ''
  }
  return buildOnlineVideoPosterUrl(rawCoverUrl, sourceId)
}

export function buildOnlineVideoStreamUrl(
  rawUrl: string | null | undefined,
  streamType: 'hls' | 'mp4' | 'auto' | string = 'auto',
  sourceId?: string | null,
): string {
  if (!rawUrl || !rawUrl.trim() || !isSafeHttpUrl(rawUrl)) return ''
  const trimmed = rawUrl.trim()
  const qs = new URLSearchParams({ url: trimmed })
  if (sourceId) qs.set('source_id', sourceId)
  const isHls =
    streamType === 'hls' || (streamType === 'auto' && /\.m3u8(\?|$)/i.test(trimmed))
  const endpoint = isHls ? 'hls-proxy' : 'stream-proxy'
  return withAuthToken(`/api/v1/video/online/${endpoint}?${qs.toString()}`)
}

export function resolveOnlineVideoStreamUrl(
  proxyStreamUrl?: string | null,
  rawStreamUrl?: string | null,
  streamType: 'hls' | 'mp4' | 'auto' | string = 'auto',
  sourceId?: string | null,
): string {
  const proxy = proxyStreamUrl?.trim()
  if (proxy) {
    if (proxy.startsWith('/') && !proxy.startsWith('//')) {
      try {
        const parsed = new URL(proxy, 'http://localhost')
        if (parsed.origin !== 'http://localhost') return ''
        if (
          parsed.pathname !== '/api/v1/video/online/hls-proxy' &&
          parsed.pathname !== '/api/v1/video/online/stream-proxy'
        ) {
          return ''
        }
        const innerUrl = parsed.searchParams.get('url')?.trim()
        if (!innerUrl || !isSafeHttpUrl(innerUrl)) return ''
        return withAuthToken(proxy)
      } catch {
        return ''
      }
    }
    if (isSafeHttpUrl(proxy)) {
      return withAuthToken(proxy)
    }
    return ''
  }
  return buildOnlineVideoStreamUrl(rawStreamUrl, streamType, sourceId)
}

export function buildOnlineVideoSubtitleUrl(
  rawUrl: string | null | undefined,
  sourceId?: string | null,
): string {
  if (!rawUrl || !rawUrl.trim() || !isSafeHttpUrl(rawUrl)) return ''
  const qs = new URLSearchParams({ url: rawUrl.trim() })
  if (sourceId) qs.set('source_id', sourceId)
  return withAuthToken(`/api/v1/video/online/subtitle-proxy?${qs.toString()}`)
}

export function resolveOnlineVideoSubtitleUrl(
  proxyUrl?: string | null,
  rawUrl?: string | null,
  sourceId?: string | null,
): string {
  const proxy = proxyUrl?.trim()
  if (proxy) {
    if (proxy.startsWith('/') && !proxy.startsWith('//')) {
      try {
        const parsed = new URL(proxy, 'http://localhost')
        if (parsed.origin !== 'http://localhost') return ''
        if (parsed.pathname !== '/api/v1/video/online/subtitle-proxy') return ''
        const innerUrl = parsed.searchParams.get('url')?.trim()
        if (!innerUrl || !isSafeHttpUrl(innerUrl)) return ''
        return withAuthToken(proxy)
      } catch {
        return ''
      }
    }
    return ''
  }
  return buildOnlineVideoSubtitleUrl(rawUrl, sourceId)
}

const baseCrud = createDomainSourceApi<VideoSourceSummaryDto>('video')

export const onlineVideoApi = {
  ...baseCrud,

  testSource: (
    id: string,
    payload?: string | VideoSourceTestInput,
    options?: { signal?: AbortSignal },
  ): Promise<VideoSourceTestReportDto> => {
    const body: VideoSourceTestInput =
      typeof payload === 'string'
        ? { keyword: payload.trim() || null }
        : {
            keyword: payload?.keyword?.trim() || null,
            mode: payload?.mode ?? 'full',
            category_id: payload?.category_id?.trim() || null,
            video_id: payload?.video_id?.trim() || null,
            detail_url: payload?.detail_url?.trim() || null,
            play_url: payload?.play_url?.trim() || null,
            group_name: payload?.group_name?.trim() || null,
            target_url: payload?.target_url?.trim() || null,
          }
    return requestJson(`/video/sources/${encodeURIComponent(id)}/test`, {
      method: 'POST',
      body: JSON.stringify(body),
      signal: options?.signal,
    })
  },

  categories: (
    sourceId: string,
    options?: { signal?: AbortSignal },
  ): Promise<OnlineVideoCategoriesResponseDto> =>
    requestJson('/video/online/categories', {
      method: 'POST',
      body: JSON.stringify({ source_id: sourceId }),
      signal: options?.signal,
    }),

  search: (
    keyword: string,
    sourceIds?: string[],
    page = 1,
    options?: { signal?: AbortSignal },
  ): Promise<OnlineVideoSearchResponseDto> =>
    requestJson('/video/online/search', {
      method: 'POST',
      body: JSON.stringify({
        keyword,
        source_ids: sourceIds && sourceIds.length > 0 ? sourceIds : null,
        page,
      }),
      signal: options?.signal,
    }),

  browse: (
    sourceId: string,
    categoryId?: string | null,
    page = 1,
    options?: { signal?: AbortSignal },
  ): Promise<OnlineVideoBrowseResponseDto> =>
    requestJson('/video/online/browse', {
      method: 'POST',
      body: JSON.stringify({
        source_id: sourceId,
        category_id: categoryId ?? null,
        page,
      }),
      signal: options?.signal,
    }),

  detail: (
    sourceId: string,
    params: { video_id?: string | null; detail_url?: string | null },
    options?: { signal?: AbortSignal },
  ): Promise<OnlineVideoDetailResponseDto> =>
    requestJson('/video/online/detail', {
      method: 'POST',
      body: JSON.stringify({
        source_id: sourceId,
        video_id: params.video_id ?? null,
        detail_url: params.detail_url ?? null,
      }),
      signal: options?.signal,
    }),

  play: (
    sourceId: string,
    playUrl: string,
    groupName?: string | null,
    options?: { signal?: AbortSignal },
  ): Promise<OnlineVideoPlayResponseDto> =>
    requestJson('/video/online/play', {
      method: 'POST',
      body: JSON.stringify({
        source_id: sourceId,
        play_url: playUrl,
        group_name: groupName ?? null,
      }),
      signal: options?.signal,
    }),

  trackOnlineVideo: (payload: TrackOnlineVideoInput): Promise<TrackOnlineVideoResultDto> =>
    requestJson('/video/online/track', {
      method: 'POST',
      body: JSON.stringify({
        source_id: payload.source_id,
        video_id: payload.video_id ?? null,
        detail_url: payload.detail_url ?? null,
        group_name: payload.group_name ?? null,
      }),
    }),

  refreshTrackedShow: (showId: string): Promise<ShowDetailDto> =>
    requestJson(`/video/shows/${encodeURIComponent(showId)}/online/refresh`, {
      method: 'POST',
    }),

  cacheTrackedShow: (showId: string, videoIds?: string[]): Promise<VideoCacheReportDto> =>
    requestJson(`/video/shows/${encodeURIComponent(showId)}/online/cache`, {
      method: 'POST',
      body: JSON.stringify(videoIds ? { video_ids: videoIds } : {}),
    }),

  cacheTrackedVideo: (videoId: string): Promise<VideoSummaryDto> =>
    requestJson(`/video/videos/${encodeURIComponent(videoId)}/online/cache`, {
      method: 'POST',
    }),

  resolveTrackedPlay: (
    videoId: string,
    options?: { signal?: AbortSignal },
  ): Promise<TrackedVideoPlayResponseDto> =>
    requestJson(`/video/videos/${encodeURIComponent(videoId)}/online/play`, {
      signal: options?.signal,
    }),
}

export const videoSourceAdapter: OnlineSourceManagerAdapter<VideoSourceSummaryDto> = {
  domain: 'video',
  exportFilePrefix: 'xmedia-video-sources',
  searchRoutePath: '/video/library',
  listSources: onlineVideoApi.listSources,
  importSources: onlineVideoApi.importSources,
  reorderSources: onlineVideoApi.reorderSources,
  batchSources: onlineVideoApi.batchSources,
  exportSourcesBatch: onlineVideoApi.exportSourcesBatch,
  updateSource: onlineVideoApi.updateSource,
  deleteSource: onlineVideoApi.deleteSource,
  exportSourceUrl: onlineVideoApi.exportSourceUrl,
  fetchSourceJson: onlineVideoApi.fetchSourceJson,
  getSourceVariable: onlineVideoApi.getSourceVariable,
  runLoginAction: onlineVideoApi.runLoginAction,
}

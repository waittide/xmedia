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

export function buildOnlineVideoStreamUrl(
  rawUrl: string | null | undefined,
  streamType: 'hls' | 'mp4' | 'auto' | string = 'auto',
  sourceId?: string | null,
): string {
  if (!rawUrl || !rawUrl.trim() || !isSafeHttpUrl(rawUrl)) return ''
  const qs = new URLSearchParams({ url: rawUrl.trim() })
  if (sourceId) qs.set('source_id', sourceId)
  const endpoint = streamType === 'hls' ? 'hls-proxy' : 'stream-proxy'
  return withAuthToken(`/api/v1/video/online/${endpoint}?${qs.toString()}`)
}

const baseCrud = createDomainSourceApi<VideoSourceSummaryDto>('video', { hasSeeds: true })

export const onlineVideoApi = {
  ...baseCrud,
  importSeeds: baseCrud.importSeeds!,

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

  categories: (sourceId: string): Promise<OnlineVideoCategoriesResponseDto> =>
    requestJson('/video/online/categories', {
      method: 'POST',
      body: JSON.stringify({ source_id: sourceId }),
    }),

  search: (
    keyword: string,
    sourceIds?: string[],
    page = 1,
  ): Promise<OnlineVideoSearchResponseDto> =>
    requestJson('/video/online/search', {
      method: 'POST',
      body: JSON.stringify({
        keyword,
        source_ids: sourceIds && sourceIds.length > 0 ? sourceIds : null,
        page,
      }),
    }),

  browse: (
    sourceId: string,
    categoryId?: string | null,
    page = 1,
  ): Promise<OnlineVideoBrowseResponseDto> =>
    requestJson('/video/online/browse', {
      method: 'POST',
      body: JSON.stringify({
        source_id: sourceId,
        category_id: categoryId ?? null,
        page,
      }),
    }),

  detail: (
    sourceId: string,
    params: { video_id?: string | null; detail_url?: string | null },
  ): Promise<OnlineVideoDetailResponseDto> =>
    requestJson('/video/online/detail', {
      method: 'POST',
      body: JSON.stringify({
        source_id: sourceId,
        video_id: params.video_id ?? null,
        detail_url: params.detail_url ?? null,
      }),
    }),

  play: (
    sourceId: string,
    playUrl: string,
    groupName?: string | null,
  ): Promise<OnlineVideoPlayResponseDto> =>
    requestJson('/video/online/play', {
      method: 'POST',
      body: JSON.stringify({
        source_id: sourceId,
        play_url: playUrl,
        group_name: groupName ?? null,
      }),
    }),
}

export const videoSourceAdapter: OnlineSourceManagerAdapter<VideoSourceSummaryDto> = {
  domain: 'video',
  exportFilePrefix: 'xmedia-video-sources',
  searchRoutePath: '/video/catalog',
  listSources: onlineVideoApi.listSources,
  importSources: onlineVideoApi.importSources,
  importSeeds: onlineVideoApi.importSeeds,
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

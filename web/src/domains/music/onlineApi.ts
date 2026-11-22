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

export type MusicSourceSummaryDto = OnlineSourceSummaryDto
export type MusicSourceTestMode = 'full' | 'search' | 'explore' | 'detail' | 'stream' | 'lyric'

export interface MusicSourceTestInput {
  keyword?: string | null
  mode?: MusicSourceTestMode
  target_url?: string | null
  explore_url?: string | null
  track_id?: string | null
  detail_url?: string | null
  play_url?: string | null
  url_id?: string | null
  cover_id?: string | null
  lyric_id?: string | null
  quality?: string | null
  title?: string | null
  artist?: string | null
}

export interface OnlineMusicTrackDto {
  source_id: string
  source_name: string
  track_id: string
  title: string
  artist: string
  album: string | null
  duration_secs: number | null
  cover_url: string | null
  proxy_cover_url: string | null
  detail_url: string | null
  play_url: string | null
  url_id: string | null
  lyric_id: string | null
  cover_id: string | null
  qualities: string[]
  format: string | null
}

export interface MusicExploreCategoryDto {
  title: string
  url: string
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

export interface OnlineMusicResolvedTrackDto {
  source_id: string
  source_name: string
  track: OnlineMusicTrackDto
  stream_url: string | null
  proxy_stream_url: string | null
  cover_url: string | null
  proxy_cover_url: string | null
  lyrics: StructuredLyricsDto | null
  translated_lyrics: StructuredLyricsDto | null
  format: string | null
  bitrate: number | null
  expires_in_secs: number | null
  diagnostics: ExecutionDiagnostics
}

export interface OnlineMusicStreamResponseDto {
  source_id: string
  stream_url: string
  proxy_stream_url: string
  format: string | null
  bitrate: number | null
  expires_in_secs: number | null
  headers_applied: boolean
}

export interface MusicSourceTestReportDto extends BaseSourceTestReportDto<MusicSourceTestMode> {
  search_items: OnlineMusicTrackDto[]
  resolved_track: OnlineMusicResolvedTrackDto | null
  stream_probe: OnlineMusicStreamResponseDto | null
  lyrics: StructuredLyricsDto | null
  explore_categories: MusicExploreCategoryDto[]
}

export interface OnlineMusicSearchErrorDto {
  source_id: string
  source_name: string
  message: string
}

export interface OnlineMusicSearchResponseDto {
  keyword: string
  page: number
  results: OnlineMusicTrackDto[]
  errors: OnlineMusicSearchErrorDto[]
  toasts: string[]
}

export interface OnlineMusicExploreResponseDto {
  source_id: string
  results: OnlineMusicTrackDto[]
  categories: MusicExploreCategoryDto[]
  diagnostics: ExecutionDiagnostics
}

export function buildOnlineMusicCoverUrl(
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
  return withAuthToken(`/api/v1/music/online/cover-proxy?${qs.toString()}`)
}

export function buildOnlineMusicAudioUrl(
  rawUrl: string | null | undefined,
  sourceId?: string | null,
): string {
  if (!rawUrl || !rawUrl.trim() || !isSafeHttpUrl(rawUrl)) return ''
  const qs = new URLSearchParams({ url: rawUrl.trim() })
  if (sourceId) qs.set('source_id', sourceId)
  return withAuthToken(`/api/v1/music/online/audio-proxy?${qs.toString()}`)
}

export function resolveOnlineMusicCoverUrl(
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
        if (parsed.pathname !== '/api/v1/music/online/cover-proxy') return ''
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
  return buildOnlineMusicCoverUrl(rawCoverUrl, sourceId)
}

export function resolveOnlineMusicAudioUrl(
  proxyStreamUrl?: string | null,
  rawStreamUrl?: string | null,
  sourceId?: string | null,
): string {
  const proxy = proxyStreamUrl?.trim()
  if (proxy) {
    if (proxy.startsWith('/') && !proxy.startsWith('//')) {
      try {
        const parsed = new URL(proxy, 'http://localhost')
        if (parsed.origin !== 'http://localhost') return ''
        if (parsed.pathname !== '/api/v1/music/online/audio-proxy') return ''
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
  return buildOnlineMusicAudioUrl(rawStreamUrl, sourceId)
}

const baseCrud = createDomainSourceApi<MusicSourceSummaryDto>('music', { hasSeeds: true })

export const onlineMusicApi = {
  ...baseCrud,
  importSeeds: baseCrud.importSeeds!,

  testSource: (
    id: string,
    payload?: string | MusicSourceTestInput,
    options?: { signal?: AbortSignal },
  ): Promise<MusicSourceTestReportDto> => {
    const body: MusicSourceTestInput =
      typeof payload === 'string'
        ? { keyword: payload.trim() || null }
        : {
            keyword: payload?.keyword?.trim() || null,
            mode: payload?.mode ?? 'full',
            target_url: payload?.target_url?.trim() || null,
            explore_url: payload?.explore_url?.trim() || null,
            track_id: payload?.track_id?.trim() || null,
            detail_url: payload?.detail_url?.trim() || null,
            play_url: payload?.play_url?.trim() || null,
            url_id: payload?.url_id?.trim() || null,
            cover_id: payload?.cover_id?.trim() || null,
            lyric_id: payload?.lyric_id?.trim() || null,
            quality: payload?.quality?.trim() || null,
            title: payload?.title?.trim() || null,
            artist: payload?.artist?.trim() || null,
          }
    return requestJson(`/music/sources/${encodeURIComponent(id)}/test`, {
      method: 'POST',
      body: JSON.stringify(body),
      signal: options?.signal,
    })
  },

  search: (
    keyword: string,
    sourceIds?: string[],
    page = 1,
    limit = 20,
  ): Promise<OnlineMusicSearchResponseDto> =>
    requestJson('/music/online/search', {
      method: 'POST',
      body: JSON.stringify({
        keyword,
        source_ids: sourceIds && sourceIds.length > 0 ? sourceIds : null,
        page,
        limit,
      }),
    }),

  explore: (
    sourceId: string,
    exploreUrl?: string | null,
    page = 1,
    limit = 20,
  ): Promise<OnlineMusicExploreResponseDto> =>
    requestJson('/music/online/explore', {
      method: 'POST',
      body: JSON.stringify({
        source_id: sourceId,
        explore_url: exploreUrl ?? null,
        page,
        limit,
      }),
    }),

  fetchTrackInfo: (payload: {
    source_id: string
    track_id?: string | null
    detail_url?: string | null
    quality?: string | null
    title?: string | null
    artist?: string | null
    album?: string | null
    duration_secs?: number | null
    cover_url?: string | null
    cover_id?: string | null
    url_id?: string | null
    lyric_id?: string | null
    play_url?: string | null
  }): Promise<OnlineMusicResolvedTrackDto> =>
    requestJson('/music/online/track-info', {
      method: 'POST',
      body: JSON.stringify(payload),
    }),

  fetchStream: (payload: {
    source_id: string
    track_id?: string | null
    url_id?: string | null
    play_url?: string | null
    quality?: string | null
    detail_url?: string | null
    title?: string | null
    artist?: string | null
  }): Promise<OnlineMusicStreamResponseDto> =>
    requestJson('/music/online/stream', {
      method: 'POST',
      body: JSON.stringify(payload),
    }),

  fetchLyrics: (payload: {
    source_id: string
    track_id?: string | null
    lyric_id?: string | null
    title?: string | null
    artist?: string | null
    detail_url?: string | null
  }): Promise<StructuredLyricsDto> =>
    requestJson('/music/online/lyrics', {
      method: 'POST',
      body: JSON.stringify(payload),
    }),
}

export const musicSourceAdapter: OnlineSourceManagerAdapter<MusicSourceSummaryDto> = {
  domain: 'music',
  exportFilePrefix: 'xmedia-music-sources',
  searchRoutePath: '/music/search',
  listSources: onlineMusicApi.listSources,
  importSources: onlineMusicApi.importSources,
  importSeeds: onlineMusicApi.importSeeds,
  reorderSources: onlineMusicApi.reorderSources,
  batchSources: onlineMusicApi.batchSources,
  exportSourcesBatch: onlineMusicApi.exportSourcesBatch,
  updateSource: onlineMusicApi.updateSource,
  deleteSource: onlineMusicApi.deleteSource,
  exportSourceUrl: onlineMusicApi.exportSourceUrl,
  fetchSourceJson: onlineMusicApi.fetchSourceJson,
  getSourceVariable: onlineMusicApi.getSourceVariable,
  runLoginAction: onlineMusicApi.runLoginAction,
}

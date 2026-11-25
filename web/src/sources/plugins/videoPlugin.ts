import type { OnlineSourceManagerAdapter, OnlineSourceSummaryDto } from '@/components/online-sources/types'
import { requestJson } from '@/api/client'
import { fetchHomeOverview } from '@/config/homeCache'
import { api as videoApi, buildPosterUrl as buildVideoPosterUrl } from '@/domains/video/api'
import { ensureVideoLocale } from '@/domains/video/i18n'
import { videoSourceAdapter } from '@/domains/video/onlineApi'
import { useVideoStore } from '@/domains/video/store'
import type { ShowSummaryDto, VideoSummaryDto } from '@/domains/video/types'
import type { DomainPlugin, MergedShelfView } from '../domainRegistry'
import { itemRoute, playRoute } from '../routes'
import type { MediaItemVM } from '../types'
import { parseTimestampMs } from '../useMergedShelf'

function mapVideoToVM(video: VideoSummaryDto, view: MergedShelfView): MediaItemVM {
  const ref = { scope: 'local' as const, domain: 'video' as const, id: video.id }
  const progressPercent =
    video.progress && video.progress.duration_secs > 0
      ? Math.min(
          100,
          Math.max(0, Math.round((video.progress.position_secs / video.progress.duration_secs) * 100)),
        )
      : undefined
  const to = video.show_id
    ? itemRoute({ scope: 'local', domain: 'video', id: video.show_id })
    : playRoute(ref)
  const ts =
    view === 'continue'
      ? parseTimestampMs(video.progress?.updated_at ?? video.created_at)
      : parseTimestampMs(video.created_at)

  return {
    ref,
    title: video.show_title ? `${video.show_title} · ${video.title}` : video.title,
    subtitle: video.year ? String(video.year) : undefined,
    cover: buildVideoPosterUrl(video.poster_key) ?? undefined,
    badge: {
      sourceKind: video.online_tracking ? 'online' : 'local',
      progressPercent,
    },
    to,
    playTo: playRoute(ref, {
      position: video.progress?.position_secs ?? 0,
      duration: video.progress?.duration_secs ?? video.duration_secs,
    }),
    sortTimestamp: ts,
  }
}

function mapShowToVM(show: ShowSummaryDto): MediaItemVM {
  const ref = { scope: 'local' as const, domain: 'video' as const, id: show.id }
  const progressPercent =
    show.episode_count > 0
      ? Math.min(
          100,
          Math.max(0, Math.round((show.watched_episode_count / show.episode_count) * 100)),
        )
      : undefined
  return {
    ref,
    title: show.title,
    subtitle: show.year ? String(show.year) : undefined,
    cover: buildVideoPosterUrl(show.poster_key) ?? undefined,
    badge: {
      sourceKind: show.online_tracking ? 'online' : 'local',
      progressPercent,
    },
    to: itemRoute(ref),
    sortTimestamp: parseTimestampMs(show.created_at),
  }
}

export const videoDomainPlugin: DomainPlugin = {
  kind: 'video',
  onlineSearchI18n: {
    titleKey: 'globalSearch.onlineVideoSearchTitle',
    subtitleKey: 'globalSearch.onlineVideoSearchSubtitle',
  },
  ensureLocale: () => ensureVideoLocale(),
  resetSessionState: () => {
    useVideoStore().resetState()
  },
  fetchHomeOverview: () => videoApi.homeOverview(),
  fetchLocalShelfItems: async (view) => {
    if (view === 'continue') {
      const home = await fetchHomeOverview('video')
      return home.continue_watching.map((v) => mapVideoToVM(v, view))
    }
    const favs = await videoApi.listFavorites()
    return [
      ...favs.shows.filter((s) => s.is_favorite).map(mapShowToVM),
      ...favs.videos.filter((v) => v.is_favorite).map((v) => mapVideoToVM(v, view)),
    ]
  },
  searchLocal: async ({ query, limit, signal, t }) => {
    const sp = new URLSearchParams({
      search: query,
      limit: String(limit),
    })
    const res = await requestJson<VideoSummaryDto[]>(`/video/videos?${sp.toString()}`, { signal })
    const items: MediaItemVM[] = res.slice(0, limit).map((v) => {
      const ref = { scope: 'local' as const, domain: 'video' as const, id: v.id }
      const to = v.show_id
        ? itemRoute({ scope: 'local', domain: 'video', id: v.show_id })
        : playRoute(ref)
      return {
        ref,
        title: v.title,
        subtitle: v.year ? String(v.year) : t('app.domains.video'),
        cover: buildVideoPosterUrl(v.poster_key) ?? undefined,
        badge: { sourceKind: 'local' },
        to,
        playTo: playRoute(ref),
      }
    })
    return { items, total: res.length }
  },
  upload: {
    extensions: [
      'mp4',
      'mkv',
      'webm',
      'mov',
      'avi',
      'ts',
      'm2ts',
      'mts',
      'm4v',
      'flv',
      'srt',
      'vtt',
      'webvtt',
      'ass',
      'ssa',
    ],
    sidecarExtensions: ['srt', 'vtt', 'webvtt', 'ass', 'ssa'],
    batchable: true,
    allowStemPrefixSidecars: true,
    executeUpload: async (files, { sendXhr }) => {
      const fd = new FormData()
      for (const f of files) fd.append('files', f, f.name)
      const res = await sendXhr<{
        uploaded_videos?: { id: string }[]
        attached_subtitles_count?: number
      }>('/api/v1/video/upload', fd)
      const videoCount = res.uploaded_videos?.length ?? 0
      const subCount = res.attached_subtitles_count ?? 0
      if (videoCount === 0 && subCount === 0) {
        throw new Error('No video files or matching subtitles were imported')
      }
      return {
        targetRoute: res.uploaded_videos?.[0]?.id
          ? `/video/play/${res.uploaded_videos[0].id}`
          : undefined,
      }
    },
  },
  pool: {
    pageSize: 24,
    paginated: false,
    skeletonMode: 'grid',
    fetchPool: async ({ query, libraryId }, t) => {
      const res = await videoApi.listPool({
        search: query,
        library_id: libraryId,
      })
      return {
        kind: 'video-sections',
        total: res.shows.length + res.movies.length,
        shows: res.shows.map((show) => ({
          id: show.id,
          title: show.title,
          subtitle: [
            show.year ? String(show.year) : '',
            t('pool.episodesCount', { count: show.episode_count }),
          ]
            .filter(Boolean)
            .join(' · '),
          coverUrl: buildVideoPosterUrl(show.poster_key),
          bound: show.bound,
          openKind: 'show' as const,
        })),
        movies: res.movies.map((movie) => ({
          id: movie.id,
          title: movie.title,
          subtitle:
            [movie.year ? String(movie.year) : '', movie.resolution || '']
              .filter(Boolean)
              .join(' · ') || '—',
          coverUrl: buildVideoPosterUrl(movie.poster_key),
          bound: movie.bound,
          openKind: 'movie' as const,
        })),
      }
    },
    bindItems: (payload) => videoApi.bindItems(payload),
    unbindItem: (id, userId) => videoApi.unbindItem(id, userId),
  },
  onlineSources: {
    remoteProtocol: null,
    getAdapter: () => videoSourceAdapter as OnlineSourceManagerAdapter<OnlineSourceSummaryDto>,
    loadEditSheet: () => import('@/domains/video/components/VideoSourceEditSheet.vue'),
    loadDebugSheet: () => import('@/domains/video/components/VideoSourceDebugSheet.vue'),
  },
}

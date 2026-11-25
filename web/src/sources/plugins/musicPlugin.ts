import type { OnlineSourceManagerAdapter, OnlineSourceSummaryDto } from '@/components/online-sources/types'
import { requestJson } from '@/api/client'
import { fetchHomeOverview } from '@/config/homeCache'
import { api as musicApi, buildCoverUrl as buildMusicCoverUrl } from '@/domains/music/api'
import { ensureMusicLocale } from '@/domains/music/i18n'
import { musicSourceAdapter } from '@/domains/music/onlineApi'
import { usePlayerStore } from '@/domains/music/player'
import { useMusicStore } from '@/domains/music/store'
import type {
  AlbumDto,
  BookmarkDto,
  SearchResultDto as MusicSearchResultDto,
  TrackDto,
} from '@/domains/music/types'
import type { DomainPlugin } from '../domainRegistry'
import { itemRoute, libraryRoute } from '../routes'
import type { MediaItemVM } from '../types'
import { parseTimestampMs } from '../useMergedShelf'

function mapMusicTrackToVM(track: TrackDto, bookmark?: BookmarkDto): MediaItemVM {
  const ref = { scope: 'local' as const, domain: 'music' as const, id: track.id }
  const cover = buildMusicCoverUrl(track.cover_key) ?? undefined
  const progressPercent =
    bookmark && track.duration_secs > 0
      ? Math.min(
          100,
          Math.max(0, Math.round((bookmark.position_ms / 1000 / track.duration_secs) * 100)),
        )
      : undefined
  const to = track.album_id
    ? itemRoute({ scope: 'local', domain: 'music', id: track.album_id })
    : libraryRoute('music', { view: 'tracks', q: track.title })
  const ts = bookmark
    ? parseTimestampMs(bookmark.updated_at)
    : parseTimestampMs(track.last_played_at ?? track.starred_at ?? track.created_at)

  return {
    ref,
    title: track.title,
    subtitle: [track.artist_name, track.album_title].filter(Boolean).join(' · ') || undefined,
    cover,
    badge: {
      sourceKind: track.online_tracking ? 'online' : 'local',
      progressPercent,
    },
    to,
    sortTimestamp: ts,
  }
}

function mapMusicAlbumToVM(album: AlbumDto): MediaItemVM {
  const ref = { scope: 'local' as const, domain: 'music' as const, id: album.id }
  return {
    ref,
    title: album.title,
    subtitle: album.album_artist_name || undefined,
    cover: buildMusicCoverUrl(album.cover_key) ?? undefined,
    badge: {
      sourceKind: 'local',
    },
    to: itemRoute(ref),
    sortTimestamp: parseTimestampMs(album.starred_at ?? album.created_at),
  }
}

export const musicDomainPlugin: DomainPlugin = {
  kind: 'music',
  onlineSearchI18n: {
    titleKey: 'globalSearch.onlineMusicSearchTitle',
    subtitleKey: 'globalSearch.onlineMusicSearchSubtitle',
  },
  ensureLocale: () => ensureMusicLocale(),
  resetSessionState: () => {
    usePlayerStore().resetForLogout()
    useMusicStore().resetState()
  },
  fetchHomeOverview: () => musicApi.homeOverview(),
  fetchLocalShelfItems: async (view) => {
    if (view === 'continue') {
      const home = await fetchHomeOverview('music')
      const seen = new Set<string>()
      const out: MediaItemVM[] = []
      for (const bm of home.bookmarks) {
        seen.add(bm.track.id)
        out.push(mapMusicTrackToVM(bm.track, bm))
      }
      for (const tr of home.recent_played_tracks) {
        if (!seen.has(tr.id)) {
          seen.add(tr.id)
          out.push(mapMusicTrackToVM(tr))
        }
      }
      return out
    }
    const starred = await musicApi.getStarred()
    return [
      ...starred.albums.filter((a) => Boolean(a.starred_at)).map(mapMusicAlbumToVM),
      ...starred.tracks.filter((tr) => Boolean(tr.starred_at)).map((tr) => mapMusicTrackToVM(tr)),
    ]
  },
  searchLocal: async ({ query, limit, signal, t }) => {
    const sp = new URLSearchParams({
      q: query,
      limit: String(limit),
    })
    const res = await requestJson<MusicSearchResultDto>(`/music/search?${sp.toString()}`, {
      signal,
    })
    const items: MediaItemVM[] = []
    const albumCap = Math.min(2, limit)
    for (const alb of res.albums.slice(0, albumCap)) {
      const ref = { scope: 'local' as const, domain: 'music' as const, id: alb.id }
      items.push({
        ref,
        title: alb.title,
        subtitle: alb.album_artist_name || t('app.domains.music'),
        cover: buildMusicCoverUrl(alb.cover_key) ?? undefined,
        badge: { sourceKind: 'local' },
        to: itemRoute(ref),
      })
    }
    const trackCap = Math.max(1, limit - items.length)
    for (const tr of res.tracks.slice(0, trackCap)) {
      const ref = { scope: 'local' as const, domain: 'music' as const, id: tr.id }
      items.push({
        ref,
        title: tr.title,
        subtitle: [tr.artist_name, tr.album_title].filter(Boolean).join(' · ') || undefined,
        cover: buildMusicCoverUrl(tr.cover_key) ?? undefined,
        badge: { sourceKind: 'local' },
        to: tr.album_id
          ? itemRoute({ scope: 'local', domain: 'music', id: tr.album_id })
          : libraryRoute('music', { view: 'tracks', q: query }),
      })
    }
    return { items, total: res.albums.length + res.tracks.length }
  },
  upload: {
    extensions: [
      'flac',
      'mp3',
      'm4a',
      'aac',
      'alac',
      'ogg',
      'opus',
      'wav',
      'aiff',
      'aif',
      'ape',
      'wv',
      'lrc',
    ],
    sidecarExtensions: ['lrc'],
    batchable: true,
    executeUpload: async (files, { sendXhr }) => {
      const fd = new FormData()
      for (const f of files) fd.append('file', f, f.name)
      const res = await sendXhr<{
        uploaded_tracks?: { id: string }[]
        attached_lyrics_count?: number
      }>('/api/v1/music/tracks/upload', fd)
      const trackCount = res.uploaded_tracks?.length ?? 0
      const lyricCount = res.attached_lyrics_count ?? 0
      if (trackCount === 0 && lyricCount === 0) {
        throw new Error('No music tracks or matching lyrics were imported')
      }
      return { targetRoute: '/music/library' }
    },
  },
  pool: {
    pageSize: 50,
    paginated: true,
    skeletonMode: 'list',
    fetchPool: async ({ query, libraryId, page, pageSize }) => {
      const res = await musicApi.listPool({
        search: query,
        library_id: libraryId,
        page,
        page_size: pageSize,
        sort_by: 'created_at',
        sort_order: 'desc',
      })
      return {
        kind: 'tracks',
        total: res.total,
        items: res.items.map((track) => ({
          id: track.id,
          title: track.title,
          subtitle: [track.artist_name, track.album_title].filter(Boolean).join(' · '),
          coverUrl: buildMusicCoverUrl(track.cover_key, 120),
          formatLabel: track.format.toUpperCase(),
          durationSecs: track.duration_secs,
          albumId: track.album_id ?? null,
          bound: track.bound,
        })),
      }
    },
    bindItems: (payload) => musicApi.bindItems(payload),
    unbindItem: (id, userId) => musicApi.unbindItem(id, userId),
    bindAlbum: (albumId, userId) => musicApi.bindItems({ album_id: albumId, user_id: userId }),
  },
  onlineSources: {
    remoteProtocol: 'subsonic',
    getAdapter: () => musicSourceAdapter as OnlineSourceManagerAdapter<OnlineSourceSummaryDto>,
    loadEditSheet: () => import('@/domains/music/components/MusicSourceEditSheet.vue'),
    loadDebugSheet: () => import('@/domains/music/components/MusicSourceDebugSheet.vue'),
  },
}

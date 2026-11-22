import type { SubsonicSongDto } from '@/api/appHost'
import type {
  OnlineItemMetadataInput,
  OnlineItemSnapshot,
  TrackedItemRef,
} from '@/sources'
import { resolveOnlineMusicCoverUrl, type OnlineMusicTrackDto } from './onlineApi'

export function getOnlineTrackItemKey(track: OnlineMusicTrackDto): string {
  return (
    track.track_id?.trim() ||
    track.detail_url?.trim() ||
    track.play_url?.trim() ||
    track.url_id?.trim() ||
    track.title
  )
}

export function onlineTrackToTrackedRef(track: OnlineMusicTrackDto): TrackedItemRef {
  const cover = resolveOnlineMusicCoverUrl(track.proxy_cover_url, track.cover_url, track.source_id)
  return {
    scope: 'online',
    domain: 'music',
    sourceId: track.source_id,
    ref: getOnlineTrackItemKey(track),
    hints: {
      title: track.title,
      author: track.artist || undefined,
      cover: cover || undefined,
    },
  }
}

export function onlineTrackToItemMeta(track: OnlineMusicTrackDto): OnlineItemMetadataInput {
  return {
    title: track.title,
    subtitle: track.artist || null,
    coverUrl: track.cover_url || null,
    kind: 'track',
    extra: {
      track_id: track.track_id,
      detail_url: track.detail_url,
      play_url: track.play_url,
      url_id: track.url_id,
      lyric_id: track.lyric_id,
      cover_id: track.cover_id,
      album: track.album,
      duration_secs: track.duration_secs,
      qualities: track.qualities,
      format: track.format,
    },
  }
}

export function buildRemoteSubsonicStreamUrl(sourceId: string, songId: string): string {
  return `/_app/subsonic/${encodeURIComponent(sourceId)}/stream/${encodeURIComponent(songId)}`
}

export function remoteSongToTrackedRef(sourceId: string, song: SubsonicSongDto): TrackedItemRef {
  return {
    scope: 'remote',
    domain: 'music',
    sourceId,
    ref: song.id,
    hints: {
      title: song.title,
      author: song.artist || undefined,
      cover: song.cover_url || undefined,
    },
  }
}

export function remoteSongToItemMeta(
  sourceId: string,
  song: SubsonicSongDto,
): OnlineItemMetadataInput {
  return {
    title: song.title,
    subtitle: song.artist || null,
    coverUrl: song.cover_url || null,
    kind: 'track',
    extra: {
      stream_url: song.stream_url || buildRemoteSubsonicStreamUrl(sourceId, song.id),
      album: song.album || null,
      duration_secs: song.duration ?? null,
      suffix: song.suffix ?? null,
      bit_rate: song.bit_rate ?? null,
    },
  }
}

function readExtraString(extra: Record<string, unknown> | undefined, key: string): string | null {
  const val = extra?.[key]
  return typeof val === 'string' && val.trim() ? val.trim() : null
}

function readExtraNumber(extra: Record<string, unknown> | undefined, key: string): number | null {
  const val = extra?.[key]
  return typeof val === 'number' && Number.isFinite(val) ? val : null
}

function readExtraStringArray(extra: Record<string, unknown> | undefined, key: string): string[] {
  const val = extra?.[key]
  if (!Array.isArray(val)) return []
  return val.filter((v): v is string => typeof v === 'string' && v.trim().length > 0)
}

export function snapshotToOnlineMusicTrack(snap: OnlineItemSnapshot): OnlineMusicTrackDto {
  const extra = snap.extra
  return {
    source_id: snap.sourceId,
    source_name: snap.sourceName || snap.sourceId,
    track_id: readExtraString(extra, 'track_id') ?? snap.itemKey,
    title: snap.title,
    artist: snap.subtitle ?? '',
    album: readExtraString(extra, 'album'),
    duration_secs: readExtraNumber(extra, 'duration_secs') ?? snap.progress?.duration ?? null,
    cover_url: snap.coverUrl ?? null,
    proxy_cover_url: null,
    detail_url: readExtraString(extra, 'detail_url'),
    play_url: readExtraString(extra, 'play_url'),
    url_id: readExtraString(extra, 'url_id'),
    lyric_id: readExtraString(extra, 'lyric_id'),
    cover_id: readExtraString(extra, 'cover_id'),
    qualities: readExtraStringArray(extra, 'qualities'),
    format: readExtraString(extra, 'format'),
  }
}

export function snapshotToRemoteSubsonicSong(snap: OnlineItemSnapshot): SubsonicSongDto {
  const extra = snap.extra
  return {
    id: snap.itemKey,
    title: snap.title,
    artist: snap.subtitle ?? '',
    album: readExtraString(extra, 'album') ?? '',
    duration: readExtraNumber(extra, 'duration_secs') ?? snap.progress?.duration ?? null,
    suffix: readExtraString(extra, 'suffix'),
    bit_rate: readExtraNumber(extra, 'bit_rate'),
    cover_url: snap.coverUrl ?? null,
    stream_url:
      readExtraString(extra, 'stream_url') ??
      buildRemoteSubsonicStreamUrl(snap.sourceId, snap.itemKey),
  }
}

import type { SubsonicSongDto } from '@/api/appHost'
import {
  resolveOnlineMusicAudioUrl,
  resolveOnlineMusicCoverUrl,
  type OnlineMusicResolvedTrackDto,
  type OnlineMusicTrackDto,
} from '../onlineApi'
import type { TrackDto, TranscodeProfile } from '../types'

export interface LocalQueueItem {
  kind: 'local'
  key: string
  track: TrackDto
}

export interface OnlineQueueItem {
  kind: 'online'
  key: string
  source_id: string
  source_name: string
  track: OnlineMusicTrackDto
  resolved: OnlineMusicResolvedTrackDto | null
  resolvedAtMs: number | null
  selectedQuality: string | null
  activeFormat: string | null
  activeBitrate: number | null
}

export interface RemoteQueueItem {
  kind: 'remote'
  key: string
  source_id: string
  source_name: string
  song: SubsonicSongDto
}

export type PlayerQueueItem = LocalQueueItem | OnlineQueueItem | RemoteQueueItem

export type PlayableQueueInput =
  | { kind: 'local'; tracks: readonly TrackDto[] }
  | { kind: 'online'; tracks: readonly OnlineMusicTrackDto[] }
  | {
      kind: 'remote'
      sourceId: string
      sourceName: string
      songs: readonly SubsonicSongDto[]
    }

export function toPlayerQueueItems(input: PlayableQueueInput): PlayerQueueItem[] {
  if (input.kind === 'local') {
    return input.tracks.map(makeLocalQueueItem)
  }
  if (input.kind === 'online') {
    return input.tracks.map(makeOnlineQueueItem)
  }
  return input.songs.map((song) => makeRemoteQueueItem(input.sourceId, input.sourceName, song))
}

export interface PlayerTrackDisplay {
  kind: 'local' | 'online' | 'remote'
  id: string
  title: string
  artist_name: string
  album_title: string
  duration_secs: number
  cover_id: string | null
  cover_key: string | null
  cover_url: string | null
  artist_id: string | null
  album_id: string | null
  starred_at: string | null
  source_id: string | null
  source_name: string | null
  qualities: string[]
}

export function buildOnlineTrackKey(track: OnlineMusicTrackDto): string {
  const fallback = track.detail_url || track.play_url || track.url_id || track.title
  return `online:${track.source_id}:${track.track_id || ''}:${fallback}`
}

export function buildRemoteSongKey(sourceId: string, songId: string): string {
  return `remote:${sourceId}:${songId}`
}

export function resolveOnlineItemCoverUrl(item: OnlineQueueItem): string | null {
  const resolved = resolveOnlineMusicCoverUrl(
    item.resolved?.proxy_cover_url || item.track.proxy_cover_url,
    item.resolved?.cover_url || item.track.cover_url,
    item.source_id,
    item.resolved?.track.cover_id || item.track.cover_id,
    item.resolved?.track.track_id || item.track.track_id,
  )
  return resolved || null
}

export function resolveOnlineItemStreamUrl(item: OnlineQueueItem): string {
  return resolveOnlineMusicAudioUrl(
    item.resolved?.proxy_stream_url,
    item.resolved?.stream_url,
    item.source_id,
  )
}

export function makeLocalQueueItem(track: TrackDto): LocalQueueItem {
  return {
    kind: 'local',
    key: track.id,
    track,
  }
}

export function pickDefaultOnlineQuality(qualities: string[]): string | null {
  if (!qualities || qualities.length === 0) return null
  if (qualities.includes('320')) return '320'
  return qualities[0] ?? null
}

export function makeOnlineQueueItem(track: OnlineMusicTrackDto): OnlineQueueItem {
  return {
    kind: 'online',
    key: buildOnlineTrackKey(track),
    source_id: track.source_id,
    source_name: track.source_name,
    track,
    resolved: null,
    resolvedAtMs: null,
    selectedQuality: pickDefaultOnlineQuality(track.qualities),
    activeFormat: track.format ?? null,
    activeBitrate: null,
  }
}

export function makeRemoteQueueItem(
  sourceId: string,
  sourceName: string,
  song: SubsonicSongDto,
): RemoteQueueItem {
  return {
    kind: 'remote',
    key: buildRemoteSongKey(sourceId, song.id),
    source_id: sourceId,
    source_name: sourceName,
    song,
  }
}

export function isOnlineItemExpired(item: OnlineQueueItem): boolean {
  return (
    item.resolved?.expires_in_secs != null &&
    item.resolvedAtMs != null &&
    Date.now() - item.resolvedAtMs >= item.resolved.expires_in_secs * 1000
  )
}

export function isTranscodeProfile(val: unknown): val is TranscodeProfile {
  return (
    val === 'raw' ||
    val === 'opus128' ||
    val === 'mp3192' ||
    val === 'mp3320' ||
    val === 'aac256'
  )
}

export function buildPlayerTrackDisplay(item: PlayerQueueItem | null): PlayerTrackDisplay | null {
  if (!item) return null
  if (item.kind === 'local') {
    const tr = item.track
    return {
      kind: 'local',
      id: tr.id,
      title: tr.title,
      artist_name: tr.artist_name,
      album_title: tr.album_title,
      duration_secs: tr.duration_secs,
      cover_id: tr.id,
      cover_key: tr.cover_key,
      cover_url: null,
      artist_id: tr.artist_id,
      album_id: tr.album_id,
      starred_at: tr.starred_at,
      source_id: null,
      source_name: null,
      qualities: [],
    }
  }
  if (item.kind === 'remote') {
    const song = item.song
    return {
      kind: 'remote',
      id: item.key,
      title: song.title,
      artist_name: song.artist,
      album_title: song.album || '',
      duration_secs: song.duration ?? 0,
      cover_id: null,
      cover_key: null,
      cover_url: song.cover_url ?? null,
      artist_id: null,
      album_id: null,
      starred_at: null,
      source_id: item.source_id,
      source_name: item.source_name,
      qualities: [],
    }
  }
  const base = item.track
  const resolvedTrack = item.resolved?.track
  return {
    kind: 'online',
    id: item.key,
    title: resolvedTrack?.title || base.title,
    artist_name: resolvedTrack?.artist || base.artist,
    album_title: resolvedTrack?.album ?? base.album ?? '',
    duration_secs: resolvedTrack?.duration_secs ?? base.duration_secs ?? 0,
    cover_id: null,
    cover_key: null,
    cover_url: resolveOnlineItemCoverUrl(item),
    artist_id: null,
    album_id: null,
    starred_at: null,
    source_id: item.source_id,
    source_name: item.source_name,
    qualities: base.qualities,
  }
}

export function formatPlayerQualitySummary(
  item: PlayerQueueItem | null,
  activeEffectiveProfile: TranscodeProfile,
): string {
  if (!item) return ''
  if (item.kind === 'remote') {
    const fmt = (item.song.suffix || '').toUpperCase()
    if (fmt && item.song.bit_rate) return `${fmt} ${item.song.bit_rate}k`
    return fmt
  }
  if (item.kind === 'online') {
    const fmt = (item.activeFormat || item.track.format || '').toUpperCase()
    const bitrate = item.activeBitrate
    if (fmt && bitrate) return `${fmt} ${bitrate}k`
    if (fmt) return fmt
    if (item.selectedQuality) {
      const q = item.selectedQuality.toUpperCase()
      return /^\d+$/.test(q) ? `${q}k` : q
    }
    return ''
  }
  const tr = item.track
  if (activeEffectiveProfile === 'raw') {
    const fmt = tr.format.toUpperCase()
    const parts: string[] = [fmt]
    if (tr.bit_depth && tr.sample_rate_hz) {
      const khz = Number((tr.sample_rate_hz / 1000).toFixed(1))
      parts.push(`${tr.bit_depth}bit/${khz}kHz`)
    } else if (tr.bit_rate_kbps) {
      parts.push(`${tr.bit_rate_kbps}k`)
    }
    return parts.join(' ')
  }
  if (activeEffectiveProfile === 'opus128') return 'OPUS 128k'
  if (activeEffectiveProfile === 'mp3192') return 'MP3 192k'
  if (activeEffectiveProfile === 'mp3320') return 'MP3 320k'
  return 'AAC 256k'
}

export function shuffleCopy<T>(items: readonly T[]): T[] {
  const copy = [...items]
  for (let i = copy.length - 1; i > 0; i--) {
    const j = Math.floor(Math.random() * (i + 1))
    const tmp = copy[i]!
    copy[i] = copy[j]!
    copy[j] = tmp
  }
  return copy
}

export function createShuffleTracker() {
  let playHistory: number[] = []
  let shufflePool: number[] = []

  function pushHistory(index: number): void {
    if (index < 0) return
    playHistory.push(index)
    if (playHistory.length > 200) playHistory.shift()
  }

  function refillPool(len: number, excludeIndex: number): void {
    const indices: number[] = []
    for (let i = 0; i < len; i++) {
      if (i !== excludeIndex) indices.push(i)
    }
    for (let i = indices.length - 1; i > 0; i--) {
      const j = Math.floor(Math.random() * (i + 1))
      const tmp = indices[i]!
      indices[i] = indices[j]!
      indices[j] = tmp
    }
    shufflePool = indices
  }

  return {
    reset(): void {
      playHistory = []
      shufflePool = []
    },
    recordJump(fromIndex: number, toIndex: number): void {
      if (fromIndex >= 0 && fromIndex !== toIndex) {
        pushHistory(fromIndex)
      }
      shufflePool = shufflePool.filter((i) => i !== toIndex)
    },
    recordInsert(insertAt: number, currentIndex: number, addToShufflePool: boolean): void {
      if (!addToShufflePool && currentIndex >= 0) {
        pushHistory(currentIndex)
      }
      playHistory = playHistory.map((i) => (i >= insertAt ? i + 1 : i))
      shufflePool = shufflePool.map((i) => (i >= insertAt ? i + 1 : i))
      if (addToShufflePool) {
        shufflePool.push(insertAt)
      }
    },
    pickNextIndex(len: number, currentIndex: number, isShuffle: boolean): number {
      pushHistory(currentIndex)
      if (isShuffle && len > 1) {
        shufflePool = shufflePool.filter((i) => i >= 0 && i < len && i !== currentIndex)
        if (shufflePool.length === 0) {
          refillPool(len, currentIndex)
        }
        return shufflePool.pop() ?? (currentIndex + 1) % len
      }
      return (currentIndex + 1) % len
    },
    pickPrevIndex(len: number, currentIndex: number, isShuffle: boolean): number {
      if (isShuffle) {
        while (playHistory.length > 0) {
          const prev = playHistory.pop()!
          if (prev >= 0 && prev < len && prev !== currentIndex) {
            return prev
          }
        }
      }
      return (currentIndex - 1 + len) % len
    },
    remapMove(fromIndex: number, toIndex: number, currentIndex: number): number {
      const remapIndex = (idx: number): number => {
        if (idx === fromIndex) return toIndex
        if (fromIndex < toIndex && idx > fromIndex && idx <= toIndex) return idx - 1
        if (fromIndex > toIndex && idx >= toIndex && idx < fromIndex) return idx + 1
        return idx
      }
      playHistory = playHistory.map(remapIndex)
      shufflePool = shufflePool.map(remapIndex)
      return currentIndex >= 0 ? remapIndex(currentIndex) : currentIndex
    },
    remapRemove(removedIdx: number): void {
      playHistory = playHistory
        .filter((i) => i !== removedIdx)
        .map((i) => (i > removedIdx ? i - 1 : i))
      shufflePool = shufflePool
        .filter((i) => i !== removedIdx)
        .map((i) => (i > removedIdx ? i - 1 : i))
    },
  }
}

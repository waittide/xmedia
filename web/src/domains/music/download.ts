import { buildDownloadUrl } from './api'
import {
  buildOnlineMusicDownloadUrl,
  onlineMusicApi,
  type OnlineMusicResolvedTrackDto,
  type OnlineMusicTrackDto,
} from './onlineApi'
import { buildRemoteSubsonicStreamUrl } from './onlineTrackRef'
import type { PlayerQueueItem } from './player'

export function formatTrackFilename(artist?: string | null, title?: string | null): string {
  const parts = [artist?.trim(), title?.trim()].filter(Boolean)
  return parts.join(' - ') || 'track'
}

export function triggerBrowserDownload(url: string, suggestedFilename?: string): void {
  if (!url) return
  const link = document.createElement('a')
  link.href = url
  link.rel = 'noopener'
  if (suggestedFilename) {
    link.download = suggestedFilename
  }
  document.body.appendChild(link)
  link.click()
  document.body.removeChild(link)
}

export function downloadLocalTrack(trackId: string): void {
  triggerBrowserDownload(buildDownloadUrl(trackId))
}

export interface DownloadOnlineTrackOptions {
  resolved?: OnlineMusicResolvedTrackDto | null
  resolvedAtMs?: number | null
  quality?: string | null
}

function isResolvedStreamFresh(
  resolved: OnlineMusicResolvedTrackDto | null | undefined,
  resolvedAtMs: number | null | undefined,
): boolean {
  if (!resolved?.stream_url) return false
  if (resolved.expires_in_secs == null || resolvedAtMs == null) return true
  return Date.now() - resolvedAtMs < resolved.expires_in_secs * 1000
}

export async function downloadOnlineTrack(
  track: OnlineMusicTrackDto,
  options: DownloadOnlineTrackOptions = {},
): Promise<void> {
  let streamUrl = ''
  let format: string | null | undefined = track.format

  if (isResolvedStreamFresh(options.resolved, options.resolvedAtMs)) {
    streamUrl = options.resolved?.stream_url ?? ''
    format = options.resolved?.format ?? track.format
  } else {
    const quality = options.quality ?? (track.qualities?.includes('320') ? '320' : track.qualities?.[0] ?? null)
    const res = await onlineMusicApi.fetchStream({
      source_id: track.source_id,
      track_id: track.track_id || null,
      url_id: track.url_id || null,
      play_url: track.play_url || null,
      detail_url: track.detail_url || null,
      title: track.title || null,
      artist: track.artist || null,
      quality,
    })
    streamUrl = res.stream_url
    format = res.format ?? track.format
  }

  const downloadUrl = buildOnlineMusicDownloadUrl(
    streamUrl,
    track.source_id,
    formatTrackFilename(track.artist, track.title),
    format,
  )
  if (!downloadUrl) {
    throw new Error('Unable to resolve downloadable audio stream')
  }
  triggerBrowserDownload(downloadUrl)
}

export async function downloadQueueItem(item: PlayerQueueItem): Promise<void> {
  if (item.kind === 'local') {
    downloadLocalTrack(item.track.id)
    return
  }
  if (item.kind === 'remote') {
    const url = item.song.stream_url || buildRemoteSubsonicStreamUrl(item.source_id, item.song.id)
    const ext = item.song.suffix?.trim() || 'mp3'
    triggerBrowserDownload(url, `${formatTrackFilename(item.song.artist, item.song.title)}.${ext}`)
    return
  }
  await downloadOnlineTrack(item.resolved?.track ?? item.track, {
    resolved: item.resolved,
    resolvedAtMs: item.resolvedAtMs,
    quality: item.selectedQuality,
  })
}

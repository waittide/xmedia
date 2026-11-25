import type {
  MediaFileDetail,
  MediaMetadataPatchEnvelope,
  ScrapedMetadata,
} from '../../types'
import {
  parseClearableInt,
  parseNullableFloat,
  parseNullableInt,
  parseOptInt,
  shouldWrite,
  splitListTokens,
  type ApplyFillMode,
  type ScrapedFieldKey,
} from './formAdaptersBookComic'
import type { MusicInspectorForm, VideoInspectorForm } from './types'

type MusicMetadataData = Extract<MediaFileDetail['metadata'], { kind: 'music' }>['data']
type VideoMetadataData = Extract<MediaFileDetail['metadata'], { kind: 'video' }>['data']

export function resetMusicForm(musicForm: MusicInspectorForm): void {
  musicForm.title = ''
  musicForm.artist = ''
  musicForm.albumArtist = ''
  musicForm.album = ''
  musicForm.track = ''
  musicForm.disc = ''
  musicForm.year = ''
  musicForm.genre = ''
  musicForm.comment = ''
  musicForm.embeddedLyrics = ''
  musicForm.sidecarLrc = ''
}

export function hydrateMusicForm(musicForm: MusicInspectorForm, mu: MusicMetadataData): void {
  musicForm.title = mu.title
  musicForm.artist = mu.artist
  musicForm.albumArtist = mu.album_artist ?? ''
  musicForm.album = mu.album
  musicForm.track = mu.track_number !== null ? String(mu.track_number) : ''
  musicForm.disc = mu.disc_number !== null ? String(mu.disc_number) : ''
  musicForm.year = mu.year !== null ? String(mu.year) : ''
  musicForm.genre = mu.genre ?? ''
  musicForm.comment = mu.comment ?? ''
  musicForm.embeddedLyrics = mu.embedded_lyrics ?? ''
  musicForm.sidecarLrc = mu.sidecar_lrc ?? ''
}

export function buildMusicPatchEnvelope(musicForm: MusicInspectorForm): MediaMetadataPatchEnvelope {
  return {
    kind: 'music',
    patch: {
      title: musicForm.title,
      artist: musicForm.artist,
      album_artist: musicForm.albumArtist,
      album: musicForm.album,
      track_number: parseOptInt(musicForm.track, 0),
      disc_number: parseOptInt(musicForm.disc, 0),
      year: parseOptInt(musicForm.year, 0),
      genre: musicForm.genre,
      comment: musicForm.comment,
      embedded_lyrics: musicForm.embeddedLyrics,
      sidecar_lrc: musicForm.sidecarLrc,
    },
  }
}

export function applyMusicScrapedField(
  musicForm: MusicInspectorForm,
  field: ScrapedFieldKey,
  meta: ScrapedMetadata,
  fillMode: ApplyFillMode,
): boolean {
  if (field === 'title' && meta.title && shouldWrite(musicForm.title, fillMode)) {
    musicForm.title = meta.title
    return true
  }
  if (field === 'creators' && meta.creators.length > 0 && shouldWrite(musicForm.artist, fillMode)) {
    musicForm.artist = meta.creators.join(' / ')
    if (shouldWrite(musicForm.albumArtist, fillMode)) {
      musicForm.albumArtist = meta.creators[0] || ''
    }
    return true
  }
  if (
    field === 'series_or_album' &&
    meta.series_or_album &&
    shouldWrite(musicForm.album, fillMode)
  ) {
    musicForm.album = meta.series_or_album
    return true
  }
  if (field === 'year' && meta.year != null && shouldWrite(musicForm.year, fillMode)) {
    musicForm.year = String(meta.year)
    return true
  }
  if (
    field === 'track_or_episode' &&
    meta.track_or_episode_number != null &&
    shouldWrite(musicForm.track, fillMode)
  ) {
    musicForm.track = String(meta.track_or_episode_number)
    return true
  }
  if (
    field === 'disc_or_season' &&
    meta.disc_or_season_number != null &&
    shouldWrite(musicForm.disc, fillMode)
  ) {
    musicForm.disc = String(meta.disc_or_season_number)
    return true
  }
  if (field === 'genres' && meta.genres.length > 0 && shouldWrite(musicForm.genre, fillMode)) {
    musicForm.genre = meta.genres[0] || ''
    return true
  }
  if (field === 'summary' && meta.summary && shouldWrite(musicForm.comment, fillMode)) {
    musicForm.comment = meta.summary
    return true
  }
  if (field === 'lyrics') {
    const lyr = meta.synced_lyrics || meta.plain_lyrics
    if (lyr && shouldWrite(musicForm.embeddedLyrics, fillMode)) {
      musicForm.embeddedLyrics = lyr
      if (meta.synced_lyrics && shouldWrite(musicForm.sidecarLrc, fillMode)) {
        musicForm.sidecarLrc = meta.synced_lyrics
      }
      return true
    }
  }
  return false
}

export function musicFormToScrapedMetadata(
  musicForm: MusicInspectorForm,
  base: ScrapedMetadata,
  coverUrl: string | null,
): ScrapedMetadata {
  const lyr = (musicForm.sidecarLrc || musicForm.embeddedLyrics).trim()
  const hasTimestamps = /\[\d{1,2}:\d{2}/.test(lyr)
  return {
    ...base,
    title: musicForm.title.trim() || null,
    creators: splitListTokens(musicForm.artist),
    series_or_album: musicForm.album.trim() || null,
    disc_or_season_number: parseNullableInt(musicForm.disc),
    track_or_episode_number: parseNullableInt(musicForm.track),
    year: parseNullableInt(musicForm.year),
    genres: splitListTokens(musicForm.genre),
    summary: musicForm.comment.trim() || null,
    synced_lyrics: lyr && hasTimestamps ? lyr : null,
    plain_lyrics: lyr && !hasTimestamps ? lyr : null,
    cover_url: coverUrl,
  }
}

export function resetVideoForm(videoForm: VideoInspectorForm): void {
  videoForm.isEpisode = false
  videoForm.title = ''
  videoForm.originalTitle = ''
  videoForm.showTitle = ''
  videoForm.season = ''
  videoForm.episode = ''
  videoForm.year = ''
  videoForm.rating = ''
  videoForm.studio = ''
  videoForm.directors = ''
  videoForm.actors = ''
  videoForm.genres = ''
  videoForm.tags = ''
  videoForm.tmdbId = ''
  videoForm.bangumiId = ''
  videoForm.summary = ''
  videoForm.lockedFields = []
}

export function hydrateVideoForm(
  videoForm: VideoInspectorForm,
  v: VideoMetadataData,
): 'poster' | 'fanart' | 'thumb' {
  videoForm.isEpisode = v.is_episode
  videoForm.title = v.title
  videoForm.originalTitle = v.original_title ?? ''
  videoForm.showTitle = v.show_title ?? ''
  videoForm.season = v.season_number !== null ? String(v.season_number) : ''
  videoForm.episode = v.episode_number !== null ? String(v.episode_number) : ''
  videoForm.year = v.year !== null ? String(v.year) : ''
  videoForm.rating = v.rating !== null ? String(v.rating) : ''
  videoForm.studio = v.studio ?? ''
  videoForm.directors = v.directors.join(' / ')
  videoForm.actors = v.actors.join(' / ')
  videoForm.genres = v.genres.join(' / ')
  videoForm.tags = v.tags.join(' / ')
  videoForm.tmdbId = v.tmdb_id ?? ''
  videoForm.bangumiId = v.bangumi_id ?? ''
  videoForm.summary = v.summary ?? ''
  videoForm.lockedFields = [...(v.locked_fields ?? [])]
  if (v.has_poster) return 'poster'
  if (v.has_fanart) return 'fanart'
  if (v.has_thumb) return 'thumb'
  return 'poster'
}

export function buildVideoPatchEnvelope(videoForm: VideoInspectorForm): MediaMetadataPatchEnvelope {
  const parsedRating = videoForm.rating.trim() ? Number.parseFloat(videoForm.rating) : 0
  return {
    kind: 'video',
    patch: {
      is_episode: videoForm.isEpisode,
      title: videoForm.title,
      original_title: videoForm.originalTitle,
      show_title: videoForm.showTitle,
      season_number: parseClearableInt(videoForm.season),
      episode_number: parseOptInt(videoForm.episode, 0),
      year: parseOptInt(videoForm.year, 0),
      rating: Number.isFinite(parsedRating) && parsedRating > 0 ? parsedRating : 0,
      studio: videoForm.studio,
      directors: splitListTokens(videoForm.directors),
      actors: splitListTokens(videoForm.actors),
      genres: splitListTokens(videoForm.genres),
      tags: splitListTokens(videoForm.tags),
      tmdb_id: videoForm.tmdbId,
      bangumi_id: videoForm.bangumiId,
      summary: videoForm.summary,
      locked_fields: videoForm.lockedFields,
    },
  }
}

export function applyVideoScrapedField(
  videoForm: VideoInspectorForm,
  isLocked: (field: string) => boolean,
  field: ScrapedFieldKey,
  meta: ScrapedMetadata,
  fillMode: ApplyFillMode,
): boolean {
  if (
    field === 'title' &&
    meta.title &&
    !isLocked('title') &&
    shouldWrite(videoForm.title, fillMode)
  ) {
    videoForm.title = meta.title
    return true
  }
  if (
    field === 'original_title' &&
    meta.original_title &&
    !isLocked('original_title') &&
    shouldWrite(videoForm.originalTitle, fillMode)
  ) {
    videoForm.originalTitle = meta.original_title
    return true
  }
  if (field === 'series_or_album') {
    const show = meta.series_or_album || (videoForm.isEpisode ? meta.title : null)
    if (show && !isLocked('show_title') && shouldWrite(videoForm.showTitle, fillMode)) {
      videoForm.showTitle = show
      return true
    }
  }
  if (
    field === 'publisher_or_studio' &&
    meta.publisher_or_studio &&
    !isLocked('studio') &&
    shouldWrite(videoForm.studio, fillMode)
  ) {
    videoForm.studio = meta.publisher_or_studio
    return true
  }
  if (
    field === 'year' &&
    meta.year != null &&
    !isLocked('year') &&
    shouldWrite(videoForm.year, fillMode)
  ) {
    videoForm.year = String(meta.year)
    return true
  }
  if (
    field === 'rating' &&
    meta.rating != null &&
    !isLocked('rating') &&
    shouldWrite(videoForm.rating, fillMode)
  ) {
    videoForm.rating = String(meta.rating)
    return true
  }
  if (field === 'directors') {
    const dirs =
      meta.illustrators_or_directors.length > 0 ? meta.illustrators_or_directors : meta.creators
    if (dirs.length > 0 && !isLocked('directors') && shouldWrite(videoForm.directors, fillMode)) {
      videoForm.directors = dirs.join(' / ')
      return true
    }
  }
  if (
    field === 'cast' &&
    meta.cast.length > 0 &&
    !isLocked('actors') &&
    shouldWrite(videoForm.actors, fillMode)
  ) {
    videoForm.actors = meta.cast.join(' / ')
    return true
  }
  if (
    field === 'genres' &&
    meta.genres.length > 0 &&
    !isLocked('genres') &&
    shouldWrite(videoForm.genres, fillMode)
  ) {
    videoForm.genres = meta.genres.join(' / ')
    return true
  }
  if (
    field === 'tags' &&
    meta.tags.length > 0 &&
    !isLocked('tags') &&
    shouldWrite(videoForm.tags, fillMode)
  ) {
    videoForm.tags = meta.tags.join(' / ')
    return true
  }
  if (
    field === 'summary' &&
    meta.summary &&
    !isLocked('summary') &&
    shouldWrite(videoForm.summary, fillMode)
  ) {
    videoForm.summary = meta.summary
    return true
  }
  if (
    field === 'track_or_episode' &&
    meta.track_or_episode_number != null &&
    !isLocked('episode_number') &&
    shouldWrite(videoForm.episode, fillMode)
  ) {
    videoForm.episode = String(meta.track_or_episode_number)
    return true
  }
  if (
    field === 'disc_or_season' &&
    meta.disc_or_season_number != null &&
    !isLocked('season_number') &&
    shouldWrite(videoForm.season, fillMode)
  ) {
    videoForm.season = String(meta.disc_or_season_number)
    return true
  }
  return false
}

export function applyVideoScrapedExternalIds(
  videoForm: VideoInspectorForm,
  isLocked: (field: string) => boolean,
  meta: ScrapedMetadata,
  fillMode: ApplyFillMode,
): void {
  const extIds = meta.external_ids ?? {}
  const tmdbId = extIds.tmdb || (meta.provider_id === 'tmdb' ? meta.external_id : '')
  const bgmId = extIds.bangumi || (meta.provider_id === 'bangumi' ? meta.external_id : '')
  if (tmdbId && !isLocked('tmdb_id') && shouldWrite(videoForm.tmdbId, fillMode)) {
    videoForm.tmdbId = tmdbId
  }
  if (bgmId && !isLocked('bangumi_id') && shouldWrite(videoForm.bangumiId, fillMode)) {
    videoForm.bangumiId = bgmId
  }
}

export function videoFormToScrapedMetadata(
  videoForm: VideoInspectorForm,
  base: ScrapedMetadata,
  coverUrl: string | null,
): ScrapedMetadata {
  const nextExtIds: Record<string, string> = { ...(base.external_ids ?? {}) }
  if (videoForm.tmdbId.trim()) nextExtIds.tmdb = videoForm.tmdbId.trim()
  else delete nextExtIds.tmdb
  if (videoForm.bangumiId.trim()) nextExtIds.bangumi = videoForm.bangumiId.trim()
  else delete nextExtIds.bangumi
  return {
    ...base,
    target: videoForm.isEpisode ? 'tv_show' : 'movie',
    title: videoForm.title.trim() || null,
    original_title: videoForm.originalTitle.trim() || null,
    series_or_album: videoForm.showTitle.trim() || null,
    disc_or_season_number: parseNullableInt(videoForm.season),
    track_or_episode_number: parseNullableInt(videoForm.episode),
    year: parseNullableInt(videoForm.year),
    rating: parseNullableFloat(videoForm.rating),
    publisher_or_studio: videoForm.studio.trim() || null,
    illustrators_or_directors: splitListTokens(videoForm.directors),
    cast: splitListTokens(videoForm.actors),
    genres: splitListTokens(videoForm.genres),
    tags: splitListTokens(videoForm.tags),
    summary: videoForm.summary.trim() || null,
    external_ids: nextExtIds,
    cover_url: coverUrl,
  }
}

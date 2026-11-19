import { computed, reactive, ref } from 'vue'
import type { MediaKind } from '@/types/api'
import type {
  MediaFileDetail,
  MediaMetadataPatchEnvelope,
  ScrapedMetadata,
  ScrapeQuery,
  ScrapeTargetKind,
} from '../../types'
import {
  isVideoFieldLockedIn,
  toggleVideoFieldLockIn,
  type BookInspectorForm,
  type ComicInspectorForm,
  type MusicInspectorForm,
  type VideoInspectorForm,
} from './types'

export type ScrapedFieldKey =
  | 'cover'
  | 'title'
  | 'original_title'
  | 'creators'
  | 'directors'
  | 'cast'
  | 'series_or_album'
  | 'publisher_or_studio'
  | 'year'
  | 'isbn'
  | 'rating'
  | 'language'
  | 'track_or_episode'
  | 'disc_or_season'
  | 'genres'
  | 'tags'
  | 'summary'
  | 'lyrics'

export type ApplyFillMode = 'overwrite' | 'fill_missing'

export function splitListTokens(raw: string): string[] {
  return raw
    .split(/[/；;、\n]/)
    .map((s) => s.trim())
    .filter((s) => s.length > 0)
}

function parseOptInt(raw: string, fallback: number): number {
  const trimmed = raw.trim()
  if (!trimmed) return fallback
  const n = Number.parseInt(trimmed, 10)
  return Number.isFinite(n) ? n : fallback
}

function parseClearableInt(raw: string): number {
  const trimmed = raw.trim()
  if (!trimmed) return -1
  const n = Number.parseInt(trimmed, 10)
  return Number.isFinite(n) && n >= 0 ? n : -1
}

export function inferTargetKindFromDetail(detail: MediaFileDetail): ScrapeTargetKind {
  const m = detail.metadata
  if (m.kind === 'book') return 'book'
  if (m.kind === 'comic') return 'comic'
  if (m.kind === 'music') return 'music_track'
  return m.data.is_episode ? 'tv_show' : 'movie'
}

export function buildDefaultQueryFromDetail(detail: MediaFileDetail): ScrapeQuery {
  const m = detail.metadata
  const fallbackName = detail.entry.file_name.replace(/\.[^.]+$/, '')
  if (m.kind === 'book') {
    return {
      target: 'book',
      keyword: (m.data.title || fallbackName).trim(),
      artist_or_author: m.data.authors[0] || undefined,
      album_or_series: m.data.series || undefined,
      isbn: m.data.isbn || undefined,
    }
  }
  if (m.kind === 'comic') {
    return {
      target: 'comic',
      keyword: (m.data.series || m.data.title || fallbackName).trim(),
      artist_or_author: m.data.writers[0] || m.data.pencillers[0] || undefined,
      year: m.data.year ?? undefined,
    }
  }
  if (m.kind === 'music') {
    return {
      target: 'music_track',
      keyword: (m.data.title || fallbackName).trim(),
      artist_or_author: m.data.artist || m.data.album_artist || undefined,
      album_or_series: m.data.album || undefined,
      year: m.data.year ?? undefined,
      duration_secs: m.data.duration_secs ?? undefined,
    }
  }
  const v = m.data
  return {
    target: v.is_episode ? 'tv_show' : 'movie',
    keyword: (v.is_episode ? v.show_title || v.title : v.title || fallbackName).trim(),
    year: v.year ?? undefined,
    season: v.season_number ?? undefined,
    episode: v.episode_number ?? undefined,
  }
}

export function useMetadataForm() {
  const baselineSnapshot = ref('')
  const pendingCoverUrl = ref<string | null>(null)
  const pendingBackdropUrl = ref<string | null>(null)
  const videoArtworkSlot = ref<'poster' | 'fanart' | 'thumb'>('poster')

  const bookForm = reactive<BookInspectorForm>({
    title: '',
    authors: '',
    series: '',
    subseries: '',
    seriesIndex: '',
    isbn: '',
    publisher: '',
    publishedAt: '',
    language: '',
    tags: '',
    description: '',
    lockedFields: [],
  })

  const comicForm = reactive<ComicInspectorForm>({
    series: '',
    seriesGroup: '',
    title: '',
    number: '',
    volume: '',
    year: '',
    language: '',
    direction: 'rtl',
    writers: '',
    pencillers: '',
    publisher: '',
    genres: '',
    tags: '',
    summary: '',
  })

  const musicForm = reactive<MusicInspectorForm>({
    title: '',
    artist: '',
    albumArtist: '',
    album: '',
    track: '',
    disc: '',
    year: '',
    genre: '',
    comment: '',
    embeddedLyrics: '',
    sidecarLrc: '',
  })

  const videoForm = reactive<VideoInspectorForm>({
    isEpisode: false,
    title: '',
    originalTitle: '',
    showTitle: '',
    season: '',
    episode: '',
    year: '',
    rating: '',
    studio: '',
    directors: '',
    actors: '',
    genres: '',
    tags: '',
    tmdbId: '',
    bangumiId: '',
    summary: '',
    lockedFields: [],
  })

  function isBookFieldLocked(field: string): boolean {
    return bookForm.lockedFields.includes(field)
  }

  function toggleBookFieldLock(field: string): void {
    if (bookForm.lockedFields.includes(field)) {
      bookForm.lockedFields = bookForm.lockedFields.filter((f) => f !== field)
    } else {
      bookForm.lockedFields = [...bookForm.lockedFields, field]
    }
  }

  function isVideoFieldLocked(field: string): boolean {
    return isVideoFieldLockedIn(videoForm.lockedFields, field)
  }

  function toggleVideoFieldLock(field: string): void {
    videoForm.lockedFields = toggleVideoFieldLockIn(videoForm.lockedFields, field)
  }

  function serializeFormSnapshot(): string {
    return JSON.stringify([
      bookForm,
      [...bookForm.lockedFields].sort().join(','),
      comicForm,
      musicForm,
      videoForm,
      [...videoForm.lockedFields].sort().join(','),
      pendingCoverUrl.value,
      pendingBackdropUrl.value,
    ])
  }

  const isDirty = computed(
    () => baselineSnapshot.value !== '' && serializeFormSnapshot() !== baselineSnapshot.value,
  )

  function hydrateForm(d: MediaFileDetail): void {
    pendingCoverUrl.value = null
    pendingBackdropUrl.value = null
    const m = d.metadata
    if (m.kind === 'book') {
      const b = m.data
      bookForm.title = b.title
      bookForm.authors = b.authors.join(' / ')
      bookForm.series = b.series ?? ''
      bookForm.subseries = b.subseries ?? ''
      bookForm.seriesIndex = b.series_index ?? ''
      bookForm.isbn = b.isbn ?? ''
      bookForm.publisher = b.publisher ?? ''
      bookForm.publishedAt = b.published_at ?? ''
      bookForm.language = b.language ?? ''
      bookForm.tags = b.tags.join(' / ')
      bookForm.description = b.description ?? ''
      bookForm.lockedFields = [...(b.locked_fields ?? [])]
    } else if (m.kind === 'comic') {
      const c = m.data
      comicForm.series = c.series ?? ''
      comicForm.seriesGroup = c.series_group ?? ''
      comicForm.title = c.title ?? ''
      comicForm.number = c.number ?? ''
      comicForm.volume = c.volume !== null ? String(c.volume) : ''
      comicForm.year = c.year !== null ? String(c.year) : ''
      comicForm.language = c.language_iso ?? ''
      comicForm.direction = c.reading_direction || 'rtl'
      comicForm.writers = c.writers.join(' / ')
      comicForm.pencillers = c.pencillers.join(' / ')
      comicForm.publisher = c.publisher ?? ''
      comicForm.genres = c.genres.join(' / ')
      comicForm.tags = c.tags.join(' / ')
      comicForm.summary = c.summary ?? ''
    } else if (m.kind === 'music') {
      const mu = m.data
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
    } else {
      const v = m.data
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
      if (v.has_poster) {
        videoArtworkSlot.value = 'poster'
      } else if (v.has_fanart) {
        videoArtworkSlot.value = 'fanart'
      } else if (v.has_thumb) {
        videoArtworkSlot.value = 'thumb'
      } else {
        videoArtworkSlot.value = 'poster'
      }
    }
    baselineSnapshot.value = serializeFormSnapshot()
  }

  function buildPatchEnvelope(kind: MediaKind): MediaMetadataPatchEnvelope {
    if (kind === 'book') {
      return {
        kind: 'book',
        patch: {
          title: bookForm.title,
          authors: splitListTokens(bookForm.authors),
          series: bookForm.series,
          subseries: bookForm.subseries,
          series_index: bookForm.seriesIndex,
          isbn: bookForm.isbn,
          publisher: bookForm.publisher,
          published_at: bookForm.publishedAt,
          language: bookForm.language,
          tags: splitListTokens(bookForm.tags),
          description: bookForm.description,
          locked_fields: bookForm.lockedFields,
        },
      }
    }
    if (kind === 'comic') {
      return {
        kind: 'comic',
        patch: {
          series: comicForm.series,
          series_group: comicForm.seriesGroup,
          title: comicForm.title,
          number: comicForm.number,
          volume: comicForm.volume.trim() ? Number.parseInt(comicForm.volume, 10) : -1,
          year: parseOptInt(comicForm.year, 0),
          language_iso: comicForm.language,
          reading_direction: comicForm.direction,
          writers: splitListTokens(comicForm.writers),
          pencillers: splitListTokens(comicForm.pencillers),
          publisher: comicForm.publisher,
          genres: splitListTokens(comicForm.genres),
          tags: splitListTokens(comicForm.tags),
          summary: comicForm.summary,
        },
      }
    }
    if (kind === 'music') {
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

  function shouldWrite(currentVal: string, fillMode: ApplyFillMode): boolean {
    return fillMode === 'overwrite' || currentVal.trim().length === 0
  }

  function applyScrapedField(
    kind: MediaKind,
    field: ScrapedFieldKey,
    meta: ScrapedMetadata,
    fillMode: ApplyFillMode = 'overwrite',
  ): boolean {
    if (field === 'cover') {
      if (!meta.cover_url) return false
      pendingCoverUrl.value = meta.cover_url
      if (kind === 'video' && meta.backdrop_url) {
        pendingBackdropUrl.value = meta.backdrop_url
      }
      return true
    }

    if (kind === 'book') {
      if (field === 'title' && meta.title && !isBookFieldLocked('title') && shouldWrite(bookForm.title, fillMode)) {
        bookForm.title = meta.title
        return true
      }
      if (field === 'creators' && meta.creators.length > 0 && !isBookFieldLocked('authors') && shouldWrite(bookForm.authors, fillMode)) {
        bookForm.authors = meta.creators.join(' / ')
        return true
      }
      if (field === 'series_or_album' && meta.series_or_album && !isBookFieldLocked('series') && shouldWrite(bookForm.series, fillMode)) {
        bookForm.series = meta.series_or_album
        return true
      }
      if (field === 'publisher_or_studio' && meta.publisher_or_studio && !isBookFieldLocked('publisher') && shouldWrite(bookForm.publisher, fillMode)) {
        bookForm.publisher = meta.publisher_or_studio
        return true
      }
      if (field === 'year' && (meta.published_date || meta.year) && !isBookFieldLocked('published_at') && shouldWrite(bookForm.publishedAt, fillMode)) {
        bookForm.publishedAt = meta.published_date || String(meta.year)
        return true
      }
      if (field === 'isbn' && meta.isbn && !isBookFieldLocked('isbn') && shouldWrite(bookForm.isbn, fillMode)) {
        bookForm.isbn = meta.isbn
        return true
      }
      if (field === 'language' && meta.language && !isBookFieldLocked('language') && shouldWrite(bookForm.language, fillMode)) {
        bookForm.language = meta.language
        return true
      }
      if ((field === 'tags' || field === 'genres') && !isBookFieldLocked('tags')) {
        const merged = [...meta.genres, ...meta.tags].filter(Boolean)
        const uniq = Array.from(new Set(merged))
        if (uniq.length > 0 && shouldWrite(bookForm.tags, fillMode)) {
          bookForm.tags = uniq.join(' / ')
          return true
        }
      }
      if (field === 'summary' && meta.summary && !isBookFieldLocked('description') && shouldWrite(bookForm.description, fillMode)) {
        bookForm.description = meta.summary
        return true
      }
      return false
    }

    if (kind === 'comic') {
      if (field === 'title' && meta.title && shouldWrite(comicForm.title, fillMode)) {
        comicForm.title = meta.title
        return true
      }
      if (field === 'series_or_album') {
        const s = meta.series_or_album || meta.title
        if (s && shouldWrite(comicForm.series, fillMode)) {
          comicForm.series = s
          return true
        }
      }
      if (field === 'creators' && meta.creators.length > 0 && shouldWrite(comicForm.writers, fillMode)) {
        comicForm.writers = meta.creators.join(' / ')
        return true
      }
      if (field === 'directors' && meta.illustrators_or_directors.length > 0 && shouldWrite(comicForm.pencillers, fillMode)) {
        comicForm.pencillers = meta.illustrators_or_directors.join(' / ')
        return true
      }
      if (field === 'publisher_or_studio' && meta.publisher_or_studio && shouldWrite(comicForm.publisher, fillMode)) {
        comicForm.publisher = meta.publisher_or_studio
        return true
      }
      if (field === 'year' && meta.year && shouldWrite(comicForm.year, fillMode)) {
        comicForm.year = String(meta.year)
        return true
      }
      if (field === 'language' && meta.language && shouldWrite(comicForm.language, fillMode)) {
        comicForm.language = meta.language
        return true
      }
      if (field === 'genres' && meta.genres.length > 0 && shouldWrite(comicForm.genres, fillMode)) {
        comicForm.genres = meta.genres.join(' / ')
        return true
      }
      if (field === 'tags' && meta.tags.length > 0 && shouldWrite(comicForm.tags, fillMode)) {
        comicForm.tags = meta.tags.join(' / ')
        return true
      }
      if (field === 'summary' && meta.summary && shouldWrite(comicForm.summary, fillMode)) {
        comicForm.summary = meta.summary
        return true
      }
      return false
    }

    if (kind === 'music') {
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
      if (field === 'series_or_album' && meta.series_or_album && shouldWrite(musicForm.album, fillMode)) {
        musicForm.album = meta.series_or_album
        return true
      }
      if (field === 'year' && meta.year && shouldWrite(musicForm.year, fillMode)) {
        musicForm.year = String(meta.year)
        return true
      }
      if (field === 'track_or_episode' && meta.track_or_episode_number && shouldWrite(musicForm.track, fillMode)) {
        musicForm.track = String(meta.track_or_episode_number)
        return true
      }
      if (field === 'disc_or_season' && meta.disc_or_season_number && shouldWrite(musicForm.disc, fillMode)) {
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

    // video
    if (field === 'title' && meta.title && !isVideoFieldLocked('title') && shouldWrite(videoForm.title, fillMode)) {
      videoForm.title = meta.title
      return true
    }
    if (field === 'original_title' && meta.original_title && !isVideoFieldLocked('original_title') && shouldWrite(videoForm.originalTitle, fillMode)) {
      videoForm.originalTitle = meta.original_title
      return true
    }
    if (field === 'series_or_album') {
      const show = meta.series_or_album || (videoForm.isEpisode ? meta.title : null)
      if (show && !isVideoFieldLocked('show_title') && shouldWrite(videoForm.showTitle, fillMode)) {
        videoForm.showTitle = show
        return true
      }
    }
    if (field === 'publisher_or_studio' && meta.publisher_or_studio && !isVideoFieldLocked('studio') && shouldWrite(videoForm.studio, fillMode)) {
      videoForm.studio = meta.publisher_or_studio
      return true
    }
    if (field === 'year' && meta.year && !isVideoFieldLocked('year') && shouldWrite(videoForm.year, fillMode)) {
      videoForm.year = String(meta.year)
      return true
    }
    if (field === 'rating' && meta.rating && !isVideoFieldLocked('rating') && shouldWrite(videoForm.rating, fillMode)) {
      videoForm.rating = String(meta.rating)
      return true
    }
    if (field === 'directors') {
      const dirs = meta.illustrators_or_directors.length > 0 ? meta.illustrators_or_directors : meta.creators
      if (dirs.length > 0 && !isVideoFieldLocked('directors') && shouldWrite(videoForm.directors, fillMode)) {
        videoForm.directors = dirs.join(' / ')
        return true
      }
    }
    if (field === 'cast' && meta.cast.length > 0 && !isVideoFieldLocked('actors') && shouldWrite(videoForm.actors, fillMode)) {
      videoForm.actors = meta.cast.join(' / ')
      return true
    }
    if (field === 'genres' && meta.genres.length > 0 && !isVideoFieldLocked('genres') && shouldWrite(videoForm.genres, fillMode)) {
      videoForm.genres = meta.genres.join(' / ')
      return true
    }
    if (field === 'tags' && meta.tags.length > 0 && !isVideoFieldLocked('tags') && shouldWrite(videoForm.tags, fillMode)) {
      videoForm.tags = meta.tags.join(' / ')
      return true
    }
    if (field === 'summary' && meta.summary && !isVideoFieldLocked('summary') && shouldWrite(videoForm.summary, fillMode)) {
      videoForm.summary = meta.summary
      return true
    }
    if (field === 'track_or_episode' && meta.track_or_episode_number && shouldWrite(videoForm.episode, fillMode)) {
      videoForm.episode = String(meta.track_or_episode_number)
      return true
    }
    if (field === 'disc_or_season' && meta.disc_or_season_number && shouldWrite(videoForm.season, fillMode)) {
      videoForm.season = String(meta.disc_or_season_number)
      return true
    }
    return false
  }

  function applyScrapedMetadata(
    kind: MediaKind,
    meta: ScrapedMetadata,
    fillMode: ApplyFillMode = 'overwrite',
  ): number {
    const fields: ScrapedFieldKey[] = [
      'cover',
      'title',
      'original_title',
      'series_or_album',
      'creators',
      'directors',
      'cast',
      'publisher_or_studio',
      'year',
      'isbn',
      'rating',
      'language',
      'track_or_episode',
      'disc_or_season',
      'genres',
      'tags',
      'summary',
      'lyrics',
    ]
    let applied = 0
    for (const f of fields) {
      if (applyScrapedField(kind, f, meta, fillMode)) {
        applied += 1
      }
    }
    if (kind === 'video') {
      const extIds = meta.external_ids ?? {}
      const tmdbId = extIds.tmdb || (meta.provider_id === 'tmdb' ? meta.external_id : '')
      const bgmId = extIds.bangumi || (meta.provider_id === 'bangumi' ? meta.external_id : '')
      if (tmdbId && shouldWrite(videoForm.tmdbId, fillMode)) {
        videoForm.tmdbId = tmdbId
      }
      if (bgmId && shouldWrite(videoForm.bangumiId, fillMode)) {
        videoForm.bangumiId = bgmId
      }
    }
    return applied
  }

  return {
    bookForm,
    comicForm,
    musicForm,
    videoForm,
    pendingCoverUrl,
    pendingBackdropUrl,
    videoArtworkSlot,
    isDirty,
    isBookFieldLocked,
    toggleBookFieldLock,
    isVideoFieldLocked,
    toggleVideoFieldLock,
    hydrateForm,
    buildPatchEnvelope,
    applyScrapedField,
    applyScrapedMetadata,
  }
}

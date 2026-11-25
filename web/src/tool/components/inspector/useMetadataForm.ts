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
  applyBookScrapedField,
  applyComicScrapedField,
  bookFormToScrapedMetadata,
  buildBookPatchEnvelope,
  buildComicPatchEnvelope,
  comicFormToScrapedMetadata,
  hydrateBookForm,
  hydrateComicForm,
  resetBookForm,
  resetComicForm,
  splitListTokens,
  type ApplyFillMode,
  type ScrapedFieldKey,
} from './formAdaptersBookComic'
import {
  applyMusicScrapedField,
  applyVideoScrapedExternalIds,
  applyVideoScrapedField,
  buildMusicPatchEnvelope,
  buildVideoPatchEnvelope,
  hydrateMusicForm,
  hydrateVideoForm,
  musicFormToScrapedMetadata,
  resetMusicForm,
  resetVideoForm,
  videoFormToScrapedMetadata,
} from './formAdaptersMusicVideo'
import {
  isVideoFieldLockedIn,
  toggleVideoFieldLockIn,
  type BookInspectorForm,
  type ComicInspectorForm,
  type MusicInspectorForm,
  type VideoInspectorForm,
} from './types'

export { splitListTokens, type ApplyFillMode, type ScrapedFieldKey }

const ALL_SCRAPED_FIELDS: ScrapedFieldKey[] = [
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
    if (m.kind === 'book') hydrateBookForm(bookForm, m.data)
    else if (m.kind === 'comic') hydrateComicForm(comicForm, m.data)
    else if (m.kind === 'music') hydrateMusicForm(musicForm, m.data)
    else videoArtworkSlot.value = hydrateVideoForm(videoForm, m.data)
    baselineSnapshot.value = serializeFormSnapshot()
  }

  function buildPatchEnvelope(kind: MediaKind): MediaMetadataPatchEnvelope {
    if (kind === 'book') return buildBookPatchEnvelope(bookForm)
    if (kind === 'comic') return buildComicPatchEnvelope(comicForm)
    if (kind === 'music') return buildMusicPatchEnvelope(musicForm)
    return buildVideoPatchEnvelope(videoForm)
  }

  function applyScrapedField(
    kind: MediaKind,
    field: ScrapedFieldKey,
    meta: ScrapedMetadata,
    fillMode: ApplyFillMode = 'overwrite',
  ): boolean {
    if (field === 'cover') {
      if (!meta.cover_url) return false
      if (kind === 'book' && isBookFieldLocked('cover')) return false
      if (kind === 'video' && isVideoFieldLocked('poster')) {
        if (meta.backdrop_url && !isVideoFieldLocked('fanart')) {
          pendingBackdropUrl.value = meta.backdrop_url
          return true
        }
        return false
      }
      pendingCoverUrl.value = meta.cover_url
      if (kind === 'video' && meta.backdrop_url && !isVideoFieldLocked('fanart')) {
        pendingBackdropUrl.value = meta.backdrop_url
      }
      return true
    }

    if (kind === 'book') {
      return applyBookScrapedField(bookForm, isBookFieldLocked, field, meta, fillMode)
    }
    if (kind === 'comic') {
      return applyComicScrapedField(comicForm, field, meta, fillMode)
    }
    if (kind === 'music') {
      return applyMusicScrapedField(musicForm, field, meta, fillMode)
    }
    return applyVideoScrapedField(videoForm, isVideoFieldLocked, field, meta, fillMode)
  }

  function applyScrapedMetadata(
    kind: MediaKind,
    meta: ScrapedMetadata,
    fillMode: ApplyFillMode = 'overwrite',
  ): number {
    let applied = 0
    for (const f of ALL_SCRAPED_FIELDS) {
      if (applyScrapedField(kind, f, meta, fillMode)) applied += 1
    }
    if (kind === 'video') {
      applyVideoScrapedExternalIds(videoForm, isVideoFieldLocked, meta, fillMode)
    }
    return applied
  }

  function resetAllForms(): void {
    pendingCoverUrl.value = null
    pendingBackdropUrl.value = null
    resetBookForm(bookForm)
    resetComicForm(comicForm)
    resetMusicForm(musicForm)
    resetVideoForm(videoForm)
  }

  function hydrateFromScrapedMetadata(kind: MediaKind, meta: ScrapedMetadata | null): void {
    resetAllForms()
    if (!meta) {
      baselineSnapshot.value = ''
      return
    }
    if (kind === 'video') {
      videoForm.isEpisode =
        meta.target === 'tv_show' ||
        meta.disc_or_season_number != null ||
        meta.track_or_episode_number != null
    }
    applyScrapedMetadata(kind, meta, 'overwrite')
    baselineSnapshot.value = serializeFormSnapshot()
  }

  function toScrapedMetadata(
    kind: MediaKind,
    base: ScrapedMetadata,
    includeCover = true,
  ): ScrapedMetadata {
    const coverUrl = includeCover ? base.cover_url : null
    if (kind === 'book') return bookFormToScrapedMetadata(bookForm, base, coverUrl)
    if (kind === 'comic') return comicFormToScrapedMetadata(comicForm, base, coverUrl)
    if (kind === 'music') return musicFormToScrapedMetadata(musicForm, base, coverUrl)
    return videoFormToScrapedMetadata(videoForm, base, coverUrl)
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
    hydrateFromScrapedMetadata,
    toScrapedMetadata,
    buildPatchEnvelope,
    applyScrapedField,
    applyScrapedMetadata,
  }
}

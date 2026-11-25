import type {
  MediaFileDetail,
  MediaMetadataPatchEnvelope,
  ScrapedMetadata,
} from '../../types'
import type { BookInspectorForm, ComicInspectorForm } from './types'

type BookMetadataData = Extract<MediaFileDetail['metadata'], { kind: 'book' }>['data']
type ComicMetadataData = Extract<MediaFileDetail['metadata'], { kind: 'comic' }>['data']

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

export function parseOptInt(raw: string, fallback: number): number {
  const trimmed = raw.trim()
  if (!trimmed) return fallback
  const n = Number.parseInt(trimmed, 10)
  return Number.isFinite(n) ? n : fallback
}

export function parseClearableInt(raw: string): number {
  const trimmed = raw.trim()
  if (!trimmed) return -1
  const n = Number.parseInt(trimmed, 10)
  return Number.isFinite(n) && n >= 0 ? n : -1
}

export function parseNullableInt(raw: string): number | null {
  const trimmed = raw.trim()
  if (!trimmed) return null
  const n = Number.parseInt(trimmed, 10)
  return Number.isFinite(n) ? n : null
}

export function parseNullableFloat(raw: string): number | null {
  const trimmed = raw.trim()
  if (!trimmed) return null
  const n = Number.parseFloat(trimmed)
  return Number.isFinite(n) ? n : null
}

export function shouldWrite(currentVal: string, fillMode: ApplyFillMode): boolean {
  return fillMode === 'overwrite' || currentVal.trim().length === 0
}

export function resetBookForm(bookForm: BookInspectorForm): void {
  bookForm.title = ''
  bookForm.authors = ''
  bookForm.series = ''
  bookForm.subseries = ''
  bookForm.seriesIndex = ''
  bookForm.isbn = ''
  bookForm.publisher = ''
  bookForm.publishedAt = ''
  bookForm.language = ''
  bookForm.tags = ''
  bookForm.description = ''
  bookForm.lockedFields = []
}

export function hydrateBookForm(bookForm: BookInspectorForm, b: BookMetadataData): void {
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
}

export function buildBookPatchEnvelope(bookForm: BookInspectorForm): MediaMetadataPatchEnvelope {
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

export function applyBookScrapedField(
  bookForm: BookInspectorForm,
  isLocked: (field: string) => boolean,
  field: ScrapedFieldKey,
  meta: ScrapedMetadata,
  fillMode: ApplyFillMode,
): boolean {
  if (field === 'title' && meta.title && !isLocked('title') && shouldWrite(bookForm.title, fillMode)) {
    bookForm.title = meta.title
    return true
  }
  if (
    field === 'creators' &&
    meta.creators.length > 0 &&
    !isLocked('authors') &&
    shouldWrite(bookForm.authors, fillMode)
  ) {
    bookForm.authors = meta.creators.join(' / ')
    return true
  }
  if (
    field === 'series_or_album' &&
    meta.series_or_album &&
    !isLocked('series') &&
    shouldWrite(bookForm.series, fillMode)
  ) {
    bookForm.series = meta.series_or_album
    return true
  }
  if (
    field === 'publisher_or_studio' &&
    meta.publisher_or_studio &&
    !isLocked('publisher') &&
    shouldWrite(bookForm.publisher, fillMode)
  ) {
    bookForm.publisher = meta.publisher_or_studio
    return true
  }
  if (
    field === 'year' &&
    (meta.published_date || meta.year) &&
    !isLocked('published_at') &&
    shouldWrite(bookForm.publishedAt, fillMode)
  ) {
    bookForm.publishedAt = meta.published_date || String(meta.year)
    return true
  }
  if (field === 'isbn' && meta.isbn && !isLocked('isbn') && shouldWrite(bookForm.isbn, fillMode)) {
    bookForm.isbn = meta.isbn
    return true
  }
  if (
    field === 'language' &&
    meta.language &&
    !isLocked('language') &&
    shouldWrite(bookForm.language, fillMode)
  ) {
    bookForm.language = meta.language
    return true
  }
  if ((field === 'tags' || field === 'genres') && !isLocked('tags')) {
    const uniq = Array.from(new Set([...meta.genres, ...meta.tags].filter(Boolean)))
    if (uniq.length > 0 && shouldWrite(bookForm.tags, fillMode)) {
      bookForm.tags = uniq.join(' / ')
      return true
    }
  }
  if (
    field === 'summary' &&
    meta.summary &&
    !isLocked('description') &&
    shouldWrite(bookForm.description, fillMode)
  ) {
    bookForm.description = meta.summary
    return true
  }
  return false
}

export function bookFormToScrapedMetadata(
  bookForm: BookInspectorForm,
  base: ScrapedMetadata,
  coverUrl: string | null,
): ScrapedMetadata {
  const trimmedDate = bookForm.publishedAt.trim()
  const yearMatch = trimmedDate.match(/^(\d{4})/)
  const parsedYear = yearMatch ? Number.parseInt(yearMatch[1]!, 10) : null
  return {
    ...base,
    title: bookForm.title.trim() || null,
    creators: splitListTokens(bookForm.authors),
    series_or_album: bookForm.series.trim() || null,
    isbn: bookForm.isbn.trim() || null,
    publisher_or_studio: bookForm.publisher.trim() || null,
    published_date: trimmedDate || null,
    year: parsedYear !== null && Number.isFinite(parsedYear) ? parsedYear : null,
    language: bookForm.language.trim() || null,
    genres: [],
    tags: splitListTokens(bookForm.tags),
    summary: bookForm.description.trim() || null,
    cover_url: coverUrl,
  }
}

export function resetComicForm(comicForm: ComicInspectorForm): void {
  comicForm.series = ''
  comicForm.seriesGroup = ''
  comicForm.title = ''
  comicForm.number = ''
  comicForm.volume = ''
  comicForm.year = ''
  comicForm.language = ''
  comicForm.direction = 'rtl'
  comicForm.writers = ''
  comicForm.pencillers = ''
  comicForm.publisher = ''
  comicForm.genres = ''
  comicForm.tags = ''
  comicForm.summary = ''
}

export function hydrateComicForm(comicForm: ComicInspectorForm, c: ComicMetadataData): void {
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
}

export function buildComicPatchEnvelope(comicForm: ComicInspectorForm): MediaMetadataPatchEnvelope {
  return {
    kind: 'comic',
    patch: {
      series: comicForm.series,
      series_group: comicForm.seriesGroup,
      title: comicForm.title,
      number: comicForm.number,
      volume: parseClearableInt(comicForm.volume),
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

export function applyComicScrapedField(
  comicForm: ComicInspectorForm,
  field: ScrapedFieldKey,
  meta: ScrapedMetadata,
  fillMode: ApplyFillMode,
): boolean {
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
  if (
    field === 'directors' &&
    meta.illustrators_or_directors.length > 0 &&
    shouldWrite(comicForm.pencillers, fillMode)
  ) {
    comicForm.pencillers = meta.illustrators_or_directors.join(' / ')
    return true
  }
  if (
    field === 'publisher_or_studio' &&
    meta.publisher_or_studio &&
    shouldWrite(comicForm.publisher, fillMode)
  ) {
    comicForm.publisher = meta.publisher_or_studio
    return true
  }
  if (field === 'year' && meta.year != null && shouldWrite(comicForm.year, fillMode)) {
    comicForm.year = String(meta.year)
    return true
  }
  if (
    field === 'disc_or_season' &&
    meta.disc_or_season_number != null &&
    shouldWrite(comicForm.volume, fillMode)
  ) {
    comicForm.volume = String(meta.disc_or_season_number)
    return true
  }
  if (
    field === 'track_or_episode' &&
    meta.track_or_episode_number != null &&
    shouldWrite(comicForm.number, fillMode)
  ) {
    comicForm.number = String(meta.track_or_episode_number)
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

export function comicFormToScrapedMetadata(
  comicForm: ComicInspectorForm,
  base: ScrapedMetadata,
  coverUrl: string | null,
): ScrapedMetadata {
  return {
    ...base,
    title: comicForm.title.trim() || null,
    series_or_album: comicForm.series.trim() || null,
    creators: splitListTokens(comicForm.writers),
    illustrators_or_directors: splitListTokens(comicForm.pencillers),
    publisher_or_studio: comicForm.publisher.trim() || null,
    year: parseNullableInt(comicForm.year),
    disc_or_season_number: parseNullableInt(comicForm.volume),
    track_or_episode_number: parseNullableInt(comicForm.number),
    language: comicForm.language.trim() || null,
    genres: splitListTokens(comicForm.genres),
    tags: splitListTokens(comicForm.tags),
    summary: comicForm.summary.trim() || null,
    cover_url: coverUrl,
  }
}

import type { ScrapeCandidate, ScrapedMetadata } from '../../types'

const CN_NOVEL_PROVIDER_IDS = new Set([
  'qidian',
  'fanqie',
  'qimao',
  'zongheng',
  'jjwxc',
  'sfacg',
  'ciweimao',
  'faloo',
])

const MUSIC_PROVIDER_IDS = new Set([
  'qq',
  'qq_music',
  'netease',
  'kugou',
  'kuwo',
  'taihe',
  'qishui',
  'musicbrainz',
  'itunes',
  'lrclib',
])

export function candKey(c: ScrapeCandidate): string {
  return `${c.provider_id}::${c.external_id}`
}

export function buildLightMetadata(c: ScrapeCandidate): ScrapedMetadata {
  return {
    provider_id: c.provider_id,
    external_id: c.external_id,
    target: c.target,
    title: c.title,
    original_title: null,
    series_or_album: null,
    creators: c.subtitle ? [c.subtitle] : [],
    illustrators_or_directors: [],
    cast: [],
    publisher_or_studio: null,
    year: c.year,
    published_date: null,
    language: null,
    isbn: null,
    rating: null,
    track_or_episode_number: null,
    disc_or_season_number: null,
    summary: c.summary,
    genres: [],
    tags: [],
    cover_url: c.cover_url,
    backdrop_url: null,
    synced_lyrics: c.synced_lyrics,
    plain_lyrics: c.plain_lyrics,
  }
}

export function buildCandidateFallbackMeta(c: ScrapeCandidate): ScrapedMetadata {
  const creators: string[] = []
  const genres: string[] = []
  let seriesOrAlbum: string | null = null
  let publisherOrStudio: string | null = null
  let language: string | null = null

  const sub = (c.subtitle ?? '').trim()
  if (sub) {
    const parts = sub
      .split(' · ')
      .map((s) => s.trim())
      .filter(Boolean)
    if (CN_NOVEL_PROVIDER_IDS.has(c.provider_id)) {
      if (parts[0]) creators.push(parts[0])
      if (parts[1]) genres.push(parts[1])
    } else if (MUSIC_PROVIDER_IDS.has(c.provider_id)) {
      if (parts[0]) {
        for (const a of parts[0]
          .split(/[/、]/)
          .map((s) => s.trim())
          .filter(Boolean)) {
          if (!creators.includes(a)) creators.push(a)
        }
      }
      if (parts[1]) seriesOrAlbum = parts[1]
    } else if (c.target === 'book' || c.target === 'comic') {
      if (parts[0]) {
        for (const a of parts[0]
          .split(' / ')
          .map((s) => s.trim())
          .filter(Boolean)) {
          if (!creators.includes(a)) creators.push(a)
        }
      }
      if (parts[1]) publisherOrStudio = parts[1]
    }
  }

  if (CN_NOVEL_PROVIDER_IDS.has(c.provider_id)) {
    if (!publisherOrStudio && c.provider_name.trim()) {
      publisherOrStudio = c.provider_name.trim()
    }
    language = 'zh-CN'
  }

  return {
    provider_id: c.provider_id,
    external_id: c.external_id,
    target: c.target,
    title: c.title || null,
    original_title: null,
    series_or_album: seriesOrAlbum,
    creators,
    illustrators_or_directors: [],
    cast: [],
    publisher_or_studio: publisherOrStudio,
    year: c.year ?? null,
    published_date: null,
    language,
    isbn: null,
    rating: null,
    track_or_episode_number: null,
    disc_or_season_number: null,
    summary: c.summary ?? null,
    genres,
    tags: [],
    cover_url: c.cover_url ?? null,
    backdrop_url: null,
    synced_lyrics: c.synced_lyrics ?? null,
    plain_lyrics: c.plain_lyrics ?? null,
  }
}

export function countPopulatedFields(meta: ScrapedMetadata): number {
  let count = 0
  if (meta.title) count += 1
  if (meta.original_title && meta.original_title !== meta.title) count += 1
  if (meta.creators.length > 0) count += 1
  if (meta.illustrators_or_directors.length > 0) count += 1
  if (meta.series_or_album) count += 1
  if (meta.publisher_or_studio) count += 1
  if (meta.published_date || meta.year) count += 1
  if (meta.isbn) count += 1
  if (meta.rating) count += 1
  if (meta.language) count += 1
  if (meta.disc_or_season_number) count += 1
  if (meta.track_or_episode_number) count += 1
  if (meta.cast.length > 0) count += 1
  if (meta.genres.length > 0) count += 1
  if (meta.tags.length > 0) count += 1
  if (meta.summary) count += 1
  if (meta.synced_lyrics || meta.plain_lyrics) count += 1
  return count
}

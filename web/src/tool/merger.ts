import type {
  FieldSourceBadge,
  MergedScrapedResult,
  ProviderBriefDto,
  ScrapeTargetKind,
  ScrapedMetadata,
} from './types'

function nonEmptyStr(val: string | null | undefined): string | null {
  const trimmed = val?.trim()
  return trimmed && trimmed.length > 0 ? trimmed : null
}

function normalizeForMatch(raw: string | null | undefined): string {
  if (!raw) return ''
  return Array.from(raw.toLowerCase())
    .filter((ch) => /[\p{L}\p{N}]/u.test(ch))
    .join('')
}

function normalizeIsbn(raw: string | null | undefined): string {
  if (!raw) return ''
  return raw.replace(/[^0-9xX]/g, '').toUpperCase()
}

function isAsciiOnly(s: string): boolean {
  for (let i = 0; i < s.length; i += 1) {
    if (s.charCodeAt(i) > 127) return false
  }
  return true
}

function promoteExactMatchAnchor(
  sources: Array<{ badge: FieldSourceBadge; data: ScrapedMetadata }>,
  keyword?: string,
  authorHint?: string,
  isbnHint?: string,
): void {
  if (sources.length <= 1) return
  if (isbnHint && isbnHint.trim().length > 0) return
  const kwNorm = normalizeForMatch(keyword)
  if (!kwNorm) return
  const authorNorm = normalizeForMatch(authorHint)

  const exactTitle = (m: ScrapedMetadata): boolean =>
    [m.title, m.original_title, m.series_or_album].some(
      (t) => Boolean(t) && normalizeForMatch(t) === kwNorm,
    )
  const exactAuthor = (m: ScrapedMetadata): boolean =>
    Boolean(authorNorm) && m.creators.some((c) => normalizeForMatch(c) === authorNorm)

  if (authorNorm) {
    if (exactTitle(sources[0]!.data) && exactAuthor(sources[0]!.data)) return
    const idx = sources.findIndex((s) => exactTitle(s.data) && exactAuthor(s.data))
    if (idx > 0) {
      const [item] = sources.splice(idx, 1)
      sources.unshift(item!)
      return
    }
  }

  if (exactTitle(sources[0]!.data)) return
  const idx = sources.findIndex((s) => exactTitle(s.data))
  if (idx > 0) {
    const [item] = sources.splice(idx, 1)
    sources.unshift(item!)
  }
}

function isCompatibleSecondarySource(
  primary: ScrapedMetadata,
  secondary: ScrapedMetadata,
  target: ScrapeTargetKind | null,
): boolean {
  const i1 = normalizeIsbn(primary.isbn)
  const i2 = normalizeIsbn(secondary.isbn)
  if (i1 && i2 && i1 !== i2) return false

  const effTarget = target ?? primary.target ?? secondary.target
  if (
    (effTarget === 'movie' ||
      effTarget === 'tv_show' ||
      effTarget === 'music_track' ||
      effTarget === 'music_album') &&
    primary.year !== null &&
    secondary.year !== null &&
    Math.abs(primary.year - secondary.year) > 2
  ) {
    return false
  }

  const collectTitles = (m: ScrapedMetadata): string[] =>
    [m.title, m.original_title, m.series_or_album]
      .map((s) => normalizeForMatch(s))
      .filter((s) => s.length > 0)

  const pTitles = collectTitles(primary)
  const sTitles = collectTitles(secondary)
  if (pTitles.length > 0 && sTitles.length > 0) {
    const overlap = pTitles.some((pt) =>
      sTitles.some(
        (st) =>
          pt === st ||
          (Array.from(pt).length >= 2 &&
            Array.from(st).length >= 2 &&
            (pt.includes(st) || st.includes(pt))),
      ),
    )
    if (!overlap) {
      const pAscii = pTitles.every(isAsciiOnly)
      const sAscii = sTitles.every(isAsciiOnly)
      if (pAscii && sAscii) {
        const pCreators = primary.creators.map(normalizeForMatch).filter(Boolean)
        const sCreators = secondary.creators.map(normalizeForMatch).filter(Boolean)
        const creatorMatch = pCreators.some((pc) =>
          sCreators.some(
            (sc) => pc === sc || (pc.length >= 2 && sc.length >= 2 && (pc.includes(sc) || sc.includes(pc))),
          ),
        )
        if (!creatorMatch) return false
      }
    }
  }

  if (effTarget === 'book' && primary.creators.length > 0 && secondary.creators.length > 0) {
    const pCreators = primary.creators.map(normalizeForMatch).filter(Boolean)
    const sCreators = secondary.creators.map(normalizeForMatch).filter(Boolean)
    const pAscii = pCreators.every(isAsciiOnly)
    const sAscii = sCreators.every(isAsciiOnly)
    if (pCreators.length > 0 && sCreators.length > 0 && pAscii === sAscii) {
      const sameAuthor = pCreators.some((pc) =>
        sCreators.some((sc) => pc === sc || pc.includes(sc) || sc.includes(pc)),
      )
      if (!sameAuthor) return false
    }
  }

  return true
}

/**
 * Pure domain waterfall merger mirroring backend merge_scraped_by_priority,
 * with additional awareness of brokenImageUrls so a 404 cover in source #1
 * automatically falls back to source #2's cover in real time.
 */
export function mergeScrapedClientSide(
  providerOrder: ProviderBriefDto[],
  detailsByProvider: Record<string, ScrapedMetadata>,
  target: ScrapeTargetKind | null,
  brokenImageUrls: Set<string>,
  ignoredProviderIds: Set<string> = new Set(),
  queryHints?: { keyword?: string; author?: string; isbn?: string },
): MergedScrapedResult | null {
  const rawOrdered: Array<{ badge: FieldSourceBadge; data: ScrapedMetadata }> = []
  for (const p of providerOrder) {
    if (ignoredProviderIds.has(p.id)) continue
    const d = detailsByProvider[p.id]
    if (d) {
      rawOrdered.push({
        badge: { provider_id: p.id, provider_name: p.name },
        data: d,
      })
    }
  }
  if (rawOrdered.length === 0) {
    return null
  }
  if (queryHints) {
    promoteExactMatchAnchor(rawOrdered, queryHints.keyword, queryHints.author, queryHints.isbn)
  }
  const primaryMeta = rawOrdered[0]!.data
  const ordered =
    rawOrdered.length > 1
      ? rawOrdered.filter(
          (entry, idx) => idx === 0 || isCompatibleSecondarySource(primaryMeta, entry.data, target),
        )
      : rawOrdered

  const externalIds: Record<string, string> = {}
  const out: ScrapedMetadata = {
    provider_id: '',
    external_id: '',
    target,
    title: null,
    original_title: null,
    series_or_album: null,
    creators: [],
    illustrators_or_directors: [],
    cast: [],
    publisher_or_studio: null,
    year: null,
    published_date: null,
    language: null,
    isbn: null,
    rating: null,
    track_or_episode_number: null,
    disc_or_season_number: null,
    summary: null,
    show_summary: null,
    season_title: null,
    season_summary: null,
    season_poster_url: null,
    episode_still_url: null,
    genres: [],
    tags: [],
    cover_url: null,
    backdrop_url: null,
    synced_lyrics: null,
    plain_lyrics: null,
    external_ids: externalIds,
  }
  const sources: Record<string, FieldSourceBadge> = {}

  for (const { badge, data: src } of ordered) {
    if (!out.provider_id && src.provider_id) {
      out.provider_id = src.provider_id
      out.external_id = src.external_id
    }
    if (src.provider_id && src.external_id && !externalIds[src.provider_id]) {
      externalIds[src.provider_id] = src.external_id
    }
    if (src.external_ids) {
      for (const [k, v] of Object.entries(src.external_ids)) {
        if (!externalIds[k]) externalIds[k] = v
      }
    }
    if (!out.target && src.target) {
      out.target = src.target
    }

    const title = nonEmptyStr(src.title)
    if (!out.title && title) {
      out.title = title
      sources.title = badge
    }

    const orig = nonEmptyStr(src.original_title)
    if (!out.original_title && orig) {
      out.original_title = orig
      sources.original_title = badge
    }

    const series = nonEmptyStr(src.series_or_album)
    if (!out.series_or_album && series) {
      out.series_or_album = series
      sources.series_or_album = badge
    }

    if (out.creators.length === 0 && src.creators.length > 0) {
      out.creators = [...src.creators]
      sources.creators = badge
    }

    if (out.illustrators_or_directors.length === 0 && src.illustrators_or_directors.length > 0) {
      out.illustrators_or_directors = [...src.illustrators_or_directors]
      sources.illustrators_or_directors = badge
    }

    if (out.cast.length === 0 && src.cast.length > 0) {
      out.cast = [...src.cast]
      sources.cast = badge
    }

    const pub = nonEmptyStr(src.publisher_or_studio)
    if (!out.publisher_or_studio && pub) {
      out.publisher_or_studio = pub
      sources.publisher_or_studio = badge
    }

    if (out.year === null && src.year !== null && src.year > 0) {
      out.year = src.year
      sources.year = badge
    }

    const pubDate = nonEmptyStr(src.published_date)
    if (!out.published_date && pubDate) {
      out.published_date = pubDate
      if (!sources.year) sources.year = badge
    }

    const lang = nonEmptyStr(src.language)
    if (!out.language && lang) {
      out.language = lang
      sources.language = badge
    }

    const isbn = nonEmptyStr(src.isbn)
    if (!out.isbn && isbn) {
      out.isbn = isbn
      sources.isbn = badge
    }

    if (out.rating === null && src.rating !== null && src.rating > 0) {
      out.rating = src.rating
      sources.rating = badge
    }

    if (
      out.track_or_episode_number === null &&
      src.track_or_episode_number !== null &&
      src.track_or_episode_number > 0
    ) {
      out.track_or_episode_number = src.track_or_episode_number
      sources.track_or_episode_number = badge
    }

    const allowZeroSeason = target === 'tv_show' || src.target === 'tv_show'
    if (
      out.disc_or_season_number === null &&
      src.disc_or_season_number !== null &&
      (src.disc_or_season_number > 0 || (allowZeroSeason && src.disc_or_season_number === 0))
    ) {
      out.disc_or_season_number = src.disc_or_season_number
      sources.disc_or_season_number = badge
    }

    const summary = nonEmptyStr(src.summary)
    if (!out.summary && summary) {
      out.summary = summary
      sources.summary = badge
    }

    const showSummary = nonEmptyStr(src.show_summary)
    if (!out.show_summary && showSummary) {
      out.show_summary = showSummary
      sources.show_summary = badge
    }

    const seasonTitle = nonEmptyStr(src.season_title)
    if (!out.season_title && seasonTitle) {
      out.season_title = seasonTitle
      sources.season_title = badge
    }

    const seasonSummary = nonEmptyStr(src.season_summary)
    if (!out.season_summary && seasonSummary) {
      out.season_summary = seasonSummary
      sources.season_summary = badge
    }

    if (out.genres.length === 0 && src.genres.length > 0) {
      out.genres = [...src.genres]
      sources.genres = badge
    }

    if (out.tags.length === 0 && src.tags.length > 0) {
      out.tags = [...src.tags]
      sources.tags = badge
    }

    const cover = nonEmptyStr(src.cover_url)
    if (!out.cover_url && cover && !brokenImageUrls.has(cover)) {
      out.cover_url = cover
      sources.cover_url = badge
    }

    const backdrop = nonEmptyStr(src.backdrop_url)
    if (!out.backdrop_url && backdrop && !brokenImageUrls.has(backdrop)) {
      out.backdrop_url = backdrop
      sources.backdrop_url = badge
    }

    const seasonPoster = nonEmptyStr(src.season_poster_url)
    if (!out.season_poster_url && seasonPoster && !brokenImageUrls.has(seasonPoster)) {
      out.season_poster_url = seasonPoster
      sources.season_poster_url = badge
    }

    const epStill = nonEmptyStr(src.episode_still_url)
    if (!out.episode_still_url && epStill && !brokenImageUrls.has(epStill)) {
      out.episode_still_url = epStill
      sources.episode_still_url = badge
    }

    const synced = nonEmptyStr(src.synced_lyrics)
    if (!out.synced_lyrics && synced) {
      out.synced_lyrics = synced
      sources.lyrics = badge
    }

    const plain = nonEmptyStr(src.plain_lyrics)
    if (!out.plain_lyrics && plain) {
      out.plain_lyrics = plain
      if (!out.synced_lyrics && !sources.lyrics) {
        sources.lyrics = badge
      }
    }
  }

  return {
    metadata: out,
    field_sources: sources,
  }
}

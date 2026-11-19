export interface BookInspectorForm {
  title: string
  authors: string
  series: string
  subseries: string
  seriesIndex: string
  isbn: string
  publisher: string
  publishedAt: string
  language: string
  tags: string
  description: string
  lockedFields: string[]
}

export interface ComicInspectorForm {
  series: string
  seriesGroup: string
  title: string
  number: string
  volume: string
  year: string
  language: string
  direction: string
  writers: string
  pencillers: string
  publisher: string
  genres: string
  tags: string
  summary: string
}

export interface MusicInspectorForm {
  title: string
  artist: string
  albumArtist: string
  album: string
  track: string
  disc: string
  year: string
  genre: string
  comment: string
  embeddedLyrics: string
  sidecarLrc: string
}

export interface VideoInspectorForm {
  isEpisode: boolean
  title: string
  originalTitle: string
  showTitle: string
  season: string
  episode: string
  year: string
  rating: string
  studio: string
  directors: string
  actors: string
  genres: string
  tags: string
  tmdbId: string
  bangumiId: string
  summary: string
  lockedFields: string[]
}

export function isVideoFieldLockedIn(lockedFields: string[], field: string): boolean {
  const f = field.toLowerCase()
  return lockedFields.some((x) => {
    const lx = x.toLowerCase()
    if (f === 'poster') return lx === 'poster' || lx === 'cover'
    return lx === f
  })
}

export function toggleVideoFieldLockIn(lockedFields: string[], field: string): string[] {
  const f = field.toLowerCase()
  if (isVideoFieldLockedIn(lockedFields, f)) {
    return lockedFields.filter((x) => {
      const lx = x.toLowerCase()
      if (f === 'poster') return lx !== 'poster' && lx !== 'cover'
      return lx !== f
    })
  }
  return [...lockedFields, f]
}

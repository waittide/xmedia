import { api as bookApi } from '@/domains/book/api'
import type { BookHomeOverviewDto } from '@/domains/book/types'
import { api as comicApi } from '@/domains/comic/api'
import type { HomeOverviewDto as ComicHomeOverviewDto } from '@/domains/comic/types'
import { api as musicApi } from '@/domains/music/api'
import type { HomeOverviewDto as MusicHomeOverviewDto } from '@/domains/music/types'
import { api as videoApi } from '@/domains/video/api'
import type { VideoHomeOverviewDto } from '@/domains/video/types'
import type { MediaKind } from '@/types/api'

export interface DomainHomeMap {
  book: BookHomeOverviewDto
  comic: ComicHomeOverviewDto
  music: MusicHomeOverviewDto
  video: VideoHomeOverviewDto
}

const cache: { [K in MediaKind]?: { data: DomainHomeMap[K]; updatedAt: number } } = {}
const inflight: { [K in MediaKind]?: Promise<DomainHomeMap[K]> } = {}

export function peekHomeCache<K extends MediaKind>(kind: K): DomainHomeMap[K] | null {
  return (cache[kind]?.data as DomainHomeMap[K] | undefined) ?? null
}

export function fetchHomeOverview<K extends MediaKind>(
  kind: K,
  options: { force?: boolean; maxAgeMs?: number } = {},
): Promise<DomainHomeMap[K]> {
  const { force = false, maxAgeMs = 15_000 } = options
  const existing = cache[kind]
  if (!force && existing && Date.now() - existing.updatedAt < maxAgeMs) {
    return Promise.resolve(existing.data as DomainHomeMap[K])
  }

  const pending = inflight[kind]
  if (pending) return pending as Promise<DomainHomeMap[K]>

  const req = (async (): Promise<DomainHomeMap[K]> => {
    let data: unknown
    if (kind === 'book') data = await bookApi.homeOverview()
    else if (kind === 'comic') data = await comicApi.homeOverview()
    else if (kind === 'music') data = await musicApi.homeOverview()
    else data = await videoApi.homeOverview()

    ;(cache as Record<MediaKind, { data: unknown; updatedAt: number }>)[kind] = {
      data,
      updatedAt: Date.now(),
    }
    return data as DomainHomeMap[K]
  })().finally(() => {
    delete inflight[kind]
  })

  ;(inflight as Record<MediaKind, Promise<unknown>>)[kind] = req
  return req
}

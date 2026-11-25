import type { BookHomeOverviewDto } from '@/domains/book/types'
import type { HomeOverviewDto as ComicHomeOverviewDto } from '@/domains/comic/types'
import type { HomeOverviewDto as MusicHomeOverviewDto } from '@/domains/music/types'
import type { VideoHomeOverviewDto } from '@/domains/video/types'
import { getDomainPlugin } from '@/sources/domainRegistry'
import type { MediaKind } from '@/types/api'

export interface DomainHomeMap {
  book: BookHomeOverviewDto
  comic: ComicHomeOverviewDto
  music: MusicHomeOverviewDto
  video: VideoHomeOverviewDto
}

type HomeCacheMap = { [K in MediaKind]?: { data: DomainHomeMap[K]; updatedAt: number } }
type HomeInflightMap = { [K in MediaKind]?: Promise<DomainHomeMap[K]> }

let currentUserKey = 'anon'
let cacheEpoch = 0
const cacheByUser: Record<string, HomeCacheMap> = {}
const inflightByUser: Record<string, HomeInflightMap> = {}

export function setHomeCacheUser(userId: string | null | undefined): void {
  const next = userId?.trim() || 'anon'
  if (next !== currentUserKey) {
    currentUserKey = next
  }
}

export function clearHomeCache(): void {
  cacheEpoch += 1
  for (const k of Object.keys(cacheByUser)) {
    delete cacheByUser[k]
  }
  for (const k of Object.keys(inflightByUser)) {
    delete inflightByUser[k]
  }
}

export function invalidateHomeCache(kind?: MediaKind): void {
  if (!kind) {
    clearHomeCache()
    return
  }
  cacheEpoch += 1
  for (const bucket of Object.values(cacheByUser)) {
    delete bucket[kind]
  }
}

export function peekHomeCache<K extends MediaKind>(kind: K): DomainHomeMap[K] | null {
  return (cacheByUser[currentUserKey]?.[kind]?.data as DomainHomeMap[K] | undefined) ?? null
}

export function hasHomeOverviewAvailable(kind: MediaKind): boolean {
  return Boolean(cacheByUser[currentUserKey]?.[kind] || inflightByUser[currentUserKey]?.[kind])
}

export function fetchHomeOverview<K extends MediaKind>(
  kind: K,
  options: { force?: boolean; maxAgeMs?: number } = {},
): Promise<DomainHomeMap[K]> {
  const { force = false, maxAgeMs = 15_000 } = options
  const userKey = currentUserKey
  const reqEpoch = cacheEpoch
  const userCache = (cacheByUser[userKey] ??= {})
  const userInflight = (inflightByUser[userKey] ??= {})

  const pending = userInflight[kind]
  if (!force && pending) return pending as Promise<DomainHomeMap[K]>

  const existing = userCache[kind]
  if (!force && existing && Date.now() - existing.updatedAt < maxAgeMs) {
    return Promise.resolve(existing.data as DomainHomeMap[K])
  }

  const req = (async (): Promise<DomainHomeMap[K]> => {
    const data = await getDomainPlugin(kind).fetchHomeOverview()

    if (reqEpoch === cacheEpoch && userKey === currentUserKey) {
      const bucket = (cacheByUser[userKey] ??= {})
      ;(bucket as Record<MediaKind, { data: unknown; updatedAt: number }>)[kind] = {
        data,
        updatedAt: Date.now(),
      }
    }
    return data as DomainHomeMap[K]
  })().finally(() => {
    if (inflightByUser[userKey]?.[kind] === req) {
      delete inflightByUser[userKey]?.[kind]
    }
  })

  ;(userInflight as Record<MediaKind, Promise<unknown>>)[kind] = req
  return req
}

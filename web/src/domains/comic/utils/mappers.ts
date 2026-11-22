import { withAuthUrl } from '@/domains/comic/api'
import {
  ONLINE_COMIC_CTX_KEY,
  resolveOnlineComicCoverUrl,
  type ComicSearchResultItem,
  type OnlineComicContextStorage,
} from '@/domains/comic/onlineApi'
import type { SeriesSummaryDto } from '@/domains/comic/types'
import {
  itemRoute,
  playRoute,
  type MediaItemVM,
  type OnlineItemRef,
  type OnlineItemSnapshot,
} from '@/sources'

export function seriesToMediaItemVM(
  series: SeriesSummaryDto,
  options?: {
    subtitleOverride?: string
    chaptersCountLabel?: string
    statusLabel?: string
  },
): MediaItemVM {
  const ref = { scope: 'local' as const, domain: 'comic' as const, id: series.id }
  const progressPercent =
    series.chapter_count > 0 && series.completed_chapters > 0
      ? Math.min(100, Math.max(0, Math.round((series.completed_chapters / series.chapter_count) * 100)))
      : undefined

  const creators = [...series.writers, ...series.pencillers].filter(Boolean).join(' · ')
  const subtitleParts: string[] = []
  if (options?.subtitleOverride) {
    subtitleParts.push(options.subtitleOverride)
  } else {
    if (creators) subtitleParts.push(creators)
    else if (options?.statusLabel) subtitleParts.push(options.statusLabel)
    if (options?.chaptersCountLabel) subtitleParts.push(options.chaptersCountLabel)
  }

  const playTo = series.continue_reading
    ? playRoute(
        { scope: 'local', domain: 'comic', id: series.continue_reading.chapter_id },
        {
          position: series.continue_reading.current_page,
          duration: series.continue_reading.total_pages,
        },
      )
    : undefined

  return {
    ref,
    title: series.title,
    subtitle: subtitleParts.join(' · ') || undefined,
    cover: series.cover_url ? withAuthUrl(series.cover_url) : undefined,
    badge: {
      sourceKind: 'local',
      progressPercent,
    },
    to: itemRoute(ref),
    playTo,
  }
}

export function onlineSearchItemToTrackedRef(item: ComicSearchResultItem): OnlineItemRef {
  const cover = resolveOnlineComicCoverUrl(item.proxy_cover_url, item.cover_url, item.source_id)
  return {
    scope: 'online',
    domain: 'comic',
    sourceId: item.source_id,
    ref: item.comic_url,
    hints: {
      title: item.title,
      cover: cover || undefined,
      author: item.author || undefined,
    },
  }
}

export function onlineSearchItemToMediaItemVM(
  item: ComicSearchResultItem,
  state?: OnlineItemSnapshot | null,
): MediaItemVM {
  const ref = onlineSearchItemToTrackedRef(item)
  const cover = resolveOnlineComicCoverUrl(item.proxy_cover_url, item.cover_url, item.source_id)
  const subtitleParts = [item.author?.trim(), item.last_chapter?.trim()].filter(Boolean)
  const progressPercent =
    state && state.progressRatio !== null && Number.isFinite(state.progressRatio)
      ? Math.min(100, Math.max(0, Math.round(state.progressRatio * 100)))
      : undefined

  return {
    ref,
    title: item.title,
    subtitle: subtitleParts.join(' · ') || undefined,
    cover: cover || undefined,
    badge: {
      source: item.source_name || state?.sourceName || undefined,
      sourceKind: 'online',
      progressPercent,
      availability: state?.availability,
      blocked: state?.blocked,
    },
    to: itemRoute(ref),
  }
}

export function saveOnlineComicContext(payload: OnlineComicContextStorage): void {
  try {
    sessionStorage.setItem(ONLINE_COMIC_CTX_KEY, JSON.stringify(payload))
  } catch {
    // ignore storage quota errors
  }
}

export function readOnlineComicContext(): OnlineComicContextStorage | null {
  try {
    const raw = sessionStorage.getItem(ONLINE_COMIC_CTX_KEY)
    if (!raw) return null
    const parsed: unknown = JSON.parse(raw)
    if (!parsed || typeof parsed !== 'object' || Array.isArray(parsed)) return null
    return parsed as OnlineComicContextStorage
  } catch {
    return null
  }
}

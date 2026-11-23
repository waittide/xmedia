import { api } from '@/domains/book/api'
import { buildOnlineCoverUrl, type OnlineShelfBookDto } from '@/domains/book/onlineApi'
import { itemRoute, playRoute, type MediaItemVM, type TrackedItemRef } from '@/sources'
import type { BookSummaryDto, ReadingLocation } from '@/domains/book/types'

export function hasBookProgress(progress: ReadingLocation | null | undefined): boolean {
  if (!progress) return false
  return progress.progress_percent > 0 || Boolean(progress.locator?.trim())
}

export function normalizeBookProgressPercent(
  progress: ReadingLocation | null | undefined,
): number | null {
  if (!hasBookProgress(progress) || !progress) return null
  const pct = progress.progress_percent
  if (Number.isFinite(pct) && pct > 0) {
    return Math.min(100, pct)
  }
  return 0.01
}

export function formatProgressPercentValue(pct: number): string {
  if (!Number.isFinite(pct) || pct <= 0) return '0'
  if (pct < 1) return '<1'
  return String(Math.min(100, Math.round(pct)))
}

export function bookSummaryToVM(
  book: BookSummaryDto,
  options?: {
    unknownAuthor?: string
    missingLabel?: string
    includeFormatInSubtitle?: boolean
  },
): MediaItemVM {
  const ref = { scope: 'local' as const, domain: 'book' as const, id: book.id }
  const normalizedPct = normalizeBookProgressPercent(book.progress)
  const progressPercent = normalizedPct !== null ? normalizedPct : undefined

  const authorsText = book.metadata.authors.join(', ')
  const subtitle =
    authorsText ||
    (options?.includeFormatInSubtitle ? book.format.toUpperCase() : options?.unknownAuthor) ||
    book.format.toUpperCase()

  const statusParts: string[] = []
  if (book.is_missing && options?.missingLabel) {
    statusParts.push(options.missingLabel)
  } else if (book.format) {
    statusParts.push(book.format.toUpperCase())
  }

  return {
    ref,
    title: book.metadata.title || book.id,
    subtitle,
    cover: book.has_cover ? api.coverUrl(book.id, book.updated_at) : undefined,
    badge: {
      sourceKind: book.online_tracking ? 'online' : 'local',
      progressPercent,
      status: statusParts.join(' · ') || undefined,
    },
    to: itemRoute(ref),
    playTo: book.is_missing
      ? undefined
      : playRoute(ref, {
          unitIndex: book.progress?.chapter_index ?? undefined,
          position: book.progress?.progress_percent ?? 0,
        }),
  }
}

export function onlineShelfBookToVM(
  book: OnlineShelfBookDto,
  unknownAuthor?: string,
): MediaItemVM {
  const cover = book.cover_url ? buildOnlineCoverUrl(book.cover_url) || undefined : undefined
  const ref: TrackedItemRef = {
    scope: 'online',
    domain: 'book',
    sourceId: book.source_id,
    ref: book.book_url,
    hints: {
      title: book.name,
      cover,
      author: book.author || undefined,
      extra: {
        toc: book.toc_url,
        chapter: String(book.current_chapter_index || 0),
      },
    },
  }
  const progressPercent =
    book.total_chapters > 0
      ? Math.min(
          100,
          Math.max(
            0,
            Math.round(((book.current_chapter_index + book.scroll_ratio) / book.total_chapters) * 100),
          ),
        )
      : undefined

  return {
    ref,
    title: book.name,
    subtitle: book.current_chapter_title || book.author || unknownAuthor || undefined,
    cover,
    badge: {
      source: book.source_name || undefined,
      sourceKind: 'online',
      progressPercent,
      availability: 'ok',
      blocked: false,
    },
    to: itemRoute(ref),
    playTo: playRoute(ref, {
      unitIndex: book.current_chapter_index,
      position: book.scroll_ratio,
    }),
  }
}

import { api } from '@/domains/book/api'
import { itemRoute, playRoute, type MediaItemVM } from '@/sources'
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

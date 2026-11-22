import { api } from '@/domains/book/api'
import { itemRoute, playRoute, type MediaItemVM } from '@/sources'
import type { BookSummaryDto } from '@/domains/book/types'

export function bookSummaryToVM(
  book: BookSummaryDto,
  options?: {
    unknownAuthor?: string
    missingLabel?: string
    includeFormatInSubtitle?: boolean
  },
): MediaItemVM {
  const ref = { scope: 'local' as const, domain: 'book' as const, id: book.id }
  const progressPercent = book.progress
    ? Math.min(100, Math.max(0, Math.round(book.progress.progress_percent)))
    : undefined

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

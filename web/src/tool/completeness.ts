import type { CompletenessFlags, MediaFileEntry } from './types'

export type ToolCompletenessFilter =
  | 'all'
  | 'complete'
  | 'incomplete'
  | 'missing_cover'
  | 'missing_summary'
  | 'missing_tag'

export function isCompletenessComplete(c: CompletenessFlags): boolean {
  return (
    c.has_title &&
    c.has_creator &&
    c.has_cover &&
    c.has_summary_or_lyrics &&
    c.has_nfo_or_tag_block
  )
}

export function isEntryComplete(entry: Pick<MediaFileEntry, 'completeness'>): boolean {
  return isCompletenessComplete(entry.completeness)
}

export function completenessScore(entry: Pick<MediaFileEntry, 'completeness'>): number {
  const c = entry.completeness
  return (
    (c.has_title ? 1 : 0) +
    (c.has_creator ? 1 : 0) +
    (c.has_cover ? 1 : 0) +
    (c.has_summary_or_lyrics ? 1 : 0) +
    (c.has_nfo_or_tag_block ? 1 : 0)
  )
}

export function entryCompositeKey(entry: Pick<MediaFileEntry, 'root_id' | 'rel_path'>): string {
  return `${entry.root_id}::${entry.rel_path}`
}

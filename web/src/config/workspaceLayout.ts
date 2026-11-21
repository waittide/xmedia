import type { DomainLayoutConfig, MediaKind, WorkspaceLayoutPreferences } from '@/types/api'

export const MAX_LAYOUT_ITEMS = 4

export const HOME_CARD_POOLS: Record<MediaKind, readonly string[]> = {
  book: ['all', 'reading', 'bookmarks', 'favorites', 'shelves', 'tags', 'authors', 'libraries'],
  comic: ['all', 'reading', 'bookmarks', 'favorites', 'chapters', 'shelves', 'tags', 'libraries'],
  music: [
    'all',
    'playlists',
    'listening',
    'favorites',
    'bookmarks',
    'albums',
    'artists',
    'genres',
    'tags',
    'libraries',
  ],
  video: [
    'all',
    'movies',
    'shows',
    'favorites',
    'watching',
    'shelves',
    'genres',
    'tags',
    'libraries',
  ],
}

export const SIDEBAR_GROUP_POOLS: Record<MediaKind, readonly string[]> = {
  book: ['authors', 'tags', 'shelves', 'libraries', 'bookmarks'],
  comic: ['writers', 'pencillers', 'genres', 'tags', 'shelves', 'libraries', 'bookmarks'],
  music: ['artists', 'tags', 'albums', 'genres', 'playlists', 'libraries'],
  video: ['shows', 'genres', 'tags', 'shelves', 'libraries'],
}

export const DEFAULT_WORKSPACE_LAYOUT: WorkspaceLayoutPreferences = {
  book: {
    home_cards: ['all', 'reading', 'bookmarks', 'favorites'],
    sidebar_groups: ['authors', 'tags', 'shelves', 'libraries'],
  },
  comic: {
    home_cards: ['all', 'reading', 'bookmarks', 'favorites'],
    sidebar_groups: ['writers', 'tags', 'shelves', 'libraries'],
  },
  music: {
    home_cards: ['all', 'playlists', 'listening', 'favorites'],
    sidebar_groups: ['artists', 'tags', 'albums', 'genres'],
  },
  video: {
    home_cards: ['all', 'movies', 'shows', 'favorites'],
    sidebar_groups: ['shows', 'genres', 'tags', 'shelves'],
  },
}

function normalizeList(
  items: readonly unknown[] | undefined,
  pool: readonly string[],
): string[] {
  if (!Array.isArray(items)) return []
  const allowed = new Set(pool)
  const seen = new Set<string>()
  const out: string[] = []
  for (const raw of items) {
    if (typeof raw !== 'string') continue
    const clean = raw.trim()
    if (!clean || !allowed.has(clean) || seen.has(clean)) continue
    seen.add(clean)
    out.push(clean)
    if (out.length >= MAX_LAYOUT_ITEMS) break
  }
  return out
}

export function sanitizeDomainLayout(
  kind: MediaKind,
  raw?: Partial<DomainLayoutConfig> | null,
): DomainLayoutConfig {
  const defaults = DEFAULT_WORKSPACE_LAYOUT[kind]
  const cards = normalizeList(raw?.home_cards, HOME_CARD_POOLS[kind])
  const hasExplicitGroups = Array.isArray(raw?.sidebar_groups)
  const groups = hasExplicitGroups
    ? normalizeList(raw?.sidebar_groups, SIDEBAR_GROUP_POOLS[kind])
    : [...defaults.sidebar_groups]

  return {
    home_cards: cards.length > 0 ? cards : [...defaults.home_cards],
    sidebar_groups: groups,
  }
}

export function sanitizeWorkspaceLayout(
  raw?: Partial<WorkspaceLayoutPreferences> | null,
): WorkspaceLayoutPreferences {
  return {
    book: sanitizeDomainLayout('book', raw?.book),
    comic: sanitizeDomainLayout('comic', raw?.comic),
    music: sanitizeDomainLayout('music', raw?.music),
    video: sanitizeDomainLayout('video', raw?.video),
  }
}

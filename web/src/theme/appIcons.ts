export type AppIconId = 'cobalt' | 'porcelain' | 'obsidian' | 'monolith'

export interface AppIconPreset {
  id: AppIconId
  src: string
}

/** Curated minimalist app icon variants (first one is the default). */
export const APP_ICON_PRESETS: readonly AppIconPreset[] = [
  { id: 'cobalt', src: '/icons/cobalt.svg' },
  { id: 'porcelain', src: '/icons/porcelain.svg' },
  { id: 'obsidian', src: '/icons/obsidian.svg' },
  { id: 'monolith', src: '/icons/monolith.svg' },
] as const

export const DEFAULT_APP_ICON: AppIconId = 'cobalt'

const VALID_ICON_IDS = new Set<string>(APP_ICON_PRESETS.map((item) => item.id))

export function normalizeAppIcon(raw: string | null | undefined): AppIconId {
  const key = typeof raw === 'string' ? raw.trim().toLowerCase() : ''
  if (VALID_ICON_IDS.has(key)) {
    return key as AppIconId
  }
  return DEFAULT_APP_ICON
}

/** Resolves a stored/remote preference value (`null` represents default `cobalt`). */
export function resolveStoredAppIcon(raw: string | null | undefined): AppIconId | null {
  const normalized = normalizeAppIcon(raw)
  return normalized === DEFAULT_APP_ICON ? null : normalized
}

export function appIconUrl(id: AppIconId): string {
  return `/icons/${id}.svg`
}

export function applyDocumentFavicon(id: AppIconId): void {
  if (typeof document === 'undefined') return
  const href = appIconUrl(id)
  let link = document.getElementById('app-favicon') as HTMLLinkElement | null
  if (!link) {
    link = document.querySelector<HTMLLinkElement>('link[rel="icon"]')
  }
  if (!link) {
    link = document.createElement('link')
    link.id = 'app-favicon'
    link.rel = 'icon'
    link.type = 'image/svg+xml'
    document.head.appendChild(link)
  }
  if (link.getAttribute('href') !== href) {
    link.setAttribute('href', href)
  }
}
